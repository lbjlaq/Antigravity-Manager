import fs from "node:fs";
import path from "node:path";

import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";

import { loadConfig } from "./config.js";
import { createGateway } from "./gateway/server.js";
import { ContextService } from "./services/context-service.js";
import { IndexService } from "./services/index-service.js";
import { McpHealthService } from "./services/mcp-health.js";
import { OpenAIService } from "./services/openai.js";
import { PlannerService } from "./services/planner-service.js";
import { checkQdrantHealth, createQdrantClient } from "./services/qdrant.js";
import { CacheRepository } from "./storage/cache.js";
import { ArtifactRepository } from "./storage/index.js";
import { SqliteStore } from "./storage/sqlite.js";

async function main(): Promise<void> {
  const config = loadConfig();

  fs.mkdirSync(path.dirname(config.sqlitePath), { recursive: true });
  fs.mkdirSync(config.artifactsDir, { recursive: true });

  const sqlite = new SqliteStore(config.sqlitePath);
  const cache = new CacheRepository(sqlite);
  const artifacts = new ArtifactRepository(sqlite, config.artifactsDir);
  const qdrant = createQdrantClient(config.qdrantUrl, config.qdrantApiKey);
  const openai = new OpenAIService(config.openaiApiKey, config.openaiBaseUrl);
  const indexService = new IndexService(config, qdrant, openai, cache, artifacts);
  const contextService = new ContextService(
    config.skillRoots,
    artifacts,
    cache,
    indexService,
    {
      skills: config.qdrantCollections.skills,
      sessionSummaries: config.qdrantCollections.sessionSummaries,
      repoDocs: config.qdrantCollections.repoDocs,
      mcpServers: config.qdrantCollections.mcpServers,
    },
  );
  const plannerService = new PlannerService(openai, config);
  const mcpHealthService = new McpHealthService(config, artifacts);
  const qdrantHealth = await checkQdrantHealth(qdrant);

  console.error(
    `Context Orchestrator starting. Qdrant ok=${qdrantHealth.ok}` +
      ` openai=${openai.isConfigured()}` +
      (qdrantHealth.ok
        ? ` collections=${qdrantHealth.collectionCount ?? 0}`
        : ` error=${qdrantHealth.error}`),
  );

  if (qdrantHealth.ok && openai.isConfigured()) {
    try {
      await indexService.bootstrap();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      console.error(
        `Context Orchestrator bootstrap warning: semantic indexing unavailable during startup: ${message}`,
      );
    }
  }

  const server = createGateway(contextService, plannerService, indexService, mcpHealthService, artifacts);
  const transport = new StdioServerTransport();
  await server.connect(transport);
  console.error("Context Orchestrator MCP server connected over stdio");
}

main().catch((error) => {
  console.error("Context Orchestrator startup failed:", error);
  process.exit(1);
});
