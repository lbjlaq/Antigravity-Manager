# 通过本地 Proxy 暴露 z.ai MCP 端点

本页描述本地 API Proxy 暴露的 z.ai MCP 端点的用户侧行为与配置规则。

## Overview
启用后，本地代理会在 `/mcp/*` 下暴露一个或多个 MCP 端点。

关键特性：
- MCP 端点通过 API Proxy UI 的开关 **按需启用**。
- z.ai 凭据仅保存在本地配置（`proxy.zai.api_key`），MCP 客户端无需配置 z.ai key。
- 若开启了代理鉴权，MCP 端点同样会受鉴权策略影响。

## 如何启用
1) 配置 z.ai：
   - 设置 `proxy.zai.api_key`（原始 token；也可以粘贴带 `Bearer ` 前缀的值）
   - `proxy.zai.enabled=true` 仅在你希望把 Anthropic `/v1/messages` 作为 provider 路由到 z.ai 时需要
2) 启用 MCP 暴露：
   - `proxy.zai.mcp.enabled=true`
3) 按需启用各 MCP server：
   - `proxy.zai.mcp.web_search_enabled`
   - `proxy.zai.mcp.web_reader_enabled`
   - `proxy.zai.mcp.zread_enabled`
   - `proxy.zai.mcp.vision_enabled`

路由规则：
- 若 `proxy.zai.mcp.enabled=false`，所有 `/mcp/*` 均返回 404（即使单项开关为 true）。

## 在 MCP 客户端中使用（Claude Code）
Claude Code 支持通过 Streamable HTTP 连接 MCP server。推荐配置为指向本地代理的 `/mcp/*` 端点，从而自动继承代理的鉴权策略与 z.ai 配置。

编辑 `~/.claude/.claude.json`，在 `mcpServers` 下添加：

```json
{
  "mcpServers": {
    "zai-web-search": {
      "type": "http",
      "url": "http://127.0.0.1:8045/mcp/web_search_prime/mcp",
      "headers": { "Authorization": "Bearer <PROXY_API_KEY>" }
    },
    "zai-web-reader": {
      "type": "http",
      "url": "http://127.0.0.1:8045/mcp/web_reader/mcp",
      "headers": { "Authorization": "Bearer <PROXY_API_KEY>" }
    },
    "zai-zread": {
      "type": "http",
      "url": "http://127.0.0.1:8045/mcp/zread/mcp",
      "headers": { "Authorization": "Bearer <PROXY_API_KEY>" }
    },
    "zai-vision": {
      "type": "http",
      "url": "http://127.0.0.1:8045/mcp/zai-mcp-server/mcp",
      "headers": { "Authorization": "Bearer <PROXY_API_KEY>" }
    }
  }
}
```

说明：
- 若代理鉴权关闭（`proxy.auth_mode=off`），可直接去掉 `headers`。
- 也可以使用 `x-api-key` 代替 `Authorization`（代理支持两种方式）。
- Claude Code 不需要 z.ai key；代理会使用本地配置为上游注入鉴权。

### 1) Web Search（远程反代）
本地端点：
- `/mcp/web_search_prime/mcp`

上游远程 MCP（Streamable HTTP）：
- `https://api.z.ai/api/mcp/web_search_prime/mcp`

行为：
- 该端点是对上游 z.ai MCP server 的 Streamable HTTP 反向代理。
- 代理会注入上游鉴权（使用本地保存的 z.ai key）并原样流式转发响应。
- 需要 session：
  - 先调用 `initialize`
  - 从响应头读取 `mcp-session-id`
  - 后续请求（`tools/list` / `tools/call`）带上 `mcp-session-id`

