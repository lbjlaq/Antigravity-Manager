import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";

import type { QdrantClient } from "@qdrant/js-client-rest";

import type { OrchestratorConfig } from "../config.js";
import type {
  CompanionArtifact,
  IndexedDocument,
  McpServerInventoryEntry,
  McpServerTransport,
  SearchHit,
} from "../types.js";
import { CacheRepository } from "../storage/cache.js";
import { ArtifactRepository } from "../storage/index.js";
import {
  checkQdrantHealth,
  ensureCollection,
  getCollectionPointCount,
  searchCollection,
  upsertDocuments,
} from "./qdrant.js";
import { OpenAIService } from "./openai.js";

interface EmbeddedPoint {
  pointId: string;
  vector: number[];
  payload: Record<string, unknown>;
}

interface EmbeddedCacheValue {
  pointId: string;
  vector: number[];
  payload: Record<string, unknown>;
}

function hashText(value: string): string {
  return createHash("sha256").update(value, "utf8").digest("hex");
}

function normalizeRepoScope(repoRoot: string): string {
  return repoRoot.replace(/[\\/:]+/g, "_").toLowerCase();
}

const IGNORE_DIRS = new Set([
  ".git",
  "node_modules",
  ".next",
  ".venv",
  "dist",
  "build",
  "__pycache__",
  "tmp",
  ".codex-swarm",
]);

function shouldIgnorePath(filePath: string): boolean {
  return filePath.split(path.sep).some((segment) => IGNORE_DIRS.has(segment));
}

function chunkText(text: string, maxChars = 1600): string[] {
  const lines = text.split(/\r?\n/);
  const chunks: string[] = [];
  let current = "";

  for (const line of lines) {
    const next = current ? `${current}\n${line}` : line;
    if (next.length > maxChars && current) {
      chunks.push(current);
      current = line;
      continue;
    }
    current = next;
  }

  if (current.trim()) {
    chunks.push(current);
  }

  return chunks.length > 0 ? chunks : [text];
}

function walk(root: string): string[] {
  if (!fs.existsSync(root)) {
    return [];
  }

  const out: string[] = [];
  const stack = [root];
  while (stack.length > 0) {
    const current = stack.pop()!;
    if (shouldIgnorePath(current)) {
      continue;
    }
    const stat = fs.statSync(current);
    if (stat.isDirectory()) {
      for (const entry of fs.readdirSync(current)) {
        stack.push(path.join(current, entry));
      }
      continue;
    }

    out.push(current);
  }
  return out;
}

function collectSkillDocuments(skillRoots: string[]): IndexedDocument[] {
  return skillRoots
    .flatMap((root) => walk(root).filter((filePath) => filePath.endsWith("SKILL.md")))
    .map((filePath) => ({
      id: filePath,
      title: path.basename(path.dirname(filePath)),
      text: fs.readFileSync(filePath, "utf8").replace(/\s+/g, " ").trim().slice(0, 12000),
      path: filePath,
      collection: "skills",
      metadata: {
        title: path.basename(path.dirname(filePath)),
        path: filePath,
      },
    }));
}

