import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import type { ContextPack, InvalidationResult, SearchHit, TaskClass } from "../types.js";
import { CacheRepository } from "../storage/cache.js";
import { ArtifactRepository } from "../storage/index.js";
import { IndexService } from "./index-service.js";

function scoreText(query: string, haystack: string): number {
  const q = query.toLowerCase();
  const h = haystack.toLowerCase();
  if (!q || !h) {
    return 0;
  }

  let score = 0;
  for (const token of q.split(/\s+/).filter(Boolean)) {
    if (h.includes(token)) {
      score += 1;
    }
  }

  return score;
}

function hashParts(parts: string[]): string {
  return createHash("sha256").update(parts.join("::"), "utf8").digest("hex");
}

function walkFiles(root: string, matcher: (filePath: string) => boolean): string[] {
  if (!fs.existsSync(root)) {
    return [];
  }

  const results: string[] = [];
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

    if (matcher(current)) {
      results.push(current);
    }
  }

  return results;
}

function fileVersionToken(files: string[]): string {
  const fingerprints = files
    .map((filePath) => {
      const stat = fs.statSync(filePath);
      return `${filePath}:${stat.size}:${stat.mtimeMs}`;
    })
    .sort();
  return hashParts(fingerprints);
}

function mergeHits(primary: SearchHit[], secondary: SearchHit[], limit: number): SearchHit[] {
  const merged = new Map<string, SearchHit>();
  for (const item of [...primary, ...secondary]) {
    const key = `${item.kind}:${item.id}`;
    if (!merged.has(key)) {
      merged.set(key, item);
    }
  }

  return Array.from(merged.values()).slice(0, limit);
}

function repoScope(repoRoot: string): string {
  return repoRoot.replace(/[\\/:]+/g, "_").toLowerCase();
}

export function classifyTask(goal: string): TaskClass {
  const normalized = goal.toLowerCase();

  if (normalized.includes("review")) return "review";
  if (normalized.includes("design") || normalized.includes("architecture")) return "architecture";
  if (normalized.includes("debug") || normalized.includes("error")) return "debugging";
  if (normalized.includes("doc")) return "docs";
  if (normalized.includes("research")) return "research";
  return "coding";
}

function toolShortlist(taskClass: TaskClass): string[] {
  switch (taskClass) {
    case "architecture":
      return ["search_docs", "search_skills", "search_mcp_servers", "plan_or_review"];
    case "review":
      return ["search_memory", "search_docs", "search_mcp_servers", "plan_or_review"];
    case "debugging":
      return ["search_memory", "search_docs", "search_mcp_servers", "prepare_task_context"];
    default:
      return ["search_skills", "search_docs", "search_mcp_servers", "search_memory"];
  }
}

export class ContextService {
  constructor(
    private readonly skillRoots: string[],
    private readonly artifacts: ArtifactRepository,
    private readonly cache: CacheRepository,
    private readonly indexService: IndexService,
    private readonly collectionNames: {
      skills: string;
      sessionSummaries: string;
      repoDocs: string;
      mcpServers: string;
    },
  ) {}

  classify(goal: string): TaskClass {
    return classifyTask(goal);
  }

  async searchSkills(query: string, limit: number): Promise<SearchHit[]> {
    const scope = "search:skills";
    const versionToken = this.skillsVersion();
    const cacheKey = this.cache.buildKey(scope, [query, String(limit)], versionToken);
    const cached = this.cache.get<SearchHit[]>(cacheKey, versionToken);
    if (cached) {
      return cached.value;
    }

    const semanticHits = await this.indexService.searchSkills(query, limit);
    const deterministicHits = this.searchSkillsFallback(query, limit);
    const hits = mergeHits(semanticHits, deterministicHits, limit);
    this.cache.set(scope, cacheKey, versionToken, hits);
    return hits;
  }

  async searchDocs(repoRoot: string, query: string, limit: number): Promise<SearchHit[]> {
    const scope = `search:docs:${repoScope(repoRoot)}`;
    const versionToken = this.docsVersion(repoRoot);
    const cacheKey = this.cache.buildKey(scope, [query, String(limit)], versionToken);
    const cached = this.cache.get<SearchHit[]>(cacheKey, versionToken);
    if (cached) {
      return cached.value;
    }

    const semanticHits = await this.indexService.searchRepoDocs(repoRoot, query, limit);
    const deterministicHits = this.searchDocsFallback(repoRoot, query, limit);
    const hits = mergeHits(semanticHits, deterministicHits, limit);
    this.cache.set(scope, cacheKey, versionToken, hits);
    return hits;
  }

