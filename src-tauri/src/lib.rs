use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    env,
    ffi::OsString,
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};

const FALLBACK_TOOL_DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"];
const FFMPEG_COMMAND_HISTORY_LIMIT: usize = 500;
const DEFAULT_YTDLP_FORMAT_SELECTOR: &str = "bv*+ba/b";
const YTDLP_OPERATION_CANCELED_MESSAGE: &str = "操作已停止。";
const DOWNLOAD_CACHE_DIR_NAME: &str = "download-cache";
const STALE_ACTIVE_HISTORY_SECONDS: u64 = 24 * 60 * 60;
static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct AppState {
    tasks: Mutex<HashMap<String, TaskControl>>,
    paused_tasks: Mutex<HashSet<String>>,
    ytdlp_operations: Mutex<HashMap<String, YtdlpOperationControl>>,
    history_lock: Mutex<()>,
    ffmpeg_command_history_lock: Mutex<()>,
    tool_settings_lock: Mutex<()>,
}

#[derive(Clone)]
struct TaskControl {
    child: Arc<Mutex<Child>>,
    canceled: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
}

#[derive(Clone)]
struct YtdlpOperationControl {
    child: Arc<Mutex<Child>>,
    canceled: Arc<AtomicBool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DependencyStatus {
    yt_dlp: ToolStatus,
    ffmpeg: ToolStatus,
    proxy: ProxyStatus,
    ready: bool,
    install_hint: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolUpdates {
    yt_dlp: ToolUpdateStatus,
    ffmpeg: ToolUpdateStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolUpdateStatus {
    name: String,
    current_version: Option<String>,
    latest_version: Option<String>,
    update_available: Option<bool>,
    checked_at: String,
    update_command: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolStatus {
    name: String,
    installed: bool,
    path: Option<String>,
    version: Option<String>,
    source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ToolSettings {
    yt_dlp_path: Option<String>,
    ffmpeg_path: Option<String>,
    proxy_mode: Option<String>,
    proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProxyStatus {
    mode: String,
    effective_proxy: Option<String>,
    source: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProxyMode {
    Auto,
    Manual,
    Off,
}

#[derive(Debug, Clone)]
struct ToolResolution {
    path: PathBuf,
    source: ToolSource,
}

#[derive(Debug, Clone, Copy)]
enum ToolSource {
    Manual,
    Env,
    Path,
}

impl ToolSource {
    fn as_str(self) -> &'static str {
        match self {
            ToolSource::Manual => "manual",
            ToolSource::Env => "env",
            ToolSource::Path => "path",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ManagedTool {
    YtDlp,
    Ffmpeg,
}

impl ManagedTool {
    fn from_name(name: &str) -> Result<Self, String> {
        match name {
            "yt-dlp" | "ytDlp" | "yt_dlp" => Ok(Self::YtDlp),
            "ffmpeg" => Ok(Self::Ffmpeg),
            _ => Err(format!("不支持的工具名称：{name}")),
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::YtDlp => "yt-dlp",
            Self::Ffmpeg => "ffmpeg",
        }
    }

    fn env_var(self) -> &'static str {
        match self {
            Self::YtDlp => "VIDEO_DOWNLOADER_YT_DLP_PATH",
            Self::Ffmpeg => "VIDEO_DOWNLOADER_FFMPEG_PATH",
        }
    }

    fn path<'a>(self, settings: &'a ToolSettings) -> Option<&'a str> {
        match self {
            Self::YtDlp => settings.yt_dlp_path.as_deref(),
            Self::Ffmpeg => settings.ffmpeg_path.as_deref(),
        }
    }

    fn set_path(self, settings: &mut ToolSettings, path: Option<String>) {
        match self {
            Self::YtDlp => settings.yt_dlp_path = path,
            Self::Ffmpeg => settings.ffmpeg_path = path,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FormatOption {
    id: String,
    label: String,
    selector: String,
    ext: Option<String>,
    resolution: Option<String>,
    vcodec: Option<String>,
    acodec: Option<String>,
    filesize: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProbeResponse {
    title: String,
    site: String,
    webpage_url: String,
    duration: Option<f64>,
    thumbnail: Option<String>,
    formats: Vec<FormatOption>,
    checked_browser: Option<String>,
    checked_at: String,
    format_count: usize,
    best_format_label: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadRequest {
    task_id: String,
    url: String,
    title: Option<String>,
    site: Option<String>,
    format: String,
    browser: Option<String>,
    output_dir: String,
    expected_media: Option<ExpectedMediaInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ExpectedMediaInfo {
    duration: Option<f64>,
    resolution_label: Option<String>,
    resolution_score: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchParseItem {
    id: String,
    url: String,
    title: Option<String>,
    site: Option<String>,
    duration: Option<f64>,
    thumbnail: Option<String>,
    source_url: Option<String>,
    playlist_title: Option<String>,
    playlist_index: Option<usize>,
    playlist_total: Option<usize>,
    source_order: Option<usize>,
    is_playlist_item: bool,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FfmpegCommandRequest {
    preset_id: String,
    input_path: String,
    secondary_input_path: Option<String>,
    output_dir: String,
    audio_format: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    crf: Option<u8>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FfmpegCommandDraft {
    command: String,
    working_dir: String,
    output_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FfmpegCommandHistoryItem {
    id: String,
    preset_id: String,
    input_path: String,
    secondary_input_path: Option<String>,
    output_dir: String,
    audio_format: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    crf: Option<u8>,
    command: String,
    working_dir: String,
    output_path: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FfmpegCommandHistoryInput {
    preset_id: String,
    input_path: String,
    secondary_input_path: Option<String>,
    output_dir: String,
    audio_format: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    crf: Option<u8>,
    command: String,
    working_dir: String,
    output_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalPrefillResult {
    prefilled: bool,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SupportedSiteExample {
    name: String,
    url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SupportedSitesResponse {
    version: Option<String>,
    total: usize,
    examples: Vec<SupportedSiteExample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryItem {
    id: String,
    url: String,
    title: String,
    site: String,
    format: String,
    browser: Option<String>,
    output_dir: String,
    status: String,
    progress: f64,
    output_path: Option<String>,
    local_media: Option<LocalMediaInfo>,
    media_comparison: Option<MediaComparison>,
    error: Option<String>,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    task_id: String,
    status: String,
    progress: f64,
    phase: Option<String>,
    phase_label: Option<String>,
    speed: Option<String>,
    eta: Option<String>,
    line: Option<String>,
    output_path: Option<String>,
    local_media: Option<LocalMediaInfo>,
    media_comparison: Option<MediaComparison>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct DownloadCleanupSummary {
    file_count: u64,
    directory_count: u64,
    bytes: u64,
    invalid_history_count: u64,
    skipped_active_tasks: u64,
}

#[derive(Debug, Default)]
struct DownloadCleanupPlan {
    summary: DownloadCleanupSummary,
    cache_dirs: Vec<PathBuf>,
    invalid_history_ids: HashSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LocalMediaInfo {
    duration: Option<f64>,
    width: Option<u64>,
    height: Option<u64>,
    video_codec: Option<String>,
    audio_codec: Option<String>,
    probed_at: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct MediaComparison {
    duration: Option<MediaComparisonDetail>,
    resolution: Option<MediaComparisonDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaComparisonDetail {
    status: String,
    expected_label: Option<String>,
    actual_label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownloadPhase {
    DownloadingVideo,
    DownloadingAudio,
    DownloadingMedia,
    Merging,
    Completed,
}

impl DownloadPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::DownloadingVideo => "downloadingVideo",
            Self::DownloadingAudio => "downloadingAudio",
            Self::DownloadingMedia => "downloadingMedia",
            Self::Merging => "merging",
            Self::Completed => "completed",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::DownloadingVideo => "下载视频流",
            Self::DownloadingAudio => "下载音频流",
            Self::DownloadingMedia => "下载媒体",
            Self::Merging => "合并封装中",
            Self::Completed => "已完成",
        }
    }
}

#[derive(Debug, Clone)]
struct ParsedProgress {
    progress: f64,
    speed: Option<String>,
    eta: Option<String>,
    media_kind: ProgressMediaKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProgressMediaKind {
    Video,
    Audio,
    Media,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
struct ProgressSnapshot {
    progress: f64,
    phase: DownloadPhase,
}

impl Default for ProgressSnapshot {
    fn default() -> Self {
        Self {
            progress: 0.0,
            phase: DownloadPhase::DownloadingMedia,
        }
    }
}

#[tauri::command]
async fn check_dependencies(app: AppHandle) -> Result<DependencyStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let settings = read_tool_settings_with_fallback(&state);
        let yt_dlp = tool_status(&state, "yt-dlp");
        let ffmpeg = tool_status(&state, "ffmpeg");
        let ready = yt_dlp.installed && ffmpeg.installed;

        DependencyStatus {
            yt_dlp,
            ffmpeg,
            proxy: proxy_status(&settings),
            ready,
            install_hint: "brew install yt-dlp ffmpeg".to_string(),
        }
    })
    .await
    .map_err(|error| format!("检查依赖失败：{error}"))
}

#[tauri::command]
async fn check_tool_updates(app: AppHandle) -> Result<ToolUpdates, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let settings = read_tool_settings_with_fallback(&state);

        ToolUpdates {
            yt_dlp: tool_update_status(&settings, ManagedTool::YtDlp),
            ffmpeg: tool_update_status(&settings, ManagedTool::Ffmpeg),
        }
    })
    .await
    .map_err(|error| format!("检查工具更新失败：{error}"))
}

#[tauri::command]
async fn list_supported_sites(app: AppHandle) -> Result<SupportedSitesResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let settings = read_tool_settings_with_fallback(&state);
        let yt_dlp_path = ensure_tool_with_settings(&settings, "yt-dlp")?;
        let version = tool_version(ManagedTool::YtDlp, &yt_dlp_path).map(first_line);
        let mut command = Command::new(&yt_dlp_path);
        apply_tool_env_from_settings(&settings, &mut command);
        command.arg("--list-extractors");

        let output = run_command_with_timeout(command, Duration::from_secs(12))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                "yt-dlp 未能返回支持站点列表。".to_string()
            } else {
                stderr
            });
        }

        let extractors = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let examples = supported_site_examples(&extractors);

        Ok(SupportedSitesResponse {
            version,
            total: extractors.len(),
            examples,
        })
    })
    .await
    .map_err(|error| format!("读取支持站点失败：{error}"))?
}

#[tauri::command]
async fn load_tool_settings(app: AppHandle) -> Result<ToolSettings, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        read_tool_settings(&state)
    })
    .await
    .map_err(|error| format!("读取工具路径设置失败：{error}"))?
}

#[tauri::command]
async fn save_tool_path(
    app: AppHandle,
    tool: String,
    path: String,
) -> Result<ToolSettings, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let tool = ManagedTool::from_name(&tool)?;
        let path = PathBuf::from(path.trim());
        validate_tool_path(tool, &path)?;

        let _guard = state
            .tool_settings_lock
            .lock()
            .map_err(|_| "工具路径设置锁已损坏。".to_string())?;
        let mut settings = read_tool_settings_unlocked()?;
        tool.set_path(&mut settings, Some(path.display().to_string()));
        write_tool_settings_unlocked(&settings)?;
        Ok(settings)
    })
    .await
    .map_err(|error| format!("保存工具路径失败：{error}"))?
}

#[tauri::command]
async fn clear_tool_path(app: AppHandle, tool: String) -> Result<ToolSettings, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let tool = ManagedTool::from_name(&tool)?;

        let _guard = state
            .tool_settings_lock
            .lock()
            .map_err(|_| "工具路径设置锁已损坏。".to_string())?;
        let mut settings = read_tool_settings_unlocked()?;
        tool.set_path(&mut settings, None);
        write_tool_settings_unlocked(&settings)?;
        Ok(settings)
    })
    .await
    .map_err(|error| format!("清除工具路径失败：{error}"))?
}

#[tauri::command]
async fn save_proxy_settings(
    app: AppHandle,
    mode: String,
    proxy_url: Option<String>,
) -> Result<ToolSettings, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mode = normalize_proxy_mode(Some(mode.as_str()));
        let proxy_url = normalize_proxy_url(proxy_url.as_deref(), mode)?;

        let _guard = state
            .tool_settings_lock
            .lock()
            .map_err(|_| "工具路径设置锁已损坏。".to_string())?;
        let mut settings = read_tool_settings_unlocked()?;
        settings.proxy_mode = Some(mode.as_str().to_string());
        settings.proxy_url = proxy_url;
        write_tool_settings_unlocked(&settings)?;
        Ok(settings)
    })
    .await
    .map_err(|error| format!("保存代理设置失败：{error}"))?
}

#[tauri::command]
fn default_download_dir() -> Option<String> {
    dirs::download_dir().map(|path| path.to_string_lossy().to_string())
}

#[tauri::command]
async fn probe_url(
    app: AppHandle,
    url: String,
    browser: Option<String>,
    operation_id: Option<String>,
) -> Result<ProbeResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        validate_url(&url)?;
        let settings = read_tool_settings_with_fallback(&state);
        let yt_dlp_path = ensure_tool_with_settings(&settings, "yt-dlp")?;
        let checked_browser = normalized_browser(browser);

        let mut command = Command::new(&yt_dlp_path);
        apply_tool_env_from_settings(&settings, &mut command);
        apply_ytdlp_proxy_from_settings(&settings, &mut command)?;
        command
            .arg("--dump-single-json")
            .arg("--skip-download")
            .arg("--no-warnings")
            .arg("--no-playlist")
            .arg("--socket-timeout")
            .arg("30")
            .arg("-f")
            .arg(DEFAULT_YTDLP_FORMAT_SELECTOR);

        if let Some(browser) = checked_browser.as_deref() {
            command.arg("--cookies-from-browser").arg(browser);
        }

        command.arg(&url);
        let output = run_ytdlp_command_with_timeout(
            &state,
            command,
            Duration::from_secs(45),
            operation_id.as_deref(),
        )?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                "yt-dlp 探测失败，未返回可用信息。".to_string()
            } else {
                stderr
            });
        }

        let json: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("解析 yt-dlp JSON 失败：{error}"))?;
        let formats = build_format_options(&json);
        let format_count = json
            .get("formats")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        let best_format_label = best_format_label_from_json(&json, &formats);

        Ok(ProbeResponse {
            title: string_field(&json, "title").unwrap_or_else(|| "Untitled video".to_string()),
            site: string_field(&json, "extractor_key")
                .or_else(|| string_field(&json, "extractor"))
                .unwrap_or_else(|| "Generic".to_string()),
            webpage_url: string_field(&json, "webpage_url").unwrap_or(url),
            duration: json.get("duration").and_then(Value::as_f64),
            thumbnail: thumbnail_url(&json),
            formats,
            checked_browser,
            checked_at: unix_timestamp(),
            format_count,
            best_format_label,
        })
    })
    .await
    .map_err(|error| format!("探测视频地址失败：{error}"))?
}

#[tauri::command]
async fn parse_download_queue(
    app: AppHandle,
    urls: Vec<String>,
    browser: Option<String>,
    operation_id: Option<String>,
) -> Result<Vec<BatchParseItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let settings = read_tool_settings_with_fallback(&state);
        let yt_dlp_path = ensure_tool_with_settings(&settings, "yt-dlp")?;
        let browser = normalized_browser(browser);
        let mut items = Vec::new();

        let mut source_order = 0;

        for raw_url in urls {
            let url = raw_url.trim().to_string();
            if url.is_empty() {
                continue;
            }
            source_order += 1;

            match parse_queue_url(
                &state,
                &settings,
                &yt_dlp_path,
                &url,
                browser.as_deref(),
                source_order,
                operation_id.as_deref(),
            ) {
                Ok(mut parsed) => items.append(&mut parsed),
                Err(error) => {
                    if error == YTDLP_OPERATION_CANCELED_MESSAGE {
                        return Err(error);
                    }

                    items.push(BatchParseItem {
                        id: uuid_like_id(),
                        url: url.clone(),
                        title: None,
                        site: None,
                        duration: None,
                        thumbnail: None,
                        source_url: None,
                        playlist_title: None,
                        playlist_index: None,
                        playlist_total: None,
                        source_order: Some(source_order),
                        is_playlist_item: false,
                        error: Some(error),
                    });
                }
            }
        }

        Ok(items)
    })
    .await
    .map_err(|error| format!("解析下载队列失败：{error}"))?
}

