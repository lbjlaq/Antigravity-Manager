# 📊 МАСТЕР ТАБЛИЦА ВСЕХ МОДЕЛЕЙ ANTIGRAVITY

**Дата**: 2026-01-12
**Источники**: Код + Документация + UI + Reverse Engineering
**Общий прогресс**: 72.2% (39/54+ моделей)
**🎉 UPDATE**: Epic-011 (API Migration) COMPLETE ✅ - Epic-010 COMPLETE (via Epic-011) ✅
**📊 VALIDATION**: Epic-010 независимая валидация завершена (2026-01-12) - см. `docs/analysis/EPIC-010-VALIDATION-ANALYSIS.md`

---

## 🎯 СВОДНАЯ СТАТИСТИКА

```yaml
ВСЕГО МОДЕЛЕЙ: 54+
  Задокументировано: 39 (72.2%)
  Осталось: 15+ (27.8%)

ПО КАТЕГОРИЯМ:
  Gemini: 27/42+ (64.3%)
  Claude: 8/8 (100% ✅)
  OpenAI: 4/4 (100% ✅)

ПО ТИПАМ ДОКУМЕНТОВ:
  Base Workflows: 30 ✅
  Thinking Workflows: 9 ✅
  COMPARISON Files: 3 ✅
  Всего файлов: 42
```

---

## 📋 ПОЛНАЯ ТАБЛИЦА ВСЕХ МОДЕЛЕЙ

### Легенда
- ✅ = Документ существует
- ❌ = Документ НЕ существует
- ⏳ = TODO (планируется)
- ❓ = Неизвестно
- 🔴 = HIGH priority
- 🟡 = MEDIUM priority
- 🟢 = LOW priority

---

## 1️⃣ GEMINI MODELS (27/42+ задокументировано)

### 1.1 Gemini 3.x Series (6/7 - 85.7% ✅)

| # | Model Name | UI Display | Base Workflow | Thinking Workflow | COMPARISON | Status | Priority | Notes |
|---|------------|------------|---------------|-------------------|------------|--------|----------|-------|
| 1 | gemini-3-pro-high | Gemini 3 Pro (High) | ✅ 38KB | ✅ 56KB | ✅ 25KB | DONE | - | v2.0, 32000 budget, Epic-005 ✅ |
| 2 | gemini-3-pro-low | Gemini 3 Pro (Low) | ✅ 40KB | ✅ 43KB | ✅ 42KB | EPIC-009 🔄 | 🔴 | v2.0, 32000 budget, In Progress Команда 2 |
| 3 | gemini-3-flash | Gemini 3 Flash | ✅ 45KB | ✅ 58KB | ✅ 38KB | ✅ COMPLETE | - | v2.0, 4 levels, ✅ Epic-010 (via Epic-011), ✅ Epic-013 COMPLETE (2026-01-12, 100% compliance) |
| 4 | gemini-3-pro-image | - | ✅ 43KB | ❌ N/A | ✅ 33KB | EPIC-007 🔄 | 🔴 | v2.0, 21 variants, In Progress Команда 1 |

**Документов**: 9/10 (90%)
**Критические открытия**:
- ✅ **RESOLVED**: Epic-011 API Migration COMPLETE (2026-01-12) - Epic-010 COMPLETE (via Epic-011)
- ⚠️ **API Breaking Change**: Gemini 3 использует `thinkingLevel` (MINIMAL/LOW/MEDIUM/HIGH)
- ⭐ **Budget Equality**: Pro Low = Pro High (32000 tokens)
- ⭐ **MEDIUM Level**: Только у Flash, НЕТ у Pro

**Completed Epics** (Q2 2026):
- **Epic-010** (Flash): ✅ COMPLETE (via Epic-011) - P0/P1 stories реализованы
- **Epic-011** (API Migration): ✅ COMPLETE (2026-01-12)
- **Epic-013** (Flash Phases 2+3): ✅ COMPLETE (2026-01-12, 100% compliance, 398/398 tests, 10/10 QA)
- **Epic-024** (Anti-Detection): ✅ COMPLETE (2026-01-12, P0 CRITICAL resolved, merged a079136, 10/10 QA)
- **Epic-015** (Pro Optimization): ✅ COMPLETE (2026-01-12, 16.4% savings, 89% accuracy, 112/113 tests, merged to main)

**Next Epic** (Ready to Start):
- **Epic-017** (Claude Sonnet Standard): 📋 READY, P1 HIGH, 9-11h (1.5 weeks), 3 stories, 90% code reuse

**Active Epics**:
- **Epic-007** (Image): 🔄 IN PROGRESS, Команда 1, 2026-01-11 → 2026-01-21
- **Epic-009** (Low): 🔄 IN PROGRESS, Команда 2, 2026-01-11 → 2026-01-25

