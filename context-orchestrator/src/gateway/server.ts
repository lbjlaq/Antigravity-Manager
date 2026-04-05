import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";

import type { ArtifactRepository } from "../storage/index.js";
import { ContextService } from "../services/context-service.js";
import { IndexService } from "../services/index-service.js";
import { McpHealthService } from "../services/mcp-health.js";
import { PlannerService } from "../services/planner-service.js";
import {
  MemorySummaryInputSchema,
  OrchestratorStatusInputSchema,
  PlanOrReviewInputSchema,
  ProbeMcpServersInputSchema,
  PrepareTaskContextInputSchema,
  ReindexInputSchema,
  SearchQuerySchema,
} from "../schema.js";

function shouldInvokePlanner(goal: string): boolean {
  const normalized = goal.toLowerCase();
  return [
    "plan",
    "design",
    "architecture",
    "review",
    "investigate",
    "refactor",
    "mcp",
    "provider",
  ].some((token) => normalized.includes(token));
}

export function createGateway(
  contextService: ContextService,
  plannerService: PlannerService,
  indexService: IndexService,
  mcpHealthService: McpHealthService,
  artifacts: ArtifactRepository,
): McpServer {
  const server = new McpServer({
    name: "context-orchestrator",
    version: "0.2.0",
  });

  server.registerTool(
    "prepare_task_context",
    {
      description: "Prepare a compact context pack using cached retrieval, semantic indexing, and optional planner escalation.",
      inputSchema: PrepareTaskContextInputSchema,
    },
    async ({ goal, cwd, repoId, taskHints, changedFiles }) => {
      const plannerArtifact = shouldInvokePlanner(goal)
        ? await plannerService.buildArtifact({
            mode: "auto",
            taskClass: contextService.classify(goal),
            taskDescription: goal,
            repoId,
            evidence: [
              {
                kind: "user_input",
                ref: "goal",
                note: goal,
              },
            ],
          })
        : undefined;

      if (plannerArtifact) {
        const filePath = artifacts.save(plannerArtifact);
        await indexService.ingestArtifact(plannerArtifact, filePath);
      }

      const contextPack = await contextService.prepareContext(
        goal,
        cwd,
        plannerArtifact?.id,
        taskHints,
        changedFiles,
      );
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(contextPack, null, 2),
          },
        ],
        structuredContent: contextPack,
      };
    },
  );

  server.registerTool(
    "plan_or_review",
    {
      description: "Generate a structured planning or review artifact using GPT-5.4 when configured, with deterministic fallback.",
      inputSchema: PlanOrReviewInputSchema,
    },
    async ({ mode, taskDescription, cwd, repoId, evidenceRefs }) => {
      const artifact = await plannerService.buildArtifact({
        mode,
        taskClass: contextService.classify(taskDescription),
        taskDescription,
        repoId,
        evidence: [
          {
            kind: "user_input",
            ref: "taskDescription",
            note: taskDescription,
          },
          ...evidenceRefs.map((ref) => ({
            kind: "repo_fact" as const,
            ref,
            note: `Explicit evidence reference: ${ref}`,
          })),
        ],
      });
      const filePath = artifacts.save(artifact);
      await indexService.ingestArtifact(artifact, filePath);

      return {
        content: [
          {
            type: "text",
            text: JSON.stringify({ artifact, filePath, cwd }, null, 2),
          },
        ],
        structuredContent: {
          artifact,
          filePath,
        },
      };
    },
  );

  server.registerTool(
    "submit_memory_summary",
    {
      description: "Persist a lightweight agent/client summary into the shared memory inbox using the global artifact format.",
      inputSchema: MemorySummaryInputSchema,
    },
    async ({ source, summary, details, category, relatedFiles, cwd, repoId, profileId }) => {
      const artifact = await plannerService.buildArtifact({
        mode: "auto",
        taskClass: contextService.classify(`${summary}\n${details ?? ""}`),
        taskDescription: summary,
        repoId,
        evidence: [
          {
            kind: "user_input",
            ref: source,
            note: details ?? summary,
          },
          ...relatedFiles.map((filePath) => ({
            kind: "repo_fact" as const,
            ref: filePath,
            note: `Related file: ${filePath}`,
          })),
        ],
      });

      const enrichedArtifact = {
        ...artifact,
        summary,
        explanation: details?.trim() ? details : artifact.explanation,
        profile_id: profileId,
        source_client: source,
        memory_category: category,
        memory_status: "NEW",
        related_files: relatedFiles,
        cwd,
      };

      const filePath = artifacts.save(enrichedArtifact);
      await indexService.ingestArtifact(enrichedArtifact, filePath);

      return {
        content: [
          {
            type: "text",
            text: JSON.stringify({ artifact: enrichedArtifact, filePath }, null, 2),
          },
        ],
        structuredContent: {
          artifact: enrichedArtifact,
          filePath,
        },
      };
    },
  );

  server.registerTool(
    "search_memory",
    {
      description: "Search persisted local planning and review artifacts using semantic retrieval plus deterministic fallback.",
      inputSchema: SearchQuerySchema,
    },
    async ({ query, limit }) => {
      const hits = await contextService.searchMemory(query, limit);
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(hits, null, 2),
          },
        ],
        structuredContent: { hits },
      };
    },
  );

  server.registerTool(
    "search_docs",
    {
      description: "Search local repo docs using semantic retrieval with persisted caches and deterministic fallback.",
      inputSchema: SearchQuerySchema,
    },
    async ({ query, cwd, limit }) => {
      const root = cwd ?? process.cwd();
      const hits = await contextService.searchDocs(root, query, limit);
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(hits, null, 2),
          },
        ],
        structuredContent: { hits },
      };
    },
  );

  server.registerTool(
    "search_skills",
    {
      description: "Search local skill libraries with semantic retrieval, the local skill router, and persisted caches.",
      inputSchema: SearchQuerySchema,
    },
    async ({ query, limit }) => {
      const hits = await contextService.searchSkills(query, limit);
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(hits, null, 2),
          },
        ],
        structuredContent: { hits },
      };
    },
  );

  server.registerTool(
    "search_mcp_servers",
    {
      description: "Search globally compatible MCP server inventories from Codex config and workspace mcp-settings files.",
      inputSchema: SearchQuerySchema,
    },
    async ({ query, cwd, limit }) => {
      const hits = await contextService.searchMcpServers(cwd, query, limit);
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(hits, null, 2),
          },
        ],
        structuredContent: { hits },
      };
    },
  );

  server.registerTool(
    "probe_mcp_servers",
    {
      description: "Probe configured MCP servers live and persist the latest reachability snapshot.",
      inputSchema: ProbeMcpServersInputSchema,
    },
    async ({ cwd }) => {
      const inventory = indexService.listMcpServerInventory(cwd);
      const payload = await mcpHealthService.probeServers(inventory);

      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(payload, null, 2),
          },
        ],
        structuredContent: payload,
      };
    },
  );

  server.registerTool(
    "get_context_artifact",
    {
      description: "Fetch a previously persisted context artifact by id.",
      inputSchema: {
        id: PrepareTaskContextInputSchema.shape.goal.describe("Artifact id"),
      },
    },
    async ({ id }) => {
      const artifact = artifacts.get(id);
      if (!artifact) {
        return {
          content: [
            {
              type: "text",
              text: JSON.stringify({ error: `Artifact not found: ${id}` }, null, 2),
            },
          ],
          isError: true,
        };
      }

      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(artifact, null, 2),
          },
        ],
        structuredContent: artifact,
      };
    },
  );

  server.registerTool(
    "list_recent_artifacts",
    {
      description: "List recent persisted context artifacts.",
      inputSchema: {
        limit: SearchQuerySchema.shape.limit,
      },
    },
    async ({ limit }) => {
      const rows = artifacts.listRecent(limit);
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(rows, null, 2),
          },
        ],
        structuredContent: { artifacts: rows },
      };
    },
  );

  server.registerTool(
    "invalidate_context_cache",
    {
      description: "Invalidate persisted cache entries for a repo scope or a global family such as skills or memory.",
      inputSchema: {
        scope: PrepareTaskContextInputSchema.shape.goal.describe("Cache invalidation scope"),
      },
    },
    async ({ scope }) => {
      const result = contextService.invalidate(scope);
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(result, null, 2),
          },
        ],
        structuredContent: {
          ...result,
        },
      };
    },
  );

  server.registerTool(
    "get_orchestrator_status",
    {
      description: "Report orchestrator health, semantic readiness, corpus freshness, MCP inventory status, and artifact/cache totals.",
      inputSchema: OrchestratorStatusInputSchema,
    },
    async ({ cwd }) => {
      const status = await indexService.getStatus(cwd);
      const inventory = indexService.listMcpServerInventory(cwd);
      const payload = {
        ...status,
        mcpHealth: mcpHealthService.getStatus(
          inventory,
          cwd ? cwd.replace(/[\\/:]+/g, "_").toLowerCase() : undefined,
        ),
      };

      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(payload, null, 2),
          },
        ],
        structuredContent: payload,
      };
    },
  );

  server.registerTool(
    "reindex_context_sources",
    {
      description: "Force reindexing for skills, memory, docs, MCP inventories, or all corpora and invalidate the matching caches.",
      inputSchema: ReindexInputSchema,
    },
    async ({ scope, cwd }) => {
      const repoRoot = cwd ?? process.cwd();
      const result = await indexService.reindex(scope, repoRoot);

      const invalidations =
        scope === "all"
          ? [
              contextService.invalidate("skills"),
              contextService.invalidate("memory"),
              contextService.invalidate("mcp_servers"),
              contextService.invalidate(repoRoot),
            ]
          : [contextService.invalidate(scope === "docs" ? repoRoot : scope)];

      const payload = {
        ...result,
        invalidations,
        status: await indexService.getStatus(scope === "docs" || scope === "all" ? repoRoot : cwd),
      };

      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(payload, null, 2),
          },
        ],
        structuredContent: payload,
      };
    },
  );

  return server;
}
