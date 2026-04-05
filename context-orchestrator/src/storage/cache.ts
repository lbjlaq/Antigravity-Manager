import { createHash } from "node:crypto";

import type { CacheEntry, InvalidationResult } from "../types.js";
import { SqliteStore } from "./sqlite.js";

function hashKey(parts: string[]): string {
  return createHash("sha256").update(parts.join("::"), "utf8").digest("hex");
}

export class CacheRepository {
  constructor(private readonly sqlite: SqliteStore) {}

  buildKey(scope: string, parts: string[], versionToken: string): string {
    return hashKey([scope, versionToken, ...parts]);
  }

  get<T>(cacheKey: string, versionToken: string): CacheEntry<T> | undefined {
    const row = this.sqlite.getCacheRow(cacheKey);
    if (!row || row.version_token !== versionToken) {
      return undefined;
    }

    return {
      key: row.cache_key,
      scope: row.scope,
      value: JSON.parse(row.value_json) as T,
      version_token: row.version_token,
      created_at: row.created_at,
    };
  }

  set<T>(scope: string, cacheKey: string, versionToken: string, value: T): CacheEntry<T> {
    const createdAt = new Date().toISOString();
    this.sqlite.upsertCacheRow({
      cache_key: cacheKey,
      scope,
      value_json: JSON.stringify(value),
      version_token: versionToken,
      created_at: createdAt,
    });

    return {
      key: cacheKey,
      scope,
      value,
      version_token: versionToken,
      created_at: createdAt,
    };
  }

  invalidate(scope: string): InvalidationResult {
    const deletedCount = this.sqlite.deleteCacheByScope(scope);
    return {
      invalidated: true,
      scope,
      deletedCount,
    };
  }

  count(): number {
    return this.sqlite.countCacheEntries();
  }
}
