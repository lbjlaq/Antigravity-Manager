# Proxy request logging (safe access log)

The API proxy supports an optional, safe-by-default access log intended for debugging and operational visibility.

## What it logs
When enabled (`proxy.access_log_enabled=true`), each request emits a single log line containing:
- HTTP method
- path (no query string)
- response status
- latency

It does **not** log:
- request/response bodies
- headers (including `Authorization` / `x-api-key`)
- query strings

## How to enable
In the UI: **API Proxy → Service Configuration → Request Logging**.

Persisted config key:
- `proxy.access_log_enabled` (default: `false`)

## Related feature: response attribution headers
For multi-client setups, you can also enable response attribution headers:
- `proxy.response_attribution_headers` (default: `false`)

This injects redacted `x-antigravity-*` headers in responses so clients (or simple curl tests) can see which provider/model/account served a request without logging bodies.

See: `docs/proxy/routing.md`

