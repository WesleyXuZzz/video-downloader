import {
  Alert,
  Button,
  Card,
  Empty,
  Input,
  Layout,
  List,
  message,
  Modal,
  Popconfirm,
  Popover,
  Progress,
  Segmented,
  Select,
  Space,
  Tag,
  Tooltip,
  Typography,
} from "antd";
import type { ProgressProps } from "antd";
import {
  ClockCircleOutlined,
  CodeOutlined,
  DashboardOutlined,
  DeleteOutlined,
  DownloadOutlined,
  ExportOutlined,
  FolderOpenOutlined,
  LinkOutlined,
  LoadingOutlined,
  MenuFoldOutlined,
  MenuUnfoldOutlined,
  PauseCircleOutlined,
  PlayCircleOutlined,
  PoweroffOutlined,
  ReloadOutlined,
  RetweetOutlined,
  SearchOutlined,
  StopOutlined,
  VideoCameraOutlined,
} from "@ant-design/icons";
import {
  lazy,
  Suspense,
  type KeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  appendFfmpegCommandHistory,
  buildFfmpegCommand,
  cancelDownload,
  cancelYtdlpOperation,
  checkDependencies,
  checkToolUpdates,
  cleanupDownloadCache,
  clearFfmpegCommandHistory,
  clearToolPath,
  deleteFfmpegCommandHistoryItems,
  deleteHistoryItem,
  loadHistory,
  getDefaultDownloadDir,
  loadFfmpegCommandHistory,
  loadToolSettings,
  loadSupportedSites,
  parseDownloadQueue,
  pauseDownload,
  prefillTerminalCommand,
  probeUrl,
  revealFile,
  scanDownloadCleanup,
  selectDirectory,
  selectMediaFile,
  selectToolExecutable,
  saveToolPath,
  saveProxySettings,
  startDownload,
  subscribeDownloadProgress,
} from "./tauri";
import type {
  BatchParseItem,
  BrowserKind,
  DependencyStatus,
  DownloadCleanupSummary,
  DownloadHistoryItem,
  DownloadPhase,
  DownloadQueueStatus,
  DownloadStatus,
  ExpectedMediaInfo,
  FfmpegCommandDraft,
  FfmpegCommandHistoryItem,
  FfmpegCommandRequest,
  FfmpegPresetId,
  FormatOption,
  LocalMediaInfo,
  MediaComparison,
  ProbeResponse,
  ProgressEvent,
  ProxyMode,
  ProxyStatus,
  SupportedSiteExample,
  SupportedSitesResponse,
  ToolName,
  ToolSettings,
  ToolSource,
  ToolUpdateStatus,
  ToolUpdates,
} from "./types";
import PanelTitle from "./components/PanelTitle";
import {
  historySite,
  historyTitle,
  statusCopy,
  statusTagColor,
} from "./historyUtils";
import {
  localMediaErrorTitle,
  localMediaSummary,
  mediaComparisonSummary,
} from "./mediaInfo";
import { isKnownFfmpegPresetId } from "./ffmpegPresets";

const { Content, Sider } = Layout;
const { Text, Title } = Typography;

const FfmpegView = lazy(() => import("./views/FfmpegView"));
const HistoryView = lazy(() => import("./views/HistoryView"));

const DEFAULT_URL = "";
const DEFAULT_CONCURRENCY = 1;
const THUMBNAIL_RETRY_LIMIT = 3;
const QUEUE_AUTH_PROBE_CONCURRENCY = 3;

const appIconUrl = new URL("./assets/app-icon.png", import.meta.url).href;

const browserOptions: Array<{ label: string; value: BrowserKind }> = [
  { label: "Chrome", value: "chrome" },
  { label: "Safari", value: "safari" },
  { label: "Firefox", value: "firefox" },
];

const concurrencyOptions = [1, 2, 3, 4, 5].map((value) => ({
  label: value === DEFAULT_CONCURRENCY ? `${value}（默认）` : String(value),
  value,
}));

const toolSourceCopy: Record<ToolSource, string> = {
  manual: "手动指定",
  env: "环境变量",
  path: "系统路径",
};

type Notice = {
  type: "success" | "warning" | "error" | "info";
  text: string;
};

type ActiveView = "download" | "ffmpeg" | "history";
const FALLBACK_OUTPUT_DIR = "";

type AuthProbeStatus =
  | "idle"
  | "checking"
  | "ready"
  | "warning"
  | "failed"
  | "unavailable"
  | "canceled";

type AuthProbeState = {
  status: AuthProbeStatus;
  browser: BrowserKind;
  url: string;
  site?: string | null;
  title?: string | null;
  duration?: number | null;
  bestFormatLabel?: string | null;
  formatCount?: number | null;
  checkedAt?: string | null;
  error?: string | null;
};

type AuthFormatParts = {
  detail: string;
  fullText: string;
  resolution: string | null;
  score: number;
};

type QueueAuthSiteSummary = {
  site: string;
  total: number;
  ready: number;
  warning: number;
  unavailable: number;
  canceled: number;
  checking: number;
  idle: number;
  parseFailed: number;
  status?: AuthProbeStatus;
  bestFormatLabel?: string | null;
  formatCount?: number | null;
  error?: string | null;
};

type QueueAuthSummary = {
  total: number;
  probeable: number;
  ready: number;
  warning: number;
  unavailable: number;
  canceled: number;
  checking: number;
  idle: number;
  parseFailed: number;
  status: AuthProbeStatus;
  bestFormatLabel?: string | null;
  sites: QueueAuthSiteSummary[];
};

type QueueItem = {
  id: string;
  url: string;
  title: string;
  site: string;
  duration?: number | null;
  thumbnail?: string | null;
  sourceUrl?: string | null;
  playlistTitle?: string | null;
  playlistIndex?: number | null;
  playlistTotal?: number | null;
  sourceOrder?: number | null;
  isPlaylistItem: boolean;
  status: DownloadQueueStatus;
  parseError?: boolean;
  authProbe?: AuthProbeState | null;
  progress: number;
  phase?: DownloadPhase | null;
  phaseLabel?: string | null;
  speed?: string | null;
  eta?: string | null;
  outputPath?: string | null;
  localMedia?: LocalMediaInfo | null;
  mediaComparison?: MediaComparison | null;
  error?: string | null;
};

type ThumbnailLoadFailure = {
  taskId: string;
  url: string;
  attempts: number;
};

type ThumbnailLoadState = {
  status: "loading" | "loaded" | "failed";
  attempts: number;
};

type ThumbnailDisplayState = {
  label: string;
  status: ThumbnailLoadState["status"] | "missing";
  attempts: number;
  canRenderImage: boolean;
  showImage: boolean;
};

class FfmpegHistorySaveError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "FfmpegHistorySaveError";
  }
}

const sidebarNavItems: Array<{
  key: ActiveView;
  label: string;
  icon: ReactNode;
}> = [
  { key: "download", label: "新建下载", icon: <DownloadOutlined /> },
  { key: "history", label: "历史记录", icon: <ClockCircleOutlined /> },
  { key: "ffmpeg", label: "FFmpeg 工具箱", icon: <CodeOutlined /> },
];

