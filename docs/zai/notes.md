# z.ai (GLM) integration notes (Anthropic passthrough + MCP + usage)

Goal: integrate z.ai as an upstream provider into Antigravity’s proxy/service, primarily via an Anthropic API-compatible passthrough, and optionally provide “budget/usage” visibility and MCP helpers (search/reader/vision).

This is a working note capturing findings, constraints, and a proposed implementation path. It intentionally does **not** copy full upstream documentation; it extracts what matters for implementation and corner cases.

## 0) Product decisions / requirements (confirmed)
- z.ai is configured and controlled from the **API Proxy** UI as an optional provider (enable/disable).
- z.ai is used **inside Antigravity** (not via Google OAuth), but it must be able to serve **API Proxy** traffic alongside the existing account pool.
- Storage: z.ai config/credentials are stored in the same **data directory** as the existing accounts and GUI config (same folder where Google account JSON lives). No Keychain/Vault.
- Dispatch strategy is user-configurable:
  - **z.ai handles all proxy requests** (exclusive mode), OR
  - **z.ai participates in the shared rotation/queue** with other accounts and only gets requests when it is selected (pooled mode), OR
  - (optional) **fallback-only** (only when the rest of the pool is unavailable).
- z.ai MCP servers should be provided via Antigravity’s proxy as optional toggles (enable/disable) and be usable by apps **without requiring users to configure z.ai keys** in the apps.
- Proxy authorization (if enabled) applies to the **entire proxy** (no per-route bypass).

## 1) Key docs / entry points
- Anthropic-compatible endpoint (Coding plan usage with existing clients): `https://docs.z.ai/devpack/tool/claude.md`
- Scenario example (same content, shorter): `https://docs.z.ai/scenario-example/develop-tools/claude.md`
- Vision MCP (local stdio, Node): `https://docs.z.ai/devpack/mcp/vision-mcp-server.md`
- Web Search MCP (remote HTTP/SSE): `https://docs.z.ai/devpack/mcp/search-mcp-server.md`
- Web Reader MCP (remote HTTP/SSE): `https://docs.z.ai/devpack/mcp/reader-mcp-server.md`
- API reference intro (general vs coding endpoint): `https://docs.z.ai/api-reference/introduction.md`
- Chat completions (OpenAI-like): `https://docs.z.ai/api-reference/llm/chat-completion.md`
- Usage query plugin (reveals monitor endpoints + auth quirks): `https://docs.z.ai/devpack/extension/usage-query-plugin.md`

## Implementation status
Developer-facing implementation details for what is already built live in:
- [`docs/zai/implementation.md`](implementation.md)
- [`docs/zai/mcp.md`](mcp.md)
- [`docs/zai/provider.md`](provider.md)
- [`docs/zai/vision-mcp.md`](vision-mcp.md)

## 2) What z.ai provides (relevant to our integration)
### 2.1 Anthropic-compatible upstream (what we’ll passthrough to)
Docs show clients can be configured with:
- `ANTHROPIC_BASE_URL = https://api.z.ai/api/anthropic`
- `ANTHROPIC_AUTH_TOKEN = <Z.AI API key>`

This implies z.ai runs an Anthropic-compatible API surface behind that base URL.

Practical implication for Antigravity:
- Add a new upstream provider “z.ai Anthropic” and forward `/v1/*` to `https://api.z.ai/api/anthropic/v1/*` (exact path joining must be verified via test calls).

### 2.2 Model mapping defaults (client-side)
Docs mention default mapping for “internal model env vars” to GLM:
- `ANTHROPIC_DEFAULT_OPUS_MODEL` → `glm-4.7`
- `ANTHROPIC_DEFAULT_SONNET_MODEL` → `glm-4.7`
- `ANTHROPIC_DEFAULT_HAIKU_MODEL` → `glm-4.5-air`

Implication:
- If a client requests “Claude” model names, z.ai may already translate them to GLM on their side OR the client may send GLM model names directly (depending on the client’s model mapping config).
- For Antigravity, the simplest first step is “treat `glm-*` as z.ai” (or require explicit `zai:` prefix), and forward model strings unchanged.

### 2.3 OpenAI-like API (optional later)
z.ai also provides OpenAI-style chat completions under:
- `POST https://api.z.ai/api/paas/v4/chat/completions`
and a dedicated “coding endpoint”:
- `https://api.z.ai/api/coding/paas/v4` (doc note: use this for coding plan scenarios)

We can defer this for phase 2 if we want to stay strictly Anthropic passthrough.

## 3) MCP ecosystem (the “3 servers”)
Important: MCP is not part of the Anthropic `/v1/messages` request itself. MCP is configured by the client (or we expose local endpoints that behave like MCP servers).

### 3.1 Vision MCP server (local stdio process)
Doc highlights:
- NPM package: `@z_ai/mcp-server`
- Requires Node.js `>= 22`
- Uses env vars:
  - `Z_AI_API_KEY` (required)
  - `Z_AI_MODE=ZAI`
- Installed as a local stdio MCP server via an MCP-compatible client.

Implication:
- This is a local process that clients spawn. Instead of requiring an extra runtime, the proxy now exposes a built-in Vision MCP endpoint (see `docs/zai/vision-mcp.md`), while still keeping compatibility with upstream behavior.

### 3.2 Web Search MCP server (remote)
Endpoints:
- MCP over HTTP (recommended): `https://api.z.ai/api/mcp/web_search_prime/mcp`
  - Header auth: `Authorization: Bearer <api_key>`
