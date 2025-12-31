# Proxy 路由与协议面

该代理设计目标是允许多个客户端同时使用（IDE、助手、自动化、HTTP 调用者等）。每个请求按 **HTTP 路径** 路由到对应协议处理器；只有部分处理器会根据配置使用 z.ai。

## 1) 协议面（代理提供哪些接口）

### Claude 协议（Anthropic 兼容）
- `POST /v1/messages`
- `POST /v1/messages/count_tokens`
- `GET /v1/models/claude`（静态列表占位）

### Gemini 协议（Google 原生）
- `GET /v1beta/models`
- `GET /v1beta/models/:model`
- `POST /v1beta/models/:model`（generate）
- `POST /v1beta/models/:model/countTokens`

### OpenAI 协议（兼容层）
- `POST /v1/chat/completions`
- `POST /v1/completions`
- `POST /v1/responses`（兼容别名，等同 `/v1/completions`）
- `POST /v1/images/generations`
- `POST /v1/images/edits`

### MCP 端点
- `ANY /mcp/web_search_prime/mcp`
- `ANY /mcp/web_reader/mcp`
- `ANY /mcp/zread/mcp`
- `ANY /mcp/zai-mcp-server/mcp`（内置 Vision MCP）

路由配置位置：
- [`src-tauri/src/proxy/server.rs`](../../src-tauri/src/proxy/server.rs)

## 2) Provider 选择规则（Google 池 vs z.ai）

### 2.1 Claude 协议（`/v1/messages`）
Claude 协议请求可能路由到：
- z.ai Anthropic 兼容上游（passthrough），或
- 既有的 Google 支持链路（Claude→Gemini 转换 + 账号池）。

决策输入：
- `proxy.zai.enabled`
- `proxy.zai.api_key` 是否已配置
- `proxy.zai.dispatch_mode`：
  - `off`：始终走 Google 链路
  - `exclusive`：Claude 协议始终走 z.ai
  - `pooled`：z.ai 作为 **一个槽位** 参与与 Google 账号的 round-robin（无优先级保证）
  - `fallback`：仅当 Google 账号池可用账号为 0 时，Claude 协议才走 z.ai

实现位置：
- 路由决策：[`src-tauri/src/proxy/handlers/claude.rs`](../../src-tauri/src/proxy/handlers/claude.rs)（`handle_messages`）
- z.ai 上游客户端：[`src-tauri/src/proxy/providers/zai_anthropic.rs`](../../src-tauri/src/proxy/providers/zai_anthropic.rs)

### 2.2 Gemini 协议（`/v1beta/*`）
Gemini 协议始终使用 Google 链路，不会路由到 z.ai。

实现位置：
- [`src-tauri/src/proxy/handlers/gemini.rs`](../../src-tauri/src/proxy/handlers/gemini.rs)

### 2.3 OpenAI 协议（`/v1/*`）
OpenAI 兼容协议使用既有代理逻辑（映射 + Google 执行）。z.ai 的 dispatch 模式不影响这些路由。

实现位置：
- [`src-tauri/src/proxy/handlers/openai.rs`](../../src-tauri/src/proxy/handlers/openai.rs)

## 3) 模型映射规则（映射影响哪些请求）

代理支持多层映射（在 API Proxy UI 中配置）：
- `proxy.anthropic_mapping` — 影响 Claude 协议
- `proxy.openai_mapping` — 影响 OpenAI 协议
- `proxy.custom_mapping` — 可选的自定义覆盖

z.ai（仅 Claude 协议）模型映射：
- `proxy.zai.models.{opus,sonnet,haiku}`：当入参 `model` 为 `claude-*` 且路由到 z.ai 时，提供默认映射
- `proxy.zai.model_mapping`：精确匹配覆盖（若入参 `model` 字符串命中 key，则替换为对应 z.ai model id）

重要行为：
- z.ai 的模型映射只在“最终路由到 z.ai”时生效。
- 若最终走 Google 链路，则沿用现有 Claude→Gemini 的映射逻辑。

配置定义：
- [`src-tauri/src/proxy/config.rs`](../../src-tauri/src/proxy/config.rs)

## 4) MCP 路由规则

MCP 由 `proxy.zai.mcp.*` 控制：
- 若 `proxy.zai.mcp.enabled=false` → 所有 `/mcp/*` 返回 404
- 各 MCP server 有独立开关（`web_search_enabled`、`web_reader_enabled`、`zread_enabled`、`vision_enabled`）

端点类型：
- Web Search MCP：反代到上游 z.ai MCP（Streamable HTTP）
- Web Reader MCP：反代到上游 z.ai MCP（Streamable HTTP，且对 `webReader` 的 tool call 支持可选 URL 归一化）
- zread MCP：反代到上游 zread MCP（Streamable HTTP）
- Vision MCP：本地内置 MCP server（无需外部 Node 进程）

更多细节：
- [`docs/zai/mcp.zh.md`](../zai/mcp.zh.md)

## 5) 安全与鉴权的交互

代理鉴权是全局的，按 `proxy.auth_mode` 影响所有协议面：
- `off`：不鉴权
- `strict`：所有路由都要求鉴权
- `all_except_health`：除 `GET /healthz` 外都要求鉴权
- `auto`：由 `proxy.allow_lan_access` 推导

开启鉴权后，客户端需发送：
- `Authorization: Bearer <proxy.api_key>`

注意：
- 代理自身的 API key 不会被转发到任何上游。
- 访问日志默认不记录 query/header/body（降低泄露风险）。

参考：
- [`docs/proxy/auth.zh.md`](auth.zh.md)
- [`docs/proxy/logging.zh.md`](logging.zh.md)

## 6) 多客户端并发使用（常见场景）

由于按路径路由，不同客户端可并发使用不同协议而互不影响：
- 一个客户端调用 `POST /v1/messages`（Claude），另一个调用 `POST /v1/chat/completions`（OpenAI），第三个连接 `/mcp/*`。
- 只有 Claude 协议受 `proxy.zai.dispatch_mode` 影响。
- 若开启鉴权，所有客户端都必须带上代理鉴权头（`all_except_health` 模式下 `GET /healthz` 例外）。