function parseTomlString(raw: string): string | undefined {
  const match = raw.match(/^\s*(['"])(.*)\1\s*$/);
  return match?.[2];
}

function parseTomlArray(raw: string): string[] | undefined {
  const match = raw.match(/^\s*\[(.*)\]\s*$/);
  if (!match) {
    return undefined;
  }

  const values = match[1]
    .split(",")
    .map((item) => parseTomlString(item.trim()))
    .filter((item): item is string => Boolean(item));

  return values.length > 0 ? values : [];
}

function detectTransport(sourceKind: string, command?: string, url?: string): McpServerTransport {
  if (sourceKind === "docker_registry_json") {
    return "docker_registry";
  }
  if (url) {
    return "streamable_http";
  }
  if (command) {
    return "stdio";
  }
  return "unknown";
}

function inventoryToDocument(entry: McpServerInventoryEntry): IndexedDocument {
  return {
    id: entry.inventoryId,
    title: entry.title,
    text: entry.text ?? `${entry.name}\n${JSON.stringify(entry.metadata ?? {}, null, 2)}`,
    path: entry.sourcePath,
    collection: "mcp_servers",
    metadata: {
      title: entry.title,
      path: entry.sourcePath,
      serverName: entry.name,
      sourceKind: entry.sourceKind,
      repoRoot: entry.repoRoot,
      repoScope: entry.repoScope,
      transport: entry.transport,
      command: entry.command,
      args: entry.args,
      cwd: entry.cwd,
      url: entry.url,
      ...(entry.metadata ?? {}),
    },
  };
}

function parseCodexTomlMcpServers(filePath: string): McpServerInventoryEntry[] {
  const raw = fs.readFileSync(filePath, "utf8");
  const lines = raw.split(/\r?\n/);
  const entries: McpServerInventoryEntry[] = [];
  let currentName: string | undefined;
  let currentBody: string[] = [];

  const flush = () => {
    if (!currentName) {
      return;
    }
    const body = currentBody.join("\n").trim();
    const command = currentBody
      .map((line) => line.match(/^\s*command\s*=\s*(.+)\s*$/)?.[1])
      .find((value): value is string => Boolean(value));
    const args = currentBody
      .map((line) => line.match(/^\s*args\s*=\s*(.+)\s*$/)?.[1])
      .find((value): value is string => Boolean(value));
    const cwd = currentBody
      .map((line) => line.match(/^\s*cwd\s*=\s*(.+)\s*$/)?.[1])
      .find((value): value is string => Boolean(value));
    const url = currentBody
      .map((line) => line.match(/^\s*url\s*=\s*(.+)\s*$/)?.[1])
      .find((value): value is string => Boolean(value));

    const parsedCommand = command ? parseTomlString(command) : undefined;
    const parsedArgs = args ? parseTomlArray(args) : undefined;
    const parsedCwd = cwd ? parseTomlString(cwd) : undefined;
    const parsedUrl = url ? parseTomlString(url) : undefined;

    entries.push({
      inventoryId: `${filePath}#${currentName}`,
      name: currentName,
      title: currentName,
      sourcePath: filePath,
      sourceKind: "codex_toml",
      repoScope: "global",
      transport: detectTransport("codex_toml", parsedCommand, parsedUrl),
      command: parsedCommand,
      args: parsedArgs,
      cwd: parsedCwd,
      url: parsedUrl,
      text: `${currentName}\n${body}`.trim(),
      metadata: {
        title: currentName,
        path: filePath,
        serverName: currentName,
      },
    });
  };

  for (const line of lines) {
    const section = line.match(/^\[mcp_servers\.("?)([^"\]]+)\1\]\s*$/);
    if (section) {
      flush();
      currentName = section[2]?.trim();
      currentBody = [];
      continue;
    }

    if (/^\[[^\]]+\]\s*$/.test(line)) {
      flush();
      currentName = undefined;
      currentBody = [];
      continue;
    }

    if (currentName) {
      currentBody.push(line);
    }
  }

  flush();
  return entries;
}

function parseJsonMcpServers(filePath: string): McpServerInventoryEntry[] {
  const raw = fs.readFileSync(filePath, "utf8");
  const json = JSON.parse(raw) as Record<string, unknown>;
  const entries: McpServerInventoryEntry[] = [];

  const mcpServers =
    json.mcpServers && typeof json.mcpServers === "object"
      ? (json.mcpServers as Record<string, unknown>)
      : {};
  for (const [name, config] of Object.entries(mcpServers)) {
    const settings =
      config && typeof config === "object"
        ? (config as Record<string, unknown>)
        : {};
    const command = typeof settings.command === "string" ? settings.command : undefined;
    const args = Array.isArray(settings.args)
      ? settings.args.filter((item): item is string => typeof item === "string")
      : undefined;
    const cwd = typeof settings.cwd === "string" ? settings.cwd : undefined;
    const url = typeof settings.url === "string" ? settings.url : undefined;

    entries.push({
      inventoryId: `${filePath}#${name}`,
      name,
      title: name,
      sourcePath: filePath,
      sourceKind: "mcpServers_json",
      repoScope: "global",
      transport: detectTransport("mcpServers_json", command, url),
      command,
      args,
      cwd,
      url,
      text: `${name}\n${JSON.stringify(config, null, 2)}`,
      metadata: {
        title: name,
        path: filePath,
        serverName: name,
        ...(settings ?? {}),
      },
    });
  }

  const dockerRegistry =
    json.dockerRegistry && typeof json.dockerRegistry === "object"
      ? (json.dockerRegistry as Record<string, unknown>)
      : undefined;
  const dockerServers =
    dockerRegistry?.servers && typeof dockerRegistry.servers === "object"
      ? (dockerRegistry.servers as Record<string, unknown>)
      : {};
  for (const [name, config] of Object.entries(dockerServers)) {
    entries.push({
      inventoryId: `${filePath}#docker:${name}`,
      name,
      title: `docker:${name}`,
      sourcePath: filePath,
      sourceKind: "docker_registry_json",
      repoScope: "global",
      transport: "docker_registry",
      text: `docker:${name}\n${JSON.stringify(config, null, 2)}`,
      metadata: {
        title: `docker:${name}`,
        path: filePath,
        serverName: name,
        ...(config && typeof config === "object" ? (config as Record<string, unknown>) : {}),
      },
    });
  }

  return entries;
}