**Validation Reference**:
- 📊 Epic-010 Independent Validation: `docs/analysis/EPIC-010-VALIDATION-ANALYSIS.md` (1283 строки, 2026-01-12)
- ✅ Подтверждение: Epic-010 Stories 1-5 (P0/P1) - 100% implemented via Epic-011
- 📋 Epic-010 Stories 6-7 (P2) - planned in Epic-013 (Q2 2026)

---

### 1.2 Gemini 2.5 Production (8/8 - 100% ✅)

| # | Model Name | UI Display | Base Workflow | Thinking Workflow | COMPARISON | Status | Priority | Notes |
|---|------------|------------|---------------|-------------------|------------|--------|----------|-------|
| 1 | gemini-2.5-flash | - | ✅ | ❌ | ❌ | DONE | - | v1.0, no thinking |
| 2 | gemini-2.5-flash-thinking | - | ❌ | ✅ | ❌ | DONE | - | v1.0, 24576 budget |
| 3 | gemini-2.5-flash-thinking-tools | - | ❌ | ✅ | ❌ | DONE | - | v1.0, 24576 budget |
| 4 | gemini-2.5-flash-lite | - | ✅ | ❌ | ❌ | DONE | - | v1.0, no thinking |
| 5 | gemini-2.5-pro | - | ✅ | ❌ | ❌ | DONE | - | v1.0, no thinking |
| 6 | gemini-2.5-flash-image-preview | - | ✅ | ❌ | ❌ | DONE | - | v1.0, image |
| 7 | gemini-2.5-pro-eval | - | ✅ | ❌ | ❌ | DONE | - | v1.0, evaluation |
| 8 | gemini-for-google-2.5-pro | - | ✅ | ❌ | ❌ | DONE | - | v1.0, Google |

**Документов**: 8/8 (100%)

---

### 1.3 Gemini 2.5 Thinking Variants (1/2 - 50% ✅)

| # | Model Name | UI Display | Base Workflow | Thinking Workflow | COMPARISON | Status | Priority | Notes |
|---|------------|------------|---------------|-------------------|------------|--------|----------|-------|
| 1 | gemini-2.5-pro-thinking | - | ❌ N/A | ✅ | ✅ 33KB | ✅ COMPLETE | - | ✅ Epic-015 COMPLETE (2026-01-12, 95%+, 16.4% cost savings, 89% accuracy) |
| 2 | gemini-2.5-flash-lite-thinking | - | ❌ N/A | ❌ NOT SUPPORTED | ❌ | BLOCKED ⚫ | ⚫ N/A | ❌ Model does NOT support thinking (Epic-006 blocked) |

**Документов**: 1/1 (100% - gemini-2.5-pro-thinking ✅, flash-lite-thinking blocked)

**Epic Reference**:
- **Epic-015** (Gemini 2.5 Pro Thinking): ✅ COMPLETE (2026-01-12), validation: `docs/qa/epic-015-FINAL-QA-REPORT.md`

---

### 1.4 Gemini 2.0 Series (0/2 - TODO)

| # | Model Name | UI Display | Base Workflow | Thinking Workflow | COMPARISON | Status | Priority | Notes |
|---|------------|------------|---------------|-------------------|------------|--------|----------|-------|
| 1 | gemini-2.0-flash-exp | - | ⏳ | ⏳ | ⏳ | TODO | 🟡 MEDIUM | Experimental |

**Документов**: 0/3 (0%)
**В коде**: `model_mapping.rs:120`

---

### 1.5 Experimental Models (12/12 - 100% ✅)

| # | Model ID | Codename | UI Display | Base Workflow | Thinking | COMPARISON | Status |
|---|----------|----------|------------|---------------|----------|------------|--------|
| 1 | 328 | NEMOSREEF | - | ✅ | ❓ | ❌ | DONE |
| 2 | 336 | HORIZONDAWN | - | ✅ | ❓ | ❌ | DONE |
| 3 | 337 | PUREPRISM | - | ✅ | ❓ | ❌ | DONE |
| 4 | 338 | GENTLEISLAND | - | ✅ | ❓ | ❌ | DONE |
| 5 | 339 | RAINSONG | - | ✅ | ❓ | ❌ | DONE |
| 6 | 343 | ORIONFIRE | - | ✅ | ❓ | ❌ | DONE |
| 7 | 347 | COSMICFORGE | - | ✅ | ❓ | ❌ | DONE |
| 8 | 348 | RIFTRUNNER | - | ✅ | ❌ | ❌ | DONE |
| 9 | 350 | INFINITYJET | - | ✅ | ❓ | ❌ | DONE |
| 10 | 351 | INFINITYBLOOM | - | ✅ | ❓ | ❌ | DONE |
| 11 | 352 | RIFTRUNNER_THINKING_LOW | - | ❌ | ✅ ~1000 | ❌ | DONE |
| 12 | 353 | RIFTRUNNER_THINKING_HIGH | - | ❌ | ✅ ~10000 | ❌ | DONE |

