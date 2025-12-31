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

Implementation:
- Handler: [`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs) (`handle_web_reader`)

### 3) Vision MCP (built-in server)
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
