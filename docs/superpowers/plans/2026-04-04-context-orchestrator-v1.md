# Context Orchestrator V1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first working vertical slice of a standalone local-first MCP orchestrator that serves context preparation, planning/review artifacts, and persisted local artifacts.

**Architecture:** Create a separate `context-orchestrator` TypeScript project at the repo root. Use `node:sqlite` for metadata, filesystem JSON artifacts for persisted payloads, and a Qdrant adapter for future semantic retrieval. Expose a small MCP surface through one server and keep the planner service advisory-only.

**Tech Stack:** TypeScript, Node 25, `@modelcontextprotocol/sdk`, `zod`, `node:sqlite`, filesystem JSON artifacts, `@qdrant/js-client-rest`

---

### Task 1: Scaffold Standalone Project

**Files:**
- Create: `context-orchestrator/package.json`
- Create: `context-orchestrator/tsconfig.json`
- Create: `context-orchestrator/src/main.ts`
- Create: `context-orchestrator/src/config.ts`
- Create: `context-orchestrator/src/types.ts`

- [ ] **Step 1: Write the package manifest**

```json
{
  "name": "context-orchestrator",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "build": "tsc -p tsconfig.json",
    "start": "node dist/main.js",
    "dev": "node --watch --experimental-sqlite src/main.ts"
  },
  "dependencies": {
    "@modelcontextprotocol/sdk": "^1.29.0",
    "@qdrant/js-client-rest": "^1.17.0",
    "zod": "^3.24.1"
  },
  "devDependencies": {
    "@types/node": "^24.3.0",
    "typescript": "^5.9.2"
  }
}
```

- [ ] **Step 2: Add TypeScript config**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "outDir": "dist",
    "rootDir": "src",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "resolveJsonModule": true,
    "types": ["node"]
  },
  "include": ["src/**/*.ts"]
}
```

- [ ] **Step 3: Add shared types**

```ts
export type TaskClass =
  | 'coding'
  | 'debugging'
  | 'architecture'
  | 'review'
  | 'docs'
  | 'research';

export type ArtifactMode = 'plan' | 'review' | 'auto';

export interface RecommendedAction {
  label: string;
  priority: 'high' | 'medium' | 'low';
  reason: string;
}

export interface RiskItem {
  label: string;
  severity: 'high' | 'medium' | 'low';
  details: string;
}

export interface QuestionItem {
  label: string;
  blocking: boolean;
}

export interface EvidenceItem {
  kind: 'memory' | 'doc' | 'skill' | 'repo_fact' | 'user_input';
  ref: string;
  note: string;
}

export interface CompanionArtifact {
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
```

- [ ] **Step 4: Add config loader**

```ts
import path from 'node:path';

export interface OrchestratorConfig {
  dataDir: string;
  sqlitePath: string;
  artifactsDir: string;
  qdrantUrl: string;
  qdrantApiKey?: string;
}

export function loadConfig(): OrchestratorConfig {
  const dataDir = process.env.CONTEXT_MCP_DATA_DIR ?? path.resolve(process.cwd(), 'data');
  return {
    dataDir,
    sqlitePath: path.join(dataDir, 'sqlite', 'orchestrator.db'),
    artifactsDir: path.join(dataDir, 'artifacts'),
    qdrantUrl: process.env.QDRANT_URL ?? 'http://127.0.0.1:6333',
    qdrantApiKey: process.env.QDRANT_API_KEY
  };
}
```

- [ ] **Step 5: Add minimal boot file**

```ts
import { loadConfig } from './config.js';

async function main(): Promise<void> {
  const config = loadConfig();
  console.log(`Context Orchestrator starting with data dir: ${config.dataDir}`);
}

void main();
```

- [ ] **Step 6: Run install and build**

Run: `cd context-orchestrator; npm install; npm run build`
Expected: install succeeds and `dist/main.js` is generated

- [ ] **Step 7: Commit**

```bash
git add context-orchestrator/package.json context-orchestrator/tsconfig.json context-orchestrator/src/main.ts context-orchestrator/src/config.ts context-orchestrator/src/types.ts
git commit -m "feat: scaffold context orchestrator project"
```

### Task 2: Add Schemas and Persistence

**Files:**
- Create: `context-orchestrator/src/schema.ts`
- Create: `context-orchestrator/src/storage/sqlite.ts`
- Create: `context-orchestrator/src/storage/artifacts.ts`
- Create: `context-orchestrator/src/storage/index.ts`
- Modify: `context-orchestrator/src/main.ts`

- [ ] **Step 1: Define request schemas**

```ts
import { z } from 'zod';

export const PrepareTaskContextInputSchema = z.object({
  goal: z.string().min(1),
  cwd: z.string().min(1),
  taskHints: z.array(z.string()).optional().default([]),
  changedFiles: z.array(z.string()).optional().default([]),
  repoId: z.string().optional(),
  profileId: z.string().optional()
});

export const PlanOrReviewInputSchema = z.object({
  mode: z.enum(['plan', 'review', 'auto']).default('auto'),
  taskDescription: z.string().min(1),
  cwd: z.string().min(1),
  repoId: z.string().optional(),
  evidenceRefs: z.array(z.string()).optional().default([])
});
```

- [ ] **Step 2: Create SQLite bootstrap**

```ts
import fs from 'node:fs';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';

export class SqliteStore {
  readonly db: DatabaseSync;