  async searchMemory(query: string, limit: number): Promise<SearchHit[]> {
    const scope = "search:memory";
    const versionToken = this.memoryVersion();
    const cacheKey = this.cache.buildKey(scope, [query, String(limit)], versionToken);
    const cached = this.cache.get<SearchHit[]>(cacheKey, versionToken);
    if (cached) {
      return cached.value;
    }

    const semanticHits = await this.indexService.searchSessionSummaries(query, limit);
    const deterministicHits = this.searchMemoryFallback(query, limit);
    const hits = mergeHits(semanticHits, deterministicHits, limit);
    this.cache.set(scope, cacheKey, versionToken, hits);
    return hits;
  }

  async searchMcpServers(repoRoot: string | undefined, query: string, limit: number): Promise<SearchHit[]> {
    const normalizedScope = repoRoot ? repoScope(repoRoot) : "global";
    const scope = `search:mcp_servers:${normalizedScope}`;
    const versionToken = this.mcpServersVersion(repoRoot);
    const cacheKey = this.cache.buildKey(scope, [query, String(limit)], versionToken);
    const cached = this.cache.get<SearchHit[]>(cacheKey, versionToken);
    if (cached) {
      return cached.value;
    }

    const semanticHits = await this.indexService.searchMcpServers(repoRoot, query, limit);
    const deterministicHits = this.searchMcpServersFallback(repoRoot, query, limit);
    const hits = mergeHits(semanticHits, deterministicHits, limit);
    this.cache.set(scope, cacheKey, versionToken, hits);
    return hits;
  }

  async prepareContext(
    goal: string,
    cwd: string,
    plannerArtifactId?: string,
    taskHints: string[] = [],
    changedFiles: string[] = [],
  ): Promise<ContextPack> {
    const taskClass = this.classify(goal);
    const query = [goal, ...taskHints, ...changedFiles].filter(Boolean).join(" ");
    const scope = `context:prepare:${repoScope(cwd)}`;
    const versionToken = hashParts([
      taskClass,
      this.skillsVersion(),
      this.docsVersion(cwd),
      this.memoryVersion(),
      this.mcpServersVersion(cwd),
    ]);
    const cacheKey = this.cache.buildKey(
      scope,
      [goal, query, plannerArtifactId ?? "", ...taskHints, ...changedFiles],
      versionToken,
    );
    const cached = this.cache.get<ContextPack>(cacheKey, versionToken);
    if (cached) {
      return {
        ...cached.value,
        cacheHit: true,
      };
    }

    const [selectedSkills, memoryHits, docHits, mcpServerHits] = await Promise.all([
      this.searchSkills(query || goal, 5),
      this.searchMemory(query || goal, 5),
      this.searchDocs(cwd, query || goal, 5),
      this.searchMcpServers(cwd, query || goal, 5),
    ]);

    const contextPack: ContextPack = {
      taskClass,
      selectedSkills,
      mcpServerHits,
      selectedTools: toolShortlist(taskClass),
      memoryHits,
      docHits,
      cacheHit: false,
      plannerArtifactId,
    };
    this.cache.set(scope, cacheKey, versionToken, contextPack);
    return contextPack;
  }

  invalidate(scope: string): InvalidationResult {
    const targets =
      scope === "skills"
        ? ["search:skills", `embedding:${this.collectionNames.skills}`]
        : scope === "memory"
          ? ["search:memory", `embedding:${this.collectionNames.sessionSummaries}`]
          : scope === "mcp_servers"
            ? ["search:mcp_servers", `embedding:${this.collectionNames.mcpServers}`, "mcp_servers"]
          : [
              scope,
              `search:docs:${repoScope(scope)}`,
              `search:mcp_servers:${repoScope(scope)}`,
              `context:prepare:${repoScope(scope)}`,
              `embedding:${this.collectionNames.repoDocs}:${repoScope(scope)}`,
              `embedding:${this.collectionNames.mcpServers}:${repoScope(scope)}`,
            ];

    let deletedCount = 0;
    for (const target of targets) {
      deletedCount += this.cache.invalidate(target).deletedCount;
    }

    return {
      invalidated: true,
      scope,
      deletedCount,
    };
  }