**Документов**: 12/12 (100%)
**Стандарт**: v1.0 (experimental)
**Примечание**: Внутренние/тестовые модели Google

---

### 1.6 Special Models (1/1 - 100% ✅)

| # | Model Name | UI Display | Base Workflow | Thinking Workflow | COMPARISON | Status | Notes |
|---|------------|------------|---------------|-------------------|------------|--------|-------|
| 1 | gemini-computer-use-experimental | - | ✅ | ❓ | ❌ | DONE | Browser control |

**Документов**: 1/1 (100%)

---

### 1.7 Missing Model IDs (0/14+ - TODO)

| Range | Predicted Count | UI Display | Base Workflow | Thinking Workflow | COMPARISON | Status | Priority |
|-------|-----------------|------------|---------------|-------------------|------------|--------|----------|
| 314-327 | 14 моделей | ❓ | ⏳ | ⏳ | ⏳ | TODO | 🔴 HIGH |
| 331 | 1 модель | ❓ | ⏳ | ⏳ | ⏳ | TODO | 🟢 LOW |
| 333-335 | 3 модели | ❓ | ⏳ | ⏳ | ⏳ | TODO | 🟢 LOW |
| 340-342 | 3 модели | ❓ | ⏳ | ⏳ | ⏳ | TODO | 🟢 LOW |
| 344-346 | 3 модели | ❓ | ⏳ | ⏳ | ⏳ | TODO | 🟢 LOW |
| 349 | 1 модель | ❓ | ⏳ | ⏳ | ⏳ | TODO | 🟢 LOW |

**Всего**: 25+ моделей
**Примечание**: Требуется исследование для определения имен

---

## 2️⃣ CLAUDE MODELS (8/8 - 100% ✅)

### 2.1 Claude 4.5 Series (4/4 - 100% ✅)

| # | Model Name | UI Display | Base Workflow | Thinking Workflow | COMPARISON | Status | Notes |
|---|------------|------------|---------------|-------------------|------------|--------|-------|
| 1 | claude-sonnet-4-5 | Claude Sonnet 4.5 | ✅ | ❌ | ❌ | DONE | v1.0, base |
| 2 | claude-sonnet-4-5-thinking | Claude Sonnet 4.5 (Thinking) | ❌ | ✅ | ❌ | DONE | v1.0, 32000 |
| 3 | claude-haiku-4-5 | - | ✅ | ❌ | ❌ | DONE | v1.0, routes to gemini-3-pro-high |
| 4 | claude-haiku-4-5-thinking | - | ❌ | ✅ | ❌ | DONE | v1.0, via gemini |

**Документов**: 4/4 (100%)
**В UI**: Sonnet 4.5 (base + thinking)
**Примечание**: Haiku роутится в Gemini 3 Pro High

---

### 2.2 Claude 4 Series (4/4 - 100% ✅)

| # | Model Name | UI Display | Base Workflow | Thinking Workflow | COMPARISON | Status | Notes |
|---|------------|------------|---------------|-------------------|------------|--------|-------|
| 1 | claude-opus-4-5 | - | ✅ | ❌ | ❌ | DONE | v1.0, base (rare) |
| 2 | claude-opus-4-5-thinking | Claude Opus 4.5 (Thinking) | ❌ | ✅ | ❌ | DONE | v1.0, 32000 |
| 3 | claude-4-sonnet | - | ✅ | ❌ | ❌ | DONE | v1.0, base |
| 4 | claude-4-sonnet-thinking | - | ❌ | ✅ | ❌ | DONE | v1.0, 32000 |

**Документов**: 4/4 (100%)
**В UI**: Opus 4.5 (Thinking)
**Примечание**: Google предоставляет Opus ТОЛЬКО с thinking

---

### 2.3 Claude Aliases (Routing Only - No Separate Docs)

| Alias | Routes To | In Code | Documented |
|-------|-----------|---------|------------|
| claude-3-5-sonnet-20241022 | claude-sonnet-4-5 | ✅ | ❌ (alias) |
| claude-3-5-sonnet-20240620 | claude-sonnet-4-5 | ✅ | ❌ (alias) |
| claude-sonnet-4-5-20250929 | claude-sonnet-4-5-thinking | ✅ | ❌ (alias) |
| claude-opus-4 | claude-opus-4-5-thinking | ✅ | ❌ (alias) |
| claude-opus-4-5-20251101 | claude-opus-4-5-thinking | ✅ | ❌ (alias) |
| claude-opus-4-5-high | claude-opus-4-5-thinking | ✅ | ❌ (alias) |
| claude-haiku-4 | gemini-3-pro-high | ✅ | ❌ (routes) |
| claude-3-haiku-20240307 | gemini-3-pro-high | ✅ | ❌ (routes) |
| claude-haiku-4-5-20251001 | gemini-3-pro-high | ✅ | ❌ (routes) |