function candidateMcpConfigPaths(config: OrchestratorConfig, repoRoot?: string): string[] {
  const out = [...config.mcpConfigPaths];
  if (repoRoot) {
    out.push(path.join(repoRoot, "mcp-settings.json"));
    out.push(path.join(repoRoot, "mcp.json"));
  }
  return Array.from(new Set(out));
}

export function listMcpServerInventory(
  configPaths: string[],
  repoRoot?: string,
): McpServerInventoryEntry[] {
  return Array.from(new Set(configPaths))
    .filter((filePath) => fs.existsSync(filePath))
    .flatMap((filePath) => {
      try {
        if (filePath.toLowerCase().endsWith(".json")) {
          return parseJsonMcpServers(filePath);
        }
        if (filePath.toLowerCase().endsWith(".toml")) {
          return parseCodexTomlMcpServers(filePath);
        }
      } catch {
        return [];
      }
      return [];
    })
    .map((entry) => ({
      ...entry,
      repoRoot,
      repoScope: repoRoot ? normalizeRepoScope(repoRoot) : "global",
      metadata: {
        ...(entry.metadata ?? {}),
        repoRoot,
        repoScope: repoRoot ? normalizeRepoScope(repoRoot) : "global",
      },
    }));
}

export function listMcpServers(
  configPaths: string[],
  repoRoot?: string,
): IndexedDocument[] {
  return listMcpServerInventory(configPaths, repoRoot).map(inventoryToDocument);
}

export function listRepoDocs(repoRoot: string): IndexedDocument[] {
  const docsRoot = path.join(repoRoot, "docs");
  return walk(docsRoot)
    .filter((filePath) => /\.(md|txt)$/i.test(filePath))
    .flatMap((filePath) => {
      const relativePath = path.relative(repoRoot, filePath);
      const raw = fs.readFileSync(filePath, "utf8").slice(0, 24000);
      return chunkText(raw).map((chunk, index, chunks) => ({
        id: `${filePath}#${index}`,
        title: chunks.length > 1 ? `${relativePath} (chunk ${index + 1}/${chunks.length})` : relativePath,
        text: chunk.replace(/\s+/g, " ").trim(),
        path: filePath,
        collection: "repo_docs" as const,
        metadata: {
          repoRoot,
          repoScope: normalizeRepoScope(repoRoot),
          title: relativePath,
          path: filePath,
          sourceDocumentId: filePath,
          chunkIndex: index,
          chunkCount: chunks.length,
        },
      }));
    });
}

function buildSessionDocuments(
  artifacts: ArtifactRepository,
  limit: number,
): IndexedDocument[] {
  return artifacts.listRecent(limit).map((row) => ({
    id: row.id,
    title: row.summary,
    text: `${row.summary}\n${row.explanation}`.replace(/\s+/g, " ").trim().slice(0, 12000),
    path: row.file_path,
    collection: "session_summaries",
    metadata: {
      artifactId: row.id,
      title: row.summary,
      path: row.file_path,
      createdAt: row.created_at,
    },
  }));
}

function cacheScopeFor(collectionName: string, repoRoot?: string): string {
  if (!repoRoot) {
    return `embedding:${collectionName}`;
  }

  return `embedding:${collectionName}:${normalizeRepoScope(repoRoot)}`;
}

function pointIdFor(document: IndexedDocument): string {
  return hashText(`${document.collection}:${document.id}`);
}

function collectionNameFor(
  config: OrchestratorConfig,
  collection: IndexedDocument["collection"],
): string {
  switch (collection) {
    case "skills":
      return config.qdrantCollections.skills;
    case "session_summaries":
      return config.qdrantCollections.sessionSummaries;
    case "repo_docs":
      return config.qdrantCollections.repoDocs;
    case "mcp_servers":
      return config.qdrantCollections.mcpServers;
  }
}

