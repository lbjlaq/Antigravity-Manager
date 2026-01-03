# z.ai MCP endpoints via local proxy

This page documents the user-visible behavior and configuration rules for the z.ai MCP endpoints exposed by the local API proxy.

## Overview
When enabled, the local proxy exposes one or more MCP endpoints under `/mcp/*`.

Key properties:
- MCP endpoints are **opt-in** via toggles in the API Proxy UI.
- z.ai credentials are stored only in the local app config (`proxy.zai.api_key`) and are **not** required in MCP clients.
- The local proxy enforces the configured proxy authorization policy (if enabled).

## How to enable
1) Configure z.ai:
   - `proxy.zai.api_key` is set (raw token; a `Bearer ` prefix is also accepted)
   - `proxy.zai.enabled=true` is only required if you want Anthropic `/v1/messages` routed to z.ai as a provider
2) Enable MCP exposure:
   - `proxy.zai.mcp.enabled=true`
3) Enable any subset of MCP servers:
   - `proxy.zai.mcp.web_search_enabled`
   - `proxy.zai.mcp.web_reader_enabled`
   - `proxy.zai.mcp.zread_enabled`
   - `proxy.zai.mcp.vision_enabled`

Routing rule:
- If `proxy.zai.mcp.enabled=false`, all `/mcp/*` routes return 404 (even if individual MCP toggles are on).

## Using with MCP clients (Claude Code)
Claude Code can connect to MCP servers via Streamable HTTP. Configure it to point to the local proxy endpoints so it inherits the proxy’s auth policy and z.ai configuration.

Edit `~/.claude/.claude.json` and add entries under `mcpServers`:

```json
{
  "mcpServers": {
    "zai-web-search": {
      "type": "http",
      "url": "http://127.0.0.1:8045/mcp/web_search_prime/mcp",
      "headers": { "Authorization": "Bearer <PROXY_API_KEY>" }
    },
    "zai-web-reader": {
      "type": "http",
      "url": "http://127.0.0.1:8045/mcp/web_reader/mcp",
      "headers": { "Authorization": "Bearer <PROXY_API_KEY>" }
    },
    "zai-zread": {
      "type": "http",
      "url": "http://127.0.0.1:8045/mcp/zread/mcp",
      "headers": { "Authorization": "Bearer <PROXY_API_KEY>" }
    },
    "zai-vision": {
      "type": "http",
      "url": "http://127.0.0.1:8045/mcp/zai-mcp-server/mcp",
      "headers": { "Authorization": "Bearer <PROXY_API_KEY>" }
    }
  }
}
```

Notes:
- If proxy authorization is disabled (`proxy.auth_mode=off`), omit the `headers` section entirely.
- You can also use `x-api-key` instead of `Authorization` (supported by the proxy).
- Claude Code does not need the z.ai key; the proxy injects upstream auth using the locally stored config.

### 1) Web Search (remote reverse-proxy)
Local endpoint:
- `/mcp/web_search_prime/mcp`

Upstream remote MCP (Streamable HTTP):
- `https://api.z.ai/api/mcp/web_search_prime/mcp`

Behavior:
- This endpoint is a Streamable HTTP reverse-proxy to the upstream z.ai MCP server.
- The proxy injects upstream auth using the stored z.ai key and streams the upstream response back as-is.
- Session is required:
  - Call `initialize` first.
  - Read the `mcp-session-id` response header.
  - Include `mcp-session-id` in subsequent requests (e.g. `tools/list`, `tools/call`).

Implementation:
- Handler: [`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs) (`handle_web_search_prime`)

### 2) Web Reader (remote reverse-proxy)
Local endpoint:
- `/mcp/web_reader/mcp`

Upstream remote MCP (Streamable HTTP):
- `https://api.z.ai/api/mcp/web_reader/mcp`

Optional URL normalization:
- Config: `proxy.zai.mcp.web_reader_url_normalization`
  - `off` (default): keep URL as-is
  - `strip_tracking_query`: removes common tracking params (e.g. `utm_*`, `hsa_*`, `gclid`, `fbclid`, `gbraid`, `wbraid`, `msclkid`)
  - `strip_query`: removes the entire query string (`?…`)
Behavior:
- This endpoint is a Streamable HTTP reverse-proxy to the upstream z.ai MCP server.
- The upstream Web Reader MCP server is strict about URL formats; normalization can improve compatibility for URLs with long tracking query strings.
- Session is required:
  - Call `initialize` first.
  - Read the `mcp-session-id` response header.
  - Include `mcp-session-id` in subsequent requests (e.g. `tools/list`, `tools/call`).
- URL normalization applies only to JSON-RPC `tools/call` where `params.name == "webReader"` and `params.arguments.url` is an `http(s)` URL.

Implementation:
- Handler: [`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs) (`handle_web_reader`)
- URL normalization helper: [`src-tauri/src/proxy/zai_web_tools.rs`](../../src-tauri/src/proxy/zai_web_tools.rs) (`normalize_web_reader_url`)

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
  - `repo_name` must be a public GitHub repository in `owner/repo` format that the upstream service can access.
  - For private repos or repos not indexed/available upstream, tool calls can return `target not found` errors.

Implementation:
- Handler: [`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs) (`handle_zread`)

### 4) Vision MCP (built-in server)
Local endpoint:
- `/mcp/zai-mcp-server/mcp`

Behavior:
- This is a local MCP server hosted by the proxy (not a reverse-proxy to a remote MCP endpoint).
- For GLM Coding Plan users, the proxy prefers the z.ai coding endpoint (`/api/coding/paas/v4`) and falls back to the general endpoint only when the coding endpoint is unavailable for the current key.
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
- z.ai upstream authorization is always injected by the proxy.
- For remote MCP reverse-proxy routes, the proxy injects both `Authorization: Bearer <zai_token>` and `x-api-key: <zai_token>` for upstream compatibility (token is normalized if the user pasted `Bearer ...`).
- Optional: MCP can use a separate key via `proxy.zai.mcp.api_key_override` (when set, it overrides `proxy.zai.api_key` for z.ai MCP features, including remote MCP upstream calls and built-in MCP tools).
- MCP clients should only authenticate to the local proxy (if proxy auth is enabled); they should not embed any z.ai key.

## Streaming / content-type specifics
- Remote z.ai MCP endpoints commonly respond as `text/event-stream` (SSE) for Streamable HTTP.
- When reverse-proxying remote MCP, the proxy sets the upstream `Accept` header to include both `application/json` and `text/event-stream` for compatibility.

## UI wiring
The MCP toggles and local endpoints are shown in:
- [`src/pages/ApiProxy.tsx`](../../src/pages/ApiProxy.tsx)

## Limitations and expectations
- Web Reader behavior is site-dependent (bot protection, redirects, dynamic rendering) and upstream may fail to extract content for some URLs.
- Web Search / zread / vision can be subject to upstream plan/entitlement/quota limits and may return 4xx/5xx depending on the upstream account state.
- Some upstream tool failures are returned as a successful JSON-RPC response whose `result.content[0].text` starts with `MCP error ...` (i.e. not a JSON-RPC `error` object). Clients should treat tool outputs as data and handle such error strings.
