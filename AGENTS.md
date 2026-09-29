# Project Maintenance Guidelines

- **Architecture**: This project is a gateway that aggregates four AI protocols — OpenAI Responses, OpenAI Chat Completions, Anthropic Claude, and Google Gemini — and outputs Antigravity-style Gemini protocol format.
- **Pipeline First**: Keep the pipeline strictly decoupled from specific protocols. The four protocols function purely as Gemini adapters. Adapters are restricted to parameter normalization, payload transformation, protocol divergence adaptation, and edge cases unsolvable within the pipeline stage. The pipeline stage uniformly handles the converted Gemini payloads, including thinking block backfilling, thinking budget filtering and backfilling, unified context structural alignment, prefix stability, and the sanitization of risky prompts and request headers.
- **Backend Fix Strategy**: Prioritize protocol-agnostic, generic fixes within the pipeline rather than localized adapter modifications. Treat adapter-level patches as a last resort only when a generic pipeline solution is infeasible or degrades compatibility.
- **Repository Layout**:
  - `src/` — React 19 + antd + Vite frontend. `npm run dev` serves on port 1420 (Tauri `devUrl`); `npm run build` = `tsc && vite build` → `dist/`.
  - `src-tauri/src/proxy/` — gateway core (axum): `server.rs` (HTTP server + `/api` admin routes), `pipeline/`, `adapters/` + `mappers/` (per-protocol Gemini adapters), `handlers/`, `thinking_store.rs`, `token_manager.rs`.
  - `src-tauri/src/commands/` — Tauri IPC command layer (thin wrappers over modules/proxy); `src-tauri/src/modules/` — domain services (accounts, oauth, db, scheduler, tray, updater).
  - `docker/` — Dockerfiles incl. headless backend image (`Dockerfile.backend`, entrypoint `--headless`); `web_site/` — marketing pages only, NOT the app UI; `docs/RELEASE_GUIDE.md` — release procedure.
- **Dual-Transport Frontend (WebUI / WebView2-independence direction)**:
  - Phased removal design (dual-track: Tauri desktop + no-WebView service build): see `docs/WEBUI_NO_WEBVIEW2_PLAN.md` before touching transport or packaging layers.
  - The React UI is already transport-agnostic: `src/utils/request.ts` calls Tauri `invoke()` inside the webview, or maps the same command to an HTTP call via `COMMAND_MAPPING` in a plain browser (detected by `isTauri()` in `src/utils/env.ts`).
  - `--headless` mode (`src-tauri/src/lib.rs`, Docker `PORT=8045`, `/health` healthcheck) runs gateway + Web UI with no visible window and forces auth mode `AllExceptHealth` — this is the existing "webui without webview" deployment path.
  - Rule: every GUI feature must work in BOTH transports — add the Tauri command AND a `COMMAND_MAPPING` entry AND the matching `/api` admin route in `proxy/server.rs`. Unmapped commands hard-fail in Web mode (`Command [x] not supported in Web mode.`).
  - Direction (dropping WebView2 dependency long-term): prefer new `/api` endpoints and avoid direct `@tauri-apps/*` plugin calls in components (`dialog`/`fs`/`opener`/`autostart`/`updater`/`process` are Tauri-only); keep desktop-specific behavior (tray, window state, single instance, updater, `WebView2Loader.dll` bundling) in Rust/tauri.conf.json, never in UI code.
  - Do not confuse the `/api` admin routes in `proxy/server.rs` (main Web UI API) with `src-tauri/src/modules/http_api.rs` (separate auxiliary HTTP API, default port 19527, account switching only).
- **UI Design & Headless Compatibility**:
  - **Minimalist & Contextual UI**: Prioritize user-friendly, non-intrusive interactions. Reuse existing design conventions (e.g., pill toggle buttons, badge switches, or contextual setting panels) placed strictly within their most relevant sections rather than scattering unrelated controls.
  - **Headless & CLI Parity**: Ensure GUI configurations maintain functional parity across headless servers, CLI environments, and cross-platform environments. Provide configuration file fields, environment variable overrides, or dedicated CLI flags/commands for essential settings.
  - **Cross-Platform Compatibility**: Evaluate every code addition and dependency change for seamless cross-platform support.
