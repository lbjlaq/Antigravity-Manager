import assert from "node:assert/strict";
import test from "node:test";

import { OpenAIService } from "../src/services/openai.js";

test("OpenAIService reads output_parsed directly for structured responses", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () =>
    ({
      ok: true,
      json: async () => ({
        output_parsed: {
          summary: "ok",
        },
      }),
    }) as Response) as typeof fetch;

  try {
    const service = new OpenAIService("test-key", "https://api.openai.com/v1");
    const payload = await service.createStructuredResponse<{ summary: string }>({
      model: "gpt-5.4",
      reasoningEffort: "high",
      schemaName: "test",
      schema: { type: "object" },
      instructions: "Return JSON",
      input: "Hello",
    });

    assert.equal(payload.summary, "ok");
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("OpenAIService parses structured responses from output_text fallback", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () =>
    ({
      ok: true,
      json: async () => ({
        output_text: "{\"summary\":\"fallback\"}",
      }),
    }) as Response) as typeof fetch;

  try {
    const service = new OpenAIService("test-key", "https://api.openai.com/v1");
    const payload = await service.createStructuredResponse<{ summary: string }>({
      model: "gpt-5.4",
      reasoningEffort: "high",
      schemaName: "test",
      schema: { type: "object" },
      instructions: "Return JSON",
      input: "Hello",
    });

    assert.equal(payload.summary, "fallback");
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("OpenAIService validates embedding response size", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () =>
    ({
      ok: true,
      json: async () => ({
        data: [{ embedding: [0.1, 0.2] }],
      }),
    }) as Response) as typeof fetch;

  try {
    const service = new OpenAIService("test-key", "https://api.openai.com/v1");
    await assert.rejects(
      service.createEmbeddings(["one", "two"], "text-embedding-3-small"),
      /unexpected payload/i,
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("OpenAIService surfaces upstream error messages", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () =>
    ({
      ok: false,
      status: 429,
      json: async () => ({
        error: {
          message: "rate limited",
        },
      }),
    }) as Response) as typeof fetch;

  try {
    const service = new OpenAIService("test-key", "https://api.openai.com/v1");
    await assert.rejects(
      service.createStructuredResponse({
        model: "gpt-5.4",
        reasoningEffort: "high",
        schemaName: "test",
        schema: { type: "object" },
        instructions: "Return JSON",
        input: "Hello",
      }),
      /rate limited/,
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("OpenAIService rejects empty response bodies with a clear error", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () =>
    ({
      ok: true,
      text: async () => "",
    }) as Response) as typeof fetch;

  try {
    const service = new OpenAIService("test-key", "https://api.openai.com/v1");
    await assert.rejects(
      service.createEmbeddings(["one"], "text-embedding-3-small"),
      /empty response body/i,
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("OpenAIService rejects non-JSON response bodies with a clear error", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () =>
    ({
      ok: true,
      text: async () => "<html>proxy error</html>",
    }) as Response) as typeof fetch;

  try {
    const service = new OpenAIService("test-key", "https://api.openai.com/v1");
    await assert.rejects(
      service.createStructuredResponse({
        model: "gpt-5.4",
        reasoningEffort: "high",
        schemaName: "test",
        schema: { type: "object" },
        instructions: "Return JSON",
        input: "Hello",
      }),
      /non-json content/i,
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});
