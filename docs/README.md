# Documentation index

This folder contains user-facing documentation (how to use the app) plus feature/behavior references for the proxy.

中文索引：
- [`docs/README.zh.md`](README.zh.md)

## User guide
- [`docs/user/README.md`](user/README.md) — how to use the app and where to find features in the UI.

## Proxy
- [`docs/proxy/auth.md`](proxy/auth.md) — proxy authorization modes, expected client behavior, and implementation pointers.
- [`docs/proxy/accounts.md`](proxy/accounts.md) — account lifecycle in the proxy pool (including auto-disable on `invalid_grant`) and UI behavior.
- [`docs/proxy/config.md`](proxy/config.md) — persisted configuration keys (`gui_config.json`) and what they control.
- [`docs/proxy/routing.md`](proxy/routing.md) — protocol surfaces, routing rules, and multi-client behavior (OpenAI / Claude / Gemini / MCP).
- [`docs/proxy/logging.md`](proxy/logging.md) — request access logging (safe-by-default, no secrets).

## App troubleshooting
- [`docs/app/frontend-logging.md`](app/frontend-logging.md) — UI error capture to help debug “white screen” failures.

## z.ai (GLM) integration
- [`docs/zai/implementation.md`](zai/implementation.md) — end-to-end “what’s implemented” and how to validate it.
- [`docs/zai/mcp.md`](zai/mcp.md) — MCP endpoints exposed by the proxy (Search / Reader / Vision) and upstream behavior.
- [`docs/zai/provider.md`](zai/provider.md) — Anthropic-compatible passthrough provider details and dispatch modes.
- [`docs/zai/vision-mcp.md`](zai/vision-mcp.md) — built-in Vision MCP server protocol and tool implementations.

## Maintainers
- Internal development task tracking is not published in this repository.
