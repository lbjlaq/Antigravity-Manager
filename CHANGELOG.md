# Changelog / 更新日志

> This changelog exclusively tracks **Antigravity Manager** (`antigravity-manager` branch, `am-core` + `am-cli` + `am-gui` workspace).  
> 本更新日志仅记录 **Antigravity Manager**（`antigravity-manager` 分支，`am-core` + `am-cli` + `am-gui` 架构）的版本演进。

---

## v0.0.2 (2026-11-26)

### English

- **Cross-Platform Active Host Account Detection (`macOS` · `Windows` · `Linux`)**: Fixed an issue where **Antigravity Desktop App (≥ 2.0.0)** showed as unbound (`not_configured` / `Not Linked`) after switching **Antigravity IDE** or signing in natively. Now combines OS Keyring (`macOS Keychain` / `Windows Credential Manager` / `Linux Secret Service`), `~/.gemini/jetski-standalone-oauth-token`, host-exclusive `app_storage.json`, SQLite `state.vscdb` (`authStateWithContextSentinelKey` + `id_token` JWT email fallback), and custom Settings paths while keeping App / IDE / ACP / CLI isolated.
- **ACP Plugin & Native `agy` CLI Credential Parity**: Unified cross-platform Keyring fallback (`gemini:antigravity-acp`) for ACP Plugin and Keyring / `oauth_creds.json` detection for native `agy` CLI installs without an exclusive `antigravity-oauth-token` file.
- **Windows & Linux Host Discovery Hardening**: Added Windows `ProgramFiles` / `ProgramFiles(x86)` install discovery, `%APPDATA%` / `%LOCALAPPDATA%` fallbacks, Linux case-sensitive folder variants (`antigravity-ide` / `antigravity_ide` / `antigravity`), and fixed Linux static version resolution when target roots are directories.
- **CLI & Dashboard Status Polish**: Added an `Active Account` column to `am current` for CLI/headless parity, and replaced the raw `'not_configured'` fallback string on the Dashboard switcher card with localized unbound labels.
- **Single-Row Multilingual Accounts Toolbar (Apple HIG Redesign)**: Eliminated toolbar line-wrapping across all 12 UI languages and narrow (`900px`) windows by unifying controls to a `28px` baseline, adding a focus-expanding search field with inline clear, converting tag/sort/refresh controls into borderless icon buttons with tooltips and hairline dividers, consolidating `Import` / `Export` into a `⋯` overflow menu, and keeping `+ Add Account` as the sole filled primary CTA (also fixing icon-text spacing).

### 中文

- **三端跨平台当前账号检测修复（`macOS` · `Windows` · `Linux`）**：修复切换 **Antigravity IDE** 或原生登录后 **Antigravity 桌面端（≥ 2.0.0）** 显示 `not_configured` / `未绑定账号` 的问题；统一整合三端系统凭据库（macOS Keychain / Windows 凭据管理器 / Linux Secret Service）、`~/.gemini/jetski-standalone-oauth-token`、宿主专属 `app_storage.json`、`state.vscdb`（含 `authStateWithContextSentinelKey` 与 `id_token` JWT 邮箱回退）以及设置页自定义路径，同时保持 App / IDE / ACP / CLI 四端隔离不串号。
- **ACP 插件与原生 `agy` CLI 凭据读取补齐**：为 ACP 插件补充跨平台系统凭据库（`gemini:antigravity-acp`）回退，支持原生未生成独立 `antigravity-oauth-token` 文件的 `agy` CLI 自动读取系统凭据库与 `oauth_creds.json` / `google_accounts.json`。
- **Windows / Linux 宿主路径与版本探测加固**：补齐 Windows `ProgramFiles` / `ProgramFiles(x86)` 全局安装目录与 `%APPDATA%` / `%LOCALAPPDATA%` 探测、Linux 大小写目录变体（`antigravity-ide` / `antigravity_ide` / `antigravity`），并修复 Linux 下目录型安装路径无法静态读取版本号的问题。
- **CLI 与仪表盘展示优化**：`am current` 新增 `Active Account` 列实现终端诊断对齐，仪表盘切号卡片未绑定时改为国际化文案显示。
- **账号页多语言单行工具栏重构（Apple HIG 风格）**：彻底解决 12 种语言及 `900px` 窄窗下的子工具栏换行问题；统一所有控件为 `28px` 基线高度，搜索框支持聚焦微扩与一键清空，标签 / 排序 / 刷新改为无边框图标按钮配悬停提示与细分割线，将导入 / 导出收纳进 `⋯` 更多菜单，仅保留蓝底 `+ 授权登录` 作为唯一主 CTA 并修复图标与文字间距。

