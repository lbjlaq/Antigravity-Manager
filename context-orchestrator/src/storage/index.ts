import type { CompanionArtifact, McpProbeResult } from "../types.js";
import { readArtifact, writeArtifact } from "./artifacts.js";
import { type ArtifactRow, type IndexRunRow, type McpProbeRow, SqliteStore } from "./sqlite.js";

function toProbeResult(row: McpProbeRow): McpProbeResult {
  return {
    inventoryId: row.inventory_id,
    serverName: row.server_name,
    transport: row.transport as McpProbeResult["transport"],
    status: row.status as McpProbeResult["status"],
    checkedAt: row.checked_at,
    responseTimeMs: row.response_time_ms ?? undefined,
    toolCount: row.tool_count ?? undefined,
    error: row.error_text ?? undefined,
    sourcePath: row.source_path,
    repoRoot: row.repo_root ?? undefined,
    repoScope: row.repo_scope,
    endpoint: row.endpoint ?? undefined,
  };
}

export class ArtifactRepository {
  constructor(
    private readonly sqlite: SqliteStore,
    private readonly artifactsDir: string,
  ) {}

  save(artifact: CompanionArtifact): string {
    const filePath = writeArtifact(this.artifactsDir, artifact);
    this.sqlite.upsertArtifact({
      id: artifact.id,
      mode: artifact.mode,
      task_class: artifact.task_class,
      summary: artifact.summary,
      explanation: artifact.explanation,
      confidence: artifact.confidence,
      created_at: artifact.created_at,
      repo_id: artifact.repo_id ?? null,
      profile_id: artifact.profile_id ?? null,
      file_path: filePath,
    });
    return filePath;
  }

  get(id: string): CompanionArtifact | undefined {
    const row = this.sqlite.getArtifact(id);
    if (!row) {
      return undefined;
    }
    return readArtifact(row.file_path);
  }

  listRecent(limit: number): ArtifactRow[] {
    return this.sqlite.listRecentArtifacts(limit);
  }

  search(query: string, limit: number): ArtifactRow[] {
    return this.sqlite.searchArtifactSummaries(query, limit);
  }

  count(): number {
    return this.sqlite.countArtifacts();
  }

  latestCreatedAt(): string | undefined {
    return this.sqlite.latestArtifactCreatedAt();
  }

  recordIndexRun(row: IndexRunRow): void {
    this.sqlite.upsertIndexRun(row);
  }

  getIndexRun(scope: string): IndexRunRow | undefined {
    return this.sqlite.getIndexRun(scope);
  }

  latestIndexRunByPrefix(prefix: string): IndexRunRow | undefined {
    return this.sqlite.latestIndexRunByPrefix(prefix);
  }

  saveMcpProbeResult(result: McpProbeResult): void {
    this.sqlite.upsertMcpProbeRow({
      inventory_id: result.inventoryId,
      server_name: result.serverName,
      transport: result.transport,
      status: result.status,
      checked_at: result.checkedAt,
      response_time_ms: result.responseTimeMs ?? null,
      tool_count: result.toolCount ?? null,
      error_text: result.error ?? null,
      source_path: result.sourcePath,
      repo_root: result.repoRoot ?? null,
      repo_scope: result.repoScope,
      endpoint: result.endpoint ?? null,
    });
  }

  listMcpProbeResults(limit: number, repoScope?: string): McpProbeResult[] {
    return this.sqlite.listMcpProbeRows(limit, repoScope).map(toProbeResult);
  }
}
