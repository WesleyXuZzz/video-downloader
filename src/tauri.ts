import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  BrowserKind,
  BatchParseItem,
  DependencyStatus,
  DownloadCleanupSummary,
  DownloadHistoryItem,
  DownloadRequest,
  FfmpegCommandDraft,
  FfmpegCommandHistoryInput,
  FfmpegCommandHistoryItem,
  FfmpegCommandRequest,
  ProgressEvent,
  ProxyMode,
  ProxyStatus,
  ProbeResponse,
  SupportedSitesResponse,
  TerminalPrefillResult,
  ToolName,
  ToolSettings,
  ToolUpdates,
} from "./types";

type Unlisten = () => void;
type ProgressHandler = (event: ProgressEvent) => void;

const mockListeners = new Set<ProgressHandler>();
const mockTimers = new Map<string, number>();
const mockProgress = new Map<string, number>();
const mockToolSettings: ToolSettings = {
  ytDlpPath: null,
  ffmpegPath: null,
  proxyMode: "auto",
  proxyUrl: null,
};
const mockFfmpegCommandHistory: FfmpegCommandHistoryItem[] = [];
const mockCanceledOperations = new Set<string>();
const MOCK_DOWNLOAD_DIR = "Downloads";

export function isTauriRuntime() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function isMockPlaylistUrl(rawUrl: string) {
  try {
    const parsed = new URL(rawUrl);
    const pathname = parsed.pathname.toLowerCase();
    const hostname = parsed.hostname.toLowerCase();
    return (
      pathname.includes("playlist") ||
      parsed.searchParams.has("list") ||
      parsed.searchParams.has("playlist") ||
      (hostname.includes("bilibili.com") && parsed.searchParams.has("p"))
    );
  } catch {
    const normalized = rawUrl.toLowerCase();
    return (
      normalized.includes("playlist") ||
      /[?&](list|playlist)=/.test(normalized) ||
      (normalized.includes("bilibili.com") && /[?&]p=/.test(normalized))
    );
  }
}

export async function checkDependencies(): Promise<DependencyStatus> {
  if (!isTauriRuntime()) {
    const ytDlpInstalled = Boolean(mockToolSettings.ytDlpPath);
    const ffmpegInstalled = Boolean(mockToolSettings.ffmpegPath);

    return {
      ytDlp: {
        name: "yt-dlp",
        installed: ytDlpInstalled,
        path: mockToolSettings.ytDlpPath,
        version: ytDlpInstalled ? "mock" : null,
        source: ytDlpInstalled ? "manual" : null,
      },
      ffmpeg: {
        name: "ffmpeg",
        installed: ffmpegInstalled,
        path: mockToolSettings.ffmpegPath,
        version: ffmpegInstalled ? "mock" : null,
        source: ffmpegInstalled ? "manual" : null,
      },
      proxy: mockProxyStatus(),
      ready: ytDlpInstalled && ffmpegInstalled,
      installHint: "brew install yt-dlp ffmpeg",
    };
  }

  return invoke<DependencyStatus>("check_dependencies");
}

export async function checkToolUpdates(): Promise<ToolUpdates> {
  if (!isTauriRuntime()) {
    return {
      ytDlp: {
        name: "yt-dlp",
        currentVersion: mockToolSettings.ytDlpPath ? "2025.01.01" : null,
        latestVersion: "2026.05.01",
        updateAvailable: Boolean(mockToolSettings.ytDlpPath),
        checkedAt: String(Math.floor(Date.now() / 1000)),
        updateCommand: "brew update && brew upgrade yt-dlp",
        error: null,
      },
      ffmpeg: {
        name: "ffmpeg",
        currentVersion: mockToolSettings.ffmpegPath ? "7.1" : null,
        latestVersion: "8.0.1",
        updateAvailable: Boolean(mockToolSettings.ffmpegPath),
        checkedAt: String(Math.floor(Date.now() / 1000)),
        updateCommand: "brew update && brew upgrade ffmpeg",
        error: null,
      },
    };
  }

  return invoke<ToolUpdates>("check_tool_updates");
}

