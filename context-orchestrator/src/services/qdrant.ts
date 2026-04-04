import { QdrantClient } from "@qdrant/js-client-rest";

export interface QdrantPoint {
  id: string;
  vector: number[];
  payload: Record<string, unknown>;
}

export function createQdrantClient(url: string, apiKey?: string): QdrantClient {
  return new QdrantClient({ url, apiKey });
}

export async function checkQdrantHealth(
  client: QdrantClient,
): Promise<{ ok: boolean; collectionCount?: number; error?: string }> {
  try {
    const response = await client.getCollections();
    return {
      ok: true,
      collectionCount: response.collections.length,
    };
  } catch (error) {
    return {
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    };
  }
}

export async function ensureCollection(
  client: QdrantClient,
  collectionName: string,
  vectorSize: number,
): Promise<void> {
  try {
    const details = await client.getCollection(collectionName);
    const configuredSize =
      "size" in (details.config?.params?.vectors ?? {})
        ? Number((details.config?.params?.vectors as { size?: number }).size ?? 0)
        : 0;

    if (configuredSize && configuredSize !== vectorSize) {
      throw new Error(
        `Collection ${collectionName} has vector size ${configuredSize}, expected ${vectorSize}.`,
      );
    }
  } catch (error) {
    const message = error instanceof Error ? error.message.toLowerCase() : String(error).toLowerCase();
    if (!message.includes("not found") && !message.includes("404")) {
      throw error;
    }

    await client.createCollection(collectionName, {
      vectors: {
        size: vectorSize,
        distance: "Cosine",
      },
      on_disk_payload: true,
    });

    for (const fieldName of ["documentId", "title", "path", "repoScope", "artifactId"]) {
      try {
        await client.createPayloadIndex(collectionName, {
          field_name: fieldName,
          field_schema: "keyword",
          wait: true,
        });
      } catch {
        // Ignore index creation races and incompatible repeated calls.
      }
    }
  }
}

export async function upsertDocuments(
  client: QdrantClient,
  collectionName: string,
  documents: QdrantPoint[],
): Promise<void> {
  if (documents.length === 0) {
    return;
  }

  await client.upsert(collectionName, {
    wait: true,
    points: documents.map((document) => ({
      id: document.id,
      vector: document.vector,
      payload: document.payload,
    })),
  });
}

export async function searchCollection(
  client: QdrantClient,
  collectionName: string,
  vector: number[],
  limit: number,
  filter?: Record<string, unknown>,
) {
  return client.search(collectionName, {
    vector,
    limit,
    with_payload: true,
    filter,
  });
}

export async function getCollectionPointCount(
  client: QdrantClient,
  collectionName: string,
): Promise<number> {
  try {
    const details = await client.getCollection(collectionName);
    return Number(details.points_count ?? 0);
  } catch {
    return 0;
  }
}
