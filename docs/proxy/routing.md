# Proxy routing & protocol surfaces

This proxy is designed to be used by multiple clients at the same time (IDEs, assistants, automation, HTTP callers). Each request is routed by **HTTP path** to a protocol handler, and only some handlers may involve z.ai depending on configuration.

## 1) Protocol surfaces (what the proxy serves)

### Claude protocol (Anthropic-compatible)
- `POST /v1/messages`
- `POST /v1/messages/count_tokens`
- `GET /v1/models/claude` (static list stub)

### Gemini protocol (Google-native)
- `GET /v1beta/models`
- `GET /v1beta/models/:model`
- `POST /v1beta/models/:model` (generate)
- `POST /v1beta/models/:model/countTokens`

### OpenAI protocol (compat)
- `POST /v1/chat/completions`
- `POST /v1/completions`
- `POST /v1/responses` (compat alias for `/v1/completions`)
- `POST /v1/images/generations`
- `POST /v1/images/edits`

### MCP endpoints
- `ANY /mcp/web_search_prime/mcp`
- `ANY /mcp/web_reader/mcp`
- `ANY /mcp/zread/mcp`
- `ANY /mcp/zai-mcp-server/mcp` (built-in Vision MCP)

Route wiring lives in [`src-tauri/src/proxy/server.rs`](../../src-tauri/src/proxy/server.rs).

## 2) Provider selection rules (Google pool vs z.ai)

### 2.1 Claude protocol (`/v1/messages`)
Claude protocol requests may be routed to **either**:
- z.ai Anthropic-compatible upstream (passthrough), or
- the existing Google-backed Claude→Gemini transform pipeline (account pool).

Decision inputs:
- `proxy.zai.enabled`
- `proxy.zai.api_key` present
- `proxy.zai.dispatch_mode`:
  - `off`: always use Google-backed flow
  - `exclusive`: always use z.ai for Claude protocol
  - `pooled`: z.ai participates as **one slot** in round-robin with Google accounts (no priority guarantee)
  - `fallback`: use z.ai for Claude protocol only when the Google pool has 0 available accounts

Implementation:
- Router decision: [`src-tauri/src/proxy/handlers/claude.rs`](../../src-tauri/src/proxy/handlers/claude.rs) (`handle_messages`)
- z.ai upstream client: [`src-tauri/src/proxy/providers/zai_anthropic.rs`](../../src-tauri/src/proxy/providers/zai_anthropic.rs)

### 2.2 Gemini protocol (`/v1beta/*`)
Gemini protocol requests always use the existing Google-backed flow and do not route to z.ai.

Implementation:
- [`src-tauri/src/proxy/handlers/gemini.rs`](../../src-tauri/src/proxy/handlers/gemini.rs)

### 2.3 OpenAI protocol (`/v1/*`)
OpenAI protocol compatibility routes use the existing proxy logic (mappings + Google-backed execution). z.ai dispatch modes do not affect these routes.

Implementation:
- [`src-tauri/src/proxy/handlers/openai.rs`](../../src-tauri/src/proxy/handlers/openai.rs)

## 3) Model mapping rules (where mappings apply)

The proxy supports multiple mapping layers (configured in the API Proxy UI):
- `proxy.anthropic_mapping` — affects Claude protocol requests
- `proxy.openai_mapping` — affects OpenAI protocol requests
- `proxy.custom_mapping` — optional custom map overrides

z.ai-specific model mapping for Claude protocol:
- `proxy.zai.models.{opus,sonnet,haiku}` provide defaults when the incoming request uses `claude-*` model ids.
- `proxy.zai.model_mapping` provides exact-match overrides: if the incoming `model` string matches a key, it is replaced with the mapped z.ai model id.

Important behavior:
- z.ai model mapping is only applied when the request is actually routed to z.ai.
- If the request is routed to the Google-backed flow, the existing Claude→Gemini mapping logic applies as before.

Implementation:
- Config schema: [`src-tauri/src/proxy/config.rs`](../../src-tauri/src/proxy/config.rs)

## 4) MCP routing rules

MCP is controlled by `proxy.zai.mcp.*`:
- If `proxy.zai.mcp.enabled=false` → all `/mcp/*` return 404.
- Each server has its own toggle (`web_search_enabled`, `web_reader_enabled`, `zread_enabled`, `vision_enabled`).

Endpoints:
- Web Search MCP: reverse-proxy to upstream z.ai MCP server
- Web Reader MCP: reverse-proxy to upstream z.ai MCP server (with optional URL normalization for `webReader` tool calls)
- zread MCP: reverse-proxy to upstream zread MCP server
- Vision MCP: built-in local MCP server (no external Node process needed)

Details:
- [`docs/zai/mcp.md`](../zai/mcp.md)

## 5) Security & auth interactions

Proxy auth is global and applies to all routes according to `proxy.auth_mode`:
- `off`: no auth
- `strict`: auth required for all routes
- `all_except_health`: auth required for all routes except `GET /healthz`
- `auto`: derived from `proxy.allow_lan_access`

When auth is enabled, clients must send:
- `Authorization: Bearer <proxy.api_key>`

Notes:
- The proxy API key is never forwarded upstream.
- Access logs never print headers/bodies/queries (to reduce leak risk).

References:
- [`docs/proxy/auth.md`](auth.md)
- [`docs/proxy/logging.md`](logging.md)

## 6) Multi-client behavior (practical cases)

Because routing is path-based, different clients can use different protocols concurrently without interfering:
- One client can use `POST /v1/messages` (Claude protocol) while another uses `POST /v1/chat/completions` (OpenAI protocol) and a third connects to `/mcp/*`.
- Only Claude protocol requests are subject to `proxy.zai.dispatch_mode`.
- If auth is enabled, all clients must attach the proxy auth header (except `GET /healthz` in `all_except_health`).