export async function loadToolSettings(): Promise<ToolSettings> {
  if (!isTauriRuntime()) {
    return { ...mockToolSettings };
  }

  return invoke<ToolSettings>("load_tool_settings");
}

export async function getDefaultDownloadDir(): Promise<string | null> {
  if (!isTauriRuntime()) {
    return MOCK_DOWNLOAD_DIR;
  }

  return invoke<string | null>("default_download_dir");
}

export async function saveToolPath(
  tool: ToolName,
  path: string,
): Promise<ToolSettings> {
  if (!isTauriRuntime()) {
    if (tool === "yt-dlp") {
      mockToolSettings.ytDlpPath = path;
    } else {
      mockToolSettings.ffmpegPath = path;
    }

    return { ...mockToolSettings };
  }

  return invoke<ToolSettings>("save_tool_path", { tool, path });
}

export async function clearToolPath(tool: ToolName): Promise<ToolSettings> {
  if (!isTauriRuntime()) {
    if (tool === "yt-dlp") {
      mockToolSettings.ytDlpPath = null;
    } else {
      mockToolSettings.ffmpegPath = null;
    }

    return { ...mockToolSettings };
  }

  return invoke<ToolSettings>("clear_tool_path", { tool });
}

export async function saveProxySettings(
  mode: ProxyMode,
  proxyUrl?: string | null,
): Promise<ToolSettings> {
  if (!isTauriRuntime()) {
    mockToolSettings.proxyMode = mode;
    mockToolSettings.proxyUrl = mode === "manual" ? (proxyUrl ?? null) : null;
    return { ...mockToolSettings };
  }

  return invoke<ToolSettings>("save_proxy_settings", { mode, proxyUrl });
}

export async function probeUrl(
  url: string,
  browser: BrowserKind,
  operationId?: string | null,
): Promise<ProbeResponse> {
  if (!isTauriRuntime()) {
    await delay(520, operationId);
    return {
      title: "Sample video · visual preview",
      site: "Generic",
      webpageUrl: url,
      duration: 482,
      thumbnail: null,
      checkedBrowser: browser,
      checkedAt: String(Math.floor(Date.now() / 1000)),
      formatCount: 3,
      bestFormatLabel: "1920x1080 · mp4 · H.264",
      formats: [
        {
          id: "best",
          label: "最佳画质 + 最佳音频",
          selector: "bv*+ba/b",
          ext: "mp4",
          resolution: "自动",
        },
        {
          id: "1080p",
          label: "1920x1080 · mp4 · H.264",
          selector: "bv*[height<=1080]+ba/b",
          ext: "mp4",
          resolution: "1920x1080",
          vcodec: "h264",
        },
        {
          id: "audio",
          label: "仅音频",
          selector: "ba",
          resolution: "audio",
        },
      ],
    };
  }

  return invoke<ProbeResponse>("probe_url", { url, browser, operationId });
}

export async function startDownload(
  request: DownloadRequest,
): Promise<string> {
  if (!isTauriRuntime()) {
    runMockDownload(request.taskId);
    return request.taskId;
  }

  return invoke<string>("start_download", { request });
}

export async function parseDownloadQueue(
  urls: string[],
  browser: BrowserKind,
  operationId?: string | null,
): Promise<BatchParseItem[]> {
  if (!isTauriRuntime()) {
    await delay(520, operationId);
    return urls.flatMap<BatchParseItem>((url, index) => {
      if (isMockPlaylistUrl(url)) {
        return [1, 2, 3].map((entry) => ({
          id: crypto.randomUUID(),
          url: `${url}#item-${entry}`,
          title: `播放列表视频 ${entry}`,
          site: "Mock Playlist",
          duration: 300 + entry * 42,
          thumbnail: null,
          sourceUrl: url,
          playlistTitle: `示例播放列表 ${index + 1}`,
          playlistIndex: entry,
          playlistTotal: 3,
          sourceOrder: index + 1,
          isPlaylistItem: true,
          error: null,
        }));
      }

      return {
        id: crypto.randomUUID(),
        url,
        title: `示例视频 ${index + 1}`,
        site: "Mock",
        duration: 482,
        thumbnail: null,
        sourceUrl: null,
        playlistTitle: null,
        playlistIndex: null,
        playlistTotal: null,
        sourceOrder: index + 1,
        isPlaylistItem: false,
        error: null,
      };
    });
  }

  return invoke<BatchParseItem[]>("parse_download_queue", {
    urls,
    browser,
    operationId,
  });
}

