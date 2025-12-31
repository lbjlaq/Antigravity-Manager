# z.ai MCP upstream references (summary)

This project integrates with several z.ai capabilities. The upstream documentation is public and can change; this file records the key integration points we rely on.

Reference pages:
- `https://docs.z.ai/devpack/mcp/search-mcp-server`
- `https://docs.z.ai/devpack/mcp/reader-mcp-server`
- `https://docs.z.ai/devpack/mcp/zread-mcp-server`
- `https://docs.z.ai/devpack/mcp/vision-mcp-server`
- `https://docs.z.ai/api-reference/tools/web-search`
- `https://docs.z.ai/api-reference/tools/web-reader`

## Search MCP (web_search_prime)
Upstream remote MCP (reference):
- Streamable HTTP endpoint: `https://api.z.ai/api/mcp/web_search_prime/mcp`
- SSE endpoint: `https://api.z.ai/api/mcp/web_search_prime/sse?Authorization=your_api_key`

## Reader MCP (web_reader)
Upstream remote MCP (reference):
- Streamable HTTP endpoint: `https://api.z.ai/api/mcp/web_reader/mcp`
- SSE endpoint: `https://api.z.ai/api/mcp/web_reader/sse?Authorization=your_api_key`

## zread MCP
- Streamable HTTP endpoint: `https://api.z.ai/api/mcp/zread/mcp`
- SSE endpoint: `https://api.z.ai/api/mcp/zread/sse?Authorization=your_api_key` (not used by this project)
- Auth header: `Authorization: Bearer <api_key>`

## Vision MCP
The upstream documentation describes a stdio-based MCP server package (executed via a Node runner) that uses environment variables:
- `Z_AI_API_KEY`
- `Z_AI_MODE=ZAI`

This project implements a local Vision MCP server endpoint instead of invoking the external package, so MCP clients can connect to the local proxy without embedding the upstream API key.
