import { z } from "zod";

export const PrepareTaskContextInputSchema = z.object({
  goal: z.string().min(1),
  cwd: z.string().min(1),
  taskHints: z.array(z.string()).optional().default([]),
  changedFiles: z.array(z.string()).optional().default([]),
  repoId: z.string().optional(),
  profileId: z.string().optional(),
});

export const PlanOrReviewInputSchema = z.object({
  mode: z.enum(["plan", "review", "auto"]).default("auto"),
  taskDescription: z.string().min(1),
  cwd: z.string().min(1),
  repoId: z.string().optional(),
  evidenceRefs: z.array(z.string()).optional().default([]),
});

export const SearchQuerySchema = z.object({
  query: z.string().min(1),
  cwd: z.string().optional(),
  limit: z.number().int().positive().max(20).optional().default(5),
});

export const OrchestratorStatusInputSchema = z.object({
  cwd: z.string().optional(),
});

export const ReindexInputSchema = z.object({
  scope: z.enum(["skills", "memory", "docs", "mcp_servers", "all"]).default("all"),
  cwd: z.string().optional(),
});

export const MemorySummaryInputSchema = z.object({
  source: z.string().min(1),
  summary: z.string().min(1),
  details: z.string().optional(),
  category: z.enum(["decision", "pattern", "finding", "other"]).default("other"),
  relatedFiles: z.array(z.string()).optional().default([]),
  cwd: z.string().optional(),
  repoId: z.string().optional(),
  profileId: z.string().optional(),
});

export type PrepareTaskContextInput = z.infer<typeof PrepareTaskContextInputSchema>;
export type PlanOrReviewInput = z.infer<typeof PlanOrReviewInputSchema>;
export type SearchQueryInput = z.infer<typeof SearchQuerySchema>;
export type OrchestratorStatusInput = z.infer<typeof OrchestratorStatusInputSchema>;
export type ReindexInput = z.infer<typeof ReindexInputSchema>;
export type MemorySummaryInput = z.infer<typeof MemorySummaryInputSchema>;