- **Code Quality**: Prioritize root-cause, future-proof fixes rather than hardcoded logic, dead code, or speculative changes. Prioritize generalized solutions that cover entire classes of problems rather than one-off patches.
  - *Model Routing Example*: Prioritize wildcard patterns (e.g., `gemini-*-flash-*`) to anticipate future model releases rather than exact string matches.
  - *Prompt Sanitization Example*: For agent-client prompt sanitization, prioritize regex-based pattern matching over static keyword replacement, ensuring full coverage without stripping pipeline system prompts or user queries.
- **Formatting & CI Discipline**:
  - **Unit Testing**: Keep focused — run targeted tests for touched modules locally; CI compiles test targets without executing them. Skip tests for trivial edits (constants, prompts, or config tweaks). No need to run the full suite locally.
  - **Pre-flight Checks**: Run the essentials before submitting PRs or release tags:
    - `cd src-tauri && cargo fmt -- --check` (for Rust edits)
    - `cd src-tauri && cargo clippy --all-targets --all-features` (comprehensive Rust gate, already includes compilation — no separate `cargo check` needed)
    - `npm run build` (when `src/` or frontend configs changed)
    - Rely on CI for full-app compilation (`tauri build`) and full test execution. Local pre-flight covers fmt + clippy + frontend build only.
- **Release Channels & Discipline**:
  - **Release Channel Separation**:
    - **Stable Releases (正式版)**: Exclusively on `main`. Deploys official production packages, updates Docker/GitHub `latest` tags, and services automatic update channels.
    - **Preview Releases (预览版 / Beta)**: Exclusively on `beta`. Independently builds and publishes pre-releases (`makeLatest: false`, `prerelease: true`) without touching production update channels.
  - **Maintainer Staging Protocol**:
    - When introducing new changes (features, major refactors, non-trivial fixes), prompt and confirm with maintainers whether to implement and test on `beta` branch first.
    - Validate stability on `beta` (with optional independent preview builds) prior to merging into `main`.
  - **Standard Release Workflow**:
    1. **Atomic Version Sync**: Run `npm run bump <patch|minor|beta|version>` to synchronize all project manifests and generate changelog skeletons.
    2. **Documentation & Attribution**:
       - Audit Git history (`<last-tag>..HEAD`) and merged PRs to summarize all authors, co-authors, and linked Issues/PRs (`Fixes #xxx`, `PR #xxx`). Attribute every contributor inline (`Thanks to @username`) in `CHANGELOG.md` (and `CHANGELOG_EN.md`).
       - **Synchronize README Changelog (同步首页更新日志)**: For stable releases, you **MUST** update the release summary in both `README.md` (under "## 📝 更新日志") and `README_EN.md` (under "## 📝 Changelog"). Never update only `CHANGELOG.md` while leaving `README.md` / `README_EN.md` with outdated release notes. Pre-release / beta versions remain exclusively in changelogs; stable releases require full synchronization across both README files.
    3. **Pre-flight before Tagging**: Run the Pre-flight Checks above on the exact commit to be tagged.
    4. **Commit, Tag & Push**: Push stable releases to `main` (`git tag vX.Y.Z && git push origin vX.Y.Z`), reserving `beta` exclusively for pre-releases (`git tag vX.Y.Z-beta.N && git push origin vX.Y.Z-beta.N`). The release gate strictly intercepts cross-branch misplacement. Tags must match `CHANGELOG.md` headings character-for-character (including `v` prefix and pre-release suffix).
  - *Full procedure*: See `docs/RELEASE_GUIDE.md` for bump options, changelog templates, and rollback steps.
- **Thinking Cache Invalidation Control (发版清理建议)**:
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