#[tauri::command]
async fn start_download(app: AppHandle, request: DownloadRequest) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        validate_url(&request.url)?;
        let settings = read_tool_settings_with_fallback(&state);
        let yt_dlp_path = ensure_tool_with_settings(&settings, "yt-dlp")?;
        let ffmpeg_path = ensure_tool_with_settings(&settings, "ffmpeg")?;

        let output_dir = PathBuf::from(&request.output_dir);
        fs::create_dir_all(&output_dir)
            .map_err(|error| format!("无法创建保存目录 {}：{error}", output_dir.display()))?;
        let task_cache_dir = download_task_cache_dir(&request.task_id)?;
        fs::create_dir_all(&task_cache_dir)
            .map_err(|error| format!("无法创建下载缓存目录 {}：{error}", task_cache_dir.display()))?;
        let ytdlp_cache_dir = task_cache_dir.join("yt-dlp-cache");
        fs::create_dir_all(&ytdlp_cache_dir)
            .map_err(|error| format!("无法创建 yt-dlp 缓存目录 {}：{error}", ytdlp_cache_dir.display()))?;

        let mut command = Command::new(&yt_dlp_path);
        apply_tool_env_from_settings(&settings, &mut command);
        apply_ytdlp_proxy_from_settings(&settings, &mut command)?;
        command
            .arg("--progress")
            .arg("--newline")
            .arg("--color")
            .arg("stderr:never")
            .arg("--progress-delta")
            .arg("0.5")
            .arg("--progress-template")
            .arg("download:VD_PROGRESS:%(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s|%(info.vcodec|)s|%(info.acodec|)s|%(info.format_id|)s")
            .arg("--progress-template")
            .arg("postprocess:VD_POSTPROCESS:%(progress.status|)s")
            .arg("--no-playlist")
            .arg("--socket-timeout")
            .arg("30")
            .arg("-f")
            .arg(&request.format)
            .arg("--ffmpeg-location")
            .arg(&ffmpeg_path)
            .arg("--merge-output-format")
            .arg("mp4")
            .arg("--paths")
            .arg(format!("home:{}", output_dir.display()))
            .arg("--paths")
            .arg(format!("temp:{}", task_cache_dir.display()))
            .arg("--cache-dir")
            .arg(&ytdlp_cache_dir)
            .arg("--continue")
            .arg("-o")
            .arg("%(title).120B-%(id)s.%(ext)s")
            .arg("--print")
            .arg("after_move:VD_OUTPUT=%(filepath)s")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(browser) = normalized_browser(request.browser.clone()) {
            command.arg("--cookies-from-browser").arg(browser);
        }

        command.arg(&request.url);

        let mut child = command
            .spawn()
            .map_err(|error| format!("无法启动下载任务：{error}"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "无法读取下载任务输出。".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "无法读取下载任务错误输出。".to_string())?;

        let child = Arc::new(Mutex::new(child));
        let canceled = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let task_id = request.task_id.clone();

        {
            if let Ok(mut paused_tasks) = state.paused_tasks.lock() {
                paused_tasks.remove(&task_id);
            }
            let mut tasks = state
                .tasks
                .lock()
                .map_err(|_| "下载任务状态锁已损坏。".to_string())?;
            tasks.insert(
                task_id.clone(),
                TaskControl {
                    child: Arc::clone(&child),
                    canceled: Arc::clone(&canceled),
                    paused: Arc::clone(&paused),
                },
            );
        }

        upsert_started_history(
            &state,
            HistoryItem {
                id: task_id.clone(),
                url: request.url.clone(),
                title: display_title(request.title.as_deref(), &request.url, None),
                site: display_site(request.site.as_deref(), &request.url),
                format: request.format.clone(),
                browser: request.browser.clone(),
                output_dir: request.output_dir.clone(),
                status: "running".to_string(),
                progress: 0.0,
                output_path: None,
                local_media: None,
                media_comparison: None,
                error: None,
                updated_at: unix_timestamp(),
            },
        )?;

        let app_for_stdout = app.clone();
        let task_for_stdout = task_id.clone();
        let app_for_stderr = app.clone();
        let task_for_stderr = task_id.clone();
        let stderr_buffer = Arc::new(Mutex::new(String::new()));
        let stderr_for_thread = Arc::clone(&stderr_buffer);
        let shared_progress = Arc::new(Mutex::new(ProgressSnapshot::default()));
        let progress_for_stdout = Arc::clone(&shared_progress);
        let progress_for_stderr = Arc::clone(&shared_progress);
        let expected_media = request.expected_media.clone();
        let ffmpeg_path_for_probe = ffmpeg_path.clone();
        let cache_dir_for_finish = task_cache_dir.clone();

        thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut buffer = String::new();
            let mut line = String::new();

            loop {
                line.clear();
                let read = reader.read_line(&mut line).unwrap_or(0);
                if read == 0 {
                    break;
                }

                buffer.push_str(&line);

                if let Some(parsed) = parse_progress_line(&line) {
                    let snapshot = update_progress_snapshot(&progress_for_stderr, &parsed);
                    let _ = app_for_stderr.emit(
                        "download-progress",
                        ProgressEvent {
                            task_id: task_for_stderr.clone(),
                            status: "running".to_string(),
                            progress: snapshot.progress,
                            phase: Some(snapshot.phase.as_str().to_string()),
                            phase_label: Some(snapshot.phase.label().to_string()),
                            speed: parsed.speed,
                            eta: parsed.eta,
                            line: Some(line.trim().to_string()),
                            output_path: None,
                            local_media: None,
                            media_comparison: None,
                            error: None,
                        },
                    );
                } else if is_merge_progress_line(&line) {
                    let snapshot = update_merge_snapshot(&progress_for_stderr);
                    let _ = app_for_stderr.emit(
                        "download-progress",
                        ProgressEvent {
                            task_id: task_for_stderr.clone(),
                            status: "running".to_string(),
                            progress: snapshot.progress,
                            phase: Some(snapshot.phase.as_str().to_string()),
                            phase_label: Some(snapshot.phase.label().to_string()),
                            speed: None,
                            eta: None,
                            line: Some(line.trim().to_string()),
                            output_path: None,
                            local_media: None,
                            media_comparison: None,
                            error: None,
                        },
                    );
                }
            }

            if let Ok(mut stderr) = stderr_for_thread.lock() {
                *stderr = buffer;
            }
        });

        thread::spawn(move || {
            let mut progress = 0.0;
            let mut output_path: Option<String> = None;
            let reader = BufReader::new(stdout);

            for line in reader.lines().map_while(Result::ok) {
                if let Some(path) = line.strip_prefix("VD_OUTPUT=") {
                    output_path = Some(path.trim().to_string());
                    continue;
                }

                if let Some(parsed) = parse_progress_line(&line) {
                    let snapshot = update_progress_snapshot(&progress_for_stdout, &parsed);
                    progress = snapshot.progress;
                    let _ = app_for_stdout.emit(
                        "download-progress",
                        ProgressEvent {
                            task_id: task_for_stdout.clone(),
                            status: "running".to_string(),
                            progress,
                            phase: Some(snapshot.phase.as_str().to_string()),
                            phase_label: Some(snapshot.phase.label().to_string()),
                            speed: parsed.speed,
                            eta: parsed.eta,
                            line: Some(line),
                            output_path: output_path.clone(),
                            local_media: None,
                            media_comparison: None,
                            error: None,
                        },
                    );
                } else if is_merge_progress_line(&line) {
                    let snapshot = update_merge_snapshot(&progress_for_stdout);
                    progress = snapshot.progress;
                    let _ = app_for_stdout.emit(
                        "download-progress",
                        ProgressEvent {
                            task_id: task_for_stdout.clone(),
                            status: "running".to_string(),
                            progress,
                            phase: Some(snapshot.phase.as_str().to_string()),
                            phase_label: Some(snapshot.phase.label().to_string()),
                            speed: None,
                            eta: None,
                            line: Some(line),
                            output_path: output_path.clone(),
                            local_media: None,
                            media_comparison: None,
                            error: None,
                        },
                    );
                }
            }

            let exit_status = child.lock().ok().and_then(|mut child| child.wait().ok());
            let was_canceled = canceled.load(Ordering::SeqCst);
            let was_paused = paused.load(Ordering::SeqCst);
            let success = exit_status.map(|status| status.success()).unwrap_or(false);
            let final_status = if was_canceled {
                progress = 0.0;
                "canceled"
            } else if was_paused {
                progress = shared_progress
                    .lock()
                    .map(|snapshot| snapshot.progress)
                    .unwrap_or(progress);
                "paused"
            } else if success {
                progress = 100.0;
                "completed"
            } else {
                progress = shared_progress
                    .lock()
                    .map(|snapshot| snapshot.progress)
                    .unwrap_or(progress);
                "failed"
            };

            let error = if final_status == "failed" {
                stderr_buffer.lock().ok().and_then(|stderr| {
                    let message = stderr.trim();
                    if message.is_empty() {
                        Some("下载失败，yt-dlp 未返回详细错误。".to_string())
                    } else {
                        Some(message.to_string())
                    }
                })
            } else {
                None
            };
            if final_status == "completed" || final_status == "canceled" || final_status == "failed" {
                let _ = remove_owned_download_cache_dir(&cache_dir_for_finish);
            }
            let final_phase = if final_status == "completed" {
                Some(DownloadPhase::Completed)
            } else {
                shared_progress.lock().ok().map(|snapshot| snapshot.phase)
            };
            let app_state = app_for_stdout.state::<AppState>();
            let (local_media, media_comparison) = if final_status == "completed" {
                let media = output_path
                    .as_deref()
                    .map(PathBuf::from)
                    .map(|path| probe_local_media(&app_state, &ffmpeg_path_for_probe, &path))
                    .unwrap_or_else(|| LocalMediaInfo {
                        probed_at: Some(unix_timestamp()),
                        error: Some("yt-dlp 未返回最终文件路径，无法读取本地媒体信息。".to_string()),
                        ..Default::default()
                    });
                let comparison = compare_media(expected_media.as_ref(), &media);

                (Some(media), comparison)
            } else {
                (None, None)
            };

            let _ = app_for_stdout.emit(
                "download-progress",
                ProgressEvent {
                    task_id: task_for_stdout.clone(),
                    status: final_status.to_string(),
                    progress,
                    phase: final_phase.map(|phase| phase.as_str().to_string()),
                    phase_label: final_phase.map(|phase| phase.label().to_string()),
                    speed: None,
                    eta: None,
                    line: None,
                    output_path: output_path.clone(),
                    local_media: local_media.clone(),
                    media_comparison: media_comparison.clone(),
                    error: error.clone(),
                },
            );

            let _ = update_history_status(
                &app_state,
                &task_for_stdout,
                final_status,
                progress,
                output_path,
                local_media,
                media_comparison,
                error,
            );

            {
                if let Ok(mut tasks) = app_state.tasks.lock() {
                    let should_remove = tasks
                        .get(&task_for_stdout)
                        .map(|control| Arc::ptr_eq(&control.child, &child))
                        .unwrap_or(false);
                    if should_remove {
                        tasks.remove(&task_for_stdout);
                    }
                };
                if final_status != "paused" {
                    if let Ok(mut paused_tasks) = app_state.paused_tasks.lock() {
                        paused_tasks.remove(&task_for_stdout);
                    }
                }
            }
        });

        Ok(task_id)
    })
    .await
    .map_err(|error| format!("启动下载任务失败：{error}"))?
}

#[tauri::command]
async fn cancel_download(app: AppHandle, task_id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let control = {
            let mut tasks = state
                .tasks
                .lock()
                .map_err(|_| "下载任务状态锁已损坏。".to_string())?;
            tasks.remove(&task_id)
        };
        if let Ok(mut paused_tasks) = state.paused_tasks.lock() {
            paused_tasks.remove(&task_id);
        }

        if let Some(control) = control {
            control.canceled.store(true, Ordering::SeqCst);

            if let Ok(mut child) = control.child.lock() {
                let _ = child.kill();
            }
        }

        let _ = remove_owned_download_cache_dir(&download_task_cache_dir(&task_id)?);
        update_history_status(&state, &task_id, "canceled", 0.0, None, None, None, None)?;

        let _ = app.emit(
            "download-progress",
            ProgressEvent {
                task_id,
                status: "canceled".to_string(),
                progress: 0.0,
                phase: None,
                phase_label: None,
                speed: None,
                eta: None,
                line: None,
                output_path: None,
                local_media: None,
                media_comparison: None,
                error: None,
            },
        );

        Ok(())
    })
    .await
    .map_err(|error| format!("取消下载任务失败：{error}"))?
}

#[tauri::command]
async fn pause_download(app: AppHandle, task_id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let progress = history_progress_for_task(&state, &task_id).unwrap_or(0.0);
        let control = {
            let mut tasks = state
                .tasks
                .lock()
                .map_err(|_| "下载任务状态锁已损坏。".to_string())?;
            tasks.remove(&task_id)
        };

        let Some(control) = control else {
            if let Ok(mut paused_tasks) = state.paused_tasks.lock() {
                paused_tasks.insert(task_id.clone());
            }
            update_history_status(&state, &task_id, "paused", progress, None, None, None, None)?;
            let _ = app.emit(
                "download-progress",
                ProgressEvent {
                    task_id,
                    status: "paused".to_string(),
                    progress,
                    phase: None,
                    phase_label: None,
                    speed: None,
                    eta: None,
                    line: None,
                    output_path: None,
                    local_media: None,
                    media_comparison: None,
                    error: None,
                },
            );
            return Ok(());
        };

        control.paused.store(true, Ordering::SeqCst);
        if let Ok(mut paused_tasks) = state.paused_tasks.lock() {
            paused_tasks.insert(task_id.clone());
        }

        if let Ok(mut child) = control.child.lock() {
            let _ = child.kill();
        }

        update_history_status(&state, &task_id, "paused", progress, None, None, None, None)?;

        let _ = app.emit(
            "download-progress",
            ProgressEvent {
                task_id,
                status: "paused".to_string(),
                progress,
                phase: None,
                phase_label: None,
                speed: None,
                eta: None,
                line: None,
                output_path: None,
                local_media: None,
                media_comparison: None,
                error: None,
            },
        );

        Ok(())
    })
    .await
    .map_err(|error| format!("暂停下载任务失败：{error}"))?
}

#[tauri::command]
async fn cancel_ytdlp_operation(app: AppHandle, operation_id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let control = {
            let mut operations = state
                .ytdlp_operations
                .lock()
                .map_err(|_| "yt-dlp 操作状态锁已损坏。".to_string())?;
            operations.remove(&operation_id)
        };

        let Some(control) = control else {
            return Ok(());
        };

        control.canceled.store(true, Ordering::SeqCst);
        if let Ok(mut child) = control.child.lock() {
            let _ = child.kill();
        }

        Ok(())
    })
    .await
    .map_err(|error| format!("停止 yt-dlp 操作失败：{error}"))?
}

#[tauri::command]
async fn scan_download_cleanup(app: AppHandle) -> Result<DownloadCleanupSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        build_download_cleanup_plan(&state).map(|plan| plan.summary)
    })
    .await
    .map_err(|error| format!("扫描下载缓存失败：{error}"))?
}

#[tauri::command]
async fn cleanup_download_cache(app: AppHandle) -> Result<DownloadCleanupSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let plan = build_download_cleanup_plan(&state)?;

        for cache_dir in &plan.cache_dirs {
            let _ = remove_owned_download_cache_dir(cache_dir);
        }

        if !plan.invalid_history_ids.is_empty() {
            remove_history_items_by_ids(&state, &plan.invalid_history_ids)?;
        }

        Ok(plan.summary)
    })
    .await
    .map_err(|error| format!("清理下载缓存失败：{error}"))?
}

