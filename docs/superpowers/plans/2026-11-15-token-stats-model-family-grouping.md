# Token 统计页面模型家族聚合展开与单价表收敛实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 治理 Token 统计页面中因思考档位后缀（-high/-medium/-low/-tiered）导致的同质模型碎片化展示问题，实现模型家族聚合与下钻展开、前五大主力模型图表聚拢、以及默认单价表的通配精简。

**Architecture:**
1. 提取统一的 `src/utils/modelFamily.ts` 工具模块，封装档位后缀剥离、模型家族聚合（`groupModelTokenStatsByFamily`、`aggregateModelTrendsByFamily`）及档位标签样式，作为 `TokenStats.tsx` 与 `ApiProxy.tsx` 的单一权威数据源。
2. 升级 `TokenStats.tsx` 底部详细表格，支持一级展示模型家族基准行，并支持点击整行展开/折叠各个具体思考档位明细（请求数、Tokens、首字耗时、单价成本）。
3. 收敛 `DEFAULT_PRICING` 规则，消除 `gemini-3.8-flash` ~ `2.5-flash` 等同价模型的行级冗余重复，利用 `getModelFamilyId` 与通配机制补齐 `gemini-4-argon` / `claude-opus-5-5` 等主流代次。

**Tech Stack:** React 19, TypeScript, Tailwind CSS, Lucide Icons, DaisyUI 5, i18next

## Global Constraints
- **零副作用读取**：统计展示属于纯读取与聚合操作，严禁修改后台 SQLite 原始 `token_usage` 表数据，保持底层原始日志的精确可追溯性。
- **向后兼容性**：自定义单价存储（`localStorage` 中的 `antigravity_model_pricing_v2026`）与既有用户设置需无缝兼容。
- **构建门禁**：改动必须通过 `npx tsx` 单元测试与 `npm run build`（TypeScript 类型检查 + Vite 构建零报错）。

---

### Task 1: 共享模型家族工具库与 TDD 验证 (Shared Model Family Utilities)

**Files:**
- Create: `src/utils/modelFamily.ts`
- Test: `src/utils/__tests__/modelFamily.test.ts`
- Modify: `src/pages/ApiProxy.tsx:1760-1810` (复用共享工具)

**Interfaces:**
- Produces:
  ```typescript
  export const TIER_SUFFIXES: string[];
  export function getModelFamilyId(modelId: string): string;
  export function getTierLabel(modelId: string, familyId?: string): string;
  export function getTierBadgeClass(tierLabel: string): string;
  export function groupModelTokenStatsByFamily(
      stats: ModelTokenStats[],
      calculateCost: (m: ModelTokenStats) => number
  ): ModelFamilyTokenStats[];
  export function aggregateModelTrendsByFamily(
      modelTrend: ModelTrendPoint[],
      topN?: number
  ): { aggregatedTrend: ModelTrendPoint[]; topFamilyKeys: string[] };
  ```

- [ ] **Step 1: 编写失败的单元测试**

编写 `src/utils/__tests__/modelFamily.test.ts`，覆盖：
- 档位后缀剥离（`gemini-3.8-flash-high` -> `gemini-3.8-flash`, `claude-opus-5-5-medium` -> `claude-opus-5-5`）
- 档位标签识别（`-xhigh`, `-high`, `-medium`, `-low`, `-extra-low`, `-tiered`, `-thinking`）
- 模型统计聚合（汇总 input/output/cached/reasoning tokens、请求数、按 tokens 加权的平均 TTFT、总费用）
- 趋势数据按模型家族累加合并

- [ ] **Step 2: 运行测试以确认失败 (RED)**

运行: `npx tsx src/utils/__tests__/modelFamily.test.ts`
预期: FAIL (Cannot find module '../../utils/modelFamily')

- [ ] **Step 3: 编写 `src/utils/modelFamily.ts` 最小实现 (GREEN)**

实现上述方法与类型定义，确保纯函数、高性能、零外部副作用。

- [ ] **Step 4: 运行测试验证全绿 (GREEN)**

运行: `npx tsx src/utils/__tests__/modelFamily.test.ts`
预期: 全部用例 PASS

- [ ] **Step 5: 将 `src/pages/ApiProxy.tsx` 中分散的本地方法替换为从 `modelFamily.ts` 导入**

替换 `src/pages/ApiProxy.tsx` 中的本地 `TIER_SUFFIXES`、`getModelFamilyId`、`getTierLabel`、`getTierBadgeClass`。

- [ ] **Step 6: 提交 Task 1 代码**

```bash
git add src/utils/modelFamily.ts src/utils/__tests__/modelFamily.test.ts src/pages/ApiProxy.tsx
git commit -m "feat(token-stats): extract shared model family normalization and grouping utilities"
```

---

### Task 2: 收敛默认单价表与匹配算法 (Simplify Default Pricing Rules)

**Files:**
- Modify: `src/pages/TokenStats.tsx:102-132, 360-385`
- Modify: `src/components/token_stats/PricingModal.tsx`
- Test: `src/utils/__tests__/modelFamily.test.ts`

**Interfaces:**
- Consumes: `getModelFamilyId` from `src/utils/modelFamily.ts`
- Modifies: `DEFAULT_PRICING` in `TokenStats.tsx`, `getPricing` resolution logic

- [ ] **Step 1: 在 `modelFamily.test.ts` 编写单价匹配测试用例**

