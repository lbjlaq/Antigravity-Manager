# z.ai provider（Anthropic 兼容 passthrough）

## 思路
将 z.ai（GLM）作为 **Anthropic 兼容请求**（`/v1/messages`）的可选上游。在选中 z.ai 时，不应用任何 Google/Gemini 特定的转换逻辑。

这样能最大化兼容性（请求/响应保持 Anthropic-like），同时避免 z.ai 流量与 Google 账号池强耦合。

## 结果
我们新增了一个可选的 “z.ai provider”，特点：
- 在代理设置中配置（`proxy.zai.*`）。
- 可启用/禁用，并支持多种分发模式。
- 将 `/v1/messages` 与 `/v1/messages/count_tokens` 转发到 z.ai 的 Anthropic 兼容 base URL。
- 响应按字节流回传，不解析 SSE。
- 不影响 Gemini（`/v1beta/*`）或 OpenAI（`/v1/*`）协议路由。

## 配置
Schema：`src-tauri/src/proxy/config.rs`
- `ZaiConfig`
- `ZaiDispatchMode`

关键字段：
- `proxy.zai.enabled`
- `proxy.zai.base_url`（默认 `https://api.z.ai/api/anthropic`）
- `proxy.zai.api_key`（原始 token；也可以粘贴带 `Bearer ` 前缀的值）
- `proxy.zai.dispatch_mode`：
  - `off`
  - `exclusive`
  - `pooled`
  - `fallback`
- 当入参是 `claude-*` model id 时的默认映射：
  - `proxy.zai.models.opus`
  - `proxy.zai.models.sonnet`
  - `proxy.zai.models.haiku`

## 路由逻辑
入口：[`src-tauri/src/proxy/handlers/claude.rs`](../../src-tauri/src/proxy/handlers/claude.rs)
- `handle_messages(...)` 决定请求走 z.ai 或走 Google 链路。
- `handle_count_tokens(...)` 使用同一套分发决策（确保 `fallback` / `pooled` 行为一致）。
- `pooled` 模式按 `(google_accounts + 1)` 轮询，其中 slot `0` 为 z.ai。

### 可用性（readiness）规则
只有当满足以下条件时，z.ai 才会被视为**可参与路由**：
- `proxy.zai.enabled=true`
- `proxy.zai.base_url` 非空
- `proxy.zai.api_key` 已设置（原始 token；也可以粘贴带 `Bearer <token>` 前缀的值）

若 `dispatch_mode=exclusive` 但 provider 未满足可用性条件，代理会返回 `invalid_request_error`（配置错误），而不是静默回退到 Google。

### 分发模式语义
- `exclusive`：所有 Anthropic 协议请求都走 z.ai。
- `pooled`：z.ai 作为 Google 账号池之外的**一个额外 slot**参与轮询（当 Google 活跃账号数为 `N` 时，理论上约 `1/(N+1)` 概率落到 z.ai）。在并发/重试场景下为 best-effort，并非严格保证。
- `fallback`：仅当 Google 池不可用时（无活跃账号 / 运行时无可用账号）才会路由到 z.ai。

## 上游实现
实现：[`src-tauri/src/proxy/providers/zai_anthropic.rs`](../../src-tauri/src/proxy/providers/zai_anthropic.rs)
- 仅转发安全的 headers（不会把代理自身的鉴权 key 转发到上游）。
- 注入 z.ai 鉴权（`Authorization` / `x-api-key`）。
- 支持出站代理（`proxy.upstream_proxy`）。

## 验证
1) 在 UI 中启用 z.ai 并设置 `dispatch_mode=exclusive`（`src/pages/ApiProxy.tsx`）。
2) 启动代理。
3) 对 `POST /v1/messages` 发送一个正常的 Anthropic 请求。
4) 验证该请求由 z.ai 响应（exclusive 模式下该端点不使用 Google 账号池）。
