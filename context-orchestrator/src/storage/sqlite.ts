import fs from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

export interface ArtifactRow {
  id: string;
  mode: string;
  task_class: string;
  summary: string;
  explanation: string;
  confidence: number;
  created_at: string;
  repo_id: string | null;
  profile_id: string | null;
  file_path: string;
}

export interface CacheRow {
  cache_key: string;
  scope: string;
  value_json: string;
  version_token: string;
  created_at: string;
}

export interface IndexRunRow {
  scope: string;
  updated_at: string;
  document_count: number;
  chunk_count: number;
  embedding_model: string;
  repo_root: string | null;
}

export class SqliteStore {
  readonly db: DatabaseSync;

  constructor(dbPath: string) {
    fs.mkdirSync(path.dirname(dbPath), { recursive: true });
    this.db = new DatabaseSync(dbPath);
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS artifacts (
        id TEXT PRIMARY KEY,
        mode TEXT NOT NULL,
        task_class TEXT NOT NULL,
        summary TEXT NOT NULL,
        explanation TEXT NOT NULL,
        confidence REAL NOT NULL,
        created_at TEXT NOT NULL,
        repo_id TEXT,
        profile_id TEXT,
        file_path TEXT NOT NULL
      );

      CREATE TABLE IF NOT EXISTS cache_entries (
        cache_key TEXT PRIMARY KEY,
        scope TEXT NOT NULL,
        value_json TEXT NOT NULL,
        version_token TEXT NOT NULL,
        created_at TEXT NOT NULL
      );

      CREATE TABLE IF NOT EXISTS index_runs (
        scope TEXT PRIMARY KEY,
        updated_at TEXT NOT NULL,
        document_count INTEGER NOT NULL,
        chunk_count INTEGER NOT NULL,
        embedding_model TEXT NOT NULL,
        repo_root TEXT
      );
    `);
  }

  upsertArtifact(row: ArtifactRow): void {
    const stmt = this.db.prepare(`
      INSERT OR REPLACE INTO artifacts (
        id, mode, task_class, summary, explanation, confidence, created_at, repo_id, profile_id, file_path
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `);

    stmt.run(
      row.id,
      row.mode,
      row.task_class,
      row.summary,
      row.explanation,
      row.confidence,
      row.created_at,
      row.repo_id,
      row.profile_id,
      row.file_path,
    );
  }

  getArtifact(id: string): ArtifactRow | undefined {
    const stmt = this.db.prepare(`
      SELECT id, mode, task_class, summary, explanation, confidence, created_at, repo_id, profile_id, file_path
      FROM artifacts
      WHERE id = ?
    `);

    return stmt.get(id) as unknown as ArtifactRow | undefined;
  }

  listRecentArtifacts(limit: number): ArtifactRow[] {
    const stmt = this.db.prepare(`
      SELECT id, mode, task_class, summary, explanation, confidence, created_at, repo_id, profile_id, file_path
      FROM artifacts
      ORDER BY created_at DESC
      LIMIT ?
    `);

    return stmt.all(limit) as unknown as ArtifactRow[];
  }

  searchArtifactSummaries(query: string, limit: number): ArtifactRow[] {
    const stmt = this.db.prepare(`
      SELECT id, mode, task_class, summary, explanation, confidence, created_at, repo_id, profile_id, file_path
      FROM artifacts
      WHERE lower(summary) LIKE lower(?) OR lower(explanation) LIKE lower(?)
      ORDER BY created_at DESC
      LIMIT ?
    `);

    const like = `%${query}%`;
    return stmt.all(like, like, limit) as unknown as ArtifactRow[];
  }

  countArtifacts(): number {
    const stmt = this.db.prepare(`
      SELECT COUNT(*) as count
      FROM artifacts
    `);

    const row = stmt.get() as { count?: number } | undefined;
    return Number(row?.count ?? 0);
  }

  latestArtifactCreatedAt(): string | undefined {
    const stmt = this.db.prepare(`
      SELECT created_at
      FROM artifacts
      ORDER BY created_at DESC
      LIMIT 1
    `);

    const row = stmt.get() as { created_at?: string } | undefined;
    return row?.created_at;
  }

  upsertCacheRow(row: CacheRow): void {
    const stmt = this.db.prepare(`
      INSERT OR REPLACE INTO cache_entries (
        cache_key, scope, value_json, version_token, created_at
      ) VALUES (?, ?, ?, ?, ?)
    `);

    stmt.run(
      row.cache_key,
      row.scope,
      row.value_json,
      row.version_token,
      row.created_at,
    );
  }

  getCacheRow(cacheKey: string): CacheRow | undefined {
    const stmt = this.db.prepare(`
      SELECT cache_key, scope, value_json, version_token, created_at
      FROM cache_entries
      WHERE cache_key = ?
    `);

    return stmt.get(cacheKey) as unknown as CacheRow | undefined;
  }

  deleteCacheByScope(scope: string): number {
    const stmt = this.db.prepare(`
      DELETE FROM cache_entries
      WHERE scope = ? OR scope LIKE ?
    `);

    const result = stmt.run(scope, `${scope}:%`);
    return Number(result.changes ?? 0);
  }

  countCacheEntries(): number {
    const stmt = this.db.prepare(`
      SELECT COUNT(*) as count
      FROM cache_entries
    `);

    const row = stmt.get() as { count?: number } | undefined;
    return Number(row?.count ?? 0);
  }

  upsertIndexRun(row: IndexRunRow): void {
    const stmt = this.db.prepare(`
      INSERT OR REPLACE INTO index_runs (
        scope, updated_at, document_count, chunk_count, embedding_model, repo_root
      ) VALUES (?, ?, ?, ?, ?, ?)
    `);

    stmt.run(
      row.scope,
      row.updated_at,
      row.document_count,
      row.chunk_count,
      row.embedding_model,
      row.repo_root,
    );
  }

  getIndexRun(scope: string): IndexRunRow | undefined {
    const stmt = this.db.prepare(`
      SELECT scope, updated_at, document_count, chunk_count, embedding_model, repo_root
      FROM index_runs
      WHERE scope = ?
    `);

    return stmt.get(scope) as unknown as IndexRunRow | undefined;
  }

  latestIndexRunByPrefix(prefix: string): IndexRunRow | undefined {
    const stmt = this.db.prepare(`
      SELECT scope, updated_at, document_count, chunk_count, embedding_model, repo_root
      FROM index_runs
      WHERE scope = ? OR scope LIKE ?
      ORDER BY updated_at DESC
      LIMIT 1
    `);

    return stmt.get(prefix, `${prefix}:%`) as unknown as IndexRunRow | undefined;
  }
}
