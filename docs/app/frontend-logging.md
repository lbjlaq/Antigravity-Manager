# Frontend error logging (UI “white screen” troubleshooting)

The UI runs inside a WebView. If the React app crashes early (bundle load error, runtime exception, rendering error), it can appear as a blank/white window with little visible context.

To make these failures actionable, the app captures frontend errors and forwards them into the backend log file.

## What is captured
- Unhandled JS errors (`window.onerror`)
- Unhandled promise rejections (`unhandledrejection`)
- React render-time errors via an `ErrorBoundary`
- `console.error`, `console.warn`, and `console.info` messages (mirrored)

## Where to look

Backend log directory:
- `~/.antigravity_tools/logs/`

Look for entries tagged with:
- `[frontend]`

## Secret handling

Frontend logs are best-effort redacted before being written to disk:
- Strips common `Authorization: Bearer ...` patterns
- Strips common `sk-...` style tokens

Note:
- This reduces risk but is not a substitute for good operational hygiene. Avoid pasting secrets into the UI console.

## Implementation pointers

Backend command:
- [`src-tauri/src/commands/mod.rs`](../../src-tauri/src/commands/mod.rs) (`frontend_log`)

Tauri wiring:
- [`src-tauri/src/lib.rs`](../../src-tauri/src/lib.rs)

Frontend setup:
- [`src/utils/frontendLogging.ts`](../../src/utils/frontendLogging.ts)
- [`src/components/common/ErrorBoundary.tsx`](../../src/components/common/ErrorBoundary.tsx)
- [`src/main.tsx`](../../src/main.tsx)

