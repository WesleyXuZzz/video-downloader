# 项目说明

## 项目用途

Video Downloader 是一个个人本地桌面视频下载器。应用提供可视化界面录入 `yt-dlp` 支持的网站 URL，通过本机 `yt-dlp` 解析与下载视频，并使用 `ffmpeg` 合并分离的音视频流。应用也提供工具路径配置、工具更新检查、支持站点示例、下载历史记录，以及 FFmpeg 命令生成工具箱。

## 主要目录结构

- `src/`：React + TypeScript 前端界面、类型与样式。
- `src/App.tsx`：主界面、下载流程、历史记录、工具状态面板和 FFmpeg 工具箱。
- `src/tauri.ts`：前端调用 Tauri 命令、文件选择器、事件监听和浏览器预览 mock。
- `src/types.ts`：前后端交互使用的 TypeScript 类型。
- `src-tauri/`：Tauri 2 桌面壳与 Rust 后端命令。
- `src-tauri/src/lib.rs`：依赖检测、工具路径设置、更新检查、支持站点读取、URL 探测、下载任务、取消任务、历史记录、FFmpeg 命令生成等核心逻辑。
- `src-tauri/capabilities/`：Tauri 权限声明。

## 常用命令

- `pnpm install`：安装前端和 Tauri CLI 依赖。
- `pnpm dev`：启动 Vite 前端开发服务。
- `pnpm tauri dev`：启动桌面应用开发模式。
- `pnpm build`：构建前端。
- `pnpm tauri build`：打包 release 桌面应用。
- `./build-app.sh`：安装依赖并打包 release 桌面应用，默认生成 macOS DMG。
- `./build-app.sh app`：只生成 macOS `.app` 包。
- `./build-app.sh all`：生成 Tauri 配置中的所有 bundle 目标。
- `pnpm typecheck`：执行 TypeScript 类型检查。

## 外部依赖

首版不内置下载工具，依赖本机可执行文件：

- `yt-dlp`：解析与下载视频。
- `ffmpeg`：合并分离的音视频流。

应用会优先使用用户手动配置的工具路径，其次使用环境变量和 `PATH` 中可发现的工具。手动配置会写入应用数据目录的 `tool-settings.json`，下载历史会写入应用数据目录的 `history.json`。

macOS 推荐安装命令：

```bash
brew install yt-dlp ffmpeg
```

Tauri 开发还需要 Rust/Cargo：

```bash
curl --proto '=https' --tlsv1.2 https://sh.rustup.rs -sSf | sh
```

## 重要约束

- 不在项目内保存 cookies、账号密码或登录令牌。
- 登录态只通过 `yt-dlp --cookies-from-browser` 从本机浏览器读取。
- 只下载用户有权下载或离线保存的内容。
- 默认不做远程下载服务，所有下载任务在本机执行。
- FFmpeg 工具箱只生成、复制或带入终端命令；默认不在应用内直接执行转码命令。
- 不要未经确认批量删除文件或目录，尤其不要使用递归强制删除命令。
- 打包命令默认面向 release；如果新增打包脚本，优先命名为 `build-app.sh`。
