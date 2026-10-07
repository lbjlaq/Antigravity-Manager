# Antigravity Tools / Antigravity Manager 项目规则与持久记忆

## 1. 项目定位与权威文档优先准则 (Documentation-First Mandate)

- **项目定位**：本项目（Antigravity Tools / Antigravity Manager）是基于Tauri v2+Rust+React 19构建的专业级AI账号管理、协议转换与高性能本地AI调度网关系统。
- **权威文档优先阅读规范**：
  - 在开展本项目的任何功能研发、缺陷修复、架构调整、调试测试、配置变动或打包分发任务时，**必须优先阅读根目录下的 [README.md](README.md)**，全面掌握项目的系统全貌、界面导览、多协议转换逻辑、模型映射机制与版本演进脉络。
  - 严禁在未阅读主文档的情况下基于外部惯性或脱离实际上下文臆测业务逻辑。

---

## 2. 技术栈与架构基线

- **技术栈构成**：
  - **前端**：React 19、TypeScript、Vite、Tailwind CSS、Ant Design、DaisyUI、Zustand。
  - **桌面与底层**：Tauri v2、Rust（基于Axum提供高性能API网关、Token管理、OAuth认证与流式转换）。
- **模块与目录边界**：
  - `src-tauri/`：Rust原生核心，包含Axum网关服务、OAuth授权、账号池健康探测、403/429故障转移与协议转换逻辑。
  - `src/`：React前端渲染层，包含仪表盘监控、账号列表、模型路由配置、反代控制与国际化资源。
  - `scripts/`：分发构建、修复补丁与自动化维护脚本。
  - `deploy/` 与 `docker/`：容器化部署与特定发行版部署配置。

---

## 3. 本地构建与打包发布工作流

针对本地开发环境与打包交付，严格遵守以下构建链路标准：

- **前端依赖安装**：
  ```bash
  npm install --legacy-peer-deps
  ```
- **本地桌面应用编译（Tauri）**：
  ```bash
  npm run tauri build
  ```
- **macOS DMG一键打包（集成修复补丁）**：
  - 脚本路径：[scripts/package_dmg.sh](scripts/package_dmg.sh)
  - 规范流程：在执行该脚本前，必须确保已先完成`npm run tauri build`并成功产出`src-tauri/target/release/bundle/macos/Antigravity Tools.app`。该脚本会自动将App产物、修复macOS提示损坏的脚本（`scripts/Fix_Damaged.command`）以及`/Applications`快捷方式软链接打包进DMG镜像。
- **Windows本地构建提示**：
  - 参考根目录下的 [exe_build.txt](exe_build.txt)，必要时配置`$env:CARGO_TARGET_DIR`以优化编译盘符与缓存路径。

---

## 4. 核心业务与协议转换规范

- **多协议适配与中继**：
  - `/v1/chat/completions`（OpenAI格式）
  - `/v1/messages`（Anthropic格式，完整支持Claude Code CLI与思维链透传）
  - Gemini原生SDK直接调用
- **状态自愈与账号轮换**：
  - 涉及429限流与401失效时，必须遵循后端毫秒级静默轮换与重试设计，不得破坏既有故障自愈链路。
  - 涉及403禁止状态时，遵循账号状态自动标记并跳过机制。

---

## 5. 代码与文档协同演进准则

- **就地感知与发现即修**：在阅读、修改或验证代码时，若发现既有行内注释、接口定义或文档（如 [README.md](README.md)、[docs/](docs/) 下的相关技术手册）与最新实现存在偏差，顺手就地同步更新。
- **零破坏原则**：修改模型映射、路由调度或构建脚本时，保持既有平台兼容性，严禁破坏CI流水线（`.github/workflows/release.yml`）与既有打包逻辑。

---

## 6. 自研定制资产绝对保留与上游安全同步准则 (Custom Asset Preservation Mandate)

