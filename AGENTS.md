# Project Maintenance Guidelines

- **Architecture**: This project is a gateway that aggregates four AI protocols — OpenAI Responses, OpenAI Chat Completions, Anthropic Claude, and Google Gemini — and outputs Antigravity-style Gemini protocol format.
- **Pipeline First**: Keep the pipeline strictly decoupled from specific protocols. The four protocols function purely as Gemini adapters. Adapters are restricted to parameter normalization, payload transformation, protocol divergence adaptation, and edge cases unsolvable within the pipeline stage. The pipeline stage uniformly handles the converted Gemini payloads, including thinking block backfilling, thinking budget filtering and backfilling, unified context structural alignment, prefix stability, and the sanitization of risky prompts and request headers.
- **Fix Strategy**: Prioritize protocol-agnostic, generic fixes within the pipeline rather than localized adapter modifications. Treat adapter-level patches as a last resort only when a generic pipeline solution is infeasible or degrades compatibility.
- **Code Quality**: Prioritize root-cause, future-proof fixes rather than hardcoded logic, dead code, or speculative changes. Prioritize generalized solutions that cover entire classes of problems rather than one-off patches.
  - *Model Routing Example*: Prioritize wildcard patterns (e.g., `gemini-*-flash-*`) to anticipate future model releases rather than exact string matches.
  - *Prompt Sanitization Example*: For agent-client prompt sanitization, prioritize regex-based pattern matching over static keyword replacement, ensuring full coverage without stripping pipeline system prompts or user queries.
- **Formatting & CI Discipline**:
  - **Unit Testing & Pre-flight Checks**: Prioritize targeted unit tests covering all scenarios described in the issue rather than exhaustive test suites, avoiding irrelevant test runs. Skip writing tests for trivial edits (e.g., hardcoded values, prompt strings, constants, or variable tweaks). Run `cargo fmt` (under `src-tauri/`) and verify that both `cargo check` and `npm run build` pass cleanly only when preparing to submit/update a PR or publish a new release tag.
- **UI Design & Headless Compatibility**:
  - **Minimalist & Contextual UI**: Prioritize user-friendly, non-intrusive interactions. Reuse existing design conventions (e.g., pill toggle buttons, badge switches, or contextual setting panels) placed strictly within their most relevant sections rather than scattering unrelated controls.
  - **Headless & CLI Parity**: Ensure GUI configurations maintain functional parity across headless servers, CLI environments, and cross-platform environments. Provide configuration file fields, environment variable overrides, or dedicated CLI flags/commands for essential settings.
  - **Cross-Platform Compatibility**: Evaluate every code addition and dependency change for seamless cross-platform support.
- **Release Discipline & Standard Workflow**:
  1. **Atomic Version Sync**: Run `npm run bump patch` (or `minor`, `beta`, or a specific version) to synchronize version strings across all configuration files and generate changelog templates.
  2. **Documentation Maintenance**: Document core release features and credit contributors (`* @username`) in `CHANGELOG.md`. Update version strings and release descriptions in both English and Chinese `README` files.
  3. **Commit, Tag & Push**: Commit release artifacts, push to `main`, and push the matching tag (`git tag vX.Y.Z && git push origin vX.Y.Z`) to trigger automated CI/CD release workflows.
- **Git & Attribution**:
  - PR merge commits should include contributor attribution.
  - Release notes should credit contributors along with their specific contributions (e.g., `Thanks to @username for implementing Claude thinking effort`).

Maintained by @jeikl