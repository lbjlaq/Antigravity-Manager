# z.ai provider + MCP proxy（已实现）

本文描述本项目已实现的 z.ai 集成：新增了什么、内部如何工作、以及如何验证。

相关文档：
- `docs/zai/provider.zh.md`
- `docs/zai/mcp.zh.md`
- `docs/zai/vision-mcp.zh.md`
- `docs/proxy/auth.zh.md`
- `docs/proxy/accounts.zh.md`

## 当前范围
- z.ai 作为**可选上游**，仅用于 **Anthropic/Claude 协议**（`/v1/messages`、`/v1/messages/count_tokens`）。
- OpenAI 协议与 Gemini 协议保持原行为，仍通过现有 Google 账号池处理。
- z.ai MCP（Search / Reader / zread）通过本地端点以“远程反代”方式暴露，并由代理向上游注入 z.ai key。
- Vision MCP 通过代理内置 MCP server 暴露（本地端点），并用本地保存的 z.ai key 调用 z.ai 视觉 API。

## 配置
所有配置仍保存在现有数据目录（与 Google accounts、`gui_config.json` 同目录）。

### Proxy 鉴权
- `proxy.auth_mode`（`off | strict | all_except_health | auto`）
  - `off`：不鉴权
  - `strict`：所有路由都鉴权
  - `all_except_health`：除 `GET /healthz` 外都鉴权
  - `auto`：若 `allow_lan_access=true` 则默认 `all_except_health`，否则 `off`
- `proxy.api_key`：开启鉴权时需要

实现：
- enum：`src-tauri/src/proxy/config.rs`（`ProxyAuthMode`）
- 策略解析：`src-tauri/src/proxy/security.rs`
- 中间件：`src-tauri/src/proxy/middleware/auth.rs`

### z.ai provider
配置位于 `proxy.zai`（`src-tauri/src/proxy/config.rs`）：
- `enabled: bool`
- `base_url: string`（默认 `https://api.z.ai/api/anthropic`）
- `api_key: string`
- `dispatch_mode: off | exclusive | pooled | fallback`
  - `off`：不使用 z.ai
  - `exclusive`：所有 Claude 协议请求走 z.ai
  - `pooled`：z.ai 作为一个槽位加入共享轮询（无优先级、无强保证）
  - `fallback`：仅当 Google 池为 0 时使用 z.ai
- `models`：当入参是 `claude-*` 时用于映射到 GLM（默认）
  - `opus` 默认 `glm-4.7`
  - `sonnet` 默认 `glm-4.7`
  - `haiku` 默认 `glm-4.5-air`
- `model_mapping`：可选精确匹配覆盖（`{ "<incoming_model>": "<glm-model-id>" }`）
- `mcp`（MCP 相关开关/选项）：
  - `enabled`
  - `web_search_enabled`
  - `web_reader_enabled`
  - `zread_enabled`
  - `vision_enabled`
  - 可选 `api_key_override`（仅 MCP 使用）
  - 可选 `web_reader_url_normalization`（仅远程 Web Reader MCP 使用）

热更新：
- `save_config` 会在不重启代理的情况下热更新 `auth` / `upstream_proxy` / `model mappings` / `z.ai`。
  - `src-tauri/src/commands/mod.rs` 调用 `axum_server.update_security(...)` 与 `axum_server.update_zai(...)`。

## 请求路由

### `/v1/messages`（Anthropic messages）
Handler：`src-tauri/src/proxy/handlers/claude.rs`（`handle_messages`）

流程（简化）：
1）接收 `HeaderMap` + 原始 JSON `Value`。
2）决定走 z.ai 还是 Google：
   - z.ai 关闭 → 走 Google
   - `dispatch_mode=exclusive` → 走 z.ai
   - `dispatch_mode=fallback` → 仅当 Google 池为 0 时走 z.ai
   - `dispatch_mode=pooled` → 在 `(google_accounts + 1)` 个槽位做 round-robin；槽位 `0` 为 z.ai，其余为 Google
3）若选中 z.ai：
   - 原始 JSON 直接转发到 z.ai（支持流式 passthrough）
   - `model` 可能会被改写：
     - `proxy.zai.model_mapping` 精确匹配优先
     - `glm-*` 保持不变
     - `claude-*` 根据名称匹配映射到 `proxy.zai.models.{opus,sonnet,haiku}`