export async function cancelYtdlpOperation(operationId: string): Promise<void> {
  if (!isTauriRuntime()) {
    mockCanceledOperations.add(operationId);
    return;
  }

  return invoke<void>("cancel_ytdlp_operation", { operationId });
}

export async function cancelDownload(taskId: string): Promise<void> {
  if (!isTauriRuntime()) {
    const timer = mockTimers.get(taskId);
    if (timer) {
      window.clearInterval(timer);
      mockTimers.delete(taskId);
    }
    mockProgress.delete(taskId);
    emitMock({
      taskId,
      status: "canceled",
      progress: 0,
      downloadedBytes: null,
      totalBytes: null,
      totalBytesEstimated: null,
    });
    return;
  }

  return invoke<void>("cancel_download", { taskId });
}

export async function pauseDownload(taskId: string): Promise<void> {
  if (!isTauriRuntime()) {
    const timer = mockTimers.get(taskId);
    if (timer) {
      window.clearInterval(timer);
      mockTimers.delete(taskId);
    }
    emitMock({
      taskId,
      status: "paused",
      progress: mockProgress.get(taskId) ?? 0,
      downloadedBytes: null,
      totalBytes: null,
      totalBytesEstimated: null,
    });
    return;
  }

  return invoke<void>("pause_download", { taskId });
}

export async function scanDownloadCleanup(): Promise<DownloadCleanupSummary> {
  if (!isTauriRuntime()) {
    return {
      fileCount: 0,
      directoryCount: 0,
      bytes: 0,
      invalidHistoryCount: 0,
      skippedActiveTasks: mockTimers.size,
    };
  }

  return invoke<DownloadCleanupSummary>("scan_download_cleanup");
}

export async function cleanupDownloadCache(): Promise<DownloadCleanupSummary> {
  if (!isTauriRuntime()) {
    return scanDownloadCleanup();
  }

  return invoke<DownloadCleanupSummary>("cleanup_download_cache");
}

export async function revealFile(path: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  return invoke<void>("reveal_file", { path });
}

export async function loadHistory(): Promise<DownloadHistoryItem[]> {
  if (!isTauriRuntime()) {
    return [];
  }

  return invoke<DownloadHistoryItem[]>("load_history");
}

export async function deleteHistoryItem(
  id: string,
  deleteFile: boolean,
): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  return invoke<void>("delete_history_item", { id, deleteFile });
}

export async function loadSupportedSites(): Promise<SupportedSitesResponse> {
  if (!isTauriRuntime()) {
    return {
      version: "browser preview",
      total: 6,
      examples: [
        {
          name: "Bilibili",
          url: "https://www.bilibili.com/video/BV1xx411c7mD/",
        },
        {
          name: "YouTube",
          url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        },
        {
          name: "Vimeo",
          url: "https://vimeo.com/76979871",
        },
        {
          name: "TikTok",
          url: "https://www.tiktok.com/@example/video/0000000000000000000",
        },
      ],
    };
  }

  return invoke<SupportedSitesResponse>("list_supported_sites");
}

export async function buildFfmpegCommand(
  request: FfmpegCommandRequest,
): Promise<FfmpegCommandDraft> {
  if (!isTauriRuntime()) {
    const outputPath = mockFfmpegOutputPath(request);
    return {
      command: mockFfmpegCommand(request, outputPath),
      workingDir: request.outputDir || parentDir(request.inputPath),
      outputPath,
    };
  }

  return invoke<FfmpegCommandDraft>("build_ffmpeg_command", { request });
}

export async function loadFfmpegCommandHistory(): Promise<
  FfmpegCommandHistoryItem[]
> {
  if (!isTauriRuntime()) {
    return [...mockFfmpegCommandHistory];
  }

  return invoke<FfmpegCommandHistoryItem[]>("load_ffmpeg_command_history");
}