**Примечание**: Aliases не требуют отдельной документации, роутятся в основные модели

---

## 3️⃣ OPENAI MODELS (4/4 - 100% ✅)

### 3.1 OpenAI via Vertex AI (4/4 - 100% ✅)

| # | Model Name | UI Display | Base Workflow | Thinking Workflow | COMPARISON | Status | Notes |
|---|------------|------------|---------------|-------------------|------------|--------|-------|
| 1 | openai-gpt-oss-120b-medium | GPT-OSS 120B (Medium) | ✅ | ❌ | ❌ | DONE | v1.0, BYOK |
| 2 | gpt-4.1-web-search | - | ✅ | ❌ | ❌ | DONE | v1.0, web search |
| 3 | o3-web-search | - | ✅ | ❌ | ❌ | DONE | v1.0, web search |
| 4 | o4-mini-web-search | - | ✅ | ❌ | ❌ | DONE | v1.0, web search |

**Документов**: 4/4 (100%)
**В UI**: GPT-OSS 120B (Medium)
**Локация документов**: `docs/antigravity/workflows/models/openai/`

---

### 3.2 OpenAI Aliases (Routing Only - No Separate Docs)

| Alias | Routes To | In Code | Documented |
|-------|-----------|---------|------------|
| gpt-4 | gemini-2.5-pro | ✅ | ❌ (routes) |
| gpt-4-turbo | gemini-2.5-pro | ✅ | ❌ (routes) |
| gpt-4-turbo-preview | gemini-2.5-pro | ✅ | ❌ (routes) |
| gpt-4-0125-preview | gemini-2.5-pro | ✅ | ❌ (routes) |
| gpt-4-1106-preview | gemini-2.5-pro | ✅ | ❌ (routes) |
| gpt-4-0613 | gemini-2.5-pro | ✅ | ❌ (routes) |
| gpt-4o | gemini-2.5-pro | ✅ | ❌ (routes) |
| gpt-4o-2024-05-13 | gemini-2.5-pro | ✅ | ❌ (routes) |
| gpt-4o-2024-08-06 | gemini-2.5-pro | ✅ | ❌ (routes) |
| gpt-4o-mini | gemini-2.5-flash | ✅ | ❌ (routes) |
| gpt-4o-mini-2024-07-18 | gemini-2.5-flash | ✅ | ❌ (routes) |
| gpt-3.5-turbo | gemini-2.5-flash | ✅ | ❌ (routes) |
| gpt-3.5-turbo-16k | gemini-2.5-flash | ✅ | ❌ (routes) |
| gpt-3.5-turbo-0125 | gemini-2.5-flash | ✅ | ❌ (routes) |
| gpt-3.5-turbo-1106 | gemini-2.5-flash | ✅ | ❌ (routes) |
| gpt-3.5-turbo-0613 | gemini-2.5-flash | ✅ | ❌ (routes) |

**Примечание**: OpenAI модели роутятся в Gemini для совместимости

---

## 4️⃣ НЕДОКУМЕНТИРОВАННЫЕ МОДЕЛИ (15+ моделей)

### 4.1 HIGH Priority - Epics In Progress/Planned

| # | Model Name | UI Display | Base Workflow | Thinking Workflow | COMPARISON | Priority | Epic Status | Notes |
|---|------------|------------|---------------|-------------------|------------|----------|-------------|-------|
| 1 | gemini-3-pro-image | - | ✅ Done | ❌ N/A | ✅ Done | 🔴 HIGH | EPIC-007 🔄 | In Progress, Команда 1, 2026-01-11→21 |
| 2 | gemini-3-pro-low | - | ✅ Done | ✅ Done | ✅ Done | 🔴 HIGH | EPIC-009 🔄 | In Progress, Команда 2, 2026-01-11→25 |
| 3 | gemini-2.5-pro-thinking | - | ❌ N/A | ⏳ | ✅ Done | 🔴 HIGH | EPIC-015 📋 | Planned Q2 2026, validation: EPIC-015-VALIDATION-REPORT.md |
| 4 | gemini-3-flash | - | ✅ Done | ✅ Done | ✅ Done | 🔴 HIGH | ✅ COMPLETE | Epic-010 реализован через Epic-011 (P0/P1), Epic-013 (P2) |

### 4.2 CRITICAL Infrastructure Epic

| # | Epic Name | Type | Duration | Priority | Status | Notes |
|---|-----------|------|----------|----------|--------|-------|
| 1 | Epic-011: API Migration | Infrastructure | 2 weeks | 🚨 P0 | ✅ COMPLETE | Gemini 3 thinkingBudget→thinkingLevel, 75/75 tests passing (100%) |

