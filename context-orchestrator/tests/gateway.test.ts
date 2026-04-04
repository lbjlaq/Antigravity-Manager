import assert from "node:assert/strict";
import test from "node:test";

import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";

import { createGateway } from "../src/gateway/server.js";
import type { CompanionArtifact, ContextPack, SearchHit } from "../src/types.js";

function createArtifact(id: string, summary: string): CompanionArtifact {
  return {
    id,
    mode: "plan",
    task_class: "architecture",
    summary,
    explanation: `${summary} explanation`,
    recommended_actions: [],
    risks: [],
    questions: [],
    evidence: [],
    confidence: 0.8,
    created_at: new Date().toISOString(),
  };
}

async function createHarness() {
  const savedArtifacts = new Map<string, CompanionArtifact>();
  const counters = {
    plannerCalls: 0,
    ingestArtifactCalls: 0,
  };

  const contextPack: ContextPack = {
    taskClass: "architecture",
    selectedSkills: [
      {
        id: "skill-1",
        kind: "skill",
        title: "search_skills",
        snippet: "Skill hit",
        score: 0.9,
      },
    ],
    mcpServerHits: [
      {
        id: "mcp-1",
        kind: "mcp_server",
        title: "filesystem",
        snippet: "MCP server hit",
        score: 0.91,
      },
    ],
    selectedTools: ["search_docs", "plan_or_review"],
    memoryHits: [],
    docHits: [],
    cacheHit: false,
  };

  const contextService = {
    classify: () => "architecture",
    prepareContext: async (_goal: string, _cwd: string, plannerArtifactId?: string) => ({
      ...contextPack,
      plannerArtifactId,
    }),
    searchMemory: async (query: string, limit: number): Promise<SearchHit[]> => [
      {
        id: `memory:${query}:${limit}`,
        kind: "memory",
        title: "Memory hit",
        snippet: query,
        score: 0.7,
      },
    ],
    searchDocs: async (_cwd: string, query: string, limit: number): Promise<SearchHit[]> => [
      {
        id: `doc:${query}:${limit}`,
        kind: "doc",
        title: "Doc hit",
        snippet: query,
        score: 0.6,
      },
    ],
    searchSkills: async (query: string, limit: number): Promise<SearchHit[]> => [
      {
        id: `skill:${query}:${limit}`,
        kind: "skill",
        title: "Skill hit",
        snippet: query,
        score: 0.8,
      },
    ],
    searchMcpServers: async (_cwd: string | undefined, query: string, limit: number): Promise<SearchHit[]> => [
      {
        id: `mcp:${query}:${limit}`,
        kind: "mcp_server",
        title: "filesystem",
        snippet: query,
        score: 0.75,
      },
    ],
    invalidate: (scope: string) => ({
      invalidated: true,
      scope,
      deletedCount: scope === "skills" ? 2 : 3,
    }),
  };

  const plannerService = {
    buildArtifact: async ({ mode, taskDescription }: { mode: "plan" | "review" | "auto"; taskDescription: string }) => {
      counters.plannerCalls += 1;
      return createArtifact(`${mode}-${counters.plannerCalls}`, taskDescription);
    },
  };

  const indexService = {
    ingestArtifact: async () => {
      counters.ingestArtifactCalls += 1;
    },
    getStatus: async (cwd?: string) => ({
      semanticReady: true,
      qdrant: {
        ok: true,
        collectionCount: 3,
      },
      dashboard: {
        artifactsTotal: savedArtifacts.size,
        latestArtifactAt: Array.from(savedArtifacts.values()).at(-1)?.created_at,
        cacheEntries: 5,
      },
      collections: {
        skills: {
          name: "skills",
          points: 10,
          freshness: {
            lastIndexedAt: "2026-04-04T10:00:00.000Z",
            ageSeconds: 120,
            documentCount: 2,
            chunkCount: 2,
            embeddingModel: "text-embedding-3-small",
            stale: false,
          },
        },
        sessionSummaries: {
          name: "memory",
          points: 4,
          freshness: {
            lastIndexedAt: "2026-04-04T10:05:00.000Z",
            ageSeconds: 90,
            documentCount: 1,
            chunkCount: 1,
            embeddingModel: "text-embedding-3-small",
            stale: false,
          },
        },
        repoDocs: {
          name: "docs",
          points: 12,
          repoRoot: cwd,
          freshness: {
            lastIndexedAt: "2026-04-04T10:06:00.000Z",
            ageSeconds: 60,
            documentCount: 3,
            chunkCount: 7,
            embeddingModel: "text-embedding-3-small",
            stale: false,
          },
        },
        mcpServers: {
          name: "mcp",
          points: 6,
          repoRoot: cwd,
          freshness: {
            lastIndexedAt: "2026-04-04T10:07:00.000Z",
            ageSeconds: 30,
            documentCount: 2,
            chunkCount: 2,
            embeddingModel: "text-embedding-3-small",
            stale: false,
          },
        },
      },
    }),
    reindex: async (scope: "skills" | "memory" | "docs" | "all", repoRoot?: string) => ({
      scope,
      repoRoot,
      skillsIndexed: scope === "skills" || scope === "all",
      memoryIndexed: scope === "memory" || scope === "all",
      docsIndexed: scope === "docs" || scope === "all",
    }),
  };

  const artifacts = {
    save: (artifact: CompanionArtifact) => {
      savedArtifacts.set(artifact.id, artifact);
      return `/tmp/${artifact.id}.json`;
    },
    get: (id: string) => savedArtifacts.get(id),
    listRecent: (limit: number) =>
      Array.from(savedArtifacts.values())
        .slice(-limit)
        .reverse()
        .map((artifact) => ({
          id: artifact.id,
          mode: artifact.mode,
          task_class: artifact.task_class,
          summary: artifact.summary,
          explanation: artifact.explanation,
          confidence: artifact.confidence,
          created_at: artifact.created_at,
          repo_id: artifact.repo_id ?? null,
          profile_id: artifact.profile_id ?? null,
          file_path: `/tmp/${artifact.id}.json`,
        })),
    count: () => savedArtifacts.size,
  };

  const server = createGateway(
    contextService as never,
    plannerService as never,
    indexService as never,
    artifacts as never,
  );
  const client = new Client({
    name: "context-orchestrator-test-client",
    version: "0.1.0",
  });
  const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
  await Promise.all([server.connect(serverTransport), client.connect(clientTransport)]);

  return { client, counters };
}