function hitFromPayload(
  kind: SearchHit["kind"],
  point: Awaited<ReturnType<typeof searchCollection>>[number],
): SearchHit | undefined {
  const payload = point.payload as Record<string, unknown> | undefined;
  if (!payload) {
    return undefined;
  }

  const title = typeof payload.title === "string" ? payload.title : "Untitled";
  const snippet = typeof payload.text === "string" ? payload.text.slice(0, 240) : "";
  const pathValue = typeof payload.path === "string" ? payload.path : undefined;
  const idValue =
    typeof payload.documentId === "string"
      ? payload.documentId
      : typeof point.id === "string"
        ? point.id
        : String(point.id);

  return {
    id: idValue,
    kind,
    title,
    snippet,
    path: pathValue,
    score: typeof point.score === "number" ? point.score : 0,
  };
}

export class IndexService {
  constructor(
    private readonly config: OrchestratorConfig,
    private readonly qdrant: QdrantClient,
    private readonly openai: OpenAIService,
    private readonly cache: CacheRepository,
    private readonly artifacts: ArtifactRepository,
  ) {}

  isSemanticReady(): boolean {
    return this.openai.isConfigured();
  }

  async bootstrap(): Promise<void> {
    await this.ingestSkills();
    await this.ingestSessionSummaries();
    await this.ingestMcpServers();
  }

  getMcpConfigPaths(repoRoot?: string): string[] {
    return candidateMcpConfigPaths(this.config, repoRoot);
  }

  listMcpServerInventory(repoRoot?: string): McpServerInventoryEntry[] {
    return listMcpServerInventory(this.getMcpConfigPaths(repoRoot), repoRoot);
  }

  listMcpServerDocuments(repoRoot?: string): IndexedDocument[] {
    return listMcpServers(this.getMcpConfigPaths(repoRoot), repoRoot);
  }

  async ingestSkills(): Promise<void> {
    await this.indexDocuments(collectSkillDocuments(this.config.skillRoots), undefined, "skills");
  }

  async ingestSessionSummaries(limit = 150): Promise<void> {
    await this.indexDocuments(buildSessionDocuments(this.artifacts, limit), undefined, "memory");
  }

  async ingestArtifact(artifact: CompanionArtifact, filePath?: string): Promise<void> {
    await this.indexDocuments([
      {
        id: artifact.id,
        title: artifact.summary,
        text: `${artifact.summary}\n${artifact.explanation}`.replace(/\s+/g, " ").trim().slice(0, 12000),
        path: filePath ?? artifact.id,
        collection: "session_summaries",
        metadata: {
          artifactId: artifact.id,
          title: artifact.summary,
          path: filePath ?? artifact.id,
          createdAt: artifact.created_at,
        },
      },
    ], undefined, "memory");
  }

  async ingestRepoDocs(repoRoot: string): Promise<void> {
    await this.indexDocuments(listRepoDocs(repoRoot), repoRoot, "docs");
  }

  async ingestMcpServers(repoRoot?: string): Promise<void> {
    await this.indexDocuments(
      this.listMcpServerDocuments(repoRoot),
      repoRoot,
      "mcp_servers",
    );
  }

  async searchSkills(query: string, limit: number): Promise<SearchHit[]> {
    await this.ingestSkills();
    return this.semanticSearch(this.config.qdrantCollections.skills, "skill", query, limit);
  }

  async searchSessionSummaries(query: string, limit: number): Promise<SearchHit[]> {
    await this.ingestSessionSummaries();
    return this.semanticSearch(
      this.config.qdrantCollections.sessionSummaries,
      "memory",
      query,
      limit,
    );
  }

  async searchRepoDocs(repoRoot: string, query: string, limit: number): Promise<SearchHit[]> {
    await this.ingestRepoDocs(repoRoot);
    const repoScope = normalizeRepoScope(repoRoot);
    return this.semanticSearch(this.config.qdrantCollections.repoDocs, "doc", query, limit, {
      must: [
        {
          key: "repoScope",
          match: {
            value: repoScope,
          },
        },
      ],
    });
  }

  async searchMcpServers(repoRoot: string | undefined, query: string, limit: number): Promise<SearchHit[]> {
    await this.ingestMcpServers(repoRoot);
    return this.semanticSearch(this.config.qdrantCollections.mcpServers, "mcp_server", query, limit, {
      must: [
        {
          key: "repoScope",
          match: {
            value: repoRoot ? normalizeRepoScope(repoRoot) : "global",
          },
        },
      ],
    });
  }

