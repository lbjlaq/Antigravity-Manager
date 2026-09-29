# 去 WebView2 改造方案（双轨并行）

> 状态：**阶段 0-5 全部落地**（详见各阶段"实施记录"）
> 目标形态：**双轨并行** —— 保留现有 Tauri 桌面包（托盘 / 迷你窗口 / 自动更新，WebView2 引导仅存在于桌面包），新增一个**不链接 WebView2/wry/WebKitGTK 的无 GUI 服务版二进制** + 系统浏览器 WebUI。
> 关联维护准则：`AGENTS.md`（Headless & CLI Parity / Cross-Platform / PR 单一问题域）。

## 1. 背景与现状结论

本项目的 React 前端已经具备"双通道"能力，WebView2 只是当前桌面形态的宿主，而不是架构必需品：

| 能力 | 现状 | 关键代码 |
| --- | --- | --- |
| 双通道请求层 | 已就绪。Tauri 内走 `invoke()`，浏览器内按 `COMMAND_MAPPING`（190+ 命令）映射到 HTTP | `src/utils/request.ts`、`src/utils/env.ts` |
| Web UI 托管 | 已就绪。`--headless` 模式在 8045 单端口用 `ServeDir` 托管前端 dist + `/api` 管理面 | `src-tauri/src/lib.rs:267-416`、`src-tauri/src/proxy/server.rs:1043-1052` |
| Web 鉴权 | 已就绪。`AdminAuthGuard` 登录 + 401 自动登出（`abv-unauthorized` 事件） | `src/components/common/AdminAuthGuard.tsx`、`request.ts:262-270` |
| 事件推送 | **缺失**。6 类 Tauri 事件无 Web 推送，前端靠轮询兜底（监控 10s / 调试台 2s） | 见下文"事件清单" |
| 功能缺口 | `.vscdb` 导入、目录选择、自动安装更新在 Web 模式不可用；OAuth 事件推送降级为弹窗 + 手动授权码 | `AddAccountDialog.tsx`、`Settings.tsx`、`UpdateNotification.tsx` |
| 构建耦合 | **tauri 为非 optional 硬依赖**：二进制必链接 wry/WebKitGTK；Docker 运行时被迫安装 `libwebkit2gtk-4.1-0`/`libgtk-3-0`；NSIS 含 WebView2 引导 Section | `src-tauri/Cargo.toml:22`、`docker/Dockerfile*`、`src-tauri/installer.nsi:570-660` |

### 事件清单（Web 模式缺失面）

| 事件 | Rust 发射点 | 前端消费 | Web 模式现状 |
| --- | --- | --- | --- |
| `proxy://request` | `proxy/monitor.rs:407` | `ProxyMonitor.tsx`、`MiniView.tsx` | 10s 轮询兜底 |
| `log-event` | `modules/log_bridge.rs:60,215` | `useDebugConsole.ts` | 2s 轮询 `/api/debug/logs` |
| `accounts://refreshed` | `commands/mod.rs:301`、`log_bridge.rs:92` | `App.tsx:115` | 收不到 |
| `oauth-url-generated` / `oauth-callback-received` | `modules/oauth_server.rs:363,246,340` | `AddAccountDialog.tsx` | 弹窗 postMessage / 手动授权码 |
| `tray://account-switched` 等 | `modules/tray.rs:170,220` | `App.tsx:97,106` | 无托盘即无事件 |
| `config://updated` | `commands/mod.rs:427`、`tray.rs:108` | 显式刷新 | 收不到 |

### AppHandle 耦合热点（阶段 3 的工作量所在）

约 40 处触点，集中在：`commands/mod.rs`（18 处，多为 `SystemManager::Desktop(h)` 构造与 `.state::<ProxyServiceState>()`）、`modules/integration.rs`（`SystemManager` 枚举，已有 `HeadlessIntegration` trait 抽象）、`modules/log_bridge.rs`（全局 `OnceLock<AppHandle>`）、`modules/oauth_server.rs`、`proxy/monitor.rs`（`Option<AppHandle>`）、`modules/tray.rs` / `lightweight.rs`（纯桌面）。业务核心（`proxy/*`、`token_manager`、account/quota）已接近零 Tauri 依赖。

## 2. 已知取舍（无 GUI 形态不提供等价物的能力）

- **系统托盘**：无等价物。开机自启 → 系统服务 / 计划任务；切号 → WebUI / CLI。
- **迷你窗口 / 窗口管理**：无等价物（`windowManager.ts` 已做 `isTauri()` 静默降级）。
- **自动安装更新**：`updater.json` 自动更新通道**只覆盖 GUI 包**；无 GUI 版走镜像 tag / Release 手动下载 / 包管理器。
- **本地路径类功能**（数据目录迁移、`.vscdb` 本地路径导入、`patch_agy_binary`、Homebrew）：浏览器拿不到本地路径，Web 模式永久降级（阶段 2 提供上传 / 手输路径等替代）。

## 3. 阶段设计

### 阶段 0 —— Web 对齐度防回归基线（✅ 已落地）

