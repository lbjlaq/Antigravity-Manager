import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { ArtifactRepository } from "../src/storage/index.js";
import { SqliteStore } from "../src/storage/sqlite.js";
import type { CompanionArtifact } from "../src/types.js";

function createTempWorkspace(): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), "context-orchestrator-test-"));
}

function createArtifact(id: string, summary: string, explanation: string): CompanionArtifact {
  return {
    id,
    mode: "plan",
    task_class: "coding",
    summary,
    explanation,
    recommended_actions: [
      {
        label: "Ship it",
        priority: "high",
        reason: "Test fixture action",
      },
    ],
    risks: [],
    questions: [],
    evidence: [],
    confidence: 0.9,
    created_at: new Date().toISOString(),
  };
}

test("ArtifactRepository persists, lists, searches, and reloads artifacts", () => {
  const root = createTempWorkspace();
  const sqlite = new SqliteStore(path.join(root, "state.sqlite"));
  const artifacts = new ArtifactRepository(sqlite, path.join(root, "artifacts"));

  const first = createArtifact("artifact-1", "Planner summary", "Review the caching path");
  const second = createArtifact("artifact-2", "Doc summary", "Search the documentation corpus");

  const firstPath = artifacts.save(first);
  const secondPath = artifacts.save(second);

  assert.ok(firstPath.startsWith(path.join(root, "artifacts")));
  assert.ok(secondPath.startsWith(path.join(root, "artifacts")));
  assert.equal(artifacts.count(), 2);

  const loaded = artifacts.get("artifact-1");
  assert.deepEqual(loaded, first);

  const recent = artifacts.listRecent(5);
  assert.equal(recent.length, 2);
  assert.deepEqual(
    new Set(recent.map((row) => row.id)),
    new Set(["artifact-1", "artifact-2"]),
  );

  const hits = artifacts.search("documentation", 5);
  assert.equal(hits.length, 1);
  assert.equal(hits[0]?.id, "artifact-2");
});
