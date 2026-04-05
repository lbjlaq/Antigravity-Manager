# Context Orchestrator Design

## Goal

Design a standalone, local-first, multi-client MCP service that augments a primary coding agent with:

- skill selection from a large skill library
- tool brokering and environment-aware tool filtering
- memory retrieval and artifact persistence
- semantic retrieval over skills, session summaries, and repo docs
- structured planning and review from a stronger companion model
- prompt and context-pack caching to reduce repeated token spend

The orchestrator must be reusable by any MCP-capable agent and must not be tied to Antigravity Manager as its only host.

## Non-Goals

- direct file editing by companion services
- replacing normal code navigation for exact code work
- network-first deployment in v1
- automatic hidden failover across multiple paid provider accounts
- full code-embedding and log-embedding coverage in v1

## Design Summary

The system exposes one MCP server to clients. Internally, it contains:

- a deterministic context layer for classification, retrieval, ranking, and context assembly
- a single planner/reviewer companion powered by `gpt-5.4` at high reasoning
- a policy engine that decides when retrieval-only is enough and when the companion should be invoked
- a persistence layer backed by SQLite, Qdrant, and filesystem artifacts

The main coding agent remains the only mutation authority. Companion services may use read-only tools but never edit files or perform overlapping execution with the main agent.

## Deployment Model

v1 is local-first and multi-client:

- runs on the user's machine
- serves multiple MCP-capable clients locally
- keeps indexes, memory, cache, and artifacts on local storage
- can later be embedded into Antigravity Manager as a first-class client and runtime host

## External Architecture

### Clients

Supported clients include:

- Codex
- Claude Code
- Cursor-like MCP-capable agents
- Antigravity Manager in a later integration phase

### MCP Boundary

Clients talk to exactly one MCP server: the orchestrator. They do not talk directly to:

- the retrieval/indexing layer
- the planner/reviewer companion
- Qdrant
- SQLite

This keeps the external surface small and stable while allowing internal services to change.

## Internal Services

### Gateway

The MCP gateway:

- validates tool inputs
- normalizes request metadata
- invokes the policy engine
- returns compact agent-friendly outputs

### Policy Engine

The policy engine decides whether the request needs:

- retrieval only
- retrieval plus planning
- retrieval plus review

v1 uses explainable rules, not opaque auto-routing. Typical triggers include:

- user explicitly asks for planning, design, review, or investigation
- task appears multi-step or cross-cutting
- task touches architecture, providers, auth, routing, infra, or runtime
- the active agent signals low confidence or ambiguity

The active agent may also explicitly escalate.

### Context Service

The context service is primarily deterministic and is responsible for:

- task classification
- skill search and ranking
- tool shortlist generation
- memory retrieval
- repo-doc retrieval
- context-pack assembly

This service should solve the majority of routine requests without invoking the planning companion.

### Planner/Reviewer Companion

The single higher-reasoning companion handles both planning and review:

- model family: `gpt-5.4`
- reasoning level: high
- authority: advisory only

It returns:

- a structured artifact
- a short human-readable explanation

It does not edit files or perform parallel mutation with the main agent.

### Cache Manager

The cache manager handles structured caching for:

- task classification
- skill shortlist results
- tool shortlist results
- memory retrieval bundles
- doc retrieval bundles
- assembled context packs
- planner/reviewer evidence bundles

Caching is versioned and invalidated when relevant state changes.

### Artifact Store

The artifact store persists:

- context packs
- planning artifacts
- review artifacts
- promoted session summaries
- retrieval evidence bundles

Artifacts are persisted for later retrieval and indexing.

### Indexer

The indexer ingests and updates:

- skills
- session summaries
- repo docs

Code embeddings are intentionally excluded from v1.

## Storage Model

v1 uses a hybrid persistence layout:

- SQLite for operational truth and metadata
- Qdrant for semantic retrieval
- filesystem artifacts for larger serialized outputs

### SQLite Responsibilities

SQLite stores:

- artifact registry
- artifact evidence references
- cache manifests and invalidation metadata
- repo registry
- optional profile registry
- task history
- planner/reviewer call history

### Qdrant Responsibilities

Qdrant stores embeddings for:

- skills
- session summaries
- repo docs

### Filesystem Responsibilities

The filesystem stores:

- JSON artifact payloads
- session summaries
- serialized context packs
- exported evidence snapshots

Suggested layout:

```text
artifacts/
  YYYY/
    MM/
      artifact-<id>.json
  session-summaries/
  context-packs/
sqlite/
indexes/
```

## Indexed Corpora in v1

v1 indexes only:

- skills
- session summaries
- repo docs

This scope gives the highest leverage with the least indexing complexity. Exact code navigation remains the job of the active coding agent using normal search tools.

## MCP Tool Surface

