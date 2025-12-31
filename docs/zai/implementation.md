# z.ai provider + MCP proxy (implemented)

This document describes the z.ai integration that is implemented on the `feat/zai-passthrough-mcp` branch: what was added, how it works internally, and how to validate it.

## Scope (current)
- z.ai is integrated as an **optional upstream** for **Anthropic/Claude protocol only** (`/v1/messages`, `/v1/messages/count_tokens`).
- OpenAI and Gemini protocol handlers are unchanged and continue to use the existing Google-backed pool.
- z.ai MCP (Search + Reader) is exposed via local proxy endpoints (reverse proxy) and injects the z.ai API key upstream.
- Vision MCP is **not implemented yet** (UI toggle exists, but no backend bridge).

## Configuration
All settings are persisted in the existing data directory (same place as Google accounts and `gui_config.json`).

### Proxy auth
- `proxy.auth_mode` (`off` | `strict` | `all_except_health` | `auto`)
  - `off`: no auth required
  - `strict`: auth required for all routes
  - `all_except_health`: auth required for all routes except `GET /healthz`
  - `auto`: if `allow_lan_access=true` -> `all_except_health`, else `off`
- `proxy.api_key`: required when auth is enabled

Implementation:
- Backend enum: `src-tauri/src/proxy/config.rs` (`ProxyAuthMode`)
- Effective policy resolver: `src-tauri/src/proxy/security.rs`
- Middleware enforcement: `src-tauri/src/proxy/middleware/auth.rs`

### z.ai provider
Config lives under `proxy.zai` (`src-tauri/src/proxy/config.rs`):
- `enabled: bool`
- `base_url: string` (default `https://api.z.ai/api/anthropic`)
- `api_key: string`
- `dispatch_mode: off | exclusive | pooled | fallback`
  - `off`: never use z.ai
  - `exclusive`: all Claude protocol requests go to z.ai
  - `pooled`: z.ai is treated as **one additional slot** in the shared pool (no priority, no strict guarantee)
  - `fallback`: z.ai is used only when the Google pool has 0 accounts
- `models`: defaults used when the incoming Anthropic request uses `claude-*` model ids
  - `opus` default `glm-4.7`
  - `sonnet` default `glm-4.7`
  - `haiku` default `glm-4.5-air`
- `mcp` toggles:
  - `enabled`
  - `web_search_enabled`
  - `web_reader_enabled`
  - `vision_enabled` (UI only; not implemented yet)

Runtime hot update:
- `save_config` hot-updates `auth`, `upstream_proxy`, `model mappings`, and `z.ai` without restart.
  - `src-tauri/src/commands/mod.rs` calls `axum_server.update_security(...)` and `axum_server.update_zai(...)`.

## Request routing

### `/v1/messages` (Anthropic messages)
Handler: `src-tauri/src/proxy/handlers/claude.rs` (`handle_messages`)

Flow:
1. The handler receives `HeaderMap` + raw JSON `Value`.
2. It decides whether to use z.ai or the existing Google flow:
   - If z.ai is disabled -> use Google flow.
   - If `dispatch_mode=exclusive` -> use z.ai.
   - If `dispatch_mode=fallback` -> use z.ai only if Google pool size is 0.
   - If `dispatch_mode=pooled` -> use round-robin across `(google_accounts + 1)` slots; slot `0` is z.ai, others are Google.
3. If z.ai is selected:
   - The raw JSON is forwarded to z.ai as-is (streaming is supported by byte passthrough).
   - The request `model` may be rewritten:
     - `glm-*` stays unchanged
     - `claude-*` becomes one of `proxy.zai.models.{opus,sonnet,haiku}` based on name match
4. Otherwise:
   - The existing Claude→Gemini transform and Google-backed execution path runs as before.

### `/v1/messages/count_tokens`
Handler: `src-tauri/src/proxy/handlers/claude.rs` (`handle_count_tokens`)
- If z.ai is enabled (mode != off), this request is forwarded to z.ai.
- Otherwise it returns the existing placeholder `{input_tokens: 0, output_tokens: 0}`.

## Upstream forwarding details (z.ai Anthropic)
Provider: `src-tauri/src/proxy/providers/zai_anthropic.rs`

Security / header handling:
- The local proxy API key must **never** be forwarded upstream.
- Only a conservative set of incoming headers is forwarded (e.g. `content-type`, `accept`, `anthropic-version`, `user-agent`).
- z.ai auth is injected:
  - If the client used `x-api-key`, it is replaced with z.ai key.
  - If the client used `Authorization`, it is replaced with `Bearer <zai_key>`.
  - If neither is present, `x-api-key: <zai_key>` is used.
- Responses are streamed back to the client without parsing SSE.

Networking:
- Respects the global upstream proxy config (`proxy.upstream_proxy`) for outbound HTTP calls.

## MCP reverse proxy (Search + Reader)
Handlers: `src-tauri/src/proxy/handlers/mcp.rs`
Routes: `src-tauri/src/proxy/server.rs`

Local endpoints:
- `/mcp/web_search_prime/mcp` → `https://api.z.ai/api/mcp/web_search_prime/mcp`
- `/mcp/web_reader/mcp` → `https://api.z.ai/api/mcp/web_reader/mcp`

Behavior:
- Controlled by `proxy.zai.mcp.*` flags:
  - If `mcp.enabled=false` -> endpoints return 404.
  - If per-server flag is false -> returns 404 for that endpoint.
- z.ai key is injected upstream as `Authorization: Bearer <zai_key>`.
- Response body is streamed back to the client.

Note:
- These endpoints are still subject to the proxy’s auth middleware depending on `proxy.auth_mode`.

## UI
Page: `src/pages/ApiProxy.tsx`

Added controls:
- Authorization toggle + mode selector (`off/strict/all_except_health/auto`)
- z.ai block:
  - enable toggle
  - base_url
  - dispatch mode
  - api key input (stored locally)
  - MCP toggles + display of local MCP endpoints

Translations:
- `src/locales/en.json`
- `src/locales/zh.json`

## Validation checklist
Build:
- Frontend: `npm run build`
- Backend: `cd src-tauri && cargo build`

Manual (example):
1) Enable proxy auth (strict or all-except-health) and note `proxy.api_key`.
2) Enable z.ai and set:
   - `dispatch_mode=exclusive`
   - `api_key=<your_z.ai.key>`
3) Start proxy and call:
   - `GET http://127.0.0.1:<port>/healthz` (should work without auth in all-except-health; always works in off)
   - `POST http://127.0.0.1:<port>/v1/messages` with `Authorization: Bearer <proxy.api_key>` and a normal Anthropic request body.
4) Enable MCP Search and call local `/mcp/web_search_prime/mcp` via an MCP client (the proxy injects z.ai auth upstream).

## Known limitations / follow-ups
- Vision MCP bridge is not implemented yet (only a UI placeholder toggle).
- z.ai usage/budget (monitor endpoints) is not implemented yet.
- Claude model list endpoint remains a static stub (`/v1/models/claude`) and is not yet provider-aware.