export default function App() {
  const [url, setUrl] = useState(DEFAULT_URL);
  const [browser, setBrowser] = useState<BrowserKind>("chrome");
  const [outputDir, setOutputDir] = useState(FALLBACK_OUTPUT_DIR);
  const [dependencies, setDependencies] = useState<DependencyStatus | null>(
    null,
  );
  const [toolSettings, setToolSettings] = useState<ToolSettings>({
    ytDlpPath: null,
    ffmpegPath: null,
    proxyMode: "auto",
    proxyUrl: null,
  });
  const [toolUpdates, setToolUpdates] = useState<ToolUpdates | null>(null);
  const [probe, setProbe] = useState<ProbeResponse | null>(null);
  const [selectedFormat, setSelectedFormat] = useState("bv*+ba/b");
  const [status, setStatus] = useState<DownloadStatus>("idle");
  const [progress, setProgress] = useState(0);
  const [speed, setSpeed] = useState<string | null>(null);
  const [eta, setEta] = useState<string | null>(null);
  const [queue, setQueue] = useState<QueueItem[]>([]);
  const [concurrency, setConcurrency] = useState(DEFAULT_CONCURRENCY);
  const [isQueuePaused, setIsQueuePaused] = useState(false);
  const [isParsingQueue, setIsParsingQueue] = useState(false);
  const [outputPath, setOutputPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState<DownloadHistoryItem[]>([]);
  const [historyDeleteTarget, setHistoryDeleteTarget] =
    useState<DownloadHistoryItem | null>(null);
  const [historyDeleteBusy, setHistoryDeleteBusy] = useState(false);
  const [cleanupSummary, setCleanupSummary] =
    useState<DownloadCleanupSummary | null>(null);
  const [isCleanupModalOpen, setIsCleanupModalOpen] = useState(false);
  const [isScanningCleanup, setIsScanningCleanup] = useState(false);
  const [isCleaningCache, setIsCleaningCache] = useState(false);
  const [supportedSites, setSupportedSites] =
    useState<SupportedSitesResponse | null>(null);
  const [isProbing, setIsProbing] = useState(false);
  const [authProbe, setAuthProbe] = useState<AuthProbeState>({
    status: "idle",
    browser: "chrome",
    url: "",
  });
  const [isRefreshingToolMetadata, setIsRefreshingToolMetadata] =
    useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [activeView, setActiveView] = useState<ActiveView>("download");
  const [ffmpegPreset, setFfmpegPreset] =
    useState<FfmpegPresetId>("convertMp4");
  const [ffmpegInputPath, setFfmpegInputPath] = useState("");
  const [ffmpegSecondaryInputPath, setFfmpegSecondaryInputPath] = useState("");
  const [ffmpegOutputDir, setFfmpegOutputDir] = useState(outputDir);
  const [ffmpegAudioFormat, setFfmpegAudioFormat] =
    useState<"mp3" | "m4a">("mp3");
  const [ffmpegCrf, setFfmpegCrf] = useState(28);
  const [ffmpegStartTime, setFfmpegStartTime] = useState("00:00:00");
  const [ffmpegEndTime, setFfmpegEndTime] = useState("00:00:30");
  const [ffmpegDraft, setFfmpegDraft] =
    useState<FfmpegCommandDraft | null>(null);
  const [ffmpegNotice, setFfmpegNotice] = useState<Notice | null>(null);
  const [ffmpegCommandHistory, setFfmpegCommandHistory] = useState<
    FfmpegCommandHistoryItem[]
  >([]);
  const [hasLoadedFfmpegCommandHistory, setHasLoadedFfmpegCommandHistory] =
    useState(false);
  const [isLoadingFfmpegCommandHistory, setIsLoadingFfmpegCommandHistory] =
    useState(false);
  const [isMutatingFfmpegCommandHistory, setIsMutatingFfmpegCommandHistory] =
    useState(false);
  const [isBuildingFfmpeg, setIsBuildingFfmpeg] = useState(false);
  const [isPrefillingTerminal, setIsPrefillingTerminal] = useState(false);
  const [failedThumbnail, setFailedThumbnail] =
    useState<ThumbnailLoadFailure | null>(null);
  const [thumbnailLoadStates, setThumbnailLoadStates] = useState<
    Record<string, ThumbnailLoadState>
  >({});
  const activeTaskRef = useRef<Set<string>>(new Set());
  const queueItemIdsRef = useRef<Set<string>>(new Set());
  const startingTaskRef = useRef<Set<string>>(new Set());
  const metadataProbeRef = useRef<Set<string>>(new Set());
  const schedulingRef = useRef(false);
  const keepNextFfmpegDraftResetRef = useRef(false);
  const authProbeRequestRef = useRef(0);
  const queueAuthProbeBatchRef = useRef(0);
  const parseRequestRef = useRef(0);
  const parseOperationRef = useRef<string | null>(null);
  const authOperationIdsRef = useRef<Set<string>>(new Set());

  const formatOptions = useMemo<FormatOption[]>(() => {
    return probe?.formats.length ? probe.formats : fallbackFormats;
  }, [probe]);

  const formatSelectOptions = useMemo(() => {
    return formatOptions.map((format) => ({
      label: format.label,
      value: format.selector,
    }));
  }, [formatOptions]);

  const canBuildFfmpegCommand =
    Boolean(ffmpegInputPath.trim()) &&
    Boolean(ffmpegOutputDir.trim()) &&
    (ffmpegPreset !== "mergeAudioVideo" ||
      Boolean(ffmpegSecondaryInputPath.trim()));

  const pageHeading = getPageHeading(activeView);
  const queueSummary = useMemo(() => summarizeQueue(queue), [queue]);
  const queueAuthSummary = useMemo(() => summarizeQueueAuth(queue), [queue]);
  const hasRunningQueue = queueSummary.running > 0;
  const hasIdleQueue = queueSummary.idle > 0;
  const hasQueuedQueue = queueSummary.queued > 0;
  const hasPausedQueue = queueSummary.paused > 0;
  const hasPausableQueue = hasRunningQueue || hasQueuedQueue;
  const hasResumableQueue = hasPausedQueue && !hasPausableQueue;
  const hasCancelableQueue =
    hasRunningQueue || hasQueuedQueue || hasPausedQueue || hasIdleQueue;
  const hasQueueItems = queue.length > 0;
  const hasQueueAuthProbeTargets = queue.some(queueItemCanAuthProbe);
  const isQueueAuthChecking = Boolean(queueAuthSummary?.checking);
  const currentQueueItem = useMemo(() => {
    return (
      queue.find((item) => item.status === "running") ??
      queue.find((item) => item.status === "queued") ??
      queue.find((item) => item.status === "paused") ??
      queue.find((item) => item.status === "idle") ??
      queue.find((item) => item.status === "completed") ??
      queue.find((item) => item.status === "failed" && !item.parseError) ??
      queue.find((item) => item.status === "canceled" && !item.parseError) ??
      null
    );
  }, [queue]);
  const currentThumbnailState = useMemo(
    () =>
      currentQueueItem
        ? thumbnailDisplayState(
            currentQueueItem,
            failedThumbnail,
            thumbnailLoadStates,
          )
        : null,
    [currentQueueItem, failedThumbnail, thumbnailLoadStates],
  );
  const currentLocalMediaText = currentQueueItem
    ? localMediaSummary(currentQueueItem.localMedia)
    : null;
  const currentLocalMediaTitle = currentQueueItem
    ? localMediaErrorTitle(currentQueueItem.localMedia)
    : null;
  const currentMediaComparisonText = currentQueueItem
    ? mediaComparisonSummary(currentQueueItem.mediaComparison)
    : null;
  const urlLineCount = useMemo(() => parseUrlLines(url).length, [url]);
  const firstUrl = useMemo(() => parseUrlLines(url)[0] ?? "", [url]);
  const parseActionLabel = urlLineCount > 1 ? "解析队列" : "解析视频";
  const startActionLabel =
    (hasQueueItems ? queue.length : urlLineCount) > 1
      ? "开始批量下载"
      : "开始下载";
  const pauseActionLabel = hasResumableQueue ? "继续下载" : "暂停队列";

  useEffect(() => {
    refreshStartupData();
    initializeDefaultOutputDir();
  }, []);

  useEffect(() => {
    let mounted = true;
    let cleanup: (() => void) | undefined;

    subscribeDownloadProgress((event) => {
      if (mounted) {
        handleProgress(event);
      }
    }).then((unlisten) => {
      cleanup = unlisten;
    });

    return () => {
      mounted = false;
      cleanup?.();
    };
  }, []);

  useEffect(() => {
    activeTaskRef.current = new Set(
      queue.filter((item) => item.status === "running").map((item) => item.id),
    );
    queueItemIdsRef.current = new Set(queue.map((item) => item.id));
  }, [queue]);

  useEffect(() => {
    scheduleQueuedDownloads();
  }, [queue, concurrency, isQueuePaused, outputDir, selectedFormat, browser]);

  useEffect(() => {
    authProbeRequestRef.current += 1;

    if (hasQueueItems) {
      setAuthProbe({
        status: "idle",
        browser,
        url: firstUrl,
      });
      return;
    }

    if (!firstUrl) {
      setAuthProbe({
        status: "idle",
        browser,
        url: "",
      });
      return;
    }

    setAuthProbe({
      status: "idle",
      browser,
      url: firstUrl,
    });

    const timer = window.setTimeout(() => {
      void refreshAuthProbe("auto");
    }, 800);

    return () => window.clearTimeout(timer);
  }, [browser, firstUrl, hasQueueItems]);

  useEffect(() => {
    if (
      activeView === "ffmpeg" &&
      !hasLoadedFfmpegCommandHistory &&
      !isLoadingFfmpegCommandHistory
    ) {
      void refreshFfmpegCommandHistory();
    }
  }, [
    activeView,
    hasLoadedFfmpegCommandHistory,
    isLoadingFfmpegCommandHistory,
  ]);

  useEffect(() => {
    const candidates = queue
      .filter((item) => queueItemNeedsMetadata(item, failedThumbnail))
      .filter((item) => {
        const probeKey = queueMetadataProbeKey(item, browser, failedThumbnail);
        return !metadataProbeRef.current.has(probeKey);
      })
      .slice(0, 3);

    if (!candidates.length) {
      return;
    }

    candidates.forEach((item) => {
      metadataProbeRef.current.add(
        queueMetadataProbeKey(item, browser, failedThumbnail),
      );
    });

    let canceled = false;

    candidates.forEach((item) => {
      void probeUrl(item.url, browser)
        .then((result) => {
          if (canceled) {
            return;
          }

          setQueue((items) =>
            items.map((candidate) =>
              candidate.id === item.id
                ? mergeQueueMetadata(candidate, result)
                : candidate,
            ),
          );
        })
        .catch(() => {
          // flat playlist 解析可能没有封面；补全失败不影响队列下载。
        });
    });

    return () => {
      canceled = true;
    };
  }, [browser, failedThumbnail, queue]);

  useEffect(() => {
    setFailedThumbnail((failure) => {
      if (!failure) {
        return null;
      }

      if (
        currentQueueItem?.id !== failure.taskId ||
        currentQueueItem.thumbnail !== failure.url
      ) {
        return null;
      }

      return failure;
    });
  }, [probe?.thumbnail, currentQueueItem?.id, currentQueueItem?.thumbnail]);

  useEffect(() => {
    if (keepNextFfmpegDraftResetRef.current) {
      keepNextFfmpegDraftResetRef.current = false;
      return;
    }

    setFfmpegDraft(null);
    setFfmpegNotice(null);
  }, [
    ffmpegAudioFormat,
    ffmpegCrf,
    ffmpegEndTime,
    ffmpegInputPath,
    ffmpegOutputDir,
    ffmpegPreset,
    ffmpegSecondaryInputPath,
    ffmpegStartTime,
  ]);

  async function refreshStartupData() {
    refreshDependencies();
    refreshHistory();
    window.setTimeout(() => {
      refreshDeferredStartupData();
    }, 800);
  }

  async function initializeDefaultOutputDir() {
    try {
      const defaultDir = await getDefaultDownloadDir();
      if (defaultDir) {
        setOutputDir(defaultDir);
        setFfmpegOutputDir(defaultDir);
      }
    } catch (caught) {
      setError(readError(caught));
    }
  }

  async function refreshDependencies() {
    try {
      const [nextDependencies, nextToolSettings] = await Promise.all([
        checkDependencies(),
        loadToolSettings(),
      ]);
      setDependencies(nextDependencies);
      setToolSettings(nextToolSettings);
    } catch (caught) {
      setError(readError(caught));
    }
  }

  async function refreshDeferredStartupData() {
    const [nextToolUpdates, nextSupportedSites] = await Promise.all([
      checkToolUpdates().catch(() => null),
      loadSupportedSites().catch(() => null),
    ]);
    setToolUpdates(nextToolUpdates);
    setSupportedSites(nextSupportedSites);
  }

  async function refreshToolMetadata() {
    if (isRefreshingToolMetadata) {
      return;
    }

    setIsRefreshingToolMetadata(true);

    try {
      await refreshDependencies();
      refreshDeferredStartupData().finally(() => {
        setIsRefreshingToolMetadata(false);
      });
    } catch (caught) {
      setError(readError(caught));
      setIsRefreshingToolMetadata(false);
    }
  }

  async function refreshHistory() {
    try {
      const items = await loadHistory();
      setHistory(items);
    } catch (caught) {
      setError(readError(caught));
    }
  }

  async function removeHistoryItem(item: DownloadHistoryItem, deleteFile: boolean) {
    if (historyDeleteBusy) {
      return;
    }

    setHistoryDeleteBusy(true);

    try {
      await deleteHistoryItem(item.id, deleteFile);
      await refreshHistory();
      setHistoryDeleteTarget(null);
      message.success(deleteFile ? "已删除历史记录和本地文件" : "已删除历史记录");
    } catch (caught) {
      const nextError = readError(caught);
      setError(nextError);
      message.error(nextError);
    } finally {
      setHistoryDeleteBusy(false);
    }
  }

  function openHistoryDeleteModal(item: DownloadHistoryItem) {
    setHistoryDeleteTarget(item);
  }

  function closeHistoryDeleteModal() {
    if (!historyDeleteBusy) {
      setHistoryDeleteTarget(null);
    }
  }

  function cancelAuthOperations() {
    if (!authOperationIdsRef.current.size) {
      return;
    }

    cancelOperationIds(authOperationIdsRef.current);
    authOperationIdsRef.current.clear();
  }

  async function refreshAuthProbe(
    trigger: "auto" | "manual" | "parse" = "manual",
  ): Promise<ProbeResponse | null> {
    const probeUrlValue = firstUrl;
    const requestId = authProbeRequestRef.current + 1;
    authProbeRequestRef.current = requestId;
    cancelAuthOperations();

    if (!probeUrlValue) {
      setAuthProbe({
        status: "idle",
        browser,
        url: "",
      });
      return null;
    }

    const operationId = createOperationId("auth");
    authOperationIdsRef.current.add(operationId);

    setAuthProbe({
      status: "checking",
      browser,
      url: probeUrlValue,
    });
    setIsProbing(true);
    if (trigger !== "auto") {
      setError(null);
    }

    try {
      const result = await probeUrl(probeUrlValue, browser, operationId);
      if (authProbeRequestRef.current !== requestId) {
        return null;
      }
      setProbe(result);
      setSelectedFormat(result.formats[0]?.selector ?? "bv*+ba/b");
      setAuthProbe(authProbeStateFromResult(result, browser, probeUrlValue));
      return result;
    } catch (caught) {
      if (authProbeRequestRef.current !== requestId) {
        return null;
      }
      const nextError = readError(caught);
      const isCanceled = isOperationCanceledError(nextError);
      setProbe(null);
      setAuthProbe({
        status: isCanceled ? "canceled" : "unavailable",
        browser,
        url: probeUrlValue,
        error: isCanceled ? "已停止检测。" : nextError,
      });
      return null;
    } finally {
      authOperationIdsRef.current.delete(operationId);
      if (authProbeRequestRef.current === requestId) {
        setIsProbing(false);
      }
    }
  }

  async function handleAuthProbeRefresh() {
    if (hasQueueAuthProbeTargets) {
      await refreshQueueAuthProbes(queue, browser);
      return;
    }

    await refreshAuthProbe("manual");
  }

  function handleAuthProbeCancel() {
    authProbeRequestRef.current += 1;
    queueAuthProbeBatchRef.current += 1;
    cancelAuthOperations();
    setIsProbing(false);
    setAuthProbe((state) =>
      state.status === "checking"
        ? { ...state, status: "canceled", error: "已停止检测。" }
        : state,
    );
    setQueue((items) =>
      items.map((item) =>
        item.authProbe?.status === "checking"
          ? {
              ...item,
              authProbe: {
                ...item.authProbe,
                status: "canceled",
                error: "已停止检测。",
              },
            }
          : item,
      ),
    );
    message.info("已停止登录态检测");
  }

  async function refreshQueueAuthProbes(
    targetItems = queue,
    targetBrowser = browser,
  ) {
    const candidates = targetItems.filter(queueItemCanAuthProbe);
    const batchId = queueAuthProbeBatchRef.current + 1;
    queueAuthProbeBatchRef.current = batchId;
    cancelAuthOperations();

    if (!candidates.length) {
      return;
    }

    const candidateIds = new Set(candidates.map((item) => item.id));
    setQueue((items) =>
      items.map((item) =>
        candidateIds.has(item.id)
          ? {
              ...item,
              authProbe: {
                status: "checking",
                browser: targetBrowser,
                url: item.url,
                site: item.site,
              },
            }
          : item,
      ),
    );

    let nextIndex = 0;
    const workerCount = Math.min(QUEUE_AUTH_PROBE_CONCURRENCY, candidates.length);
    const workers = Array.from({ length: workerCount }, async () => {
      while (nextIndex < candidates.length) {
        const item = candidates[nextIndex];
        nextIndex += 1;

        if (queueAuthProbeBatchRef.current !== batchId) {
          return;
        }

        const operationId = createOperationId("queue-auth");
        authOperationIdsRef.current.add(operationId);

        try {
          const result = await probeUrl(item.url, targetBrowser, operationId);
          if (queueAuthProbeBatchRef.current !== batchId) {
            return;
          }

          setQueue((items) =>
            items.map((candidate) =>
              candidate.id === item.id
                ? {
                    ...mergeQueueMetadata(candidate, result),
                    authProbe: authProbeStateFromResult(
                      result,
                      targetBrowser,
                      item.url,
                    ),
                  }
                : candidate,
            ),
          );
        } catch (caught) {
          if (queueAuthProbeBatchRef.current !== batchId) {
            return;
          }

          const nextError = readError(caught);
          const isCanceled = isOperationCanceledError(nextError);
          setQueue((items) =>
            items.map((candidate) =>
              candidate.id === item.id
                ? {
                    ...candidate,
                    authProbe: {
                      status: isCanceled ? "canceled" : "unavailable",
                      browser: targetBrowser,
                      url: item.url,
                      site: candidate.site || siteFromUrl(item.url) || item.site,
                      error: isCanceled ? "已停止检测。" : nextError,
                    },
                  }
                : candidate,
            ),
          );
        } finally {
          authOperationIdsRef.current.delete(operationId);
        }
      }
    });

    await Promise.all(workers);
  }

  function handleBrowserChange(value: BrowserKind) {
    queueAuthProbeBatchRef.current += 1;
    authProbeRequestRef.current += 1;
    cancelAuthOperations();
    setBrowser(value);
    setQueue((items) =>
      items.map((item) =>
        item.parseError
          ? {
              ...item,
              authProbe: queueParseErrorAuthProbe(item, value),
            }
          : {
              ...item,
              authProbe: {
                status: "idle",
                browser: value,
                url: item.url,
                site: item.site,
              },
            },
      ),
    );
  }

  function handleAppContextMenu(event: ReactMouseEvent<HTMLElement>) {
    const target = event.target;

    if (
      target instanceof Element &&
      target.closest('input, textarea, [contenteditable="true"]')
    ) {
      return;
    }

    event.preventDefault();
  }

  async function handleStart() {
    await handleParseQueue(true);
    setIsQueuePaused(false);
  }

  async function handleCancel() {
    if (!hasCancelableQueue) {
      return;
    }

    await cancelAllQueueItems();
  }

  async function handleParseQueue(startAfterParse = false) {
    const urls = parseUrlLines(url);
    if (!urls.length) {
      setError("请输入至少一个 http:// 或 https:// 开头的视频地址。");
      return;
    }

    const requestId = parseRequestRef.current + 1;
    const operationId = createOperationId("parse");
    parseRequestRef.current = requestId;
    parseOperationRef.current = operationId;
    queueAuthProbeBatchRef.current += 1;
    cancelAuthOperations();
    setIsParsingQueue(true);
    setError(null);

    try {
      const parsed = await parseDownloadQueue(urls, browser, operationId);
      if (parseRequestRef.current !== requestId) {
        return;
      }
      const nextQueue = parsed.map((item) => {
        const queueItem = queueItemFromParsed(item, browser);
        return startAfterParse && queueItem.status === "idle"
          ? { ...queueItem, status: "queued" as const }
          : queueItem;
      });
      startingTaskRef.current.clear();
      metadataProbeRef.current.clear();
      setFailedThumbnail(null);
      setThumbnailLoadStates({});
      setQueue(nextQueue);
      setStatus("idle");
      setProgress(0);
      setSpeed(null);
      setEta(null);
      setOutputPath(null);
      message.success(`已解析 ${nextQueue.length} 个队列项`);
      void refreshQueueAuthProbes(nextQueue, browser);
    } catch (caught) {
      if (parseRequestRef.current !== requestId) {
        return;
      }
      const nextError = readError(caught);
      if (isOperationCanceledError(nextError)) {
        setError(null);
        message.info("已停止解析");
        return;
      }
      setError(nextError);
      message.error(nextError);
    } finally {
      if (parseRequestRef.current === requestId) {
        setIsParsingQueue(false);
        parseOperationRef.current = null;
      }
    }
  }

  function handleStopParseQueue() {
    const operationId = parseOperationRef.current;
    parseRequestRef.current += 1;
    parseOperationRef.current = null;
    setIsParsingQueue(false);

    if (operationId) {
      void cancelYtdlpOperation(operationId);
    }

    message.info("已停止解析");
  }

  async function handleOpenCleanupModal() {
    if (isScanningCleanup || isCleaningCache) {
      return;
    }

    setIsScanningCleanup(true);
    setError(null);

    try {
      const summary = await scanDownloadCleanup();
      setCleanupSummary(summary);
      setIsCleanupModalOpen(true);
    } catch (caught) {
      const nextError = readError(caught);
      setError(nextError);
      message.error(nextError);
    } finally {
      setIsScanningCleanup(false);
    }
  }

  function closeCleanupModal() {
    if (!isCleaningCache) {
      setIsCleanupModalOpen(false);
    }
  }

  async function handleCleanupCache() {
    if (isCleaningCache) {
      return;
    }

    setIsCleaningCache(true);
    setError(null);

    try {
      const summary = await cleanupDownloadCache();
      setCleanupSummary(summary);
      setIsCleanupModalOpen(false);
      await refreshHistory();
      setQueue((items) =>
        items.filter((item) => !isInvalidQueueAfterCleanup(item)),
      );
      message.success(`清理完成，预计释放 ${formatBytes(summary.bytes)}`);
    } catch (caught) {
      const nextError = readError(caught);
      setError(nextError);
      message.error(nextError);
    } finally {
      setIsCleaningCache(false);
    }
  }

  function handleStartQueue() {
    if (!hasQueueItems) {
      void handleParseQueue(true).then(() => {
        setIsQueuePaused(false);
      });
      return;
    }

    setIsQueuePaused(false);
    setQueue((items) =>
      items.map((item) =>
        item.status === "failed" || item.status === "canceled"
          ? item
          : { ...item, status: item.status === "idle" ? "queued" : item.status },
      ),
    );
  }

  async function handleToggleQueuePause() {
    if (hasResumableQueue) {
      setQueue((items) =>
        items.map((item) =>
          item.status === "paused"
            ? {
                ...item,
                status: "queued",
                speed: null,
                eta: null,
                error: null,
              }
            : item,
        ),
      );
      setIsQueuePaused(false);
      return;
    }

    if (!hasPausableQueue) {
      return;
    }

    setIsQueuePaused(true);
    const runningIds = queue
      .filter((item) => item.status === "running")
      .map((item) => item.id);
    runningIds.forEach((taskId) => startingTaskRef.current.delete(taskId));
    setQueue((items) =>
      items.map((item) =>
        item.status === "queued"
          ? {
              ...item,
              status: "paused",
              speed: null,
              eta: null,
            }
          : item,
      ),
    );

    const results = await Promise.allSettled(
      runningIds.map((taskId) => pauseDownload(taskId)),
    );
    const rejected = results.find((result) => result.status === "rejected");
    if (rejected?.status === "rejected") {
      const nextError = readError(rejected.reason);
      setError(nextError);
      message.error(nextError);
    }
  }

  async function handleCancelQueueItem(item: QueueItem) {
    if (
      item.status === "running" ||
      item.status === "queued" ||
      item.status === "paused" ||
      item.status === "idle"
    ) {
      try {
        await cancelDownload(item.id);
        startingTaskRef.current.delete(item.id);
        setQueue((items) =>
          items.map((candidate) =>
            candidate.id === item.id
              ? {
                  ...candidate,
                  status: "canceled",
                  progress: 0,
                  phase: null,
                  phaseLabel: null,
                  speed: null,
                  eta: null,
                  error: null,
                }
              : candidate,
          ),
        );
      } catch (caught) {
        const nextError = readError(caught);
        setError(nextError);
        message.error(nextError);
      }
      return;
    }
  }

  async function handlePauseQueueItem(item: QueueItem) {
    if (item.status === "queued") {
      startingTaskRef.current.delete(item.id);
      setQueue((items) =>
        items.map((candidate) =>
          candidate.id === item.id
            ? { ...candidate, status: "paused", speed: null, eta: null }
            : candidate,
        ),
      );
      return;
    }

    if (item.status !== "running") {
      return;
    }

    try {
      startingTaskRef.current.delete(item.id);
      await pauseDownload(item.id);
    } catch (caught) {
      const nextError = readError(caught);
      setError(nextError);
      message.error(nextError);
    }
  }

  function handleResumeQueueItem(item: QueueItem) {
    if (item.status !== "paused") {
      return;
    }

    setQueue((items) =>
      items.map((candidate) =>
        candidate.id === item.id
          ? {
              ...candidate,
              status: "queued",
              speed: null,
              eta: null,
              error: null,
            }
          : candidate,
      ),
    );
    setIsQueuePaused(false);
  }

  function handleRetryQueueItem(item: QueueItem) {
    startingTaskRef.current.delete(item.id);
    clearThumbnailLoadState(item);
    setQueue((items) =>
      items.map((candidate) =>
        candidate.id === item.id
          ? {
              ...candidate,
              status: "queued",
              parseError: false,
              authProbe: {
                status: "idle",
                browser,
                url: candidate.url,
                site: candidate.site,
              },
              progress: 0,
              phase: null,
              phaseLabel: null,
              speed: null,
              eta: null,
              outputPath: null,
              localMedia: null,
              mediaComparison: null,
              error: null,
            }
          : candidate,
      ),
    );
    setIsQueuePaused(false);
  }

  function handleRemoveQueueItem(item: QueueItem) {
    startingTaskRef.current.delete(item.id);
    clearThumbnailLoadState(item);
    setQueue((items) => items.filter((candidate) => candidate.id !== item.id));
  }

  function handleThumbnailLoad(item: QueueItem) {
    const thumbnailUrl = item.thumbnail;
    const key = thumbnailLoadKey(item);

    if (!thumbnailUrl || !key) {
      return;
    }

    setThumbnailLoadStates((states) => ({
      ...states,
      [key]: {
        status: "loaded",
        attempts: states[key]?.attempts ?? 0,
      },
    }));
    setFailedThumbnail((failure) =>
      failure?.taskId === item.id && failure.url === thumbnailUrl ? null : failure,
    );
  }

  function handleThumbnailError(item: QueueItem) {
    const thumbnailUrl = item.thumbnail;
    const key = thumbnailLoadKey(item);

    if (!thumbnailUrl || !key) {
      return;
    }

    const failedAttempts =
      failedThumbnail?.taskId === item.id && failedThumbnail.url === thumbnailUrl
        ? failedThumbnail.attempts
        : 0;
    const nextAttempts =
      Math.max(thumbnailLoadStates[key]?.attempts ?? 0, failedAttempts) + 1;

    setThumbnailLoadStates((states) => ({
      ...states,
      [key]: {
        status: nextAttempts >= THUMBNAIL_RETRY_LIMIT ? "failed" : "loading",
        attempts: nextAttempts,
      },
    }));
    setFailedThumbnail({
      taskId: item.id,
      url: thumbnailUrl,
      attempts: nextAttempts,
    });
  }

  function clearThumbnailLoadState(item: QueueItem) {
    const key = thumbnailLoadKey(item);

    if (!key) {
      return;
    }

    setThumbnailLoadStates((states) => {
      const { [key]: _removed, ...rest } = states;
      return rest;
    });
    setFailedThumbnail((failure) =>
      failure?.taskId === item.id && failure.url === item.thumbnail
        ? null
        : failure,
    );
  }

  async function cancelAllQueueItems() {
    const cancelableIds = queue
      .filter((item) =>
        ["running", "queued", "paused", "idle"].includes(item.status),
      )
      .map((item) => item.id);

    setIsQueuePaused(true);
    cancelableIds.forEach((taskId) => startingTaskRef.current.delete(taskId));
    setQueue((items) =>
      items.map((item) =>
        ["running", "queued", "paused", "idle"].includes(item.status)
          ? {
              ...item,
              status: "canceled",
              progress: 0,
              phase: null,
              phaseLabel: null,
              speed: null,
              eta: null,
            }
          : item,
      ),
    );

    const results = await Promise.allSettled(
      cancelableIds.map((taskId) => cancelDownload(taskId)),
    );
    const rejected = results.find((result) => result.status === "rejected");
    if (rejected?.status === "rejected") {
      const nextError = readError(rejected.reason);
      setError(nextError);
      message.error(nextError);
    }
  }

  function scheduleQueuedDownloads() {
    if (schedulingRef.current || isQueuePaused || !outputDir.trim()) {
      return;
    }

    const runningCount = queue.filter((item) => item.status === "running").length;
    const availableSlots = Math.max(0, concurrency - runningCount);
    if (availableSlots === 0) {
      return;
    }

    const nextItems = queue
      .filter(
        (item) =>
          item.status === "queued" &&
          !item.error &&
          !startingTaskRef.current.has(item.id),
      )
      .slice(0, availableSlots);

    if (!nextItems.length) {
      return;
    }

    schedulingRef.current = true;
    nextItems.forEach((item) => {
      startingTaskRef.current.add(item.id);
      void startQueueItem(item);
    });
  }

  async function startQueueItem(item: QueueItem) {
    let downloadItem = item;
    const shouldResumeProgress = item.progress > 0 && item.status === "queued";

    if (queueItemHasPlaceholderTitle(item)) {
      try {
        const result = await probeUrl(item.url, browser);
        downloadItem = mergeQueueMetadata(item, result);
        setQueue((items) =>
          items.map((candidate) =>
            candidate.id === item.id ? mergeQueueMetadata(candidate, result) : candidate,
          ),
        );
      } catch {
        // 标题补全失败不阻断下载，后端仍会从最终文件名兜底历史标题。
      }
    }

    activeTaskRef.current.add(item.id);
    setQueue((items) =>
      items.map((candidate) =>
        candidate.id === item.id
          ? {
              ...candidate,
              status: "running",
              parseError: false,
              progress: shouldResumeProgress ? candidate.progress : 0,
              phase: shouldResumeProgress ? candidate.phase : null,
              phaseLabel: shouldResumeProgress ? candidate.phaseLabel : null,
              speed: null,
              eta: null,
              localMedia: null,
              mediaComparison: null,
              error: null,
            }
          : candidate,
      ),
    );
    setStatus("running");

    try {
      await startDownload({
        taskId: downloadItem.id,
        url: downloadItem.url,
        title: downloadItem.title,
        site: downloadItem.site,
        format: selectedFormat,
        browser,
        outputDir,
        expectedMedia: expectedMediaForQueueItem(
          downloadItem,
          selectedFormat,
          formatOptions,
        ),
      });
    } catch (caught) {
      const nextError = readError(caught);
      startingTaskRef.current.delete(item.id);
      setQueue((items) =>
        items.map((candidate) =>
          candidate.id === item.id
            ? {
                ...candidate,
                status: "failed",
                parseError: false,
                progress: 0,
                phase: null,
                phaseLabel: null,
                error: nextError,
              }
            : candidate,
        ),
      );
      setStatus("failed");
      setError(nextError);
    } finally {
      schedulingRef.current = false;
      window.setTimeout(scheduleQueuedDownloads, 0);
    }
  }

  async function handleChooseDirectory() {
    const selected = await selectDirectory();
    if (selected) {
      setOutputDir(selected);
    }
  }

  async function handleChooseFfmpegInputFile() {
    const selected = await selectMediaFile("选择输入视频");
    if (selected) {
      setFfmpegInputPath(selected);
    }
  }

  async function handleChooseFfmpegSecondaryFile() {
    const selected = await selectMediaFile("选择音频文件");
    if (selected) {
      setFfmpegSecondaryInputPath(selected);
    }
  }

  async function handleChooseFfmpegOutputDirectory() {
    const selected = await selectDirectory();
    if (selected) {
      setFfmpegOutputDir(selected);
    }
  }

  function handleUseDownloadedFile() {
    if (!outputPath) {
      return;
    }

    setFfmpegInputPath(outputPath);
    setFfmpegOutputDir(parentPath(outputPath) ?? ffmpegOutputDir);
  }

  async function handleBuildFfmpegDraft() {
    const request = currentFfmpegCommandRequest();

    try {
      const draft = await createFfmpegDraft(request);
      await appendGeneratedFfmpegCommand(request, draft);
    } catch (caught) {
      if (caught instanceof FfmpegHistorySaveError) {
        setFfmpegNotice({
          type: "warning",
          text: `命令已生成，但历史保存失败：${caught.message}`,
        });
        return;
      }
      // createFfmpegDraft already exposes the error in the tool panel.
    }
  }

  async function handleCopyFfmpegCommand() {
    let draft: FfmpegCommandDraft;

    try {
      draft = await createFfmpegDraft();
    } catch {
      return;
    }

    try {
      await copyTextToClipboard(draft.command);
      setFfmpegNotice({ type: "success", text: "命令已复制。" });
    } catch (caught) {
      setFfmpegNotice({ type: "warning", text: readError(caught) });
    }
  }

  async function handlePrefillTerminal() {
    setIsPrefillingTerminal(true);

    try {
      const draft = await createFfmpegDraft();
      const result = await prefillTerminalCommand(
        draft.command,
        draft.workingDir,
      );
      setFfmpegNotice({
        type: result.prefilled ? "success" : "warning",
        text: result.message,
      });
    } catch (caught) {
      setFfmpegNotice({ type: "error", text: readError(caught) });
    } finally {
      setIsPrefillingTerminal(false);
    }
  }

  function currentFfmpegCommandRequest(): FfmpegCommandRequest {
    return {
      presetId: ffmpegPreset,
      inputPath: ffmpegInputPath.trim(),
      secondaryInputPath: ffmpegSecondaryInputPath.trim() || null,
      outputDir: ffmpegOutputDir.trim(),
      audioFormat: ffmpegAudioFormat,
      crf: ffmpegCrf,
      startTime: ffmpegStartTime.trim() || null,
      endTime: ffmpegEndTime.trim() || null,
    };
  }

  async function createFfmpegDraft(
    request = currentFfmpegCommandRequest(),
  ): Promise<FfmpegCommandDraft> {
    if (!canBuildFfmpegCommand) {
      throw new Error(
        ffmpegPreset === "mergeAudioVideo"
          ? "请选择输入视频、音频文件和输出目录。"
          : "请选择输入文件和输出目录。",
      );
    }

    setIsBuildingFfmpeg(true);

    try {
      const draft = await buildFfmpegCommand(request);
      setFfmpegDraft(draft);
      setFfmpegNotice({ type: "success", text: "命令已生成。" });
      return draft;
    } catch (caught) {
      setFfmpegNotice({ type: "error", text: readError(caught) });
      throw caught;
    } finally {
      setIsBuildingFfmpeg(false);
    }
  }

  async function appendGeneratedFfmpegCommand(
    request: FfmpegCommandRequest,
    draft: FfmpegCommandDraft,
  ) {
    setIsMutatingFfmpegCommandHistory(true);

    try {
      const items = await appendFfmpegCommandHistory({
        ...request,
        command: draft.command,
        workingDir: draft.workingDir,
        outputPath: draft.outputPath,
      });
      setFfmpegCommandHistory(items);
      setHasLoadedFfmpegCommandHistory(true);
    } catch (caught) {
      throw new FfmpegHistorySaveError(readError(caught));
    } finally {
      setIsMutatingFfmpegCommandHistory(false);
    }
  }

  async function refreshFfmpegCommandHistory() {
    setIsLoadingFfmpegCommandHistory(true);

    try {
      const items = await loadFfmpegCommandHistory();
      setFfmpegCommandHistory(items);
      setHasLoadedFfmpegCommandHistory(true);
    } catch (caught) {
      setFfmpegNotice({
        type: "warning",
        text: `读取命令历史失败：${readError(caught)}`,
      });
      setHasLoadedFfmpegCommandHistory(true);
    } finally {
      setIsLoadingFfmpegCommandHistory(false);
    }
  }

  async function handleDeleteFfmpegCommandHistoryItems(ids: string[]) {
    if (ids.length === 0 || isMutatingFfmpegCommandHistory) {
      return;
    }

    setIsMutatingFfmpegCommandHistory(true);

    try {
      const items = await deleteFfmpegCommandHistoryItems(ids);
      setFfmpegCommandHistory(items);
      setHasLoadedFfmpegCommandHistory(true);
    } catch (caught) {
      setFfmpegNotice({
        type: "warning",
        text: `删除命令历史失败：${readError(caught)}`,
      });
    } finally {
      setIsMutatingFfmpegCommandHistory(false);
    }
  }

  async function handleClearFfmpegCommandHistory() {
    if (isMutatingFfmpegCommandHistory) {
      return;
    }

    setIsMutatingFfmpegCommandHistory(true);

    try {
      await clearFfmpegCommandHistory();
      setFfmpegCommandHistory([]);
      setHasLoadedFfmpegCommandHistory(true);
    } catch (caught) {
      setFfmpegNotice({
        type: "warning",
        text: `清空命令历史失败：${readError(caught)}`,
      });
    } finally {
      setIsMutatingFfmpegCommandHistory(false);
    }
  }

  function applyFfmpegCommandHistoryItem(item: FfmpegCommandHistoryItem) {
    if (!isKnownFfmpegPresetId(item.presetId)) {
      setFfmpegNotice({
        type: "warning",
        text: "当前版本不支持这条历史命令的预设，无法填回表单。",
      });
      return;
    }

    keepNextFfmpegDraftResetRef.current = true;
    setFfmpegPreset(item.presetId);
    setFfmpegInputPath(item.inputPath);
    setFfmpegSecondaryInputPath(item.secondaryInputPath ?? "");
    setFfmpegOutputDir(item.outputDir);
    setFfmpegAudioFormat(item.audioFormat ?? "mp3");
    setFfmpegCrf(item.crf ?? 28);
    setFfmpegStartTime(item.startTime ?? "00:00:00");
    setFfmpegEndTime(item.endTime ?? "00:00:30");
    setFfmpegDraft({
      command: item.command,
      workingDir: item.workingDir,
      outputPath: item.outputPath,
    });
    setFfmpegNotice({ type: "success", text: "已填回历史命令。" });
  }

  async function handleCopyFfmpegHistoryCommand(command: string) {
    try {
      await copyTextToClipboard(command);
      setFfmpegNotice({ type: "success", text: "历史命令已复制。" });
    } catch (caught) {
      setFfmpegNotice({ type: "warning", text: readError(caught) });
    }
  }

  async function handleChooseToolPath(tool: ToolName) {
    setError(null);
    const selected = await selectToolExecutable(tool);
    if (!selected) {
      return;
    }

    try {
      const nextToolSettings = await saveToolPath(tool, selected);
      setToolSettings(nextToolSettings);
      await refreshToolMetadata();
    } catch (caught) {
      setError(readError(caught));
    }
  }

  async function handleClearToolPath(tool: ToolName) {
    setError(null);

    try {
      const nextToolSettings = await clearToolPath(tool);
      setToolSettings(nextToolSettings);
      await refreshToolMetadata();
    } catch (caught) {
      setError(readError(caught));
    }
  }

  async function handleSaveProxySettings(
    mode: ProxyMode,
    proxyUrl?: string | null,
  ) {
    setError(null);

    try {
      const nextToolSettings = await saveProxySettings(mode, proxyUrl);
      setToolSettings(nextToolSettings);
      await refreshDependencies();
      message.success("已保存代理设置");
    } catch (caught) {
      const nextError = readError(caught);
      setError(nextError);
      message.error(nextError);
    }
  }

  function handleProgress(event: ProgressEvent) {
    if (
      !activeTaskRef.current.has(event.taskId) &&
      !queueItemIdsRef.current.has(event.taskId)
    ) {
      return;
    }

    const safeProgress = Math.max(0, Math.min(100, event.progress));
    setQueue((items) =>
      items.map((item) =>
        item.id === event.taskId
          ? {
              ...item,
              status: event.status,
              progress: safeProgress,
              phase: event.phase ?? null,
              phaseLabel: event.phaseLabel ?? null,
              speed: event.speed ?? null,
              eta: event.eta ?? null,
              outputPath: event.outputPath ?? item.outputPath ?? null,
              localMedia: event.localMedia ?? item.localMedia ?? null,
              mediaComparison:
                event.mediaComparison ?? item.mediaComparison ?? null,
              error: event.error ?? null,
            }
          : item,
      ),
    );
    setStatus(event.status);
    setProgress(safeProgress);
    setSpeed(event.speed ?? null);
    setEta(event.eta ?? null);
    setOutputPath(event.outputPath ?? null);
    setError(event.error ?? null);

    if (["completed", "failed", "canceled", "paused"].includes(event.status)) {
      startingTaskRef.current.delete(event.taskId);
      refreshHistory();
      if (event.status !== "paused") {
        window.setTimeout(scheduleQueuedDownloads, 0);
      }
    }
  }

  function applyHistoryItem(item: DownloadHistoryItem) {
    const historyUrl = item.url?.trim() ?? "";

    queueAuthProbeBatchRef.current += 1;
    setUrl(historyUrl);
    setOutputDir(item.outputDir || FALLBACK_OUTPUT_DIR);
    setSelectedFormat(item.format || "bv*+ba/b");
    setFailedThumbnail(null);
    setThumbnailLoadStates({});
    setQueue(
      historyUrl
        ? [
            {
              id: crypto.randomUUID(),
              url: historyUrl,
              title: historyTitle(item),
              site: historySite(item),
              duration: null,
              thumbnail: null,
              sourceUrl: null,
              playlistTitle: null,
              playlistIndex: null,
              playlistTotal: null,
              sourceOrder: null,
              isPlaylistItem: false,
              status: "idle",
              parseError: false,
              authProbe: {
                status: "idle",
                browser,
                url: historyUrl,
                site: historySite(item),
              },
              progress: 0,
              phase: null,
              phaseLabel: null,
              localMedia: null,
              mediaComparison: null,
              error: null,
            },
          ]
        : [],
    );
    setActiveView("download");
  }

  async function openHistoryItem(item: DownloadHistoryItem) {
    const outputPath = item.outputPath?.trim();

    if (outputPath) {
      try {
        await revealFile(outputPath);
        return;
      } catch (caught) {
        message.warning("本地文件不存在，已填入下载信息");
      }
    }

    applyHistoryItem(item);
  }

  function applySupportedSiteExample(example: SupportedSiteExample) {
    queueAuthProbeBatchRef.current += 1;
    setUrl(example.url);
    setQueue([]);
  }

  function handleHistoryKeyDown(
    event: KeyboardEvent<HTMLDivElement>,
    item: DownloadHistoryItem,
  ) {
    if (event.key !== "Enter" && event.key !== " ") {
      return;
    }

    event.preventDefault();
    void openHistoryItem(item);
  }

  const historyDeleteTitle = historyDeleteTarget
    ? historyTitle(historyDeleteTarget)
    : "";
  const historyDeleteOutputPath = historyDeleteTarget?.outputPath?.trim() ?? "";
  const cleanupHasItems = cleanupSummary
    ? cleanupSummary.fileCount > 0 ||
      cleanupSummary.directoryCount > 0 ||
      cleanupSummary.invalidHistoryCount > 0
    : false;

  return (
    <Layout className="app-shell" onContextMenu={handleAppContextMenu}>
      <Sider
        className="history-rail"
        collapsed={sidebarCollapsed}
        collapsedWidth={84}
        trigger={null}
        width={292}
      >
        <div className="brand-lockup">
          {!sidebarCollapsed ? (
            <>
              <div className="brand-mark">
                <img draggable={false} src={appIconUrl} alt="视频下载器" />
              </div>
              <div className="brand-copy">
                <Title level={1}>视频下载器</Title>
              </div>
            </>
          ) : null}
          <Tooltip title={sidebarCollapsed ? "展开侧边栏" : "收起侧边栏"}>
            <Button
              aria-label={sidebarCollapsed ? "展开侧边栏" : "收起侧边栏"}
              className="sidebar-toggle"
              icon={sidebarCollapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
              onClick={() => setSidebarCollapsed((collapsed) => !collapsed)}
              shape="circle"
              type="text"
            />
          </Tooltip>
        </div>

        <nav className="sidebar-nav" aria-label="主导航">
          {sidebarNavItems.map((item) => (
            <Tooltip
              key={item.key}
              placement="right"
              title={sidebarCollapsed ? item.label : ""}
            >
              <Button
                aria-label={item.label}
                className={activeView === item.key ? "active" : ""}
                icon={item.icon}
                onClick={() => setActiveView(item.key)}
                type="text"
              >
                {!sidebarCollapsed ? item.label : null}
              </Button>
            </Tooltip>
          ))}
        </nav>

        {!sidebarCollapsed ? (
          <SidebarHistoryPreview
            history={history}
            onHistoryKeyDown={handleHistoryKeyDown}
            onOpenHistoryItem={openHistoryItem}
            onOpenHistory={() => setActiveView("history")}
            onRefresh={refreshHistory}
          />
        ) : null}

        <SidebarDependencyPanel
          collapsed={sidebarCollapsed}
          dependencies={dependencies}
          isRefreshing={isRefreshingToolMetadata}
          onChooseToolPath={handleChooseToolPath}
          onClearToolPath={handleClearToolPath}
          onRefresh={refreshToolMetadata}
          onSaveProxySettings={handleSaveProxySettings}
          toolSettings={toolSettings}
          toolUpdates={toolUpdates}
        />
      </Sider>

      <Layout className="main-layout">
        <Content className="workspace">
          <header className="topbar">
            <div className="page-heading">
              <div className="page-heading-mark">
                {pageHeading.icon}
              </div>
              <div className="page-heading-copy">
                <Title level={2}>{pageHeading.title}</Title>
              </div>
            </div>
          </header>

          {activeView === "download" ? (
          <div className="download-workbench">
          <div className="download-main-column">
          <Card className="download-card">
            <Space className="form-stack" direction="vertical" size={16}>
              <div className="field-block">
                <div className="field-label-row">
                  <Text strong>地址</Text>
                  {supportedSites ? (
                    <Text className="supported-sites-meta" type="secondary">
                      {supportedSitesMeta(supportedSites)}
                    </Text>
                  ) : null}
                </div>
                <div className="url-command-row">
                  <div className="url-input-wrap">
                    <span className="url-input-icon">
                      <LinkOutlined />
                    </span>
                    <Input.TextArea
                      autoSize={{ minRows: 4, maxRows: 8 }}
                      className="url-textarea"
                      id="video-url"
                      onChange={(event) => {
                        const nextUrl = event.target.value;
                        queueAuthProbeBatchRef.current += 1;
                        authProbeRequestRef.current += 1;
                        cancelAuthOperations();
                        setUrl(nextUrl);
                        setQueue([]);
                        setAuthProbe({
                          status: "idle",
                          browser,
                          url: parseUrlLines(nextUrl)[0] ?? "",
                        });
                      }}
                      placeholder="每行一个视频或播放列表链接"
                      value={url}
                    />
                  </div>
                  <Button
                    className={`url-probe-button ${isParsingQueue ? "is-stop" : ""}`}
                    danger={isParsingQueue}
                    disabled={!isParsingQueue && !url.trim()}
                    icon={isParsingQueue ? <StopOutlined /> : <SearchOutlined />}
                    onClick={() =>
                      isParsingQueue ? handleStopParseQueue() : handleParseQueue()
                    }
                    type={isParsingQueue ? "default" : "primary"}
                  >
                    {isParsingQueue ? "停止解析" : parseActionLabel}
                  </Button>
                </div>
                <Text className="batch-helper" type="secondary">
                  每行一个链接。播放列表会自动拆分为单个视频任务。
                </Text>
                <SupportedSiteExamples
                  examples={supportedSites?.examples ?? []}
                  onChoose={applySupportedSiteExample}
                />
              </div>

              <div className="form-grid download-options-grid">
                <div className="field-block">
                  <div className="field-label-row auth-probe-label-row">
                    <Text strong>登录态</Text>
                    <AuthProbePanel
                      browser={browser}
                      canRefresh={
                        hasQueueItems ? hasQueueAuthProbeTargets : Boolean(firstUrl)
                      }
                      isChecking={
                        (isProbing && authProbe.status === "checking") ||
                        isQueueAuthChecking
                      }
                      onRefresh={() => {
                        void handleAuthProbeRefresh();
                      }}
                      onCancel={handleAuthProbeCancel}
                      queueSummary={queueAuthSummary}
                      state={authProbe}
                    />
                  </div>
                  <Segmented
                    block
                    onChange={(value) => handleBrowserChange(value as BrowserKind)}
                    options={browserOptions}
                    value={browser}
                  />
                </div>

                <div className="field-block">
                  <Text strong>格式</Text>
                  <Select<string>
                    className="control-select"
                    id="format-select"
                    onChange={setSelectedFormat}
                    optionFilterProp="label"
                    options={formatSelectOptions}
                    value={selectedFormat}
                  />
                </div>

                <div className="field-block">
                  <Text strong>并发数</Text>
                  <Select<number>
                    className="control-select"
                    onChange={setConcurrency}
                    options={concurrencyOptions}
                    value={concurrency}
                  />
                </div>
              </div>

              <div className="field-block save-field">
                <Text strong>保存到</Text>
                <div className="path-command-row">
                  <div className="path-input-wrap">
                    <span className="path-input-icon">
                      <FolderOpenOutlined />
                    </span>
                    <Input
                      className="path-input"
                      id="output-dir"
                      onChange={(event) => setOutputDir(event.target.value)}
                      value={outputDir}
                    />
                  </div>
                  <Tooltip title="选择保存目录">
                    <Button
                      className="path-picker-button"
                      aria-label="选择保存目录"
                      onClick={handleChooseDirectory}
                      type="primary"
                    >
                      选择
                    </Button>
                  </Tooltip>
                </div>
              </div>

              <Space className="command-row" size={14} wrap>
                <Button
                  className="queue-command-button is-start"
                  disabled={
                    (hasQueueItems ? !hasIdleQueue : !url.trim()) ||
                    !outputDir.trim() ||
                    hasRunningQueue
                  }
                  icon={<PlayCircleOutlined />}
                  onClick={hasQueueItems ? handleStartQueue : handleStart}
                  type="primary"
                >
                  {startActionLabel}
                </Button>
                <Button
                  className="queue-command-button is-pause"
                  disabled={!hasPausableQueue && !hasResumableQueue}
                  icon={hasResumableQueue ? <PlayCircleOutlined /> : <PauseCircleOutlined />}
                  onClick={handleToggleQueuePause}
                >
                  {pauseActionLabel}
                </Button>
                <Button
                  className="queue-command-button is-cancel"
                  danger
                  disabled={!hasCancelableQueue}
                  icon={<PoweroffOutlined />}
                  onClick={handleCancel}
                >
                  取消全部
                </Button>
                <Button
                  className="queue-command-button is-clear"
                  icon={<DeleteOutlined />}
                  loading={isScanningCleanup}
                  onClick={handleOpenCleanupModal}
                >
                  清理缓存
                </Button>
              </Space>
            </Space>
          </Card>

          {currentQueueItem ? (
            <Card
              className="download-status-card"
              title={<PanelTitle icon={<DashboardOutlined />} label="下载状态" />}
            >
              <div className="download-status-strip">
                <div className="download-status-media">
                  <div className="download-status-cover">
                    {currentQueueItem.thumbnail &&
                    currentThumbnailState?.canRenderImage ? (
                      <>
                        <img
                          alt={currentQueueItem.title}
                          className={
                            currentThumbnailState.showImage ? "is-loaded" : ""
                          }
                          draggable={false}
                          key={`${currentQueueItem.id}:${currentQueueItem.thumbnail}:${currentThumbnailState.attempts}`}
                          onError={() => {
                            handleThumbnailError(currentQueueItem);
                          }}
                          onLoad={() => {
                            handleThumbnailLoad(currentQueueItem);
                          }}
                          referrerPolicy="no-referrer"
                          src={thumbnailImageSrc(
                            currentQueueItem,
                            failedThumbnail,
                          )}
                        />
                        {!currentThumbnailState.showImage ? (
                          <ThumbnailPlaceholder state={currentThumbnailState} />
                        ) : null}
                      </>
                    ) : (
                      <ThumbnailPlaceholder state={currentThumbnailState} />
                    )}
                  </div>
                </div>

                <div className="download-status-content">
                  <div className="download-status-heading">
                    <Space size={8} wrap>
                      <Tag color={queueStatusColor(currentQueueItem.status)}>
                        {queueStatusCopy(currentQueueItem.status)}
                      </Tag>
                      {currentQueueItem.isPlaylistItem ? (
                        <Tag className="queue-source-tag">
                          {queuePlaylistTagText(currentQueueItem)}
                        </Tag>
                      ) : null}
                    </Space>
                    {currentQueueItem.outputPath ? (
                      <Button
                        className="download-status-open-button"
                        icon={<ExportOutlined />}
                        onClick={() => revealFile(currentQueueItem.outputPath as string)}
                      >
                        打开位置
                      </Button>
                    ) : null}
                  </div>

                  <Tooltip title={currentQueueItem.title}>
                    <Text className="download-status-title">
                      {currentQueueItem.title}
                    </Text>
                  </Tooltip>

                  <div className="download-status-meta-row">
                    <div className="download-status-meta">
                      <Text type="secondary">{currentQueueItem.site || "-"}</Text>
                      {currentQueueItem.duration ? (
                        <Text type="secondary">
                          {formatDuration(currentQueueItem.duration)}
                        </Text>
                      ) : null}
                    </div>
                    <Text className="download-status-runtime" type="secondary">
                      {`${currentQueueItem.speed ?? "-"} / ${currentQueueItem.eta ?? "-"}`}
                    </Text>
                    <div className="download-status-progress-summary">
                      <span aria-hidden="true" className="download-status-phase-dot" />
                      <Text className="download-status-phase-value" strong>
                        {currentQueueItem.phaseLabel ??
                          queueStatusCopy(currentQueueItem.status)}
                      </Text>
                      <Text className="download-status-progress-value" strong>
                        {Math.round(currentQueueItem.progress)}%
                      </Text>
                    </div>
                  </div>

                  {currentLocalMediaText ? (
                    <div className="download-status-media-info">
                      <Tooltip title={currentLocalMediaTitle}>
                        <Text className="download-status-media-summary" type="secondary">
                          {currentLocalMediaText}
                        </Text>
                      </Tooltip>
                      {currentMediaComparisonText ? (
                        <Text className="download-status-media-comparison" type="secondary">
                          {currentMediaComparisonText}
                        </Text>
                      ) : null}
                    </div>
                  ) : null}

                  <div className="download-status-progress-row">
                    <Progress
                      percent={Math.round(currentQueueItem.progress)}
                      showInfo={false}
                      size="small"
                      status={queueProgressStatus(currentQueueItem.status)}
                    />
                  </div>
                </div>
              </div>
            </Card>
          ) : null}

          <Card
            className="queue-card"
            title={<PanelTitle icon={<DownloadOutlined />} label="下载队列" />}
            extra={
              <Text type="secondary">
                {queueSummary.running} 运行中 / {queue.length} 总计
              </Text>
            }
          >
            {queue.length === 0 ? (
              <Empty
                description="解析后的视频会出现在这里"
                image={Empty.PRESENTED_IMAGE_SIMPLE}
              />
            ) : (
              <List
                className="queue-list"
                dataSource={queue}
                renderItem={(item) => {
                  const mediaText = localMediaSummary(item.localMedia);
                  const mediaTitle = localMediaErrorTitle(item.localMedia);
                  const comparisonText = mediaComparisonSummary(
                    item.mediaComparison,
                  );

                  return (
                    <List.Item className="queue-item" key={item.id}>
                    <div className="queue-item-main">
                      <div className="queue-title-row">
                        <Tag className={`queue-status-tag is-${item.status}`}>
                          {queueStatusCopy(item.status)}
                        </Tag>
                        <Tooltip title={item.title}>
                          <Text className="queue-title">{item.title}</Text>
                        </Tooltip>
                      </div>
                      <Text className="queue-meta" type="secondary">
                        {item.site || "-"} · {item.url}
                      </Text>
                      <QueueAuthProbeLine item={item} />
                      {item.isPlaylistItem ? (
                        <div className="queue-playlist-meta">
                          <Tag className="queue-playlist-tag">
                            {queuePlaylistPositionText(item)}
                          </Tag>
                          {queueSourceOrderText(item) ? (
                            <Tag className="queue-source-tag">
                              {queueSourceOrderText(item)}
                            </Tag>
                          ) : null}
                          {item.playlistTitle?.trim() ? (
                            <Text className="queue-playlist-title" type="secondary">
                              {item.playlistTitle.trim()}
                            </Text>
                          ) : null}
                        </div>
                      ) : null}
                      {queueRuntimeMeta(item) ? (
                        <Text className="queue-runtime-meta" type="secondary">
                          {queueRuntimeMeta(item)}
                        </Text>
                      ) : null}
                      {mediaText ? (
                        <Tooltip title={mediaTitle}>
                          <Text className="queue-media-meta" type="secondary">
                            {mediaText}
                          </Text>
                        </Tooltip>
                      ) : null}
                      {comparisonText ? (
                        <Text className="queue-media-comparison" type="secondary">
                          {comparisonText}
                        </Text>
                      ) : null}
                      {item.error ? (
                        <Text className="queue-error" type="danger">
                          {item.error}
                        </Text>
                      ) : null}
                      <Progress
                        percent={Math.round(item.progress)}
                        showInfo={false}
                        size="small"
                        status={queueProgressStatus(item.status)}
                      />
                    </div>
                    <div className="queue-item-side">
                      <Text strong>{Math.round(item.progress)}%</Text>
                      <Space size={8} wrap>
                        {item.outputPath ? (
                          <Tooltip title="打开文件位置">
                            <Button
                              aria-label={`打开文件位置 ${item.title}`}
                              className="history-action-button"
                              icon={<FolderOpenOutlined />}
                              onClick={() => revealFile(item.outputPath as string)}
                            />
                          </Tooltip>
                        ) : null}
                        {item.status === "failed" || item.status === "canceled" ? (
                          <Tooltip title="重试">
                            <Button
                              aria-label={`重试 ${item.title}`}
                              className="history-action-button"
                              icon={<RetweetOutlined />}
                              onClick={() => handleRetryQueueItem(item)}
                            />
                          </Tooltip>
                        ) : null}
                        {item.status === "running" || item.status === "queued" ? (
                          <Tooltip title="暂停">
                            <Button
                              aria-label={`暂停 ${item.title}`}
                              className="history-action-button queue-pause-button"
                              icon={<PauseCircleOutlined />}
                              onClick={() => {
                                void handlePauseQueueItem(item);
                              }}
                            />
                          </Tooltip>
                        ) : null}
                        {item.status === "paused" ? (
                          <Tooltip title="继续">
                            <Button
                              aria-label={`继续 ${item.title}`}
                              className="history-action-button"
                              icon={<PlayCircleOutlined />}
                              onClick={() => handleResumeQueueItem(item)}
                            />
                          </Tooltip>
                        ) : null}
                        {["running", "queued", "paused"].includes(item.status) ? (
                          <Tooltip title="取消">
                            <Button
                              aria-label={`取消 ${item.title}`}
                              className="history-action-button queue-danger-button"
                              icon={<PoweroffOutlined />}
                              onClick={() => handleCancelQueueItem(item)}
                            />
                          </Tooltip>
                        ) : null}
                        {!["running", "queued", "paused"].includes(item.status) ? (
                          <Popconfirm
                            cancelText="取消"
                            description="任务会从队列中移除，已下载文件不会被删除。"
                            okButtonProps={{ danger: true }}
                            okText="移除"
                            onConfirm={() => handleRemoveQueueItem(item)}
                            placement="topRight"
                            title="移除这个下载任务？"
                          >
                            <Button
                              aria-label={`移除 ${item.title}`}
                              className="history-action-button queue-remove-button"
                              icon={<DeleteOutlined />}
                            />
                          </Popconfirm>
                        ) : null}
                      </Space>
                    </div>
                    </List.Item>
                  );
                }}
              />
            )}
          </Card>

          </div>
          <aside className="download-status-rail">
            <Card
              className="status-side-card"
              title={<PanelTitle icon={<DashboardOutlined />} label="状态概览" />}
            >
              <div className="status-count-grid">
                <StatusCount label="已完成" value={queueSummary.completed} tone="success" />
                <StatusCount label="下载中" value={queueSummary.running} tone="processing" />
                <StatusCount label="已暂停" value={queueSummary.paused} tone="waiting" />
                <StatusCount label="等待中" value={queueSummary.queued} tone="waiting" />
                <StatusCount label="失败" value={queueSummary.failed} tone="danger" />
                <StatusCount label="已解析" value={queueSummary.idle} tone="neutral" />
              </div>
            </Card>
          </aside>
          </div>
          ) : null}

          {activeView === "ffmpeg" ? (
            <Suspense fallback={<LazyViewFallback />}>
              <FfmpegView
                audioFormat={ffmpegAudioFormat}
                canBuildCommand={canBuildFfmpegCommand}
                crf={ffmpegCrf}
                draft={ffmpegDraft}
                endTime={ffmpegEndTime}
                inputPath={ffmpegInputPath}
                isBuilding={isBuildingFfmpeg}
                isHistoryLoading={isLoadingFfmpegCommandHistory}
                isHistoryMutating={isMutatingFfmpegCommandHistory}
                isPrefillingTerminal={isPrefillingTerminal}
                notice={ffmpegNotice}
                onAudioFormatChange={setFfmpegAudioFormat}
                onBuildDraft={handleBuildFfmpegDraft}
                onChooseInputFile={handleChooseFfmpegInputFile}
                onChooseOutputDirectory={handleChooseFfmpegOutputDirectory}
                onChooseSecondaryFile={handleChooseFfmpegSecondaryFile}
                onClearHistory={handleClearFfmpegCommandHistory}
                onCopyCommand={handleCopyFfmpegCommand}
                onCopyHistoryCommand={handleCopyFfmpegHistoryCommand}
                onCrfChange={setFfmpegCrf}
                onDeleteHistoryItems={handleDeleteFfmpegCommandHistoryItems}
                onEndTimeChange={setFfmpegEndTime}
                onInputPathChange={setFfmpegInputPath}
                onOutputDirChange={setFfmpegOutputDir}
                onPrefillTerminal={handlePrefillTerminal}
                onPresetChange={setFfmpegPreset}
                onRefreshHistory={refreshFfmpegCommandHistory}
                onSecondaryInputPathChange={setFfmpegSecondaryInputPath}
                onStartTimeChange={setFfmpegStartTime}
                onUseHistoryItem={applyFfmpegCommandHistoryItem}
                onUseDownloadedFile={handleUseDownloadedFile}
                outputDir={ffmpegOutputDir}
                outputPath={outputPath}
                preset={ffmpegPreset}
                commandHistory={ffmpegCommandHistory}
                secondaryInputPath={ffmpegSecondaryInputPath}
                startTime={ffmpegStartTime}
              />
            </Suspense>
          ) : null}

          {activeView === "history" ? (
            <Suspense fallback={<LazyViewFallback />}>
              <HistoryView
                history={history}
                onApplyHistoryItem={applyHistoryItem}
                onDeleteHistoryItem={openHistoryDeleteModal}
                onHistoryKeyDown={handleHistoryKeyDown}
                onOpenHistoryItem={openHistoryItem}
                onRefresh={refreshHistory}
              />
            </Suspense>
          ) : null}

          {error ? (
            <Alert
              className="error-banner"
              message={error}
              showIcon
              type="error"
            />
          ) : null}
        </Content>
      </Layout>
      <Modal
        centered
        destroyOnClose
        footer={
          historyDeleteTarget ? (
            <div className="history-delete-actions">
              <Button
                disabled={historyDeleteBusy}
                onClick={closeHistoryDeleteModal}
              >
                取消
              </Button>
              <Button
                loading={historyDeleteBusy}
                onClick={() => removeHistoryItem(historyDeleteTarget, false)}
                type="primary"
              >
                {historyDeleteOutputPath ? "仅删除记录" : "删除记录"}
              </Button>
              {historyDeleteOutputPath ? (
                <Button
                  danger
                  loading={historyDeleteBusy}
                  onClick={() => removeHistoryItem(historyDeleteTarget, true)}
                  type="primary"
                >
                  删除记录和本地文件
                </Button>
              ) : null}
            </div>
          ) : null
        }
        onCancel={closeHistoryDeleteModal}
        open={Boolean(historyDeleteTarget)}
        title="删除这条历史记录？"
      >
        {historyDeleteTarget ? (
          <DeleteHistoryConfirmContent
            outputPath={historyDeleteOutputPath}
            title={historyDeleteTitle}
          />
        ) : null}
      </Modal>
      <Modal
        centered
        destroyOnClose
        okButtonProps={{ disabled: !cleanupHasItems, danger: true }}
        okText="确认清理"
        onCancel={closeCleanupModal}
        onOk={handleCleanupCache}
        open={isCleanupModalOpen}
        confirmLoading={isCleaningCache}
        title="清理下载缓存？"
      >
        <CleanupConfirmContent summary={cleanupSummary} />
      </Modal>
    </Layout>
  );
}

function SidebarHistoryPreview({
  history,
  onHistoryKeyDown,
  onOpenHistoryItem,
  onOpenHistory,
  onRefresh,
}: {
  history: DownloadHistoryItem[];
  onHistoryKeyDown: (
    event: KeyboardEvent<HTMLDivElement>,
    item: DownloadHistoryItem,
  ) => void;
  onOpenHistoryItem: (item: DownloadHistoryItem) => void;
  onOpenHistory: () => void;
  onRefresh: () => void;
}) {
  const previewItems = history.slice(0, 3);

  return (
    <section className="history-preview">
      <div className="history-title">
        <Space size={8}>
          <ClockCircleOutlined />
          <Text>最近记录</Text>
        </Space>
        <Tooltip title="刷新历史记录">
          <Button
            aria-label="刷新历史记录"
            icon={<ReloadOutlined />}
            onClick={onRefresh}
            shape="circle"
            type="text"
          />
        </Tooltip>
      </div>

      <div className="history-scroll">
        {previewItems.length === 0 ? (
          <Empty
            className="history-empty"
            description="暂无记录"
            image={Empty.PRESENTED_IMAGE_SIMPLE}
          />
        ) : (
          <List
            dataSource={previewItems}
            renderItem={(item, index) => {
              const title = historyTitle(item);
              const site = historySite(item);
              const itemStatus = statusCopy[item.status] ?? "未知状态";
              const mediaText = localMediaSummary(item.localMedia, "");

              return (
                <List.Item
                  className="history-item"
                  key={item.id || `${item.url}-${index}`}
                  onClick={() => onOpenHistoryItem(item)}
                  onKeyDown={(event) => onHistoryKeyDown(event, item)}
                  role="button"
                  tabIndex={0}
                >
                  <Text className="history-name">{title}</Text>
                  <Text className="history-meta">
                    {[site, itemStatus, mediaText].filter(Boolean).join(" · ")}
                  </Text>
                </List.Item>
              );
            }}
            split={false}
          />
        )}
      </div>

      <Button className="history-open-button" onClick={onOpenHistory} type="text">
        查看全部历史
      </Button>
    </section>
  );
}

function DeleteHistoryConfirmContent({
  title,
  outputPath,
}: {
  title: string;
  outputPath?: string;
}) {
  return (
    <div className="history-delete-confirm">
      <Text strong>{title}</Text>
      {outputPath ? (
        <Text type="secondary">本地文件：{outputPath}</Text>
      ) : null}
    </div>
  );
}

function CleanupConfirmContent({
  summary,
}: {
  summary: DownloadCleanupSummary | null;
}) {
  if (!summary) {
    return <Text type="secondary">正在扫描可清理内容...</Text>;
  }

  const hasItems =
    summary.fileCount > 0 ||
    summary.directoryCount > 0 ||
    summary.invalidHistoryCount > 0;

  return (
    <div className="cleanup-confirm">
      <Text>
        {hasItems
          ? "将清理应用确认归属的未完成缓存，并移除无效历史记录。"
          : "没有发现需要清理的下载缓存或无效历史记录。"}
      </Text>
      <div className="cleanup-summary-grid">
        <CleanupMetric label="预计释放" value={formatBytes(summary.bytes)} />
        <CleanupMetric label="缓存文件" value={`${summary.fileCount} 个`} />
        <CleanupMetric label="缓存目录" value={`${summary.directoryCount} 个`} />
        <CleanupMetric
          label="无效历史"
          value={`${summary.invalidHistoryCount} 条`}
        />
      </div>
      {summary.skippedActiveTasks > 0 ? (
        <Text type="secondary">
          已跳过 {summary.skippedActiveTasks} 个正在运行或可继续的任务缓存。
        </Text>
      ) : null}
    </div>
  );
}

function CleanupMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="cleanup-metric">
      <Text type="secondary">{label}</Text>
      <Text strong>{value}</Text>
    </div>
  );
}