export async function appendFfmpegCommandHistory(
  item: FfmpegCommandHistoryInput,
): Promise<FfmpegCommandHistoryItem[]> {
  if (!isTauriRuntime()) {
    const nextItem: FfmpegCommandHistoryItem = {
      ...item,
      id: crypto.randomUUID(),
      createdAt: String(Math.floor(Date.now() / 1000)),
    };
    mockFfmpegCommandHistory.unshift(nextItem);
    return [...mockFfmpegCommandHistory];
  }

  return invoke<FfmpegCommandHistoryItem[]>("append_ffmpeg_command_history", {
    item,
  });
}

export async function deleteFfmpegCommandHistoryItems(
  ids: string[],
): Promise<FfmpegCommandHistoryItem[]> {
  if (!isTauriRuntime()) {
    const selectedIds = new Set(ids);
    for (let index = mockFfmpegCommandHistory.length - 1; index >= 0; index -= 1) {
      if (selectedIds.has(mockFfmpegCommandHistory[index].id)) {
        mockFfmpegCommandHistory.splice(index, 1);
      }
    }
    return [...mockFfmpegCommandHistory];
  }

  return invoke<FfmpegCommandHistoryItem[]>(
    "delete_ffmpeg_command_history_items",
    { ids },
  );
}

export async function clearFfmpegCommandHistory(): Promise<void> {
  if (!isTauriRuntime()) {
    mockFfmpegCommandHistory.splice(0);
    return;
  }

  return invoke<void>("clear_ffmpeg_command_history");
}

export async function prefillTerminalCommand(
  command: string,
  workingDir: string,
): Promise<TerminalPrefillResult> {
  if (!isTauriRuntime()) {
    await navigator.clipboard?.writeText(command);
    return {
      prefilled: false,
      message: "浏览器预览环境无法打开本地终端，命令已复制。",
    };
  }

  return invoke<TerminalPrefillResult>("prefill_terminal_command", {
    command,
    workingDir,
  });
}

export async function selectDirectory(): Promise<string | null> {
  if (!isTauriRuntime()) {
    return MOCK_DOWNLOAD_DIR;
  }

  const selected = await open({
    directory: true,
    multiple: false,
    title: "选择保存目录",
  });

  return typeof selected === "string" ? selected : null;
}

export async function selectMediaFile(title = "选择媒体文件"): Promise<
  string | null
> {
  if (!isTauriRuntime()) {
    return `${MOCK_DOWNLOAD_DIR}/sample-video.mp4`;
  }

  const selected = await open({
    directory: false,
    multiple: false,
    title,
  });

  return typeof selected === "string" ? selected : null;
}

export async function selectToolExecutable(
  tool: ToolName,
): Promise<string | null> {
  if (!isTauriRuntime()) {
    return tool === "yt-dlp"
      ? "/opt/homebrew/bin/yt-dlp"
      : "/opt/homebrew/bin/ffmpeg";
  }

  const selected = await open({
    directory: false,
    multiple: false,
    title: `选择 ${tool} 可执行文件`,
  });

  return typeof selected === "string" ? selected : null;
}

export async function subscribeDownloadProgress(
  handler: ProgressHandler,
): Promise<Unlisten> {
  if (!isTauriRuntime()) {
    mockListeners.add(handler);
    return () => mockListeners.delete(handler);
  }

  const unlisten = await listen<ProgressEvent>("download-progress", (event) => {
    handler(event.payload);
  });

  return unlisten;
}