#[tauri::command]
async fn reveal_file(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = PathBuf::from(path);
        if !path.exists() {
            return Err("文件不存在，无法打开文件位置。".to_string());
        }

        #[cfg(target_os = "macos")]
        let status = Command::new("open")
            .arg("-R")
            .arg(&path)
            .status()
            .map_err(|error| format!("打开 Finder 失败：{error}"))?;

        #[cfg(target_os = "windows")]
        let status = Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .status()
            .map_err(|error| format!("打开资源管理器失败：{error}"))?;

        #[cfg(all(unix, not(target_os = "macos")))]
        let status = Command::new("xdg-open")
            .arg(path.parent().unwrap_or_else(|| Path::new(".")))
            .status()
            .map_err(|error| format!("打开文件夹失败：{error}"))?;

        if status.success() {
            Ok(())
        } else {
            Err("系统文件管理器返回了失败状态。".to_string())
        }
    })
    .await
    .map_err(|error| format!("打开文件位置失败：{error}"))?
}

#[tauri::command]
async fn load_history(app: AppHandle) -> Result<Vec<HistoryItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        read_history(&state)
    })
    .await
    .map_err(|error| format!("读取历史记录失败：{error}"))?
}

#[tauri::command]
async fn delete_history_item(app: AppHandle, id: String, delete_file: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        delete_history_item_by_id(&state, &id, delete_file)
    })
    .await
    .map_err(|error| format!("删除历史记录失败：{error}"))?
}

#[tauri::command]
async fn build_ffmpeg_command(
    app: AppHandle,
    request: FfmpegCommandRequest,
) -> Result<FfmpegCommandDraft, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let ffmpeg_path = ensure_tool(&state, "ffmpeg")?;
        let input_path = validate_media_file("输入文件", &request.input_path)?;
        let working_dir = resolve_ffmpeg_working_dir(&request.output_dir, &input_path)?;
        let output_path = unique_ffmpeg_output_path(&working_dir, &input_path, &request)?;
        let args = ffmpeg_command_args(&ffmpeg_path, &input_path, &output_path, &request)?;
        let command = args
            .iter()
            .map(|arg| shell_quote(arg))
            .collect::<Vec<_>>()
            .join(" ");

        Ok(FfmpegCommandDraft {
            command,
            working_dir: working_dir.display().to_string(),
            output_path: output_path.display().to_string(),
        })
    })
    .await
    .map_err(|error| format!("生成 FFmpeg 命令失败：{error}"))?
}

#[tauri::command]
async fn load_ffmpeg_command_history(
    app: AppHandle,
) -> Result<Vec<FfmpegCommandHistoryItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        read_ffmpeg_command_history(&state)
    })
    .await
    .map_err(|error| format!("读取 FFmpeg 命令历史失败：{error}"))?
}

#[tauri::command]
async fn append_ffmpeg_command_history(
    app: AppHandle,
    item: FfmpegCommandHistoryInput,
) -> Result<Vec<FfmpegCommandHistoryItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        append_ffmpeg_command_history_item(&state, item)
    })
    .await
    .map_err(|error| format!("保存 FFmpeg 命令历史失败：{error}"))?
}

#[tauri::command]
async fn delete_ffmpeg_command_history_items(
    app: AppHandle,
    ids: Vec<String>,
) -> Result<Vec<FfmpegCommandHistoryItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        delete_ffmpeg_command_history_by_ids(&state, &ids)
    })
    .await
    .map_err(|error| format!("删除 FFmpeg 命令历史失败：{error}"))?
}

#[tauri::command]
async fn clear_ffmpeg_command_history(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        clear_ffmpeg_command_history_items(&state)
    })
    .await
    .map_err(|error| format!("清空 FFmpeg 命令历史失败：{error}"))?
}

#[tauri::command]
async fn prefill_terminal_command(
    command: String,
    working_dir: String,
) -> Result<TerminalPrefillResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if command.trim().is_empty() {
            return Err("没有可带入终端的命令。".to_string());
        }

        let working_dir = PathBuf::from(working_dir.trim());
        if !working_dir.is_dir() {
            return Err(format!("工作目录不存在：{}", working_dir.display()));
        }

        copy_to_clipboard(&command)?;
        open_terminal_at(&working_dir)?;

        #[cfg(target_os = "macos")]
        {
            thread::sleep(Duration::from_millis(650));
            let output = Command::new("osascript")
                .arg("-e")
                .arg("tell application \"Terminal\" to activate")
                .arg("-e")
                .arg("delay 0.2")
                .arg("-e")
                .arg("tell application \"System Events\" to keystroke \"v\" using command down")
                .output()
                .map_err(|error| format!("调用 AppleScript 失败：{error}"))?;

            if output.status.success() {
                return Ok(TerminalPrefillResult {
                    prefilled: true,
                    message: "命令已带入终端，尚未执行。".to_string(),
                });
            }

            return Ok(TerminalPrefillResult {
                prefilled: false,
                message: "命令已复制并打开终端，但 macOS 拒绝自动粘贴，请手动 Command+V。"
                    .to_string(),
            });
        }

        #[cfg(not(target_os = "macos"))]
        Ok(TerminalPrefillResult {
            prefilled: false,
            message: "命令已复制，并已尝试打开终端或输出目录。".to_string(),
        })
    })
    .await
    .map_err(|error| format!("带入终端失败：{error}"))?
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            check_dependencies,
            check_tool_updates,
            list_supported_sites,
            load_tool_settings,
            save_tool_path,
            clear_tool_path,
            save_proxy_settings,
            default_download_dir,
            probe_url,
            parse_download_queue,
            start_download,
            cancel_download,
            pause_download,
            cancel_ytdlp_operation,
            scan_download_cleanup,
            cleanup_download_cache,
            reveal_file,
            load_history,
            delete_history_item,
            build_ffmpeg_command,
            load_ffmpeg_command_history,
            append_ffmpeg_command_history,
            delete_ffmpeg_command_history_items,
            clear_ffmpeg_command_history,
            prefill_terminal_command
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn validate_url(url: &str) -> Result<(), String> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(())
    } else {
        Err("请输入 http:// 或 https:// 开头的视频地址。".to_string())
    }
}

fn uuid_like_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let count = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("task-{nanos}-{count}")
}

fn parse_queue_url(
    state: &AppState,
    settings: &ToolSettings,
    yt_dlp_path: &Path,
    url: &str,
    browser: Option<&str>,
    source_order: usize,
    operation_id: Option<&str>,
) -> Result<Vec<BatchParseItem>, String> {
    validate_url(url)?;

    let mut command = Command::new(yt_dlp_path);
    apply_tool_env_from_settings(settings, &mut command);
    apply_ytdlp_proxy_from_settings(settings, &mut command)?;
    command
        .arg("--dump-single-json")
        .arg("--skip-download")
        .arg("--no-warnings")
        .arg("--socket-timeout")
        .arg("30")
        .arg("--yes-playlist")
        .arg("--flat-playlist");

    if let Some(browser) = browser {
        command.arg("--cookies-from-browser").arg(browser);
    }

    command.arg(url);

    let output =
        run_ytdlp_command_with_timeout(state, command, Duration::from_secs(60), operation_id)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "yt-dlp 解析失败，未返回可用信息。".to_string()
        } else {
            stderr
        });
    }

    let json: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("解析 yt-dlp JSON 失败：{error}"))?;

    if let Some(entries) = json.get("entries").and_then(Value::as_array) {
        let playlist_site = string_field(&json, "extractor_key")
            .or_else(|| string_field(&json, "extractor"))
            .unwrap_or_else(|| "Playlist".to_string());
        let playlist_title = string_field(&json, "title");
        let playlist_thumbnail = thumbnail_url(&json);
        let playlist_total = entries.len();
        let parsed = entries
            .iter()
            .enumerate()
            .filter_map(|(entry_index, entry)| {
                queue_item_from_playlist_entry(
                    entry,
                    entry_index + 1,
                    playlist_total,
                    url,
                    &playlist_site,
                    playlist_title.as_deref(),
                    playlist_thumbnail.as_deref(),
                    source_order,
                )
            })
            .collect::<Vec<_>>();

        if !parsed.is_empty() {
            return Ok(parsed);
        }

        if !entries.is_empty() {
            return Err(
                "已检测到播放列表条目，但 yt-dlp 未返回可下载的子项链接。请升级 yt-dlp 后重试。"
                    .to_string(),
            );
        }
    }

    Ok(vec![BatchParseItem {
        id: uuid_like_id(),
        url: string_field(&json, "webpage_url").unwrap_or_else(|| url.to_string()),
        title: string_field(&json, "title"),
        site: string_field(&json, "extractor_key").or_else(|| string_field(&json, "extractor")),
        duration: json.get("duration").and_then(Value::as_f64),
        thumbnail: thumbnail_url(&json),
        source_url: None,
        playlist_title: None,
        playlist_index: None,
        playlist_total: None,
        source_order: Some(source_order),
        is_playlist_item: false,
        error: None,
    }])
}

fn queue_item_from_playlist_entry(
    entry: &Value,
    fallback_index: usize,
    playlist_total: usize,
    source_url: &str,
    fallback_site: &str,
    fallback_title: Option<&str>,
    fallback_thumbnail: Option<&str>,
    source_order: usize,
) -> Option<BatchParseItem> {
    let url = playlist_entry_url(entry, source_url)?;
    let entry_title = string_field(entry, "title");
    let title = entry_title
        .filter(|title| !is_placeholder_text(title) && !is_opaque_id_text(title))
        .or_else(|| fallback_title.and_then(clean_text));
    let playlist_index = usize_field(entry, "playlist_index")
        .or_else(|| usize_field(entry, "playlist_autonumber"))
        .unwrap_or(fallback_index);

    Some(BatchParseItem {
        id: uuid_like_id(),
        url,
        title,
        site: string_field(entry, "extractor_key")
            .or_else(|| string_field(entry, "extractor"))
            .or_else(|| Some(fallback_site.to_string())),
        duration: entry.get("duration").and_then(Value::as_f64),
        thumbnail: thumbnail_url(entry).or_else(|| fallback_thumbnail.map(ToString::to_string)),
        source_url: Some(source_url.to_string()),
        playlist_title: fallback_title.and_then(clean_text),
        playlist_index: Some(playlist_index),
        playlist_total: Some(playlist_total),
        source_order: Some(source_order),
        is_playlist_item: true,
        error: None,
    })
}

fn playlist_entry_url(entry: &Value, source_url: &str) -> Option<String> {
    let raw = string_field(entry, "webpage_url")
        .or_else(|| string_field(entry, "original_url"))
        .or_else(|| string_field(entry, "url"))?;
    normalize_playlist_entry_url(&raw, entry, source_url)
}

fn normalize_playlist_entry_url(raw_url: &str, entry: &Value, source_url: &str) -> Option<String> {
    let raw_url = raw_url.trim();
    if raw_url.is_empty() {
        return None;
    }

    if raw_url.starts_with("http://") || raw_url.starts_with("https://") {
        return Some(raw_url.to_string());
    }

    if raw_url.starts_with("//") {
        return Some(format!("https:{raw_url}"));
    }

    if raw_url.starts_with('/') {
        return url_origin(source_url).map(|origin| format!("{origin}{raw_url}"));
    }

    let extractor = string_field(entry, "ie_key")
        .or_else(|| string_field(entry, "extractor_key"))
        .or_else(|| string_field(entry, "extractor"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let source_lower = source_url.to_ascii_lowercase();

    if extractor.contains("youtube")
        || source_lower.contains("youtube.com")
        || source_lower.contains("youtu.be")
    {
        if raw_url.starts_with("watch?")
            || raw_url.starts_with("shorts/")
            || raw_url.starts_with("live/")
        {
            return url_origin(source_url).map(|origin| format!("{origin}/{raw_url}"));
        }

        return Some(format!("https://www.youtube.com/watch?v={raw_url}"));
    }

    if extractor.contains("bilibili") || source_lower.contains("bilibili.com") {
        if raw_url.starts_with("BV") || raw_url.starts_with("av") {
            return Some(format!("https://www.bilibili.com/video/{raw_url}"));
        }
    }

    None
}

fn url_origin(url: &str) -> Option<String> {
    let scheme_end = url.find("://")?;
    let after_scheme = scheme_end + 3;
    let host_end = url[after_scheme..]
        .find('/')
        .map(|index| after_scheme + index)
        .unwrap_or(url.len());

    Some(url[..host_end].to_string())
}

fn supported_site_examples(extractors: &[String]) -> Vec<SupportedSiteExample> {
    let candidates = [
        (
            "Bilibili",
            "https://www.bilibili.com/video/BV1xx411c7mD/",
            &["bilibili"][..],
        ),
        (
            "YouTube",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            &["youtube"][..],
        ),
        ("Vimeo", "https://vimeo.com/76979871", &["vimeo"][..]),
        (
            "TikTok",
            "https://www.tiktok.com/@example/video/0000000000000000000",
            &["tiktok"][..],
        ),
        (
            "X / Twitter",
            "https://x.com/example/status/0000000000000000000",
            &["twitter"][..],
        ),
        (
            "Instagram",
            "https://www.instagram.com/p/C0000000000/",
            &["instagram"][..],
        ),
        (
            "Twitch",
            "https://www.twitch.tv/videos/0000000000",
            &["twitch"][..],
        ),
        (
            "Youku",
            "https://v.youku.com/v_show/id_XNDAw0000000.html",
            &["youku"][..],
        ),
        (
            "Douyin",
            "https://www.douyin.com/video/0000000000000000000",
            &["douyin"][..],
        ),
        (
            "Xiaohongshu",
            "https://www.xiaohongshu.com/explore/000000000000000000000000",
            &["xiaohongshu"][..],
        ),
    ];
    let normalized = extractors
        .iter()
        .map(|extractor| extractor.to_ascii_lowercase())
        .collect::<Vec<_>>();

    candidates
        .iter()
        .filter(|(_, _, needles)| {
            needles.iter().any(|needle| {
                normalized
                    .iter()
                    .any(|extractor| extractor.contains(needle))
            })
        })
        .take(6)
        .map(|(name, url, _)| SupportedSiteExample {
            name: (*name).to_string(),
            url: (*url).to_string(),
        })
        .collect()
}

fn validate_media_file(label: &str, value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value.trim());
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("{label}不存在或不是文件：{}", path.display()))
    }
}

fn resolve_ffmpeg_working_dir(output_dir: &str, input_path: &Path) -> Result<PathBuf, String> {
    let working_dir = if output_dir.trim().is_empty() {
        input_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        PathBuf::from(output_dir.trim())
    };

    if working_dir.is_dir() {
        Ok(working_dir)
    } else {
        Err(format!("输出目录不存在：{}", working_dir.display()))
    }
}

fn ffmpeg_command_args(
    ffmpeg_path: &Path,
    input_path: &Path,
    output_path: &Path,
    request: &FfmpegCommandRequest,
) -> Result<Vec<String>, String> {
    let mut args = vec![
        ffmpeg_path.display().to_string(),
        "-hide_banner".to_string(),
        "-n".to_string(),
    ];

    match request.preset_id.as_str() {
        "convertMp4" => {
            args.extend([
                "-i".to_string(),
                input_path.display().to_string(),
                "-c:v".to_string(),
                "libx264".to_string(),
                "-preset".to_string(),
                "medium".to_string(),
                "-crf".to_string(),
                "20".to_string(),
                "-c:a".to_string(),
                "aac".to_string(),
                "-b:a".to_string(),
                "192k".to_string(),
                "-movflags".to_string(),
                "+faststart".to_string(),
                output_path.display().to_string(),
            ]);
        }
        "compress" => {
            let crf = request.crf.unwrap_or(28).clamp(18, 35);
            args.extend([
                "-i".to_string(),
                input_path.display().to_string(),
                "-c:v".to_string(),
                "libx264".to_string(),
                "-preset".to_string(),
                "medium".to_string(),
                "-crf".to_string(),
                crf.to_string(),
                "-c:a".to_string(),
                "aac".to_string(),
                "-b:a".to_string(),
                "128k".to_string(),
                "-movflags".to_string(),
                "+faststart".to_string(),
                output_path.display().to_string(),
            ]);
        }
        "extractAudio" => {
            args.extend([
                "-i".to_string(),
                input_path.display().to_string(),
                "-vn".to_string(),
            ]);
            match normalized_audio_format(request.audio_format.as_deref())? {
                "mp3" => args.extend([
                    "-c:a".to_string(),
                    "libmp3lame".to_string(),
                    "-q:a".to_string(),
                    "2".to_string(),
                ]),
                "m4a" => args.extend([
                    "-c:a".to_string(),
                    "aac".to_string(),
                    "-b:a".to_string(),
                    "192k".to_string(),
                ]),
                _ => unreachable!(),
            }
            args.push(output_path.display().to_string());
        }
        "trim" => {
            let start_time = normalized_time(request.start_time.as_deref())
                .ok_or_else(|| "请填写截取开始时间。".to_string())?;
            validate_time_value("开始时间", &start_time)?;
            args.extend(["-ss".to_string(), start_time]);

            if let Some(end_time) = normalized_time(request.end_time.as_deref()) {
                validate_time_value("结束时间", &end_time)?;
                args.extend(["-to".to_string(), end_time]);
            }

            args.extend([
                "-i".to_string(),
                input_path.display().to_string(),
                "-c".to_string(),
                "copy".to_string(),
                output_path.display().to_string(),
            ]);
        }
        "mergeAudioVideo" => {
            let secondary_input = request
                .secondary_input_path
                .as_deref()
                .ok_or_else(|| "请选择要合并的音频文件。".to_string())?;
            let audio_path = validate_media_file("音频文件", secondary_input)?;
            args.extend([
                "-i".to_string(),
                input_path.display().to_string(),
                "-i".to_string(),
                audio_path.display().to_string(),
                "-map".to_string(),
                "0:v:0".to_string(),
                "-map".to_string(),
                "1:a:0".to_string(),
                "-c:v".to_string(),
                "copy".to_string(),
                "-c:a".to_string(),
                "aac".to_string(),
                "-shortest".to_string(),
                output_path.display().to_string(),
            ]);
        }
        _ => return Err(format!("不支持的 FFmpeg 预设：{}", request.preset_id)),
    }

    Ok(args)
}

