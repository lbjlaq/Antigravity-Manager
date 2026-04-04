export type TaskClass =
  | "coding"
  | "debugging"
  | "architecture"
  | "review"
  | "docs"
  | "research";

export type ArtifactMode = "plan" | "review" | "auto";

export interface RecommendedAction {
  label: string;
  priority: "high" | "medium" | "low";
  reason: string;
}

export interface RiskItem {
  label: string;
  severity: "high" | "medium" | "low";
  details: string;
}

export interface QuestionItem {
  label: string;
  blocking: boolean;
}

export interface EvidenceItem {
  kind: "memory" | "doc" | "skill" | "mcp_server" | "repo_fact" | "user_input";
  ref: string;
  note: string;
}

export interface CompanionArtifact {
  [key: string]: unknown;
  id: string;
  mode: ArtifactMode;
  task_class: TaskClass;
  summary: string;
  explanation: string;
  recommended_actions: RecommendedAction[];
  risks: RiskItem[];
  questions: QuestionItem[];
  evidence: EvidenceItem[];
  confidence: number;
  created_at: string;
  repo_id?: string;
  profile_id?: string;
}

export interface SearchHit {
  [key: string]: unknown;
  id: string;
  kind: "memory" | "doc" | "skill" | "mcp_server";
  title: string;
  snippet: string;
  path?: string;
  score: number;
}

export interface ContextPack {
  [key: string]: unknown;
  taskClass: TaskClass;
  selectedSkills: SearchHit[];
  mcpServerHits: SearchHit[];
  selectedTools: string[];
  memoryHits: SearchHit[];
  docHits: SearchHit[];
  cacheHit: boolean;
  plannerArtifactId?: string;
}

export interface MemorySummaryInput {
  source: string;
  summary: string;
  details?: string;
  category: "decision" | "pattern" | "finding" | "other";
  relatedFiles?: string[];
  cwd?: string;
  repoId?: string;
  profileId?: string;
}

export interface CacheEntry<T = unknown> {
  key: string;
  scope: string;
  value: T;
  version_token: string;
  created_at: string;
}

export interface InvalidationResult {
  invalidated: boolean;
  scope: string;
  deletedCount: number;
}

export interface PlannerRequest {
  mode: ArtifactMode;
  taskClass: TaskClass;
  taskDescription: string;
  repoId?: string;
  evidence: EvidenceItem[];
}

export interface IndexedDocument {
  id: string;
  title: string;
  text: string;
  path: string;
  collection: "skills" | "session_summaries" | "repo_docs" | "mcp_servers";
  metadata?: Record<string, unknown>;
}
