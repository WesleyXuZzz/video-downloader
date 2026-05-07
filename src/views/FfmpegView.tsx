import {
  Alert,
  Button,
  Card,
  Checkbox,
  Empty,
  Input,
  List,
  Pagination,
  Select,
  Space,
  Tag,
  Tooltip,
  Typography,
} from "antd";
import {
  AudioOutlined,
  CheckSquareOutlined,
  CodeOutlined,
  ConsoleSqlOutlined,
  CompressOutlined,
  CopyOutlined,
  DeleteOutlined,
  ExportOutlined,
  FileAddOutlined,
  FileSyncOutlined,
  FolderOpenOutlined,
  MergeCellsOutlined,
  ReloadOutlined,
  ScissorOutlined,
} from "@ant-design/icons";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import PanelTitle from "../components/PanelTitle";
import { formatHistoryDate } from "../historyUtils";
import type {
  FfmpegCommandDraft,
  FfmpegCommandHistoryItem,
  FfmpegPresetId,
} from "../types";

const { Text } = Typography;

type Notice = {
  type: "success" | "warning" | "error" | "info";
  text: string;
};

type PresetFilter = FfmpegPresetId | "all";

type FfmpegViewProps = {
  audioFormat: "mp3" | "m4a";
  canBuildCommand: boolean;
  commandHistory: FfmpegCommandHistoryItem[];
  crf: number;
  draft: FfmpegCommandDraft | null;
  endTime: string;
  inputPath: string;
  isBuilding: boolean;
  isHistoryLoading: boolean;
  isHistoryMutating: boolean;
  isPrefillingTerminal: boolean;
  notice: Notice | null;
  onAudioFormatChange: (value: "mp3" | "m4a") => void;
  onBuildDraft: () => void;
  onChooseInputFile: () => void;
  onChooseOutputDirectory: () => void;
  onChooseSecondaryFile: () => void;
  onClearHistory: () => void;
  onCopyCommand: () => void;
  onCopyHistoryCommand: (command: string) => void;
  onCrfChange: (value: number) => void;
  onDeleteHistoryItems: (ids: string[]) => void;
  onEndTimeChange: (value: string) => void;
  onInputPathChange: (value: string) => void;
  onOutputDirChange: (value: string) => void;
  onPrefillTerminal: () => void;
  onPresetChange: (value: FfmpegPresetId) => void;
  onRefreshHistory: () => void;
  onSecondaryInputPathChange: (value: string) => void;
  onStartTimeChange: (value: string) => void;
  onUseHistoryItem: (item: FfmpegCommandHistoryItem) => void;
  onUseDownloadedFile: () => void;
  outputDir: string;
  outputPath: string | null;
  preset: FfmpegPresetId;
  secondaryInputPath: string;
  startTime: string;
};

const ffmpegPresetOptions: Array<{ label: ReactNode; value: FfmpegPresetId }> = [
  {
    label: <FfmpegPresetOption icon={<FileSyncOutlined />} label="转 MP4" />,
    value: "convertMp4",
  },
  {
    label: <FfmpegPresetOption icon={<CompressOutlined />} label="压缩视频" />,
    value: "compress",
  },
  {
    label: <FfmpegPresetOption icon={<AudioOutlined />} label="提取音频" />,
    value: "extractAudio",
  },
  {
    label: <FfmpegPresetOption icon={<ScissorOutlined />} label="截取片段" />,
    value: "trim",
  },
  {
    label: (
      <FfmpegPresetOption icon={<MergeCellsOutlined />} label="合并音视频" />
    ),
    value: "mergeAudioVideo",
  },
];

const audioFormatOptions = [
  { label: "MP3", value: "mp3" },
  { label: "M4A", value: "m4a" },
];

const crfOptions = [
  { label: "轻压缩 · CRF 23", value: 23 },
  { label: "均衡 · CRF 26", value: 26 },
  { label: "更小体积 · CRF 28", value: 28 },
  { label: "极小体积 · CRF 30", value: 30 },
];

const pageSizeOptions = [20, 50, 200];

const presetCopy: Record<FfmpegPresetId, string> = {
  convertMp4: "转 MP4",
  compress: "压缩视频",
  extractAudio: "提取音频",
  trim: "截取片段",
  mergeAudioVideo: "合并音视频",
};

