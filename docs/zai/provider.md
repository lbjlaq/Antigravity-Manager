# z.ai provider (Anthropic-compatible passthrough)

## Idea
Support z.ai (GLM) as an optional upstream for **Anthropic-compatible requests** (`/v1/messages`), without applying any Google/Gemini-specific transformations when z.ai is selected.

This keeps compatibility high (request/response shapes stay Anthropic-like) and avoids coupling z.ai traffic to the Google account pool.

## Result
We added an optional “z.ai provider” that:
- Is configured in proxy settings (`proxy.zai.*`).
- Can be enabled/disabled and used via dispatch modes.
- Forwards `/v1/messages` and `/v1/messages/count_tokens` to a z.ai Anthropic-compatible base URL.
- Streams responses back without parsing SSE.
 - Does not affect Gemini (`/v1beta/*`) or OpenAI (`/v1/*`) protocol routes.

## Configuration
Schema: `src-tauri/src/proxy/config.rs`
- `ZaiConfig` in `src-tauri/src/proxy/config.rs`
- `ZaiDispatchMode` in `src-tauri/src/proxy/config.rs`

Key fields:
- `proxy.zai.enabled`
- `proxy.zai.base_url` (default `https://api.z.ai/api/anthropic`)
- `proxy.zai.api_key`
- `proxy.zai.dispatch_mode`:
  - `off`
  - `exclusive`
  - `pooled`
  - `fallback`
- `proxy.zai.models` default mapping for `claude-*` request models:
  - `opus`, `sonnet`, `haiku`

## Routing logic
Entry point: [`src-tauri/src/proxy/handlers/claude.rs`](../../src-tauri/src/proxy/handlers/claude.rs)
- `handle_messages(...)` decides whether to route the request to z.ai or to the existing Google-backed flow.
- `pooled` mode uses round-robin across `(google_accounts + 1)` slots, where slot `0` is z.ai.

## Upstream implementation
Provider implementation: [`src-tauri/src/proxy/providers/zai_anthropic.rs`](../../src-tauri/src/proxy/providers/zai_anthropic.rs)
- Forwarding is conservative about headers (does not forward the proxy’s own auth key).
- Injects z.ai auth (`Authorization` / `x-api-key`) and forwards the request body as-is.
- Uses the global upstream proxy config when configured.

## Validation
1) Enable z.ai in the UI (`src/pages/ApiProxy.tsx`) and set `dispatch_mode=exclusive`.
   - UI: [`src/pages/ApiProxy.tsx`](../../src/pages/ApiProxy.tsx)
2) Start the proxy.
3) Send a normal Anthropic request to `POST /v1/messages`.
4) Verify the request is served by z.ai (and Google accounts are not involved for this endpoint in exclusive mode).
