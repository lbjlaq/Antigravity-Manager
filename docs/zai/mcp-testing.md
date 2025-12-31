# z.ai MCP smoke testing (via local proxy)

This document captures a repeatable smoke-test procedure and observed outcomes for the z.ai MCP endpoints exposed by the local API proxy.

## Preconditions
- Local proxy is running (default: `http://127.0.0.1:8045`).
- Proxy authorization may be enabled (example: `auth_mode=all_except_health`), so `/mcp/*` requires an API key.
- z.ai MCP toggles are enabled in the UI:
  - `proxy.zai.enabled=true`
  - `proxy.zai.mcp.enabled=true`
  - plus any subset of `{web_search_enabled, web_reader_enabled, zread_enabled, vision_enabled}`

## Request format notes
- Remote z.ai MCP upstreams respond as `text/event-stream` (SSE) even for single JSON-RPC requests.
- The local proxy forces `Accept` upstream to include both `application/json` and `text/event-stream` to stay compatible with strict upstream behavior.
- The local Vision MCP server uses an `mcp-session-id` header: call `initialize` first, then include that header in subsequent requests.

## cURL snippets (placeholders)
Use a placeholder key to avoid leaking secrets:
- `Authorization: Bearer <PROXY_API_KEY>`

All requests below use Streamable HTTP with JSON-RPC payloads:
- `initialize`
- `tools/list`
- `tools/call`

## Results (last run: 2025-12-31)
Configuration observed during the run (no secrets):
- `proxy.port=8045`
- `proxy.auth_mode=all_except_health`
- `proxy.zai.mcp.web_reader_url_normalization=strip_tracking_query`

### 1) Web Search MCP (`/mcp/web_search_prime/mcp`)
- `initialize`: OK (SSE)
- `tools/list`: OK → tool `webSearchPrime`
- `tools/call` with `search_query`: OK → returns search results (as text content)

Notes:
- Tool schema requires `search_query` (not `query`).

### 2) Web Reader MCP (`/mcp/web_reader/mcp`)
- `initialize`: OK (SSE)
- `tools/list`: OK → tool `webReader`
- `tools/call` (`url=https://example.com`, `https://httpbin.org/html`, `https://www.wikipedia.org/`): returns
  - `MCP error -500: Reader response missing data`

Notes:
- The response is a successful JSON-RPC result containing an error-like string in `result.content[0].text` (not a JSON-RPC `error` object).
- This appears upstream-side (site-specific behavior, bot protection, or account/feature entitlement can affect it).
- The URL normalization setting is meant to address “URL format” rejections for long tracking query strings; it does not fix upstream “missing data”.

### 3) zread MCP (`/mcp/zread/mcp`)
- `initialize`: OK (SSE)
- `tools/list`: OK → tools `search_doc`, `read_file`, `get_repo_structure`
- `tools/call` (`get_repo_structure`, `repo_name=salacoste/Antigravity-Manager`): returns
  - `MCP error -500: Unexpected system error occurred … logId: …`

Notes:
- Tool schema uses `repo_name` in `owner/repo` form (not a URL).
- The error looks like an upstream transient/system failure (retry later).

### 4) Vision MCP (`/mcp/zai-mcp-server/mcp`)
- `initialize`: OK → returns `mcp-session-id` header
- `tools/list`: OK when `mcp-session-id` header is provided
- `tools/call` (`analyze_image`): returned `HTTP 429` with “Insufficient balance…” (upstream balance/quota)

Notes:
- If `tools/list` returns `400 missing Mcp-Session-Id`, call `initialize` again and pass the returned session header.

## Suggested regression checklist
- Verify each endpoint returns `401` without proxy auth headers when auth is enabled.
- Confirm `tools/list` always works:
  - Web Search: `webSearchPrime`
  - Web Reader: `webReader`
  - zread: `search_doc`, `read_file`, `get_repo_structure`
  - Vision: a non-empty tool list (requires `mcp-session-id`)
- For functional checks, use:
  - Web Search: a simple `search_query` and ensure results are returned.
  - Web Reader: try a few different sites; treat “missing data” as an upstream variability signal.
  - zread: retry on transient errors; validate schema keys.
  - Vision: expect balance/quota constraints depending on the upstream account.

