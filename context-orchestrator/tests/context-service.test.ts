import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { ContextService } from "../src/services/context-service.js";
import { CacheRepository } from "../src/storage/cache.js";
import { ArtifactRepository } from "../src/storage/index.js";
import { SqliteStore } from "../src/storage/sqlite.js";
import type { SearchHit } from "../src/types.js";

function createHarness() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "context-orchestrator-context-"));
  const docsRoot = path.join(root, "docs");
  const skillsRoot = path.join(root, "skills", "sample-skill");
  const artifactsRoot = path.join(root, "artifacts");

  fs.mkdirSync(docsRoot, { recursive: true });
  fs.mkdirSync(skillsRoot, { recursive: true });
  fs.writeFileSync(path.join(docsRoot, "guide.md"), "# Guide\ncontext retrieval testing\n");
  fs.writeFileSync(
    path.join(skillsRoot, "SKILL.md"),
    "---\nname: sample-skill\ndescription: test fixture\n---\n",
  );

  const sqlite = new SqliteStore(path.join(root, "state.sqlite"));
  const artifacts = new ArtifactRepository(sqlite, artifactsRoot);
  const cache = new CacheRepository(sqlite);

  const calls = {
    searchSkills: 0,
    searchRepoDocs: 0,
    searchSessionSummaries: 0,
  };

  const skillHit: SearchHit = {
    id: "skill-1",
    kind: "skill",
    title: "sample-skill",
    snippet: "semantic skill",
    score: 0.9,
  };
  const docHit: SearchHit = {
    id: "doc-1",
    kind: "doc",
    title: "docs/guide.md",
    snippet: "semantic doc",
    path: path.join(docsRoot, "guide.md"),
    score: 0.8,
  };
  const memoryHit: SearchHit = {
    id: "memory-1",
    kind: "memory",
    title: "Previous plan",
    snippet: "semantic memory",
    score: 0.7,
  };

  const indexService = {
    searchSkills: async () => {
      calls.searchSkills += 1;
      return [skillHit];
    },
    searchRepoDocs: async () => {
      calls.searchRepoDocs += 1;
      return [docHit];
    },
    searchSessionSummaries: async () => {
      calls.searchSessionSummaries += 1;
      return [memoryHit];
    },
  };

  const service = new ContextService(
    [path.join(root, "skills")],
    artifacts,
    cache,
    indexService as never,
    {
      skills: "skills_collection",
      sessionSummaries: "memory_collection",
      repoDocs: "docs_collection",
    },
  );

  return { root, service, calls };
}

test("ContextService caches prepareContext results and invalidates repo-scoped entries", async () => {
  const { root, service, calls } = createHarness();

  const first = await service.prepareContext("review docs", root, undefined, ["docs"], ["docs/guide.md"]);
  assert.equal(first.cacheHit, false);
  assert.equal(calls.searchSkills, 1);
  assert.equal(calls.searchRepoDocs, 1);
  assert.equal(calls.searchSessionSummaries, 1);
  assert.equal(first.selectedSkills[0]?.title, "sample-skill");
  assert.equal(first.docHits[0]?.title, "docs/guide.md");

  const second = await service.prepareContext("review docs", root, undefined, ["docs"], ["docs/guide.md"]);
  assert.equal(second.cacheHit, true);
  assert.equal(calls.searchSkills, 1);
  assert.equal(calls.searchRepoDocs, 1);
  assert.equal(calls.searchSessionSummaries, 1);

  const invalidation = service.invalidate(root);
  assert.equal(invalidation.invalidated, true);
  assert.ok(invalidation.deletedCount >= 2);

  const third = await service.prepareContext("review docs", root, undefined, ["docs"], ["docs/guide.md"]);
  assert.equal(third.cacheHit, false);
  assert.equal(calls.searchSkills, 1);
  assert.equal(calls.searchRepoDocs, 2);
  assert.equal(calls.searchSessionSummaries, 1);
});