  async reindex(scope: "skills" | "memory" | "docs" | "mcp_servers" | "all", repoRoot?: string): Promise<{
    scope: "skills" | "memory" | "docs" | "mcp_servers" | "all";
    repoRoot?: string;
    skillsIndexed: boolean;
    memoryIndexed: boolean;
    docsIndexed: boolean;
    mcpServersIndexed: boolean;
  }> {
    let skillsIndexed = false;
    let memoryIndexed = false;
    let docsIndexed = false;
    let mcpServersIndexed = false;

    if (scope === "skills" || scope === "all") {
      await this.ingestSkills();
      skillsIndexed = true;
    }

    if (scope === "memory" || scope === "all") {
      await this.ingestSessionSummaries();
      memoryIndexed = true;
    }

    if ((scope === "docs" || scope === "all") && repoRoot) {
      await this.ingestRepoDocs(repoRoot);
      docsIndexed = true;
    }

    if (scope === "mcp_servers" || scope === "all") {
      await this.ingestMcpServers(repoRoot);
      mcpServersIndexed = true;
    }

    return {
      scope,
      repoRoot,
      skillsIndexed,
      memoryIndexed,
      docsIndexed,
      mcpServersIndexed,
    };
  }

  async getStatus(repoRoot?: string): Promise<{
    semanticReady: boolean;
    qdrant: { ok: boolean; collectionCount?: number; error?: string };
    dashboard: {
      artifactsTotal: number;
      latestArtifactAt?: string;
      cacheEntries: number;
    };
      collections: {
        skills: CollectionStatus;
        sessionSummaries: CollectionStatus;
        repoDocs: CollectionStatus;
        mcpServers: CollectionStatus;
      };
  }> {
    const qdrant = await checkQdrantHealth(this.qdrant);
    const [skillsPoints, sessionPoints, repoDocsPoints, mcpServersPoints] = await Promise.all([
      getCollectionPointCount(this.qdrant, this.config.qdrantCollections.skills),
      getCollectionPointCount(this.qdrant, this.config.qdrantCollections.sessionSummaries),
      getCollectionPointCount(this.qdrant, this.config.qdrantCollections.repoDocs),
      getCollectionPointCount(this.qdrant, this.config.qdrantCollections.mcpServers),
    ]);

    const skillsRun = this.artifacts.getIndexRun("skills");
    const sessionRun = this.artifacts.getIndexRun("memory");
    const repoRun = repoRoot
      ? this.artifacts.getIndexRun(`docs:${normalizeRepoScope(repoRoot)}`)
      : this.artifacts.latestIndexRunByPrefix("docs");
    const mcpRun = repoRoot
      ? this.artifacts.getIndexRun(`mcp_servers:${normalizeRepoScope(repoRoot)}`)
      : this.artifacts.latestIndexRunByPrefix("mcp_servers");

    return {
      semanticReady: this.isSemanticReady(),
      qdrant,
      dashboard: {
        artifactsTotal: this.artifacts.count(),
        latestArtifactAt: this.artifacts.latestCreatedAt(),
        cacheEntries: this.cache.count(),
      },
      collections: {
        skills: {
          name: this.config.qdrantCollections.skills,
          points: skillsPoints,
          freshness: describeFreshness(skillsRun),
        },
        sessionSummaries: {
          name: this.config.qdrantCollections.sessionSummaries,
          points: sessionPoints,
          freshness: describeFreshness(sessionRun),
        },
        repoDocs: {
          name: this.config.qdrantCollections.repoDocs,
          points: repoDocsPoints,
          repoRoot,
          freshness: describeFreshness(repoRun),
        },
        mcpServers: {
          name: this.config.qdrantCollections.mcpServers,
          points: mcpServersPoints,
          repoRoot,
          freshness: describeFreshness(mcpRun),
        },
      },
    };
  }