  private skillsVersion(): string {
    const files = this.skillRoots.flatMap((root) =>
      walkFiles(root, (filePath) => filePath.endsWith("SKILL.md")),
    );
    return fileVersionToken(files);
  }

  private docsVersion(repoRoot: string): string {
    const docsRoot = path.join(repoRoot, "docs");
    const files = walkFiles(docsRoot, (filePath) => /\.(md|txt)$/i.test(filePath));
    return fileVersionToken(files);
  }

  private memoryVersion(): string {
    const rows = this.artifacts.listRecent(200);
    return hashParts(rows.map((row) => `${row.id}:${row.created_at}:${row.summary}`));
  }

  private mcpServersVersion(repoRoot?: string): string {
    const paths = this.indexService.getMcpConfigPaths(repoRoot);
    const existing = paths.filter((filePath) => fs.existsSync(filePath));
    return fileVersionToken(existing);
  }

  private searchSkillsFallback(query: string, limit: number): SearchHit[] {
    const routerScript = path.join(
      os.homedir(),
      ".gemini",
      "antigravity",
      "scripts",
      "skill_router.py",
    );
    const codexRoot = path.join(os.homedir(), ".codex");
    const indexPath = path.join(codexRoot, "skills_index.json");

    if (fs.existsSync(routerScript) && fs.existsSync(indexPath)) {
      try {
        const raw = execFileSync(
          "python",
          [
            routerScript,
            "--query",
            query,
            "--top",
            String(limit),
            "--format",
            "json",
            "--index",
            indexPath,
            "--root",
            codexRoot,
          ],
          {
            encoding: "utf8",
          },
        );
        const parsed = JSON.parse(raw) as Array<{
          id: string;
          path: string;
          score: number;
          why: string;
          description: string;
        }>;

        return parsed.map((item) => ({
          id: item.id,
          kind: "skill" as const,
          title: item.id,
          snippet: item.description || item.why,
          path: item.path,
          score: item.score,
        }));
      } catch {
        // Fall through to the local file-based fallback if the router is unavailable.
      }
    }

    const files = this.skillRoots.flatMap((root) =>
      walkFiles(root, (filePath) => filePath.endsWith("SKILL.md")),
    );

    return files
      .map((filePath) => {
        const content = fs.readFileSync(filePath, "utf8");
        const snippet = content.slice(0, 240).replace(/\s+/g, " ").trim();
        const title = path.basename(path.dirname(filePath));
        return {
          id: filePath,
          kind: "skill" as const,
          title,
          snippet,
          path: filePath,
          score: scoreText(query, `${title} ${content}`),
        };
      })
      .filter((item) => item.score > 0)
      .sort((a, b) => b.score - a.score)
      .slice(0, limit);
  }

  private searchDocsFallback(repoRoot: string, query: string, limit: number): SearchHit[] {
    const docsRoot = path.join(repoRoot, "docs");
    const files = walkFiles(docsRoot, (filePath) => /\.(md|txt)$/i.test(filePath));

    return files
      .map((filePath) => {
        const content = fs.readFileSync(filePath, "utf8");
        const snippet = content.slice(0, 240).replace(/\s+/g, " ").trim();
        return {
          id: filePath,
          kind: "doc" as const,
          title: path.relative(repoRoot, filePath),
          snippet,
          path: filePath,
          score: scoreText(query, `${filePath} ${content}`),
        };
      })
      .filter((item) => item.score > 0)
      .sort((a, b) => b.score - a.score)
      .slice(0, limit);
  }

  private searchMemoryFallback(query: string, limit: number): SearchHit[] {
    return this.artifacts.search(query, limit).map((row) => ({
      id: row.id,
      kind: "memory",
      title: row.summary,
      snippet: row.explanation,
      path: row.file_path,
      score: scoreText(query, `${row.summary} ${row.explanation}`),
    }));
  }

  private searchMcpServersFallback(repoRoot: string | undefined, query: string, limit: number): SearchHit[] {
    return this.indexService
      .listMcpServerDocuments(repoRoot)
      .map((document) => ({
        id: document.id,
        kind: "mcp_server" as const,
        title: document.title,
        snippet: document.text.slice(0, 240).replace(/\s+/g, " ").trim(),
        path: document.path,
        score: scoreText(query, `${document.title} ${document.text}`),
      }))
      .filter((item) => item.score > 0)
      .sort((a, b) => b.score - a.score)
      .slice(0, limit);
  }
}
