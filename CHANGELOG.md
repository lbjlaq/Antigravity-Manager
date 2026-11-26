# Changelog / 更新日志

> This changelog exclusively tracks **Antigravity Manager** (`antigravity-manager` branch, `am-core` + `am-cli` + `am-gui` workspace).  
> 本更新日志仅记录 **Antigravity Manager**（`antigravity-manager` 分支，`am-core` + `am-cli` + `am-gui` 架构）的版本演进。

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
