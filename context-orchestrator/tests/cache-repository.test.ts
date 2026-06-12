import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { CacheRepository } from "../src/storage/cache.js";
import { SqliteStore } from "../src/storage/sqlite.js";

function createTempStore(): CacheRepository {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "context-orchestrator-cache-"));
  return new CacheRepository(new SqliteStore(path.join(root, "state.sqlite")));
}

test("CacheRepository invalidates nested scopes", () => {
  const cache = createTempStore();
  const version = "v1";

  const rootKey = cache.buildKey("search:docs:repo", ["alpha"], version);
  const nestedKey = cache.buildKey("search:docs:repo:extra", ["beta"], version);
  const otherKey = cache.buildKey("search:skills", ["gamma"], version);

  cache.set("search:docs:repo", rootKey, version, { ok: "root" });
  cache.set("search:docs:repo:extra", nestedKey, version, { ok: "nested" });
  cache.set("search:skills", otherKey, version, { ok: "other" });

  const result = cache.invalidate("search:docs:repo");
  assert.equal(result.invalidated, true);
  assert.equal(result.deletedCount, 2);

  assert.equal(cache.get(rootKey, version), undefined);
  assert.equal(cache.get(nestedKey, version), undefined);
  assert.deepEqual(cache.get(otherKey, version)?.value, { ok: "other" });
  assert.equal(cache.count(), 1);
});