function SidebarDependencyPanel({
  collapsed,
  dependencies,
  isRefreshing,
  onChooseToolPath,
  onClearToolPath,
  onRefresh,
  onSaveProxySettings,
  toolSettings,
  toolUpdates,
}: {
  collapsed: boolean;
  dependencies: DependencyStatus | null;
  isRefreshing: boolean;
  onChooseToolPath: (tool: ToolName) => void;
  onClearToolPath: (tool: ToolName) => void;
  onRefresh: () => void;
  onSaveProxySettings: (mode: ProxyMode, proxyUrl?: string | null) => void;
  toolSettings: ToolSettings;
  toolUpdates: ToolUpdates | null;
}) {
  const healthClass = dependencyHealthClass(dependencies, toolUpdates);

  if (collapsed) {
    return (
      <div className="sidebar-dependency-collapsed">
        <Popover
          content={
            <CollapsedDependencyPopover
              dependencies={dependencies}
              isRefreshing={isRefreshing}
              onChooseToolPath={onChooseToolPath}
              onClearToolPath={onClearToolPath}
              onRefresh={onRefresh}
              onSaveProxySettings={onSaveProxySettings}
              toolSettings={toolSettings}
              toolUpdates={toolUpdates}
            />
          }
          placement="rightBottom"
          trigger="click"
        >
          <Button
            aria-label="依赖状态"
            className={`sidebar-dependency-trigger ${healthClass}`}
            type="text"
          >
            <span className="dependency-status-lamp" aria-hidden="true" />
          </Button>
        </Popover>
      </div>
    );
  }

  if (!dependencies) {
    return (
      <section className="sidebar-dependency-panel">
        <div className="sidebar-dependency-row">
          <Space className="sidebar-dependency-tags" size={6} wrap={false}>
            <span
              aria-live="polite"
              className="dependency-tag is-checking is-static"
              role="status"
            >
              <span className="dependency-tag-dot" aria-hidden="true" />
              检测中
            </span>
          </Space>
          <Tooltip title="重新检测">
            <Button
              aria-label="重新检测依赖"
              disabled={isRefreshing}
              icon={<ReloadOutlined />}
              loading={isRefreshing}
              onClick={onRefresh}
              size="small"
              type="text"
            />
          </Tooltip>
        </div>
      </section>
    );
  }

  return (
    <section className="sidebar-dependency-panel">
      <div className="sidebar-dependency-row">
        <Space className="sidebar-dependency-tags" size={6} wrap={false}>
          <DependencyTag
            installHint={dependencies.installHint}
            manualPath={toolManualPath(toolSettings, dependencies.ytDlp.name)}
            onChooseToolPath={onChooseToolPath}
            onClearToolPath={onClearToolPath}
            placement="rightBottom"
            tool={dependencies.ytDlp}
            update={toolUpdates?.ytDlp ?? null}
          />
          <DependencyTag
            installHint={dependencies.installHint}
            manualPath={toolManualPath(toolSettings, dependencies.ffmpeg.name)}
            onChooseToolPath={onChooseToolPath}
            onClearToolPath={onClearToolPath}
            placement="rightBottom"
            tool={dependencies.ffmpeg}
            update={toolUpdates?.ffmpeg ?? null}
          />
          <ProxyTag
            onSaveProxySettings={onSaveProxySettings}
            placement="rightBottom"
            proxy={dependencies.proxy}
            toolSettings={toolSettings}
          />
        </Space>
        <Tooltip title="重新检测">
          <Button
            aria-label="重新检测依赖"
            disabled={isRefreshing}
            icon={<ReloadOutlined />}
            loading={isRefreshing}
            onClick={onRefresh}
            size="small"
            type="text"
          />
        </Tooltip>
      </div>
    </section>
  );
}

