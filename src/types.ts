export type BrowserKind = "chrome" | "safari" | "firefox";

export type ToolName = "yt-dlp" | "ffmpeg";

export type ToolSource = "manual" | "env" | "path";

export type ProxyMode = "auto" | "manual" | "off";

export type ProxySource =
  | "manual"
  | "off"
  | "none"
  | "systemHttps"
  | "systemHttp"
  | "systemSocks"
  | "pacUnsupported"
  | "unsupported"
  | "error";

export type DownloadStatus =
  | "idle"
  | "running"
  | "paused"
  | "completed"
  | "failed"
  | "canceled";

export type DownloadQueueStatus = DownloadStatus | "queued";

export type DownloadPhase =
  | "downloadingVideo"
  | "downloadingAudio"
  | "downloadingMedia"
  | "merging"
  | "completed";

export type FfmpegPresetId =
  | "convertMp4"
  | "compress"
  | "extractAudio"
  | "trim"
  | "mergeAudioVideo";

export interface ToolStatus {
  name: ToolName;
  installed: boolean;
  path?: string | null;
  version?: string | null;
  source?: ToolSource | null;
}

export interface ToolSettings {
  ytDlpPath?: string | null;
  ffmpegPath?: string | null;
  proxyMode?: ProxyMode | null;
  proxyUrl?: string | null;
}

export interface ProxyStatus {
  mode: ProxyMode;
  effectiveProxy?: string | null;
  source?: ProxySource | string | null;
  message?: string | null;
}

export interface DependencyStatus {
  ytDlp: ToolStatus;
  ffmpeg: ToolStatus;
  proxy: ProxyStatus;
  ready: boolean;
  installHint: string;
}

export interface ToolUpdateStatus {
  name: ToolName;
  currentVersion?: string | null;
  latestVersion?: string | null;
  updateAvailable?: boolean | null;
  checkedAt: string;
  updateCommand?: string | null;
  error?: string | null;
}

export interface ToolUpdates {
  ytDlp: ToolUpdateStatus;
  ffmpeg: ToolUpdateStatus;
}

export interface FormatOption {
  id: string;
  label: string;
  selector: string;
  ext?: string | null;
  resolution?: string | null;
  vcodec?: string | null;
  acodec?: string | null;
  filesize?: number | null;
}

export interface ProbeResponse {
  title: string;
  site: string;
  webpageUrl: string;
  duration?: number | null;
  thumbnail?: string | null;
  formats: FormatOption[];
  checkedBrowser?: BrowserKind | null;
  checkedAt: string;
  formatCount: number;
  bestFormatLabel?: string | null;
}

export interface DownloadRequest {
  taskId: string;
  url: string;
  title?: string | null;
  site?: string | null;
  format: string;
  browser?: BrowserKind | null;
  outputDir: string;
  expectedMedia?: ExpectedMediaInfo | null;
}

export interface ExpectedMediaInfo {
  duration?: number | null;
  resolutionLabel?: string | null;
  resolutionScore?: number | null;
}

export interface BatchParseItem {
  id: string;
  url: string;
  title?: string | null;
  site?: string | null;
  duration?: number | null;
  thumbnail?: string | null;
  sourceUrl?: string | null;
  playlistTitle?: string | null;
  playlistIndex?: number | null;
  playlistTotal?: number | null;
  sourceOrder?: number | null;
  isPlaylistItem: boolean;
  error?: string | null;
}

export interface DownloadHistoryItem {
  id: string;
  url: string;
  title: string;
  site: string;
  format: string;
  browser?: BrowserKind | null;
  outputDir: string;
  status: DownloadStatus;
  progress: number;
  outputPath?: string | null;
  localMedia?: LocalMediaInfo | null;
  mediaComparison?: MediaComparison | null;
  error?: string | null;
  updatedAt: string;
}

export interface ProgressEvent {
  taskId: string;
  status: DownloadStatus;
  progress: number;
  phase?: DownloadPhase | null;
  phaseLabel?: string | null;
  speed?: string | null;
  eta?: string | null;
  line?: string | null;
  outputPath?: string | null;
  localMedia?: LocalMediaInfo | null;
  mediaComparison?: MediaComparison | null;
  error?: string | null;
}

export interface DownloadCleanupSummary {
  fileCount: number;
  directoryCount: number;
  bytes: number;
  invalidHistoryCount: number;
  skippedActiveTasks: number;
}

export interface LocalMediaInfo {
  duration?: number | null;
  width?: number | null;
  height?: number | null;
  videoCodec?: string | null;
  audioCodec?: string | null;
  probedAt?: string | null;
  error?: string | null;
}

export interface MediaComparison {
  duration?: MediaComparisonDetail | null;
  resolution?: MediaComparisonDetail | null;
}

export interface MediaComparisonDetail {
  status: "shorter" | "longer" | "lower" | "higher";
  expectedLabel?: string | null;
  actualLabel?: string | null;
}

export interface FfmpegCommandRequest {
  presetId: FfmpegPresetId;
  inputPath: string;
  secondaryInputPath?: string | null;
  outputDir: string;
  audioFormat?: "mp3" | "m4a" | null;
  startTime?: string | null;
  endTime?: string | null;
  crf?: number | null;
}

export interface FfmpegCommandDraft {
  command: string;
  workingDir: string;
  outputPath: string;
}

export interface FfmpegCommandHistoryItem {
  id: string;
  presetId: string;
  inputPath: string;
  secondaryInputPath?: string | null;
  outputDir: string;
  audioFormat?: "mp3" | "m4a" | null;
  startTime?: string | null;
  endTime?: string | null;
  crf?: number | null;
  command: string;
  workingDir: string;
  outputPath: string;
  createdAt: string;
}

export type FfmpegCommandHistoryInput = FfmpegCommandRequest &
  Omit<FfmpegCommandHistoryItem, "id" | "createdAt" | keyof FfmpegCommandRequest>;

export interface TerminalPrefillResult {
  prefilled: boolean;
  message: string;
}

export interface SupportedSiteExample {
  name: string;
  url: string;
}

export interface SupportedSitesResponse {
  version?: string | null;
  total: number;
  examples: SupportedSiteExample[];
}
