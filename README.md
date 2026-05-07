# Video Downloader

Video Downloader 是一个个人本地桌面视频下载器。它提供可视化界面录入 `yt-dlp` 支持的网站 URL，在本机调用 `yt-dlp` 解析和下载视频，并通过 `ffmpeg` 合并分离的音视频流。

English summary: Video Downloader is a local desktop app for downloading videos with `yt-dlp` and merging media streams with `ffmpeg`.

## 功能

- 输入视频 URL 并探测可用格式。
- 选择浏览器登录态来源，支持通过 `yt-dlp --cookies-from-browser` 读取本机浏览器 cookies。
- 手动配置或自动发现 `yt-dlp`、`ffmpeg` 可执行文件。
- 查看工具版本和更新提示。
- 管理下载进度、取消任务和下载历史。
- 生成 FFmpeg 常用命令，并复制或带入终端。
- 管理 FFmpeg 命令历史，支持复用、复制、筛选、分页、删除和清空历史命令。

## 依赖要求

首版不内置下载工具，需要用户在本机安装：

- `yt-dlp`
- `ffmpeg`
- Rust/Cargo，用于 Tauri 开发和打包
- Node.js 与 pnpm

macOS 推荐安装：

```bash
brew install yt-dlp ffmpeg
```

安装 Rust：

```bash
curl --proto '=https' --tlsv1.2 https://sh.rustup.rs -sSf | sh
```

## 开发

```bash
pnpm install
pnpm dev
pnpm tauri dev
```

常用检查：

```bash
pnpm typecheck
pnpm build
```

## 打包

默认生成 macOS DMG：

```bash
./build-app.sh
```

只生成 `.app`：

```bash
./build-app.sh app
```

生成 Tauri 配置中的所有 bundle 目标：

```bash
./build-app.sh all
```

也可以直接运行：

```bash
pnpm tauri build
```

## 数据与隐私

- 应用不在项目目录内保存 cookies、账号密码或登录令牌。
- 手动配置的工具路径会写入应用数据目录的 `tool-settings.json`。
- 下载历史会写入应用数据目录的 `history.json`。
- FFmpeg 命令历史会写入应用数据目录的 `ffmpeg-command-history.json`，其中可能包含用户选择的本地媒体文件路径。
- 下载任务在用户本机执行，不提供远程下载服务。

## 合规说明

请只下载你有权下载或离线保存的内容。本项目不鼓励、不支持绕过版权保护、DRM、付费墙、访问控制或平台服务条款。

`yt-dlp` 和 `ffmpeg` 是用户本机外部依赖，本项目不内置它们的二进制文件。如果未来发布包内置这些工具，需要按各自许可证要求提供署名、许可证文本、源码或下载链接。

## 许可证

本项目使用 MIT License，见 [LICENSE](./LICENSE)。
