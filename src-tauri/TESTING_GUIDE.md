# Testing Guide - Extended Thinking Models

Comprehensive test suite для валидации работоспособности всех вариантов использования LLM моделей с Extended Thinking через Google Cloud Code API.

## Структура тестов

### `src/proxy/tests/thinking_models.rs`

Comprehensive test suite покрывающий все вариации моделей и thinking конфигураций.

## Категории тестов

### 1. Unit Tests: Model Routing (8 тестов)

Тесты проверяют правильность маршрутизации моделей согласно `THINKING_MODELS.md`.

#### `test_claude_opus_routing`
**Что тестирует:** Claude Opus всегда роутится в thinking версию
**Почему:** Google предоставляет Opus только с Extended Thinking
**Проверяет:**
- `claude-opus-4-5` → `claude-opus-4-5-thinking`
- `claude-opus-4` → `claude-opus-4-5-thinking`
- `claude-opus-4-5-20251101` → `claude-opus-4-5-thinking`

#### `test_claude_sonnet_routing`
**Что тестирует:** Claude Sonnet сохраняет thinking/non-thinking версии
**Проверяет:**
- `claude-sonnet-4-5` → `claude-sonnet-4-5` (без thinking)
- `claude-sonnet-4-5-thinking` → `claude-sonnet-4-5-thinking` (с thinking)
- Legacy: `claude-3-5-sonnet-20241022` → `claude-sonnet-4-5`

#### `test_claude_haiku_routing`
**Что тестирует:** Haiku роутится в Gemini Pro High БЕЗ `-thinking` суффикса
**Почему критично:** Gemini использует параметр `thinkingConfig`, НЕ model name!
**Проверяет:**
- `claude-haiku-4-5` → `gemini-3-pro-high`
- `claude-haiku-4` → `gemini-3-pro-high`
- `claude-3-haiku-20240307` → `gemini-3-pro-high`

#### `test_gemini_routing_no_thinking_suffix`
**Что тестирует:** Gemini модели НЕ используют `-thinking` суффикс
**Критическая проверка:** Это была основная ошибка causing 404 errors!
**Проверяет:**
- `gemini-3-pro` → `gemini-3-pro-high`
- `gemini-3-pro-high` → `gemini-3-pro-high`
- `gemini-3-pro-low` → `gemini-3-pro-low`
- `gemini-3-flash` → `gemini-3-flash`

#### `test_fallback_routing`
**Что тестирует:** Неизвестные модели роутятся в Gemini Pro High
**Проверяет:**
- `unknown-model` → `gemini-3-pro-high`
- `some-random-model` → `gemini-3-pro-high`

#### `test_gemini_passthrough`
**Что тестирует:** Gemini модели с префиксом проходят как есть
**Проверяет:**
- `gemini-2.5-flash-mini-test` → `gemini-2.5-flash-mini-test`

#### `test_custom_routing_exact_match`
**Что тестирует:** Custom mapping с точным совпадением
**Проверяет приоритеты:** Custom > System default

#### `test_wildcard_routing`
**Что тестирует:** Wildcard маппинг с `*`
**Проверяет:**
- `gpt-4*` pattern matches `gpt-4-turbo`, `gpt-4`

---

### 2. Integration Tests: Request Transformation (6 тестов)

Тесты проверяют правильность трансформации Claude requests в Google API формат.

#### `test_claude_opus_with_thinking_request`
**Что тестирует:** Claude Opus + Extended Thinking
**Проверяет:**
1. Model routing: `claude-opus-4-5` → `claude-opus-4-5-thinking`
2. `thinkingConfig` присутствует в request body
3. `includeThoughts: true`
4. `thinkingBudget > 0`

#### `test_claude_sonnet_without_thinking_request`
**Что тестирует:** Claude Sonnet БЕЗ thinking
**Проверяет:**
1. Model остается `claude-sonnet-4-5`
2. `thinkingConfig` отсутствует (null)

#### `test_gemini_with_thinking_request`
**Что тестирует:** Gemini + Extended Thinking через параметр
**Критическая проверка:** БЕЗ `-thinking` в model name!
**Проверяет:**
1. Model: `gemini-3-pro-high` (БЕЗ `-thinking`!)
2. `thinkingConfig` присутствует
3. `includeThoughts: true`

#### `test_gemini_without_thinking_request`
**Что тестирует:** Gemini БЕЗ thinking
**Проверяет:**
1. Model: `gemini-3-flash`
2. `thinkingConfig` отсутствует

#### `test_haiku_to_gemini_routing`
**Что тестирует:** Haiku → Gemini с thinking через параметр
**Проверяет:**
1. Model routing: `claude-haiku-4-5` → `gemini-3-pro-high`
2. `thinkingConfig` присутствует (Gemini style)

---

