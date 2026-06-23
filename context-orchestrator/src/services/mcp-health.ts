import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";

import type { OrchestratorConfig } from "../config.js";
import type {
  McpProbeResult,
  McpProbeStatus,
  McpProbeSummary,
  McpServerInventoryEntry,
} from "../types.js";
import { ArtifactRepository } from "../storage/index.js";

const DEFAULT_PROBE_TIMEOUT_MS = 8000;
const STALE_AFTER_MS = 60 * 60 * 1000;

function nowIso(): string {
  return new Date().toISOString();
}

function probeSummary(entries: McpServerInventoryEntry[], probes: McpProbeResult[]): McpProbeSummary {
  const inventoryIds = new Set(entries.map((entry) => entry.inventoryId));
  const relevant = entries.length > 0
    ? probes.filter((probe) => inventoryIds.has(probe.inventoryId))
    : probes;

  const counts: McpProbeSummary = {
    configured: entries.length > 0 ? entries.length : relevant.length,
    healthy: 0,
    unreachable: 0,
    invalidConfig: 0,
    inventoryOnly: 0,
    stale: 0,
  };

  const matched = new Set<string>();
  for (const probe of relevant) {
    matched.add(probe.inventoryId);
    if (probe.status === "healthy") {
      counts.healthy += 1;
    } else if (probe.status === "unreachable") {
      counts.unreachable += 1;
    } else if (probe.status === "invalid_config") {
      counts.invalidConfig += 1;
    } else if (probe.status === "inventory_only") {
      counts.inventoryOnly += 1;
    }

    const checkedAt = new Date(probe.checkedAt).getTime();
    if (Number.isFinite(checkedAt) && Date.now() - checkedAt > STALE_AFTER_MS) {
      counts.stale += 1;
    }
  }

  if (entries.length > 0) {
    counts.stale += entries.filter((entry) => !matched.has(entry.inventoryId)).length;
  }

  return counts;
}

function normalizeError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

async function withTimeout<T>(promise: Promise<T>, timeoutMs: number, label: string): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<T>((_, reject) => {
        timer = setTimeout(() => reject(new Error(`${label} timed out after ${timeoutMs}ms`)), timeoutMs);
      }),
    ]);
  } finally {
    if (timer) {
      clearTimeout(timer);
    }
  }
}

export class McpHealthService {
  constructor(
    private readonly config: OrchestratorConfig,
    private readonly artifacts: ArtifactRepository,
  ) {}

  async probeServers(
    entries: McpServerInventoryEntry[],
    options?: { timeoutMs?: number },
  ): Promise<{ summary: McpProbeSummary; probes: McpProbeResult[] }> {
    const timeoutMs = options?.timeoutMs ?? DEFAULT_PROBE_TIMEOUT_MS;
    const probes: McpProbeResult[] = [];

    for (const entry of entries) {
      const probe = await this.probeEntry(entry, timeoutMs);
      this.artifacts.saveMcpProbeResult(probe);
      probes.push(probe);
    }

    return {
      summary: probeSummary(entries, probes),
      probes,
    };
  }

  getStatus(
    entries: McpServerInventoryEntry[] = [],
    repoScope?: string,
    limit = 50,
  ): { summary: McpProbeSummary; probes: McpProbeResult[] } {
    const probes = this.artifacts.listMcpProbeResults(limit, repoScope);
    return {
      summary: probeSummary(entries, probes),
      probes,
    };
  }

  private async probeEntry(
    entry: McpServerInventoryEntry,
    timeoutMs: number,
  ): Promise<McpProbeResult> {
    if (entry.transport === "docker_registry") {
      return this.baseProbe(entry, "inventory_only");
    }

    if (entry.transport === "stdio") {
      if (!entry.command) {
        return this.baseProbe(entry, "invalid_config", {
          error: "Missing stdio command in MCP inventory entry",
        });
      }
      return this.probeStdio(entry, timeoutMs);
    }

    if (entry.transport === "streamable_http") {
      if (!entry.url) {
        return this.baseProbe(entry, "invalid_config", {
          error: "Missing HTTP URL in MCP inventory entry",
        });
      }
      return this.probeHttp(entry, timeoutMs);
    }

    return this.baseProbe(entry, "invalid_config", {
      error: "Unsupported or unknown MCP transport",
    });
  }

  private baseProbe(
    entry: McpServerInventoryEntry,
    status: McpProbeStatus,
    extras?: Partial<McpProbeResult>,
  ): McpProbeResult {
    return {
      inventoryId: entry.inventoryId,
      serverName: entry.name,
      transport: entry.transport,
      status,
      checkedAt: nowIso(),
      sourcePath: entry.sourcePath,
      repoRoot: entry.repoRoot,
      repoScope: entry.repoScope,
      endpoint: entry.url ?? entry.command,
      ...extras,
    };
  }

  private async probeStdio(
    entry: McpServerInventoryEntry,
    timeoutMs: number,
  ): Promise<McpProbeResult> {
    const startedAt = Date.now();
    const transport = new StdioClientTransport({
      command: entry.command!,
      args: entry.args,
      cwd: entry.cwd,
      stderr: "pipe",
    });

    let stderr = "";
    const stderrStream = transport.stderr;
    if (stderrStream) {
      stderrStream.on("data", (chunk) => {
        stderr += chunk.toString("utf8");
      });
    }

    const client = new Client({
      name: "context-orchestrator-probe",
      version: "0.1.0",
    });

    try {
      await withTimeout(client.connect(transport), timeoutMs, `Connecting to ${entry.name}`);
      const tools = await withTimeout(client.listTools(), timeoutMs, `Listing tools for ${entry.name}`);
      return this.baseProbe(entry, "healthy", {
        responseTimeMs: Date.now() - startedAt,
        toolCount: tools.tools.length,
        error: stderr.trim() || undefined,
      });
    } catch (error) {
      return this.baseProbe(entry, "unreachable", {
        responseTimeMs: Date.now() - startedAt,
        error: stderr.trim() || normalizeError(error),
      });
    } finally {
      await transport.close().catch(() => undefined);
    }
  }

  private async probeHttp(
    entry: McpServerInventoryEntry,
    timeoutMs: number,
  ): Promise<McpProbeResult> {
    const startedAt = Date.now();
    let url: URL;
    try {
      url = new URL(entry.url!);
    } catch {
      return this.baseProbe(entry, "invalid_config", {
        error: `Invalid MCP server URL: ${entry.url}`,
      });
    }

    const transport = new StreamableHTTPClientTransport(url);
    const client = new Client({
      name: "context-orchestrator-probe",
      version: "0.1.0",
    });

    try {
      await withTimeout(client.connect(transport), timeoutMs, `Connecting to ${entry.name}`);
      const tools = await withTimeout(client.listTools(), timeoutMs, `Listing tools for ${entry.name}`);
      return this.baseProbe(entry, "healthy", {
        responseTimeMs: Date.now() - startedAt,
        toolCount: tools.tools.length,
      });
    } catch (error) {
      return this.baseProbe(entry, "unreachable", {
        responseTimeMs: Date.now() - startedAt,
        error: normalizeError(error),
      });
    } finally {
      await transport.close().catch(() => undefined);
    }
  }
}