function CollapsedDependencyPopover({
  dependencies,
  isRefreshing,
  onChooseToolPath,
  onClearToolPath,
  onRefresh,
  onSaveProxySettings,
  toolSettings,
  toolUpdates,
}: {
  dependencies: DependencyStatus | null;
  isRefreshing: boolean;
  onChooseToolPath: (tool: ToolName) => void;
  onClearToolPath: (tool: ToolName) => void;
  onRefresh: () => void;
  onSaveProxySettings: (mode: ProxyMode, proxyUrl?: string | null) => void;
  toolSettings: ToolSettings;
  toolUpdates: ToolUpdates | null;
}) {
  if (!dependencies) {
    return (
      <div className="dependency-popover sidebar-dependency-popover">
        <div className="sidebar-dependency-popover-header">
          <Text strong>依赖状态</Text>
          <Button
            icon={<ReloadOutlined />}
            loading={isRefreshing}
            onClick={onRefresh}
            size="small"
          >
            重新检测
          </Button>
        </div>
        <Alert message="正在检测 yt-dlp 与 ffmpeg" showIcon type="info" />
      </div>
    );
  }

  return (
    <div className="dependency-popover sidebar-dependency-popover is-wide">
      <div className="sidebar-dependency-popover-header">
        <Text strong>依赖状态</Text>
        <Button
          icon={<ReloadOutlined />}
          loading={isRefreshing}
          onClick={onRefresh}
          size="small"
        >
          重新检测
        </Button>
      </div>
      <div className="sidebar-dependency-popover-tools">
        <div className="sidebar-dependency-popover-tool">
          <DependencyPopover
            installHint={dependencies.installHint}
            manualPath={toolManualPath(toolSettings, dependencies.ytDlp.name)}
            onChooseToolPath={onChooseToolPath}
            onClearToolPath={onClearToolPath}
            tool={dependencies.ytDlp}
            update={toolUpdates?.ytDlp ?? null}
          />
        </div>
        <div className="sidebar-dependency-popover-tool">
          <DependencyPopover
            installHint={dependencies.installHint}
            manualPath={toolManualPath(toolSettings, dependencies.ffmpeg.name)}
            onChooseToolPath={onChooseToolPath}
            onClearToolPath={onClearToolPath}
            tool={dependencies.ffmpeg}
            update={toolUpdates?.ffmpeg ?? null}
          />
        </div>
        <div className="sidebar-dependency-popover-tool">
          <ProxyPopover
            onSaveProxySettings={onSaveProxySettings}
            proxy={dependencies.proxy}
            toolSettings={toolSettings}
          />
        </div>
      </div>
    </div>
  );
}

