import { Button, Card, Empty, List, Space, Tag, Tooltip, Typography } from "antd";
import {
  ClockCircleOutlined,
  DeleteOutlined,
  FileAddOutlined,
  FolderOpenOutlined,
  ReloadOutlined,
} from "@ant-design/icons";
import type { KeyboardEvent } from "react";
import PanelTitle from "../components/PanelTitle";
import {
  formatHistoryDate,
  historySite,
  historyTitle,
  statusCopy,
  statusTagColor,
} from "../historyUtils";
import {
  localMediaErrorTitle,
  localMediaSummary,
  mediaComparisonSummary,
} from "../mediaInfo";
import { revealFile } from "../tauri";
import type { DownloadHistoryItem } from "../types";

const { Text } = Typography;

export default function HistoryView({
  history,
  onApplyHistoryItem,
  onDeleteHistoryItem,
  onHistoryKeyDown,
  onOpenHistoryItem,
  onRefresh,
}: {
  history: DownloadHistoryItem[];
  onApplyHistoryItem: (item: DownloadHistoryItem) => void;
  onDeleteHistoryItem: (item: DownloadHistoryItem) => void;
  onHistoryKeyDown: (
    event: KeyboardEvent<HTMLDivElement>,
    item: DownloadHistoryItem,
  ) => void;
  onOpenHistoryItem: (item: DownloadHistoryItem) => void;
  onRefresh: () => void;
}) {
  return (
    <Card
      className="history-page-card"
      title={<PanelTitle icon={<ClockCircleOutlined />} label="下载历史" />}
      extra={
        <Button icon={<ReloadOutlined />} onClick={onRefresh}>
          刷新
        </Button>
      }
    >
      {history.length === 0 ? (
        <Empty description="暂无下载历史" image={Empty.PRESENTED_IMAGE_SIMPLE} />
      ) : (
        <List
          className="history-page-list"
          dataSource={history}
          renderItem={(item, index) => {
            const title = historyTitle(item);
            const site = historySite(item);
            const itemStatus = statusCopy[item.status] ?? "未知状态";
            const mediaText = localMediaSummary(item.localMedia);
            const mediaTitle = localMediaErrorTitle(item.localMedia);
            const comparisonText = mediaComparisonSummary(item.mediaComparison);

            return (
              <List.Item
                className="history-page-item"
                key={item.id || `${item.url}-${index}`}
                onClick={() => onOpenHistoryItem(item)}
                onKeyDown={(event) => onHistoryKeyDown(event, item)}
                role="button"
                tabIndex={0}
              >
                <div className="history-page-copy">
                  <div className="history-page-title-row">
                    <Text className="history-page-name">{title}</Text>
                    <Tag color={statusTagColor(item.status)}>{itemStatus}</Tag>
                  </div>
                  <Text className="history-page-meta" type="secondary">
                    {site} · {formatHistoryDate(item.updatedAt)}
                  </Text>
                  <div className="history-page-details">
                    <Text type="secondary">格式：{item.format || "-"}</Text>
                    <Text type="secondary">目录：{item.outputDir || "-"}</Text>
                  </div>
                  {mediaText ? (
                    <Tooltip title={mediaTitle}>
                      <Text className="history-page-media" type="secondary">
                        {mediaText}
                      </Text>
                    </Tooltip>
                  ) : null}
                  {comparisonText ? (
                    <Text className="history-page-media-comparison" type="secondary">
                      {comparisonText}
                    </Text>
                  ) : null}
                  {item.error ? (
                    <Text className="history-page-error" type="danger">
                      {item.error}
                    </Text>
                  ) : null}
                </div>
                <Space className="history-page-actions" size={8} wrap>
                  <Tooltip title="填入下载">
                    <Button
                      aria-label={`填入下载 ${title}`}
                      className="history-action-button"
                      icon={<FileAddOutlined />}
                      onKeyDown={(event) => event.stopPropagation()}
                      onClick={(event) => {
                        event.stopPropagation();
                        onApplyHistoryItem(item);
                      }}
                    />
                  </Tooltip>
                  {item.outputPath ? (
                    <Tooltip title="打开文件位置">
                      <Button
                        aria-label={`打开文件位置 ${title}`}
                        className="history-action-button"
                        icon={<FolderOpenOutlined />}
                        onKeyDown={(event) => event.stopPropagation()}
                        onClick={(event) => {
                          event.stopPropagation();
                          revealFile(item.outputPath as string);
                        }}
                      />
                    </Tooltip>
                  ) : null}
                  <Tooltip title="删除历史记录">
                    <Button
                      aria-label={`删除 ${title}`}
                      className="history-action-button history-delete-button"
                      icon={<DeleteOutlined />}
                      onKeyDown={(event) => event.stopPropagation()}
                      onClick={(event) => {
                        event.stopPropagation();
                        onDeleteHistoryItem(item);
                      }}
                    />
                  </Tooltip>
                </Space>
              </List.Item>
            );
          }}
        />
      )}
    </Card>
  );
}
