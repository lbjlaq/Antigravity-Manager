import { randomUUID } from "node:crypto";

import type { OrchestratorConfig } from "../config.js";
import type {
  ArtifactMode,
  CompanionArtifact,
  PlannerRequest,
  RecommendedAction,
  RiskItem,
  QuestionItem,
  EvidenceItem,
} from "../types.js";
import { OpenAIService } from "./openai.js";

interface RemotePlannerArtifact {
  summary: string;
  explanation: string;
  recommended_actions: RecommendedAction[];
  risks: RiskItem[];
  questions: QuestionItem[];
  evidence: EvidenceItem[];
  confidence: number;
}

const ARTIFACT_SCHEMA: Record<string, unknown> = {
  type: "object",
  additionalProperties: false,
  properties: {
    summary: { type: "string" },
    explanation: { type: "string" },
    recommended_actions: {
      type: "array",
      items: {
        type: "object",
        additionalProperties: false,
        properties: {
          label: { type: "string" },
          priority: {
            type: "string",
            enum: ["high", "medium", "low"],
          },
          reason: { type: "string" },
        },
        required: ["label", "priority", "reason"],
      },
    },
    risks: {
      type: "array",
      items: {
        type: "object",
        additionalProperties: false,
        properties: {
          label: { type: "string" },
          severity: {
            type: "string",
            enum: ["high", "medium", "low"],
          },
          details: { type: "string" },
        },
        required: ["label", "severity", "details"],
      },
    },
    questions: {
      type: "array",
      items: {
        type: "object",
        additionalProperties: false,
        properties: {
          label: { type: "string" },
          blocking: { type: "boolean" },
        },
        required: ["label", "blocking"],
      },
    },
    evidence: {
      type: "array",
      items: {
        type: "object",
        additionalProperties: false,
        properties: {
          kind: {
            type: "string",
            enum: ["memory", "doc", "skill", "mcp_server", "repo_fact", "user_input"],
          },
          ref: { type: "string" },
          note: { type: "string" },
        },
        required: ["kind", "ref", "note"],
      },
    },
    confidence: {
      type: "number",
      minimum: 0,
      maximum: 1,
    },
  },
  required: [
    "summary",
    "explanation",
    "recommended_actions",
    "risks",
    "questions",
    "evidence",
    "confidence",
  ],
};

function resolveMode(mode: ArtifactMode, taskClass: PlannerRequest["taskClass"]): Exclude<ArtifactMode, "auto"> {
  if (mode !== "auto") {
    return mode;
  }

  return taskClass === "review" ? "review" : "plan";
}

function clampConfidence(value: number | undefined): number {
  if (typeof value !== "number" || Number.isNaN(value)) {
    return 0.55;
  }
  return Math.max(0, Math.min(1, value));
}

function normalizeString(value: string | undefined, fallback: string): string {
  return typeof value === "string" && value.trim() ? value.trim() : fallback;
}

function normalizeEvidence(evidence: EvidenceItem[], taskDescription: string): EvidenceItem[] {
  if (evidence.length > 0) {
    return evidence.slice(0, 12);
  }

  return [
    {
      kind: "user_input",
      ref: "taskDescription",
      note: taskDescription,
    },
  ];
}

export class PlannerService {
  constructor(
    private readonly openai: OpenAIService,
    private readonly config: Pick<
      OrchestratorConfig,
      "plannerModel" | "plannerReasoningEffort"
    >,
  ) {}

  async buildArtifact(request: PlannerRequest): Promise<CompanionArtifact> {
    const mode = resolveMode(request.mode, request.taskClass);

    if (this.openai.isConfigured()) {
      try {
        const remote = await this.openai.createStructuredResponse<RemotePlannerArtifact>({
          model: this.config.plannerModel,
          reasoningEffort: this.config.plannerReasoningEffort,
          schemaName: "context_mcp_companion_artifact",
          schema: ARTIFACT_SCHEMA,
          instructions: [
            "You are the planner/reviewer companion for a local coding orchestrator.",
            "Return a compact JSON artifact for the task.",
            `Current mode: ${mode}.`,
            "In plan mode, prioritize concrete recommended actions.",
            "In review mode, prioritize concrete risks and gaps.",
            "Stay grounded in the supplied evidence and do not invent repo state.",
            "Keep the explanation concise and useful for the main coding agent.",
          ].join(" "),
          input: [
            `Task class: ${request.taskClass}`,
            `Task description: ${request.taskDescription}`,
            `Repo id: ${request.repoId ?? "unknown"}`,
            "Evidence:",
            ...normalizeEvidence(request.evidence, request.taskDescription).map(
              (item, index) => `${index + 1}. [${item.kind}] ${item.ref}: ${item.note}`,
            ),
          ].join("\n"),
        });

        return {
          id: randomUUID(),
          mode,
          task_class: request.taskClass,
          summary: normalizeString(remote.summary, `Prepared ${mode} artifact for ${request.taskClass}`),
          explanation: normalizeString(
            remote.explanation,
            "Planner/reviewer returned an empty explanation.",
          ),
          recommended_actions: remote.recommended_actions ?? [],
          risks: remote.risks ?? [],
          questions: remote.questions ?? [],
          evidence: normalizeEvidence(remote.evidence ?? request.evidence, request.taskDescription),
          confidence: clampConfidence(remote.confidence),
          created_at: new Date().toISOString(),
          repo_id: request.repoId,
        };
      } catch (error) {
        return this.buildFallbackArtifact(request, mode, error);
      }
    }

    return this.buildFallbackArtifact(request, mode);
  }

  private buildFallbackArtifact(
    request: PlannerRequest,
    mode: Exclude<ArtifactMode, "auto">,
    error?: unknown,
  ): CompanionArtifact {
    return {
      id: randomUUID(),
      mode,
      task_class: request.taskClass,
      summary: `Prepared ${mode} artifact for ${request.taskClass} task`,
      explanation:
        `Generated by the local deterministic fallback because the remote planner was unavailable. ` +
        `Task: ${request.taskDescription}` +
        (error ? ` Remote error: ${error instanceof Error ? error.message : String(error)}` : ""),
      recommended_actions: [
        {
          label: "Review the assembled context pack before making edits",
          priority: "high",
          reason: "This keeps the main execution agent aligned with the available evidence.",
        },
      ],
      risks:
        mode === "review"
          ? [
              {
                label: "Planner fallback path is active",
                severity: "medium",
                details:
                  "The artifact was generated without a live GPT-5.4 call, so edge-case review depth is reduced.",
              },
            ]
          : [],
      questions: [],
      evidence: normalizeEvidence(request.evidence, request.taskDescription),
      confidence: 0.55,
      created_at: new Date().toISOString(),
      repo_id: request.repoId,
    };
  }
}
