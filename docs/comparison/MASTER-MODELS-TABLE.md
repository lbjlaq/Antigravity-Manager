# 📊 МАСТЕР ТАБЛИЦА ВСЕХ МОДЕЛЕЙ ANTIGRAVITY

**Дата**: 2026-01-11
**Источники**: Код + Документация + UI + Reverse Engineering
**Общий прогресс**: 72.2% (39/54+ моделей)

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
| 1 | gemini-3-pro-high | Gemini 3 Pro (High) | ✅ 38KB | ✅ 56KB | ✅ 25KB | DONE | - | v2.0, 32000 budget |
| 2 | gemini-3-pro-low | Gemini 3 Pro (Low) | ✅ 40KB | ✅ 43KB | ✅ 42KB | DONE | - | v2.0, 32000 budget |
| 3 | gemini-3-flash | Gemini 3 Flash | ✅ 45KB | ✅ 58KB | ✅ 38KB | DONE | - | v2.0, 4 levels, ⚠️ API |
| 4 | gemini-3-pro-image | - | ✅ 43KB | ❌ N/A | ✅ 33KB | EPIC-007 | 🔴 | v2.0, 21 variants, Epic planning |

**Документов**: 9/10 (90%)
**Критические открытия**:
- ⚠️ **API Breaking Change**: Gemini 3 использует `thinkingLevel` (MINIMAL/LOW/MEDIUM/HIGH)
- ⭐ **Budget Equality**: Pro Low = Pro High (32000 tokens)
- ⭐ **MEDIUM Level**: Только у Flash, НЕТ у Pro

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

### 1.3 Gemini 2.5 Missing Thinking Variants (0/2 - TODO)

| # | Model Name | UI Display | Base Workflow | Thinking Workflow | COMPARISON | Status | Priority | Notes |
|---|------------|------------|---------------|-------------------|------------|--------|----------|-------|
| 1 | gemini-2.5-pro-thinking | - | ❌ N/A | ⏳ | ✅ 33KB | EPIC-008 | 🔴 HIGH | 32000 budget, 90.6%, Next after 007 |
| 2 | gemini-2.5-flash-lite-thinking | - | ❌ N/A | ❌ NOT SUPPORTED | ❌ | BLOCKED | ⚫ N/A | ❌ Model does NOT support thinking (Epic-006 blocked) |

**Документов**: 0/4 (0%)

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

### 4.1 HIGH Priority (16 моделей - 2-3 недели)

| # | Model Name | UI Display | Base Workflow | Thinking Workflow | COMPARISON | Priority | Effort | Notes |
|---|------------|------------|---------------|-------------------|------------|----------|--------|-------|
| 1 | gemini-3-pro-image | - | ⏳ | ❌ N/A | ⏳ | 🔴 HIGH | 1 день | Завершит Gemini 3.x |
| 2 | gemini-2.5-pro-thinking | - | ❌ N/A | ⏳ | ⏳ | 🔴 HIGH | 1 день | Pro thinking |
| 3-16 | Model IDs 314-327 | ❓ | ⏳ | ⏳ | ⏳ | 🔴 HIGH | 2-3 недели | 14 моделей gap |

**Документов нужно**: ~32 (16 моделей × 2 docs avg)

---

### 4.2 MEDIUM Priority (2 модели - 4 дня)

| # | Model Name | UI Display | Base Workflow | Thinking Workflow | COMPARISON | Priority | Effort | Notes |
|---|------------|------------|---------------|-------------------|------------|----------|--------|-------|
| 1 | gemini-2.0-flash-exp | - | ⏳ | ⏳ | ⏳ | 🟡 MEDIUM | 2 дня | Experimental |
| 2 | gemini-2.5-flash-lite-thinking | - | ❌ N/A | ⏳ | ⏳ | 🟡 MEDIUM | 1 день | Lite thinking |

**Документов нужно**: ~5

---

### 4.3 LOW Priority (~10 моделей - переменно)

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

## 🎯 РЕКОМЕНДУЕМЫЕ СЛЕДУЮЩИЕ ШАГИ

### Option A: Завершить Gemini 3.x Series (1 день)

```yaml
модель: gemini-3-pro-image
документы:
  - base_workflow (15+ sections)
  - COMPARISON (compliance analysis)
effort: 1 день
прогресс_после: 73.7% (40/54+)
benefit: "Полностью закрыта Gemini 3.x серия (7/7 моделей = 100%)"
```

### Option B: Добавить Gemini 2.5 Pro Thinking (1 день)

```yaml
модель: gemini-2.5-pro-thinking
документы:
  - thinking_workflow (15+ sections)
  - COMPARISON (compliance analysis)
effort: 1 день
прогресс_после: 73.7% (40/54+)
benefit: "Pro tier получает thinking capability"
```

### Option C: Исследовать Model IDs 314-327 (2-3 недели)

```yaml
модели: 14 моделей (потенциально)
документы: 28+ документов
effort: 2-3 недели
прогресс_после: ~98% (52/54+)
benefit: "Закрывает большой gap, почти полная документация"
complexity: "Требуется reverse engineering названий"
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

### Следующий Модель

**Рекомендация**:
1. **Gemini 3 Pro Image** (1 день) - завершит Gemini 3.x на 100%
2. **Gemini 2.5 Pro Thinking** (1 день) - HIGH priority thinking variant
3. **Model IDs 314-327** (2-3 недели) - большой gap

---

**Обновлено**: 2026-01-11 02:45 UTC
**Источники**: model_mapping.rs + файлы docs + UI screenshot + DOCUMENTATION_STATUS.md
**Статус**: ✅ Полная инвентаризация завершена