**Completion**: Epic-011 (2026-01-12) ✅ COMPLETE → Epic-010 COMPLETE (P0/P1 stories implemented)
**Validation**: Epic-010 independent analysis confirms Stories 1-5 реализованы в Epic-011 (см. `EPIC-010-VALIDATION-ANALYSIS.md`)

### 4.3 HIGH Priority - Future Investigation (14 моделей)

| # | Model Name | UI Display | Base Workflow | Thinking Workflow | COMPARISON | Priority | Effort | Notes |
|---|------------|------------|---------------|-------------------|------------|----------|--------|-------|
| 1-14 | Model IDs 314-327 | ❓ | ⏳ | ⏳ | ⏳ | 🔴 HIGH | 2-3 недели | 14 моделей gap, needs research |

**Документов нужно**: ~28 (14 моделей × 2 docs avg)

---

### 4.4 MEDIUM Priority (2 модели - 4 дня)

| # | Model Name | UI Display | Base Workflow | Thinking Workflow | COMPARISON | Priority | Effort | Notes |
|---|------------|------------|---------------|-------------------|------------|----------|--------|-------|
| 1 | gemini-2.0-flash-exp | - | ⏳ | ⏳ | ⏳ | 🟡 MEDIUM | 2 дня | Experimental, audio focus |
| 2 | gemini-2.5-flash-lite-thinking | - | ❌ N/A | ❌ | ❌ | ⚫ BLOCKED | N/A | ❌ Model does NOT support thinking |

**Документов нужно**: ~3 (только gemini-2.0-flash-exp)

---

### 4.5 LOW Priority (~10 моделей - переменно)

| Range | Description | Priority | Estimated Count | Notes |
|-------|-------------|----------|-----------------|-------|
| 331, 333-335, 340-342, 344-346, 349 | Gap IDs | 🟢 LOW | ~10 моделей | May be duplicates or Vertex variants |

**Документов нужно**: ~20+

---

## 📊 СВОДКА ПО ТИПАМ ДОКУМЕНТОВ

### Созданные Документы (42 файла)

```yaml
Gemini Models (27 файлов):
  gemini/ directory:
    - gemini-2.5-flash-workflow.md
    - gemini-2.5-flash-thinking-workflow.md
    - gemini-2.5-flash-thinking-tools-workflow.md
    - gemini-2.5-flash-lite-workflow.md
    - gemini-2.5-pro-workflow.md
    - gemini-2.5-flash-image-preview-workflow.md
    - gemini-2.5-pro-eval-workflow.md
    - gemini-for-google-2.5-pro-workflow.md
    - gemini-3-pro-high-workflow.md
    - gemini-3-pro-high-COMPARISON.md
    - gemini-3-pro-low-workflow.md
    - gemini-3-pro-low-COMPARISON.md
    - gemini-3-flash-workflow.md
    - gemini-3-flash-COMPARISON.md
    - cosmicforge-experimental-workflow.md
    - gemini-computer-use-experimental-workflow.md
    - gentleisland-experimental-workflow.md
    - horizondawn-experimental-workflow.md
    - infinitybloom-experimental-workflow.md
    - infinityjet-experimental-workflow.md
    - nemosreef-experimental-workflow.md
    - orionfire-experimental-workflow.md
    - pureprism-experimental-workflow.md
    - rainsong-experimental-workflow.md
    - riftrunner-experimental-workflow.md
    - riftrunner-thinking-low-experimental-workflow.md
    - riftrunner-thinking-high-experimental-workflow.md

  openai/ directory (thinking variants):
    - gemini-3-pro-high-thinking-workflow.md
    - gemini-3-pro-low-thinking-workflow.md
    - gemini-3-flash-thinking-workflow.md

Claude Models (8 файлов):
  claude/ directory:
    - claude-4-5-sonnet-workflow.md
    - claude-4-5-sonnet-thinking-workflow.md
    - claude-4-5-haiku-workflow.md
    - claude-4-5-haiku-thinking-workflow.md
    - claude-4-opus-workflow.md
    - claude-4-opus-thinking-workflow.md
    - claude-4-sonnet-workflow.md
    - claude-4-sonnet-thinking-workflow.md

OpenAI Models (4 файла):
  openai/ directory:
    - openai-gpt-oss-120b-medium-workflow.md
    - gpt-4.1-web-search-workflow.md
    - o3-web-search-workflow.md
    - o4-mini-web-search-workflow.md

Итого: 42 файла
```

---

## 🎯 МОДЕЛИ ВИДИМЫЕ В UI (7 моделей)

**Из вашего скриншота**:

