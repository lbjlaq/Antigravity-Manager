# Vision MCP（内置 server）

## 为什么这样实现
上游 Vision MCP 包（`@z_ai/mcp-server`）是一个需要 Node 运行时的本地 stdio server。对桌面应用 + 内置代理来说，引入额外 runtime/进程会增加运维复杂度。

因此我们在代理中直接实现一个**内置 Vision MCP server**：
- 不需要额外 runtime
- z.ai key 只在代理配置中保存一份
- 其它应用可通过标准 MCP over HTTP 连接本地代理

## 本地端点
- `/mcp/zai-mcp-server/mcp`

路由：
- `src-tauri/src/proxy/server.rs`

## 协议面（最小 Streamable HTTP MCP）
Handler：
- `src-tauri/src/proxy/handlers/mcp.rs`（`handle_zai_mcp_server`）

已实现的方法：
- `POST /mcp`：
  - `initialize`
  - `tools/list`
  - `tools/call`
- `GET /mcp`：
  - 对已初始化 session 返回 SSE keepalive 流
- `DELETE /mcp`：
  - 终止 session

Session 存储：
- `src-tauri/src/proxy/zai_vision_mcp.rs`

说明：
- 该实现以“支持工具调用”为目标，prompts/resources、恢复能力与 streamed tool output 可后续补齐。

## 工具集合
工具注册：
- `tool_specs()` in `src-tauri/src/proxy/zai_vision_tools.rs`

工具执行：
- `call_tool(...)` in `src-tauri/src/proxy/zai_vision_tools.rs`

支持的工具：
- `ui_to_artifact`
- `extract_text_from_screenshot`
- `diagnose_error_screenshot`
- `understand_technical_diagram`
- `analyze_data_visualization`
- `ui_diff_check`
- `analyze_image`
- `analyze_video`

## 上游调用
Vision 工具调用 z.ai 视觉 chat completions 端点：
- `https://api.z.ai/api/paas/v4/chat/completions`

实现：
- `vision_chat_completion(...)` in `src-tauri/src/proxy/zai_vision_tools.rs`

鉴权：
- 使用 `Authorization: Bearer <zai_key>`，其中：
  - 默认取 `proxy.zai.api_key`
  - 若设置了 `proxy.zai.mcp.api_key_override`，则 MCP 侧优先使用该 key
  - 若用户粘贴了 `Bearer ...` 会先归一化

请求结构：
- `model: glm-4.6v`（当前硬编码）
- `messages`：system prompt + 多模态 user message（图片/视频 + 文本 prompt）
- `stream: false`（当前返回单段 tool 结果）

## 本地文件处理
为支持 MCP 客户端传入本地路径：
- 图片（`.png`/`.jpg`/`.jpeg`）：读取后编码为 `data:<mime>;base64,...`（5MB 上限）
- 视频（`.mp4`/`.mov`/`.m4v`）：读取后编码为 `data:<mime>;base64,...`（8MB 上限）

实现：
- `image_source_to_content(...)` in `src-tauri/src/proxy/zai_vision_tools.rs`
- `video_source_to_content(...)` in `src-tauri/src/proxy/zai_vision_tools.rs`

## 快速验证（原始 JSON-RPC）
1）Initialize：
   - `POST /mcp/zai-mcp-server/mcp`，body：`{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{}}}`
   - 读取响应头 `Mcp-Session-Id`
2）List tools：
   - `POST /mcp/zai-mcp-server/mcp`，header：`Mcp-Session-Id: <id>`，body：`{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}`
3）Call tool：
   - `POST /mcp/zai-mcp-server/mcp`，header：`Mcp-Session-Id: <id>`，body：`{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"analyze_image\",\"arguments\":{\"image_source\":\"/path/to/file.png\",\"prompt\":\"Describe this image\"}}}`