---

## v0.0.1 (2026-11-25)

### English

- **Modular Rust Workspace (`am-core` + `am-cli` + `am-gui`)**: Decoupled core domain engine from desktop GUI (Tauri v2 + React 18) and standalone cross-platform CLI (`am` / `am.exe`) sharing one SQLite store.
- **Zero-Proxy 4-Host Account Switching**: Direct SQLite & system Keyring (macOS Keychain / Windows Credential Manager / Linux Secret Service) credential injection for **Antigravity Desktop App**, **Antigravity IDE**, **ACP Plugin (VS Code / Cursor)**, and **Antigravity CLI (`agy`)**, with per-account hardware fingerprint isolation.
- **Dual-Pool `5h` / `7d` Quota Monitor & Smart-Pick Engine**: Real-time Gemini & Claude quota tracking with `Max Quota`, `Expiring Soon`, `Tier First`, and `Balanced LRU` smart-pick strategies.
- **Zero-MITM Local Telemetry Studio & Share Posters**: Purely local read-only SQLite analytics for token throughput, cache hit rate, TTFT/TPS, workspace leaderboards, and one-click `1280×720` (`16:9`) HD shareable posters in 4 themes.
- **Menu Bar / System Tray Mini Popover & Lightweight Mode**: Dual-mode tray popover (`Quick Switch` / `Today Stats`), quiet login autostart, and WebView renderer release in background lightweight mode.
- **Cross-Platform CLI Packaging & Setup (`macOS` · `Windows` · `Linux`)**: Bundled `am` / `am.exe` sidecar inside desktop installers via Tauri `externalBin`, with OS-aware global `am` registration commands in Settings.
- **12-Language UI Localization**: On-demand i18n packs (`zh`, `zh-TW`, `en`, `ja`, `ko`, `ru`, `es`, `pt`, `tr`, `vi`, `ar`, `my`).

### 中文

- **模块化 Rust 工作空间（`am-core` + `am-cli` + `am-gui`）**：核心领域引擎与 Tauri v2 + React 18 桌面端、独立跨平台命令行（`am` / `am.exe`）彻底解耦，共享同一本地 SQLite 凭据库。
- **零反代四宿主无感热切**：原生直写 SQLite 与三端系统凭据库（macOS Keychain / Windows 凭据管理器 / Linux Secret Service），覆盖 **Antigravity 原生桌面端**、**Antigravity IDE**、**ACP 插件（VS Code / Cursor）** 与 **Antigravity CLI（`agy`）**，单号独立硬件指纹隔离。
- **双池 `5h` / `7d` 配额监控与智能选号**：实时监控 Gemini / Claude 双模型池余量与重置窗口，内置「余量优先 / 临近重置 / 等级优先 / 均衡轮换」四种智能选号策略。
- **零劫持本地遥测大盘与战报海报**：纯本地只读解析 SQLite 流水，呈现 Token 吞吐、缓存命中率、TTFT / TPS 速率与工程排行，支持 4 套暗黑主题一键生成 `1280×720`（`16:9`）高清战报海报。
- **菜单栏 / 系统托盘双模 Mini 弹窗与轻量模式**：支持「账号切号 / 今日数据」双模秒切、免打扰开机自启以及后台释放 WebView 渲染进程的轻量模式。
- **全平台 CLI Sidecar 内嵌打包（`macOS` · `Windows` · `Linux`）**：通过 Tauri `externalBin` 将 `am` / `am.exe` 随各端安装包一并分发，设置页按当前系统动态展示对应注册命令。
- **12 国语言按需国际化**：内置 `zh`、`zh-TW`、`en`、`ja`、`ko`、`ru`、`es`、`pt`、`tr`、`vi`、`ar`、`my` 按需加载语言包。