function DependencyTag({
  installHint,
  manualPath,
  onChooseToolPath,
  onClearToolPath,
  placement = "bottomRight",
  tool,
  update,
}: {
  installHint: string;
  manualPath?: string | null;
  onChooseToolPath: (tool: ToolName) => void;
  onClearToolPath: (tool: ToolName) => void;
  placement?: "bottomRight" | "rightBottom";
  tool: DependencyStatus["ytDlp"];
  update?: ToolUpdateStatus | null;
}) {
  const installed = tool.installed;
  const hasError = !installed;
  const hasUpdate = installed && update?.updateAvailable === true;
  const statusClass = hasError
    ? "has-error"
    : hasUpdate
      ? "has-warning"
      : "is-ready";
  const statusLabel = hasError
    ? `${tool.name} 未安装`
    : hasUpdate
      ? `${tool.name} 可更新`
      : `${tool.name} 已就绪`;

  return (
    <Popover
      content={
        <DependencyPopover
          installHint={installHint}
          manualPath={manualPath}
          onChooseToolPath={onChooseToolPath}
          onClearToolPath={onClearToolPath}
          tool={tool}
          update={update}
        />
      }
      placement={placement}
      trigger="click"
    >
      <Tag aria-label={statusLabel} className={`dependency-tag ${statusClass}`}>
        <span className="dependency-tag-dot" aria-hidden="true" />
        <span className="dependency-tag-label">{tool.name}</span>
      </Tag>
    </Popover>
  );
}

function ProxyTag({
  onSaveProxySettings,
  placement = "bottomRight",
  proxy,
  toolSettings,
}: {
  onSaveProxySettings: (mode: ProxyMode, proxyUrl?: string | null) => void;
  placement?: "bottomRight" | "rightBottom";
  proxy: ProxyStatus;
  toolSettings: ToolSettings;
}) {
  const hasProxy = Boolean(proxy.effectiveProxy);
  const isWarning = proxy.source === "pacUnsupported" || proxy.source === "error";
  const statusClass = isWarning ? "has-warning" : hasProxy ? "is-ready" : "is-checking";

  return (
    <Popover
      content={
        <ProxyPopover
          onSaveProxySettings={onSaveProxySettings}
          proxy={proxy}
          toolSettings={toolSettings}
        />
      }
      placement={placement}
      trigger="click"
    >
      <Tag
        aria-label={`代理：${proxyStatusText(proxy)}`}
        className={`dependency-tag ${statusClass}`}
      >
        <span className="dependency-tag-dot" aria-hidden="true" />
        <span className="dependency-tag-label">代理</span>
      </Tag>
    </Popover>
  );
}

