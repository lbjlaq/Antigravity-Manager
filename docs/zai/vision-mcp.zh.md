# Vision MCP（内置 server）

## 为什么这样实现
上游 Vision MCP 包（`@z_ai/mcp-server`）是 **本地 stdio server**（由客户端启动的 Node 进程）。在桌面应用 + 内置代理场景里，引入额外运行时/进程会增加运维复杂度。

因此我们在代理内部实现了一个 **内置 Vision MCP server**：
- 不需要额外 runtime。
- z.ai key 统一存放于代理配置。
- 应用只需连接本地代理即可使用 MCP。

## 本地端点
- `/mcp/zai-mcp-server/mcp`

路由接入：
- [`src-tauri/src/proxy/server.rs`](../../src-tauri/src/proxy/server.rs)

## 协议面（最小化 Streamable HTTP MCP）
Handler：
- [`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs)（`handle_zai_mcp_server`）

已实现：
- `POST /mcp`：
  - `initialize`
  - `tools/list`
  - `tools/call`
- `GET /mcp`：
  - 返回 SSE keepalive（针对已存在 session）
- `DELETE /mcp`：
  - 关闭 session

Session 存储：
- [`src-tauri/src/proxy/zai_vision_mcp.rs`](../../src-tauri/src/proxy/zai_vision_mcp.rs)

说明：
- 该实现聚焦支持 tool calls。
- prompts/resources、恢复能力、tool 输出流式化可作为后续增强。

## 工具集合
工具注册：
- `tool_specs()`：[`src-tauri/src/proxy/zai_vision_tools.rs`](../../src-tauri/src/proxy/zai_vision_tools.rs)

工具执行：
- `call_tool(...)`：[`src-tauri/src/proxy/zai_vision_tools.rs`](../../src-tauri/src/proxy/zai_vision_tools.rs)

支持的工具（与上游包大体对齐）：
- `ui_to_artifact`
- `extract_text_from_screenshot`
- `diagnose_error_screenshot`
- `understand_technical_diagram`
- `analyze_data_visualization`
- `ui_diff_check`
- `image_analysis`（别名：`analyze_image`）
- `video_analysis`（别名：`analyze_video`）

## 上游调用
Vision 工具通过 z.ai chat completions（vision model）实现：
- 优先（Coding Plan）：`https://api.z.ai/api/coding/paas/v4/chat/completions`
- 回退：`https://api.z.ai/api/paas/v4/chat/completions`

实现：
- `vision_chat_completion(...)`：[`src-tauri/src/proxy/zai_vision_tools.rs`](../../src-tauri/src/proxy/zai_vision_tools.rs)

鉴权：
- `Authorization: Bearer <proxy.zai.api_key>`

请求 payload（当前）：
- `model: glm-4.6v`（暂为硬编码）
- `messages`：system prompt + multimodal user message（图像/视频 + 文本）
- `stream: false`（当前返回单次结果）

## 本地文件处理
支持 MCP 客户端传入本地文件路径：
- 图片（`.png` / `.jpg` / `.jpeg`）：读取并编码为 `data:<mime>;base64,...`（最大 5MB）
- 视频（`.mp4` / `.mov` / `.m4v`）：读取并编码为 `data:<mime>;base64,...`（最大 8MB）

实现：
- `image_source_to_content(...)`：[`src-tauri/src/proxy/zai_vision_tools.rs`](../../src-tauri/src/proxy/zai_vision_tools.rs)
- `video_source_to_content(...)`：[`src-tauri/src/proxy/zai_vision_tools.rs`](../../src-tauri/src/proxy/zai_vision_tools.rs)

## 快速验证（raw JSON-RPC）
1) Initialize：
   - `POST /mcp/zai-mcp-server/mcp`，body 包含 `initialize`
   - 读取响应头 `mcp-session-id`
2) List tools：
   - 带 `mcp-session-id` 调用 `tools/list`
3) Call tool：
   - 带 `mcp-session-id` 调用 `tools/call`，例如 `image_analysis`