| # | UI Display | Actual Model ID | Base Doc | Thinking Doc | COMPARISON | Status |
|---|------------|-----------------|----------|--------------|------------|--------|
| 1 | Gemini 3 Pro (High) | gemini-3-pro-high | ✅ | ✅ | ✅ | DONE |
| 2 | Gemini 3 Pro (Low) | gemini-3-pro-low | ✅ | ✅ | ✅ | DONE |
| 3 | Gemini 3 Flash **New** | gemini-3-flash | ✅ | ✅ | ✅ | DONE |
| 4 | Claude Sonnet 4.5 | claude-sonnet-4-5 | ✅ | ❌ | ❌ | DONE |
| 5 | Claude Sonnet 4.5 (Thinking) | claude-sonnet-4-5-thinking | ❌ | ✅ | ❌ | DONE |
| 6 | Claude Opus 4.5 (Thinking) | claude-opus-4-5-thinking | ✅ | ✅ | ❌ | DONE |
| 7 | GPT-OSS 120B (Medium) | openai-gpt-oss-120b-medium | ✅ | ❌ | ❌ | DONE |

**Документация UI моделей**: 100% ✅ (все модели из UI задокументированы!)

---

## 🔍 ДОПОЛНИТЕЛЬНЫЕ МОДЕЛИ (не в UI, но в коде)

### Gemini Models (20+ моделей)

```yaml
Production:
  - gemini-2.5-flash (base) ✅
  - gemini-2.5-flash-thinking ✅
  - gemini-2.5-flash-thinking-tools ✅
  - gemini-2.5-flash-lite ✅
  - gemini-2.5-pro ✅
  - gemini-2.5-flash-image-preview ✅
  - gemini-2.5-pro-eval ✅
  - gemini-for-google-2.5-pro ✅

Gemini 3.x:
  - gemini-3-pro-image ⏳ TODO

Missing Thinking:
  - gemini-2.5-pro-thinking ⏳ TODO
  - gemini-2.5-flash-lite-thinking ⏳ TODO

Experimental (12 моделей):
  - NEMOSREEF, HORIZONDAWN, PUREPRISM, GENTLEISLAND ✅
  - RAINSONG, ORIONFIRE, COSMICFORGE ✅
  - RIFTRUNNER (base + thinking LOW/HIGH) ✅
  - INFINITYJET, INFINITYBLOOM ✅
  - Computer Use ✅

Unknown (25+ моделей):
  - Model IDs 314-327 ⏳ TODO
  - Other gaps (331, 333-335, etc.) ⏳ TODO
```

### Claude Models (все в UI)

```yaml
В UI:
  - Claude Sonnet 4.5 (base + thinking) ✅
  - Claude Opus 4.5 (thinking) ✅

Дополнительно документированы:
  - Claude Haiku 4.5 (routes to Gemini) ✅
  - Claude 4 Sonnet (base + thinking) ✅
  - Claude 4 Opus (base + thinking) ✅
```

### OpenAI Models (1 в UI)

```yaml
В UI:
  - GPT-OSS 120B (Medium) ✅

Дополнительно документированы:
  - GPT 4.1 Web Search ✅
  - O3 Web Search ✅
  - O4 Mini Web Search ✅

Роутятся в Gemini:
  - gpt-4, gpt-4o, gpt-4o-mini → gemini-2.5-pro/flash
  - gpt-3.5-turbo → gemini-2.5-flash
```

---

## 📈 ПРИОРИТЕТЫ ДОКУМЕНТИРОВАНИЯ

### 🔴 HIGH Priority (16 моделей → ~32 документа)

**Немедленно** (2 модели, 4 документа, 2 дня):
1. gemini-3-pro-image (base + COMPARISON)
2. gemini-2.5-pro-thinking (thinking + COMPARISON)

**Исследование** (14 моделей, 28+ документов, 2-3 недели):
3. Model IDs 314-327 - требуется reverse engineering для определения названий

---

### 🟡 MEDIUM Priority (2 модели → ~5 документов, 4 дня)

1. gemini-2.0-flash-exp (base + thinking + COMPARISON)
2. gemini-2.5-flash-lite-thinking (thinking + COMPARISON)

---

### 🟢 LOW Priority (~10 моделей → ~20 документов)

1. Gap Model IDs (331, 333-335, 340-342, 344-346, 349)
   - Могут быть дубликатами или Vertex вариантами
   - Требуется исследование

---

## 🎯 ТЕКУЩИЕ EPICS И ROADMAP

### Active Epics (In Progress)

