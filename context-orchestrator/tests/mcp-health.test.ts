import assert from "node:assert/strict";
import fs from "node:fs";
import { fileURLToPath } from "node:url";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { loadConfig } from "../src/config.js";
import { McpHealthService } from "../src/services/mcp-health.js";
import type { McpServerInventoryEntry } from "../src/types.js";
import { ArtifactRepository } from "../src/storage/index.js";
import { SqliteStore } from "../src/storage/sqlite.js";

function createWorkspace() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "context-orchestrator-mcp-health-"));
  const sqlite = new SqliteStore(path.join(root, "state.sqlite"));
  const artifacts = new ArtifactRepository(sqlite, path.join(root, "artifacts"));
  const config = loadConfig();
  return {
    root,
    sqlite,
    artifacts,
    service: new McpHealthService(config, artifacts),
  };
}

function fixturePath(): string {
  const currentFile = fileURLToPath(import.meta.url);
  return path.join(
    path.dirname(currentFile),
    "fixtures",
    "stdio-test-server.mjs",
  );
}

test("McpHealthService probes stdio servers and persists latest probe state", async () => {
  const { artifacts, service } = createWorkspace();
  const entry: McpServerInventoryEntry = {
    inventoryId: "fixture-stdio",
    name: "fixture-stdio",
    title: "fixture-stdio",
    transport: "stdio",
    command: process.execPath,
    args: [fixturePath()],
    cwd: process.cwd(),
    sourcePath: fixturePath(),
    sourceKind: "codex_toml",
    repoScope: "global",
  };

  const result = await service.probeServers([entry]);

  assert.equal(result.summary.configured, 1);
  assert.equal(result.summary.healthy, 1);
  assert.equal(result.probes[0]?.status, "healthy");
  assert.equal(result.probes[0]?.toolCount, 1);
  assert.equal(result.probes[0]?.serverName, "fixture-stdio");

  const status = service.getStatus();
  assert.equal(status.summary.healthy, 1);
  assert.equal(status.probes[0]?.inventoryId, "fixture-stdio");
  assert.equal(artifacts.listMcpProbeResults(10)[0]?.status, "healthy");
});

test("McpHealthService classifies unsupported and broken inventory entries", async () => {
  const { service } = createWorkspace();
  const entries: McpServerInventoryEntry[] = [
    {
      inventoryId: "docker-only",
      name: "docker:github",
      title: "docker:github",
      transport: "docker_registry",
      sourcePath: "C:/repo/mcp-settings.json",
      sourceKind: "docker_registry_json",
      repoScope: "c_repo",
    },
    {
      inventoryId: "missing-command",
      name: "broken-stdio",
      title: "broken-stdio",
      transport: "stdio",
      sourcePath: "C:/repo/config.toml",
      sourceKind: "codex_toml",
      repoScope: "c_repo",
    },
    {
      inventoryId: "spawn-fail",
      name: "spawn-fail",
      title: "spawn-fail",
      transport: "stdio",
      command: "definitely-not-a-real-mcp-command-123",
      sourcePath: "C:/repo/config.toml",
      sourceKind: "codex_toml",
      repoScope: "c_repo",
    },
  ];

  const result = await service.probeServers(entries);
  const byId = new Map(result.probes.map((probe) => [probe.inventoryId, probe]));

  assert.equal(result.summary.inventoryOnly, 1);
  assert.equal(result.summary.invalidConfig, 1);
  assert.equal(result.summary.unreachable, 1);
  assert.equal(byId.get("docker-only")?.status, "inventory_only");
  assert.equal(byId.get("missing-command")?.status, "invalid_config");
  assert.equal(byId.get("spawn-fail")?.status, "unreachable");
});
