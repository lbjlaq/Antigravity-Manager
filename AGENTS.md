# Project Maintenance Guidelines

- **Architecture**: This project is a gateway that aggregates four AI protocols — OpenAI Responses, OpenAI Chat Completions, Anthropic Claude, and Google Gemini — and outputs Antigravity-style Gemini protocol format.
- **Pipeline First**: Keep the pipeline strictly decoupled from specific protocols. The four protocols function purely as Gemini adapters. Adapters are restricted to parameter normalization, payload transformation, protocol divergence adaptation, and edge cases unsolvable within the pipeline stage. The pipeline stage uniformly handles the converted Gemini payloads, including thinking block backfilling, thinking budget filtering and backfilling, unified context structural alignment, prefix stability, and the sanitization of risky prompts and request headers.
- **Fix Strategy**: Prioritize protocol-agnostic, generic fixes within the pipeline rather than localized adapter modifications. Treat adapter-level patches as a last resort only when a generic pipeline solution is infeasible or degrades compatibility.
- **Code Quality**: Prioritize root-cause, future-proof fixes rather than hardcoded logic, dead code, or speculative changes. Prioritize generalized solutions that cover entire classes of problems rather than one-off patches.
  - *Model Routing Example*: Prioritize wildcard patterns (e.g., `gemini-*-flash-*`) to anticipate future model releases rather than exact string matches.
  - *Prompt Sanitization Example*: For agent-client prompt sanitization, prioritize regex-based pattern matching over static keyword replacement, ensuring full coverage without stripping pipeline system prompts or user queries.
- **Formatting & CI Discipline**:
  - **Unit Testing**: Prioritize targeted unit tests covering all scenarios described in the issue rather than exhaustive test suites, avoiding irrelevant test runs. Skip writing tests for trivial edits (e.g., hardcoded values, prompt strings, constants, or variable tweaks).
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
  2. **Documentation Maintenance**: Document core release features and credit contributors (`* @username`) in `CHANGELOG.md`. Update version strings and release descriptions in both English and Chinese `README` files.
     - README reflects the latest **stable** release only. Pre-release / derived versions (any tag containing `-`, e.g. `-beta`, `-cleaned`, `-rc`) are recorded in `CHANGELOG.md` alone and must never appear in any README version string or release summary.
  3. **Pre-flight before Tagging**: Run the Pre-flight Checks above on the exact commit to be tagged. CI gates only `main` pushes and PRs targeting `main`; a tag pushed from any other branch triggers the Release workflow without a CI gate of its own.
  4. **Commit, Tag & Push**: Commit release artifacts, push to `main`, and push the matching tag (`git tag vX.Y.Z && git push origin vX.Y.Z`) to trigger automated CI/CD release workflows.
     - The tag must match the `CHANGELOG.md` version heading **character-for-character**, including the `v` prefix and the full pre-release suffix (`npm run bump beta` yields `X.Y.Z-beta.1`, so the tag is `vX.Y.Z-beta.1`, not `vX.Y.Z-beta`). Release notes are extracted by matching the tag name against the heading; a mismatch silently falls back to placeholder text.
  - **Detailed Procedure**: See `docs/RELEASE_GUIDE.md` for the full step-by-step release SOP (bump scenarios, changelog template, tag conventions, verification and rollback).
- **Git & Attribution**:
  - PR merge commits should include contributor attribution.
  - Release notes should credit contributors along with their specific contributions (e.g., `Thanks to @username for implementing Claude thinking effort`).

Maintained by @jeikl