- **核心原则**：除非官方发布底层断代式重大重构且事先获得明确确认，否则在任何日常维护、功能扩展或上游同步中，**绝对禁止覆盖或回退用户的任何历史自研修改**。
- **神圣保留的自研核心清单**：
  1. **前端监控与统计看板**：[src/pages/Dashboard.tsx](src/pages/Dashboard.tsx)（倒计时、今日使用汇总）与 [src/pages/TokenStats.tsx](src/pages/TokenStats.tsx)（多维度趋势图表）；
  2. **配额关注系统**：[src/config/modelConfig.ts](src/config/modelConfig.ts) 中的虚拟配额组（`gemini-5h`、`gemini-weekly`、`3p-5h`、`3p-weekly`）及 [src/components/accounts/AccountCard.tsx](src/components/accounts/AccountCard.tsx) 中的动态注入提取逻辑，严禁引入强制劫持显示单一模型的代码；
  3. **纯净导航体验**：[src/components/navbar/Navbar.tsx](src/components/navbar/Navbar.tsx) 严禁强塞商业推广路由（如 `/apikey-fun` 中转站）；
  4. **自研错峰与保护调度**：相控阵错峰调度系统（[PhaseScheduler.tsx](src/components/settings/PhaseScheduler.tsx)）及反重力IDE活跃主号智能探测与API保底避让机制；
  5. **模型映射与协议修复**：`src-tauri/src/proxy/common/model_mapping.rs` 中全套自研的Gemini 3.8/3.7/3.6 Flash细粒度映射与流式终端状态修复。
- **增量合入规范**：同步上游主线时，必须始终以当前自研分支为主干基准，采取防御性单点补丁与三方合并方式合入后端底层修复，严禁使用 `git reset --hard` 或全量覆写前端目录。
- **冲突显式呈报与决策等待准则**：在合并或同步过程中，如果遇到任何代码冲突、接口签名不兼容或业务逻辑冲突，**绝对严禁擅自裁决、静默丢弃自研代码或单方面倾向官方**；必须立即挂起操作，向用户清晰呈报冲突细节、技术影响与备选方案，**严格等待用户明确抉择指示后方可推进**。

---

## 7. 上游底层架构规范 (Upstream Maintenance Guidelines)

- **Architecture**: This project is a gateway that aggregates four AI protocols — OpenAI Responses, OpenAI Chat Completions, Anthropic Claude, and Google Gemini — and outputs Antigravity-style Gemini protocol format.
- **Pipeline First**: Keep the pipeline strictly decoupled from specific protocols. The four protocols function purely as Gemini adapters. Adapters are restricted to parameter normalization, payload transformation, protocol divergence adaptation, and edge cases unsolvable within the pipeline stage. The pipeline stage uniformly handles the converted Gemini payloads, including thinking block backfilling, thinking budget filtering and backfilling, unified context structural alignment, prefix stability, and the sanitization of risky prompts and request headers.
- **Backend Fix Strategy**: Prioritize protocol-agnostic, generic fixes within the pipeline rather than localized adapter modifications. Treat adapter-level patches as a last resort only when a generic pipeline solution is infeasible or degrades compatibility.
- **UI Design & Headless Compatibility**:
  - **Minimalist & Contextual UI**: Prioritize user-friendly, non-intrusive interactions. Reuse existing design conventions (e.g., pill toggle buttons, badge switches, or contextual setting panels) placed strictly within their most relevant sections rather than scattering unrelated controls.
  - **Headless & CLI Parity**: Ensure GUI configurations maintain functional parity across headless servers, CLI environments, and cross-platform environments. Provide configuration file fields, environment variable overrides, or dedicated CLI flags/commands for essential settings.
  - **Cross-Platform Compatibility**: Evaluate every code addition and dependency change for seamless cross-platform support.