```yaml
epic_007_gemini_3_pro_image:
  team: "Команда 1"
  timeline: "2026-01-11 → 2026-01-21 (10 days)"
  status: "🔄 IN PROGRESS"
  compliance: "86.7% → 100%"
  stories: 5
  focus: "End-to-End Testing, Safety Settings, Caching"

epic_009_gemini_3_pro_low:
  team: "Команда 2"
  timeline: "2026-01-11 → 2026-01-25 (14 days)"
  status: "🔄 IN PROGRESS"
  compliance: "82.1% → 100%"
  stories: 6
  focus: "P0/P1 gaps, thinking mode, cost optimization"
  note: "Will benefit from Epic-011 API migration"
```

### Completed Epics

```yaml
epic_011_api_migration:
  team: "3 developers + QA"
  completion_date: "2026-01-12"
  status: "✅ COMPLETE"
  priority: "🚨 P0 CRITICAL"
  type: "Infrastructure"
  focus: "Gemini 3 thinkingBudget → thinkingLevel API"
  test_results: "75/75 tests passing (100%)"
  result: "Epic-010 COMPLETE (P0/P1 stories implemented) ✅"
  improves: "Epic-009 (Low thinking mode) ✅"
  stories: "6/6 complete"
```

### Planned Epics (Next in Queue)

```yaml
epic_015_gemini_2_5_pro_thinking:
  status: "📋 PLANNED Q2 2026"
  validation: "docs/qa/EPIC-015-VALIDATION-REPORT.md"
  compliance: "90.6% → 100%"
  stories: 3
  focus: "Adaptive Budget Optimization, Cache Monitoring"
  note: "Renumbered from Epic-008/012 to resolve conflict with Epic-012 CI/CD"
```

### Completed Epics (via Epic-011)

```yaml
epic_010_gemini_3_flash:
  status: "✅ COMPLETE (via Epic-011)"
  validation: "docs/analysis/EPIC-010-VALIDATION-ANALYSIS.md (1283 строки)"
  stories_complete: "Stories 1-5 (P0/P1) - 100% implemented via Epic-011"
  stories_deferred: "Stories 6-7 (P2) - planned in Epic-013 (Q2 2026)"
  compliance: "85% (P0/P1 complete)"
  next_phase: "Epic-013 (Phases 2+3) for P2 optimization → 95%+ compliance"
  test_results: "75/75 tests passing via Epic-011 (100%)"
  conclusion: "Epic-010 as separate epic NOT NEEDED"
```

### Q1 2026 Strategic Milestones

```yaml
milestone_0_completed:
  date: "2026-01-12 ✅"
  achievement: "Epic-011 API Migration + Epic-010 validation complete"
  impact: "Gemini 3 API debt eliminated, Epic-010 COMPLETE (P0/P1 via Epic-011)"
  test_results: "75/75 tests passing (100%)"
  validation: "Epic-010 independent analysis confirms completion"
  status: "✅ COMPLETE"

milestone_1:
  date: "2026-02-16"
  achievement: "Epic-007/009/015 planning complete"
  impact: "Gemini 3.x Image/Low in progress, Gemini 2.5 Pro Thinking validated"
  status: "📋 IN PROGRESS"

milestone_2:
  date: "Q2 2026"
  decision: "Epic-013 (Flash Phase 2+3) OR Epic-015 (Pro Thinking)"
  recommendation: "Strategic Review for Q2 priority"
  note: "Epic-010 P2 stories (6-7) planned in Epic-013"
```

## 🎯 СЛЕДУЮЩИЕ ШАГИ (После текущих Epics)

### ✅ Completed: Epic-011 API Migration (CRITICAL)

```yaml
priority: "🚨 P0 - HIGHEST"
completion_date: "2026-01-12 ✅"
effort: "3 developers + QA + tech writer"
result: "Epic-010 COMPLETE (P0/P1 stories) ✅"
validation: "Epic-010 independent analysis confirms 100% P0/P1 implementation"
benefit: "Gemini 3 API technical debt eliminated, 100% series compliance enabled"
test_results: "75/75 tests passing (100%)"

deliverables_completed:
  phase_1: "✅ API detection, thinkingLevel implementation, validation"
  phase_2: "✅ Flash auto-injection (71/71 tests), documentation"

key_achievements:
  - "Detection pattern updated: Flash NOW INCLUDED"
  - "All protocols validated (OpenAI 12/12, Claude 11/11, Gemini 10/10)"
  - "Flash auto-injection working in production"
```

### Option A: Complete Epic-010 P2 Stories via Epic-013 Phases 2+3

```yaml
модель: gemini-3-flash
p0_p1_status: "✅ COMPLETE via Epic-011"
p2_stories: "Stories 6-7 (Adaptive Optimization, Monitoring) - planned in Epic-013"
current_compliance: "85% (P0/P1 complete)"
target_compliance: "95%+ (with P2 optimization)"
timeline: "Epic-013 (Q2 2026)"
benefit: "Gemini 3 Flash 95%+ compliance with advanced optimization"
status: "✅ P0/P1 COMPLETE, P2 planned in Epic-013"
note: "Epic-010 as separate epic NOT NEEDED per validation analysis"
```

