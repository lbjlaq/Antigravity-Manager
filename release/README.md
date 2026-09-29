# Antigravity Tools — 无 GUI 服务版（免 WebView2）本地运行包

本目录是一个**自包含**的服务运行包：不依赖 WebView2 / 任何浏览器内核，
AI 网关 + `/api` 管理面 + WebUI 全部由一个二进制 + 静态 `dist/` 提供。

## 一键启动

- **Windows**：双击 `start-webui.bat`（或命令行运行）
- **Linux / macOS**：`chmod +x start-webui.sh && ./start-webui.sh`

脚本会以 `--headless --open` 启动服务，并自动用系统默认浏览器打开 Web UI。

- 访问地址：`http://localhost:8045`（AI 代理端点：`http://localhost:8045/v1`，健康检查：`/health`）
- 登录密码：优先使用 `WEB_PASSWORD` 环境变量；未设置时与 API Key 相同；首次启动若两者皆无，
  控制台会打印自动生成的随机密钥（同时写入 `%USERPROFILE%\.antigravity_tools\gui_config.json`）。
- 数据目录：`%USERPROFILE%\.antigravity_tools`（Linux/macOS: `~/.antigravity_tools`）

## 常用环境变量（可在启动前设置）

| 变量 | 说明 | 默认 |
| --- | --- | --- |
| `WEB_PASSWORD` | Web UI 登录密码 | 同 API Key |
| `API_KEY` | AI 代理 API Key | 首次运行自动生成 |
| `PORT` | 服务端口 | `8045` |
| `ABV_BIND_LOCAL_ONLY` | `1` 仅绑定 127.0.0.1（一键脚本默认）；`0` 允许局域网访问 | 脚本内默认 `1` |
| `ABV_DIST_PATH` | WebUI 静态资源目录 | 脚本内指向 `./dist` |

## 目录结构

```
release/
├── antigravity-tools.exe   # 服务二进制（--no-default-features 构建，不链接 WebView2）
├── dist/                   # WebUI 静态资源（React 构建产物）
├── start-webui.bat         # Windows 一键启动
├── start-webui.sh          # Linux/macOS 一键启动
└── README.md
```

## 与桌面版的区别

- 无托盘、迷你窗口、自动安装更新（自动更新通道仅覆盖 GUI 桌面包，服务版请随版本更新本包）；
- WebUI 在浏览器中运行，功能与桌面窗口一致（含实时事件推送、.vscdb 上传导入等）；
- 完整设计与实现说明见仓库 `docs/WEBUI_NO_WEBVIEW2_PLAN.md`。

## 停止服务

在启动窗口按 `Ctrl+C`。如需后台常驻，建议注册为计划任务或系统服务（Windows 可用 NSSM）。