fn unique_ffmpeg_output_path(
    working_dir: &Path,
    input_path: &Path,
    request: &FfmpegCommandRequest,
) -> Result<PathBuf, String> {
    let stem = input_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("video");
    let suffix = ffmpeg_preset_suffix(&request.preset_id)?;
    let extension = ffmpeg_output_extension(input_path, request)?;

    for index in 1..10_000 {
        let file_name = if index == 1 {
            format!("{stem}-{suffix}.{extension}")
        } else {
            format!("{stem}-{suffix}-{index}.{extension}")
        };
        let candidate = working_dir.join(file_name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err("无法生成未被占用的输出文件名。".to_string())
}

fn ffmpeg_preset_suffix(preset_id: &str) -> Result<&'static str, String> {
    match preset_id {
        "convertMp4" => Ok("converted"),
        "compress" => Ok("compressed"),
        "extractAudio" => Ok("audio"),
        "trim" => Ok("clip"),
        "mergeAudioVideo" => Ok("merged"),
        _ => Err(format!("不支持的 FFmpeg 预设：{preset_id}")),
    }
}

fn ffmpeg_output_extension(
    input_path: &Path,
    request: &FfmpegCommandRequest,
) -> Result<String, String> {
    match request.preset_id.as_str() {
        "convertMp4" | "compress" | "mergeAudioVideo" => Ok("mp4".to_string()),
        "extractAudio" => Ok(normalized_audio_format(request.audio_format.as_deref())?.to_string()),
        "trim" => Ok(input_path
            .extension()
            .and_then(|value| value.to_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("mp4")
            .to_ascii_lowercase()),
        _ => Err(format!("不支持的 FFmpeg 预设：{}", request.preset_id)),
    }
}

fn normalized_audio_format(value: Option<&str>) -> Result<&'static str, String> {
    match value.unwrap_or("mp3").trim().to_ascii_lowercase().as_str() {
        "mp3" => Ok("mp3"),
        "m4a" => Ok("m4a"),
        other => Err(format!("不支持的音频格式：{other}")),
    }
}

fn normalized_time(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn validate_time_value(label: &str, value: &str) -> Result<(), String> {
    let valid = value
        .chars()
        .all(|character| character.is_ascii_digit() || character == ':' || character == '.');
    if valid && value.len() <= 24 {
        Ok(())
    } else {
        Err(format!("{label}只能包含数字、冒号和小数点。"))
    }
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_./:+=@%-".contains(character))
    {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}

fn copy_to_clipboard(value: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        return write_to_clipboard_command("pbcopy", &[], value);
    }

    #[cfg(target_os = "windows")]
    {
        return write_to_clipboard_command("cmd", &["/C", "clip"], value);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        write_to_clipboard_command("wl-copy", &[], value)
            .or_else(|_| write_to_clipboard_command("xclip", &["-selection", "clipboard"], value))
            .or_else(|_| write_to_clipboard_command("xsel", &["--clipboard", "--input"], value))
            .map_err(|_| "无法写入系统剪贴板，请手动复制命令。".to_string())
    }
}

fn write_to_clipboard_command(command: &str, args: &[&str], value: &str) -> Result<(), String> {
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("无法启动剪贴板命令 {command}：{error}"))?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(value.as_bytes())
            .map_err(|error| format!("写入剪贴板失败：{error}"))?;
    }

    let status = child
        .wait()
        .map_err(|error| format!("等待剪贴板命令失败：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("剪贴板命令 {command} 返回了失败状态。"))
    }
}

fn open_terminal_at(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("open")
            .arg("-a")
            .arg("Terminal")
            .arg(path)
            .status()
            .map_err(|error| format!("打开终端失败：{error}"))?;

        if status.success() {
            return Ok(());
        }

        return Err("Terminal 返回了失败状态。".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        let cd_command = format!("cd /d \"{}\"", path.display());
        let status = Command::new("cmd")
            .args(["/C", "start", "", "cmd", "/K", &cd_command])
            .status()
            .map_err(|error| format!("打开命令提示符失败：{error}"))?;

        if status.success() {
            return Ok(());
        }

        return Err("命令提示符返回了失败状态。".to_string());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if Command::new("x-terminal-emulator")
            .current_dir(path)
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
        {
            return Ok(());
        }

        let status = Command::new("xdg-open")
            .arg(path)
            .status()
            .map_err(|error| format!("打开输出目录失败：{error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err("系统返回了失败状态。".to_string())
        }
    }
}

fn ensure_tool(state: &AppState, name: &str) -> Result<PathBuf, String> {
    let settings = read_tool_settings_with_fallback(state);
    ensure_tool_with_settings(&settings, name)
}

fn ensure_tool_with_settings(settings: &ToolSettings, name: &str) -> Result<PathBuf, String> {
    find_executable_with_settings(settings, name)
        .map(|resolution| resolution.path)
        .ok_or_else(|| format!(
            "未找到 {name}。已检查手动指定路径、环境变量、当前 PATH 以及常见 Homebrew 路径：{}。请确认已安装并可执行：brew install yt-dlp ffmpeg",
            FALLBACK_TOOL_DIRS.join(", ")
        ))
}

fn tool_status(state: &AppState, name: &str) -> ToolStatus {
    let tool = ManagedTool::from_name(name).ok();
    let resolution = find_executable(state, name);
    let installed = resolution.is_some();
    let version = tool
        .zip(resolution.as_ref())
        .and_then(|(tool, resolution)| tool_version(tool, &resolution.path))
        .map(first_line);

    ToolStatus {
        name: name.to_string(),
        installed,
        path: resolution
            .as_ref()
            .map(|resolution| resolution.path.display().to_string()),
        version,
        source: resolution.map(|resolution| resolution.source.as_str().to_string()),
    }
}

fn find_executable(state: &AppState, name: &str) -> Option<ToolResolution> {
    let settings = read_tool_settings_with_fallback(state);
    find_executable_with_settings(&settings, name)
}

fn find_executable_with_settings(settings: &ToolSettings, name: &str) -> Option<ToolResolution> {
    let tool = ManagedTool::from_name(name).ok()?;
    if let Some(path) = tool
        .path(settings)
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Some(ToolResolution {
            path,
            source: ToolSource::Manual,
        });
    }

    if let Some(path) = env::var_os(tool.env_var())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Some(ToolResolution {
            path,
            source: ToolSource::Env,
        });
    }

    for dir in candidate_tool_dirs() {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(ToolResolution {
                path: candidate,
                source: ToolSource::Path,
            });
        }

        #[cfg(target_os = "windows")]
        {
            let candidate = dir.join(format!("{name}.exe"));
            if candidate.is_file() {
                return Some(ToolResolution {
                    path: candidate,
                    source: ToolSource::Path,
                });
            }
        }
    }

    None
}

fn candidate_tool_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(path_var) = env::var_os("PATH") {
        dirs.extend(env::split_paths(&path_var));
    }

    dirs.extend(FALLBACK_TOOL_DIRS.iter().map(PathBuf::from));
    dedupe_paths(dirs)
}

fn enhanced_path_env(state: &AppState) -> OsString {
    let settings = read_tool_settings_with_fallback(state);
    enhanced_path_env_from_settings(&settings)
}

fn enhanced_path_env_from_settings(settings: &ToolSettings) -> OsString {
    let mut dirs = Vec::new();

    dirs.extend(
        [ManagedTool::YtDlp, ManagedTool::Ffmpeg]
            .iter()
            .filter_map(|tool| tool.path(settings))
            .map(PathBuf::from)
            .filter_map(|path| path.parent().map(Path::to_path_buf)),
    );

    dirs.extend(
        [ManagedTool::YtDlp, ManagedTool::Ffmpeg]
            .iter()
            .filter_map(|tool| env::var_os(tool.env_var()))
            .map(PathBuf::from)
            .filter_map(|path| path.parent().map(Path::to_path_buf)),
    );

    dirs.extend(candidate_tool_dirs());
    let dirs = dedupe_paths(dirs);
    env::join_paths(dirs).unwrap_or_else(|_| {
        env::var_os("PATH").unwrap_or_else(|| OsString::from(FALLBACK_TOOL_DIRS.join(":")))
    })
}

fn apply_tool_env(state: &AppState, command: &mut Command) {
    command.env("PATH", enhanced_path_env(state));
}

fn apply_tool_env_from_settings(settings: &ToolSettings, command: &mut Command) {
    command.env("PATH", enhanced_path_env_from_settings(settings));
}

fn apply_ytdlp_proxy_from_settings(
    settings: &ToolSettings,
    command: &mut Command,
) -> Result<(), String> {
    match normalize_proxy_mode(settings.proxy_mode.as_deref()) {
        ProxyMode::Manual => {
            let proxy = normalize_proxy_url(settings.proxy_url.as_deref(), ProxyMode::Manual)?
                .ok_or_else(|| "请填写手动代理地址。".to_string())?;
            command.arg("--proxy").arg(proxy);
        }
        ProxyMode::Off => {
            command.arg("--proxy").arg("");
        }
        ProxyMode::Auto => {
            if let Some(proxy) = proxy_status(settings).effective_proxy {
                command.arg("--proxy").arg(proxy);
            }
        }
    }

    Ok(())
}

fn normalize_proxy_mode(value: Option<&str>) -> ProxyMode {
    match value.unwrap_or("auto").trim().to_ascii_lowercase().as_str() {
        "manual" => ProxyMode::Manual,
        "off" | "none" | "disabled" => ProxyMode::Off,
        _ => ProxyMode::Auto,
    }
}

impl ProxyMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
            Self::Off => "off",
        }
    }
}

fn normalize_proxy_url(value: Option<&str>, mode: ProxyMode) -> Result<Option<String>, String> {
    if mode != ProxyMode::Manual {
        return Ok(None);
    }

    let value = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "请填写手动代理地址。".to_string())?;
    validate_proxy_url(value)?;
    Ok(Some(value.to_string()))
}

fn validate_proxy_url(value: &str) -> Result<(), String> {
    let lower = value.to_ascii_lowercase();
    let supported = ["http://", "https://", "socks5://", "socks5h://"]
        .iter()
        .any(|prefix| lower.starts_with(prefix));

    if supported {
        Ok(())
    } else {
        Err("代理地址仅支持 http://、https://、socks5:// 或 socks5h://。".to_string())
    }
}

fn proxy_status(settings: &ToolSettings) -> ProxyStatus {
    let mode = normalize_proxy_mode(settings.proxy_mode.as_deref());

    match mode {
        ProxyMode::Manual => match normalize_proxy_url(settings.proxy_url.as_deref(), mode) {
            Ok(Some(proxy)) => ProxyStatus {
                mode: mode.as_str().to_string(),
                effective_proxy: Some(proxy),
                source: Some("manual".to_string()),
                message: Some("手动指定代理".to_string()),
            },
            Ok(None) => ProxyStatus {
                mode: mode.as_str().to_string(),
                effective_proxy: None,
                source: Some("manual".to_string()),
                message: Some("请填写手动代理地址。".to_string()),
            },
            Err(error) => ProxyStatus {
                mode: mode.as_str().to_string(),
                effective_proxy: None,
                source: Some("manual".to_string()),
                message: Some(error),
            },
        },
        ProxyMode::Off => ProxyStatus {
            mode: mode.as_str().to_string(),
            effective_proxy: None,
            source: Some("off".to_string()),
            message: Some("不使用代理".to_string()),
        },
        ProxyMode::Auto => auto_proxy_status(),
    }
}

fn auto_proxy_status() -> ProxyStatus {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("scutil");
        command.arg("--proxy");
        return match run_command_with_timeout_named(command, Duration::from_secs(4), "系统代理读取")
        {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout);
                proxy_status_from_scutil_output(&text)
            }
            Ok(output) => {
                let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
                ProxyStatus {
                    mode: ProxyMode::Auto.as_str().to_string(),
                    effective_proxy: None,
                    source: Some("error".to_string()),
                    message: Some(if message.is_empty() {
                        "读取 macOS 系统代理失败。".to_string()
                    } else {
                        message
                    }),
                }
            }
            Err(error) => ProxyStatus {
                mode: ProxyMode::Auto.as_str().to_string(),
                effective_proxy: None,
                source: Some("error".to_string()),
                message: Some(error),
            },
        };
    }

    #[cfg(not(target_os = "macos"))]
    {
        ProxyStatus {
            mode: ProxyMode::Auto.as_str().to_string(),
            effective_proxy: None,
            source: Some("unsupported".to_string()),
            message: Some("自动读取系统代理目前仅支持 macOS。".to_string()),
        }
    }
}

fn proxy_status_from_scutil_output(output: &str) -> ProxyStatus {
    let entries = parse_scutil_proxy_entries(output);

    if let Some(proxy) = scutil_proxy_url(&entries, "HTTPS", "http") {
        return ProxyStatus {
            mode: ProxyMode::Auto.as_str().to_string(),
            effective_proxy: Some(proxy),
            source: Some("systemHttps".to_string()),
            message: Some("自动读取 HTTPS 系统代理".to_string()),
        };
    }

    if let Some(proxy) = scutil_proxy_url(&entries, "HTTP", "http") {
        return ProxyStatus {
            mode: ProxyMode::Auto.as_str().to_string(),
            effective_proxy: Some(proxy),
            source: Some("systemHttp".to_string()),
            message: Some("自动读取 HTTP 系统代理".to_string()),
        };
    }

    if let Some(proxy) = scutil_proxy_url(&entries, "SOCKS", "socks5h") {
        return ProxyStatus {
            mode: ProxyMode::Auto.as_str().to_string(),
            effective_proxy: Some(proxy),
            source: Some("systemSocks".to_string()),
            message: Some("自动读取 SOCKS 系统代理".to_string()),
        };
    }

    if scutil_enabled(&entries, "ProxyAutoConfigEnable") {
        return ProxyStatus {
            mode: ProxyMode::Auto.as_str().to_string(),
            effective_proxy: None,
            source: Some("pacUnsupported".to_string()),
            message: Some("检测到自动代理脚本（PAC），暂不支持解析，未传入代理。".to_string()),
        };
    }

    ProxyStatus {
        mode: ProxyMode::Auto.as_str().to_string(),
        effective_proxy: None,
        source: Some("none".to_string()),
        message: Some("未检测到系统代理".to_string()),
    }
}

fn parse_scutil_proxy_entries(output: &str) -> HashMap<String, String> {
    output
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            let key = key.trim();
            if key.is_empty() {
                return None;
            }

            Some((
                key.to_string(),
                value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string(),
            ))
        })
        .collect()
}