实现：
- Handler：[`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs)（`handle_web_search_prime`）

### 2) Web Reader（远程反代）
本地端点：
- `/mcp/web_reader/mcp`

上游远程 MCP（Streamable HTTP）：
- `https://api.z.ai/api/mcp/web_reader/mcp`

可选 URL 归一化：
- 配置：`proxy.zai.mcp.web_reader_url_normalization`
  - `off`（默认）：不改 URL
  - `strip_tracking_query`：移除常见跟踪参数（`utm_*`、`hsa_*`、`gclid`、`fbclid`、`gbraid`、`wbraid`、`msclkid`）
  - `strip_query`：移除整个 query string（`?…`）

行为：
- 该端点是对上游 Web Reader MCP 的 Streamable HTTP 反代。
- 上游对 URL 格式较严格；对带大量 tracking query 的 URL，归一化可提高兼容性。
- 需要 session（同上）。
- URL 归一化仅作用于 JSON-RPC `tools/call`，且满足：
  - `params.name == "webReader"`
  - `params.arguments.url` 为 `http(s)` URL

实现：
- Handler：[`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs)（`handle_web_reader`）
- URL 归一化：[`src-tauri/src/proxy/zai_web_tools.rs`](../../src-tauri/src/proxy/zai_web_tools.rs)（`normalize_web_reader_url`）

### 3) zread（远程反代）
本地端点：
- `/mcp/zread/mcp`

上游：
- `https://api.z.ai/api/mcp/zread/mcp`

说明：
- zread 提供仓库/文档读取工具（如 `search_doc`、`read_file`、`get_repo_structure`），与 Web Reader 不同。

行为：
- 代理将 Streamable HTTP JSON-RPC 请求转发到上游。
- tool 输入以上游 schema 为准（例如 `repo_name` 通常为 `owner/repo`）。
  - `repo_name` 必须是上游可访问/可索引的公共 GitHub 仓库。
  - 对私有仓库或上游不可见的仓库，可能返回 `target not found` 等错误。

实现：
- Handler：[`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs)（`handle_zread`）

### 4) Vision MCP（内置 server）
本地端点：
- `/mcp/zai-mcp-server/mcp`

行为：
- 该端点为代理内置 MCP server（不是远程 MCP 反代）。
- 对拥有 Coding Plan 的 key，会优先使用 z.ai 的 coding endpoint（`/api/coding/paas/v4`）；仅在特定错误场景才回退到 general endpoint。
- 需要 session（同上）。

实现：
- 路由：[`src-tauri/src/proxy/server.rs`](../../src-tauri/src/proxy/server.rs)
- Handler：[`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs)（`handle_zai_mcp_server`）
- Session state：[`src-tauri/src/proxy/zai_vision_mcp.rs`](../../src-tauri/src/proxy/zai_vision_mcp.rs)
- Tool 执行：[`src-tauri/src/proxy/zai_vision_tools.rs`](../../src-tauri/src/proxy/zai_vision_tools.rs)

## 鉴权模型
- 若开启了代理鉴权，`/mcp/*` 也会受影响：
  - Middleware：[`src-tauri/src/proxy/middleware/auth.rs`](../../src-tauri/src/proxy/middleware/auth.rs)
- 上游 z.ai 的鉴权始终由代理注入。
- 对远程 MCP 反代端点，代理会同时注入 `Authorization: Bearer <zai_token>` 与 `x-api-key: <zai_token>` 以提高上游兼容性（若用户粘贴了 `Bearer ...`，会先做归一化）。
- 可选：`proxy.zai.mcp.api_key_override` 可为 MCP 使用独立 key（覆盖 `proxy.zai.api_key`，适用于 z.ai MCP 能力：远程 MCP 上游与内置 MCP 工具）。
- MCP 客户端只需对本地代理做鉴权（如果启用），不应在客户端侧配置 z.ai key。

## Streaming / content-type
- 远程 z.ai MCP 常以 `text/event-stream`（SSE）形式响应。
- 反代远程 MCP 时，代理会设置上游 `Accept` 同时包含 `application/json` 与 `text/event-stream`，以兼容不同客户端。

## UI
- MCP 开关与端点展示：[`src/pages/ApiProxy.tsx`](../../src/pages/ApiProxy.tsx)

## 限制与预期
- Web Reader 的可用性与站点强相关（反爬、重定向、动态渲染等），上游可能对部分 URL 解析失败。
- Web Search / zread / vision 会受到上游套餐/额度/限流影响，可能返回 4xx/5xx。
- 部分上游 tool 失败会以 JSON-RPC `result` 形式返回（`result.isError=true` 且 `result.content[0].text` 为错误文本），而非 JSON-RPC `error` 对象；客户端应按数据处理。