function ProxyPopover({
  onSaveProxySettings,
  proxy,
  toolSettings,
}: {
  onSaveProxySettings: (mode: ProxyMode, proxyUrl?: string | null) => void;
  proxy: ProxyStatus;
  toolSettings: ToolSettings;
}) {
  const [draftMode, setDraftMode] = useState<ProxyMode>(
    toolSettings.proxyMode ?? proxy.mode ?? "auto",
  );
  const [draftProxyUrl, setDraftProxyUrl] = useState(
    toolSettings.proxyUrl ?? proxy.effectiveProxy ?? "",
  );

  useEffect(() => {
    setDraftMode(toolSettings.proxyMode ?? proxy.mode ?? "auto");
    setDraftProxyUrl(toolSettings.proxyUrl ?? proxy.effectiveProxy ?? "");
  }, [proxy.effectiveProxy, proxy.mode, toolSettings.proxyMode, toolSettings.proxyUrl]);

  const canSave =
    draftMode !== "manual" ||
    /^(https?|socks5h?):\/\//i.test(draftProxyUrl.trim());

  return (
    <div className="dependency-popover proxy-popover">
      <Text type="secondary">代理设置</Text>
      <div className="dependency-popover-detail">
        <Text strong>当前模式</Text>
        <Text>{proxyModeLabel(proxy.mode)}</Text>
      </div>
      <div className="dependency-popover-detail">
        <Text strong>生效代理</Text>
        <Text className="dependency-path">
          {proxy.effectiveProxy ?? "未传入代理"}
        </Text>
      </div>
      <div className="dependency-popover-detail">
        <Text strong>来源</Text>
        <Text>{proxySourceLabel(proxy)}</Text>
      </div>
      {proxy.message ? (
        <Alert message={proxy.message} showIcon type={proxyAlertType(proxy)} />
      ) : null}
      <div className="proxy-settings-form">
        <Select<ProxyMode>
          className="proxy-mode-select"
          onChange={setDraftMode}
          options={[
            { label: "自动读取", value: "auto" },
            { label: "手动指定", value: "manual" },
            { label: "不使用代理", value: "off" },
          ]}
          value={draftMode}
        />
        {draftMode === "manual" ? (
          <Input
            onChange={(event) => setDraftProxyUrl(event.target.value)}
            placeholder="http://127.0.0.1:7890"
            value={draftProxyUrl}
          />
        ) : null}
        <Button
          disabled={!canSave}
          onClick={() =>
            onSaveProxySettings(
              draftMode,
              draftMode === "manual" ? draftProxyUrl.trim() : null,
            )
          }
          size="small"
          type="primary"
        >
          保存代理设置
        </Button>
      </div>
    </div>
  );
}

function SupportedSiteExamples({
  examples,
  onChoose,
}: {
  examples: SupportedSiteExample[];
  onChoose: (example: SupportedSiteExample) => void;
}) {
  if (examples.length === 0) {
    return null;
  }

  return (
    <div className="supported-site-row">
      <Text className="supported-site-label" type="secondary">
        示例
      </Text>
      <Space size={8} wrap>
        {examples.map((example) => (
          <Tooltip key={example.name} title={example.url}>
            <Button
              className="supported-site-button"
              onClick={() => onChoose(example)}
              size="small"
              type="text"
            >
              {example.name}
            </Button>
          </Tooltip>
        ))}
      </Space>
    </div>
  );
}

function QueueAuthProbeLine({ item }: { item: QueueItem }) {
  const status = queueItemAuthStatus(item);
  const tagText = queueItemAuthTagText(item);
  const details = queueItemAuthDetails(item);
  const formatParts = queueItemBestFormatParts(item);

  return (
    <div className="queue-auth-row">
      <Popover
        content={
          <div className={`queue-auth-popover is-${status}`}>
            <Text strong>{tagText}</Text>
            {details ? (
              <Text className="queue-auth-detail" type="secondary">
                {details}
              </Text>
            ) : null}
          </div>
        }
        placement="topLeft"
        trigger="hover"
      >
        <Tag className={`queue-auth-tag is-${status}`}>{tagText}</Tag>
      </Popover>
      {formatParts ? (
        <Text
          className="queue-auth-format"
          title={formatParts.fullText}
          type="secondary"
        >
          <span className="queue-auth-format-prefix">最高可下载：</span>
          {formatParts.resolution ? (
            <>
              <span className="queue-auth-format-resolution">
                {formatParts.resolution}
              </span>
              {formatParts.detail ? (
                <span className="queue-auth-format-detail">
                  {" · "}
                  {formatParts.detail}
                </span>
              ) : null}
            </>
          ) : (
            <span className="queue-auth-format-detail">
              {formatParts.detail}
            </span>
          )}
        </Text>
      ) : null}
    </div>
  );
}

function AuthProbePanel({
  browser,
  canRefresh,
  isChecking,
  onCancel,
  onRefresh,
  queueSummary,
  state,
}: {
  browser: BrowserKind;
  canRefresh: boolean;
  isChecking: boolean;
  onCancel: () => void;
  onRefresh: () => void;
  queueSummary?: QueueAuthSummary | null;
  state: AuthProbeState;
}) {
  const status = queueSummary?.status ?? state.status;
  const hint = queueSummary ? queueAuthHint(queueSummary) : authProbeHint(state);
  const triggerText = queueSummary
    ? queueAuthTriggerText(queueSummary)
    : authProbeTriggerText(state);

  return (
    <div className="auth-probe-controls">
      <Popover
        content={
          queueSummary ? (
            <QueueAuthPopoverContent
              hint={hint}
              status={status}
              summary={queueSummary}
            />
          ) : (
            <SingleAuthPopoverContent
              browser={browser}
              hint={hint}
              state={state}
              status={status}
            />
          )
        }
        placement="bottom"
        trigger="click"
      >
        <Button
          className={`auth-probe-status-trigger is-${status}`}
          size="small"
          type="text"
        >
          <span className="auth-probe-dot" aria-hidden="true" />
          <span className="auth-probe-trigger-text">{triggerText}</span>
        </Button>
      </Popover>
      <Tooltip title={isChecking ? "停止检测" : "重新检测登录态"}>
        <Button
          aria-label={isChecking ? "停止检测登录态" : "重新检测登录态"}
          className="auth-probe-refresh-button"
          danger={isChecking}
          disabled={!canRefresh && !isChecking}
          icon={isChecking ? <StopOutlined /> : <ReloadOutlined />}
          onClick={isChecking ? onCancel : onRefresh}
          size="small"
          type="text"
        />
      </Tooltip>
    </div>
  );
}

function QueueAuthPopoverContent({
  hint,
  status,
  summary,
}: {
  hint: string | null;
  status: AuthProbeStatus;
  summary: QueueAuthSummary;
}) {
  const highestSites =
    summary.sites.length > 1 ? queueHighestAuthSites(summary) : new Set<string>();

  return (
    <div className={`auth-probe-popover is-${status}`}>
      <div className="auth-probe-popover-head">
        <Text strong>队列登录态详情</Text>
      </div>
      <AuthProbeStatusSummary
        label={queueAuthSummaryLabel(summary)}
        meta={queueAuthSummaryMeta(summary)}
        status={status}
      />
      {summary.sites.length ? (
        <div className="auth-probe-site-list">
          {summary.sites.map((site) => (
            <AuthProbeSiteRow
              isHighest={highestSites.has(site.site)}
              key={site.site}
              site={site}
            />
          ))}
        </div>
      ) : null}
      {hint ? <Text className="auth-probe-hint">{hint}</Text> : null}
    </div>
  );
}

function SingleAuthPopoverContent({
  browser,
  hint,
  state,
  status,
}: {
  browser: BrowserKind;
  hint: string | null;
  state: AuthProbeState;
  status: AuthProbeStatus;
}) {
  const shouldShowSiteRow = state.status === "ready" || state.status === "warning";
  const details = shouldShowSiteRow ? null : authProbeDetails(state);

  return (
    <div className={`auth-probe-popover is-${status}`}>
      <div className="auth-probe-popover-head">
        <Text strong>{browserLabel(browser)} 登录态详情</Text>
      </div>
      <AuthProbeStatusSummary
        label={authProbeStatusLabel(state)}
        meta={authProbeStatusMeta(state)}
        status={status}
      />
      {shouldShowSiteRow ? (
        <div className="auth-probe-site-list">
          <AuthProbeSingleSiteRow state={state} />
        </div>
      ) : null}
      {details ? (
        <Text className="auth-probe-message" type="secondary">
          {details}
        </Text>
      ) : null}
      {hint ? <Text className="auth-probe-hint">{hint}</Text> : null}
    </div>
  );
}

function AuthProbeStatusSummary({
  label,
  meta,
  status,
}: {
  label: string;
  meta: string | null;
  status: AuthProbeStatus;
}) {
  return (
    <div className={`auth-probe-summary is-${status}`}>
      <span className="auth-probe-summary-dot" aria-hidden="true" />
      <span className={`auth-probe-summary-pill is-${status}`}>{label}</span>
      {meta ? <span className="auth-probe-summary-meta">{meta}</span> : null}
    </div>
  );
}

function AuthProbeSiteRow({
  isHighest,
  site,
}: {
  isHighest: boolean;
  site: QueueAuthSiteSummary;
}) {
  const status = site.status ?? queueAuthSiteStatus(site);
  const formatParts =
    site.ready + site.warning > 0
      ? bestFormatParts(site.bestFormatLabel)
      : null;
  const message = formatParts ? null : queueAuthSiteMessage(site);

  return (
    <div
      className={`auth-probe-site-row is-${status}${isHighest ? " is-highest" : ""}`}
    >
      <div className="auth-probe-site-main">
        <span className="auth-probe-site-name">{site.site}</span>
        {isHighest ? <span className="auth-probe-highest-pill">最高</span> : null}
        <span className={`auth-probe-site-count is-${status}`}>
          {queueAuthSiteCountText(site)}
        </span>
      </div>
      <div className="auth-probe-site-format">
        {formatParts ? (
          <AuthProbeFormat parts={formatParts} />
        ) : (
          <span className="auth-probe-site-message">{message}</span>
        )}
      </div>
    </div>
  );
}

function AuthProbeSingleSiteRow({ state }: { state: AuthProbeState }) {
  const formatParts = bestFormatParts(state.bestFormatLabel);
  const site = state.site?.trim() || siteFromUrl(state.url) || "当前链接";

  return (
    <div className={`auth-probe-site-row is-${state.status}`}>
      <div className="auth-probe-site-main">
        <span className="auth-probe-site-name">{site}</span>
        <span className={`auth-probe-site-count is-${state.status}`}>
          {authProbeSiteCountText(state)}
        </span>
      </div>
      <div className="auth-probe-site-format">
        <AuthProbeFormat parts={formatParts} />
      </div>
    </div>
  );
}

function AuthProbeFormat({ parts }: { parts: AuthFormatParts }) {
  return (
    <span className="auth-probe-format" title={parts.fullText}>
      {parts.resolution ? (
        <>
          <span className="auth-probe-format-resolution">
            {parts.resolution}
          </span>
          {parts.detail ? (
            <span className="auth-probe-format-detail">
              {" · "}
              {parts.detail}
            </span>
          ) : null}
        </>
      ) : (
        <span className="auth-probe-format-detail">{parts.detail}</span>
      )}
    </span>
  );
}

function StatusCount({
  label,
  tone,
  value,
}: {
  label: string;
  tone: "success" | "processing" | "waiting" | "danger" | "neutral";
  value: number;
}) {
  return (
    <div className={`status-count-item is-${tone}`}>
      <span className="status-count-dot" aria-hidden="true" />
      <Text>{label}</Text>
      <Text strong>{value}</Text>
    </div>
  );
}

function LazyViewFallback() {
  return (
    <Card className="lazy-view-card">
      <Text type="secondary">加载中...</Text>
    </Card>
  );
}

function DependencyPopover({
  installHint,
  manualPath,
  onChooseToolPath,
  onClearToolPath,
  tool,
  update,
}: {
  installHint: string;
  manualPath?: string | null;
  onChooseToolPath: (tool: ToolName) => void;
  onClearToolPath: (tool: ToolName) => void;
  tool: DependencyStatus["ytDlp"];
  update?: ToolUpdateStatus | null;
}) {
  const actions = (
    <Space className="dependency-actions" size={8} wrap>
      <Button
        icon={<FolderOpenOutlined />}
        onClick={() => onChooseToolPath(tool.name)}
        size="small"
      >
        选择文件
      </Button>
      {manualPath ? (
        <Button
          icon={<DeleteOutlined />}
          onClick={() => onClearToolPath(tool.name)}
          size="small"
        >
          清除手动路径
        </Button>
      ) : null}
    </Space>
  );

  if (!tool.installed) {
    return (
      <div className="dependency-popover">
        <Text type="secondary">{tool.name} 未安装</Text>
        {manualPath ? (
          <div className="dependency-popover-detail">
            <Text strong>手动路径</Text>
            <Text className="dependency-path">{manualPath}</Text>
          </div>
        ) : null}
        <Alert
          description={<code className="install-command">{installHint}</code>}
          message="安装后点击重新检测"
          showIcon
          type="warning"
        />
        {actions}
      </div>
    );
  }

  return (
    <div className="dependency-popover">
      <Text type="secondary">{tool.name} 已安装</Text>
      <div className="dependency-popover-detail">
        <Text strong>版本</Text>
        <Text>{tool.version ?? "未返回版本信息"}</Text>
      </div>
      <ToolUpdateBlock manualPath={manualPath} tool={tool} update={update} />
      <div className="dependency-popover-detail">
        <Text strong>来源</Text>
        <Text>{toolSourceLabel(tool.source)}</Text>
      </div>
      <div className="dependency-popover-detail">
        <Text strong>路径</Text>
        <Text className="dependency-path">{tool.path ?? "未返回路径"}</Text>
      </div>
      {manualPath && tool.source !== "manual" ? (
        <div className="dependency-popover-detail">
          <Text strong>手动路径</Text>
          <Text className="dependency-path">{manualPath}</Text>
        </div>
      ) : null}
      {actions}
    </div>
  );
}

function ToolUpdateBlock({
  manualPath,
  tool,
  update,
}: {
  manualPath?: string | null;
  tool: DependencyStatus["ytDlp"];
  update?: ToolUpdateStatus | null;
}) {
  if (!update) {
    return (
      <div className="dependency-popover-detail">
        <Text strong>更新检查</Text>
        <Text type="secondary">启动后检查中</Text>
      </div>
    );
  }

  if (update.error) {
    return (
      <div className="dependency-update-block">
        <Alert
          description={
            <Text className="dependency-update-error" type="secondary">
              {update.error}
            </Text>
          }
          message="更新检查失败，稍后重试"
          showIcon
          type="warning"
        />
      </div>
    );
  }

  const hasUpdate = update.updateAvailable === true;
  const checkedAtText = `检查时间：${formatUnixTime(update.checkedAt)}`;
  const sourceHint =
    manualPath && tool.source === "manual"
      ? "当前使用手动路径，请用安装该可执行文件的方式更新。"
      : null;

  return (
    <div className="dependency-update-block">
      <div className="dependency-popover-detail">
        <Text strong>最新版本</Text>
        <Text>{update.latestVersion ?? "未返回最新版本"}</Text>
      </div>
      <Alert
        message={
          hasUpdate
            ? `可更新到 ${update.latestVersion ?? "最新版本"} · ${checkedAtText}`
            : `已是最新 · ${checkedAtText}`
        }
        showIcon
        type={hasUpdate ? "warning" : "success"}
      />
      {sourceHint ? <Text type="secondary">{sourceHint}</Text> : null}
      {update.updateCommand ? (
        <div className="dependency-popover-detail">
          <Text strong>更新命令</Text>
          <code className="install-command">{update.updateCommand}</code>
        </div>
      ) : null}
    </div>
  );
}

function getPageHeading(activeView: ActiveView) {
  if (activeView === "ffmpeg") {
    return {
      icon: <CodeOutlined />,
      title: "FFmpeg 工具箱",
    };
  }

  if (activeView === "history") {
    return {
      icon: <ClockCircleOutlined />,
      title: "历史记录",
    };
  }

  return {
    icon: <DownloadOutlined />,
    title: "新建下载",
  };
}

function supportedSitesMeta(supportedSites: SupportedSitesResponse) {
  if (supportedSites.version === "browser preview") {
    return `浏览器预览数据 · ${supportedSites.total} 个示例站点`;
  }

  return `yt-dlp ${supportedSites.version ?? "当前版本"} · ${supportedSites.total} 个提取器`;
}

function dependencyHealthClass(
  dependencies: DependencyStatus | null,
  toolUpdates: ToolUpdates | null,
) {
  if (!dependencies) {
    return "is-checking";
  }

  if (!dependencies.ready) {
    return "has-error";
  }

  if (hasDependencyUpdate(toolUpdates)) {
    return "has-warning";
  }

  return "is-ready";
}