fn scutil_enabled(entries: &HashMap<String, String>, key: &str) -> bool {
    entries
        .get(key)
        .map(|value| {
            let value = value.trim().to_ascii_lowercase();
            value == "1" || value == "yes" || value == "true"
        })
        .unwrap_or(false)
}

fn scutil_proxy_url(
    entries: &HashMap<String, String>,
    prefix: &str,
    scheme: &str,
) -> Option<String> {
    if !scutil_enabled(entries, &format!("{prefix}Enable")) {
        return None;
    }

    let host = entries.get(&format!("{prefix}Proxy"))?.trim();
    let port = entries
        .get(&format!("{prefix}Port"))?
        .trim()
        .parse::<u16>()
        .ok()?;

    if host.is_empty() || port == 0 {
        return None;
    }

    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };

    Some(format!("{scheme}://{host}:{port}"))
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();

    for path in paths {
        if !deduped.iter().any(|existing| existing == &path) {
            deduped.push(path);
        }
    }

    deduped
}

fn tool_version(tool: ManagedTool, path: &Path) -> Option<String> {
    let version_arg = match tool {
        ManagedTool::YtDlp => "--version",
        ManagedTool::Ffmpeg => "-version",
    };
    command_version(path, version_arg)
}

fn command_version(path: &Path, version_arg: &str) -> Option<String> {
    let output = Command::new(path).arg(version_arg).output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn validate_tool_path(tool: ManagedTool, path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!(
            "{} 路径不是有效文件：{}",
            tool.display_name(),
            path.display()
        ));
    }

    tool_version(tool, path)
        .filter(|version| !version.trim().is_empty())
        .map(|_| ())
        .ok_or_else(|| {
            format!(
                "{} 路径不可执行或未能返回版本信息：{}",
                tool.display_name(),
                path.display()
            )
        })
}

fn tool_update_status(settings: &ToolSettings, tool: ManagedTool) -> ToolUpdateStatus {
    let current_version = find_executable_with_settings(settings, tool.display_name())
        .and_then(|resolution| tool_version(tool, &resolution.path))
        .map(first_line)
        .and_then(|version| normalized_tool_version(tool, &version));
    let checked_at = unix_timestamp();
    let update_command = Some(format!(
        "brew update && brew upgrade {}",
        tool.display_name()
    ));

    let latest_result = latest_tool_version(tool);
    match latest_result {
        Ok(latest_version) => {
            let update_available = current_version
                .as_deref()
                .map(|current| is_version_newer(&latest_version, current));

            ToolUpdateStatus {
                name: tool.display_name().to_string(),
                current_version,
                latest_version: Some(latest_version),
                update_available,
                checked_at,
                update_command,
                error: None,
            }
        }
        Err(error) => ToolUpdateStatus {
            name: tool.display_name().to_string(),
            current_version,
            latest_version: None,
            update_available: None,
            checked_at,
            update_command,
            error: Some(error),
        },
    }
}

fn latest_tool_version(tool: ManagedTool) -> Result<String, String> {
    let url = match tool {
        ManagedTool::YtDlp => "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest",
        ManagedTool::Ffmpeg => "https://formulae.brew.sh/api/formula/ffmpeg.json",
    };
    let json = fetch_json_url(url)?;
    let version = match tool {
        ManagedTool::YtDlp => string_field(&json, "tag_name"),
        ManagedTool::Ffmpeg => json
            .get("versions")
            .and_then(|versions| string_field(versions, "stable")),
    };

    version
        .and_then(|version| normalized_tool_version(tool, &version))
        .ok_or_else(|| format!("未能解析 {} 最新版本。", tool.display_name()))
}

fn fetch_json_url(url: &str) -> Result<Value, String> {
    let mut command = Command::new("curl");
    command.args([
        "-fsSL",
        "--connect-timeout",
        "6",
        "--max-time",
        "12",
        "-H",
        "User-Agent: VideoDownloader/0.1",
        url,
    ]);

    let output = run_command_with_timeout_named(command, Duration::from_secs(14), "版本检查")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "版本检查请求失败。".to_string()
        } else {
            stderr
        });
    }

    serde_json::from_slice(&output.stdout).map_err(|error| format!("解析版本检查响应失败：{error}"))
}

fn normalized_tool_version(tool: ManagedTool, value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let candidate = match tool {
        ManagedTool::YtDlp => trimmed.trim_start_matches('v').to_string(),
        ManagedTool::Ffmpeg => trimmed
            .strip_prefix("ffmpeg version ")
            .unwrap_or(trimmed)
            .split_whitespace()
            .next()
            .unwrap_or(trimmed)
            .trim_start_matches('n')
            .to_string(),
    };

    if candidate.is_empty() {
        None
    } else {
        Some(candidate)
    }
}

fn is_version_newer(latest: &str, current: &str) -> bool {
    compare_versions(latest, current).is_gt()
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left_parts = version_parts(left);
    let right_parts = version_parts(right);
    let length = left_parts.len().max(right_parts.len());

    for index in 0..length {
        let left = left_parts.get(index).copied().unwrap_or(0);
        let right = right_parts.get(index).copied().unwrap_or(0);
        match left.cmp(&right) {
            std::cmp::Ordering::Equal => {}
            ordering => return ordering,
        }
    }

    std::cmp::Ordering::Equal
}

fn version_parts(value: &str) -> Vec<u64> {
    value
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u64>().ok())
        .collect()
}

fn run_command_with_timeout(command: Command, timeout: Duration) -> Result<Output, String> {
    run_command_with_timeout_named(command, timeout, "yt-dlp")
}

fn run_command_with_timeout_named(
    mut command: Command,
    timeout: Duration,
    label: &str,
) -> Result<Output, String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("无法启动 {label}：{error}"))?;

    let started_at = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|error| format!("等待 {label} 失败：{error}"))?
            .is_some()
        {
            return child
                .wait_with_output()
                .map_err(|error| format!("读取 {label} 输出失败：{error}"));
        }

        if started_at.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            let hint = if label == "yt-dlp" {
                "，请检查网络或代理"
            } else {
                ""
            };
            return Err(format!(
                "{label} 超过 {} 秒未返回{hint}。",
                timeout.as_secs(),
            ));
        }

        thread::sleep(Duration::from_millis(120));
    }
}

fn run_ytdlp_command_with_timeout(
    state: &AppState,
    mut command: Command,
    timeout: Duration,
    operation_id: Option<&str>,
) -> Result<Output, String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("无法启动 yt-dlp：{error}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法读取 yt-dlp 输出。".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "无法读取 yt-dlp 错误输出。".to_string())?;
    let stdout_handle = thread::spawn(move || read_stream_to_end(stdout));
    let stderr_handle = thread::spawn(move || read_stream_to_end(stderr));
    let child = Arc::new(Mutex::new(child));
    let canceled = Arc::new(AtomicBool::new(false));
    let operation_key = operation_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    if let Some(operation_key) = operation_key.as_deref() {
        register_ytdlp_operation(
            state,
            operation_key,
            Arc::clone(&child),
            Arc::clone(&canceled),
        )?;
    }

    let started_at = Instant::now();
    loop {
        if canceled.load(Ordering::SeqCst) {
            terminate_ytdlp_child(&child);
            cleanup_ytdlp_operation(state, operation_key.as_deref());
            let _ = join_stream_handle(stdout_handle, "yt-dlp stdout");
            let _ = join_stream_handle(stderr_handle, "yt-dlp stderr");
            return Err(YTDLP_OPERATION_CANCELED_MESSAGE.to_string());
        }

        let status = {
            let mut child = child
                .lock()
                .map_err(|_| "yt-dlp 子进程锁已损坏。".to_string())?;
            child
                .try_wait()
                .map_err(|error| format!("等待 yt-dlp 失败：{error}"))?
        };

        if let Some(status) = status {
            cleanup_ytdlp_operation(state, operation_key.as_deref());
            let stdout = join_stream_handle(stdout_handle, "yt-dlp stdout")?;
            let stderr = join_stream_handle(stderr_handle, "yt-dlp stderr")?;
            return Ok(Output {
                status,
                stdout,
                stderr,
            });
        }

        if started_at.elapsed() >= timeout {
            terminate_ytdlp_child(&child);
            cleanup_ytdlp_operation(state, operation_key.as_deref());
            let _ = join_stream_handle(stdout_handle, "yt-dlp stdout");
            let _ = join_stream_handle(stderr_handle, "yt-dlp stderr");
            return Err(format!(
                "yt-dlp 超过 {} 秒未返回，请检查网络或代理。",
                timeout.as_secs(),
            ));
        }

        thread::sleep(Duration::from_millis(120));
    }
}

fn read_stream_to_end<R: Read>(mut reader: R) -> Vec<u8> {
    let mut output = Vec::new();
    let _ = reader.read_to_end(&mut output);
    output
}

fn join_stream_handle(handle: thread::JoinHandle<Vec<u8>>, label: &str) -> Result<Vec<u8>, String> {
    handle
        .join()
        .map_err(|_| format!("读取 {label} 输出线程失败。"))
}

fn register_ytdlp_operation(
    state: &AppState,
    operation_id: &str,
    child: Arc<Mutex<Child>>,
    canceled: Arc<AtomicBool>,
) -> Result<(), String> {
    let previous = {
        let mut operations = state
            .ytdlp_operations
            .lock()
            .map_err(|_| "yt-dlp 操作状态锁已损坏。".to_string())?;
        operations.insert(
            operation_id.to_string(),
            YtdlpOperationControl { child, canceled },
        )
    };

    if let Some(previous) = previous {
        previous.canceled.store(true, Ordering::SeqCst);
        if let Ok(mut child) = previous.child.lock() {
            let _ = child.kill();
        }
    }

    Ok(())
}

fn cleanup_ytdlp_operation(state: &AppState, operation_id: Option<&str>) {
    let Some(operation_id) = operation_id else {
        return;
    };

    if let Ok(mut operations) = state.ytdlp_operations.lock() {
        operations.remove(operation_id);
    }
}

