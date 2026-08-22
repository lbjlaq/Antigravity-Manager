# OrcaRouter provider (Anthropic-compatible gateway)

## Idea
Support [OrcaRouter](https://www.orcarouter.ai) as an optional upstream for **Anthropic-compatible requests** (`/v1/messages`), without applying any Google/Gemini-specific transformations when OrcaRouter is selected.

This keeps compatibility high (request/response shapes stay Anthropic-like) and avoids coupling OrcaRouter traffic to the Google account pool.

OrcaRouter is an OpenAI/Anthropic-compatible gateway. It also runs gateway-level, zero-trust security for AI agents on the same endpoint — screening every prompt/response and governing every tool call on a default-deny basis, with no application code changes.

## Result
We added an optional "OrcaRouter provider" that:
- Is configured in proxy settings (`proxy.orcarouter.*`).
- Can be enabled/disabled and used via dispatch modes.
- Forwards `/v1/messages` and `/v1/messages/count_tokens` to the OrcaRouter Anthropic-compatible base URL (default `https://api.orcarouter.ai`).
- Streams responses back without parsing SSE.

## Configuration
Schema: `src-tauri/src/proxy/config.rs`
- `OrcaRouterConfig` in `src-tauri/src/proxy/config.rs`
- `OrcaRouterDispatchMode` in `src-tauri/src/proxy/config.rs`

Key fields:
- `proxy.orcarouter.enabled`
- `proxy.orcarouter.base_url` (default `https://api.orcarouter.ai`)
- `proxy.orcarouter.api_key`
- `proxy.orcarouter.dispatch_mode`:
  - `off`
  - `exclusive`
  - `pooled`
  - `fallback`
- `proxy.orcarouter.models` default mapping for `claude-*` request models:
  - `opus` (default `anthropic/claude-opus-4.8`)
  - `sonnet` (default `anthropic/claude-sonnet-5`)
  - `haiku` (default `anthropic/claude-haiku-4.5`)
- `proxy.orcarouter.model_mapping`: optional exact-match overrides (`{ "<incoming_model>": "<orcarouter-model-id>" }`)

### Dispatch modes
- `off`: never use OrcaRouter.
- `exclusive`: all Anthropic protocol requests go to OrcaRouter.
- `pooled`: OrcaRouter is treated as **one additional slot** in the shared pool (no priority, no strict guarantee).
- `fallback`: OrcaRouter is used only when the Google pool has 0 accounts or all accounts are unavailable.

## Request routing
In `src-tauri/src/proxy/handlers/claude.rs`, when OrcaRouter is enabled and its dispatch mode selects it, the request is forwarded via `src-tauri/src/proxy/providers/orcarouter.rs` (`forward_anthropic_json`) to:
- `{base_url}/v1/messages`
- `{base_url}/v1/messages/count_tokens`

The incoming model name is mapped through the configured model mapping / family defaults before forwarding (see `map_model_for_orcarouter`).

## Runtime hot update
`save_config` hot-updates `orcarouter` without restart (`AxumServer::update_orcarouter`).

## Related
- [`docs/zai/provider.md`](../zai/provider.md) — the z.ai provider this mirrors
