import { Space } from "antd";
import type { ReactNode } from "react";

export default function PanelTitle({
  icon,
  label,
}: {
  icon: ReactNode;
  label: string;
}) {
  return (
    <Space size={8}>
      {icon}
      <span>{label}</span>
    </Space>
  );
}