fn terminate_ytdlp_child(child: &Arc<Mutex<Child>>) {
    if let Ok(mut child) = child.lock() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn first_line(value: String) -> String {
    value.lines().next().unwrap_or("").trim().to_string()
}

fn normalized_browser(browser: Option<String>) -> Option<String> {
    match browser.as_deref() {
        Some("chrome") | Some("safari") | Some("firefox") => browser,
        _ => None,
    }
}

fn string_field(json: &Value, key: &str) -> Option<String> {
    json.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn usize_field(json: &Value, key: &str) -> Option<usize> {
    json.get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn thumbnail_url(json: &Value) -> Option<String> {
    string_field(json, "thumbnail")
        .or_else(|| {
            json.get("thumbnails")
                .and_then(Value::as_array)
                .and_then(|thumbnails| thumbnails.iter().find_map(|item| string_field(item, "url")))
        })
        .map(normalize_thumbnail_url)
}

fn probe_local_media(state: &AppState, ffmpeg_path: &Path, media_path: &Path) -> LocalMediaInfo {
    let probed_at = Some(unix_timestamp());

    if !media_path.is_file() {
        return LocalMediaInfo {
            probed_at,
            error: Some(format!("本地媒体文件不存在：{}", media_path.display())),
            ..Default::default()
        };
    }

    let mut command = Command::new(ffprobe_path_from_ffmpeg(ffmpeg_path));
    apply_tool_env(state, &mut command);
    command
        .arg("-v")
        .arg("error")
        .arg("-print_format")
        .arg("json")
        .arg("-show_format")
        .arg("-show_streams")
        .arg(media_path);

    let output = match run_command_with_timeout_named(command, Duration::from_secs(20), "ffprobe") {
        Ok(output) => output,
        Err(error) => {
            return LocalMediaInfo {
                probed_at,
                error: Some(error),
                ..Default::default()
            };
        }
    };

    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return LocalMediaInfo {
            probed_at,
            error: Some(if message.is_empty() {
                "ffprobe 未返回可用媒体信息。".to_string()
            } else {
                message
            }),
            ..Default::default()
        };
    }

    let json = match serde_json::from_slice::<Value>(&output.stdout) {
        Ok(json) => json,
        Err(error) => {
            return LocalMediaInfo {
                probed_at,
                error: Some(format!("解析 ffprobe JSON 失败：{error}")),
                ..Default::default()
            };
        }
    };

    parse_local_media_info(&json, probed_at)
}

fn ffprobe_path_from_ffmpeg(ffmpeg_path: &Path) -> PathBuf {
    let executable_name = if cfg!(target_os = "windows") {
        "ffprobe.exe"
    } else {
        "ffprobe"
    };

    if let Some(parent) = ffmpeg_path.parent() {
        let candidate = parent.join(executable_name);
        if candidate.is_file() {
            return candidate;
        }
    }

    PathBuf::from(executable_name)
}

fn parse_local_media_info(json: &Value, probed_at: Option<String>) -> LocalMediaInfo {
    let streams = json
        .get("streams")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let video_stream = streams
        .iter()
        .find(|stream| string_field(stream, "codec_type").as_deref() == Some("video"));
    let audio_stream = streams
        .iter()
        .find(|stream| string_field(stream, "codec_type").as_deref() == Some("audio"));
    let duration = json
        .get("format")
        .and_then(|format| number_field(format, "duration"))
        .or_else(|| {
            streams
                .iter()
                .filter_map(|stream| number_field(stream, "duration"))
                .max_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
        });

    LocalMediaInfo {
        duration,
        width: video_stream.and_then(|stream| u64_field(stream, "width")),
        height: video_stream.and_then(|stream| u64_field(stream, "height")),
        video_codec: video_stream.and_then(|stream| codec_field(stream, "codec_name")),
        audio_codec: audio_stream.and_then(|stream| codec_field(stream, "codec_name")),
        probed_at,
        error: None,
    }
}

fn compare_media(
    expected: Option<&ExpectedMediaInfo>,
    actual: &LocalMediaInfo,
) -> Option<MediaComparison> {
    let expected = expected?;
    let duration = compare_duration(expected.duration, actual.duration);
    let resolution = compare_resolution(
        expected.resolution_label.as_deref(),
        expected.resolution_score,
        actual.width,
        actual.height,
    );

    if duration.is_none() && resolution.is_none() {
        None
    } else {
        Some(MediaComparison {
            duration,
            resolution,
        })
    }
}

fn compare_duration(expected: Option<f64>, actual: Option<f64>) -> Option<MediaComparisonDetail> {
    let expected = expected.filter(|value| value.is_finite() && *value > 0.0)?;
    let actual = actual.filter(|value| value.is_finite() && *value > 0.0)?;
    let tolerance = (expected * 0.02).max(5.0);
    let delta = actual - expected;

    if delta.abs() <= tolerance {
        return None;
    }

    Some(MediaComparisonDetail {
        status: if delta < 0.0 { "shorter" } else { "longer" }.to_string(),
        expected_label: Some(format_media_duration(expected)),
        actual_label: Some(format_media_duration(actual)),
    })
}

fn compare_resolution(
    expected_label: Option<&str>,
    expected_score: Option<u64>,
    actual_width: Option<u64>,
    actual_height: Option<u64>,
) -> Option<MediaComparisonDetail> {
    let expected_score = expected_score.filter(|score| *score > 0)?;
    let (actual_width, actual_height) = actual_width.zip(actual_height)?;
    if actual_width == 0 || actual_height == 0 {
        return None;
    }

    let actual_score = actual_width.saturating_mul(actual_height);
    if actual_score == expected_score {
        return None;
    }

    Some(MediaComparisonDetail {
        status: if actual_score < expected_score {
            "lower"
        } else {
            "higher"
        }
        .to_string(),
        expected_label: expected_label
            .and_then(clean_text)
            .or_else(|| Some(format!("{expected_score} px"))),
        actual_label: Some(format!("{actual_width}×{actual_height}")),
    })
}

fn format_media_duration(duration: f64) -> String {
    let total_seconds = duration.max(0.0).round() as u64;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn number_field(json: &Value, key: &str) -> Option<f64> {
    match json.get(key)? {
        Value::Number(value) => value.as_f64(),
        Value::String(value) => value.parse::<f64>().ok(),
        _ => None,
    }
    .filter(|value| value.is_finite())
}

fn u64_field(json: &Value, key: &str) -> Option<u64> {
    match json.get(key)? {
        Value::Number(value) => value.as_u64(),
        Value::String(value) => value.parse::<u64>().ok(),
        _ => None,
    }
}

fn codec_field(json: &Value, key: &str) -> Option<String> {
    string_field(json, key).filter(|value| !value.eq_ignore_ascii_case("none"))
}

fn normalize_thumbnail_url(url: String) -> String {
    url.strip_prefix("http://")
        .map(|rest| format!("https://{rest}"))
        .unwrap_or(url)
}

fn build_format_options(json: &Value) -> Vec<FormatOption> {
    let mut options = vec![
        FormatOption {
            id: "best".to_string(),
            label: "最佳画质 + 最佳音频".to_string(),
            selector: "bv*+ba/b".to_string(),
            ext: Some("mp4".to_string()),
            resolution: Some("自动".to_string()),
            vcodec: None,
            acodec: None,
            filesize: None,
        },
        FormatOption {
            id: "best-mp4".to_string(),
            label: "优先 MP4".to_string(),
            selector: "bv*[ext=mp4]+ba[ext=m4a]/b[ext=mp4]/b".to_string(),
            ext: Some("mp4".to_string()),
            resolution: Some("自动".to_string()),
            vcodec: None,
            acodec: None,
            filesize: None,
        },
        FormatOption {
            id: "audio".to_string(),
            label: "仅音频".to_string(),
            selector: "ba".to_string(),
            ext: None,
            resolution: Some("audio".to_string()),
            vcodec: None,
            acodec: None,
            filesize: None,
        },
    ];

    if let Some(formats) = json.get("formats").and_then(Value::as_array) {
        let mut video_formats: Vec<&Value> = formats
            .iter()
            .filter(|format| has_real_video(format))
            .collect();

        video_formats.sort_by(|left, right| {
            let left_score = format_score(left);
            let right_score = format_score(right);
            right_score.cmp(&left_score)
        });

        options.extend(
            video_formats
                .into_iter()
                .filter_map(format_from_json)
                .take(12),
        );
    }

    options
}

fn best_format_label_from_json(json: &Value, formats: &[FormatOption]) -> Option<String> {
    if let Some(label) = json
        .get("requested_formats")
        .and_then(Value::as_array)
        .and_then(|requested| {
            requested
                .iter()
                .find(|format| has_real_video(format))
                .and_then(format_label_from_json)
        })
    {
        return Some(label);
    }

    if has_real_video(json) {
        if let Some(label) = format_label_from_json(json) {
            return Some(label);
        }
    }

    if let Some(label) = json
        .get("formats")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .filter(|format| has_real_video(format))
                .max_by_key(|format| format_score(format))
                .and_then(format_label_from_json)
        })
    {
        return Some(label);
    }

    best_format_label(formats)
}

fn best_format_label(formats: &[FormatOption]) -> Option<String> {
    formats
        .iter()
        .find(|format| {
            !matches!(format.id.as_str(), "best" | "best-mp4" | "audio")
                && format.resolution.as_deref() != Some("audio")
        })
        .map(|format| format.label.clone())
}

fn format_from_json(format: &Value) -> Option<FormatOption> {
    let id = string_field(format, "format_id")?;
    let ext = string_field(format, "ext");
    let resolution = actual_resolution_label(format);
    let vcodec = string_field(format, "vcodec");
    let acodec = string_field(format, "acodec");
    let filesize = format
        .get("filesize")
        .or_else(|| format.get("filesize_approx"))
        .and_then(Value::as_u64);

    let display_resolution = resolution
        .clone()
        .unwrap_or_else(|| "未返回明确分辨率".to_string());
    let label = [
        Some(display_resolution),
        ext.clone(),
        display_video_codec(vcodec.as_deref()),
    ]
    .into_iter()
    .flatten()
    .filter(|value| !value.eq_ignore_ascii_case("none"))
    .collect::<Vec<_>>()
    .join(" · ");

    Some(FormatOption {
        selector: format!("{id}+ba/b"),
        id,
        label: if label.is_empty() {
            "指定视频流 + 最佳音频".to_string()
        } else {
            label
        },
        ext,
        resolution,
        vcodec,
        acodec,
        filesize,
    })
}

fn format_label_from_json(format: &Value) -> Option<String> {
    let display_resolution =
        actual_resolution_label(format).unwrap_or_else(|| "未返回明确分辨率".to_string());
    let vcodec = codec_field(format, "vcodec");
    let parts = [
        Some(display_resolution),
        string_field(format, "ext"),
        display_video_codec(vcodec.as_deref()),
    ]
    .into_iter()
    .flatten()
    .filter(|value| !value.trim().is_empty())
    .collect::<Vec<_>>();

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

fn display_video_codec(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("none") {
        return None;
    }

    let lower = value.to_ascii_lowercase();
    if lower.starts_with("av01") {
        return Some("AV1".to_string());
    }
    if lower.starts_with("avc1") || lower.starts_with("avc3") || lower == "h264" {
        return Some("H.264".to_string());
    }
    if lower.starts_with("hev1") || lower.starts_with("hvc1") || lower == "hevc" || lower == "h265"
    {
        return Some("H.265".to_string());
    }
    if lower.starts_with("vp09") || lower == "vp9" {
        return Some("VP9".to_string());
    }
    if lower.starts_with("vp08") || lower == "vp8" {
        return Some("VP8".to_string());
    }

    if value.contains('.') {
        if let Some(prefix) = value.split('.').next().filter(|prefix| !prefix.is_empty()) {
            return Some(prefix.to_ascii_uppercase());
        }
    }

    Some(value.to_string())
}

fn has_real_video(format: &Value) -> bool {
    codec_field(format, "vcodec").is_some()
}

fn actual_resolution_label(format: &Value) -> Option<String> {
    if let Some((width, height)) = u64_field(format, "width").zip(u64_field(format, "height")) {
        if width > 0 && height > 0 {
            return Some(format!("{width}x{height}"));
        }
    }

    string_field(format, "resolution").and_then(|value| {
        parse_dimension_score(&value).map(|(width, height, _)| format!("{width}x{height}"))
    })
}

fn format_score(format: &Value) -> u64 {
    if let Some((width, height)) = u64_field(format, "width").zip(u64_field(format, "height")) {
        if width > 0 && height > 0 {
            return width.saturating_mul(height);
        }
    }

    [
        string_field(format, "resolution"),
        string_field(format, "format_note"),
        u64_field(format, "height").map(|height| format!("{height}p")),
    ]
    .into_iter()
    .flatten()
    .filter_map(|value| resolution_score_from_text(&value))
    .max()
    .unwrap_or(0)
}

fn resolution_score_from_text(value: &str) -> Option<u64> {
    if let Some((_, _, score)) = parse_dimension_score(value) {
        return Some(score);
    }

    if let Some(height) = parse_progressive_height(value) {
        let width = height.saturating_mul(16) / 9;
        return Some(width.saturating_mul(height));
    }

    let text = value.to_ascii_lowercase();
    if contains_quality_token(&text, "8k") {
        return Some(7680 * 4320);
    }
    if contains_quality_token(&text, "4k") || text.contains("uhd") {
        return Some(3840 * 2160);
    }
    if contains_quality_token(&text, "2k") || text.contains("qhd") {
        return Some(2560 * 1440);
    }

    None
}

fn parse_dimension_score(value: &str) -> Option<(u64, u64, u64)> {
    let chars = value.chars().collect::<Vec<_>>();
    for (index, character) in chars.iter().enumerate() {
        if !matches!(character, 'x' | 'X' | '×') {
            continue;
        }

        let mut left_end = index;
        while left_end > 0 && chars[left_end - 1].is_whitespace() {
            left_end -= 1;
        }
        let mut left_start = left_end;
        while left_start > 0 && chars[left_start - 1].is_ascii_digit() {
            left_start -= 1;
        }

        let mut right_start = index + 1;
        while right_start < chars.len() && chars[right_start].is_whitespace() {
            right_start += 1;
        }
        let mut right_end = right_start;
        while right_end < chars.len() && chars[right_end].is_ascii_digit() {
            right_end += 1;
        }

        if left_start == left_end || right_start == right_end {
            continue;
        }

        let width = chars[left_start..left_end]
            .iter()
            .collect::<String>()
            .parse::<u64>()
            .ok()?;
        let height = chars[right_start..right_end]
            .iter()
            .collect::<String>()
            .parse::<u64>()
            .ok()?;

        if width > 0 && height > 0 {
            return Some((width, height, width.saturating_mul(height)));
        }
    }

    None
}

fn parse_progressive_height(value: &str) -> Option<u64> {
    let chars = value.chars().collect::<Vec<_>>();
    for (index, character) in chars.iter().enumerate() {
        if !matches!(character, 'p' | 'P') {
            continue;
        }

        let mut end = index;
        while end > 0 && chars[end - 1].is_whitespace() {
            end -= 1;
        }
        let mut start = end;
        while start > 0 && chars[start - 1].is_ascii_digit() {
            start -= 1;
        }

        if start == end {
            continue;
        }

        let height = chars[start..end]
            .iter()
            .collect::<String>()
            .parse::<u64>()
            .ok()?;
        if height > 0 {
            return Some(height);
        }
    }

    None
}

fn contains_quality_token(text: &str, token: &str) -> bool {
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .any(|part| part == token)
}

fn update_progress_snapshot(
    progress: &Arc<Mutex<ProgressSnapshot>>,
    parsed: &ParsedProgress,
) -> ProgressSnapshot {
    let mapped = map_download_progress(parsed.progress, parsed.media_kind);
    let phase = progress_phase(parsed.media_kind);

    progress
        .lock()
        .map(|mut snapshot| {
            let should_update_phase =
                mapped >= snapshot.progress || phase_rank(phase) > phase_rank(snapshot.phase);
            snapshot.progress = snapshot.progress.max(mapped).min(99.0);
            if should_update_phase {
                snapshot.phase = phase;
            }
            *snapshot
        })
        .unwrap_or(ProgressSnapshot {
            progress: mapped,
            phase,
        })
}

fn update_merge_snapshot(progress: &Arc<Mutex<ProgressSnapshot>>) -> ProgressSnapshot {
    progress
        .lock()
        .map(|mut snapshot| {
            snapshot.progress = snapshot.progress.max(99.0);
            snapshot.phase = DownloadPhase::Merging;
            *snapshot
        })
        .unwrap_or(ProgressSnapshot {
            progress: 99.0,
            phase: DownloadPhase::Merging,
        })
}

fn map_download_progress(progress: f64, media_kind: ProgressMediaKind) -> f64 {
    let progress = progress.clamp(0.0, 100.0);

    match media_kind {
        ProgressMediaKind::Video => progress * 0.5,
        ProgressMediaKind::Audio => 50.0 + progress * 0.49,
        ProgressMediaKind::Media | ProgressMediaKind::Unknown => progress * 0.99,
    }
}

fn progress_phase(media_kind: ProgressMediaKind) -> DownloadPhase {
    match media_kind {
        ProgressMediaKind::Video => DownloadPhase::DownloadingVideo,
        ProgressMediaKind::Audio => DownloadPhase::DownloadingAudio,
        ProgressMediaKind::Media | ProgressMediaKind::Unknown => DownloadPhase::DownloadingMedia,
    }
}

fn phase_rank(phase: DownloadPhase) -> u8 {
    match phase {
        DownloadPhase::DownloadingMedia => 0,
        DownloadPhase::DownloadingVideo => 1,
        DownloadPhase::DownloadingAudio => 2,
        DownloadPhase::Merging => 3,
        DownloadPhase::Completed => 4,
    }
}

fn is_merge_progress_line(line: &str) -> bool {
    let line = strip_ansi_codes(line);
    let line = line.trim();

    line.starts_with("VD_POSTPROCESS:")
        || line.starts_with("[Merger]")
        || line.contains("Merging formats into")
}

fn parse_progress_line(line: &str) -> Option<ParsedProgress> {
    let line = strip_ansi_codes(line);

    if let Some(progress_line) = line.trim().strip_prefix("VD_PROGRESS:") {
        return parse_machine_progress_line(progress_line);
    }

    if !line.contains("[download]") || !line.contains('%') {
        return None;
    }

    let percent_index = line.find('%')?;
    let before_percent = &line[..percent_index];
    let number_start = before_percent
        .rfind(|character: char| !(character.is_ascii_digit() || character == '.'))
        .map(|index| index + 1)
        .unwrap_or(0);
    let progress = before_percent[number_start..].parse::<f64>().ok()?;

    let speed =
        extract_after(&line, " at ", " ETA ").and_then(|value| normalize_speed_value(&value));
    let eta = line.split(" ETA ").nth(1).and_then(normalize_eta_value);

    Some(ParsedProgress {
        progress,
        speed,
        eta,
        media_kind: ProgressMediaKind::Unknown,
    })
}

fn parse_machine_progress_line(line: &str) -> Option<ParsedProgress> {
    let mut fields = line.split('|');
    let progress = fields.next().and_then(parse_percent_value)?;
    let speed = fields.next().and_then(normalize_speed_value);
    let eta = fields.next().and_then(normalize_eta_value);
    let vcodec = fields.next().and_then(normalize_progress_value);
    let acodec = fields.next().and_then(normalize_progress_value);
    let _format_id = fields.next().and_then(normalize_progress_value);

    Some(ParsedProgress {
        progress,
        speed,
        eta,
        media_kind: progress_media_kind(vcodec.as_deref(), acodec.as_deref()),
    })
}

fn progress_media_kind(vcodec: Option<&str>, acodec: Option<&str>) -> ProgressMediaKind {
    let has_video = vcodec.is_some_and(is_real_codec);
    let has_audio = acodec.is_some_and(is_real_codec);

    match (has_video, has_audio) {
        (true, false) => ProgressMediaKind::Video,
        (false, true) => ProgressMediaKind::Audio,
        (true, true) => ProgressMediaKind::Media,
        (false, false) => ProgressMediaKind::Unknown,
    }
}

fn is_real_codec(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();

    !value.is_empty()
        && value != "none"
        && value != "null"
        && value != "unknown"
        && value != "n/a"
        && value != "na"
}

fn parse_percent_value(value: &str) -> Option<f64> {
    let value = value.trim().trim_end_matches('%').trim();
    if value.is_empty() {
        return None;
    }

    value.parse::<f64>().ok()
}

fn normalize_eta_value(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value.split_whitespace().next().unwrap_or(value);
    normalize_progress_value(value)
}

fn normalize_speed_value(value: &str) -> Option<String> {
    normalize_progress_value(value).map(|value| format_speed_value(&value))
}

fn format_speed_value(value: &str) -> String {
    let normalized = value.trim();
    let lower_value = normalized.to_ascii_lowercase();
    let units = [
        ("gib/s", 1024_f64 * 1024_f64 * 1024_f64),
        ("mib/s", 1024_f64 * 1024_f64),
        ("kib/s", 1024_f64),
        ("gb/s", 1_000_000_000_f64),
        ("mb/s", 1_000_000_f64),
        ("kb/s", 1_000_f64),
        ("b/s", 1_f64),
    ];

    for (unit, multiplier) in units {
        if let Some(number) = lower_value.strip_suffix(unit) {
            if let Ok(speed) = number.trim().parse::<f64>() {
                return format_decimal_speed(speed * multiplier);
            }
        }
    }

    normalized.to_string()
}

fn format_decimal_speed(bytes_per_second: f64) -> String {
    if !bytes_per_second.is_finite() || bytes_per_second <= 0.0 {
        return "0B/s".to_string();
    }

    if bytes_per_second >= 1_000_000.0 {
        return format_speed_number(bytes_per_second / 1_000_000.0, "MB/s");
    }

    if bytes_per_second >= 1_000.0 {
        return format_speed_number(bytes_per_second / 1_000.0, "KB/s");
    }

    format!("{:.0}B/s", bytes_per_second.round())
}

fn format_speed_number(value: f64, unit: &str) -> String {
    let text = if value >= 100.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };
    format!("{}{}", text.trim_end_matches(".0"), unit)
}

fn normalize_progress_value(value: &str) -> Option<String> {
    let value = value.trim();
    let lower_value = value.to_ascii_lowercase();
    if value.is_empty()
        || lower_value == "n/a"
        || lower_value == "na"
        || lower_value == "unknown"
        || lower_value.starts_with("unknown ")
        || value == "-"
    {
        None
    } else {
        Some(value.to_string())
    }
}

fn strip_ansi_codes(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();

    while let Some(character) = chars.next() {
        if character == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
            continue;
        }

        result.push(character);
    }

    result
}

