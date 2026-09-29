# Project Maintenance Guidelines

## What This Project Is（先读这个）

- 本项目是 [lbjlaq/Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager)（上游 4.8.4）的 **Windows-only 无 WebView2 fork**：同一套 React 前端 + Rust 网关，剥离对 WebView2 / wry / WebKitGTK 的编译依赖。
- **单一维护形态：无 WebView 服务版**（2026-09 起唯一维护目标）：`--no-default-features` 构建的单二进制 + 系统浏览器 WebUI，单端口 8045 同时承载静态页面、`/api` 管理面与 AI 代理（`/v1`）；Windows 下带系统托盘（`modules/tray_headless.rs`，独立线程 + Win32 消息泵，`ANTIGRAVITY_DISABLE_TRAY=1` 可禁用）。
- **桌面 GUI 形态（`gui` feature / 默认构建）不再维护**：`gui` feature 与相关代码仅为保持可编译而保留，不接受新功能、不修复运行时问题、不出桌面包；不要把开发精力投入 Tauri 窗口 / WebView2 / 桌面托盘 / 自动更新路径。
- 去 WebView2 改造（阶段 0-5）已全部完成，设计/实现/取舍记录见 `docs/WEBUI_NO_WEBVIEW2_PLAN.md`；`tauri` 以 `default-features = false` 引入，仅为复用其核心（IPC 状态管理、命令注册），运行期不构造窗口。
- 遵循上游的协议管线设计决策；**不同步**上游的发布/更新通道与多平台分发逻辑（见下方维护范围）。

## Platform Scope: Windows-Only（2026-09 起）

- 仅积极维护 **Windows**（无 WebView 服务包 + WebUI + Windows 托盘）。
- macOS / Linux 只要求 CI 编译成功（Linux no-GUI 因 tauri 核心 Linux 实现依赖 gtk3，CI 需 `libgtk-3-dev`；服务版托盘仅 Windows 启用），**不验证运行时行为**。
- 新增依赖只需评估：Windows 影响面 + Linux CI 可编译性。

## Repository Layout

- `src/` — React 19 + antd + Vite 前端；`npm run dev` 端口 1420（Tauri `devUrl`）；`npm run build` = `tsc && vite build` → `dist/`。
- `src-tauri/src/proxy/` — 网关核心（axum）：`server.rs`（HTTP 服务 + `/api` 管理路由 + `/api/events` SSE 事件桥 + `ABV_DIST_PATH` 静态托管）、`pipeline/`、`adapters/` + `mappers/`、`handlers/`、`thinking_store.rs`、`token_manager.rs`、`event_bus.rs`（双轨事件总线）。
- `src-tauri/src/commands/` — Tauri IPC 命令层（薄壳，供 Web `/api` 与命令注册复用）；`src-tauri/src/modules/` — 领域服务（accounts、oauth、db、scheduler、tray_headless、updater 等）。`modules/http_api.rs` 仅剩设置读写（19527 服务是已移除的死代码）。
- `release/` — 自包含服务运行包（`antigravity-tools.exe` + `dist/` + `start-webui.bat/.sh` + README）。二进制与 dist 被 gitignore，仅启动脚本入库。
- `.github/workflows/build-headless-manual.yml` — 手动触发的无 GUI CI 构建（4 平台，Windows 产物即 release 包）。
- `docker/` — GUI 镜像（`Dockerfile`）与无 GUI 服务镜像（`Dockerfile.service`，无 webkit/gtk 运行时依赖）；`web_site/` — 营销页，非应用 UI；`docs/` — 计划与发布文档。

## Architecture: Pipeline First

- 网关聚合四协议（OpenAI Responses / OpenAI Chat Completions / Anthropic Claude / Google Gemini）输出 Antigravity 风格 Gemini 协议。四协议仅作为 Gemini 适配器，管线保持协议无关；适配器只做参数归一化、载荷转换、协议分歧适配与管线无法解决的边缘情况。
- **Backend Fix Strategy**: 优先在管线内做协议无关的通用修复，适配器级补丁是最后手段。

