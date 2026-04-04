import type { CompanionArtifact } from "../types.js";
import { readArtifact, writeArtifact } from "./artifacts.js";
import { type ArtifactRow, SqliteStore } from "./sqlite.js";

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
}