const ffmpegHistoryPresetFilterOptions: Array<{
  label: string;
  value: PresetFilter;
}> = [
  { label: "全部", value: "all" },
  { label: presetCopy.convertMp4, value: "convertMp4" },
  { label: presetCopy.compress, value: "compress" },
  { label: presetCopy.extractAudio, value: "extractAudio" },
  { label: presetCopy.trim, value: "trim" },
  { label: presetCopy.mergeAudioVideo, value: "mergeAudioVideo" },
];

export default function FfmpegView({
  audioFormat,
  canBuildCommand,
  commandHistory,
  crf,
  draft,
  endTime,
  inputPath,
  isBuilding,
  isHistoryLoading,
  isHistoryMutating,
  isPrefillingTerminal,
  notice,
  onAudioFormatChange,
  onBuildDraft,
  onChooseInputFile,
  onChooseOutputDirectory,
  onChooseSecondaryFile,
  onClearHistory,
  onCopyCommand,
  onCopyHistoryCommand,
  onCrfChange,
  onDeleteHistoryItems,
  onEndTimeChange,
  onInputPathChange,
  onOutputDirChange,
  onPrefillTerminal,
  onPresetChange,
  onRefreshHistory,
  onSecondaryInputPathChange,
  onStartTimeChange,
  onUseHistoryItem,
  onUseDownloadedFile,
  outputDir,
  outputPath,
  preset,
  secondaryInputPath,
  startTime,
}: FfmpegViewProps) {
  return (
    <Space
      className="ffmpeg-workbench"
      direction="vertical"
      size={18}
    >
      <Card
        className="ffmpeg-card"
        title={<PanelTitle icon={<CodeOutlined />} label="命令配置" />}
      >
        <Space className="form-stack" direction="vertical" size={16}>
          <div className="form-grid">
            <div className="field-block">
              <Text strong>预设</Text>
              <Select<FfmpegPresetId>
                className="control-select"
                classNames={{
                  popup: { root: "control-select-dropdown" },
                }}
                onChange={onPresetChange}
                options={ffmpegPresetOptions}
                value={preset}
              />
            </div>

            <div className="field-block">
              <Text strong>输出目录</Text>
              <div className="path-command-row">
                <div className="path-input-wrap">
                  <span className="path-input-icon">
                    <FolderOpenOutlined />
                  </span>
                  <Input
                    className="path-input"
                    onChange={(event) => onOutputDirChange(event.target.value)}
                    value={outputDir}
                  />
                </div>
                <Tooltip title="选择输出目录">
                  <Button
                    className="path-picker-button"
                    aria-label="选择输出目录"
                    onClick={onChooseOutputDirectory}
                    type="primary"
                  >
                    选择
                  </Button>
                </Tooltip>
              </div>
            </div>
          </div>

          <div className="field-block">
            <Text strong>输入文件</Text>
            <div className="path-command-row">
              <div className="path-input-wrap">
                <span className="path-input-icon">
                  <FileAddOutlined />
                </span>
                <Input
                  className="path-input"
                  onChange={(event) => onInputPathChange(event.target.value)}
                  placeholder="/path/to/video.mp4"
                  value={inputPath}
                />
              </div>
              <Tooltip title="选择输入文件">
                <Button
                  className="path-picker-button"
                  aria-label="选择输入文件"
                  onClick={onChooseInputFile}
                  type="primary"
                >
                  选择
                </Button>
              </Tooltip>
            </div>
          </div>

          {preset === "mergeAudioVideo" ? (
            <div className="field-block">
              <Text strong>音频文件</Text>
              <div className="path-command-row">
                <div className="path-input-wrap">
                  <span className="path-input-icon">
                    <FileAddOutlined />
                  </span>
                  <Input
                    className="path-input"
                    onChange={(event) =>
                      onSecondaryInputPathChange(event.target.value)
                    }
                    placeholder="/path/to/audio.m4a"
                    value={secondaryInputPath}
                  />
                </div>
                <Tooltip title="选择音频文件">
                  <Button
                    className="path-picker-button"
                    aria-label="选择音频文件"
                    onClick={onChooseSecondaryFile}
                    type="primary"
                  >
                    选择
                  </Button>
                </Tooltip>
              </div>
            </div>
          ) : null}

          <FfmpegPresetControls
            audioFormat={audioFormat}
            crf={crf}
            endTime={endTime}
            onAudioFormatChange={onAudioFormatChange}
            onCrfChange={onCrfChange}
            onEndTimeChange={onEndTimeChange}
            onStartTimeChange={onStartTimeChange}
            preset={preset}
            startTime={startTime}
          />

          {outputPath ? (
            <Space className="ffmpeg-inline-actions" size={8} wrap>
              <Button icon={<ExportOutlined />} onClick={onUseDownloadedFile}>
                使用刚下载的文件
              </Button>
            </Space>
          ) : null}

          <div className="field-block">
            <Text strong>命令预览</Text>
            <Input.TextArea
              autoSize={{ minRows: 3, maxRows: 7 }}
              className="command-preview"
              readOnly
              value={draft?.command ?? ""}
            />
            {draft ? (
              <Text className="ffmpeg-output-path" type="secondary">
                输出：{draft.outputPath}
              </Text>
            ) : null}
          </div>

          <Space className="command-row ffmpeg-command-actions" size={14} wrap>
            <Button
              className="ffmpeg-action-button is-build"
              disabled={!canBuildCommand}
              icon={<CodeOutlined />}
              loading={isBuilding}
              onClick={onBuildDraft}
            >
              生成命令
            </Button>
            <Button
              className="ffmpeg-action-button is-copy"
              disabled={!canBuildCommand}
              icon={<CopyOutlined />}
              onClick={onCopyCommand}
            >
              复制命令
            </Button>
            <Button
              className="ffmpeg-action-button is-terminal"
              disabled={!canBuildCommand}
              icon={<ConsoleSqlOutlined />}
              loading={isPrefillingTerminal}
              onClick={onPrefillTerminal}
            >
              带入终端
            </Button>
          </Space>

          <div
            className={`ffmpeg-notice-region ${notice ? "has-notice" : ""}`}
          >
            {notice ? (
              <Alert
                className="ffmpeg-notice"
                message={notice.text}
                showIcon
                type={notice.type}
              />
            ) : null}
          </div>
        </Space>
      </Card>

      <FfmpegCommandHistoryCard
        history={commandHistory}
        isLoading={isHistoryLoading}
        isMutating={isHistoryMutating}
        onClearHistory={onClearHistory}
        onCopyCommand={onCopyHistoryCommand}
        onDeleteItems={onDeleteHistoryItems}
        onRefresh={onRefreshHistory}
        onUseItem={onUseHistoryItem}
      />
    </Space>
  );
}

