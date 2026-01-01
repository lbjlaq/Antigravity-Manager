# Proxy request logging

## Goal
Provide a low-noise way to debug client traffic through the local proxy without risking secret leakage.

## Configuration
Stored under `proxy.access_log_enabled` in `gui_config.json` (app data directory).

- `false` (default): no per-request access logs.
- `true`: logs method/path/status/latency for each request.

## What is logged
When enabled, each request produces a single access log entry with:
- HTTP method
- path (no query string)
- status code
- latency (ms)
- a best-effort `upstream` label (e.g. `zai`, `zai_mcp`, or `unknown`)

Security guarantees:
- No query strings
- No headers
- No request/response bodies

## Implementation
Backend:
- Config flag: `src-tauri/src/proxy/config.rs`
- Middleware: `src-tauri/src/proxy/middleware/access_log.rs`
- Server wiring + hot update: `src-tauri/src/proxy/server.rs`, `src-tauri/src/commands/mod.rs`
- Upstream labels: `src-tauri/src/proxy/observability.rs`, set in handlers:
  - `src-tauri/src/proxy/handlers/claude.rs` (z.ai)
  - `src-tauri/src/proxy/handlers/mcp.rs` (z.ai MCP reverse-proxy)

Frontend:
- UI toggle: `src/pages/ApiProxy.tsx`
- Strings: `src/locales/en.json`, `src/locales/zh.json`

Related troubleshooting:
- UI/runtime error capture (helps diagnose “white screen”): [`docs/app/frontend-logging.md`](../app/frontend-logging.md)
