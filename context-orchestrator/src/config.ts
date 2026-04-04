import os from "node:os";
import path from "node:path";

export interface OrchestratorConfig {
  dataDir: string;
  sqlitePath: string;
  artifactsDir: string;
  qdrantUrl: string;
  qdrantApiKey?: string;
  openaiApiKey?: string;
  openaiBaseUrl: string;
  plannerModel: string;
  plannerReasoningEffort: "low" | "medium" | "high" | "xhigh";
  embeddingModel: string;
  qdrantCollections: {
    skills: string;
    sessionSummaries: string;
    repoDocs: string;
    mcpServers: string;
  };
  skillRoots: string[];
  mcpConfigPaths: string[];
}

function parseRoots(raw: string | undefined): string[] {
  if (!raw) {
    return [
      path.join(os.homedir(), ".codex", "skills"),
      path.join(os.homedir(), ".codex", "shared", "skills-general"),
    ];
  }

  return raw
    .split(path.delimiter)
    .map((item) => item.trim())
    .filter(Boolean);
}

function parseOptionalPaths(raw: string | undefined): string[] {
  if (!raw) {
    return [];
  }

  return raw
    .split(path.delimiter)
    .map((item) => item.trim())
    .filter(Boolean);
}

export function loadConfig(): OrchestratorConfig {
  const dataDir =
    process.env.CONTEXT_MCP_DATA_DIR ??
    path.resolve(process.cwd(), "context-orchestrator-data");

  return {
    dataDir,
    sqlitePath: path.join(dataDir, "sqlite", "orchestrator.db"),
    artifactsDir: path.join(dataDir, "artifacts"),
    qdrantUrl: process.env.QDRANT_URL ?? "http://127.0.0.1:6333",
    qdrantApiKey: process.env.QDRANT_API_KEY,
    openaiApiKey: process.env.OPENAI_API_KEY,
    openaiBaseUrl: process.env.OPENAI_BASE_URL ?? "https://api.openai.com/v1",
    plannerModel: process.env.CONTEXT_MCP_PLANNER_MODEL ?? "gpt-5.4",
    plannerReasoningEffort:
      (process.env.CONTEXT_MCP_PLANNER_REASONING_EFFORT as
        | "low"
        | "medium"
        | "high"
        | "xhigh"
        | undefined) ?? "high",
    embeddingModel: process.env.CONTEXT_MCP_EMBEDDING_MODEL ?? "text-embedding-3-small",
    qdrantCollections: {
      skills: process.env.CONTEXT_MCP_QDRANT_SKILLS_COLLECTION ?? "context_mcp_skills",
      sessionSummaries:
        process.env.CONTEXT_MCP_QDRANT_SESSION_SUMMARIES_COLLECTION ??
        "context_mcp_session_summaries",
      repoDocs:
        process.env.CONTEXT_MCP_QDRANT_REPO_DOCS_COLLECTION ?? "context_mcp_repo_docs",
      mcpServers:
        process.env.CONTEXT_MCP_QDRANT_MCP_SERVERS_COLLECTION ?? "context_mcp_mcp_servers",
    },
    skillRoots: parseRoots(process.env.CONTEXT_MCP_SKILL_ROOTS),
    mcpConfigPaths: parseOptionalPaths(process.env.CONTEXT_MCP_CONFIG_PATHS).length
      ? parseOptionalPaths(process.env.CONTEXT_MCP_CONFIG_PATHS)
      : [
          path.join(os.homedir(), ".codex", "config.toml"),
          path.resolve(process.cwd(), "mcp-settings.json"),
        ],
  };
}
