import type { FfmpegPresetId } from "./types";

export const FFMPEG_PRESET_LABELS: Record<FfmpegPresetId, string> = {
  convertMp4: "转 MP4",
  compress: "压缩视频",
  extractAudio: "提取音频",
  trim: "截取片段",
  mergeAudioVideo: "合并音视频",
};

export function isKnownFfmpegPresetId(value: string): value is FfmpegPresetId {
  return Object.prototype.hasOwnProperty.call(FFMPEG_PRESET_LABELS, value);
}

export function ffmpegPresetLabel(value: string) {
  return isKnownFfmpegPresetId(value)
    ? FFMPEG_PRESET_LABELS[value]
    : "未知预设";
}
