# Project Maintenance Guidelines

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