### 3. Validation Tests: Thinking Budget Limits (3 теста)

Тесты проверяют правильность применения model-specific budget limits.

#### `test_claude_thinking_budget_limits`
**Что тестирует:** Claude models имеют max 32000 tokens budget
**Входные данные:** `budget_tokens: 64000` (превышает лимит)
**Ожидаемый результат:** budget clamped to ≤ 32000

#### `test_gemini_flash_thinking_budget_limits`
**Что тестирует:** Gemini Flash имеет max 24576 tokens budget
**Входные данные:** `budget_tokens: 64000`
**Ожидаемый результат:** budget clamped to ≤ 24576

#### `test_gemini_pro_thinking_budget_limits`
**Что тестирует:** Gemini Pro имеет max 32000 tokens budget
**Входные данные:** `budget_tokens: 64000`
**Ожидаемый результат:** budget clamped to ≤ 32000

---

### 4. Edge Cases (3 теста)

#### `test_thinking_model_with_disabled_thinking`
**Что тестирует:** Model с `-thinking` суффиксом но thinking отключен
**Проверяет:**
- Model сохраняет `-thinking` суффикс
- `thinkingConfig` отсутствует (respect user choice)

#### `test_empty_thinking_config`
**Что тестирует:** `thinking.type = "disabled"`
**Проверяет:** `thinkingConfig` отсутствует

#### `test_max_tokens_with_thinking`
**Что тестирует:** `max_tokens` validation с thinking
**Проверяет:** `maxOutputTokens > thinkingBudget` (fix applied)

---

## Запуск тестов

### Все тесты thinking models
```bash
cargo test --lib proxy::tests::thinking_models
```

### Все тесты библиотеки
```bash
cargo test --lib
```

### Конкретный тест
```bash
cargo test --lib test_gemini_routing_no_thinking_suffix
```

### С подробным выводом
```bash
cargo test --lib -- --nocapture
```

---

## Покрытие

**Total tests:** 19 tests

### Покрытие по моделям:
- ✅ Claude Opus (with thinking)
- ✅ Claude Sonnet (with/without thinking)
- ✅ Claude Haiku → Gemini routing
- ✅ Gemini Pro High/Low (with/without thinking)
- ✅ Gemini Flash (with/without thinking)
- ✅ Fallback routing
- ✅ Custom routing rules
- ✅ Wildcard patterns

### Покрытие по функционалу:
- ✅ Model routing correctness
- ✅ Thinking parameter vs model name suffix
- ✅ Request transformation
- ✅ Budget limits validation
- ✅ Edge cases handling

---

## Критические проверки

### ❌ НИКОГДА не должно происходить:
1. ❌ Gemini model с `-thinking` суффиксом (404 Not Found!)
2. ❌ `thinkingBudget` превышает model limits
3. ❌ `max_tokens < thinkingBudget`

### ✅ ВСЕГДА должно быть:
1. ✅ Claude thinking через model name suffix
2. ✅ Gemini thinking через `thinkingConfig` parameter
3. ✅ Budget clamping based on model type
4. ✅ Правильный routing согласно THINKING_MODELS.md

---

## Интеграция с CI/CD

Рекомендуется добавить в `.github/workflows/test.yml`:

```yaml
- name: Run thinking models tests
  run: cargo test --lib proxy::tests::thinking_models -- --nocapture

- name: Verify no Gemini -thinking suffix
  run: |
    cargo test --lib test_gemini_routing_no_thinking_suffix
    cargo test --lib test_gemini_with_thinking_request
```

---

## Manual Testing

Для полной валидации рекомендуется также manual testing с реальными запросами к Google API:

1. **Claude Opus with thinking** - проверить response содержит thinking blocks
2. **Gemini Pro with thinking** - проверить response содержит thinking blocks
3. **Haiku → Gemini routing** - проверить работает без 404
4. **Budget limits** - проверить не превышаются при разных значениях

См. `THINKING_MODELS.md` для debugging commands.

---

## Troubleshooting

### Тесты падают с "String to replace not found"
**Причина:** Code changed since test was written
**Решение:** Read actual file content and update test expectations

### Тесты проходят, но runtime 404 errors
**Причина:** Tests mock transformations, но upstream может отличаться
**Решение:** Check logs для actual model names sent to Google API

### Budget limits не применяются
**Причина:** Model detection logic issue
**Решение:** Check `build_generation_config` в `request.rs:946-979`

---

## Связанные документы

- `THINKING_MODELS.md` - Документация thinking approaches
- `CLAUDE_FIX_SUMMARY.md` - История исправлений
- `src/proxy/common/model_mapping.rs` - Model routing logic
- `src/proxy/mappers/claude/request.rs` - Request transformation

---

**Дата создания:** 2026-01-09
**Версия:** 1.0
**Автор:** Claude Sonnet 4.5
