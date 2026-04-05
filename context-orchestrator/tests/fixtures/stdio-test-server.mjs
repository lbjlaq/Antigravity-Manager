import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import * as z from "zod";

const server = new McpServer({
  name: "stdio-test-server",
  version: "1.0.0",
});

server.registerTool(
  "echo_status",
  {
    description: "Echo a status string for MCP health probe tests.",
    inputSchema: {
      status: z.string().default("ok"),
    },
  },
  async ({ status }) => ({
    content: [
      {
        type: "text",
        text: status,
      },
    ],
    structuredContent: { status },
  }),
);

const transport = new StdioServerTransport();
await server.connect(transport);
