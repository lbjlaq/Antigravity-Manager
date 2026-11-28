# Changelog / 更新日志

> This changelog exclusively tracks **Antigravity Manager** (`antigravity-manager` branch, `am-core` + `am-cli` + `am-gui` workspace).  
> 本更新日志仅记录 **Antigravity Manager**（`antigravity-manager` 分支，`am-core` + `am-cli` + `am-gui` 架构）的版本演进。

---

## v0.0.5 (2026-11-28)

### English

- **Fixed Infinite Console Window Popups on Windows Toolbox Tab (Fixes #4118)**:
  - Replaced the 5-second Windows PowerShell `Get-Process` polling in `am-proxy::get_app_runtime_status()` with native two-phase `sysinfo` process-table inspection (`NtQuerySystemInformation` name scan + targeted `cmd`/`environ` lookup), eliminating repeated `conhost.exe` / `WindowsTerminal.exe` flashes, cutting polling overhead to sub-millisecond latency, and enabling live Windows `proxy_active` / `proxy_url` detection.
  - Enforced `CREATE_NO_WINDOW` (`0x0800_0000`) across all remaining Windows auxiliary subprocesses (`reg query`, graceful-exit `powershell`, `cmd /C` batch shims, and `am-core::runtime::spawn_and_reap` toast notifications), plus `windowsHide: true` on `am-monitor` plugin `execFile` calls.
  - Added an in-flight guard and `document.hidden` pause/resume (`visibilitychange`) to the Toolbox status poll, and moved `proxy_runtime_status`, `proxy_launch_app`, and desktop shortcut creation off Tokio worker threads via `spawn_blocking`.
- **Full Windows & Linux Toolbox Parity (`No-TUN Proxy`, Desktop Shortcuts & Custom Install Paths)**:
  - Implemented cross-platform desktop shortcuts (`create_desktop_shortcut` / `remove_desktop_shortcut` / `is_desktop_shortcut_installed`): silent zero-console Windows `.lnk` + `wscript.exe` `.vbs` launchers with native EXE icons (supporting OneDrive-redirected Desktop via `dirs::desktop_dir()`) and executable Linux `.desktop` entries alongside macOS `.app` wrappers.
  - Added Windows listening proxy process auto-detection (`netstat -ano -p tcp` with `CREATE_NO_WINDOW` + `sysinfo` PID lookup) for custom-port Clash / Mihomo / v2rayN / FlClash / sing-box clients, and hardened `reg query` parsing for `ProxyEnable` (`0x`/`0X`) and `ProxyServer` values containing spaces or prefix collisions.
  - Expanded Windows binary discovery (`%ProgramFiles%`, `%ProgramFiles(x86)%`, `PATH`, plus live running-process `exe()` fallback for custom drives such as `D:\`), excluded `antigravity-ide` / `antigravity-manager` / `am-gui` from Linux classic-app matching, prioritized `USERPROFILE` for Windows plugin directories, and added Windows `.exe` discovery + case-insensitive matching to `cli_statusline`.
- **AM Monitor Plugin Efficiency & Cross-Platform Wake**:
  - Deduplicated double `am` CLI polling in the Antigravity IDE extension with a 15-second TTL cache (invalidated immediately on account switch), expanded cross-platform `am` binary lookup paths, and added Windows/Linux support for waking the Antigravity Manager desktop app.

### 中文

- **彻底修复 Windows 工具箱标签页无限弹出命令行窗口（Fixes #4118）**：
  - 将 `am-proxy::get_app_runtime_status()` 在 Windows 下每 5 秒拉起 `powershell` `Get-Process` 的探测实现替换为原生两阶段 `sysinfo` 进程表扫描（`NtQuerySystemInformation` 轻量名匹配 + 按需读取命中进程的 `cmd`/`environ`），彻底消除 `conhost.exe` / `WindowsTerminal.exe` 反复闪烁弹窗，同时将探测耗时降至毫秒级并补齐 Windows 免 TUN 代理注入态识别。
  - 为 Windows 下其余所有后台辅助子进程统一补齐 `CREATE_NO_WINDOW (0x0800_0000)`（涵盖 `reg query`、优雅退出 `powershell`、`cmd /C` 启动器及 `am-core::runtime::spawn_and_reap` 桌面通知），并为 `am-monitor` 插件的所有 `execFile` 调用加上 `windowsHide: true`。
  - 为工具箱前端 5 秒状态轮询增加防重入锁与后台静默（`document.hidden` 暂停、切回前景 `visibilitychange` 立即刷新），并将 `proxy_runtime_status` / `proxy_launch_app` / 桌面快捷方式创建迁入 `tokio::task::spawn_blocking` 避免阻塞异步工作线程。
- **补齐 Windows / Linux 工具箱全能力对等（免 TUN 代理探测、无黑窗桌面图标与自定义盘符查找）**：
  - 新增跨平台桌面快捷启动器（`create_desktop_shortcut` / `remove_desktop_shortcut` / `is_desktop_shortcut_installed`）：在 macOS `.app` 基础上补齐 Windows 无黑窗 `.lnk` + `wscript.exe` `.vbs` 快捷方式（自动复用原版 EXE 图标，支持 OneDrive 重定向桌面路径）以及 Linux 可执行 `.desktop` 入口，修复点击「桌面图标」直接报错的问题。
  - 补齐 Windows 代理进程监听端口自动扫描（`netstat -ano -p tcp` + `CREATE_NO_WINDOW` + `sysinfo` 进程名映射），自动发现非默认端口的 Clash / Mihomo / v2rayN / FlClash / sing-box 客户端；重构 `reg query` 3 列严格解析器，修复含空格 `ProxyServer` 截断与相似键名误匹配问题。
  - 增强 Windows 目标程序定位（覆盖 `%ProgramFiles%`、`%ProgramFiles(x86)%`、`PATH` 及自定义盘符运行态进程反查），修复 Linux 下经典版进程名误匹配 IDE/管理器、Windows `USERPROFILE` 插件目录优先级以及 `cli_statusline` 的 `.exe` 路径与大小写兼容。
- **AM Monitor 插件减负与跨平台唤醒主程序**：
  - 合并 Antigravity IDE 扩展每次状态栏刷新重复调用 `am` CLI 的问题并增加 15 秒 TTL 缓存（切号即刻失效），扩展三端 `am` 可执行路径查找，并补齐 Windows / Linux 一键唤醒 Antigravity Manager 主程序支持。

---

## v0.0.4 (2026-11-28)

### English

- **No-TUN Transparent Proxy Acceleration (`am-proxy` Crate + Update-Proof Client Launchers)**:
  - Added a dedicated `am-proxy` crate that auto-detects local & system proxy endpoints with live TCP latency probing (reusing existing local proxy clients without TUN mode or patching original `.app` bundles), supports custom proxy URLs, and includes a fixed-port hot-switchable local relay (`RelayServer`, default `17890`).
  - Added one-click No-TUN launch and desktop shortcut wrappers for **Antigravity Desktop App** and **Antigravity IDE** (automatically syncing user proxy & loopback bypass settings so native Dock launches also work, with one-click restore), plus a managed Shell Hook for **`agy` CLI** with instant rollback.
  - Enabled automatic OS system proxy inheritance in `am-core`'s `shared_http_client()` (with `localhost` / `127.0.0.1` / `::1` bypass) so quota sync and update checks work seamlessly without TUN mode.
- **Native Zero-Patch Context Monitor Plugins Across All 3 Hosts (`AM Monitor`)**:
  - **Antigravity Desktop (`Sidecar UI`)**: Built a native right-hand Aux Pane plugin (`~/.gemini/config/plugins/am-monitor`, powered by `sidecar_sdk`) featuring live context window watermarks (`Current Window` post-`/compact` vs. `Full Session`), dual-ring cache hit & context gauge, TPS speed, 10-cell tool call matrix, model breakdown, denoised Top 4 heaviest steps & error diagnostics, 26-week heatmap, HD share report PNG export, in-panel account hot-switching, and smart auto-follow language & dark/light theme.
  - **Antigravity IDE Extension (`~/.antigravity-ide/extensions/am-monitor`)**: Added a native bottom status bar pill with customizable live metrics (context %, cache hit %, TPS, cost, turns, active account, G/C quota), rich hover summary, and one-click full Bento monitor panel.
  - **Antigravity CLI (`agy` Dual-Line Statusline)**: Added a zero-network, millisecond-fast dual-line ANSI statusline (`am statusline`) hooked into `~/.gemini/antigravity-cli/settings.json` displaying live context usage, active account quota, and proxy status — automatically backing up and losslessly restoring any pre-existing `statusLine` config on uninstall.
- **Apple HIG Toolbox Console & Full 12-Language Support**:
  - Added a new **Toolbox** tab (`NavMenu` + `Toolbox.tsx`) styled to match Dashboard, Stats, and Settings: top Apple segmented filter (`All Tools` / `No-TUN Proxy` / `Monitor Plugins`), 4-card status KPI strip, asymmetric `7 + 5` Bento grid, live running/proxy badges, and a post-install quick-entry guide.
  - Added full translations (69 keys) across all 12 locale packs (`zh`, `en`, `zh-TW`, `ja`, `ko`, `es`, `pt`, `ru`, `tr`, `vi`, `ar`, `my`).
- **Expanded `am` CLI Subcommands for Full Headless Parity**:
  - Added `am proxy` (`detect`, `relay`, `launch`, `hook`), `am session-context [--session] [--json]`, `am statusline`, and `am plugin` (`install`, `uninstall`, `status`).

### 中文

- **免 TUN 透明代理加速引擎（新增 `am-proxy` Crate + 零侵入抗更新启动器）**：
  - 新增独立 `am-proxy` 模块，自动探测本机与系统代理出口及实时 TCP 延迟（直接复用 Shadowrocket / Clash / Surge / v2ray 等本机代理软件，无需开启 TUN 模式，也绝不修改原版 `.app` 包），支持自定义代理地址与固定端口热切换本地中继（`RelayServer`，默认 `17890`）。
  - 支持对 **Antigravity 桌面版** 与 **Antigravity IDE** 一键免 TUN 启动及创建带原版图标的桌面快捷启动器（IDE 自动同步用户级代理与本地回环保护，直接点 Dock 原版图标同样生效，支持一键恢复默认），并为 **`agy` CLI** 提供托管式 Shell Hook 无感代理与一键还原。
  - 在 `am-core` 全局 `shared_http_client()` 中自动继承操作系统代理设置（内置 `localhost` / `127.0.0.1` / `::1` 回环直连保护），不开 TUN 即可稳定刷新配额与检查更新。
- **三宿主原生零侵入上下文监控插件（`AM Monitor` · 抗官方更新）**：
  - **Antigravity 桌面端（`Sidecar UI`）**：基于官方 `sidecar_sdk` 构建右侧原生辅助面板插件（安装至 `~/.gemini/config/plugins/am-monitor`），支持实时上下文水位与 `/compact` 压缩窗口识别（当前窗口 / 全会话切换）、双环缓存命中率与 ROI 回本倍率、TPS 生成速率、10 格工具调用矩阵、模型排行、降噪后的 Top 4 最重步骤与异常排查、近 26 周活跃热力图、高清 PNG 战报海报导出、面板内多账号热切换以及中英文与深浅色智能跟随。
  - **Antigravity IDE 扩展（`~/.antigravity-ide/extensions/am-monitor`）**：提供底部状态栏实时胶囊（可自定义勾选上下文 %、缓存命中 %、吐字速度、会话费用、轮次、当前账号与 G/C 额度）、富文本悬浮摘要与单击直达完整监控面板。
  - **Antigravity CLI（`agy` 双行实时状态栏）**：新增零网络请求、毫秒级响应的双行 ANSI 终端状态栏（`am statusline`），写入 `~/.gemini/antigravity-cli/settings.json` 实时展示 CTX 水位、当前账号额度与代理端口，安装前自动备份原有配置、卸载时无损还原。
- **全新 Apple HIG「工具箱」控制台与 12 国语言全量适配**：
  - 顶栏新增 **工具箱**（`Toolbox`）独立模块，与仪表盘 / 统计 / 设置保持 1:1 视觉统一：顶部 Apple 分段控制器（`全部工具 / 免 TUN 代理 / 监控插件`）、4 列状态 KPI 矩阵、`7 + 5` 非对称 Bento 双栏布局、运行态实时徽章及「安装后入口速览」引导卡。
  - 全部 12 份多语言包（`zh` / `en` / `zh-TW` / `ja` / `ko` / `es` / `pt` / `ru` / `tr` / `vi` / `ar` / `my`）同步补齐 69 条工具箱与导航词条。
- **`am` CLI 命令行能力全面对等扩展**：
  - 新增 `am proxy`（`detect` / `relay` / `launch` / `hook`）、`am session-context [--session] [--json]`、`am statusline` 与 `am plugin`（`install` / `uninstall` / `status`）子命令，完整覆盖终端与无头工作流。

---

## v0.0.3 (2026-11-26)

### English

- **Proactive Auto-Update & Two-Phase In-App Hot Update (`Rust UpdateController` + Apple HIG UX)**:
  - Added a background update scheduler in Rust (quietly checks 3s after launch and periodically every 24h while resident in tray, configurable via `config.json` `update.auto_check`, Settings toggle, or `am update --auto-check on|off`) with system proxy pass-through (`HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY`) and persistent version skipping (`skipped_version`).
  - Replaced single-shot destructive updates with a two-phase verified flow (`download()` with signature check → `Ready` → `install()` + safe tray-aware restart) hosted in a process-level Rust state machine (`updater://state`), surviving lightweight-mode WebView destruction and keeping the top Navbar update pill, translucent `UpdatePopover`, and Settings About panel synchronized.
  - Added install-channel routing for macOS **Homebrew Cask** (`brew upgrade --cask antigravity-manager` with native fallback), **Linux non-AppImage** (DEB/RPM graceful manual download to avoid `ENOEXEC`), and updated `Casks/antigravity-manager.rb` + `scripts/bump-version.mjs` to track explicit SemVer versions (`0.0.3`) instead of `:latest`.
  - Upgraded `am update` CLI with `--json`, `--install`, and `--auto-check on|off` for full headless/terminal parity.
- **Structured Bilingual Release Notes Renderer (`ReleaseNotesView`)**: Replaced raw Markdown `<pre>` dumps in both `UpdatePopover` and Settings About with a zero-dependency native renderer that auto-strips GitHub Release edition banners, matches the active UI language (`中文` / `English`) with a one-click `中文 | EN` segmented toggle, and formats nested bullets, bold titles, inline code pills, and external links cleanly.
- **1:1 HD Share Poster PNG Export Parity (`Polar` · `Radial` · `Hex Core` · `Matrix`)**: Rebuilt the `1280×720` Canvas PNG exporter so copied and downloaded posters match their live modal previews without losing data. Restored top time-slot / session-slice column headers (`00:00`, `01:15`, … / `00h`, `04h`, …) and the 2-column layout in **Matrix**, 6-axis radial spokes and labels (`TOK` / `HIT` / `CTX` / `TPS` / `OUT` / `REQ`) in **Polar**, `CACHE HIT` KPI + inner ring + colored model bars in **Radial**, 2-column honeycomb lattice strokes + token progress bars in **Hex Core**, template-specific gradient backgrounds/badges/footers, and cross-WebView `roundRect` fallback.
- **Smart Model Label Formatting & Variant Disambiguation**: Rewrote matrix axis shortening (`G4 Hydro·H`, `G4 Neon·XH`, `G4 Argon·H`) to eliminate broken parentheses and inconsistent casing, and preserved effort/variant suffixes (`(High)` / `(Med)` / `(XHigh)`) when truncating long model names in narrow telemetry lists and exported posters.
- **Unified Apple HIG Controls, Equal-Height Cards & Dark Mode Polish**:
  - Unified segmented active pills (`Dashboard` Reserve Accounts, `Token Stats` Ledger / Models / Projects Perf) and primary brand-blue actions with crisp dark-mode ring elevation (`dark:ring-white/[0.08]`), widened the Ledger search field (`w-56`) to prevent placeholder clipping, and fixed Hex Core orphan line-wrapping.
  - Balanced single-window (`5h` or `7d`) vs. dual-window (`5h` + `7d`) cards in the Accounts Grid via symmetrical equal-height quota containers, and fixed the host-switching spinner contrast on blue active buttons.
  - Added right-edge fade masks (`.fade-mask-right`) for horizontally scrollable model pill strips, soft bottom fade masks (`.fade-mask-bottom-soft`) for Mini Popover lists, sleek custom scrollbars (`.app-scroll` / `.mini-scroll`) across main views and modals, and informative hover tooltips on Dashboard Equalizer bars.

### 中文

- **主动更新发现与两阶段应用内热更新（Rust 统一状态机 + Apple HIG 交互）**：
  - 在 Rust 后端新增静默更新调度器（启动延迟 3 秒检查、托盘常驻期间按 24 小时冷却周期复检，支持 `config.json`、设置页开关及 `am update --auto-check on|off` 控制），并自动透传系统代理（`HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY`）与支持持久化「跳过此版本」。
  - 将应用内更新升级为进程级两阶段状态机（`download()` 先下载并校验签名 → 就绪后由用户决定「立即重启 / 稍后」再 `install()` 落盘替换并安全重启），支持轻量模式关窗重建 WebView 不丢进度，并打通顶栏克制小胶囊、右上角毛玻璃 `UpdatePopover` 卡片与「设置 → 数据存储与关于」的实时同步，同时修正 12 国语言按钮文案并透出真实错误详情。
  - 补齐跨平台安装渠道分流：macOS **Homebrew Cask** 优先走 `brew upgrade --cask antigravity-manager`（支持切回原生更新），**Linux 非 AppImage（DEB/RPM）** 自动降级手动下载避免 `ENOEXEC`；将 `Casks/antigravity-manager.rb` 从 `:latest` 修正为显式版本号（`0.0.3`）并纳入 `scripts/bump-version.mjs` 原子同步。
  - 增强终端 `am update` 子命令，支持 `--json`、`--install` 与 `--auto-check on|off`，实现 CLI / 无头环境与 GUI 能力对等。
- **结构化双语更新日志视图（`ReleaseNotesView`）**：告别原始 Markdown `<pre>` 裸文本与中英混排刷屏——自动剥离 GitHub Release 降级头部的安装指引、按当前界面语言默认展示中文或英文并提供 `中文 | EN` 一键胶囊切换，零依赖原生渲染分层小标题、多级列表、等宽代码胶囊与外部链接。
- **分享海报 1:1 高清 PNG 导出保真修复（`极坐标` · `放射光谱` · `蜂巢热核` · `暗夜矩阵`）**：重构 `1280×720` Canvas 导出渲染器，彻底解决复制图片 / 下载 PNG 时与弹窗预览脱节、丢失数据的问题——补齐 **暗夜矩阵** 顶部时间段列头（`00:00` / `01:15` … 或 `00h` / `04h` …）与双栏布局、**极坐标** 6 维径向轴线与标签（`TOK` / `HIT` / `CTX` / `TPS` / `OUT` / `REQ`）、**放射光谱** 缓存命中率主指标 / 内环辅助圆 / 彩色模型进度条、**蜂巢热核** 双栏晶格描边与 Token 进度条，统一深空渐变底板、模板专属徽标与跨 WebView `roundRect` 兜底。
- **模型名智能缩写与规格后缀保真**：重写热力矩阵纵轴模型缩写（`G4 Hydro·H`、`G4 Neon·XH`、`G4 Argon·H`），消除断括号与大小写不统一问题；窄列表及导出海报截断长模型名时自动保留尾部规格后缀（`(High)` / `(Med)` / `(XHigh)`），避免同系列模型撞名。
- **全站 Apple HIG 视觉统一、等高卡片与暗黑模式细节精修**：
  - 统一 `Dashboard` 备用账号、`Token Stats` 流水 / 模型 / 工程页的分段胶囊选中态与品牌蓝主操作色，补齐暗黑模式微描边层级（`dark:ring-white/[0.08]`）；扩宽流水页搜索框避免占位符截断，修复蜂巢海报副标题孤行换行。
  - 优化账号网格视图单窗口（仅 `5h` 或 `7d`）与双窗口卡片并排时的等高对称节奏，并修复切号加载中蓝底蓝字导致旋转图标不可见的细节问题。
  - 为横向模型筛选条增加右侧渐隐遮罩（`.fade-mask-right`），为 Mini 悬浮窗列表增加柔和底部渐隐（`.fade-mask-bottom-soft`），全站主画布与弹窗切换轻盈细滚动条（`.app-scroll` / `.mini-scroll`），并为仪表盘脉冲等化器补齐悬停语义提示。

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
