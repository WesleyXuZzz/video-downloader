import type { DownloadHistoryItem, DownloadStatus } from "./types";

export const statusCopy: Record<DownloadStatus, string> = {
  idle: "待开始",
  running: "下载中",
  completed: "已完成",
  failed: "失败",
  canceled: "已取消",
};

export function statusTagColor(status: DownloadStatus) {
  if (status === "running") {
    return "processing";
  }

  if (status === "completed") {
    return "success";
  }

  if (status === "failed") {
    return "error";
  }

  if (status === "canceled") {
    return "warning";
  }

  return "default";
}

export function formatHistoryDate(value?: string | null) {
  if (!value) {
    return "未知时间";
  }

  const numericValue = Number(value);
  const date = Number.isFinite(numericValue)
    ? new Date(numericValue * (numericValue > 1_000_000_000_000 ? 1 : 1000))
    : new Date(value);

  if (Number.isNaN(date.getTime())) {
    return "未知时间";
  }

  return date.toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

export function historyTitle(item: DownloadHistoryItem) {
  const outputTitle = titleFromOutputPath(item.outputPath);
  const storedTitle = item.title?.trim();

  if (storedTitle && outputTitle && isOpaqueHistoryId(storedTitle)) {
    return outputTitle;
  }

  return (
    cleanHistoryText(item.title)
    ?? outputTitle
    ?? titleFromUrl(item.url)
    ?? "待解析视频"
  );
}

export function historySite(item: DownloadHistoryItem) {
  return cleanHistoryText(item.site) ?? siteFromUrl(item.url) ?? "未知站点";
}

function cleanHistoryText(value?: string | null) {
  const text = value?.trim();
  if (!text || isPlaceholderHistoryText(text)) {
    return null;
  }

  return text;
}

function isPlaceholderHistoryText(value: string) {
  const normalized = value.trim().toLowerCase();

  return (
    ["undefined", "null", "unknown", "untitled", "untitled video"].includes(
      normalized,
    )
    || /^bv[a-z0-9]+$/i.test(value.trim())
    || /^av\d+$/i.test(value.trim())
  );
}

function isOpaqueHistoryId(value: string) {
  const text = value.trim();
  const hasDigit = /\d/.test(text);
  const hasUppercase = /[A-Z]/.test(text);

  return (
    isPlaceholderHistoryText(text)
    || (hasDigit && hasUppercase && /^[a-z0-9_-]{8,24}$/i.test(text))
  );
}

function titleFromOutputPath(path?: string | null) {
  const fileName = path?.split("/").pop()?.replace(/\.[^.]+$/, "");
  const title = fileName?.split("-BV")[0];
  return cleanHistoryText(title);
}

function titleFromUrl(url?: string | null) {
  const tail = url?.split("?")[0]?.split("/").filter(Boolean).pop();
  return cleanHistoryText(tail);
}

function siteFromUrl(url?: string | null) {
  if (!url) {
    return null;
  }

  try {
    return cleanHistoryText(new URL(url).hostname.replace(/^www\./, ""));
  } catch {
    return null;
  }
}