fn extract_after(line: &str, marker: &str, until: &str) -> Option<String> {
    let start = line.find(marker)? + marker.len();
    let remainder = &line[start..];
    let end = remainder.find(until).unwrap_or(remainder.len());
    let value = remainder[..end].trim();

    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_machine_progress_line() {
        let parsed =
            parse_progress_line("VD_PROGRESS: 12.3%|8.4MiB/s|00:42|h264|none|137").unwrap();

        assert_eq!(parsed.progress, 12.3);
        assert_eq!(parsed.speed.as_deref(), Some("8.8MB/s"));
        assert_eq!(parsed.eta.as_deref(), Some("00:42"));
        assert_eq!(parsed.media_kind, ProgressMediaKind::Video);

        let parsed =
            parse_progress_line("VD_PROGRESS: 40%|Unknown B/s|Unknown ETA|none|opus|251").unwrap();

        assert_eq!(parsed.progress, 40.0);
        assert_eq!(parsed.speed, None);
        assert_eq!(parsed.eta, None);
        assert_eq!(parsed.media_kind, ProgressMediaKind::Audio);
    }

    #[test]
    fn parses_fragment_download_progress_line() {
        let parsed = parse_progress_line(
            "[download]  35.2% of ~120.00MiB at 3.1MiB/s ETA 00:25 (frag 20/80)",
        )
        .unwrap();

        assert_eq!(parsed.progress, 35.2);
        assert_eq!(parsed.speed.as_deref(), Some("3.3MB/s"));
        assert_eq!(parsed.eta.as_deref(), Some("00:25"));
        assert_eq!(parsed.media_kind, ProgressMediaKind::Unknown);
    }

    #[test]
    fn parses_completed_download_progress_line() {
        let parsed =
            parse_progress_line("[download] 100% of 120.00MiB in 00:30 at 4.0MiB/s").unwrap();

        assert_eq!(parsed.progress, 100.0);
        assert_eq!(parsed.speed.as_deref(), Some("4.2MB/s"));
        assert_eq!(parsed.eta, None);
    }

    #[test]
    fn ignores_non_progress_lines() {
        assert!(parse_progress_line("[info] Extracting URL").is_none());
        assert!(parse_progress_line("VD_PROGRESS: Unknown|N/A|Unknown").is_none());
    }

    #[test]
    fn maps_split_stream_progress_to_single_total_progress() {
        assert_eq!(map_download_progress(0.0, ProgressMediaKind::Video), 0.0);
        assert_eq!(map_download_progress(50.0, ProgressMediaKind::Video), 25.0);
        assert_eq!(map_download_progress(100.0, ProgressMediaKind::Video), 50.0);

        assert_eq!(map_download_progress(0.0, ProgressMediaKind::Audio), 50.0);
        assert_eq!(map_download_progress(50.0, ProgressMediaKind::Audio), 74.5);
        assert_eq!(map_download_progress(100.0, ProgressMediaKind::Audio), 99.0);
    }

    #[test]
    fn merge_lines_hold_progress_at_ninety_nine() {
        assert!(is_merge_progress_line(
            "[Merger] Merging formats into \"video.mp4\""
        ));
        assert!(is_merge_progress_line("VD_POSTPROCESS:started"));

        let progress = Arc::new(Mutex::new(ProgressSnapshot {
            progress: 74.5,
            phase: DownloadPhase::DownloadingAudio,
        }));
        let snapshot = update_merge_snapshot(&progress);

        assert_eq!(snapshot.progress, 99.0);
        assert_eq!(snapshot.phase, DownloadPhase::Merging);
    }

    #[test]
    fn progress_snapshot_never_regresses_between_streams() {
        let progress = Arc::new(Mutex::new(ProgressSnapshot::default()));
        let video = ParsedProgress {
            progress: 100.0,
            speed: None,
            eta: None,
            media_kind: ProgressMediaKind::Video,
        };
        let audio_start = ParsedProgress {
            progress: 0.0,
            speed: None,
            eta: None,
            media_kind: ProgressMediaKind::Audio,
        };

        assert_eq!(update_progress_snapshot(&progress, &video).progress, 50.0);
        let snapshot = update_progress_snapshot(&progress, &audio_start);

        assert_eq!(snapshot.progress, 50.0);
        assert_eq!(snapshot.phase, DownloadPhase::DownloadingAudio);

        let stale_video = ParsedProgress {
            progress: 80.0,
            speed: None,
            eta: None,
            media_kind: ProgressMediaKind::Video,
        };
        let snapshot = update_progress_snapshot(&progress, &stale_video);

        assert_eq!(snapshot.progress, 50.0);
        assert_eq!(snapshot.phase, DownloadPhase::DownloadingAudio);
    }

    #[test]
    fn best_format_label_uses_first_real_video_format() {
        let formats = vec![
            FormatOption {
                id: "best".to_string(),
                label: "最佳画质 + 最佳音频".to_string(),
                selector: "bv*+ba/b".to_string(),
                ext: Some("mp4".to_string()),
                resolution: Some("自动".to_string()),
                vcodec: None,
                acodec: None,
                filesize: None,
            },
            FormatOption {
                id: "audio".to_string(),
                label: "仅音频".to_string(),
                selector: "ba".to_string(),
                ext: None,
                resolution: Some("audio".to_string()),
                vcodec: None,
                acodec: None,
                filesize: None,
            },
            FormatOption {
                id: "30121".to_string(),
                label: "3840x1634 · mp4 · H.265".to_string(),
                selector: "30121+bestaudio/best".to_string(),
                ext: Some("mp4".to_string()),
                resolution: Some("3840x1634".to_string()),
                vcodec: Some("hev1.1.6.L153".to_string()),
                acodec: Some("none".to_string()),
                filesize: Some(2_400_000_000),
            },
        ];

        assert_eq!(
            best_format_label(&formats).as_deref(),
            Some("3840x1634 · mp4 · H.265")
        );
    }

    #[test]
    fn format_labels_use_short_video_codec_names() {
        let cases = [
            ("av01.0.00M.10.0.110.01.01.01.0", "AV1"),
            ("avc1.640034", "H.264"),
            ("h264", "H.264"),
            ("hev1.1.6.L153", "H.265"),
            ("hvc1.2.4.L150", "H.265"),
            ("vp09.00.51.08", "VP9"),
            ("vp08.00.10.08", "VP8"),
            ("mystery.codec.profile", "MYSTERY"),
            ("theora", "theora"),
        ];

        for (vcodec, expected_codec) in cases {
            let format = serde_json::json!({
                "format_id": "test",
                "width": 1920,
                "height": 1080,
                "ext": "mp4",
                "vcodec": vcodec,
                "acodec": "none"
            });
            let expected_label = format!("1920x1080 · mp4 · {expected_codec}");

            assert_eq!(
                format_label_from_json(&format).as_deref(),
                Some(expected_label.as_str())
            );
        }
    }

    #[test]
    fn format_option_keeps_raw_codec_but_displays_short_name() {
        let format = serde_json::json!({
            "format_id": "30121",
            "width": 3840,
            "height": 1634,
            "ext": "mp4",
            "vcodec": "av01.0.00M.10.0.110.01.01.01.0",
            "acodec": "none"
        });

        let option = format_from_json(&format).expect("format option");

        assert_eq!(
            option.vcodec.as_deref(),
            Some("av01.0.00M.10.0.110.01.01.01.0")
        );
        assert_eq!(option.label, "3840x1634 · mp4 · AV1");
    }

    #[test]
    fn format_label_prefers_raw_width_and_height() {
        let format = serde_json::json!({
            "format_id": "30121",
            "width": 3840,
            "height": 2160,
            "format_note": "4K",
            "ext": "mp4",
            "vcodec": "avc1.640034",
            "acodec": "none"
        });

        assert_eq!(
            actual_resolution_label(&format).as_deref(),
            Some("3840x2160")
        );
        assert_eq!(format_score(&format), 3840 * 2160);
        assert_eq!(
            format_label_from_json(&format).as_deref(),
            Some("3840x2160 · mp4 · H.264")
        );
    }

    #[test]
    fn fallback_resolution_scores_do_not_become_display_labels() {
        let format = serde_json::json!({
            "format_id": "137",
            "height": 2160,
            "format_note": "4K",
            "ext": "mp4",
            "vcodec": "h264",
            "acodec": "none"
        });

        assert_eq!(actual_resolution_label(&format), None);
        assert_eq!(format_score(&format), 3840 * 2160);
        assert_eq!(
            format_label_from_json(&format).as_deref(),
            Some("未返回明确分辨率 · mp4 · H.264")
        );
    }

    #[test]
    fn best_format_label_uses_requested_video_format() {
        let json = serde_json::json!({
            "requested_formats": [
                {
                    "format_id": "30121",
                    "width": 3840,
                    "height": 2160,
                    "format_note": "4K",
                    "ext": "mp4",
                    "vcodec": "avc1.640034",
                    "acodec": "none"
                },
                {
                    "format_id": "30280",
                    "ext": "m4a",
                    "vcodec": "none",
                    "acodec": "mp4a.40.2"
                }
            ],
            "formats": [
                {
                    "format_id": "low",
                    "width": 1280,
                    "height": 720,
                    "ext": "mp4",
                    "vcodec": "h264",
                    "acodec": "none"
                }
            ]
        });
        let formats = build_format_options(&json);

        assert_eq!(
            best_format_label_from_json(&json, &formats).as_deref(),
            Some("3840x2160 · mp4 · H.264")
        );
    }

    #[test]
    fn parses_macos_proxy_output_by_priority() {
        let output = r#"
<dictionary> {
  HTTPEnable : 1
  HTTPPort : 7890
  HTTPProxy : 127.0.0.1
  HTTPSEnable : 1
  HTTPSPort : 7891
  HTTPSProxy : 127.0.0.2
  SOCKSEnable : 1
  SOCKSPort : 7892
  SOCKSProxy : 127.0.0.3
}
"#;

        let status = proxy_status_from_scutil_output(output);

        assert_eq!(status.mode, "auto");
        assert_eq!(
            status.effective_proxy.as_deref(),
            Some("http://127.0.0.2:7891")
        );
        assert_eq!(status.source.as_deref(), Some("systemHttps"));
    }

    #[test]
    fn parses_macos_socks_proxy_when_http_is_absent() {
        let output = r#"
<dictionary> {
  SOCKSEnable : 1
  SOCKSPort : 1080
  SOCKSProxy : 127.0.0.1
}
"#;

        let status = proxy_status_from_scutil_output(output);

        assert_eq!(
            status.effective_proxy.as_deref(),
            Some("socks5h://127.0.0.1:1080")
        );
        assert_eq!(status.source.as_deref(), Some("systemSocks"));
    }

    #[test]
    fn reports_pac_as_unsupported_without_proxy() {
        let output = r#"
<dictionary> {
  ProxyAutoConfigEnable : 1
  ProxyAutoConfigURLString : http://example.test/proxy.pac
}
"#;

        let status = proxy_status_from_scutil_output(output);

        assert_eq!(status.effective_proxy, None);
        assert_eq!(status.source.as_deref(), Some("pacUnsupported"));
    }

    #[test]
    fn parses_ffprobe_media_info_from_format_and_streams() {
        let json = serde_json::json!({
            "streams": [
                {
                    "codec_type": "video",
                    "codec_name": "h264",
                    "width": 1920,
                    "height": 1080,
                    "duration": "480.1"
                },
                {
                    "codec_type": "audio",
                    "codec_name": "aac",
                    "duration": "481.2"
                }
            ],
            "format": {
                "duration": "481.2"
            }
        });

        let media = parse_local_media_info(&json, Some("123".to_string()));

        assert_eq!(media.duration, Some(481.2));
        assert_eq!(media.width, Some(1920));
        assert_eq!(media.height, Some(1080));
        assert_eq!(media.video_codec.as_deref(), Some("h264"));
        assert_eq!(media.audio_codec.as_deref(), Some("aac"));
        assert_eq!(media.probed_at.as_deref(), Some("123"));
        assert_eq!(media.error, None);
    }

    #[test]
    fn parses_ffprobe_duration_from_streams_when_format_duration_is_missing() {
        let json = serde_json::json!({
            "streams": [
                {
                    "codec_type": "video",
                    "codec_name": "hevc",
                    "width": "1280",
                    "height": "720",
                    "duration": "300.5"
                },
                {
                    "codec_type": "audio",
                    "codec_name": "opus",
                    "duration": "302.0"
                }
            ]
        });

        let media = parse_local_media_info(&json, None);

        assert_eq!(media.duration, Some(302.0));
        assert_eq!(media.width, Some(1280));
        assert_eq!(media.height, Some(720));
        assert_eq!(media.video_codec.as_deref(), Some("hevc"));
        assert_eq!(media.audio_codec.as_deref(), Some("opus"));
    }

    #[test]
    fn compare_media_omits_matching_expected_values() {
        let expected = ExpectedMediaInfo {
            duration: Some(480.0),
            resolution_label: Some("1080p".to_string()),
            resolution_score: Some(1920 * 1080),
        };
        let actual = LocalMediaInfo {
            duration: Some(482.0),
            width: Some(1920),
            height: Some(1080),
            ..Default::default()
        };

        assert!(compare_media(Some(&expected), &actual).is_none());
    }

    #[test]
    fn compare_media_reports_shorter_duration_and_lower_resolution() {
        let expected = ExpectedMediaInfo {
            duration: Some(600.0),
            resolution_label: Some("1080p".to_string()),
            resolution_score: Some(1920 * 1080),
        };
        let actual = LocalMediaInfo {
            duration: Some(500.0),
            width: Some(1280),
            height: Some(720),
            ..Default::default()
        };

        let comparison = compare_media(Some(&expected), &actual).unwrap();

        assert_eq!(
            comparison
                .duration
                .as_ref()
                .map(|detail| detail.status.as_str()),
            Some("shorter")
        );
        assert_eq!(
            comparison
                .resolution
                .as_ref()
                .map(|detail| detail.status.as_str()),
            Some("lower")
        );
        assert_eq!(
            comparison
                .resolution
                .as_ref()
                .and_then(|detail| detail.expected_label.as_deref()),
            Some("1080p")
        );
        assert_eq!(
            comparison
                .resolution
                .as_ref()
                .and_then(|detail| detail.actual_label.as_deref()),
            Some("1280×720")
        );
    }
}

fn app_data_dir() -> Result<PathBuf, String> {
    let base_dir =
        dirs::data_local_dir().ok_or_else(|| "无法定位本机应用数据目录。".to_string())?;
    Ok(base_dir.join("VideoDownloader"))
}

fn history_path() -> Result<PathBuf, String> {
    Ok(app_data_dir()?.join("history.json"))
}

fn ffmpeg_command_history_path() -> Result<PathBuf, String> {
    Ok(app_data_dir()?.join("ffmpeg-command-history.json"))
}

fn tool_settings_path() -> Result<PathBuf, String> {
    Ok(app_data_dir()?.join("tool-settings.json"))
}

fn download_cache_root() -> Result<PathBuf, String> {
    Ok(app_data_dir()?.join(DOWNLOAD_CACHE_DIR_NAME))
}

fn download_task_cache_dir(task_id: &str) -> Result<PathBuf, String> {
    Ok(download_cache_root()?.join(sanitize_cache_component(task_id)))
}

fn sanitize_cache_component(value: &str) -> String {
    let text: String = value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || *character == '-' || *character == '_'
        })
        .collect();

    if text.is_empty() {
        "unknown-task".to_string()
    } else {
        text
    }
}

fn remove_owned_download_cache_dir(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }

    let root = download_cache_root()?;
    ensure_owned_cache_child(&root, path)?;
    fs::remove_dir_all(path)
        .map_err(|error| format!("删除下载缓存失败 {}：{error}", path.display()))
}