验证：
- `gemini-3.8-flash-high` 自动匹配到 `gemini-flash` 或 `gemini-3.8-flash` 家族单价
- `gemini-4-argon` / `gemini-4-argon-high` 正确匹配 Pro 系列单价（$2.00 / $12.00 / $0.20）
- `claude-opus-5-5-medium` 正确匹配 Opus 系列单价（$5.00 / $25.00 / $0.50）
- `claude-sonnet-5.5` 正确匹配 Sonnet 系列单价（$3.00 / $15.00 / $0.30）
- 未命中时平滑兜底到 `default`（$1.00 / $5.00 / $0.10）

- [ ] **Step 2: 运行测试以确认失败 (RED)**

运行: `npx tsx src/utils/__tests__/modelFamily.test.ts`
预期: 单价匹配用例 FAIL

- [ ] **Step 3: 精简 `DEFAULT_PRICING` 并重构 `getPricing` (GREEN)**

在 `src/pages/TokenStats.tsx` 中：
1. 精简 `DEFAULT_PRICING`，消除 6 个相同 Flash 与 3 个相同 Pro 的机械重复，补齐 `gemini-4-argon`、`gemini-4-hydrogen`、`claude-opus-5`、`claude-fable-5`、`claude-sonnet-5`、`gpt-5` 等主流代次规则。
2. 升级 `getPricing(key)` 解析器：
   - 先精准匹配完整 key
   - 再通过 `getModelFamilyId(key)` 匹配家族名
   - 再剥除点分/破折号（`claude-sonnet-4.6` <-> `claude-sonnet-4-6`）
   - 再通过前缀通配规则匹配
   - 最后 fallback 到 `default`
3. 确保在 `PricingModal.tsx` 中呈现清晰紧凑的家族分类（Google Gemini、Anthropic Claude、OpenAI、默认兜底），支持用户按需对具体模型或全系列进行自定义单价微调。

- [ ] **Step 4: 运行测试验证全绿 (GREEN)**

运行: `npx tsx src/utils/__tests__/modelFamily.test.ts`
预期: 全部 PASS

- [ ] **Step 5: 提交 Task 2 代码**

```bash
git add src/pages/TokenStats.tsx src/components/token_stats/PricingModal.tsx src/utils/__tests__/modelFamily.test.ts
git commit -m "feat(token-stats): condense default pricing rules with intelligent family inheritance"
```

---

### Task 3: Token 统计页面模型家族聚合与下钻展开 (Token Stats Family Grouping & Drill-down)

**Files:**
- Modify: `src/pages/TokenStats.tsx`
- Modify: `src/locales/zh.json`, `src/locales/en.json`, `src/locales/zh-TW.json`

**Interfaces:**
- Consumes: `groupModelTokenStatsByFamily`, `getTierLabel`, `getTierBadgeClass` from `src/utils/modelFamily.ts`
- Produces: Expandable model family table with variant breakdown, clean Top 5 model aggregation for Bento/Cockpit views

- [ ] **Step 1: 在 `TokenStats.tsx` 中集成家族聚合数据管道**

1. 在 `fetchData` 中：
   - 对 `modelData` 调用 `groupModelTokenStatsByFamily` 生成 `modelFamilyGroups`。
   - `modelShareItems` 改为基于家族级汇总数据构建，避免 4 个 `gemini-3.8-flash` 档位挤占 Top 5 榜单与赛博大盘 360 度彩针。
   - 顶部统计卡片中的“使用模型数”展示聚合后的核心模型家族数（同时注明具体包含多少个细分档位）。
2. 在趋势图（`modelTrendData`）中：
   - 提取 Top 5 时按模型家族累加求和，确保趋势柱/面积图展示的是代表性主模型，而不是同模型的重复条目。

- [ ] **Step 2: 重构底部分模型详细统计表格为折叠展开树**

1. 为表格增加展开状态管理 `expandedFamilies: Record<string, boolean>` 与一键全展开/全折叠切换按钮。
2. 一级行展示模型家族汇总（基准模型名、家族图标、档位数徽章、汇总请求数、汇总输入/输出/缓存/总Token、加权平均首字耗时、总占比、总估算费用）。
3. 包含多档位的家族支持点击整行或左侧箭头展开；子行以缩进排版展示各具体档位（`-high`, `-medium` 等）的详细指标与彩色档位徽章。

- [ ] **Step 3: 补齐多语言词条**

在 `zh.json`、`en.json`、`zh-TW.json` 等中添加：
- `token_stats.expand_all` ("全部展开" / "Expand All")
- `token_stats.collapse_all` ("全部折叠" / "Collapse All")
- `token_stats.variants_count` ("{{count}} 个档位" / "{{count}} tiers")
- `token_stats.tier_label` ("档位" / "Tier")

- [ ] **Step 4: 运行构建与单测验证**

运行:
```bash
npx tsx src/utils/__tests__/modelFamily.test.ts
npm run build
```
预期: 测试全绿，构建零警告/零报错。

- [ ] **Step 5: 提交 Task 3 代码**

```bash
git add src/pages/TokenStats.tsx src/locales/
git commit -m "feat(token-stats): support expandable model family drill-down in detailed statistics"
```

---

### Task 4: 端到端集成验证与交付收尾 (E2E Verification & Review)

**Files:**
- Review: all modified files

- [ ] **Step 1: 运行完整前端构建**

```bash
npm run build
```
验证: 输出无任何 TypeScript 报错。

- [ ] **Step 2: 运行全部相关单元测试**

```bash
npx tsx src/utils/__tests__/modelFamily.test.ts
npx tsx src/utils/__tests__/pedalMode.test.ts
```
验证: 所有用例全绿通过。

- [ ] **Step 3: 运行 Rust 静态检查与单测 (保持零回归)**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml official_alignment_tests
```
验证: Rust 侧全绿。
