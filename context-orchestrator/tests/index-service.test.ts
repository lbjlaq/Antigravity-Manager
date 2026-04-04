import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { listMcpServers, listRepoDocs } from "../src/services/index-service.js";

test("listRepoDocs ignores noisy directories and chunks large docs", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "context-orchestrator-index-"));
  const docsDir = path.join(root, "docs");
  const ignoredDir = path.join(docsDir, "node_modules", "nested");
  fs.mkdirSync(ignoredDir, { recursive: true });

  const largeDoc = Array.from({ length: 220 }, (_, index) => `Line ${index} context chunking test`).join("\n");
  fs.writeFileSync(path.join(docsDir, "guide.md"), largeDoc);
  fs.writeFileSync(path.join(ignoredDir, "skip.md"), "ignored");

  const docs = listRepoDocs(root);

  assert.ok(docs.length > 1);
  assert.ok(docs.every((doc) => doc.path.endsWith("guide.md")));
  assert.ok(docs.every((doc) => doc.metadata?.sourceDocumentId === path.join(docsDir, "guide.md")));
  assert.ok(docs.every((doc) => typeof doc.metadata?.chunkIndex === "number"));
});

test("listMcpServers reads both JSON inventories and Codex config sections", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "context-orchestrator-mcp-"));
  const jsonPath = path.join(root, "mcp-settings.json");
  const tomlPath = path.join(root, "config.toml");

  fs.writeFileSync(
    jsonPath,
    JSON.stringify(
      {
        mcpServers: {
          filesystem: {
            command: "npx",
            args: ["@modelcontextprotocol/server-filesystem"],
          },
        },
        dockerRegistry: {
          servers: {
            github: {
              required: true,
            },
          },
        },
      },
      null,
      2,
    ),
  );
  fs.writeFileSync(
    tomlPath,
    [
      "[mcp_servers.playwright]",
      "command = 'npx'",
      "args = ['@playwright/mcp@latest']",
      "",
      "[model_providers.custom]",
      "base_url = 'http://127.0.0.1:8045/v1'",
    ].join("\n"),
  );

  const docs = listMcpServers([jsonPath, tomlPath], root);
  const titles = docs.map((doc) => doc.title).sort();

  assert.deepEqual(titles, ["docker:github", "filesystem", "playwright"]);
  assert.ok(docs.every((doc) => doc.collection === "mcp_servers"));
});
