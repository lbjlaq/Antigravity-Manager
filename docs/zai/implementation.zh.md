# z.ai provider + MCP 代理（已实现）

本文描述 `feat/zai-passthrough-mcp` 分支上已实现的 z.ai 集成：新增了哪些能力、内部如何工作、以及如何验证。

相关深入文档：
- [`docs/zai/provider.zh.md`](provider.zh.md)
- [`docs/zai/mcp.zh.md`](mcp.zh.md)
- [`docs/zai/vision-mcp.zh.md`](vision-mcp.zh.md)
- [`docs/proxy/auth.zh.md`](../proxy/auth.zh.md)
- [`docs/proxy/accounts.zh.md`](../proxy/accounts.zh.md)
- [`docs/proxy/routing.zh.md`](../proxy/routing.zh.md)
- [`docs/proxy/config.zh.md`](../proxy/config.zh.md)

## 当前范围
- z.ai 作为 **可选上游** 仅用于 **Claude/Anthropic 协议**（`/v1/messages`、`/v1/messages/count_tokens`）。
- OpenAI 与 Gemini 协议处理器不变，继续使用既有的 Google 账号池。
- z.ai MCP（Search/Reader/zread）通过本地端点暴露（远程反代），并由代理注入上游 z.ai key。
- Vision MCP 通过代理内部 **内置 MCP server** 暴露（本地端点），并使用配置中的 z.ai key 调用 vision API。

## 配置
所有设置保存在既有数据目录（与 Google accounts / `gui_config.json` 同目录）。

### Proxy 鉴权
- `proxy.auth_mode`（`off` | `strict` | `all_except_health` | `auto`）
- `proxy.api_key`（开启鉴权时必填）

实现：
- 配置枚举：[`src-tauri/src/proxy/config.rs`](../../src-tauri/src/proxy/config.rs)
- 有效策略：[`src-tauri/src/proxy/security.rs`](../../src-tauri/src/proxy/security.rs)
- 中间件：[`src-tauri/src/proxy/middleware/auth.rs`](../../src-tauri/src/proxy/middleware/auth.rs)

### z.ai provider
配置位于 `proxy.zai`（`src-tauri/src/proxy/config.rs`）：
- `enabled: bool`
- `base_url: string`（默认 `https://api.z.ai/api/anthropic`）
- `api_key: string`
- `dispatch_mode: off | exclusive | pooled | fallback`
  - `off`：不使用 z.ai
  - `exclusive`：所有 Claude 协议请求走 z.ai
  - `pooled`：z.ai 作为共享池的一个槽位（无优先级保证）
  - `fallback`：仅在 Google 池为 0 时才使用 z.ai
- `models`：当入参 `model` 是 `claude-*` 时的默认映射
- `model_mapping`：精确匹配覆盖（命中则替换为指定 z.ai model id）
- `mcp`：
  - `enabled`
  - `web_search_enabled`
  - `web_reader_enabled`
  - `vision_enabled`
  - `zread_enabled`
  - `api_key_override`（可选，仅用于远程 MCP 上游）
  - `web_reader_url_normalization`（可选）

热更新：
- `save_config` 可热更新 `auth`、`upstream_proxy`、映射与 z.ai 配置（无需重启）。

## 请求路由

### `/v1/messages`（Claude/Anthropic messages）
入口：`src-tauri/src/proxy/handlers/claude.rs`（`handle_messages`）

流程概述：
1. 接收 `HeaderMap` + JSON body。
2. 根据 z.ai 配置决定走 z.ai 或走 Google 链路。
3. 若选中 z.ai：
   - 请求体基本原样转发（支持 streaming 字节透传）。
   - 可对 `model` 做重写（精确覆盖优先，其次按 `opus/sonnet/haiku` 默认映射）。
4. 否则走既有 Claude→Gemini + Google 执行路径。

### `/v1/messages/count_tokens`
入口：`src-tauri/src/proxy/handlers/claude.rs`（`handle_count_tokens`）
- z.ai 启用且 mode != off：转发到 z.ai。
- 否则返回既有占位响应。

## 上游转发细节（z.ai Anthropic）
Provider：`src-tauri/src/proxy/providers/zai_anthropic.rs`

安全与 header 处理：
- 代理自身的 API key 不会被转发到上游。
- 仅转发保守的 header 集合（如 `content-type`、`accept`、`anthropic-version`、`user-agent`）。
- 注入 z.ai 鉴权：
  - 若客户端发送 `x-api-key`，会被替换为 z.ai key
  - 若客户端发送 `Authorization`，会被替换为 `Bearer <zai_key>`
  - 若两者都没有，则使用 `x-api-key: <zai_key>`

网络：
- 支持 `proxy.upstream_proxy` 出站代理。

## MCP servers（Search / Reader / zread）
Handlers：`src-tauri/src/proxy/handlers/mcp.rs`
Routes：`src-tauri/src/proxy/server.rs`

本地端点：
- `/mcp/web_search_prime/mcp` → `https://api.z.ai/api/mcp/web_search_prime/mcp`（远程反代）
- `/mcp/web_reader/mcp` → `https://api.z.ai/api/mcp/web_reader/mcp`（远程反代）
  - 对 `webReader` 的 `tools/call` 可按 `proxy.zai.mcp.web_reader_url_normalization` 归一化 `arguments.url`
- `/mcp/zread/mcp` → `https://api.z.ai/api/mcp/zread/mcp`（远程反代）

行为：
- `proxy.zai.mcp.enabled=false` → 404
- 单项开关为 false → 该端点 404
- MCP 客户端无需配置 z.ai key（由代理注入）
- 远程 MCP 响应通常为 `text/event-stream`；代理会转发 `mcp-session-id` 响应头以支持会话延续

## Vision MCP（内置 server）
本地端点：
- `/mcp/zai-mcp-server/mcp`

实现：
- Handler：`src-tauri/src/proxy/handlers/mcp.rs`（`handle_zai_mcp_server`）
- 工具：`src-tauri/src/proxy/zai_vision_tools.rs`

## UI
页面：`src/pages/ApiProxy.tsx`
- Proxy 鉴权配置与模式提示
- 访问日志开关
- z.ai 配置（启用、base_url、dispatch_mode、api_key、模型映射）
- MCP 开关与本地端点展示