- **Code Quality**: Prioritize root-cause, future-proof fixes rather than hardcoded logic, dead code, or speculative changes. Prioritize generalized solutions that cover entire classes of problems rather than one-off patches.
  - *Model Routing Example*: Prioritize wildcard patterns (e.g., `gemini-*-flash-*`) to anticipate future model releases rather than exact string matches.
  - *Prompt Sanitization Example*: For agent-client prompt sanitization, prioritize regex-based pattern matching over static keyword replacement, ensuring full coverage without stripping pipeline system prompts or user queries.
- **Formatting & CI Discipline**:
  - **Unit Testing**: Keep focused — run targeted tests for touched modules locally; CI compiles test targets without executing them. Skip tests for trivial edits (constants, prompts, or config tweaks). No need to run the full suite locally.
  - **Local Test Discretion**: For strings, constants, hardcoded values, or other trivial edits, ask after the task whether a small functional check is wanted rather than testing on your own. Self-test only when four or more core backend or frontend interaction files are involved; hardcoded-only edits do not count as core-file changes. In general, follow the user's preference on whether to test.
  - **Pre-flight Checks**: Execute only when tagging final releases; should not be executed during daily tasks, code reviews, simple edits, simple debugging, or trivial hardcoded changes. Run on demand before releasing:
    - `cd src-tauri && cargo fmt -- --check` (for Rust edits)
    - `cd src-tauri && cargo clippy --all-targets --all-features` (comprehensive Rust gate, already includes compilation — no separate `cargo check` needed)
    - `npm run build` (when `src/` or frontend configs changed)
    - Rely on CI for full-app compilation (`tauri build`) and full test execution. Local pre-flight covers fmt + clippy + frontend build only. If the local host lacks the required dependencies or toolchains (e.g. MinGW windres on Windows, specific linkers, or platform libraries), skip the local check and delegate verification to the remote CI pipeline.
- **Release Channels & Discipline**:
  - **Release Channel Separation**:
    - **Stable Releases**: Exclusively on `main`. Deploys official production packages, updates Docker/GitHub `latest` tags, and services automatic update channels.
    - **Preview Releases (Beta)**: Exclusively on `beta`. Independently builds and publishes pre-releases (`makeLatest: false`, `prerelease: true`) without touching production update channels.
  - **Maintainer Staging Protocol**:
    - When introducing new changes (features, major refactors, non-trivial fixes), prompt and confirm with maintainers whether to implement and test on `beta` branch first.
    - Validate stability on `beta` (with optional independent preview builds) prior to merging into `main`.
  - **Standard Release Workflow**:
    1. **Atomic Version Sync**: Run `npm run bump <patch|minor|beta|version>` to synchronize all project manifests and generate changelog skeletons.
    2. **Documentation & Attribution**:
       - Audit Git history (`<last-tag>..HEAD`) and merged PRs to summarize all authors, co-authors, and linked Issues/PRs (`Fixes #xxx`, `PR #xxx`). Attribute every contributor inline (`Thanks to @username`) in `CHANGELOG.md` (and `CHANGELOG_EN.md`).
       - **Synchronize README Changelog**: For stable releases, update the release summary in both `README.md` (English home under "## 📝 Changelog") and `README_ZH.md` (Chinese home under "## 📝 更新日志"). Keep both README files synchronized with the release notes alongside `CHANGELOG.md`. Pre-release / beta versions remain exclusively in changelogs; stable releases require full synchronization across both README files.
    3. **Pre-flight before Tagging**: Run the Pre-flight Checks above on the exact commit to be tagged.
    4. **Commit, Tag & Push**: Push stable releases to `main` (`git tag vX.Y.Z && git push origin vX.Y.Z`), reserving `beta` exclusively for pre-releases (`git tag vX.Y.Z-beta.N && git push origin vX.Y.Z-beta.N`). The release gate strictly intercepts cross-branch misplacement. Tags must match `CHANGELOG.md` headings character-for-character (including `v` prefix and pre-release suffix).
  - *Full procedure*: See `docs/RELEASE_GUIDE.md` for bump options, changelog templates, and rollback steps.