### `prepare_task_context`

Primary entry point for most tasks.

Input:

- task goal
- working directory
- optional task hints
- optional changed files
- optional repo id
- optional profile id

Output:

- task class
- ranked skills
- ranked tool shortlist
- memory hits
- repo-doc hits
- cache metadata
- optional planner/reviewer artifact reference
- compact assembled context pack

### `plan_or_review`

Explicit escalation tool.

Input:

- mode hint: `plan`, `review`, or `auto`
- task description
- working directory
- optional evidence references

Output:

- unified artifact
- short explanation

### `search_memory`

Returns ranked memory and session-summary hits.

### `search_docs`

Returns ranked repo-doc hits.

### `search_skills`

Returns ranked skills with selection reasons.

### `get_context_artifact`

Fetches a previously persisted artifact by id.

### `list_recent_artifacts`

Returns recent artifacts with optional filters.

### `invalidate_context_cache`

Invalidates cache entries by repo, task class, or profile scope.

## Unified Companion Artifact

v1 uses a single schema for both planning and review results.

```json
{
  "id": "artifact_123",
  "mode": "plan",
  "task_class": "architecture",
  "summary": "Adopt a single local MCP orchestrator with policy-driven companion escalation.",
  "explanation": "This keeps agent-facing contracts stable while avoiding duplicate reasoning for routine tasks.",
  "recommended_actions": [
    {
      "label": "Build the MCP gateway first",
      "priority": "high",
      "reason": "All clients depend on this boundary."
    }
  ],
  "risks": [
    {
      "label": "Companion overuse",
      "severity": "medium",
      "details": "Always-on planning would double token usage and increase latency."
    }
  ],
  "questions": [
    {
      "label": "Should profiles be included in v1?",
      "blocking": false
    }
  ],
  "evidence": [
    {
      "kind": "repo_doc",
      "ref": "docs/superpowers/specs/...",
      "note": "Prior design approval."
    }
  ],
  "confidence": 0.86,
  "created_at": "2026-04-04T00:00:00Z"
}
```

For planning calls, `recommended_actions` is emphasized. For review calls, `risks` is emphasized.

## Prompt and Context Caching

Prompt caching is implemented through structured artifacts rather than blind raw prompt replay.

Cache units include:

- classification result
- skill shortlist
- tool shortlist
- memory retrieval bundle
- doc retrieval bundle
- context pack
- planner/reviewer evidence bundle

Suggested cache key components:

- repo id
- normalized query hash
- task class
- profile id
- skill index version
- doc index version
- memory version
- tool-state hash

Cache invalidation occurs when:

- repo docs change
- indexed skill content changes
- session-summary corpus changes
- tool availability or auth state changes
- profile routing inputs change

## Main-Agent Contract

The main coding agent remains the sole mutation and execution authority for write actions.

Companion responsibilities:

- classify
- retrieve
- rank
- summarize
- plan
- review

Main-agent responsibilities:

- edit files
- run write-capable tools
- integrate feedback
- resolve conflicts
- decide whether to apply recommended actions

This prevents overlapping file edits and reduces coordination failures.

## Multi-Client Behavior

Because the system is multi-client local-first:

- all artifacts are persisted centrally
- multiple agents may retrieve from the same indexes
- concurrency safety is required for cache and artifact writes
- the gateway should attach per-client request identifiers and timestamps

Clients should not share mutable work ownership, but they may share retrieval, memory, and advisory outputs.

## Future Integration with Antigravity

Antigravity should consume this orchestrator as a client and host adapter, not as the only home of the system.

Antigravity-specific advantages reserved for later:

- Codex profile UI
- worker pool management
- provider routing and health display
- sticky session routing
- model/profile switching

Those concerns should remain outside the standalone orchestrator core.

## Risks

Primary risks:

- companion overuse increases latency and cost
- stale cache entries degrade recommendations
- too-large MCP surfaces reduce discoverability
- excessive retrieval payloads bloat active context
- weak ownership boundaries cause duplicated reasoning

Mitigations:

- policy-triggered invocation
- compact structured outputs
- explicit cache versioning and invalidation
- strict read-only companion authority
- one MCP entry point for all clients

## V1 Success Criteria

v1 is successful if it provides:

- one standalone MCP server usable by multiple local clients
- `prepare_task_context` end-to-end
- `plan_or_review` end-to-end
- persisted and searchable artifacts
- Qdrant-backed retrieval for skills, session summaries, and repo docs
- structured context-pack caching
- no direct companion file edits

## Implementation Order

Recommended build sequence:

1. schemas
2. storage layer
3. MCP gateway
4. indexing for skills, session summaries, and repo docs
5. context service
6. policy engine
7. planner/reviewer service
8. cache manager
9. artifact retrieval and search
10. multi-client hardening and observability