function FfmpegCommandHistoryCard({
  history,
  isLoading,
  isMutating,
  onClearHistory,
  onCopyCommand,
  onDeleteItems,
  onRefresh,
  onUseItem,
}: {
  history: FfmpegCommandHistoryItem[];
  isLoading: boolean;
  isMutating: boolean;
  onClearHistory: () => void;
  onCopyCommand: (command: string) => void;
  onDeleteItems: (ids: string[]) => void;
  onRefresh: () => void;
  onUseItem: (item: FfmpegCommandHistoryItem) => void;
}) {
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [isSelecting, setIsSelecting] = useState(false);
  const [presetFilter, setPresetFilter] = useState<PresetFilter>("all");
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(20);

  const selectedIdSet = useMemo(() => new Set(selectedIds), [selectedIds]);
  const filteredHistory = useMemo(() => {
    if (presetFilter === "all") {
      return history;
    }

    return history.filter((item) => item.presetId === presetFilter);
  }, [history, presetFilter]);
  const pagedHistory = useMemo(() => {
    const start = (page - 1) * pageSize;
    return filteredHistory.slice(start, start + pageSize);
  }, [filteredHistory, page, pageSize]);

  useEffect(() => {
    setSelectedIds((ids) =>
      ids.filter((id) => history.some((item) => item.id === id)),
    );
    if (history.length === 0) {
      setIsSelecting(false);
    }
  }, [history]);

  useEffect(() => {
    const maxPage = Math.max(1, Math.ceil(filteredHistory.length / pageSize));
    if (page > maxPage) {
      setPage(maxPage);
    }
  }, [filteredHistory.length, page, pageSize]);

  useEffect(() => {
    setPage(1);
    setSelectedIds([]);
    setIsSelecting(false);
  }, [presetFilter]);

  const shouldShowPagination = filteredHistory.length > 0;
  const historyCountCopy = historySummaryCopy(
    history.length,
    filteredHistory.length,
    selectedIds.length,
    presetFilter,
  );

  function toggleSelecting() {
    setIsSelecting((value) => {
      if (value) {
        setSelectedIds([]);
      }

      return !value;
    });
  }

  function toggleItem(id: string, checked: boolean) {
    setSelectedIds((ids) =>
      checked ? [...new Set([...ids, id])] : ids.filter((item) => item !== id),
    );
  }

  function handlePresetFilterChange(value: PresetFilter) {
    setPresetFilter(value);
  }

  function deleteSelected() {
    onDeleteItems(selectedIds);
  }

  function clearAll() {
    setSelectedIds([]);
    setIsSelecting(false);
    onClearHistory();
  }

  return (
    <Card
      className="ffmpeg-history-card"
      title={<PanelTitle icon={<CheckSquareOutlined />} label="命令历史" />}
      extra={
        <Space size={8} wrap>
          <Button
            className="ffmpeg-history-head-button is-refresh"
            icon={<ReloadOutlined />}
            loading={isLoading}
            onClick={onRefresh}
          >
            刷新
          </Button>
          <Button
            className="ffmpeg-history-head-button is-select"
            disabled={filteredHistory.length === 0}
            icon={<CheckSquareOutlined />}
            onClick={toggleSelecting}
          >
            {isSelecting ? "取消选择" : "选择"}
          </Button>
          <Button
            className="ffmpeg-history-head-button is-delete-selected"
            disabled={selectedIds.length === 0}
            icon={<DeleteOutlined />}
            loading={isMutating && selectedIds.length > 0}
            onClick={deleteSelected}
          >
            删除选中
          </Button>
          <Button
            className="ffmpeg-history-head-button is-clear"
            disabled={history.length === 0}
            icon={<DeleteOutlined />}
            loading={isMutating && selectedIds.length === 0}
            onClick={clearAll}
          >
            清空全部
          </Button>
        </Space>
      }
    >
      {history.length === 0 ? (
        <Empty
          className="ffmpeg-history-empty"
          description="暂无命令历史"
          image={Empty.PRESENTED_IMAGE_SIMPLE}
        />
      ) : (
        <>
          <div
            className={`ffmpeg-history-table-head ${isSelecting ? "is-selecting" : ""}`}
          >
            {isSelecting ? <span aria-hidden="true" /> : null}
            <div className="ffmpeg-history-preset-heading">
              <Text strong>预设类型</Text>
              <Select<PresetFilter>
                className="ffmpeg-history-filter"
                onChange={handlePresetFilterChange}
                options={ffmpegHistoryPresetFilterOptions}
                value={presetFilter}
              />
            </div>
            <Text strong>命令</Text>
          </div>
          {filteredHistory.length === 0 ? (
            <Empty
              className="ffmpeg-history-empty"
              description="该预设暂无命令历史"
              image={Empty.PRESENTED_IMAGE_SIMPLE}
            />
          ) : (
            <List
              className="ffmpeg-history-list"
              dataSource={pagedHistory}
              loading={isLoading}
              renderItem={(item) => (
                <List.Item
                  className={`ffmpeg-history-item ${isSelecting ? "is-selecting" : ""}`}
                  key={item.id}
                >
                  {isSelecting ? (
                    <Checkbox
                      checked={selectedIdSet.has(item.id)}
                      onChange={(event) =>
                        toggleItem(item.id, event.target.checked)
                      }
                    />
                  ) : null}
                  <div className="ffmpeg-history-preset-cell">
                    <Tag color="blue">{presetCopy[item.presetId]}</Tag>
                    <Text className="ffmpeg-history-time" type="secondary">
                      {formatHistoryDate(item.createdAt)}
                    </Text>
                  </div>
                  <div className="ffmpeg-history-command-cell">
                    <Input.TextArea
                      autoSize={{ minRows: 2, maxRows: 5 }}
                      className="command-preview ffmpeg-history-command"
                      readOnly
                      value={item.command}
                    />
                    <Space className="ffmpeg-history-actions" size={8} wrap>
                      <Tooltip title="复制命令">
                        <Button
                          aria-label="复制历史命令"
                          className="history-action-button"
                          icon={<CopyOutlined />}
                          onClick={() => onCopyCommand(item.command)}
                        />
                      </Tooltip>
                      <Tooltip title="填回表单">
                        <Button
                          aria-label="填回历史命令"
                          className="history-action-button"
                          icon={<FileAddOutlined />}
                          onClick={() => onUseItem(item)}
                        />
                      </Tooltip>
                    </Space>
                  </div>
                </List.Item>
              )}
            />
          )}
        </>
      )}
      <div className="ffmpeg-history-footer">
        {shouldShowPagination ? (
          <Pagination
            className="ffmpeg-history-pagination"
            current={page}
            onChange={(nextPage, nextPageSize) => {
              setPage(nextPage);
              setPageSize(nextPageSize);
            }}
            onShowSizeChange={(_, nextPageSize) => {
              setPage(1);
              setPageSize(nextPageSize);
            }}
            pageSize={pageSize}
            pageSizeOptions={pageSizeOptions}
            showSizeChanger
            total={filteredHistory.length}
          />
        ) : null}
        <Text className="ffmpeg-history-count" type="secondary">
          {historyCountCopy}
        </Text>
      </div>
    </Card>
  );
}