  constructor(dbPath: string) {
    fs.mkdirSync(path.dirname(dbPath), { recursive: true });
    this.db = new DatabaseSync(dbPath);
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS artifacts (
        id TEXT PRIMARY KEY,
        mode TEXT NOT NULL,
        task_class TEXT NOT NULL,
        summary TEXT NOT NULL,
        explanation TEXT NOT NULL,
        confidence REAL NOT NULL,
        created_at TEXT NOT NULL,
        repo_id TEXT,
        profile_id TEXT,
        file_path TEXT NOT NULL
      );
    `);
  }
}
```

- [ ] **Step 3: Create filesystem artifact writer**

```ts
import fs from 'node:fs';
import path from 'node:path';
import type { CompanionArtifact } from '../types.js';

export function writeArtifact(baseDir: string, artifact: CompanionArtifact): string {
  const date = new Date(artifact.created_at);
  const dir = path.join(
    baseDir,
    String(date.getUTCFullYear()),
    String(date.getUTCMonth() + 1).padStart(2, '0')
  );
  fs.mkdirSync(dir, { recursive: true });
  const filePath = path.join(dir, `${artifact.id}.json`);
  fs.writeFileSync(filePath, JSON.stringify(artifact, null, 2));
  return filePath;
}
```

- [ ] **Step 4: Add artifact persistence facade**

```ts
import type { CompanionArtifact } from '../types.js';
import { SqliteStore } from './sqlite.js';
import { writeArtifact } from './artifacts.js';

export class ArtifactRepository {
  constructor(
    private readonly sqlite: SqliteStore,
    private readonly artifactsDir: string
  ) {}

  save(artifact: CompanionArtifact): string {
    const filePath = writeArtifact(this.artifactsDir, artifact);
    const stmt = this.sqlite.db.prepare(`
      INSERT OR REPLACE INTO artifacts
      (id, mode, task_class, summary, explanation, confidence, created_at, repo_id, profile_id, file_path)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `);
    stmt.run(
      artifact.id,
      artifact.mode,
      artifact.task_class,
      artifact.summary,
      artifact.explanation,
      artifact.confidence,
      artifact.created_at,
      artifact.repo_id ?? null,
      artifact.profile_id ?? null,
      filePath
    );
    return filePath;
  }
}
```

- [ ] **Step 5: Wire storage into main**

```ts
import fs from 'node:fs';
import path from 'node:path';
import { loadConfig } from './config.js';
import { SqliteStore } from './storage/sqlite.js';
import { ArtifactRepository } from './storage/index.js';

async function main(): Promise<void> {
  const config = loadConfig();
  fs.mkdirSync(path.dirname(config.sqlitePath), { recursive: true });
  fs.mkdirSync(config.artifactsDir, { recursive: true });
  const sqlite = new SqliteStore(config.sqlitePath);
  const artifacts = new ArtifactRepository(sqlite, config.artifactsDir);
  console.log(`Artifact repository ready at ${config.artifactsDir}`);
}
```

- [ ] **Step 6: Run build**

Run: `cd context-orchestrator; npm run build`
Expected: TypeScript build succeeds with storage modules compiled

- [ ] **Step 7: Commit**

```bash
git add context-orchestrator/src/schema.ts context-orchestrator/src/storage/sqlite.ts context-orchestrator/src/storage/artifacts.ts context-orchestrator/src/storage/index.ts context-orchestrator/src/main.ts
git commit -m "feat: add orchestrator persistence layer"
```

### Task 3: Implement Context and Planner Services

**Files:**
- Create: `context-orchestrator/src/services/context-service.ts`
- Create: `context-orchestrator/src/services/planner-service.ts`
- Create: `context-orchestrator/src/services/index-service.ts`
- Create: `context-orchestrator/src/services/qdrant.ts`

- [ ] **Step 1: Add a small deterministic context service**

```ts
import type { TaskClass } from '../types.js';

export function classifyTask(goal: string): TaskClass {
  const normalized = goal.toLowerCase();
  if (normalized.includes('review')) return 'review';
  if (normalized.includes('design') || normalized.includes('architecture')) return 'architecture';
  if (normalized.includes('debug') || normalized.includes('error')) return 'debugging';
  if (normalized.includes('doc')) return 'docs';
  if (normalized.includes('research')) return 'research';
  return 'coding';
}
```

- [ ] **Step 2: Add Qdrant adapter**

```ts
import { QdrantClient } from '@qdrant/js-client-rest';

export function createQdrantClient(url: string, apiKey?: string): QdrantClient {
  return new QdrantClient({ url, apiKey });
}
```

- [ ] **Step 3: Add initial planner service**

```ts
import { randomUUID } from 'node:crypto';
import type { ArtifactMode, CompanionArtifact, TaskClass } from '../types.js';

export function buildCompanionArtifact(
  mode: ArtifactMode,
  taskClass: TaskClass,
  taskDescription: string
): CompanionArtifact {
  return {
    id: randomUUID(),
    mode,
    task_class: taskClass,
    summary: `Prepared ${mode} artifact for ${taskClass} task`,
    explanation: `Initial v1 artifact generated from local policy and task text: ${taskDescription}`,
    recommended_actions: [
      {
        label: 'Review the proposed context pack',
        priority: 'high',
        reason: 'This validates the gateway-to-artifact flow.'
      }
    ],
    risks: [],
    questions: [],
    evidence: [],
    confidence: 0.55,
    created_at: new Date().toISOString()
  };
}
```

- [ ] **Step 4: Add index-service placeholders with working scan behavior**

```ts
import fs from 'node:fs';
import path from 'node:path';

export function listDocs(repoRoot: string): string[] {
  const docsDir = path.join(repoRoot, 'docs');
  if (!fs.existsSync(docsDir)) return [];
  return fs.readdirSync(docsDir).map((name) => path.join(docsDir, name));
}
```

- [ ] **Step 5: Run build**

Run: `cd context-orchestrator; npm run build`
Expected: service modules compile cleanly

- [ ] **Step 6: Commit**

```bash
git add context-orchestrator/src/services/context-service.ts context-orchestrator/src/services/planner-service.ts context-orchestrator/src/services/index-service.ts context-orchestrator/src/services/qdrant.ts
git commit -m "feat: add orchestrator context and planner services"
```

### Task 4: Expose MCP Gateway

**Files:**
- Create: `context-orchestrator/src/gateway/server.ts`
- Modify: `context-orchestrator/src/main.ts`

- [ ] **Step 1: Implement MCP server with core tools**

```ts
import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';

export function createServer(): Server {
  return new Server(
    { name: 'context-orchestrator', version: '0.1.0' },
    { capabilities: { tools: {} } }
  );
}
```

- [ ] **Step 2: Register `prepare_task_context`**

```ts
server.tool(
  'prepare_task_context',
  'Prepare a compact context pack for a local coding task.',
  PrepareTaskContextInputSchema.shape,
  async (input) => {
    const taskClass = classifyTask(input.goal);
    return {
      content: [
        {
          type: 'text',
          text: JSON.stringify({
            taskClass,
            selectedSkills: [],
            selectedTools: [],
            memoryHits: [],
            docHits: []
          }, null, 2)
        }
      ]
    };
  }
);
```

- [ ] **Step 3: Register `plan_or_review`**

```ts
server.tool(
  'plan_or_review',
  'Generate a structured plan or review artifact.',
  PlanOrReviewInputSchema.shape,
  async (input) => {
    const taskClass = classifyTask(input.taskDescription);
    const artifact = buildCompanionArtifact(input.mode, taskClass, input.taskDescription);
    const filePath = artifacts.save(artifact);
    return {
      content: [
        { type: 'text', text: JSON.stringify({ artifact, filePath }, null, 2) }
      ]
    };
  }
);
```

- [ ] **Step 4: Start stdio transport in `main.ts`**

```ts
const transport = new StdioServerTransport();
await server.connect(transport);
console.error('Context Orchestrator MCP server connected over stdio');
```

- [ ] **Step 5: Run build**

Run: `cd context-orchestrator; npm run build`
Expected: build succeeds with MCP gateway compiled

- [ ] **Step 6: Smoke test startup**

Run: `cd context-orchestrator; node dist/main.js`
Expected: process starts without throwing and waits on stdio

- [ ] **Step 7: Commit**

```bash
git add context-orchestrator/src/gateway/server.ts context-orchestrator/src/main.ts
git commit -m "feat: expose context orchestrator MCP gateway"
```

### Task 5: Validate Spec Coverage

**Files:**
- Modify: `docs/superpowers/plans/2026-04-04-context-orchestrator-v1.md`

- [ ] **Step 1: Check spec coverage**

Verify the plan covers:

- standalone local-first multi-client MCP server
- retrieval and context-pack flow
- planning/review artifact flow
- SQLite plus filesystem persistence
- Qdrant adapter wiring

Expected: all v1 vertical-slice requirements appear in Tasks 1-4

- [ ] **Step 2: Placeholder scan**

Run: `rg -n "TBD|TODO|placeholder|implement later|similar to" docs/superpowers/plans/2026-04-04-context-orchestrator-v1.md`
Expected: no matches

- [ ] **Step 3: Type consistency scan**

Confirm all planned identifiers match:

- `CompanionArtifact`
- `PrepareTaskContextInputSchema`
- `PlanOrReviewInputSchema`
- `buildCompanionArtifact`
- `classifyTask`

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/plans/2026-04-04-context-orchestrator-v1.md
git commit -m "docs: add context orchestrator v1 implementation plan"
```

