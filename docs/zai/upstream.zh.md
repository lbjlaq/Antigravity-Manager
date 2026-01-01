# z.ai MCP 上游参考（摘要）

本项目集成了多个 z.ai 能力。上游文档公开且可能变化；本文件记录我们依赖的关键入口与端点。

参考页面：
- `https://docs.z.ai/devpack/mcp/search-mcp-server`
- `https://docs.z.ai/devpack/mcp/reader-mcp-server`
- `https://docs.z.ai/devpack/mcp/zread-mcp-server`
- `https://docs.z.ai/devpack/mcp/vision-mcp-server`
- `https://docs.z.ai/api-reference/tools/web-search`
- `https://docs.z.ai/api-reference/tools/web-reader`

## Search MCP（web_search_prime）
上游远程 MCP：
- Streamable HTTP：`https://api.z.ai/api/mcp/web_search_prime/mcp`
- SSE：`https://api.z.ai/api/mcp/web_search_prime/sse?Authorization=your_api_key`

## Reader MCP（web_reader）
上游远程 MCP：
- Streamable HTTP：`https://api.z.ai/api/mcp/web_reader/mcp`
- SSE：`https://api.z.ai/api/mcp/web_reader/sse?Authorization=your_api_key`

## zread MCP
- Streamable HTTP：`https://api.z.ai/api/mcp/zread/mcp`
- SSE：`https://api.z.ai/api/mcp/zread/sse?Authorization=your_api_key`（本项目不使用）
- 鉴权 header：`Authorization: Bearer <api_key>`

## Vision MCP
上游文档描述的是 stdio 形态（Node runner）：
- `Z_AI_API_KEY`
- `Z_AI_MODE=ZAI`

本项目实现了内置 Vision MCP（HTTP），以便 MCP 客户端只连接本地代理且无需在客户端侧配置上游 key。

