# OpenCode Sync Follow-up Todo List

## Implementation Checklist

- [x] Replace wrong Opus catalog entry in `src-tauri/src/proxy/opencode_sync.rs`:
  - from `claude-opus-4-5-thinking`
  - to `claude-opus-4-6-thinking`
- [x] Keep legacy cleanup compatibility by including `claude-opus-4-6-thinking` in `ANTIGRAVITY_MODEL_IDS`.
- [x] Add/extend sync tests in `opencode_sync.rs`:
  - catalog contains Opus 4.6
  - filtered sync for Opus 4.6 works
  - Opus 4.6 variants are present (`low`, `medium`, `high`, `max`)
- [x] Enhance thinking extraction in `src-tauri/src/proxy/handlers/claude.rs`:
  - support root fields and `providerOptions`
  - support multiple budget/level keys
  - preserve root-over-providerOptions precedence
- [x] Add unit tests in `claude.rs` for extraction and precedence.

## Verification Checklist

### Rust checks

- [x] Run `cargo check` in `src-tauri`.
- [ ] Run `cargo test proxy::opencode_sync::tests --lib` in `src-tauri`.
  - Note: may fail on current baseline due unrelated existing test compile errors in `openai/request.rs`.

### Frontend/build checks

- [ ] Run `npm run build` in repo root.
  - Note: may fail if local environment misses `@tailwindcss/container-queries`.

### Manual OpenCode checks

- [ ] Sync OpenCode models with `Claude 4.6 TK` selected.
- [ ] Confirm `claude-opus-4-6-thinking` exists in `~/.config/opencode/opencode.json` under `provider.antigravity-manager.models`.
- [ ] Open model picker and confirm Opus 4.6 is visible.
- [ ] Use thinking variant switch (for example `Ctrl+T`) and submit prompt.
- [ ] Confirm proxy logs show applied thinking hint and mapped budget/effort.

## Acceptance Criteria

- [ ] Issue 1 resolved: Opus 4.6 appears after sync.
- [ ] Issue 2 resolved: thinking variant hints are extracted reliably from root or providerOptions payloads.
- [ ] No regressions introduced in OpenCode sync clear/restore flow.

## PR Completion Checklist

- [ ] Update PR description with:
  - root cause summary
  - files changed
  - verification results (including known baseline/environment blockers)
- [ ] Push branch to remote.
- [ ] Open PR against `main` with roadmap/todo docs linked.