4）否则沿用原 Claude→Gemini 变换与 Google 账号池路径。

### `/v1/messages/count_tokens`
Handler：`src-tauri/src/proxy/handlers/claude.rs`（`handle_count_tokens`）
- 若 z.ai 启用（mode != off），则转发到 z.ai。
- 否则返回现有占位 `{input_tokens: 0, output_tokens: 0}`。

## MCP 远程反代（Search / Reader / zread）
Handlers：`src-tauri/src/proxy/handlers/mcp.rs`
Routes：`src-tauri/src/proxy/server.rs`

本地端点：
- `/mcp/web_search_prime/mcp` → `https://api.z.ai/api/mcp/web_search_prime/mcp`
- `/mcp/web_reader/mcp` → `https://api.z.ai/api/mcp/web_reader/mcp`
- `/mcp/zread/mcp` → `https://api.z.ai/api/mcp/zread/mcp`

行为：
- 受 `proxy.zai.mcp.*` 控制：
  - `mcp.enabled=false` → 所有 `/mcp/*` 返回 404
  - 对应单项开关为 false → 该端点返回 404
- 代理向上游注入 z.ai key（若用户粘贴了 `Bearer ...` 会先归一化）：
  - `Authorization: Bearer <zai_key>`
  - `x-api-key: <zai_key>`
- 响应以流式方式原样转发给客户端。

Web Reader URL 归一化：
- 若 `proxy.zai.mcp.web_reader_url_normalization != off`，代理会在转发前改写 JSON-RPC `tools/call` 且 `params.name == "webReader"` 的请求体，归一化 `params.arguments.url`。

说明：
- 是否需要对本地 MCP 端点鉴权，仍由 `proxy.auth_mode` 决定（与其它代理路由一致）。

## Vision MCP（内置 server）
Handlers：
- `src-tauri/src/proxy/handlers/mcp.rs`（`handle_zai_mcp_server`）
- `src-tauri/src/proxy/zai_vision_tools.rs`（工具注册 + 上游调用）

本地端点：
- `/mcp/zai-mcp-server/mcp`

行为：
- 受 `proxy.zai.mcp.enabled` 与 `proxy.zai.mcp.vision_enabled` 控制（关闭则 404）。
- MCP 客户端无需配置 z.ai key：
  - 代理调用上游时使用 `proxy.zai.api_key`（或 `proxy.zai.mcp.api_key_override` 覆盖）。
- 实现了最小 Streamable HTTP MCP：
  - `POST` 支持 `initialize` / `tools/list` / `tools/call`
  - `GET` 为已初始化 session 返回 SSE keepalive
  - `DELETE` 终止 session

上游调用：
- `https://api.z.ai/api/paas/v4/chat/completions`
- `Authorization: Bearer <zai_key>`（需要时会做归一化）
- 默认模型：`glm-4.6v`（当前硬编码）

## UI
页面：`src/pages/ApiProxy.tsx`

新增控制项（概览）：
- Proxy 鉴权开关 + 模式选择
- z.ai provider（开关、base_url、dispatch mode、api key、模型映射）
- MCP 开关与本地端点提示

## 验证清单（建议）
构建：
- 前端：`npm run build`
- 后端：`cd src-tauri && cargo build`

手动验证（示例）：
1）开启鉴权并记录 `proxy.api_key`。
2）配置 z.ai（`dispatch_mode=exclusive`，设置 `api_key`）。
3）启动代理并调用：
   - `GET /healthz`
   - `POST /v1/messages`（携带 `Authorization: Bearer <proxy.api_key>`）
4）开启 MCP Search/Reader/zread 并用 MCP 客户端连接本地 `/mcp/*`。
5）开启 Vision MCP 并验证 `initialize` → `tools/list` → `tools/call`。

## 已知限制 / 后续
- Vision MCP 当前仅实现满足工具调用的最小方法集；prompts/resources、恢复与 streamed tool output 可后续补齐。
- z.ai usage/budget（monitor endpoints）尚未实现。
- Claude 模型列表接口仍为静态 stub（`/v1/models/claude`），尚未做到 provider-aware。

