# OpenCode Sync Follow-up Roadmap

## Context

This follow-up addresses two production issues reported after the OpenCode provider-isolation change:

1. `Claude Opus 4.6 Thinking` can be selected in the app sync modal, but does not appear in OpenCode model picker after sync.
2. Thinking variant changes (for example via `Ctrl+T` in OpenCode) are not reliably applied by proxy logic.

## Objectives

- Keep OpenCode model catalog in sync with app model registry.
- Ensure thinking hints are extracted from both root payload fields and `providerOptions` fields.
- Preserve backward compatibility for existing thinking payload shapes.

## Scope

### In scope

- `src-tauri/src/proxy/opencode_sync.rs`
  - Correct catalog model ID and metadata for Opus 4.6.
  - Keep legacy cleanup compatibility list up to date.
  - Add regression tests for Opus 4.6 sync output.
- `src-tauri/src/proxy/handlers/claude.rs`
  - Add robust extraction of thinking hints from `providerOptions`.
  - Keep root-level fields as highest-priority source.
  - Add focused unit tests for extraction precedence and provider keys.

### Out of scope

- Frontend UI redesign.
- OpenCode client implementation changes.
- Global rustfmt cleanup across unrelated modules.

## Design Notes

- Root-level thinking fields remain highest precedence.
- `providerOptions` fallback is supported with preferred provider keys:
  - `anthropic`
  - `antigravity-manager`
  - `google`
- Additional provider keys are scanned as a safe fallback to reduce schema drift risk.

## Delivery Phases

1. Catalog correction and cleanup-list update.
2. Thinking extraction enhancement and precedence enforcement.
3. Unit test additions for both modules.
4. Build/check verification and manual OpenCode validation.

## Validation Matrix

| Area | Method | Expected |
|---|---|---|
| Catalog sync output | Rust unit tests in `opencode_sync.rs` | `claude-opus-4-6-thinking` exists and includes variants |
| Thinking extraction | Unit tests in `claude.rs` | Root precedence works, providerOptions fallback works |
| Rust compile | `cargo check` in `src-tauri` | Pass |
| OpenCode manual flow | Sync config then inspect model picker | Opus 4.6 appears |
| Thinking behavior manual flow | Select thinking variant and send prompt | Proxy logs show applied hint and mapped budget/effort |

## Known Verification Constraints

- `cargo test proxy::opencode_sync::tests --lib` on current baseline can fail due unrelated existing test compilation issues in `openai/request.rs` (not introduced by this follow-up).
- Frontend build may fail locally if environment is missing `@tailwindcss/container-queries` dependency.

## Rollback Plan

- Revert follow-up commit(s) if any behavior regression is observed.
- Restore previous OpenCode config via built-in restore feature or backup files (`*.antigravity-manager.bak`).
