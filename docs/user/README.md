# User Guide

This guide explains what the app provides, where to find each feature in the UI, and how the proxy behaves at runtime.
If you are maintaining the codebase, start from `docs/README.md` and the feature references under `docs/proxy/*` and `docs/zai/*`.

## What this app is

The app runs a local **API Proxy** that exposes multiple “protocol surfaces” so different tools can use the same proxy:
- **Claude protocol** (Anthropic-compatible): `POST /v1/messages`, `POST /v1/messages/count_tokens`
- **OpenAI protocol** (compat): `POST /v1/chat/completions`, `POST /v1/completions`, `POST /v1/responses`, image endpoints
- **Gemini protocol** (Google-native): `/v1beta/*`
- Optional **MCP** endpoints under `/mcp/*`

The proxy can serve requests from:
- a **Google account pool** (rotates accounts, refreshes tokens, tracks quota), and/or
- an optional **z.ai (GLM) provider** for Claude-protocol requests only (passthrough).

## Where settings live (on disk)

The app stores configuration locally:
- Main config: `~/.antigravity_tools/gui_config.json`
- Google accounts: `~/.antigravity_tools/accounts/*.json`

Do not commit or share these files.

## UI: API Proxy page (main screen)

Open the **API Proxy** screen to control:

### 1) Start/Stop + status
- Start/stop the proxy service.
- View the base URL (typically `http://127.0.0.1:<port>`), port, and whether the service is running.

### 2) Authorization (global)
Location: **Service Configuration → Authorization**

When authorization is enabled, clients must send:
- `Authorization: Bearer <proxy.api_key>` (preferred), or
- `x-api-key: <proxy.api_key>` (fallback).

Auth modes:
- `off` — open proxy (no auth).
- `strict` — auth required for all routes.
- `all_except_health` — auth required for all routes **except** `GET /healthz` and `GET /health`.
- `auto` — recommended: `off` for localhost-only, `all_except_health` when LAN access is enabled.

Details: `docs/proxy/auth.md`

### 3) Request logging (safe access log)
Location: **Service Configuration → Request Logging**

This is a safe access log:
- logs method/path/status/latency only
- does not log query strings, headers, or bodies

Details: `docs/proxy/logging.md`

### 4) Response attribution headers (optional)
Location: **Service Configuration → Response Attribution Headers**

When enabled, the proxy adds redacted response headers:
- `x-antigravity-provider`: `google` or `zai`
- `x-antigravity-model`: resolved upstream model id (best effort)
- `x-antigravity-account`: anonymized account id (ASCII-safe, e.g. `abcd...wxyz`) for Google pool requests

This is useful when you have multiple tools calling the proxy and want to see which provider/model served a request.

Details: `docs/proxy/routing.md`

### 5) Diagnostics endpoints
The proxy exposes lightweight endpoints:
- `GET /healthz` (canonical)
- `GET /health` (alias)
- `GET /test-connection` (checks if the Google pool can select an account; response is redacted)

Notes:
- Auth still applies based on the selected auth mode.
- In `all_except_health`, `/healthz` and `/health` stay open, but `/test-connection` requires auth.

### 6) “Now serving” / “Recent usage”
Location: **API Proxy → Runtime / Recent usage panel**

Shows recent request attribution without enabling payload logging:
- provider (`google` / `zai`)
- resolved model
- account id / masked email (Google pool)

### 7) Model Router (mappings)
Location: **Model Router**

The proxy supports multiple mapping layers:
- **Series mappings** (group keys) for Claude/OpenAI families
- **Family/tier mapping** for Claude (`claude-opus-family`, `claude-sonnet-family`, `claude-haiku-family`)
- **Custom exact mappings** (highest priority)

Mapping affects routing for Google-backed flows. z.ai has its own mapping section when z.ai is enabled.

#### How the Series Groups interact (important)

The 8 “Series Groups” blocks are not all the same kind of rule:
- **Claude groups** write keys into `proxy.anthropic_mapping` (Claude protocol → Google pool path).
- **OpenAI groups** write keys into `proxy.openai_mapping` (OpenAI-compat routes → Google pool path).

They do not conflict across these two mapping tables.

Precedence (what wins if multiple rules could apply):
1) `proxy.custom_mapping` exact match overrides everything.
2) Claude family/tier rules (`claude-*-family`) override Claude series rules (`claude-*.?-series`).
3) Claude series rules apply if no family rule matched (or family is unset).
4) Built-in defaults apply when nothing else matched.

Notes / “gotchas”:
- These groups affect **Google-backed flows** only. If the request is routed to **z.ai passthrough**, z.ai’s mapping rules apply instead.
- The OpenAI group “GPT-4o / 3.5 Series” intentionally includes many `turbo`/`mini` variants; this means some names like `gpt-4-turbo` are treated as part of that group by design.
- If `gpt-5-series` is unset, routing may fall back to the `gpt-4-series` group mapping (if configured).

Details: `docs/proxy/routing.md`

## z.ai (GLM) provider (optional)

Location: **z.ai (GLM) Provider** section on the API Proxy page.

Key points:
- Only affects **Claude protocol** requests (`/v1/messages`, `/v1/messages/count_tokens`).
- Gemini-native and OpenAI-compat routes still use the Google pool.
- Dispatch modes:
  - `off`: never use z.ai
  - `exclusive`: all Claude-protocol requests go to z.ai
  - `pooled`: z.ai participates as one slot alongside Google accounts (no priority)
  - `fallback`: use z.ai only when the Google pool is unavailable

Details:
- `docs/zai/provider.md`
- `docs/zai/implementation.md`

## MCP endpoints (optional)

Location: **z.ai MCP** section on the API Proxy page.

If enabled, the proxy exposes MCP endpoints under `/mcp/*` and can enforce the same proxy auth policy.

Details: `docs/zai/mcp.md`

## Troubleshooting

### “White screen” UI
Use: `docs/app/frontend-logging.md`

### “No available accounts / invalid_grant”
This typically means a Google refresh token is revoked/expired and the proxy auto-disables that account until re-authorized.

Details: `docs/proxy/accounts.md`
