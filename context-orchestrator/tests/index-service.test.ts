import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { listMcpServerInventory, listMcpServers, listRepoDocs } from "../src/services/index-service.js";

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

test("listMcpServerInventory classifies transports and preserves runnable fields", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "context-orchestrator-mcp-inventory-"));
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
            cwd: root,
          },
          remote_docs: {
            url: "http://127.0.0.1:9010/mcp",
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
      `cwd = '${root.replace(/\\/g, "\\\\")}'`,
      "",
      "[mcp_servers.remote_design]",
      "url = 'https://example.com/mcp'",
    ].join("\n"),
  );

  const entries = listMcpServerInventory([jsonPath, tomlPath], root);
  const byName = new Map(entries.map((entry) => [entry.name, entry]));

  assert.equal(byName.get("filesystem")?.transport, "stdio");
  assert.equal(byName.get("filesystem")?.command, "npx");
  assert.equal(byName.get("filesystem")?.cwd, root);

  assert.equal(byName.get("remote_docs")?.transport, "streamable_http");
  assert.equal(byName.get("remote_docs")?.url, "http://127.0.0.1:9010/mcp");

  assert.equal(byName.get("playwright")?.transport, "stdio");
  assert.equal(byName.get("playwright")?.args?.[0], "@playwright/mcp@latest");

  assert.equal(byName.get("remote_design")?.transport, "streamable_http");
  assert.equal(byName.get("github")?.transport, "docker_registry");
});
