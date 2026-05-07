# Contributing

感谢你考虑贡献 Video Downloader。

## 开发流程

1. Fork 仓库并创建功能分支。
2. 安装依赖：

```bash
pnpm install
```

3. 本地开发：

```bash
pnpm tauri dev
```

4. 提交前运行检查：

```bash
pnpm typecheck
pnpm build
```

## 贡献要求

- 不提交下载的视频、音频、cookies、账号信息、日志、证书或本机私密配置。
- 不引入绕过 DRM、付费墙、访问控制或平台限制的功能。
- 新增下载、探测或工具路径逻辑时，优先保持所有任务在用户本机执行。
- 修改打包流程时，默认面向 release 版本，并优先沿用 `build-app.sh`。

## 问题反馈

提交 issue 时请尽量包含：

- 操作系统与版本。
- `yt-dlp` 和 `ffmpeg` 版本。
- 应用版本或提交号。
- 可复现步骤和错误信息。

请不要在 issue 中粘贴 cookies、登录令牌、私有 URL 或其他敏感信息。