function hasDependencyUpdate(toolUpdates: ToolUpdates | null) {
  return (
    toolUpdates?.ytDlp.updateAvailable === true ||
    toolUpdates?.ffmpeg.updateAvailable === true
  );
}

function progressStatus(status: DownloadStatus): ProgressProps["status"] {
  if (status === "completed") {
    return "success";
  }

  if (status === "failed") {
    return "exception";
  }

  if (status === "running") {
    return "active";
  }

  if (status === "paused") {
    return "normal";
  }

  return "normal";
}

function queueProgressStatus(status: DownloadQueueStatus): ProgressProps["status"] {
  if (status === "completed") {
    return "success";
  }

  if (status === "failed") {
    return "exception";
  }

  if (status === "running") {
    return "active";
  }

  if (status === "paused") {
    return "normal";
  }

  return "normal";
}

function queueStatusCopy(status: DownloadQueueStatus) {
  if (status === "idle") {
    return "未开始";
  }

  if (status === "queued") {
    return "等待中";
  }

  return statusCopy[status] ?? "未知状态";
}

function queueStatusColor(status: DownloadQueueStatus) {
  if (status === "queued") {
    return "blue";
  }

  return statusTagColor(status);
}

function parseUrlLines(value: string) {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

function queueItemFromParsed(item: BatchParseItem, browser: BrowserKind): QueueItem {
  const fallbackTitle = titleFromUrl(item.url);
  const parsedTitle = item.title?.trim();
  const site = item.site?.trim() || siteFromUrl(item.url) || "未知站点";

  return {
    id: item.id || crypto.randomUUID(),
    url: item.url,
    title: parsedTitle || fallbackTitle,
    site,
    duration: item.duration ?? null,
    thumbnail: item.thumbnail ?? null,
    sourceUrl: item.sourceUrl ?? null,
    playlistTitle: item.playlistTitle ?? null,
    playlistIndex: item.playlistIndex ?? null,
    playlistTotal: item.playlistTotal ?? null,
    sourceOrder: item.sourceOrder ?? null,
    isPlaylistItem: item.isPlaylistItem,
    status: item.error ? "failed" : "idle",
    parseError: Boolean(item.error),
    authProbe: item.error
      ? queueParseErrorAuthProbe(
          {
            url: item.url,
            site,
            error: item.error,
          },
          browser,
        )
      : {
          status: "idle",
          browser,
          url: item.url,
          site,
        },
    progress: 0,
    phase: null,
    phaseLabel: null,
    speed: null,
    eta: null,
    outputPath: null,
    localMedia: null,
    mediaComparison: null,
    error: item.error ?? null,
  };
}

function summarizeQueue(items: QueueItem[]) {
  return items.reduce(
    (summary, item) => {
      if (item.status === "completed") {
        summary.completed += 1;
      } else if (item.status === "running") {
        summary.running += 1;
      } else if (item.status === "paused") {
        summary.paused += 1;
      } else if (item.status === "queued") {
        summary.queued += 1;
      } else if (item.status === "idle") {
        summary.idle += 1;
      } else if (item.status === "failed") {
        summary.failed += 1;
      } else if (item.status === "canceled") {
        summary.canceled += 1;
      }

      return summary;
    },
    {
      completed: 0,
      running: 0,
      paused: 0,
      queued: 0,
      idle: 0,
      failed: 0,
      canceled: 0,
    },
  );
}

function summarizeQueueAuth(items: QueueItem[]): QueueAuthSummary | null {
  if (!items.length) {
    return null;
  }

  const summary: QueueAuthSummary = {
    total: items.length,
    probeable: 0,
    ready: 0,
    warning: 0,
    unavailable: 0,
    canceled: 0,
    checking: 0,
    idle: 0,
    parseFailed: 0,
    status: "idle",
    bestFormatLabel: null,
    sites: [],
  };
  const siteMap = new Map<string, QueueAuthSiteSummary>();

  items.forEach((item) => {
    const status = queueItemAuthStatus(item);
    const site = queueItemAuthSite(item);
    const siteSummary =
      siteMap.get(site) ??
      {
        site,
        total: 0,
        ready: 0,
        warning: 0,
        unavailable: 0,
        canceled: 0,
        checking: 0,
        idle: 0,
        parseFailed: 0,
        bestFormatLabel: null,
        formatCount: null,
        error: null,
      };

    siteSummary.total += 1;

    if (item.parseError) {
      summary.parseFailed += 1;
      siteSummary.parseFailed += 1;
      siteSummary.error ??= item.error ?? item.authProbe?.error ?? null;
    } else {
      summary.probeable += 1;
    }

    if (status === "ready") {
      summary.ready += 1;
      siteSummary.ready += 1;
    } else if (status === "warning") {
      summary.warning += 1;
      siteSummary.warning += 1;
    } else if (status === "unavailable") {
      summary.unavailable += 1;
      siteSummary.unavailable += 1;
      siteSummary.error ??= item.authProbe?.error ?? item.error ?? null;
    } else if (status === "canceled") {
      summary.canceled += 1;
      siteSummary.canceled += 1;
      siteSummary.error ??= item.authProbe?.error ?? null;
    } else if (status === "checking") {
      summary.checking += 1;
      siteSummary.checking += 1;
    } else if (status === "failed") {
      siteSummary.error ??= item.error ?? item.authProbe?.error ?? null;
    } else {
      summary.idle += 1;
      siteSummary.idle += 1;
    }

    const bestFormatLabel = item.authProbe?.bestFormatLabel ?? null;
    if (bestFormatLabel) {
      summary.bestFormatLabel = betterFormatLabel(
        summary.bestFormatLabel,
        bestFormatLabel,
      );
      siteSummary.bestFormatLabel = betterFormatLabel(
        siteSummary.bestFormatLabel,
        bestFormatLabel,
      );
    }

    if (item.authProbe?.formatCount) {
      siteSummary.formatCount = Math.max(
        siteSummary.formatCount ?? 0,
        item.authProbe.formatCount,
      );
    }

    siteMap.set(site, siteSummary);
  });

  summary.status = queueAuthSummaryStatus(summary);
  summary.sites = Array.from(siteMap.values()).map((site) => ({
    ...site,
    status: queueAuthSiteStatus(site),
  }));

  return summary;
}

function mergeQueueMetadata(item: QueueItem, result: ProbeResponse): QueueItem {
  const title = result.title?.trim();
  const site = result.site?.trim();
  const shouldReplaceTitle =
    title && isBetterQueueTitle(title, item.title, item.url);

  return {
    ...item,
    title: shouldReplaceTitle ? title : item.title,
    site: site && !isPlaceholderTitle(site) ? site : item.site,
    duration: result.duration ?? item.duration,
    thumbnail: result.thumbnail ?? item.thumbnail,
  };
}

function expectedMediaForQueueItem(
  item: QueueItem,
  selectedFormat: string,
  formatOptions: FormatOption[],
): ExpectedMediaInfo | null {
  const selectedOption = formatOptions.find(
    (format) => format.selector === selectedFormat,
  );
  const duration = item.authProbe?.duration ?? item.duration ?? null;
  const explicitResolution = selectedOption?.resolution?.trim();
  const isAudioOnly = explicitResolution?.toLowerCase() === "audio";
  const selectedResolution =
    explicitResolution &&
    !["自动", "audio"].includes(explicitResolution.toLowerCase())
      ? explicitResolution
      : null;
  const resolutionSource = isAudioOnly
    ? null
    : selectedResolution ?? item.authProbe?.bestFormatLabel?.trim() ?? null;
  const resolutionParts = bestFormatParts(resolutionSource);
  const resolutionLabel = resolutionParts.score > 0
    ? resolutionParts.resolution
    : null;
  const expected: ExpectedMediaInfo = {
    duration,
    resolutionLabel,
    resolutionScore: resolutionParts.score > 0 ? resolutionParts.score : null,
  };

  return expected.duration || expected.resolutionScore ? expected : null;
}

function queueMetadataProbeKey(
  item: QueueItem,
  browser: BrowserKind,
  failedThumbnail: ThumbnailLoadFailure | null,
) {
  const thumbnailKey = item.thumbnail ?? "no-thumbnail";
  const failureKey =
    failedThumbnail?.taskId === item.id && failedThumbnail.url === item.thumbnail
      ? `failed-${failedThumbnail.attempts}`
      : "ok";

  return `${item.id}:${browser}:metadata:${thumbnailKey}:${failureKey}`;
}

function queueItemNeedsMetadata(
  item: QueueItem,
  failedThumbnail: ThumbnailLoadFailure | null,
) {
  if (item.error || item.status === "failed" || item.status === "canceled") {
    return false;
  }

  return (
    queueItemHasPlaceholderTitle(item)
    || !item.thumbnail
    || Boolean(
      item.thumbnail &&
        failedThumbnail?.taskId === item.id &&
        failedThumbnail.url === item.thumbnail,
    )
  );
}

function thumbnailDisplayState(
  item: QueueItem,
  failedThumbnail: ThumbnailLoadFailure | null,
  states: Record<string, ThumbnailLoadState>,
): ThumbnailDisplayState {
  const key = thumbnailLoadKey(item);

  if (!item.thumbnail || !key) {
    return {
      label: "未获取到封面",
      status: "missing",
      attempts: 0,
      canRenderImage: false,
      showImage: false,
    };
  }

  const state = states[key];
  const failedAttempts =
    failedThumbnail?.taskId === item.id && failedThumbnail.url === item.thumbnail
      ? failedThumbnail.attempts
      : 0;
  const attempts = Math.max(state?.attempts ?? 0, failedAttempts);

  if (state?.status === "loaded") {
    return {
      label: "",
      status: "loaded",
      attempts,
      canRenderImage: true,
      showImage: true,
    };
  }

  if (attempts >= THUMBNAIL_RETRY_LIMIT || state?.status === "failed") {
    return {
      label: "封面加载失败",
      status: "failed",
      attempts,
      canRenderImage: false,
      showImage: false,
    };
  }

  return {
    label:
      attempts > 0
        ? `封面重试中 ${attempts}/${THUMBNAIL_RETRY_LIMIT}`
        : "封面加载中",
    status: "loading",
    attempts,
    canRenderImage: true,
    showImage: false,
  };
}

function thumbnailLoadKey(item: QueueItem) {
  return item.thumbnail ? `${item.id}:${item.thumbnail}` : null;
}

function ThumbnailPlaceholder({
  state,
}: {
  state: ThumbnailDisplayState | null;
}) {
  const displayState =
    state ?? {
      label: "未获取到封面",
      status: "missing" as const,
      attempts: 0,
      canRenderImage: false,
      showImage: false,
    };
  const icon =
    displayState.status === "loading" ? (
      <LoadingOutlined spin />
    ) : (
      <VideoCameraOutlined />
    );

  return (
    <div className={`download-status-cover-placeholder is-${displayState.status}`}>
      {icon}
      <span>{displayState.label}</span>
    </div>
  );
}

function thumbnailImageSrc(
  item: QueueItem,
  failedThumbnail: ThumbnailLoadFailure | null,
) {
  const thumbnail = item.thumbnail ?? "";

  if (
    !thumbnail ||
    failedThumbnail?.taskId !== item.id ||
    failedThumbnail.url !== thumbnail ||
    failedThumbnail.attempts === 0
  ) {
    return thumbnail;
  }

  try {
    const url = new URL(thumbnail);
    url.searchParams.set("vd_thumbnail_retry", String(failedThumbnail.attempts));
    return url.toString();
  } catch {
    const joiner = thumbnail.includes("?") ? "&" : "?";
    return `${thumbnail}${joiner}vd_thumbnail_retry=${failedThumbnail.attempts}`;
  }
}

function queueItemHasPlaceholderTitle(item: QueueItem) {
  return (
    isPlaceholderQueueTitle(item.title, item.url)
    || (item.isPlaylistItem && isOpaqueIdTitle(item.title))
  );
}

function isPlaceholderQueueTitle(title: string, url: string) {
  const normalizedTitle = normalizeTitleToken(title);
  const normalizedUrlTitle = normalizeTitleToken(titleFromUrl(url));

  return (
    isPlaceholderTitle(title)
    || normalizedTitle === normalizedUrlTitle
    || /^bv[a-z0-9]+$/i.test(title.trim())
    || /^av\d+$/i.test(title.trim())
  );
}

function isBetterQueueTitle(nextTitle: string, currentTitle: string, url: string) {
  if (isPlaceholderQueueTitle(nextTitle, url) || isOpaqueIdTitle(nextTitle)) {
    return false;
  }

  if (!currentTitle.trim()) {
    return true;
  }

  if (isPlaceholderQueueTitle(currentTitle, url) || isOpaqueIdTitle(currentTitle)) {
    return true;
  }

  return false;
}

function isPlaceholderTitle(title: string) {
  return ["", "undefined", "null", "unknown", "untitled", "untitled video"].includes(
    title.trim().toLowerCase(),
  );
}

function normalizeTitleToken(title: string) {
  return title.trim().replace(/\.[^.]+$/, "").toLowerCase();
}

function isOpaqueIdTitle(title: string) {
  const text = title.trim();

  return /\d/.test(text) && /[A-Z]/.test(text) && /^[a-z0-9_-]{8,24}$/i.test(text);
}

function queuePlaylistTagText(item: QueueItem) {
  const parts = [queuePlaylistPositionText(item), queueSourceOrderText(item)].filter(Boolean);

  return parts.length ? parts.join(" · ") : "多P条目";
}

function queuePlaylistPositionText(item: QueueItem) {
  const index = positiveInteger(item.playlistIndex);
  const total = positiveInteger(item.playlistTotal);

  if (index && total) {
    return `多P ${index}/${total}`;
  }

  if (index) {
    return `多P ${index}`;
  }

  return item.isPlaylistItem ? "多P条目" : null;
}

function queueSourceOrderText(item: QueueItem) {
  const sourceOrder = positiveInteger(item.sourceOrder);

  return sourceOrder ? `来源${sourceOrder}` : null;
}

function positiveInteger(value?: number | null) {
  return typeof value === "number" && Number.isInteger(value) && value > 0
    ? value
    : null;
}

function queueRuntimeMeta(item: QueueItem) {
  if (item.status === "paused") {
    return "已暂停，可继续下载";
  }

  if (item.status !== "running") {
    return null;
  }

  const parts = [
    item.phaseLabel ?? null,
    item.speed ? `速度 ${item.speed}` : null,
    item.eta ? `剩余 ${item.eta}` : null,
  ].filter(Boolean);

  return parts.length ? parts.join(" · ") : null;
}

function isInvalidQueueAfterCleanup(item: QueueItem) {
  return item.status === "failed" || item.status === "canceled";
}

function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0B";
  }

  if (bytes >= 1_000_000_000) {
    return `${trimNumber(bytes / 1_000_000_000)}GB`;
  }

  if (bytes >= 1_000_000) {
    return `${trimNumber(bytes / 1_000_000)}MB`;
  }

  if (bytes >= 1_000) {
    return `${trimNumber(bytes / 1_000)}KB`;
  }

  return `${Math.round(bytes)}B`;
}

function trimNumber(value: number) {
  return (value >= 100 ? value.toFixed(0) : value.toFixed(1)).replace(/\.0$/, "");
}

function titleFromUrl(url: string) {
  return decodeURIComponent(
    url.split("?")[0]?.split("/").filter(Boolean).pop() || "未命名视频",
  );
}

function siteFromUrl(url: string) {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return null;
  }
}

function toolManualPath(settings: ToolSettings, tool: ToolName) {
  return tool === "yt-dlp" ? settings.ytDlpPath : settings.ffmpegPath;
}

function toolSourceLabel(source?: ToolSource | null) {
  return source ? toolSourceCopy[source] : "未识别";
}

function proxyModeLabel(mode?: ProxyMode | null) {
  if (mode === "manual") {
    return "手动指定";
  }

  if (mode === "off") {
    return "不使用代理";
  }

  return "自动读取";
}

function proxyStatusText(proxy: ProxyStatus) {
  if (proxy.effectiveProxy) {
    return `${proxyModeLabel(proxy.mode)} · ${proxy.effectiveProxy}`;
  }

  return proxy.message ?? proxyModeLabel(proxy.mode);
}

function proxySourceLabel(proxy: ProxyStatus) {
  switch (proxy.source) {
    case "manual":
      return "手动指定";
    case "off":
      return "已关闭";
    case "systemHttps":
      return "macOS HTTPS";
    case "systemHttp":
      return "macOS HTTP";
    case "systemSocks":
      return "macOS SOCKS";
    case "pacUnsupported":
      return "PAC 暂不支持";
    case "none":
      return "系统未配置";
    case "unsupported":
      return "当前平台暂不支持";
    case "error":
      return "读取失败";
    default:
      return proxy.source ?? "未识别";
  }
}

function proxyAlertType(proxy: ProxyStatus): "success" | "info" | "warning" {
  if (proxy.source === "error" || proxy.source === "pacUnsupported") {
    return "warning";
  }

  return proxy.effectiveProxy ? "success" : "info";
}

function formatUnixTime(value: string) {
  const seconds = Number(value);
  if (!Number.isFinite(seconds) || seconds <= 0) {
    return "未知";
  }

  return new Date(seconds * 1000).toLocaleString("zh-CN", {
    hour12: false,
  });
}

