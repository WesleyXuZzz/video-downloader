use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    env,
    ffi::OsString,
    fs,
    io::{BufRead, BufReader, Write},
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
static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct AppState {
    tasks: Mutex<HashMap<String, TaskControl>>,
    history_lock: Mutex<()>,
    ffmpeg_command_history_lock: Mutex<()>,
    tool_settings_lock: Mutex<()>,
}

#[derive(Clone)]
struct TaskControl {
    child: Arc<Mutex<Child>>,
    canceled: Arc<AtomicBool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DependencyStatus {
    yt_dlp: ToolStatus,
    ffmpeg: ToolStatus,
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
    error: Option<String>,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    task_id: String,
    status: String,
    progress: f64,
    speed: Option<String>,
    eta: Option<String>,
    line: Option<String>,
    output_path: Option<String>,
    error: Option<String>,
}

#[tauri::command]
async fn check_dependencies(app: AppHandle) -> Result<DependencyStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let yt_dlp = tool_status(&state, "yt-dlp");
        let ffmpeg = tool_status(&state, "ffmpeg");
        let ready = yt_dlp.installed && ffmpeg.installed;

        DependencyStatus {
            yt_dlp,
            ffmpeg,
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
fn default_download_dir() -> Option<String> {
    dirs::download_dir().map(|path| path.to_string_lossy().to_string())
}

#[tauri::command]
async fn probe_url(
    app: AppHandle,
    url: String,
    browser: Option<String>,
) -> Result<ProbeResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        validate_url(&url)?;
        let yt_dlp_path = ensure_tool(&state, "yt-dlp")?;

        let mut command = Command::new(&yt_dlp_path);
        apply_tool_env(&state, &mut command);
        command
            .arg("--dump-single-json")
            .arg("--skip-download")
            .arg("--no-warnings")
            .arg("--no-playlist")
            .arg("--socket-timeout")
            .arg("30");

        if let Some(browser) = normalized_browser(browser) {
            command.arg("--cookies-from-browser").arg(browser);
        }

        command.arg(&url);
        let output = run_command_with_timeout(command, Duration::from_secs(45))?;

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

        Ok(ProbeResponse {
            title: string_field(&json, "title").unwrap_or_else(|| "Untitled video".to_string()),
            site: string_field(&json, "extractor_key")
                .or_else(|| string_field(&json, "extractor"))
                .unwrap_or_else(|| "Generic".to_string()),
            webpage_url: string_field(&json, "webpage_url").unwrap_or(url),
            duration: json.get("duration").and_then(Value::as_f64),
            thumbnail: thumbnail_url(&json),
            formats: build_format_options(&json),
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
) -> Result<Vec<BatchParseItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let yt_dlp_path = ensure_tool(&state, "yt-dlp")?;
        let browser = normalized_browser(browser);
        let mut items = Vec::new();

        let mut source_order = 0;

        for raw_url in urls {
            let url = raw_url.trim().to_string();
            if url.is_empty() {
                continue;
            }
            source_order += 1;

            match parse_queue_url(&state, &yt_dlp_path, &url, browser.as_deref(), source_order) {
                Ok(mut parsed) => items.append(&mut parsed),
                Err(error) => items.push(BatchParseItem {
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
                }),
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
        let yt_dlp_path = ensure_tool(&state, "yt-dlp")?;
        let ffmpeg_path = ensure_tool(&state, "ffmpeg")?;

        let output_dir = PathBuf::from(&request.output_dir);
        fs::create_dir_all(&output_dir)
            .map_err(|error| format!("无法创建保存目录 {}：{error}", output_dir.display()))?;

        let mut command = Command::new(&yt_dlp_path);
        apply_tool_env(&state, &mut command);
        command
            .arg("--newline")
            .arg("--no-playlist")
            .arg("--socket-timeout")
            .arg("30")
            .arg("-f")
            .arg(&request.format)
            .arg("--ffmpeg-location")
            .arg(&ffmpeg_path)
            .arg("--merge-output-format")
            .arg("mp4")
            .arg("-P")
            .arg(&request.output_dir)
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
        let task_id = request.task_id.clone();

        {
            let mut tasks = state
                .tasks
                .lock()
                .map_err(|_| "下载任务状态锁已损坏。".to_string())?;
            tasks.insert(
                task_id.clone(),
                TaskControl {
                    child: Arc::clone(&child),
                    canceled: Arc::clone(&canceled),
                },
            );
        }

        upsert_history(
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
        let shared_progress = Arc::new(Mutex::new(0.0));
        let progress_for_stdout = Arc::clone(&shared_progress);
        let progress_for_stderr = Arc::clone(&shared_progress);

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
                    if let Ok(mut progress) = progress_for_stderr.lock() {
                        *progress = parsed.0;
                    }

                    let _ = app_for_stderr.emit(
                        "download-progress",
                        ProgressEvent {
                            task_id: task_for_stderr.clone(),
                            status: "running".to_string(),
                            progress: parsed.0,
                            speed: parsed.1,
                            eta: parsed.2,
                            line: Some(line.trim().to_string()),
                            output_path: None,
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
                    progress = parsed.0;
                    if let Ok(mut shared) = progress_for_stdout.lock() {
                        *shared = progress;
                    }
                    let _ = app_for_stdout.emit(
                        "download-progress",
                        ProgressEvent {
                            task_id: task_for_stdout.clone(),
                            status: "running".to_string(),
                            progress,
                            speed: parsed.1,
                            eta: parsed.2,
                            line: Some(line),
                            output_path: output_path.clone(),
                            error: None,
                        },
                    );
                }
            }

            let exit_status = child.lock().ok().and_then(|mut child| child.wait().ok());
            let was_canceled = canceled.load(Ordering::SeqCst);
            let success = exit_status.map(|status| status.success()).unwrap_or(false);
            let final_status = if was_canceled {
                "canceled"
            } else if success {
                progress = 100.0;
                "completed"
            } else {
                progress = shared_progress
                    .lock()
                    .map(|progress| *progress)
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

            let _ = app_for_stdout.emit(
                "download-progress",
                ProgressEvent {
                    task_id: task_for_stdout.clone(),
                    status: final_status.to_string(),
                    progress,
                    speed: None,
                    eta: None,
                    line: None,
                    output_path: output_path.clone(),
                    error: error.clone(),
                },
            );

            let app_state = app_for_stdout.state::<AppState>();
            let _ = update_history_status(
                &app_state,
                &task_for_stdout,
                final_status,
                progress,
                output_path,
                error,
            );

            {
                if let Ok(mut tasks) = app_state.tasks.lock() {
                    tasks.remove(&task_for_stdout);
                };
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

        let Some(control) = control else {
            return Err("未找到正在运行的下载任务。".to_string());
        };

        control.canceled.store(true, Ordering::SeqCst);

        if let Ok(mut child) = control.child.lock() {
            child
                .kill()
                .map_err(|error| format!("取消下载失败：{error}"))?;
        }

        update_history_status(&state, &task_id, "canceled", 0.0, None, None)?;

        let _ = app.emit(
            "download-progress",
            ProgressEvent {
                task_id,
                status: "canceled".to_string(),
                progress: 0.0,
                speed: None,
                eta: None,
                line: None,
                output_path: None,
                error: None,
            },
        );

        Ok(())
    })
    .await
    .map_err(|error| format!("取消下载任务失败：{error}"))?
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
            default_download_dir,
            probe_url,
            parse_download_queue,
            start_download,
            cancel_download,
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
    yt_dlp_path: &Path,
    url: &str,
    browser: Option<&str>,
    source_order: usize,
) -> Result<Vec<BatchParseItem>, String> {
    validate_url(url)?;

    let mut command = Command::new(yt_dlp_path);
    apply_tool_env(state, &mut command);
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

    let output = run_command_with_timeout(command, Duration::from_secs(60))?;
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
            return Err(format!(
                "{label} 超过 {} 秒未返回，请检查网络或代理。",
                timeout.as_secs()
            ));
        }

        thread::sleep(Duration::from_millis(120));
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
        let mut video_formats: Vec<FormatOption> = formats
            .iter()
            .filter_map(format_from_json)
            .filter(|format| format.vcodec.as_deref() != Some("none"))
            .collect();

        video_formats.sort_by(|left, right| {
            let left_score = resolution_score(left.resolution.as_deref());
            let right_score = resolution_score(right.resolution.as_deref());
            right_score.cmp(&left_score)
        });

        options.extend(video_formats.into_iter().take(12));
    }

    options
}

fn format_from_json(format: &Value) -> Option<FormatOption> {
    let id = string_field(format, "format_id")?;
    let ext = string_field(format, "ext");
    let resolution = string_field(format, "resolution")
        .or_else(|| string_field(format, "format_note"))
        .or_else(|| {
            format
                .get("height")
                .and_then(Value::as_u64)
                .map(|height| format!("{height}p"))
        });
    let vcodec = string_field(format, "vcodec");
    let acodec = string_field(format, "acodec");
    let filesize = format
        .get("filesize")
        .or_else(|| format.get("filesize_approx"))
        .and_then(Value::as_u64);

    let label = [resolution.clone(), ext.clone(), vcodec.clone()]
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

fn resolution_score(resolution: Option<&str>) -> u64 {
    let Some(resolution) = resolution else {
        return 0;
    };

    let digits: String = resolution.chars().filter(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(0)
}

fn parse_progress_line(line: &str) -> Option<(f64, Option<String>, Option<String>)> {
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

    let speed = extract_after(line, " at ", " ETA ");
    let eta = line
        .split(" ETA ")
        .nth(1)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    Some((progress, speed, eta))
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
    fs::write(&path, bytes)
        .map_err(|error| format!("写入 FFmpeg 命令历史失败 {}：{error}", path.display()))
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
    items.insert(
        0,
        FfmpegCommandHistoryItem {
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
        },
    );
    sort_ffmpeg_command_history(&mut items);
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

    let selected: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
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

fn upsert_history(state: &AppState, item: HistoryItem) -> Result<(), String> {
    let _guard = state
        .history_lock
        .lock()
        .map_err(|_| "历史记录锁已损坏。".to_string())?;
    let mut items = read_history_unlocked()?;

    if let Some(existing) = items.iter_mut().find(|existing| existing.id == item.id) {
        *existing = item;
    } else {
        items.insert(0, item);
    }

    write_history_unlocked(&items)
}

fn update_history_status(
    state: &AppState,
    task_id: &str,
    status: &str,
    progress: f64,
    output_path: Option<String>,
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
