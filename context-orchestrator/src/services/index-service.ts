import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";

import type { QdrantClient } from "@qdrant/js-client-rest";

import type { OrchestratorConfig } from "../config.js";
import type { CompanionArtifact, IndexedDocument, SearchHit } from "../types.js";
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

function walk(root: string): string[] {
  if (!fs.existsSync(root)) {
    return [];
  }

  const out: string[] = [];
  const stack = [root];
  while (stack.length > 0) {
    const current = stack.pop()!;
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

export function listRepoDocs(repoRoot: string): IndexedDocument[] {
  const docsRoot = path.join(repoRoot, "docs");
  return walk(docsRoot)
    .filter((filePath) => /\.(md|txt)$/i.test(filePath))
    .map((filePath) => ({
      id: filePath,
      title: path.relative(repoRoot, filePath),
      text: fs.readFileSync(filePath, "utf8").replace(/\s+/g, " ").trim().slice(0, 12000),
      path: filePath,
      collection: "repo_docs",
      metadata: {
        repoRoot,
        repoScope: normalizeRepoScope(repoRoot),
        title: path.relative(repoRoot, filePath),
        path: filePath,
      },
    }));
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
  }

  async ingestSkills(): Promise<void> {
    await this.indexDocuments(collectSkillDocuments(this.config.skillRoots));
  }

  async ingestSessionSummaries(limit = 150): Promise<void> {
    await this.indexDocuments(buildSessionDocuments(this.artifacts, limit));
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
    ]);
  }

  async ingestRepoDocs(repoRoot: string): Promise<void> {
    await this.indexDocuments(listRepoDocs(repoRoot), repoRoot);
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

  async reindex(scope: "skills" | "memory" | "docs" | "all", repoRoot?: string): Promise<{
    scope: "skills" | "memory" | "docs" | "all";
    repoRoot?: string;
    skillsIndexed: boolean;
    memoryIndexed: boolean;
    docsIndexed: boolean;
  }> {
    let skillsIndexed = false;
    let memoryIndexed = false;
    let docsIndexed = false;

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

    return {
      scope,
      repoRoot,
      skillsIndexed,
      memoryIndexed,
      docsIndexed,
    };
  }

  async getStatus(repoRoot?: string): Promise<{
    semanticReady: boolean;
    qdrant: { ok: boolean; collectionCount?: number; error?: string };
    collections: {
      skills: { name: string; points: number };
      sessionSummaries: { name: string; points: number };
      repoDocs: { name: string; points: number; repoRoot?: string };
    };
  }> {
    const qdrant = await checkQdrantHealth(this.qdrant);
    const [skillsPoints, sessionPoints, repoDocsPoints] = await Promise.all([
      getCollectionPointCount(this.qdrant, this.config.qdrantCollections.skills),
      getCollectionPointCount(this.qdrant, this.config.qdrantCollections.sessionSummaries),
      getCollectionPointCount(this.qdrant, this.config.qdrantCollections.repoDocs),
    ]);

    return {
      semanticReady: this.isSemanticReady(),
      qdrant,
      collections: {
        skills: {
          name: this.config.qdrantCollections.skills,
          points: skillsPoints,
        },
        sessionSummaries: {
          name: this.config.qdrantCollections.sessionSummaries,
          points: sessionPoints,
        },
        repoDocs: {
          name: this.config.qdrantCollections.repoDocs,
          points: repoDocsPoints,
          repoRoot,
        },
      },
    };
  }

  private async indexDocuments(documents: IndexedDocument[], repoRoot?: string): Promise<void> {
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
}
