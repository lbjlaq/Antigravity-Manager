import assert from "node:assert/strict";
import test from "node:test";

import { PlannerService } from "../src/services/planner-service.js";
import type { PlannerRequest } from "../src/types.js";

const baseRequest: PlannerRequest = {
  mode: "auto",
  taskClass: "review",
  taskDescription: "Review the MCP inventory search behavior",
  repoId: "repo-1",
  evidence: [
    {
      kind: "mcp_server",
      ref: "filesystem",
      note: "Configured in workspace MCP settings",
    },
  ],
};

test("PlannerService returns deterministic fallback when OpenAI is not configured", async () => {
  const planner = new PlannerService(
    {
      isConfigured: () => false,
    } as never,
    {
      plannerModel: "gpt-5.4",
      plannerReasoningEffort: "high",
    },
  );

  const artifact = await planner.buildArtifact(baseRequest);

  assert.equal(artifact.mode, "review");
  assert.equal(artifact.task_class, "review");
  assert.equal(artifact.evidence[0]?.kind, "mcp_server");
  assert.equal(artifact.confidence, 0.55);
  assert.ok(Array.isArray(artifact.risks));
});

test("PlannerService uses remote structured output when available", async () => {
  const planner = new PlannerService(
    {
      isConfigured: () => true,
      createStructuredResponse: async () => ({
        summary: "Remote planner summary",
        explanation: "Remote planner explanation",
        recommended_actions: [
          {
            label: "Inspect MCP inventory",
            priority: "high",
            reason: "Server sprawl is part of the task.",
          },
        ],
        risks: [],
        questions: [],
        evidence: [
          {
            kind: "mcp_server",
            ref: "playwright",
            note: "Reachable MCP server",
          },
        ],
        confidence: 2,
      }),
    } as never,
    {
      plannerModel: "gpt-5.4",
      plannerReasoningEffort: "high",
    },
  );

  const artifact = await planner.buildArtifact({
    ...baseRequest,
    mode: "plan",
    taskClass: "architecture",
  });

  assert.equal(artifact.mode, "plan");
  assert.equal(artifact.summary, "Remote planner summary");
  assert.equal(artifact.evidence[0]?.kind, "mcp_server");
  assert.equal(artifact.confidence, 1);
});

test("PlannerService falls back cleanly when remote planner errors", async () => {
  const planner = new PlannerService(
    {
      isConfigured: () => true,
      createStructuredResponse: async () => {
        throw new Error("upstream unavailable");
      },
    } as never,
    {
      plannerModel: "gpt-5.4",
      plannerReasoningEffort: "high",
    },
  );

  const artifact = await planner.buildArtifact({
    ...baseRequest,
    mode: "plan",
    taskClass: "coding",
  });

  assert.equal(artifact.mode, "plan");
  assert.match(artifact.explanation, /upstream unavailable/);
  assert.equal(artifact.confidence, 0.55);
});
