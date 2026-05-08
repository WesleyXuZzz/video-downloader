import type { LocalMediaInfo, MediaComparison } from "./types";

export function localMediaSummary(
  media?: LocalMediaInfo | null,
  prefix = "实际：",
) {
  if (!media) {
    return null;
  }

  const parts = [
    media.duration ? formatMediaDuration(media.duration) : null,
    localMediaResolution(media),
    localMediaCodecs(media),
  ].filter(Boolean);

  if (!parts.length) {
    return "未读取到本地媒体信息";
  }

  return `${prefix}${parts.join(" · ")}`;
}

export function localMediaErrorTitle(media?: LocalMediaInfo | null) {
  return media?.error?.trim() || null;
}

export function mediaComparisonSummary(comparison?: MediaComparison | null) {
  if (!comparison) {
    return null;
  }

  const parts = [
    comparison.resolution ? resolutionComparisonText(comparison.resolution) : null,
    comparison.duration ? durationComparisonText(comparison.duration) : null,
  ].filter(Boolean);

  return parts.length ? parts.join("；") : null;
}

export function formatMediaDuration(duration?: number | null) {
  if (!duration || !Number.isFinite(duration) || duration <= 0) {
    return null;
  }

  const totalSeconds = Math.round(duration);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;

  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
  }

  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

function localMediaResolution(media: LocalMediaInfo) {
  if (!media.width || !media.height) {
    return null;
  }

  return `${media.width}×${media.height}`;
}

function localMediaCodecs(media: LocalMediaInfo) {
  const codecs = [media.videoCodec, media.audioCodec]
    .map((codec) => codec?.trim())
    .filter(Boolean);

  return codecs.length ? codecs.join(" / ") : null;
}

function resolutionComparisonText(
  detail: NonNullable<MediaComparison["resolution"]>,
) {
  const expected = detail.expectedLabel?.trim();
  const actual = detail.actualLabel?.trim();

  if (expected && actual) {
    return `预期 ${expected}，实际 ${actual}`;
  }

  if (detail.status === "lower") {
    return "实际分辨率低于预期";
  }

  return "实际分辨率不同于预期";
}

function durationComparisonText(
  detail: NonNullable<MediaComparison["duration"]>,
) {
  const expected = detail.expectedLabel?.trim();
  const actual = detail.actualLabel?.trim();
  const direction = detail.status === "shorter" ? "短于" : "长于";

  if (expected && actual) {
    return `实际时长${direction}预期（预期 ${expected}，实际 ${actual}）`;
  }

  return `实际时长${direction}预期`;
}