### Option B: Strategic Review Week (Recommended)

```yaml
timeline: "1 week (2026-03-04 → 2026-03-10)"
team: "PO + Tech Lead + 1 developer"
deliverables:
  - "Q1 Retrospective"
  - "Usage Metrics Analysis"
  - "Q2 Roadmap (Epic-013 vs Epic-015 prioritization)"
benefit: "Data-driven Q2 planning, Epic-010 P2 stories in Epic-013"
```

### Option C: Future Investigation - Model IDs 314-327

```yaml
models: "14 models (gap investigation)"
effort: "2-3 weeks"
priority: "🔴 HIGH (deferred to Q2)"
complexity: "Requires reverse engineering"
benefit: "~98% documentation coverage"
```

---

## 📊 СТАТИСТИКА ПО СТАНДАРТАМ

### v2.0 Standard (9 файлов - Gemini 3.x)

```yaml
модели:
  - Gemini 3 Pro High (base + thinking + COMPARISON)
  - Gemini 3 Pro Low (base + thinking + COMPARISON)
  - Gemini 3 Flash (base + thinking + COMPARISON)

особенности:
  - 15+ секций
  - 4 Configuration Profiles
  - COMPARISON с gap analysis
  - Roadmap для исправлений

размер: ~385KB
```

### v1.0 Standard (33 файла - все остальные)

```yaml
модели:
  - Gemini 2.5 Production (8 моделей)
  - Gemini Experimental (12 моделей)
  - Gemini Special (1 модель)
  - Claude (8 моделей)
  - OpenAI (4 модели)

особенности:
  - 10-12 секций (меньше чем v2.0)
  - Базовые configuration examples
  - Без COMPARISON файлов

размер: ~300KB+
```

---

## 🎉 ИТОГОВАЯ СТАТИСТИКА

### Прогресс Документации

```yaml
ЗАДОКУМЕНТИРОВАНО: 39/54+ (72.2%)

ПО КАТЕГОРИЯМ:
  Gemini: 27/42+ (64.3%)
    - Production: 8/8 (100%)
    - Gemini 3.x: 6/7 (85.7%)
    - Experimental: 12/12 (100%)
    - Special: 1/1 (100%)

  Claude: 8/8 (100% ✅)
  OpenAI: 4/4 (100% ✅)

ПО ПРИОРИТЕТАМ:
  HIGH: 6/22 (27.3%) - 16 моделей осталось
  MEDIUM: 0/2 (0%) - 2 модели осталось
  LOW: 0/10+ (0%) - 10+ моделей осталось

ПО ТИПАМ DOCS:
  Base Workflows: 30 ✅
  Thinking Workflows: 9 ✅
  COMPARISON Files: 3 ✅ (только Gemini 3.x)

РАЗМЕР ДОКУМЕНТАЦИИ: ~685KB
```

### Epic Execution Status

**Active Development** (2026-01-11):
1. **Epic-007** (Gemini 3 Pro Image) - 🔄 IN PROGRESS, Команда 1, 10 days
2. **Epic-009** (Gemini 3 Pro Low) - 🔄 IN PROGRESS, Команда 2, 14 days

**Completed** (2026-01-12):
1. **Epic-011** (API Migration) - ✅ COMPLETE, 75/75 tests passing (100%)
2. **Epic-010** (Gemini 3 Flash P0/P1) - ✅ COMPLETE via Epic-011, validation: EPIC-010-VALIDATION-ANALYSIS.md

**Planned Q2 2026**:
1. **Epic-013** (Gemini 3 Flash Phases 2+3) - 📋 PLANNED, includes Epic-010 P2 stories
2. **Epic-015** (Gemini 2.5 Pro Thinking) - 📋 PLANNED, validation: EPIC-015-VALIDATION-REPORT.md

---

**Обновлено**: 2026-01-12 (Epic-011 completion + Epic-010 validation + Epic renumbering)
**Источники**: model_mapping.rs + docs files + UI screenshot + Epic planning docs + Epic-011 test results + Epic-010 validation analysis
**Статус**: ✅ Полная инвентаризация завершена + Epic roadmap updated + Epic-010 validated
**🎉 КРИТИЧЕСКИЕ ДОСТИЖЕНИЯ**:
- Epic-011 API Migration COMPLETE ✅ (75/75 tests passing)
- Epic-010 COMPLETE via Epic-011 ✅ (P0/P1 stories implemented, independent validation confirmed)
- Epic renumbering: Epic-008/012 → Epic-015 (Gemini 2.5 Pro Thinking)
**Воздействие**: Epic-010 P0/P1 COMPLETE, Flash auto-injection working, Gemini 3 API debt eliminated, P2 optimization stories planned in Epic-013