- **`scripts/check-web-parity.mjs`**：解析 `lib.rs` 的 `invoke_handler` 注册名单、`src/**` 中前端实际 `request(...)`/`invoke(...)` 调用点、`request.ts` 的 `COMMAND_MAPPING`、`server.rs` 的 `/api` 路由，输出缺失矩阵。失败条件：
  1. 前端实际调用、但既无 `COMMAND_MAPPING` 又不在桌面专属白名单的命令；
  2. `COMMAND_MAPPING` 指向的 URL 在 `server.rs` 中不存在对应路由。
  白名单（桌面专属 / 暂缓项，均在脚本内注释理由）：`greet`、`show_main_window`、`set_window_theme`、`save_text_file`、`read_text_file`、`check_appimage_installation`、`check_native_update`、`check_homebrew_installation`、`brew_upgrade_cask`、`patch_agy_binary`、`migrate_data_dir`、`set_data_dir`、`get_data_dir_path`、`cloudflared_check`、`get_antigravity_cli_path`（本地可执行文件探测）、`query_transit_info`（任意 URL CORS 中继，Web 暴露前需评审 SSRF 面，移入阶段 2）。
- **脚本首次运行即发现并修复的真实缺口**：
  - `update_account_label` 有 Web 映射但后端缺路由 → 标签更新逻辑从 Tauri 命令下沉为 `modules::account::update_account_label`（与 `update_account_priority` 同款写锁模式），新增 `POST /api/accounts/:accountId/label`，Tauri 命令与 admin 路由共用同一实现；
  - Droid 同步四个命令未映射（而 opencode/hermes/openclaw 均已映射）→ `request.ts` 补 `get_droid_sync_status` / `execute_droid_sync` / `execute_droid_restore` / `get_droid_config_content` → `/api/proxy/droid/*`（后端路由已存在，请求体 serde 均为 camelCase，直接可用）。
- **Web 模式 bug 修复**：`Layout.tsx` 顶部拖拽区 `startDragging()` 加 `isTauri()` 守卫；`MiniView.tsx` 去掉硬编码版本号（Web 模式隐藏版本角标）。
- **CI 接入**：`npm run check:parity` 加入 `ci.yml` 的 `build-frontend` job。
- 验收：脚本对当前代码库全绿；此后新增 Tauri 命令而无 Web 映射会在 CI 直接失败。
- 回滚：删脚本与 CI 步骤即可，零运行时影响。

### 阶段 1 —— 事件桥（SSE `/api/events`）✅ 已落地

**实施记录**：
- 后端：新增 `src-tauri/src/proxy/event_bus.rs`（`tokio::sync::broadcast`，容量 1024，`bus://lagged` 丢帧提示）；`/api/events` SSE 端点挂在 admin 路由组（`admin_auth_middleware` 鉴权），`async_stream` 生成 + `KeepAlive`。
- 8 处 emit 点全部双轨化（总线必发 + 有 AppHandle 时补发 Tauri 事件）：`log_bridge.rs`（log-event 缓冲回放/实时、accounts://refreshed）、`commands/mod.rs`（accounts://refreshed、config://updated）、`tray.rs`（config://updated）、`oauth_server.rs`（oauth-url-generated、oauth-callback-received ×2）、`monitor.rs`（proxy://request）。
- 前端：`src/utils/events.ts` 统一订阅（Tauri → `listen`；Web → fetch 流式读 SSE，携带 `Authorization` 头避免 query token 泄漏；指数退避重连，连续 5 次失败进入 dead 终态）；接入 `ProxyMonitor`（SSE 优先、dead 降级 10s 轮询）、`MiniView`、`useDebugConsole`（同款降级）、`AddAccountDialog`（Web 模式可获得 OAuth 实时回调推送）、`App.tsx`（accounts://refreshed 双轨；`app://trigger-update` 统一为 window CustomEvent；tray 事件保持桌面专属）。
- 验收：Web 模式监控页/调试台/OAuth 实时性与桌面一致；断连自动重连 + 轮询兜底。

### 阶段 2 —— 功能缺口收口 ✅ 已落地

**实施记录**：
- `.vscdb` 导入：`POST /api/accounts/import/db-custom-upload`（axum multipart，落临时文件后复用 `migration::import_from_custom_db_path`，成败均清理临时文件）；前端 `uploadRequest` 助手 + store `importFromCustomDbUpload` action，Web 模式文件选择器可用。
- 路径类设置：`POST /api/system/validate-path`（返回 exists/is_dir）；Settings 6 个本地路径选择 handler 在 Web 模式改为手输路径 + 校验（`promptPathWeb`）。
- transit 中转查询：实现收敛为 `server.rs::relay_get_with_bearer`，桌面命令 `query_transit_info` 与 `POST /api/transit/query` 共用；parity 白名单相应移除。
- 自动更新：Web 模式维持"检查 + 手动下载"（设计取舍，见第 2 节）。

### 阶段 3 —— 业务层去 AppHandle + tauri 转 optional（结构改造核心）✅ 已落地

