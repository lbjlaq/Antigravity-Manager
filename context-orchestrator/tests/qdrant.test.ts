import assert from "node:assert/strict";
import test from "node:test";

import {
  checkQdrantHealth,
  ensureCollection,
  getCollectionPointCount,
  searchCollection,
  upsertDocuments,
} from "../src/services/qdrant.js";

test("checkQdrantHealth reports collection counts", async () => {
  const health = await checkQdrantHealth({
    getCollections: async () => ({
      collections: [{ name: "a" }, { name: "b" }],
    }),
  } as never);

  assert.equal(health.ok, true);
  assert.equal(health.collectionCount, 2);
});

test("ensureCollection creates missing collections and payload indexes", async () => {
  const created: string[] = [];
  const indexed: string[] = [];

  await ensureCollection(
    {
      getCollection: async () => {
        throw new Error("404 not found");
      },
      createCollection: async (name: string) => {
        created.push(name);
      },
      createPayloadIndex: async (_name: string, args: { field_name: string }) => {
        indexed.push(args.field_name);
      },
    } as never,
    "docs",
    1536,
  );

  assert.deepEqual(created, ["docs"]);
  assert.ok(indexed.includes("documentId"));
  assert.ok(indexed.includes("repoScope"));
});

test("ensureCollection rejects vector size mismatches", async () => {
  await assert.rejects(
    ensureCollection(
      {
        getCollection: async () => ({
          config: {
            params: {
              vectors: {
                size: 384,
              },
            },
          },
        }),
      } as never,
      "skills",
      1536,
    ),
    /expected 1536/i,
  );
});

test("upsertDocuments and searchCollection delegate to the client", async () => {
  const recorded: Array<{ type: string; payload: unknown }> = [];
  const client = {
    upsert: async (_collection: string, payload: unknown) => {
      recorded.push({ type: "upsert", payload });
    },
    search: async (_collection: string, payload: unknown) => {
      recorded.push({ type: "search", payload });
      return [{ id: "point-1", score: 0.9, payload: { title: "Doc" } }];
    },
  };

  await upsertDocuments(client as never, "docs", [
    {
      id: "point-1",
      vector: [0.1, 0.2],
      payload: { title: "Doc" },
    },
  ]);
  const results = await searchCollection(client as never, "docs", [0.1, 0.2], 5, {
    must: [{ key: "repoScope", match: { value: "repo" } }],
  });

  assert.equal(recorded[0]?.type, "upsert");
  assert.equal(recorded[1]?.type, "search");
  assert.equal(results[0]?.id, "point-1");
});

test("getCollectionPointCount returns zero for missing collections", async () => {
  const count = await getCollectionPointCount(
    {
      getCollection: async () => {
        throw new Error("missing");
      },
    } as never,
    "missing",
  );

  assert.equal(count, 0);
});
