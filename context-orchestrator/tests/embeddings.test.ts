import assert from "node:assert/strict";
import test from "node:test";

import { GeminiEmbeddingService, OpenAIEmbeddingService } from "../src/services/embeddings.js";
import { OpenAIService } from "../src/services/openai.js";

test("OpenAIEmbeddingService delegates to OpenAIService", async () => {
  const service = new OpenAIEmbeddingService({
    isConfigured: () => true,
    createEmbeddings: async (input: string[], model: string) =>
      input.map((_item, index) => [model.length, index]),
  } as unknown as OpenAIService);

  assert.equal(service.isConfigured(), true);
  const vectors = await service.createEmbeddings(["a", "b"], "text-embedding-3-small");
  assert.deepEqual(vectors, [
    ["text-embedding-3-small".length, 0],
    ["text-embedding-3-small".length, 1],
  ]);
});

test("GeminiEmbeddingService requests embeddings with configured dimensionality", async () => {
  const originalFetch = globalThis.fetch;
  const requests: Array<{ url: string; body: string }> = [];
  globalThis.fetch = (async (url, init) => {
    requests.push({ url: String(url), body: String(init?.body ?? "") });
    return {
      ok: true,
      text: async () => JSON.stringify({ embedding: { values: [0.1, 0.2, 0.3] } }),
    } as Response;
  }) as typeof fetch;

  try {
    const service = new GeminiEmbeddingService(
      "gemini-key",
      "https://generativelanguage.googleapis.com/v1beta",
      3072,
    );

    assert.equal(service.isConfigured(), true);
    const vectors = await service.createEmbeddings(["hello"], "gemini-embedding-001");
    assert.deepEqual(vectors, [[0.1, 0.2, 0.3]]);
    assert.match(requests[0]?.url ?? "", /models\/gemini-embedding-001:embedContent$/);
    assert.match(requests[0]?.body ?? "", /"outputDimensionality":3072/);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("GeminiEmbeddingService surfaces clear errors for empty bodies", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () =>
    ({
      ok: true,
      text: async () => "",
    }) as Response) as typeof fetch;

  try {
    const service = new GeminiEmbeddingService(
      "gemini-key",
      "https://generativelanguage.googleapis.com/v1beta",
      3072,
    );
    await assert.rejects(
      service.createEmbeddings(["hello"], "gemini-embedding-001"),
      /empty response body/i,
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});
