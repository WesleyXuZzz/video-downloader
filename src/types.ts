export type BrowserKind = "chrome" | "safari" | "firefox";

export type ToolName = "yt-dlp" | "ffmpeg";

export type ToolSource = "manual" | "env" | "path";

export type DownloadStatus =
  | "idle"
  | "running"
  | "completed"
  | "failed"
  | "canceled";

export type DownloadQueueStatus = DownloadStatus | "queued";

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
}

export interface DependencyStatus {
  ytDlp: ToolStatus;
  ffmpeg: ToolStatus;
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
}

export interface DownloadRequest {
  taskId: string;
  url: string;
  title?: string | null;
  site?: string | null;
  format: string;
  browser?: BrowserKind | null;
  outputDir: string;
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
  error?: string | null;
  updatedAt: string;
}

export interface ProgressEvent {
  taskId: string;
  status: DownloadStatus;
  progress: number;
  speed?: string | null;
  eta?: string | null;
  line?: string | null;
  outputPath?: string | null;
  error?: string | null;
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

export interface FfmpegCommandHistoryItem extends FfmpegCommandRequest {
  id: string;
  command: string;
  workingDir: string;
  outputPath: string;
  createdAt: string;
}

export type FfmpegCommandHistoryInput = Omit<
  FfmpegCommandHistoryItem,
  "id" | "createdAt"
>;

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