## Dual-Transport Rules（前端双通道铁律）

- 前端传输无关：`src/utils/request.ts` 在 Tauri 内走 `invoke()`，浏览器内按 `COMMAND_MAPPING` 走 HTTP；`isTauri()` 检测在 `src/utils/env.ts`。**开发以浏览器（HTTP）通道为第一优先**。
- **每个功能必须双通道可用**：新增能力 = Tauri 命令 + `COMMAND_MAPPING` 条目 + `proxy/server.rs` 的 `/api` 路由，缺一则 Web 模式硬失败（`npm run check:parity` 在 CI 把关）。
- 事件推送走双轨：Rust 侧 `proxy/event_bus.rs`（总线必发）+ AppHandle 补发 Tauri 事件；前端 `src/utils/events.ts` 统一订阅（Tauri listen / Web fetch-SSE，断连降级轮询）。
- 优先新增 `/api` 端点而非前端直接调用 `@tauri-apps/*` 插件（`dialog`/`fs`/`opener`/`autostart`/`updater`/`process` 仅桌面可用）；`tauri-plugin-dialog` 已是 `gui`-only 可选依赖。
- `build.rs`：no-GUI 构建将 capabilities 校验指向空目录 `capabilities-no-gui/`（GUI 插件权限文件在该形态下不存在）。
- 凭据：`gui_config.json` 的 `proxy.admin_password`（Web UI 登录）/ `proxy.api_key`（AI 代理）。headless 启动时环境变量 `WEB_PASSWORD` / `ABV_API_KEY` 会持久化覆盖——这是修改密码的正规途径（配置文件中被掩码污染过的历史值不可信）。

## Build & CI

- **服务版（唯一产物）**：`cargo build --release --no-default-features`；无本机工具链时用 GitHub Actions（`build-headless-manual.yml`，workflow_dispatch 手动触发，产物为自包含 zip：二进制 + dist + 启动脚本）。
- `--headless` / `--serve` 运行服务（`--open` 自动开浏览器）；no-GUI 二进制无参数时默认服务模式；强制 `AllExceptHealth` 鉴权，`/health` 健康检查；Windows 下自动挂系统托盘。
- 桌面构建（`npm run tauri build`，`gui` feature）仅保留编译通路，**不再维护也不出产物**。
- **Pre-flight Checks**（Rust 改动）：`cargo fmt -- --check` + `cargo clippy --all-targets --all-features` + `cargo check --no-default-features`；前端改动跑 `npm run build`。CI 承担完整编译验证（本机工具链可能不齐，GitHub Actions 是可靠的编译验证环境）。
- `npm run check:parity`：Tauri 命令 ↔ Web 映射 ↔ `/api` 路由对齐检查（CI 必过）。

## Code Quality

- 优先根因、面向未来的修复，避免硬编码、死代码与投机性改动；优先覆盖一类问题的通用方案。
  - *模型路由*：优先通配（如 `gemini-*-flash-*`）而非精确串。
  - *Prompt 清洗*：优先正则模式匹配而非静态关键字替换。

## PR & Commit Discipline

- **单一问题域**：一个 PR 只解决一类问题，无关治理/文档拆独立 PR。
- **自包含且可独立回滚**：提交前收敛调试痕迹，每个提交描述最终状态与理由。
- **Risky Path Replacement**：替换已验证路径时保留显式回退（如"0 matches → full restart"）；副作用排序到不可逆点之前；先验证后删除；检测面放宽、作用面收窄；所有等待必须有超时。

## Inherited from Upstream（仅在同步上游时适用）

- 上游发布通道纪律（stable 仅 `main` / 预发布仅 `beta`、`npm run bump` 版本原子同步、CHANGELOG 与 README 更新日志同步、`docs/RELEASE_GUIDE.md`）——本 fork 不主动发版，仅在做上游同步或需要出正式包时参考。
- 上游的 Branch & History Hygiene / Contributor Respect 条款，作为协作礼仪基线保留。

Maintained by @YUE546 (fork of @lbjlaq/Antigravity-Manager)