async function copyTextToClipboard(value: string) {
  if (!navigator.clipboard?.writeText) {
    throw new Error("当前环境无法写入剪贴板，请从命令预览中手动复制。");
  }

  await navigator.clipboard.writeText(value);
}

function parentPath(path: string) {
  const normalized = path.replace(/\\/g, "/");
  const parts = normalized.split("/").filter(Boolean);
  if (parts.length <= 1) {
    return null;
  }

  const prefix = normalized.startsWith("/") ? "/" : "";
  return `${prefix}${parts.slice(0, -1).join("/")}`;
}

function readError(value: unknown) {
  return value instanceof Error ? value.message : String(value);
}

function createOperationId(prefix: string) {
  return `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function isOperationCanceledError(value: string) {
  return value.includes("操作已停止") || value.includes("已停止");
}

function cancelOperationIds(ids: Iterable<string>) {
  Array.from(ids).forEach((operationId) => {
    void cancelYtdlpOperation(operationId);
  });
}

function queueItemCanAuthProbe(item: QueueItem) {
  return Boolean(item.url.trim()) && !item.parseError;
}

function queueParseErrorAuthProbe(
  item: Pick<QueueItem, "url" | "site"> & { error?: string | null },
  browser: BrowserKind,
): AuthProbeState {
  return {
    status: "failed",
    browser,
    url: item.url,
    site: item.site,
    error: item.error ?? "解析失败，无法检测登录态。",
  };
}

function queueItemAuthStatus(item: QueueItem): AuthProbeStatus {
  if (item.parseError) {
    return "failed";
  }

  return item.authProbe?.status ?? "idle";
}

function queueItemAuthSite(item: QueueItem) {
  return (
    item.authProbe?.site?.trim() ||
    item.site?.trim() ||
    siteFromUrl(item.url) ||
    "未知站点"
  );
}

function queueItemAuthTagText(item: QueueItem) {
  const status = queueItemAuthStatus(item);

  if (status === "checking") {
    return "检测中";
  }

  if (status === "ready") {
    return "登录态可用";
  }

  if (status === "warning") {
    return "需注意";
  }

  if (status === "unavailable") {
    return "无法检测";
  }

  if (status === "canceled") {
    return "已停止";
  }

  if (status === "failed") {
    return item.parseError ? "解析失败" : "检测失败";
  }

  return "待检测";
}

function queueItemBestFormatParts(item: QueueItem) {
  const status = queueItemAuthStatus(item);

  if (status !== "ready" && status !== "warning") {
    return null;
  }

  const parts = bestFormatParts(item.authProbe?.bestFormatLabel);

  return {
    ...parts,
    fullText: `最高可下载：${parts.fullText}`,
  };
}

function queueItemAuthDetails(item: QueueItem) {
  const status = queueItemAuthStatus(item);

  if (item.parseError) {
    return item.error ?? item.authProbe?.error ?? "解析失败，无法检测登录态。";
  }

  if (status === "checking") {
    return `${queueItemAuthSite(item)} · 正在检测当前浏览器可用格式`;
  }

  if (status === "unavailable" || status === "failed") {
    return item.authProbe?.error ?? "当前链接未能完成视频探测。";
  }

  if (status === "canceled") {
    return item.authProbe?.error ?? "已停止检测。";
  }

  if (status === "idle") {
    return `${queueItemAuthSite(item)} · 点击重新检测登录态`;
  }

  const parts = [
    queueItemAuthSite(item),
    item.authProbe?.bestFormatLabel
      ? `最高可下载：${item.authProbe.bestFormatLabel}`
      : "最高可下载：未返回明确视频格式",
    item.authProbe?.formatCount ? `${item.authProbe.formatCount} 个格式` : null,
    item.authProbe?.checkedAt ? `检测于 ${formatProbeTime(item.authProbe.checkedAt)}` : null,
  ].filter(Boolean);

  return parts.join(" · ");
}

function queueAuthSummaryStatus(summary: QueueAuthSummary): AuthProbeStatus {
  if (summary.checking > 0) {
    return "checking";
  }

  if (summary.unavailable > 0 || summary.parseFailed > 0) {
    return "unavailable";
  }

  if (summary.canceled > 0 && summary.ready + summary.warning + summary.idle === 0) {
    return "canceled";
  }

  if (summary.warning > 0) {
    return "warning";
  }

  if (summary.ready > 0 && summary.idle === 0) {
    return "ready";
  }

  return "idle";
}

function queueAuthSiteStatus(site: QueueAuthSiteSummary): AuthProbeStatus {
  if (site.checking > 0) {
    return "checking";
  }

  if (site.unavailable > 0 || site.parseFailed > 0) {
    return "unavailable";
  }

  if (site.canceled > 0 && site.ready + site.warning + site.idle === 0) {
    return "canceled";
  }

  if (site.warning > 0) {
    return "warning";
  }

  if (site.ready > 0 && site.idle === 0) {
    return "ready";
  }

  return "idle";
}

function queueAuthTriggerText(summary: QueueAuthSummary) {
  if (summary.checking > 0) {
    return "队列检测中";
  }

  if (summary.unavailable > 0 || summary.parseFailed > 0) {
    return "队列部分异常";
  }

  if (summary.canceled > 0 && summary.ready + summary.warning + summary.idle === 0) {
    return "队列已停止";
  }

  if (summary.warning > 0) {
    return "队列需注意";
  }

  if (summary.ready > 0 && summary.idle === 0) {
    return "队列已检测";
  }

  return "队列待检测";
}

function queueAuthSummaryLabel(summary: QueueAuthSummary) {
  if (summary.checking > 0) {
    return "检测中";
  }

  if (summary.unavailable > 0 || summary.parseFailed > 0) {
    return summary.probeable === 0 ? "均异常" : "部分异常";
  }

  if (summary.canceled > 0 && summary.ready + summary.warning + summary.idle === 0) {
    return "已停止";
  }

  if (summary.warning > 0) {
    return "需注意";
  }

  if (summary.ready > 0 && summary.idle === 0) {
    return "全部可用";
  }

  if (summary.ready > 0) {
    return "部分可用";
  }

  return "待检测";
}

function queueAuthSummaryMeta(summary: QueueAuthSummary) {
  if (summary.probeable === 0) {
    return summary.parseFailed > 0 ? `${summary.parseFailed} 项异常` : null;
  }

  if (summary.unavailable > 0 || summary.parseFailed > 0) {
    return `${summary.ready}/${summary.probeable} 可用 · ${
      summary.unavailable + summary.parseFailed
    } 异常`;
  }

  if (summary.warning > 0) {
    return `${summary.ready}/${summary.probeable} 可用 · ${summary.warning} 需确认`;
  }

  if (summary.canceled > 0) {
    return `${summary.canceled} 项已停止`;
  }

  if (summary.checking > 0) {
    const checked = Math.max(0, summary.probeable - summary.idle - summary.checking);
    return `${checked}/${summary.probeable} 已检测`;
  }

  if (summary.ready > 0) {
    return `${summary.ready}/${summary.probeable} 已检测`;
  }

  return `${summary.probeable} 项待检测`;
}

function queueHighestAuthSites(summary: QueueAuthSummary) {
  const readySites = summary.sites.filter(
    (site) => site.ready > 0 && site.bestFormatLabel,
  );
  const candidates = readySites.length
    ? readySites
    : summary.sites.filter((site) => site.warning > 0 && site.bestFormatLabel);
  const scoredSites = candidates
    .map((site) => ({
      score: bestFormatParts(site.bestFormatLabel).score,
      site,
    }))
    .filter(({ score }) => score > 0);
  const highestScore = Math.max(0, ...scoredSites.map(({ score }) => score));

  return new Set(
    scoredSites
      .filter(({ score }) => score === highestScore)
      .map(({ site }) => site.site),
  );
}

function queueAuthSiteCountText(site: QueueAuthSiteSummary) {
  const counts = [
    site.ready ? `${site.ready} 可用` : null,
    site.warning ? `${site.warning} 注意` : null,
    site.checking ? `${site.checking} 检测中` : null,
    site.canceled ? `${site.canceled} 已停止` : null,
    site.unavailable + site.parseFailed
      ? `${site.unavailable + site.parseFailed} 异常`
      : null,
    site.idle ? `${site.idle} 待检测` : null,
  ].filter(Boolean);

  return counts.join(" / ") || `${site.total} 项`;
}

function queueAuthSiteMessage(site: QueueAuthSiteSummary) {
  const status = site.status ?? queueAuthSiteStatus(site);

  if (status === "checking") {
    return "正在检测可用格式";
  }

  if (status === "idle") {
    return "待检测";
  }

  if (status === "canceled") {
    return site.error ?? "已停止检测";
  }

  if (status === "unavailable" || status === "failed") {
    return site.error ?? "当前站点未能完成视频探测";
  }

  return "未返回明确视频格式";
}

function queueAuthHint(summary: QueueAuthSummary) {
  if (summary.checking > 0) {
    return "正在逐项检测当前浏览器登录态和可下载格式。";
  }

  if (summary.unavailable > 0 || summary.parseFailed > 0) {
    return "部分队列项无法检测，请查看对应队列行的错误原因。";
  }

  if (summary.canceled > 0 && summary.ready + summary.warning + summary.idle === 0) {
    return "登录态检测已停止。";
  }

  if (summary.warning > 0) {
    return "部分链接可能受账号权限、会员或试看限制影响。";
  }

  return null;
}

function betterFormatLabel(
  current?: string | null,
  candidate?: string | null,
) {
  if (!candidate) {
    return current ?? null;
  }

  if (!current) {
    return candidate;
  }

  return formatLabelScore(candidate) > formatLabelScore(current) ? candidate : current;
}

function formatLabelScore(value: string) {
  return bestFormatParts(value).score;
}

function bestFormatParts(value?: string | null): AuthFormatParts {
  const fallbackText = "未返回明确视频格式";
  const fullText = value?.trim();

  if (!fullText) {
    return {
      detail: fallbackText,
      fullText: fallbackText,
      resolution: null,
      score: 0,
    };
  }

  const parts = fullText
    .split(" · ")
    .map((part) => part.trim())
    .filter(Boolean);
  const scoredParts = parts.map((part) => ({
    part,
    score: resolutionScoreFromText(part),
    isActualResolution: isActualResolutionText(part),
    isFallbackResolution: isFallbackResolutionText(part),
  }));
  const resolutionPart = scoredParts.find(
    ({ isActualResolution, score }) => isActualResolution && score > 0,
  );
  const score =
    resolutionPart?.score ??
    scoredParts.find(({ score }) => score > 0)?.score ??
    0;
  const detailParts = parts.filter((part) => {
    if (resolutionPart && part === resolutionPart.part) {
      return false;
    }

    return !isFallbackResolutionText(part);
  });
  const detail = detailParts.join(" · ") || fallbackText;

  if (!resolutionPart) {
    return {
      detail,
      fullText,
      resolution: null,
      score,
    };
  }

  return {
    detail: detail === fallbackText ? "" : detail,
    fullText,
    resolution: resolutionPart.part,
    score,
  };
}

function isActualResolutionText(value: string) {
  return /(\d{3,5})\s*[x×]\s*(\d{3,5})/.test(value.toLowerCase());
}

function isFallbackResolutionText(value: string) {
  const text = value.toLowerCase().trim();

  return (
    /\b\d{3,4}\s*p\b/.test(text) ||
    /\b[248]k\b/.test(text) ||
    text.includes("uhd") ||
    text.includes("qhd")
  );
}

function resolutionScoreFromText(value: string) {
  const text = value.toLowerCase();
  const dimensionMatch = text.match(/(\d{3,5})\s*[x×]\s*(\d{3,5})/);

  if (dimensionMatch) {
    const width = Number(dimensionMatch[1]);
    const height = Number(dimensionMatch[2]);
    return width * height;
  }

  const progressiveMatch = text.match(/\b(\d{3,4})\s*p\b/);

  if (progressiveMatch) {
    const height = Number(progressiveMatch[1]);
    const width = Math.round((height * 16) / 9);
    return width * height;
  }

  if (/\b8k\b/.test(text)) {
    return 7680 * 4320;
  }

  if (/\b4k\b/.test(text) || text.includes("uhd")) {
    return 3840 * 2160;
  }

  if (/\b2k\b/.test(text) || text.includes("qhd")) {
    return 2560 * 1440;
  }

  return 0;
}

function authProbeStateFromResult(
  result: ProbeResponse,
  browser: BrowserKind,
  url: string,
): AuthProbeState {
  return {
    status: "ready",
    browser: result.checkedBrowser ?? browser,
    url,
    site: result.site,
    title: result.title,
    duration: result.duration ?? null,
    bestFormatLabel: result.bestFormatLabel ?? null,
    formatCount: result.formatCount ?? result.formats.length,
    checkedAt: result.checkedAt,
  };
}

function authProbeTriggerText(state: AuthProbeState) {
  if (!state.url) {
    return "待检测";
  }

  if (state.status === "checking") {
    return "检测中";
  }

  if (state.status === "failed") {
    return "检测失败";
  }

  if (state.status === "unavailable") {
    return "无法检测";
  }

  if (state.status === "canceled") {
    return "已停止";
  }

  if (state.status === "warning") {
    return "需注意";
  }

  if (state.status === "ready") {
    return "已检测";
  }

  return "待检测";
}

function authProbeStatusLabel(state: AuthProbeState) {
  if (!state.url) {
    return "待检测";
  }

  if (state.status === "checking") {
    return "检测中";
  }

  if (state.status === "failed") {
    return "检测失败";
  }

  if (state.status === "unavailable") {
    return "无法检测";
  }

  if (state.status === "canceled") {
    return "已停止";
  }

  if (state.status === "warning") {
    return "需注意";
  }

  if (state.status === "ready") {
    return "登录态可用";
  }

  return "待检测";
}

function authProbeStatusMeta(state: AuthProbeState) {
  if (state.status === "checking") {
    return "正在探测视频信息";
  }

  if (state.status === "canceled") {
    return "已停止检测";
  }

  if (state.status === "ready" || state.status === "warning") {
    const parts = [
      state.duration ? formatDuration(state.duration) : null,
      state.formatCount ? `${state.formatCount} 个格式` : null,
      state.checkedAt ? `检测于 ${formatProbeTime(state.checkedAt)}` : null,
    ].filter(Boolean);

    return parts.join(" · ") || null;
  }

  if (!state.url) {
    return "输入链接后可检测";
  }

  return null;
}

function authProbeSiteCountText(state: AuthProbeState) {
  if (state.status === "warning") {
    return "需注意";
  }

  if (state.status === "canceled") {
    return "已停止";
  }

  if (state.formatCount) {
    return `${state.formatCount} 格式`;
  }

  return "可用";
}

function authProbeDetails(state: AuthProbeState) {
  if (
    state.status !== "ready" &&
    state.status !== "warning" &&
    state.status !== "unavailable"
  ) {
    return state.error ?? null;
  }

  if (state.status === "unavailable") {
    return state.error ?? null;
  }

  const parts = [
    state.site ?? null,
    state.duration ? formatDuration(state.duration) : null,
    state.bestFormatLabel ?? null,
    state.formatCount ? `${state.formatCount} 个格式` : null,
    state.checkedAt ? `检测于 ${formatProbeTime(state.checkedAt)}` : null,
  ].filter(Boolean);

  return parts.length ? parts.join(" · ") : null;
}

function authProbeHint(state: AuthProbeState) {
  if (state.status === "failed") {
    return state.error ?? null;
  }

  if (state.status === "unavailable") {
    return "当前链接未能完成视频探测，这通常表示链接失效、视频不可访问或网络暂时不可用。";
  }

  if (state.status === "canceled") {
    return "登录态检测已停止。";
  }

  if (state.status !== "warning") {
    return null;
  }

  const base =
    "如果浏览器能看完整内容，但检测到的时长/画质不完整，请检查网站账号状态或重新检测登录态。";

  return isBilibiliSite(state.site, state.url)
    ? `${base} B 站充电/UPower 内容可能在未读取到登录态时只返回试看流。`
    : base;
}

function isBilibiliSite(site?: string | null, url?: string | null) {
  const value = `${site ?? ""} ${url ?? ""}`.toLowerCase();
  return value.includes("bilibili") || value.includes("b23.tv");
}

function browserLabel(browser: BrowserKind) {
  return browserOptions.find((option) => option.value === browser)?.label ?? browser;
}

function formatProbeTime(value: string) {
  const seconds = Number(value);
  const date = Number.isFinite(seconds) ? new Date(seconds * 1000) : new Date(value);

  if (Number.isNaN(date.getTime())) {
    return "刚刚";
  }

  return date.toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

function formatDuration(duration?: number | null) {
  if (!duration) {
    return "--:--";
  }

  const minutes = Math.floor(duration / 60);
  const seconds = Math.floor(duration % 60);
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

const fallbackFormats: FormatOption[] = [
  {
    id: "best",
    label: "最佳画质 + 最佳音频",
    selector: "bv*+ba/b",
    ext: "mp4",
    resolution: "自动",
  },
  {
    id: "best-mp4",
    label: "优先 MP4",
    selector: "bv*[ext=mp4]+ba[ext=m4a]/b[ext=mp4]/b",
    ext: "mp4",
    resolution: "自动",
  },
  {
    id: "audio",
    label: "仅音频",
    selector: "ba",
    resolution: "audio",
  },
];
