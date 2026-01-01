# Proxy configuration (persisted)

This page summarizes the configuration keys used by the proxy and how they affect runtime behavior. Settings are edited via the UI and stored in the app data directory.

## Where config lives

Primary file:
- `~/.antigravity_tools/gui_config.json`

Google account pool files:
- `~/.antigravity_tools/accounts/*.json`

Notes:
- Credentials are stored locally on disk by design.
- Do not commit or share these files.

## Top-level proxy keys (`proxy.*`)

### Network & binding
- `proxy.enabled` — starts/stops the proxy server.
- `proxy.port` — listening port (default is UI-driven).
- `proxy.allow_lan_access` — if enabled, binds to LAN-accessible host (and affects `auth_mode=auto`).
- `proxy.request_timeout` — outbound request timeout (seconds).
- `proxy.upstream_proxy` — optional outbound HTTP proxy for all upstream calls (Google + z.ai + remote MCP).

### Authorization (global)
- `proxy.auth_mode` — `off | strict | all_except_health | auto`
- `proxy.api_key` — required when auth is enabled

Behavior details:
- [`docs/proxy/auth.md`](auth.md)

### Access logging (safe-by-default)
- `proxy.access_log_enabled` — logs method/path/status/latency only (no secrets)

Behavior details:
- [`docs/proxy/logging.md`](logging.md)

### Protocol mappings
These mappings influence how the proxy translates incoming model names (and related routing decisions) for non-z.ai flows:
- `proxy.anthropic_mapping`
- `proxy.openai_mapping`
- `proxy.custom_mapping`

Routing overview:
- [`docs/proxy/routing.md`](routing.md)

## z.ai configuration (`proxy.zai.*`)

### Provider (Claude protocol only)
- `proxy.zai.enabled`
- `proxy.zai.base_url` (default `https://api.z.ai/api/anthropic`)
- `proxy.zai.api_key`
- `proxy.zai.dispatch_mode` — `off | exclusive | pooled | fallback`
- `proxy.zai.models.opus|sonnet|haiku` — defaults for `claude-*` model names when routed to z.ai
- `proxy.zai.model_mapping` — exact match overrides for `model` strings when routed to z.ai

Details:
- [`docs/zai/provider.md`](../zai/provider.md)
- [`docs/zai/implementation.md`](../zai/implementation.md)

### MCP exposure & knobs
MCP can be enabled independently:
- `proxy.zai.mcp.enabled`
- `proxy.zai.mcp.web_search_enabled`
- `proxy.zai.mcp.web_reader_enabled`
- `proxy.zai.mcp.zread_enabled`
- `proxy.zai.mcp.vision_enabled`

Optional per-MCP upstream key:
- `proxy.zai.mcp.api_key_override` (when set, overrides `proxy.zai.api_key` for remote MCP upstream calls)

Optional Web Reader URL normalization:
- `proxy.zai.mcp.web_reader_url_normalization` — `off | strip_tracking_query | strip_query`

Details:
- [`docs/zai/mcp.md`](../zai/mcp.md)