- **Thinking Cache Invalidation Control (Release Cache Guidance)**:
  - File: `src/components/common/SuggestionDeleteThinkingModal.tsx`
  - Routine releases (no prompt): Keep `SUGGESTION_DELETE_THINKING_STORE = false`.
  - Architecture / schema refactors (prompt users to clean once):
    1. Set `SUGGESTION_DELETE_THINKING_STORE = true`.
    2. Set `SUGGESTION_TARGET_VERSION = '<version>'` (e.g. `'4.8.2'`).
    3. On upgrade, users with existing cache get a one-time prompt; action state persists in `gui_config.json`.
- **Branch & History Hygiene**:
  - Branch from remote bases (`origin/beta` for staged features, `origin/main` for direct hotfixes) rather than local branches to prevent untracked ancestor commits.
  - Inspect in-flight PRs (`gh pr list --base main`) before rewriting published tips, avoiding force-pushes across shared branches.
  - Retain local rollback refs (`backup/*`) before history rewrites, confirming zero content drift via `git diff --stat <backup> HEAD`.
- **PR Scope & Grouping**:
  - **Single Problem Scope**: A PR represents a cohesive collection of fixes or features dedicated to a single problem class. Keep unrelated concerns (such as governance, release tooling, or documentation) in isolated PRs.
  - **Self-Contained & Individually Revertable**: A PR may contain multiple commits, but each commit must represent an independent, self-contained functional unit that is individually revertable, avoiding messy or tangled changesets.
  - **Local Convergence & Final-State Commits**: Commit freely during local debugging on development branches; however, before opening or merging a PR, audit and consolidate scattered iterative attempts into clean, high-quality units. Each consolidated commit must describe only its successful final state and rationale, eliminating intermediate trial-and-error noise.
  - **Review & Template Alignment**: Route every PR through peer review and complete `.github/PULL_REQUEST_TEMPLATE.md` (problem classification, behavior alterations, unverified paths, and rollback strategy).
- **Commit & Attribution Discipline (提交信息与致谢纪律)**:
  - **Issue/PR Linkage in Commit Messages**: Every commit message must explicitly state and link the relevant Issue and PR numbers involved or resolved (e.g. `Fixes #xxx`, `Resolves #xxx`, `Ref #xxx`, `PR #xxx`). Vague, unreferenced commits are strictly prohibited.
  - **Strictly Scoped Attribution (致谢范围约束)**: Gratitude, inline attribution, and co-authorship are strictly limited to:
    1. The current active developer/author;
    2. The contributor/author of the referenced PR;
    3. User-defined co-creators (e.g., `Co-Authored-By: JeikCode <code@jeikcode.top>`).
    Never emit indiscriminate, unverified, or irrelevant thanks/attributions to arbitrary third parties.
- **Contributor Respect & Attribution**:
  - Preserve authorship by preferring the contributor's own PR for squash commits, or attaching explicit `Co-authored-by:` trailers on merge commits and proxy PRs.
  - Disclose costs before merging: highlight affected existing behaviors and unverified paths alongside improvements.
  - Own tooling and documentation gaps directly rather than attributing downstream frictions to contributors.
  - Accompany every rejection with a concrete file-by-file accept/drop breakdown and an actionable path forward.
- **Risky Path Replacement**:
  - Keep an explicit fallback to the verified path whenever you replace it (e.g. "0 matches found → full restart"). A silent no-op is worse than the failure it was meant to fix.
  - Order side effects to fail before the point of no return: write credentials before killing a process, validate before deleting.
  - Detect broadly, act narrowly: a matcher may recognize a whole class of problems, while its effect stays inside the intended data — not across line breaks, tags, or other clauses. Bound every wait with a timeout.

Maintained by @jeikl