**实施记录**（实现方式与原稿略有出入，以本记录为准）：
- **双 runtime 隐患修复**：新增 `src-tauri/src/utils/spawn.rs::spawn_task`（`tokio::runtime::Handle::try_current()` 探测——headless 手建 runtime 内直接 `tokio::spawn`，GUI 主线程等无 ambient runtime 场景回退 `tauri::async_runtime::spawn`）；scheduler 已接入。
- **tauri 转 optional 的实现方式**：tauri v2 的 `wry`（webview 绑定）本身是可选 feature，因此**无需 cfg 门控命令层**——`tauri = { default-features = false, features = ["compression", "dynamic-acl", "test"] }` 保住核心（AppHandle/命令宏/插件生态），`gui` feature 才引入 wry/tray-icon：
  - `[features] default = ["gui", "custom-protocol"]`；`gui = ["tauri/wry", "tauri/tray-icon", "tauri/image-png", "tauri/common-controls-v6", "tauri/x11"]`；
  - 跨模式代码中的裸 `tauri::AppHandle`/`tauri::Window` 机械替换为 `crate::AppHandle`/`crate::Window` 别名（gui=Wry；no-GUI=`tauri::test::MockRuntime`，仅保类型存在、运行期永不构造）；
  - `lib.rs` 拆分三段：`run()`（参数解析 + 公共初始化）→ `run_headless()`（单 tokio runtime 服务模式）+ `run_gui()`（`#[cfg(feature = "gui")]`，Builder/托盘/轻量模式）；`modules::tray` / `modules::lightweight` 模块声明与 `commands/mod.rs`、`integration.rs` 内 11 处托盘调用点加 `#[cfg(feature = "gui")]` 门控；
  - 无 GUI 构建不再需要 dist 占位 hack（`generate_context!` 随 gui feature 关闭）。
- 双 feature 编译验证：`cargo check --no-default-features` 与 `cargo check`（GUI）双双通过；`SystemManager::Desktop(AppHandle)` 变体保留（tauri 核心仍在依赖树中，无需移除；运行期 no-GUI 路径只使用 `Headless` 变体）。

### 阶段 4 —— 无 GUI 分发形态 ✅ 已落地

**实施记录**：
- 运行方式：`--headless`（已有）；新增 `--serve` 别名与 `--open`（服务启动成功后用系统默认浏览器打开 Web UI，`open_in_system_browser` 跨平台实现）；no-GUI 构建下未传参数时**默认按服务模式运行**。
- 构建：`cargo build --release --no-default-features`（不链接 WebView2/wry/WebKitGTK）。
- Docker：新增 `docker/Dockerfile.service` + `docker/docker-compose.service.yml`（构建期剔除 GTK/WebKitGTK 头文件、运行期剔除 gtk3/webkit2gtk/appindicator/librsvg，boring-sys2 前置 cmake/go/clang/git 保留）；ENTRYPOINT/healthcheck/`ABV_DIST_PATH`/`ABV_*` 约定不变。
- CI：`release.yml` 新增 `build-headless` job（windows-2025 / ubuntu-22.04 / ubuntu-24.04-arm / macos-latest 矩阵，产物 `antigravity-manager_<ver>_<arch>-headless.zip`，随 Release 发布、**不进 updater.json**）；`ci.yml` check-rust 新增 `cargo check --no-default-features` 步。
- 安装面：`install.ps1` 增 `$Headless` 参数（下载 zip 解压至 `%LOCALAPPDATA%\AntigravityTools-Service`）；`install.sh` 增 `HEADLESS=1`（解压至 `~/.local/bin`）；GUI 包的 NSIS WebView2 引导**原样保留**；`deploy/arch/PKGBUILD.headless.template` 为无 webkit 依赖变体。
- Windows 服务化：以计划任务 / NSSM 文档化方案起步（后续按需引入 `windows-service` crate）。

### 阶段 5 —— 收尾 ✅ 已落地

- `modules/http_api.rs`：19527 端口死代码（`spawn_server`/`start_server`/`ApiState` 等，无调用方）已剔除，保留 `HttpApiSettings` 读写（`/api/system/http-api/settings` 与同名 Tauri 命令共用）。
- 根目录过期 `workflows/` 副本（与 `.github/workflows/` 重复且逻辑过期）已删除。
- 待维护者决策：Windows 默认下载是否切换为无 WebView 服务版，Tauri GUI 改称"桌面增强版"。

## 4. 明确不做 / 备选方案记录

- **替换 WebView 引擎**（Windows 上即 CEF 嵌入）：二进制体积 + 数十 MB、维护成本高，且收益不如"服务版 + 浏览器"形态，**不做**。
- **原生 UI 重写**（WinUI/egui）：整个控制台 UI 重写，成本远超收益，**不做**。
- Tauri 官方不支持在 Windows 上换用非 WebView2 引擎，故"去 WebView2"的唯一务实路径就是上述"无 GUI 服务版 + 浏览器 WebUI"双轨。
