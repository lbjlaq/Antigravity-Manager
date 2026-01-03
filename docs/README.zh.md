# 文档索引（中文）

本目录包含面向使用者的文档（如何使用应用）以及代理功能/行为参考。

English index:
- [`docs/README.md`](README.md)

## 用户指南
- [`docs/user/README.zh.md`](user/README.zh.md) — 如何使用应用、在哪里找到各功能、运行时行为说明。

## Proxy
- [`docs/proxy/routing.zh.md`](proxy/routing.zh.md) — 代理提供的协议面与路由规则（OpenAI / Claude / Gemini / MCP），以及多客户端并发使用时的行为。
- [`docs/proxy/config.zh.md`](proxy/config.zh.md) — 持久化配置（`gui_config.json`）的关键字段与其影响。
- [`docs/proxy/auth.zh.md`](proxy/auth.zh.md) — 代理鉴权模式、客户端契约与实现要点。
- [`docs/proxy/accounts.zh.md`](proxy/accounts.zh.md) — Google 账号池生命周期（含 `invalid_grant` 自动禁用）及 UI 行为。
- [`docs/proxy/logging.zh.md`](proxy/logging.zh.md) — 代理请求访问日志（默认不泄露敏感信息）。

## App 排障
- [`docs/app/frontend-logging.zh.md`](app/frontend-logging.zh.md) — UI “白屏”/异常的前端日志采集与定位方式。

## z.ai（GLM）集成
- [`docs/zai/implementation.zh.md`](zai/implementation.zh.md) — 当前已实现的能力全景与验证方式。
- [`docs/zai/mcp.zh.md`](zai/mcp.zh.md) — 代理暴露的 z.ai MCP 端点（Search / Reader / zread / Vision）与行为规则。
- [`docs/zai/provider.zh.md`](zai/provider.zh.md) — Anthropic 兼容 passthrough provider 与分发模式。
- [`docs/zai/vision-mcp.zh.md`](zai/vision-mcp.zh.md) — 内置 Vision MCP 的协议面与工具实现细节。

## 维护者
- 本仓库不公开内部开发任务/排期/迭代跟踪文档。
