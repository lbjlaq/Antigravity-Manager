# Proxy authorization (auth modes)

## What we wanted
- Allow running the proxy **open** for local-only workflows.
- Allow enabling **request authentication** when exposing the proxy more widely (LAN, shared host, etc.).
- Keep behavior predictable for tools that cannot add auth headers by providing a mode that keeps health checks open.
- Apply changes **without restart** (hot reload).

## What we got
The proxy supports `proxy.auth_mode` with four modes:
- `off` — no auth required.
- `strict` — auth required for all routes.
- `all_except_health` — auth required for all routes except `GET /healthz`.
- `auto` — derived policy: if `proxy.allow_lan_access=true` then `all_except_health`, otherwise `off`.

Implementation:
- Config enum and serialization: [`src-tauri/src/proxy/config.rs`](../../src-tauri/src/proxy/config.rs)
  - `ProxyAuthMode` in [`src-tauri/src/proxy/config.rs`](../../src-tauri/src/proxy/config.rs)
- Policy resolver (“effective mode”): [`src-tauri/src/proxy/security.rs`](../../src-tauri/src/proxy/security.rs)
  - `ProxySecurityConfig::from_proxy_config(...)` in [`src-tauri/src/proxy/security.rs`](../../src-tauri/src/proxy/security.rs)
- Request middleware enforcement: [`src-tauri/src/proxy/middleware/auth.rs`](../../src-tauri/src/proxy/middleware/auth.rs)
  - `auth_middleware(...)` validates `Authorization: Bearer <proxy.api_key>`
  - `OPTIONS` requests are allowed (CORS preflight)
  - In `all_except_health`, `GET /healthz` bypasses auth

Hot reload:
- Config save triggers running server updates in [`src-tauri/src/commands/mod.rs`](../../src-tauri/src/commands/mod.rs)
  - `save_config(...)` calls `axum_server.update_security(&config.proxy).await`

## Client contract
When auth is enabled, clients should send:
- `Authorization: Bearer <proxy.api_key>`

Notes:
- The proxy API key is **not** forwarded upstream to providers.
- Health may remain open depending on the selected mode.

## Validation
1) Set `proxy.auth_mode=all_except_health` and `proxy.api_key` in the UI (`src/pages/ApiProxy.tsx`).
   - UI: [`src/pages/ApiProxy.tsx`](../../src/pages/ApiProxy.tsx)
2) Start the proxy.
3) Verify:
   - `GET /healthz` succeeds without auth.
   - Other endpoints (e.g. `POST /v1/messages`) return 401 without auth and succeed with the header.

UI implementation detail:
- The auth “Enabled” switch uses a `button[role="switch"]` control (`src/components/common/Switch.tsx`) to keep toggling reliable in embedded WebViews.