test("gateway exposes the expected MCP tools", async () => {
  const { client } = await createHarness();
  const result = await client.listTools();
  const names = result.tools.map((tool) => tool.name).sort();

  assert.deepEqual(names, [
    "get_context_artifact",
    "get_orchestrator_status",
    "invalidate_context_cache",
    "list_recent_artifacts",
    "plan_or_review",
    "prepare_task_context",
    "reindex_context_sources",
    "search_docs",
    "search_mcp_servers",
    "search_memory",
    "search_skills",
    "submit_memory_summary",
  ]);
});

test("gateway returns planner-backed prepared context and status freshness payloads", async () => {
  const { client, counters } = await createHarness();

  const prepared = await client.callTool({
    name: "prepare_task_context",
    arguments: {
      goal: "architecture review for context service",
      cwd: "C:/repo",
      taskHints: ["cache"],
      changedFiles: ["src/services/context-service.ts"],
    },
  });

  const preparedPayload = prepared.structuredContent as { plannerArtifactId?: string; taskClass: string; mcpServerHits: Array<{ title: string }> };
  assert.equal(preparedPayload.taskClass, "architecture");
  assert.ok(preparedPayload.plannerArtifactId);
  assert.equal(preparedPayload.mcpServerHits[0]?.title, "filesystem");
  assert.equal(counters.plannerCalls, 1);
  assert.equal(counters.ingestArtifactCalls, 1);

  const status = await client.callTool({
    name: "get_orchestrator_status",
    arguments: {
      cwd: "C:/repo",
    },
  });

  const statusPayload = status.structuredContent as {
    dashboard: { artifactsTotal: number; cacheEntries: number };
    collections: {
      repoDocs: {
        freshness: {
          chunkCount: number;
          stale: boolean;
        };
      };
      mcpServers: {
        freshness: {
          documentCount: number;
        };
      };
    };
  };

  assert.equal(statusPayload.dashboard.cacheEntries, 5);
  assert.equal(statusPayload.collections.repoDocs.freshness.chunkCount, 7);
  assert.equal(statusPayload.collections.repoDocs.freshness.stale, false);
  assert.equal(statusPayload.collections.mcpServers.freshness.documentCount, 2);
});

test("gateway reindex tool returns invalidations plus refreshed status", async () => {
  const { client } = await createHarness();

  const result = await client.callTool({
    name: "reindex_context_sources",
    arguments: {
      scope: "docs",
      cwd: "C:/repo",
    },
  });

  const payload = result.structuredContent as {
    docsIndexed: boolean;
    invalidations: Array<{ scope: string; deletedCount: number }>;
    status: {
      collections: {
        repoDocs: {
          repoRoot?: string;
        };
      };
    };
  };

  assert.equal(payload.docsIndexed, true);
  assert.equal(payload.invalidations.length, 1);
  assert.equal(payload.invalidations[0]?.scope, "C:/repo");
  assert.equal(payload.status.collections.repoDocs.repoRoot, "C:/repo");
});

test("gateway can ingest client-agnostic memory summaries", async () => {
  const { client, counters } = await createHarness();

  const result = await client.callTool({
    name: "submit_memory_summary",
    arguments: {
      source: "codex",
      summary: "Prefer repo-scoped invalidation for docs caches",
      details: "This avoids evicting skills and memory unnecessarily.",
      category: "decision",
      relatedFiles: ["src/services/context-service.ts"],
      cwd: "C:/repo",
      repoId: "repo-1",
      profileId: "profile-1",
    },
  });

  const payload = result.structuredContent as {
    artifact: {
      source_client: string;
      memory_category: string;
      memory_status: string;
      related_files: string[];
    };
  };

  assert.equal(payload.artifact.source_client, "codex");
  assert.equal(payload.artifact.memory_category, "decision");
  assert.equal(payload.artifact.memory_status, "NEW");
  assert.equal(payload.artifact.related_files[0], "src/services/context-service.ts");
  assert.equal(counters.ingestArtifactCalls, 1);
});
