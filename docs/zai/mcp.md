# z.ai MCP endpoints via local proxy

This page documents the user-visible behavior and configuration rules for the z.ai MCP endpoints exposed by the local API proxy.

## Overview
When enabled, the local proxy exposes one or more MCP endpoints under `/mcp/*` and forwards them upstream to z.ai (or serves a built-in MCP server for vision).

Key properties:
- MCP endpoints are **opt-in** via toggles in the API Proxy UI.
- z.ai credentials are stored only in the local app config (`proxy.zai.api_key`) and are **not** required in MCP clients.
- The local proxy enforces the configured proxy authorization policy (if enabled).

## How to enable
1) Configure z.ai:
   - `proxy.zai.enabled=true`
   - `proxy.zai.api_key` is set
2) Enable MCP exposure:
   - `proxy.zai.mcp.enabled=true`
3) Enable any subset of MCP servers:
   - `proxy.zai.mcp.web_search_enabled`
   - `proxy.zai.mcp.web_reader_enabled`
   - `proxy.zai.mcp.zread_enabled`
   - `proxy.zai.mcp.vision_enabled`

Routing rule:
- If `proxy.zai.mcp.enabled=false`, all `/mcp/*` routes return 404 (even if individual MCP toggles are on).

### 1) Web Search (remote reverse-proxy)
Local endpoint:
- `/mcp/web_search_prime/mcp`

Upstream:
- `https://api.z.ai/api/mcp/web_search_prime/mcp`

Behavior:
- The proxy forwards Streamable HTTP JSON-RPC calls to the upstream.
- The proxy injects upstream auth using `proxy.zai.api_key` (clients do not provide a z.ai key).

Implementation:
- Handler: [`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs) (`handle_web_search_prime`)

### 2) Web Reader (remote reverse-proxy)
Local endpoint:
- `/mcp/web_reader/mcp`

Upstream:
- `https://api.z.ai/api/mcp/web_reader/mcp`

Optional URL normalization:
- Config: `proxy.zai.mcp.web_reader_url_normalization`
  - `off` (default): forward URL as-is
  - `strip_tracking_query`: removes common tracking params (e.g. `utm_*`, `hsa_*`, `gclid`, `fbclid`, `gbraid`, `wbraid`, `msclkid`)
  - `strip_query`: removes the entire query string (`?…`)
Behavior:
- Applies only to JSON-RPC `tools/call` where `params.name == "webReader"` and `params.arguments.url` is an `http(s)` URL.
- Other MCP methods/tools/endpoints are not modified.
- Intended to improve compatibility with URLs that include long tracking query strings.

Implementation:
- Handler: [`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs) (`handle_web_reader`)

### 3) zread (remote reverse-proxy)
Local endpoint:
- `/mcp/zread/mcp`

Upstream:
- `https://api.z.ai/api/mcp/zread/mcp`

Notes:
- This MCP server exposes repository/document reading tools (e.g. `search_doc`, `read_file`, `get_repo_structure`). It is not the same as the Web Reader server.

Behavior:
- The proxy forwards Streamable HTTP JSON-RPC calls to the upstream.
- Tool inputs follow upstream schemas (e.g. `repo_name` is typically `owner/repo`).

Implementation:
- Handler: [`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs) (`handle_zread`)

### 4) Vision MCP (built-in server)
Local endpoint:
- `/mcp/zai-mcp-server/mcp`

Behavior:
- This is a local MCP server hosted by the proxy (not a reverse-proxy to a remote MCP endpoint).
- Session is required:
  - Call `initialize` first.
  - Read the `mcp-session-id` response header.
  - Include `mcp-session-id` in subsequent requests (e.g. `tools/list`, `tools/call`).

Implementation:
- Route wiring: [`src-tauri/src/proxy/server.rs`](../../src-tauri/src/proxy/server.rs)
- Handler: [`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs) (`handle_zai_mcp_server`)
- Session state: [`src-tauri/src/proxy/zai_vision_mcp.rs`](../../src-tauri/src/proxy/zai_vision_mcp.rs)
- Tool execution: [`src-tauri/src/proxy/zai_vision_tools.rs`](../../src-tauri/src/proxy/zai_vision_tools.rs)

## Authorization model
- Local proxy authorization (if enabled) applies to `/mcp/*` like any other proxy route:
  - Middleware: [`src-tauri/src/proxy/middleware/auth.rs`](../../src-tauri/src/proxy/middleware/auth.rs)
- z.ai upstream authorization is always injected by the proxy using `proxy.zai.api_key`.
- MCP clients should only authenticate to the local proxy (if proxy auth is enabled); they should not embed any z.ai key.

## Streaming / content-type specifics
- Remote z.ai MCP endpoints commonly respond as `text/event-stream` (SSE) for Streamable HTTP.
- The proxy sets the upstream `Accept` header to include both `application/json` and `text/event-stream` for compatibility with strict upstream behavior.

## UI wiring
The MCP toggles and local endpoints are shown in:
- [`src/pages/ApiProxy.tsx`](../../src/pages/ApiProxy.tsx)

## Limitations and expectations
- Web Reader behavior is site-dependent (bot protection, redirects, dynamic rendering) and upstream may fail to extract content for some URLs.
- Web Search / zread / vision can be subject to upstream plan/entitlement/quota limits and may return 4xx/5xx depending on the upstream account state.
