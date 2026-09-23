# Project Maintenance Guidelines

- **Architecture**: This project is a gateway that aggregates four AI protocols — OpenAI Responses, OpenAI Chat Completions, Anthropic Claude, and Google Gemini — and outputs Antigravity-style Gemini protocol format.
- **Pipeline First**: Keep the pipeline strictly decoupled from specific protocols. The four protocols function purely as Gemini adapters. Adapters are restricted to parameter normalization, payload transformation, protocol divergence adaptation, and edge cases unsolvable within the pipeline stage. The pipeline stage uniformly handles the converted Gemini payloads, including thinking block backfilling, thinking budget filtering and backfilling, unified context structural alignment, prefix stability, and the sanitization of risky prompts and request headers.
- **Fix Strategy**: Prioritize protocol-agnostic, generic fixes within the pipeline rather than localized adapter modifications. Treat adapter-level patches as a last resort only when a generic pipeline solution is infeasible or degrades compatibility.
- **Code Quality**: Prioritize root-cause, future-proof fixes rather than hardcoded logic, dead code, or speculative changes. Prioritize generalized solutions that cover entire classes of problems rather than one-off patches.
  - *Model Routing Example*: Prioritize wildcard patterns (e.g., `gemini-*-flash-*`) to anticipate future model releases rather than exact string matches.
  - *Prompt Sanitization Example*: For agent-client prompt sanitization, prioritize regex-based pattern matching over static keyword replacement, ensuring full coverage without stripping pipeline system prompts or user queries.
- **Formatting & CI Discipline**:
  - **Unit Testing**: Prioritize targeted unit tests covering all scenarios described in the issue rather than exhaustive test suites, avoiding irrelevant test runs. Skip writing tests for trivial edits (e.g., hardcoded values, prompt strings, constants, or variable tweaks). `cargo test` is **not** part of the gate — CI compiles the test target rather than running it; run the tests covering the modules you touched, and avoid full-suite runs for convenience.
  - **Pre-flight Checks**: Before submitting/updating a PR or publishing a release tag, run the same commands as CI and fix all errors until they pass cleanly:
    - `cd src-tauri && cargo fmt -- --check`
    - `cd src-tauri && cargo clippy --all-targets --all-features`
    - `cd src-tauri && cargo check`
    - `npm run build`
    - `npm run tauri build -- --debug --no-bundle` (when Rust or Tauri config changed)
- **UI Design & Headless Compatibility**:
  - **Minimalist & Contextual UI**: Prioritize user-friendly, non-intrusive interactions. Reuse existing design conventions (e.g., pill toggle buttons, badge switches, or contextual setting panels) placed strictly within their most relevant sections rather than scattering unrelated controls.
  - **Headless & CLI Parity**: Ensure GUI configurations maintain functional parity across headless servers, CLI environments, and cross-platform environments. Provide configuration file fields, environment variable overrides, or dedicated CLI flags/commands for essential settings.
  - **Cross-Platform Compatibility**: Evaluate every code addition and dependency change for seamless cross-platform support.
- **Release Discipline & Standard Workflow**:
  1. **Atomic Version Sync**: Run `npm run bump patch` (or `minor`, `beta`, or a specific version) to synchronize version strings across all configuration files and generate changelog templates.
  2. **Documentation Maintenance**: Document core release features and credit contributors with their specific contributions in `CHANGELOG.md` (`* @username`, e.g. `Thanks to @username for implementing Claude thinking effort`). Update version strings and release descriptions in both English and Chinese `README` files.
     - README reflects the latest **stable** release only. Pre-release / derived versions (any tag containing `-`, e.g. `-beta`, `-cleaned`, `-rc`) are recorded in `CHANGELOG.md` alone — keep them out of any README version string and release summary.
  3. **Pre-flight before Tagging**: Run the Pre-flight Checks above on the exact commit to be tagged. CI gates only `main` pushes and PRs targeting `main`; a tag pushed from any other branch triggers the Release workflow without a CI gate of its own.
  4. **Commit, Tag & Push**: Commit release artifacts, push to `main`, and push the matching tag (`git tag vX.Y.Z && git push origin vX.Y.Z`) to trigger automated CI/CD release workflows.
     - The tag must match the `CHANGELOG.md` version heading **character-for-character**, including the `v` prefix and the full pre-release suffix (`npm run bump beta` yields `X.Y.Z-beta.1`, so the tag is `vX.Y.Z-beta.1`, not `vX.Y.Z-beta`). Release notes are extracted by matching the tag name against the heading; a mismatch silently falls back to placeholder text.
  - **Detailed Procedure**: See `docs/RELEASE_GUIDE.md` for the full step-by-step release SOP (bump scenarios, changelog template, tag conventions, verification and rollback).
- **Git & Attribution**:
  - PR merge commits should include contributor attribution.
- **Branch & History Hygiene**:
  - Branch from the remote base — `git checkout -b <name> origin/main`. Avoid a local `main` as the base: its unpushed commits become ancestors and land in your PR unnoticed.
  - Before rewriting a published tip, list its in-flight PRs (`gh pr list --base main --json number,headRefOid`). Avoid force-pushing a tip that an open PR sits on — the contributor inherits phantom commits and phantom diffs.
  - Keep a rollback ref before any rewrite (`backup/*`), then prove zero content change with `git diff --stat <backup> HEAD` (empty = identical).
  - Get every commit somewhere reachable the same day — a PR branch or `main`. Avoid leaving work that exists only on one machine.
  - Land one fix on one branch and keep it there. Avoid shuttling a fix between branches: every hop risks phantom commits and content drift.
- **PR Scope & Grouping**:
  - Open one PR per class of problem: group related changes (e.g. all governance or doc rules, all CI wiring) into a single PR, and keep the PR narrow enough that its title names one class.
  - Treat "separate PR" as *not mixed into a feature PR* — it does not mean one PR per edit. Batch same-class documentation, governance, and release-infrastructure edits into one such PR.
  - Keep the commits inside a PR independent and individually revertable: order them so any single commit can be reverted on its own.
  - Fill in `.github/PULL_REQUEST_TEMPLATE.md` — it asks for the class of problem, behaviour changes, unverified paths, and how to roll back.
- **Contributor Respect**:
  - Decide the carrier before writing code, and make it the contributor's own PR, so the squash commit stays authored by them (squash author = PR opener). If it must go through your own PR, add an explicit `Co-authored-by:` trailer and say so in the PR.
  - Disclose what your change costs before merging — behaviour changes to inputs that already worked, paths you could not verify — not only what it fixes.
  - Own defects that trace back to your own tooling, scripts, or docs. Avoid letting your own gap read as the contributor's fault.
  - Pair every rejection with a file-by-file accept/drop list (with line counts) and a concrete next step. Avoid leaving a contributor holding a "no" with no way forward.
- **Risky Path Replacement**:
  - Keep an explicit fallback to the verified path whenever you replace it (e.g. "0 matches found → full restart"). A silent no-op is worse than the failure it was meant to fix.
  - Order side effects to fail before the point of no return: write credentials before killing a process, validate before deleting.
  - Detect broadly, act narrowly: a matcher may recognize a whole class of problems, while its effect stays inside the intended data — not across line breaks, tags, or other clauses. Bound every wait with a timeout.

Maintained by @jeikl