  private async indexDocuments(
    documents: IndexedDocument[],
    repoRoot?: string,
    runScope?: "skills" | "memory" | "docs" | "mcp_servers",
  ): Promise<void> {
    if (!this.isSemanticReady() || documents.length === 0) {
      return;
    }

    const collectionName = collectionNameFor(this.config, documents[0].collection);
    const embeddingScope = cacheScopeFor(collectionName, repoRoot);
    const prepared: EmbeddedPoint[] = [];
    const pending: IndexedDocument[] = [];

    for (const document of documents) {
      const versionToken = hashText(`${document.text}:${JSON.stringify(document.metadata ?? {})}`);
      const cacheKey = this.cache.buildKey(embeddingScope, [document.id], versionToken);
      const cached = this.cache.get<EmbeddedCacheValue>(cacheKey, versionToken);
      if (cached?.value?.vector?.length) {
        prepared.push({
          pointId: cached.value.pointId,
          vector: cached.value.vector,
          payload: cached.value.payload,
        });
        continue;
      }

      pending.push(document);
    }

    for (let index = 0; index < pending.length; index += 32) {
      const batch = pending.slice(index, index + 32);
      const vectors = await this.openai.createEmbeddings(
        batch.map((item) => item.text),
        this.config.embeddingModel,
      );

      for (const [offset, document] of batch.entries()) {
        const vector = vectors[offset];
        const pointId = pointIdFor(document);
        const payload = {
          collection: document.collection,
          documentId: document.id,
          title: document.title,
          text: document.text,
          path: document.path,
          ...(document.metadata ?? {}),
        };
        const versionToken = hashText(`${document.text}:${JSON.stringify(document.metadata ?? {})}`);
        const cacheKey = this.cache.buildKey(embeddingScope, [document.id], versionToken);
        this.cache.set<EmbeddedCacheValue>(embeddingScope, cacheKey, versionToken, {
          pointId,
          vector,
          payload,
        });
        prepared.push({
          pointId,
          vector,
          payload,
        });
      }
    }

    if (prepared.length === 0) {
      return;
    }

    await ensureCollection(this.qdrant, collectionName, prepared[0].vector.length);
    await upsertDocuments(
      this.qdrant,
      collectionName,
      prepared.map((item) => ({
        id: item.pointId,
        vector: item.vector,
        payload: item.payload,
        })),
    );

    this.recordIndexRun(runScope ?? documents[0].collection, documents, repoRoot);
  }

  private async semanticSearch(
    collectionName: string,
    kind: SearchHit["kind"],
    query: string,
    limit: number,
    filter?: Record<string, unknown>,
  ): Promise<SearchHit[]> {
    if (!this.isSemanticReady()) {
      return [];
    }

    try {
      const [vector] = await this.openai.createEmbeddings([query], this.config.embeddingModel);
      const points = await searchCollection(this.qdrant, collectionName, vector, limit, filter);
      return points
        .map((point) => hitFromPayload(kind, point))
        .filter((item): item is SearchHit => Boolean(item));
    } catch {
      return [];
    }
  }

  private recordIndexRun(
    scope: "skills" | "memory" | "docs" | "mcp_servers" | IndexedDocument["collection"],
    documents: IndexedDocument[],
    repoRoot?: string,
  ): void {
    const normalizedScope =
      scope === "docs" || scope === "repo_docs"
        ? `docs:${normalizeRepoScope(repoRoot ?? "unknown")}`
        : scope === "mcp_servers"
          ? `mcp_servers:${normalizeRepoScope(repoRoot ?? "global")}`
        : scope === "memory" || scope === "session_summaries"
          ? "memory"
          : "skills";

    this.artifacts.recordIndexRun({
      scope: normalizedScope,
      updated_at: new Date().toISOString(),
      document_count: new Set(
        documents.map((document) =>
          typeof document.metadata?.sourceDocumentId === "string"
            ? document.metadata.sourceDocumentId
            : document.id,
        ),
      ).size,
      chunk_count: documents.length,
      embedding_model: this.config.embeddingModel,
      repo_root: repoRoot ?? null,
    });
  }
}

interface CollectionFreshness {
  lastIndexedAt?: string;
  ageSeconds?: number;
  documentCount?: number;
  chunkCount?: number;
  embeddingModel?: string;
  stale: boolean;
}

interface CollectionStatus {
  name: string;
  points: number;
  repoRoot?: string;
  freshness: CollectionFreshness;
}

function describeFreshness(
  run:
    | {
        updated_at: string;
        document_count: number;
        chunk_count: number;
        embedding_model: string;
      }
    | undefined,
): CollectionFreshness {
  if (!run) {
    return {
      stale: true,
    };
  }

  const ageSeconds = Math.max(
    0,
    Math.floor((Date.now() - new Date(run.updated_at).getTime()) / 1000),
  );

  return {
    lastIndexedAt: run.updated_at,
    ageSeconds,
    documentCount: run.document_count,
    chunkCount: run.chunk_count,
    embeddingModel: run.embedding_model,
    stale: ageSeconds > 3600,
  };
}
