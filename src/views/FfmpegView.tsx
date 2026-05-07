import { Alert, Button, Card, Input, Select, Space, Tooltip, Typography } from "antd";
import {
  AudioOutlined,
  CodeOutlined,
  ConsoleSqlOutlined,
  CompressOutlined,
  CopyOutlined,
  ExportOutlined,
  FileAddOutlined,
  FileSyncOutlined,
  FolderOpenOutlined,
  MergeCellsOutlined,
  ScissorOutlined,
} from "@ant-design/icons";
import type { ReactNode } from "react";
import PanelTitle from "../components/PanelTitle";
import type { FfmpegCommandDraft, FfmpegPresetId } from "../types";

const { Text } = Typography;

type Notice = {
  type: "success" | "warning" | "error" | "info";
  text: string;
};

type FfmpegViewProps = {
  audioFormat: "mp3" | "m4a";
  canBuildCommand: boolean;
  crf: number;
  draft: FfmpegCommandDraft | null;
  endTime: string;
  inputPath: string;
  isBuilding: boolean;
  isPrefillingTerminal: boolean;
  notice: Notice | null;
  onAudioFormatChange: (value: "mp3" | "m4a") => void;
  onBuildDraft: () => void;
  onChooseInputFile: () => void;
  onChooseOutputDirectory: () => void;
  onChooseSecondaryFile: () => void;
  onCopyCommand: () => void;
  onCrfChange: (value: number) => void;
  onEndTimeChange: (value: string) => void;
  onInputPathChange: (value: string) => void;
  onOutputDirChange: (value: string) => void;
  onPrefillTerminal: () => void;
  onPresetChange: (value: FfmpegPresetId) => void;
  onSecondaryInputPathChange: (value: string) => void;
  onStartTimeChange: (value: string) => void;
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

export default function FfmpegView({
  audioFormat,
  canBuildCommand,
  crf,
  draft,
  endTime,
  inputPath,
  isBuilding,
  isPrefillingTerminal,
  notice,
  onAudioFormatChange,
  onBuildDraft,
  onChooseInputFile,
  onChooseOutputDirectory,
  onChooseSecondaryFile,
  onCopyCommand,
  onCrfChange,
  onEndTimeChange,
  onInputPathChange,
  onOutputDirChange,
  onPrefillTerminal,
  onPresetChange,
  onSecondaryInputPathChange,
  onStartTimeChange,
  onUseDownloadedFile,
  outputDir,
  outputPath,
  preset,
  secondaryInputPath,
  startTime,
}: FfmpegViewProps) {
  return (
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
            type="primary"
          >
            带入终端
          </Button>
        </Space>

        {notice ? (
          <Alert
            className="ffmpeg-notice"
            message={notice.text}
            showIcon
            type={notice.type}
          />
        ) : null}
      </Space>
    </Card>
  );
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