fn ensure_owned_cache_child(root: &Path, path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("读取缓存路径失败 {}：{error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("跳过符号链接缓存路径：{}", path.display()));
    }

    let root = root
        .canonicalize()
        .map_err(|error| format!("定位缓存根目录失败 {}：{error}", root.display()))?;
    let path = path
        .canonicalize()
        .map_err(|error| format!("定位缓存路径失败 {}：{error}", path.display()))?;

    if path == root || !path.starts_with(&root) {
        return Err(format!("拒绝清理非应用缓存路径：{}", path.display()));
    }

    Ok(())
}

fn build_download_cleanup_plan(state: &AppState) -> Result<DownloadCleanupPlan, String> {
    let active_ids = active_download_task_ids(state)?;
    let mut plan = DownloadCleanupPlan::default();
    let root = download_cache_root()?;

    if root.exists() {
        let entries = fs::read_dir(&root)
            .map_err(|error| format!("读取下载缓存目录失败 {}：{error}", root.display()))?;

        for entry in entries {
            let entry = entry.map_err(|error| format!("读取下载缓存条目失败：{error}"))?;
            let path = entry.path();
            let task_id = entry.file_name().to_string_lossy().to_string();

            if active_ids.contains(&task_id) {
                plan.summary.skipped_active_tasks += 1;
                continue;
            }

            if ensure_owned_cache_child(&root, &path).is_err() {
                continue;
            }

            let stats = path_stats(&path)?;
            plan.summary.file_count += stats.file_count;
            plan.summary.directory_count += stats.directory_count.max(1);
            plan.summary.bytes += stats.bytes;
            plan.cache_dirs.push(path);
        }
    }

    let history = read_history(state)?;
    for item in &history {
        if is_invalid_history_item(item, &active_ids) {
            plan.invalid_history_ids.insert(item.id.clone());
        }
    }
    plan.summary.invalid_history_count = plan.invalid_history_ids.len() as u64;

    Ok(plan)
}

fn active_download_task_ids(state: &AppState) -> Result<HashSet<String>, String> {
    let tasks = state
        .tasks
        .lock()
        .map_err(|_| "下载任务状态锁已损坏。".to_string())?;
    let mut ids: HashSet<String> = tasks.keys().cloned().collect();
    drop(tasks);

    let paused_tasks = state
        .paused_tasks
        .lock()
        .map_err(|_| "暂停任务状态锁已损坏。".to_string())?;
    ids.extend(paused_tasks.iter().cloned());

    Ok(ids)
}

#[derive(Default)]
struct PathStats {
    file_count: u64,
    directory_count: u64,
    bytes: u64,
}

fn path_stats(path: &Path) -> Result<PathStats, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("读取缓存路径失败 {}：{error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Ok(PathStats::default());
    }
    if metadata.is_file() {
        return Ok(PathStats {
            file_count: 1,
            directory_count: 0,
            bytes: metadata.len(),
        });
    }
    if !metadata.is_dir() {
        return Ok(PathStats::default());
    }

    let mut stats = PathStats {
        directory_count: 1,
        ..Default::default()
    };
    let entries = fs::read_dir(path)
        .map_err(|error| format!("读取缓存目录失败 {}：{error}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("读取缓存条目失败：{error}"))?;
        let child_stats = path_stats(&entry.path())?;
        stats.file_count += child_stats.file_count;
        stats.directory_count += child_stats.directory_count;
        stats.bytes += child_stats.bytes;
    }

    Ok(stats)
}

fn is_invalid_history_item(item: &HistoryItem, active_ids: &HashSet<String>) -> bool {
    if active_ids.contains(&item.id) {
        return false;
    }

    match item.status.as_str() {
        "failed" | "canceled" => true,
        "running" | "paused" => history_is_stale(item),
        "completed" => item
            .output_path
            .as_deref()
            .map(|path| !Path::new(path).is_file())
            .unwrap_or(true),
        _ => false,
    }
}

fn history_is_stale(item: &HistoryItem) -> bool {
    let updated_at = item.updated_at.parse::<u64>().unwrap_or_default();
    let now = unix_timestamp().parse::<u64>().unwrap_or_default();
    now.saturating_sub(updated_at) >= STALE_ACTIVE_HISTORY_SECONDS
}

fn read_tool_settings(state: &AppState) -> Result<ToolSettings, String> {
    let _guard = state
        .tool_settings_lock
        .lock()
        .map_err(|_| "工具路径设置锁已损坏。".to_string())?;
    read_tool_settings_unlocked()
}

fn read_tool_settings_with_fallback(state: &AppState) -> ToolSettings {
    read_tool_settings(state).unwrap_or_default()
}

fn read_tool_settings_unlocked() -> Result<ToolSettings, String> {
    let path = tool_settings_path()?;
    if !path.exists() {
        return Ok(ToolSettings::default());
    }

    let bytes = fs::read(&path)
        .map_err(|error| format!("读取工具路径设置失败 {}：{error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析工具路径设置失败 {}：{error}", path.display()))
}

fn write_tool_settings_unlocked(settings: &ToolSettings) -> Result<(), String> {
    let path = tool_settings_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("创建工具路径设置目录失败 {}：{error}", parent.display()))?;
    }

    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("序列化工具路径设置失败：{error}"))?;
    fs::write(&path, bytes)
        .map_err(|error| format!("写入工具路径设置失败 {}：{error}", path.display()))
}

fn read_history(state: &AppState) -> Result<Vec<HistoryItem>, String> {
    let _guard = state
        .history_lock
        .lock()
        .map_err(|_| "历史记录锁已损坏。".to_string())?;
    read_history_unlocked()
}

fn read_history_unlocked() -> Result<Vec<HistoryItem>, String> {
    let path = history_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let bytes =
        fs::read(&path).map_err(|error| format!("读取历史记录失败 {}：{error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析历史记录失败 {}：{error}", path.display()))
}

fn write_history_unlocked(items: &[HistoryItem]) -> Result<(), String> {
    let path = history_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("创建历史记录目录失败 {}：{error}", parent.display()))?;
    }

    let bytes =
        serde_json::to_vec_pretty(items).map_err(|error| format!("序列化历史记录失败：{error}"))?;
    fs::write(&path, bytes).map_err(|error| format!("写入历史记录失败 {}：{error}", path.display()))
}

fn read_ffmpeg_command_history(state: &AppState) -> Result<Vec<FfmpegCommandHistoryItem>, String> {
    let _guard = state
        .ffmpeg_command_history_lock
        .lock()
        .map_err(|_| "FFmpeg 命令历史记录锁已损坏。".to_string())?;
    let mut items = read_ffmpeg_command_history_unlocked()?;
    sort_ffmpeg_command_history(&mut items);
    Ok(items)
}

fn read_ffmpeg_command_history_unlocked() -> Result<Vec<FfmpegCommandHistoryItem>, String> {
    let path = ffmpeg_command_history_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let bytes = fs::read(&path)
        .map_err(|error| format!("读取 FFmpeg 命令历史失败 {}：{error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("解析 FFmpeg 命令历史失败 {}：{error}", path.display()))
}

fn write_ffmpeg_command_history_unlocked(items: &[FfmpegCommandHistoryItem]) -> Result<(), String> {
    let path = ffmpeg_command_history_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!("创建 FFmpeg 命令历史目录失败 {}：{error}", parent.display())
        })?;
    }

    let bytes = serde_json::to_vec_pretty(items)
        .map_err(|error| format!("序列化 FFmpeg 命令历史失败：{error}"))?;
    write_file_atomic(&path, &bytes, "FFmpeg 命令历史")
}

fn append_ffmpeg_command_history_item(
    state: &AppState,
    input: FfmpegCommandHistoryInput,
) -> Result<Vec<FfmpegCommandHistoryItem>, String> {
    let _guard = state
        .ffmpeg_command_history_lock
        .lock()
        .map_err(|_| "FFmpeg 命令历史记录锁已损坏。".to_string())?;
    let mut items = read_ffmpeg_command_history_unlocked()?;
    let next_item = FfmpegCommandHistoryItem {
        id: uuid_like_id(),
        preset_id: input.preset_id,
        input_path: input.input_path,
        secondary_input_path: input.secondary_input_path,
        output_dir: input.output_dir,
        audio_format: input.audio_format,
        start_time: input.start_time,
        end_time: input.end_time,
        crf: input.crf,
        command: input.command,
        working_dir: input.working_dir,
        output_path: input.output_path,
        created_at: unix_timestamp(),
    };

    items.retain(|item| item.command != next_item.command);
    items.insert(0, next_item);
    sort_ffmpeg_command_history(&mut items);
    items.truncate(FFMPEG_COMMAND_HISTORY_LIMIT);
    write_ffmpeg_command_history_unlocked(&items)?;
    Ok(items)
}

fn delete_ffmpeg_command_history_by_ids(
    state: &AppState,
    ids: &[String],
) -> Result<Vec<FfmpegCommandHistoryItem>, String> {
    let _guard = state
        .ffmpeg_command_history_lock
        .lock()
        .map_err(|_| "FFmpeg 命令历史记录锁已损坏。".to_string())?;

    if ids.is_empty() {
        let mut items = read_ffmpeg_command_history_unlocked()?;
        sort_ffmpeg_command_history(&mut items);
        return Ok(items);
    }

    let selected: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut items = read_ffmpeg_command_history_unlocked()?;
    items.retain(|item| !selected.contains(item.id.as_str()));
    sort_ffmpeg_command_history(&mut items);
    write_ffmpeg_command_history_unlocked(&items)?;
    Ok(items)
}

fn clear_ffmpeg_command_history_items(state: &AppState) -> Result<(), String> {
    let _guard = state
        .ffmpeg_command_history_lock
        .lock()
        .map_err(|_| "FFmpeg 命令历史记录锁已损坏。".to_string())?;
    write_ffmpeg_command_history_unlocked(&[])
}

fn sort_ffmpeg_command_history(items: &mut [FfmpegCommandHistoryItem]) {
    items.sort_by(|left, right| right.created_at.cmp(&left.created_at));
}

fn write_file_atomic(path: &Path, bytes: &[u8], label: &str) -> Result<(), String> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("data.json");
    let temp_name = format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        ID_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let temp_path = path.with_file_name(temp_name);

    let write_result = (|| {
        let mut file = fs::File::create(&temp_path).map_err(|error| {
            format!("创建 {label} 临时文件失败 {}：{error}", temp_path.display())
        })?;
        file.write_all(bytes).map_err(|error| {
            format!("写入 {label} 临时文件失败 {}：{error}", temp_path.display())
        })?;
        file.sync_all().map_err(|error| {
            format!("同步 {label} 临时文件失败 {}：{error}", temp_path.display())
        })?;
        drop(file);
        fs::rename(&temp_path, path)
            .map_err(|error| format!("替换 {label} 文件失败 {}：{error}", path.display()))
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    write_result
}

fn delete_history_item_by_id(state: &AppState, id: &str, delete_file: bool) -> Result<(), String> {
    let _guard = state
        .history_lock
        .lock()
        .map_err(|_| "历史记录锁已损坏。".to_string())?;
    let mut items = read_history_unlocked()?;
    let Some(index) = items.iter().position(|item| item.id == id) else {
        return Ok(());
    };
    let item = items.remove(index);

    if delete_file {
        if let Some(output_path) = item.output_path.as_deref().filter(|path| !path.is_empty()) {
            let path = PathBuf::from(output_path);

            if path.exists() && path.is_file() {
                fs::remove_file(&path)
                    .map_err(|error| format!("删除本地文件失败 {}：{error}", path.display()))?;
            }
        }
    }

    write_history_unlocked(&items)
}

fn remove_history_items_by_ids(state: &AppState, ids: &HashSet<String>) -> Result<(), String> {
    let _guard = state
        .history_lock
        .lock()
        .map_err(|_| "历史记录锁已损坏。".to_string())?;
    let mut items = read_history_unlocked()?;
    items.retain(|item| !ids.contains(&item.id));
    write_history_unlocked(&items)
}

fn upsert_started_history(state: &AppState, item: HistoryItem) -> Result<(), String> {
    let _guard = state
        .history_lock
        .lock()
        .map_err(|_| "历史记录锁已损坏。".to_string())?;
    let mut items = read_history_unlocked()?;

    if let Some(existing) = items.iter_mut().find(|existing| existing.id == item.id) {
        let preserved_progress = if existing.status == "paused" {
            existing.progress
        } else {
            item.progress
        };
        existing.url = item.url;
        existing.title = item.title;
        existing.site = item.site;
        existing.format = item.format;
        existing.browser = item.browser;
        existing.output_dir = item.output_dir;
        existing.status = "running".to_string();
        existing.progress = preserved_progress;
        existing.output_path = None;
        existing.local_media = None;
        existing.media_comparison = None;
        existing.error = None;
        existing.updated_at = unix_timestamp();
    } else {
        items.insert(0, item);
    }

    write_history_unlocked(&items)
}

fn history_progress_for_task(state: &AppState, task_id: &str) -> Option<f64> {
    read_history(state)
        .ok()
        .and_then(|items| items.into_iter().find(|item| item.id == task_id))
        .map(|item| item.progress)
}

fn update_history_status(
    state: &AppState,
    task_id: &str,
    status: &str,
    progress: f64,
    output_path: Option<String>,
    local_media: Option<LocalMediaInfo>,
    media_comparison: Option<MediaComparison>,
    error: Option<String>,
) -> Result<(), String> {
    let _guard = state
        .history_lock
        .lock()
        .map_err(|_| "历史记录锁已损坏。".to_string())?;
    let mut items = read_history_unlocked()?;

    if let Some(existing) = items.iter_mut().find(|item| item.id == task_id) {
        existing.status = status.to_string();
        existing.progress = progress;
        existing.output_path = output_path.clone();
        existing.local_media = local_media;
        existing.media_comparison = media_comparison;
        existing.error = error;
        let title_is_replaceable = is_placeholder_text(&existing.title)
            || (output_path
                .as_deref()
                .and_then(title_from_output_path)
                .is_some()
                && is_opaque_id_text(&existing.title));
        if title_is_replaceable {
            existing.title = display_title(None, &existing.url, output_path.as_deref());
        }
        if is_placeholder_text(&existing.site) {
            existing.site = display_site(None, &existing.url);
        }
        existing.updated_at = unix_timestamp();
    }

    write_history_unlocked(&items)
}

fn unix_timestamp() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    seconds.to_string()
}

fn display_title(title: Option<&str>, url: &str, output_path: Option<&str>) -> String {
    title
        .and_then(clean_text)
        .filter(|value| !is_placeholder_text(value))
        .or_else(|| output_path.and_then(title_from_output_path))
        .unwrap_or_else(|| title_from_url(url))
}

fn display_site(site: Option<&str>, url: &str) -> String {
    site.and_then(clean_text)
        .filter(|value| !is_placeholder_text(value))
        .unwrap_or_else(|| site_from_url(url))
}

fn clean_text(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn is_placeholder_text(value: &str) -> bool {
    let value = value.trim();
    let normalized = value.to_ascii_lowercase();

    matches!(
        normalized.as_str(),
        "" | "undefined" | "null" | "unknown" | "untitled" | "untitled video"
    ) || is_bilibili_video_id(value)
}

fn is_bilibili_video_id(value: &str) -> bool {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();

    if lower.starts_with("av")
        && lower[2..]
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return lower.len() > 2;
    }

    lower.starts_with("bv")
        && lower.len() > 2
        && lower[2..]
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

fn is_opaque_id_text(value: &str) -> bool {
    let value = value.trim();
    let length = value.chars().count();

    (8..=24).contains(&length)
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
        && value.chars().any(|character| character.is_ascii_digit())
        && value
            .chars()
            .any(|character| character.is_ascii_uppercase())
}

fn title_from_output_path(output_path: &str) -> Option<String> {
    let file_stem = Path::new(output_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .and_then(clean_text)?;

    if let Some((title, _)) = file_stem.rsplit_once("-BV") {
        return clean_text(title);
    }

    Some(file_stem)
}

fn title_from_url(url: &str) -> String {
    url.rsplit('/')
        .find(|part| !part.trim().is_empty())
        .and_then(|part| part.split('?').next())
        .and_then(clean_text)
        .unwrap_or_else(|| "待解析视频".to_string())
}

fn site_from_url(url: &str) -> String {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let host = without_scheme.split('/').next().unwrap_or("").trim();
    let host = host.strip_prefix("www.").unwrap_or(host);

    if host.is_empty() {
        "未知站点".to_string()
    } else {
        host.to_string()
    }
}