function historySummaryCopy(
  totalCount: number,
  filteredCount: number,
  selectedCount: number,
  presetFilter: PresetFilter,
) {
  if (presetFilter === "all") {
    return selectedCount > 0
      ? `已选 ${selectedCount} 条 / 共 ${totalCount} 条`
      : `共 ${totalCount} 条`;
  }

  return selectedCount > 0
    ? `已选 ${selectedCount} 条 / 筛选 ${filteredCount} 条 / 共 ${totalCount} 条`
    : `筛选 ${filteredCount} 条 / 共 ${totalCount} 条`;
}

function FfmpegPresetOption({
  icon,
  label,
}: {
  icon: ReactNode;
  label: string;
}) {
  return (
    <span className="ffmpeg-preset-option">
      <span className="ffmpeg-preset-option-icon">{icon}</span>
      <span>{label}</span>
    </span>
  );
}

function FfmpegPresetControls({
  audioFormat,
  crf,
  endTime,
  onAudioFormatChange,
  onCrfChange,
  onEndTimeChange,
  onStartTimeChange,
  preset,
  startTime,
}: {
  audioFormat: "mp3" | "m4a";
  crf: number;
  endTime: string;
  onAudioFormatChange: (value: "mp3" | "m4a") => void;
  onCrfChange: (value: number) => void;
  onEndTimeChange: (value: string) => void;
  onStartTimeChange: (value: string) => void;
  preset: FfmpegPresetId;
  startTime: string;
}) {
  if (preset === "extractAudio") {
    return (
      <div className="form-grid ffmpeg-options-grid">
        <div className="field-block">
          <Text strong>音频格式</Text>
          <Select<"mp3" | "m4a">
            className="control-select"
            onChange={onAudioFormatChange}
            options={audioFormatOptions}
            value={audioFormat}
          />
        </div>
      </div>
    );
  }

  if (preset === "compress") {
    return (
      <div className="form-grid ffmpeg-options-grid">
        <div className="field-block">
          <Text strong>压缩强度</Text>
          <Select<number>
            className="control-select"
            onChange={onCrfChange}
            options={crfOptions}
            value={crf}
          />
        </div>
      </div>
    );
  }

  if (preset === "trim") {
    return (
      <div className="form-grid ffmpeg-options-grid">
        <div className="field-block">
          <Text strong>开始时间</Text>
          <Input
            className="standalone-input"
            onChange={(event) => onStartTimeChange(event.target.value)}
            placeholder="00:00:00"
            value={startTime}
          />
        </div>
        <div className="field-block">
          <Text strong>结束时间</Text>
          <Input
            className="standalone-input"
            onChange={(event) => onEndTimeChange(event.target.value)}
            placeholder="00:00:30"
            value={endTime}
          />
        </div>
      </div>
    );
  }

  return null;
}
