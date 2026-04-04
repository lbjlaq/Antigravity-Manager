interface StructuredResponseArgs<T> {
  model: string;
  reasoningEffort: "low" | "medium" | "high" | "xhigh";
  schemaName: string;
  schema: Record<string, unknown>;
  instructions: string;
  input: string;
}

interface EmbeddingsResponse {
  data?: Array<{
    embedding?: number[];
  }>;
  error?: {
    message?: string;
  };
}

interface ResponsesApiResponse {
  output_parsed?: unknown;
  output_text?: string;
  output?: Array<{
    content?: Array<{
      text?: string | { value?: string };
    }>;
  }>;
  error?: {
    message?: string;
  };
}

function trimBaseUrl(baseUrl: string): string {
  return baseUrl.replace(/\/+$/, "");
}

function extractOutputText(payload: ResponsesApiResponse): string {
  if (typeof payload.output_text === "string" && payload.output_text.trim()) {
    return payload.output_text;
  }

  const parts: string[] = [];
  for (const outputItem of payload.output ?? []) {
    for (const contentItem of outputItem.content ?? []) {
      if (typeof contentItem.text === "string" && contentItem.text.trim()) {
        parts.push(contentItem.text);
        continue;
      }

      if (
        contentItem.text &&
        typeof contentItem.text === "object" &&
        typeof contentItem.text.value === "string" &&
        contentItem.text.value.trim()
      ) {
        parts.push(contentItem.text.value);
      }
    }
  }

  return parts.join("\n").trim();
}

export class OpenAIService {
  constructor(
    private readonly apiKey: string | undefined,
    private readonly baseUrl: string,
  ) {}

  isConfigured(): boolean {
    return Boolean(this.apiKey);
  }

  async createStructuredResponse<T>({
    model,
    reasoningEffort,
    schemaName,
    schema,
    instructions,
    input,
  }: StructuredResponseArgs<T>): Promise<T> {
    const payload = await this.request<ResponsesApiResponse>("responses", {
      model,
      reasoning: {
        effort: reasoningEffort,
      },
      input: [
        {
          role: "system",
          content: [
            {
              type: "input_text",
              text: instructions,
            },
          ],
        },
        {
          role: "user",
          content: [
            {
              type: "input_text",
              text: input,
            },
          ],
        },
      ],
      text: {
        format: {
          type: "json_schema",
          name: schemaName,
          schema,
          strict: true,
        },
      },
    });

    if (payload.output_parsed && typeof payload.output_parsed === "object") {
      return payload.output_parsed as T;
    }

    const outputText = extractOutputText(payload);
    if (!outputText) {
      throw new Error("Responses API returned no parseable output text.");
    }

    return JSON.parse(outputText) as T;
  }

  async createEmbeddings(input: string[], model: string): Promise<number[][]> {
    const payload = await this.request<EmbeddingsResponse>("embeddings", {
      model,
      input,
    });

    const embeddings = payload.data?.map((item) => item.embedding).filter(Array.isArray);
    if (!embeddings || embeddings.length !== input.length) {
      throw new Error("Embeddings API returned an unexpected payload.");
    }

    return embeddings;
  }

  private async request<T>(endpoint: string, body: Record<string, unknown>): Promise<T> {
    if (!this.apiKey) {
      throw new Error("OPENAI_API_KEY is not configured.");
    }

    const response = await fetch(`${trimBaseUrl(this.baseUrl)}/${endpoint}`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${this.apiKey}`,
      },
      body: JSON.stringify(body),
    });

    const rawText = await response.text();
    if (!rawText.trim()) {
      throw new Error(
        response.ok
          ? `${endpoint} request returned an empty response body.`
          : `${endpoint} request failed with ${response.status} and an empty response body.`,
      );
    }

    let payload: (T & {
      error?: {
        message?: string;
      };
    }) | undefined;
    try {
      payload = JSON.parse(rawText) as T & {
        error?: {
          message?: string;
        };
      };
    } catch {
      const snippet = rawText.slice(0, 240);
      throw new Error(
        `${endpoint} request returned non-JSON content: ${snippet}`,
      );
    }

    if (!payload) {
      throw new Error(`${endpoint} request returned no parseable payload.`);
    }

    const parsedPayload = payload as T & {
      error?: {
        message?: string;
      };
    };

    if (!response.ok) {
      throw new Error(parsedPayload.error?.message ?? `${endpoint} request failed with ${response.status}`);
    }

    return parsedPayload;
  }
}