- MCP over SSE (legacy/alternative): `https://api.z.ai/api/mcp/web_search_prime/sse?Authorization=<api_key>`

Quota note in docs (plan-dependent):
- Lite/Pro/Max include a number of web searches/readers and vision resource pool.

### 3.3 Web Reader MCP server (remote)
Endpoints:
- MCP over HTTP (recommended): `https://api.z.ai/api/mcp/web_reader/mcp`
  - Header auth: `Authorization: Bearer <api_key>`
- MCP over SSE (alternative): `https://api.z.ai/api/mcp/web_reader/sse?Authorization=<api_key>`

### 3.4 MCP-specific corner cases
- Query-string auth for SSE is high risk (easy leakage into logs/history/screenshots). Prefer header-based auth.
- If we proxy MCP endpoints locally, we should only expose header-based auth to the upstream and never include secrets in URLs.
- Remote MCP endpoints may use “streamable-http” semantics; we should avoid buffering and proxy as a streaming response.

## 4) Usage / budget integration (tokens + MCP quotas)
There are “monitor/usage” endpoints used by z.ai’s usage query tooling.
The reference script from `zai-org/zai-coding-plugins` uses:
- `GET /api/monitor/usage/model-usage?startTime=...&endTime=...`
- `GET /api/monitor/usage/tool-usage?startTime=...&endTime=...`
- `GET /api/monitor/usage/quota/limit`
based on `ANTHROPIC_BASE_URL` domain (if it contains `api.z.ai`).

Auth quirk:
- The script sets `Authorization: <token>` (raw token) for these monitor endpoints (no `Bearer`).
- Remote MCP endpoints use `Authorization: Bearer <token>`.
- For z.ai’s general API reference (Bearer auth), it’s also `Authorization: Bearer <ZAI_API_KEY>`.

Implication:
- We must treat these as separate integration surfaces:
  - Anthropic-compatible upstream auth format (to be validated)
  - Monitor endpoints auth format (raw token per script)
  - MCP remote auth format (Bearer)

## 5) Our current proxy architecture constraints
Today the proxy’s “upstream” client is hardwired to Google `v1internal` and uses:
- token pool (per-account OAuth tokens)
- `project_id` resolution
- retry/rotation logic

For z.ai passthrough we should bypass all Google-specific logic:
- z.ai uses a single API key (or a key pool later), not OAuth refresh tokens.
- project_id is not relevant.

Therefore phase 1 should introduce a provider-level router that can pick:
- `provider=google` (existing flow)
- `provider=zai` (passthrough flow)

## 6) Proposed implementation approach (phase 1 “minimal but real”)
### 6.1 New provider: z.ai Anthropic passthrough
- Add config fields:
  - `zai.enabled`
  - `zai.api_key` (stored securely; never logged)
  - `zai.base_url` (default `https://api.z.ai/api/anthropic`)
  - optional: `zai.request_timeout_ms`
- Routing:
  - If resolved model starts with `glm-` OR mapping returns `zai:<model>` → use z.ai provider.
  - Keep existing mappings intact; add support for values with a provider prefix (e.g. `zai:glm-4.7`).
- Endpoint handling:
  - Forward `POST /v1/messages` and other `/v1/*` requests by path passthrough.
  - Streaming: proxy bytes end-to-end (do not parse/reshape SSE in phase 1).
  - Error mapping: preserve upstream status/body; only wrap errors if needed for compatibility with current clients.

### 6.2 MCP (remote) local reverse-proxy (phase 1.5)
Provide local endpoints so clients do not store the z.ai key:
- `GET/POST /mcp/web_search_prime/mcp` → upstream `https://api.z.ai/api/mcp/web_search_prime/mcp`
- `GET/POST /mcp/web_reader/mcp` → upstream `https://api.z.ai/api/mcp/web_reader/mcp`

Behavior:
- Require Antigravity’s local proxy auth (existing `api_key`) for access.
- Inject upstream header: `Authorization: Bearer <zai_api_key>`.
- Stream responses.

Explicitly avoid:
- exposing SSE endpoints that require key-in-query.

### 6.3 Vision MCP (local stdio) “setup helper” only (phase 1)
Status update:
- Vision MCP is implemented directly inside the proxy and exposed at `/mcp/zai-mcp-server/mcp`.
- Implementation details: `docs/zai/vision-mcp.md`.

## 7) Corner cases checklist (must handle)
- Auth header rewriting:
  - never forward client-provided secrets upstream
  - never log `Authorization`, `x-api-key`, cookies, tokens
- Timeout differences:
  - docs show `API_TIMEOUT_MS` set very high in some configs; we should allow per-provider timeout config
- Streaming cancellation:
  - client disconnect should abort upstream request
- 401/429:
  - surface meaningful error and do not retry blindly
- Model naming:
  - support both `glm-*` and “Claude-like” names if needed (either client-side mapping or server-side mapping)
- Mixed mode:
  - if z.ai disabled or misconfigured, either fail fast or fallback to google (config-driven)
- Monitor endpoints:
  - auth format differences (raw vs Bearer); cache results; avoid rate limit
- Security:
  - never accept key in querystring for local endpoints
  - if we provide “test connection”, ensure it does not leak the key in logs

## 8) Open questions to settle before coding
1) Which surface is the priority: Anthropic passthrough only, or also OpenAI-like `chat/completions`?
2) Do we want multi-key support for z.ai (rotation) or a single key per installation initially?
3) Fallback policy when z.ai quota is near/at limit: error vs automatic fallback to other provider.
4) Should the UI expose MCP helper endpoints/config snippets for clients?
