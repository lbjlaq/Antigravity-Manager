# z.ai MCP endpoints via local proxy

## What we wanted
- Allow apps to use z.ai MCP servers **without configuring z.ai keys** in those apps.
- Keep secrets out of URLs (avoid query-string auth).
- Make each MCP capability toggleable.

## What we got
When `proxy.zai.mcp.enabled=true`, the proxy can expose MCP endpoints under its own base URL.

### 1) Web Search (remote reverse-proxy)
Local endpoint:
- `/mcp/web_search_prime/mcp`

Upstream:
- `https://api.z.ai/api/mcp/web_search_prime/mcp`

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
- Behavior:
  - Applies only to JSON-RPC `tools/call` where `params.name == "webReader"` and `params.arguments.url` is an `http(s)` URL.
  - Other MCP methods/tools/endpoints are not modified.
  - This exists to work around upstream quirks observed with long/tracking query strings (sometimes rejected as “URL format”).

Implementation:
- Handler: [`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs) (`handle_web_reader`)

### 3) zread (remote reverse-proxy)
Local endpoint:
- `/mcp/zread/mcp`

Upstream:
- `https://api.z.ai/api/mcp/zread/mcp`

Notes:
- This MCP server exposes repository/document reading tools (e.g. `search_doc`, `read_file`, `get_repo_structure`). It is not the same as the Web Reader server.

Implementation:
- Handler: [`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs) (`handle_zread`)

### 4) Vision MCP (built-in server)
Local endpoint:
- `/mcp/zai-mcp-server/mcp`

Implementation:
- Route wiring: [`src-tauri/src/proxy/server.rs`](../../src-tauri/src/proxy/server.rs)
- Handler: [`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs) (`handle_zai_mcp_server`)
- Session state: [`src-tauri/src/proxy/zai_vision_mcp.rs`](../../src-tauri/src/proxy/zai_vision_mcp.rs)
- Tool execution: [`src-tauri/src/proxy/zai_vision_tools.rs`](../../src-tauri/src/proxy/zai_vision_tools.rs)

## Auth model
- Local proxy auth (if enabled) is handled by the proxy middleware:
  - [`src-tauri/src/proxy/middleware/auth.rs`](../../src-tauri/src/proxy/middleware/auth.rs)
- z.ai auth is always injected upstream by the proxy using `proxy.zai.api_key`.
- No z.ai key needs to be configured in MCP clients that point at the local endpoints.

## UI wiring
The MCP toggles and local endpoints are shown in:
- [`src/pages/ApiProxy.tsx`](../../src/pages/ApiProxy.tsx)

## Validation
1) Enable `proxy.zai.enabled=true` and set `proxy.zai.api_key`.
2) Enable:
   - `proxy.zai.mcp.enabled=true`
   - any subset of `{web_search_enabled, web_reader_enabled, vision_enabled}`
3) Start the proxy and point an MCP client at the corresponding local endpoint(s).

For a repeatable smoke-test procedure and recorded outcomes, see:
- [`docs/zai/mcp-testing.md`](mcp-testing.md)

## Troubleshooting notes (observed)
- **Upstream expects Streamable HTTP semantics**:
  - For the remote z.ai MCP endpoints, upstream responses are typically `text/event-stream` (SSE).
  - Some upstreams require the request `Accept` header to include both `application/json` and `text/event-stream`.
- **Web Reader URL quirks**:
  - Some URLs with percent-encoded characters in the query string (e.g. `%20`) may be rejected by upstream as “URL format” errors.
  - Some sites may fail to fetch/convert upstream and return “missing data” errors (site-specific; can be due to bot protection, redirects, or dynamic rendering).
- **Search entitlements**:
  - `webSearchPrime` may return auth/entitlement errors depending on the upstream account/plan.
- **Vision MCP quota**:
  - Vision tool calls can return quota/balance errors (HTTP 429) depending on the upstream account balance.
