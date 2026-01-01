# Vision MCP (built-in server)

## Why we implemented it this way
The upstream Vision MCP package (`@z_ai/mcp-server`) is designed as a **local stdio server**. In a desktop app + embedded proxy, requiring users (or the app) to manage a separate Node runtime/process increases operational complexity.

Instead, we implement a **built-in Vision MCP server** directly in the proxy:
- No extra runtime dependency.
- Single place to store the z.ai key (proxy config).
- Apps can talk to the local proxy using standard MCP over HTTP.

## Local endpoint
- `/mcp/zai-mcp-server/mcp`

Wired in:
- [`src-tauri/src/proxy/server.rs`](../../src-tauri/src/proxy/server.rs) (router)

## Protocol surface (minimal Streamable HTTP MCP)
Handler:
- [`src-tauri/src/proxy/handlers/mcp.rs`](../../src-tauri/src/proxy/handlers/mcp.rs) (`handle_zai_mcp_server`)

Implemented methods:
- `POST /mcp`:
  - `initialize`
  - `tools/list`
  - `tools/call`
- `GET /mcp`:
  - returns an SSE stream (keepalive) for an existing session
- `DELETE /mcp`:
  - terminates a session

Session storage:
- [`src-tauri/src/proxy/zai_vision_mcp.rs`](../../src-tauri/src/proxy/zai_vision_mcp.rs)

Notes:
- This is intentionally minimal to support tool calls.
- Prompts/resources, resumability, and streamed tool output can be added later if needed.

## Tool set
Tool registry:
- `tool_specs()` in [`src-tauri/src/proxy/zai_vision_tools.rs`](../../src-tauri/src/proxy/zai_vision_tools.rs)

Tool execution:
- `call_tool(...)` in [`src-tauri/src/proxy/zai_vision_tools.rs`](../../src-tauri/src/proxy/zai_vision_tools.rs)

Supported tools (mirrors the upstream package at a high level):
- `ui_to_artifact`
- `extract_text_from_screenshot`
- `diagnose_error_screenshot`
- `understand_technical_diagram`
- `analyze_data_visualization`
- `ui_diff_check`
- `image_analysis` (alias: `analyze_image`)
- `video_analysis` (alias: `analyze_video`)

## Upstream calls
Vision tools call the z.ai chat completions endpoint for the vision model:
- Primary (GLM Coding Plan): `https://api.z.ai/api/coding/paas/v4/chat/completions`
- Fallback: `https://api.z.ai/api/paas/v4/chat/completions`

Implementation:
- `vision_chat_completion(...)` in [`src-tauri/src/proxy/zai_vision_tools.rs`](../../src-tauri/src/proxy/zai_vision_tools.rs)

Auth:
- Uses `Authorization: Bearer <proxy.zai.api_key>`

Payload:
- `model: glm-4.6v` (currently hardcoded)
- `messages`: system prompt + a multimodal user message containing images/videos + text prompt
- `stream: false` (currently returns a single tool result)

## Local file handling
To support local file paths passed by MCP clients:
- Images (`.png`, `.jpg`, `.jpeg`) are read and encoded as `data:<mime>;base64,...` (5 MB max)
- Videos (`.mp4`, `.mov`, `.m4v`) are read and encoded as `data:<mime>;base64,...` (8 MB max)

Implementation:
- `image_source_to_content(...)` in [`src-tauri/src/proxy/zai_vision_tools.rs`](../../src-tauri/src/proxy/zai_vision_tools.rs)
- `video_source_to_content(...)` in [`src-tauri/src/proxy/zai_vision_tools.rs`](../../src-tauri/src/proxy/zai_vision_tools.rs)

## Quick validation (raw JSON-RPC)
1) Initialize:
   - `POST /mcp/zai-mcp-server/mcp` with `{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{}}}`
   - capture `mcp-session-id` response header
2) List tools:
   - `POST /mcp/zai-mcp-server/mcp` with `mcp-session-id: <id>` and `{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}`
3) Call tool:
   - `POST /mcp/zai-mcp-server/mcp` with `mcp-session-id: <id>` and `{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"image_analysis\",\"arguments\":{\"image_source\":\"/path/to/file.png\",\"prompt\":\"Describe this image\"}}}`