function runMockDownload(taskId: string) {
  let progress = mockProgress.get(taskId) ?? 0;
  const timer = window.setInterval(() => {
    progress = Math.min(100, progress + 7 + Math.random() * 9);
    mockProgress.set(taskId, progress);
    const phase =
      progress >= 100
        ? "completed"
        : progress >= 99
          ? "merging"
          : progress >= 50
            ? "downloadingAudio"
            : "downloadingVideo";
    const phaseLabel =
      phase === "completed"
        ? "已完成"
        : phase === "merging"
          ? "合并封装中"
          : phase === "downloadingAudio"
            ? "下载音频流"
            : "下载视频流";
    emitMock({
      taskId,
      status: progress >= 100 ? "completed" : "running",
      progress,
      phase,
      phaseLabel,
      speed: progress >= 100 ? null : "8.4MB/s",
      eta: progress >= 100 ? null : "00:12",
      downloadedBytes: Math.round((progress / 100) * 185_000_000),
      totalBytes: 185_000_000,
      totalBytesEstimated: progress >= 100 ? false : true,
      outputPath:
        progress >= 100
          ? `${MOCK_DOWNLOAD_DIR}/sample-video.mp4`
          : undefined,
      localMedia:
        progress >= 100
          ? {
              duration: 481,
              width: 1920,
              height: 1080,
              videoCodec: "h264",
              audioCodec: "aac",
              fileSizeBytes: 185_000_000,
              probedAt: String(Math.floor(Date.now() / 1000)),
              error: null,
            }
          : undefined,
      mediaComparison: null,
    });

    if (progress >= 100) {
      window.clearInterval(timer);
      mockTimers.delete(taskId);
      mockProgress.delete(taskId);
    }
  }, 580);

  mockTimers.set(taskId, timer);
}

function emitMock(event: ProgressEvent) {
  mockListeners.forEach((listener) => listener(event));
}

function delay(milliseconds: number, operationId?: string | null) {
  return new Promise<void>((resolve, reject) => {
    const startedAt = Date.now();
    const tick = () => {
      if (operationId && mockCanceledOperations.has(operationId)) {
        mockCanceledOperations.delete(operationId);
        reject(new Error("操作已停止。"));
        return;
      }

      if (Date.now() - startedAt >= milliseconds) {
        resolve();
        return;
      }

      window.setTimeout(tick, 40);
    };

    tick();
  });
}

function mockProxyStatus(): ProxyStatus {
  const mode = mockToolSettings.proxyMode ?? "auto";

  if (mode === "manual") {
    return {
      mode,
      effectiveProxy: mockToolSettings.proxyUrl ?? null,
      source: "manual",
      message: mockToolSettings.proxyUrl ? "手动指定代理" : "请填写手动代理地址。",
    };
  }

  if (mode === "off") {
    return {
      mode,
      effectiveProxy: null,
      source: "off",
      message: "不使用代理",
    };
  }

  return {
    mode: "auto",
    effectiveProxy: null,
    source: "none",
    message: "未检测到系统代理",
  };
}

function mockFfmpegCommand(
  request: FfmpegCommandRequest,
  outputPath: string,
) {
  const base = [
    "/opt/homebrew/bin/ffmpeg",
    "-hide_banner",
    "-n",
    "-i",
    request.inputPath,
  ];

  if (request.presetId === "mergeAudioVideo" && request.secondaryInputPath) {
    base.push("-i", request.secondaryInputPath);
  }

  base.push(outputPath);
  return base.map(shellQuote).join(" ");
}

function mockFfmpegOutputPath(request: FfmpegCommandRequest) {
  const outputDir = request.outputDir || parentDir(request.inputPath);
  const stem = fileStem(request.inputPath) || "video";
  const ext =
    request.presetId === "extractAudio"
      ? (request.audioFormat ?? "mp3")
      : "mp4";
  return `${outputDir}/${stem}-${ffmpegPresetSuffix(request.presetId)}.${ext}`;
}

function ffmpegPresetSuffix(presetId: FfmpegCommandRequest["presetId"]) {
  const suffixes: Record<FfmpegCommandRequest["presetId"], string> = {
    convertMp4: "converted",
    compress: "compressed",
    extractAudio: "audio",
    trim: "clip",
    mergeAudioVideo: "merged",
  };

  return suffixes[presetId];
}

function parentDir(path: string) {
  const normalized = path.replace(/\\/g, "/");
  return normalized.split("/").slice(0, -1).join("/") || ".";
}

function fileStem(path: string) {
  const fileName = path.replace(/\\/g, "/").split("/").pop() ?? "";
  return fileName.replace(/\.[^.]+$/, "");
}

function shellQuote(value: string) {
  if (/^[A-Za-z0-9_./:+=@%-]+$/.test(value)) {
    return value;
  }

  return `'${value.replace(/'/g, `'\\''`)}'`;
}
