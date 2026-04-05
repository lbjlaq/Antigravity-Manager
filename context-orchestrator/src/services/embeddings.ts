import type { OrchestratorConfig } from "../config.js";
import { OpenAIService } from "./openai.js";

export interface EmbeddingService {
  isConfigured(): boolean;
  createEmbeddings(input: string[], model: string): Promise<number[][]>;
}

function trimBaseUrl(baseUrl: string): string {
  return baseUrl.replace(/\/+$/, "");
}

export class OpenAIEmbeddingService implements EmbeddingService {
  constructor(private readonly openai: OpenAIService) {}

  isConfigured(): boolean {
    return this.openai.isConfigured();
  }

  createEmbeddings(input: string[], model: string): Promise<number[][]> {
    return this.openai.createEmbeddings(input, model);
  }
}

interface GeminiEmbeddingResponse {
  embedding?: {
    values?: number[];
  };
}

export class GeminiEmbeddingService implements EmbeddingService {
  constructor(
    private readonly apiKey: string | undefined,
    private readonly baseUrl: string,
    private readonly outputDimensionality?: number,
  ) {}

  isConfigured(): boolean {
    return Boolean(this.apiKey);
  }

  async createEmbeddings(input: string[], model: string): Promise<number[][]> {
    const apiKey = this.apiKey;
    if (!apiKey) {
      throw new Error("Gemini embedding API key is not configured.");
    }

    const vectors = await Promise.all(
      input.map(async (text) => {
        const response = await fetch(
          `${trimBaseUrl(this.baseUrl)}/models/${model}:embedContent`,
          {
            method: "POST",
            headers: {
              "content-type": "application/json",
              "x-goog-api-key": apiKey,
            },
            body: JSON.stringify({
              content: {
                parts: [{ text }],
              },
              ...(this.outputDimensionality
                ? { outputDimensionality: this.outputDimensionality }
                : {}),
            }),
          },
        );

        const rawText = await response.text();
        if (!rawText.trim()) {
          throw new Error(
            response.ok
              ? "Gemini embeddings request returned an empty response body."
              : `Gemini embeddings request failed with ${response.status} and an empty response body.`,
          );
        }

        let payload: GeminiEmbeddingResponse & { error?: { message?: string } };
        try {
          payload = JSON.parse(rawText) as GeminiEmbeddingResponse & {
            error?: { message?: string };
          };
        } catch {
          throw new Error(`Gemini embeddings request returned non-JSON content: ${rawText.slice(0, 240)}`);
        }

        if (!response.ok) {
          throw new Error(payload.error?.message ?? `Gemini embeddings request failed with ${response.status}`);
        }

        const values = payload.embedding?.values;
        if (!Array.isArray(values) || values.length === 0) {
          throw new Error("Gemini embeddings request returned no embedding values.");
        }

        return values;
      }),
    );

    return vectors;
  }
}

export function createEmbeddingService(
  config: Pick<
    OrchestratorConfig,
    "embeddingProvider" | "embeddingApiKey" | "embeddingBaseUrl" | "embeddingOutputDimensionality"
  >,
  openai: OpenAIService,
): EmbeddingService {
  if (config.embeddingProvider === "gemini") {
    return new GeminiEmbeddingService(
      config.embeddingApiKey,
      config.embeddingBaseUrl,
      config.embeddingOutputDimensionality,
    );
  }

  return new OpenAIEmbeddingService(openai);
}
