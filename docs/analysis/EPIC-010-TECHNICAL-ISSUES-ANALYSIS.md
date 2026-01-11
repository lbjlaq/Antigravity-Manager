# Epic-010 (Gemini 3 Flash) - Детальный технический анализ проблем

**Дата анализа**: 2026-01-11
**Статус**: 🚨 CRITICAL - Требуется исправление API
**Compliance**: 68.8% (22/32 features)
**Для обсуждения с**: Команда Reverse Engineering

---

## 🚨 Критическая проблема: API Incompatibility

### Суть проблемы

**Gemini 3 Flash использует НЕПРАВИЛЬНЫЙ API** для thinking mode. Код отправляет параметры в формате Gemini 2.5, но Gemini 3.x требует ДРУГОЙ API.

```yaml
critical_issue:
  severity: "🚨 CRITICAL (P0)"
  impact: "Thinking mode может не работать или работать нестабильно"
  production_risk: "HIGH - непредсказуемое поведение"
  blocks: "Все функции thinking mode для Gemini 3 Flash"
```

### Техническая детализация

**Текущая реализация** (НЕПРАВИЛЬНО для Gemini 3):
```rust
// Файл: src-tauri/src/proxy/mappers/openai/request.rs
// Строки: 263-272

if is_gemini_3_thinking {
    gen_config["thinkingConfig"] = json!({
        "includeThoughts": true,
        "thinkingBudget": 16000  // ❌ WRONG API!
    });
}

// Параметр thinkingBudget — это Gemini 2.5 API
// Gemini 3.x НЕ ПОДДЕРЖИВАЕТ этот параметр
```

**Правильная реализация** (для Gemini 3):
```rust
// ✅ CORRECT for Gemini 3
gen_config["thinkingConfig"] = json!({
    "includeThoughts": true,
    "thinkingLevel": "HIGH"  // enum: MINIMAL, LOW, MEDIUM, HIGH
});

// Параметр thinkingLevel — это Gemini 3.x API
// Вместо числового бюджета используются уровни
```

### API Breaking Change

```yaml
gemini_2_5_api:
  parameter: "thinkingBudget"
  type: "integer"
  range: "1-32000 tokens"
  example: 16000
  description: "Точное количество токенов для thinking"

gemini_3_api:
  parameter: "thinkingLevel"
  type: "enum"
  values:
    - "MINIMAL"  # Минимальное размышление
    - "LOW"      # Низкий уровень
    - "MEDIUM"   # Средний уровень (только Flash!)
    - "HIGH"     # Высокий уровень
  description: "Качественный уровень вместо точного бюджета"

compatibility: "NONE - параметры взаимоисключающие"
```

**ВАЖНО**: Gemini 3 Flash — единственная модель с поддержкой **MEDIUM level**. Pro High и Pro Low поддерживают только LOW и HIGH.

---

## 🔍 Затронутые файлы и локации

### 1. OpenAI Protocol Mapper

**Файл**: `src-tauri/src/proxy/mappers/openai/request.rs`
**Локация**: Строки 247-272

**Проблема 1: Неправильная логика определения Gemini 3 thinking**
```rust
// Строки 247-250
let is_gemini_3_thinking = mapped_model.contains("gemini-3")
    && (mapped_model.ends_with("-high")
        || mapped_model.ends_with("-low")
        || mapped_model.contains("-pro"));
// ❌ Flash не попадает под это условие!
```

**Почему Flash исключен**:
- Flash заканчивается на `-flash`, а не на `-high`, `-low`, или `-pro`
- Следовательно, `is_gemini_3_thinking = false` для Flash
- Результат: Flash НЕ получает auto-injection thinking mode через OpenAI protocol

**Проблема 2: Использование неправильного API**
```rust
// Строки 263-272
if is_gemini_3_thinking {
    gen_config["thinkingConfig"] = json!({
        "includeThoughts": true,
        "thinkingBudget": 16000  // ❌ WRONG API for Gemini 3!
    });
}
```

**Что должно быть**:
```rust
// ✅ Правильная реализация
let is_gemini_3_thinking = mapped_model.starts_with("gemini-3")
    && !mapped_model.contains("image");  // Все Gemini 3 кроме image

if is_gemini_3_thinking {
    gen_config["thinkingConfig"] = json!({
        "includeThoughts": true,
        "thinkingLevel": determine_thinking_level(&mapped_model, budget)
    });
}

fn determine_thinking_level(model: &str, budget: i32) -> &str {
    // Для Flash: поддержка MEDIUM level
    if model.contains("-flash") {
        match budget {
            0..=4000 => "MINIMAL",
            4001..=10000 => "LOW",
            10001..=20000 => "MEDIUM",  // Flash exclusive!
            _ => "HIGH"
        }
    } else {
        // Для Pro High/Low: только LOW и HIGH
        match budget {
            0..=16000 => "LOW",
            _ => "HIGH"
        }
    }
}
```

### 2. Claude Protocol Mapper

**Файл**: `src-tauri/src/proxy/mappers/claude/request.rs`
**Локация**: Строки 1376-1381

**Проблема**: Бюджет ограничивается (clamping), но затем используется напрямую
```rust
// Строки 1376-1379
else if mapped_model.contains("gemini") {
    budget = budget.min(32000);  // ← Ограничивает бюджет
}
// Затем budget используется напрямую с thinkingBudget API ❌
```

**Что происходит**:
1. Claude client отправляет: `thinking.budget_tokens = 25000`
2. Код ограничивает: `budget = 25000.min(32000) = 25000`
3. Код отправляет в Google: `{"thinkingBudget": 25000}`
4. Google API (Gemini 3) ожидает: `{"thinkingLevel": "HIGH"}`
5. Результат: **API mismatch**

**Что должно быть**:
```rust
else if mapped_model.contains("gemini-3") {
    budget = budget.min(32000);
    // Конвертируем бюджет в level
    let level = map_budget_to_level(&mapped_model, budget);
    // Используем thinkingLevel вместо thinkingBudget
}
```

### 3. Model Mapping

**Файл**: `src-tauri/src/proxy/common/model_mapping.rs`
**Локация**: Строки 56-69

**Состояние**: ✅ Routing работает правильно
```rust
// Строки 56-69
"gemini-3-flash" => "gemini-3-flash"  // Прямое соответствие
```

**Проблема**: Нет проблем с routing, но последующая обработка thinking неверна.

### 4. Tests

**Файл**: `src-tauri/src/proxy/tests/thinking_models.rs`
**Проблема**: **КРИТИЧЕСКОЕ ОТСУТСТВИЕ ТЕСТОВ**

**Существующие тесты для Flash**:
- ✅ `test_gemini_3_flash_basic_routing` (строки 105-110)
- ✅ `test_gemini_3_flash_without_thinking` (строки 280-302)

**Отсутствующие тесты**:
- ❌ `test_gemini_3_flash_thinking_request` - Активация thinking mode
- ❌ `test_gemini_3_flash_budget_limits` - Проверка лимитов бюджета
- ❌ `test_gemini_3_flash_level_mapping` - Конвертация budget → level
- ❌ `test_gemini_3_flash_medium_level` - Уникальный MEDIUM level для Flash
- ❌ `test_gemini_3_api_format_validation` - Валидация формата API

**Последствия отсутствия тестов**:
```yaml
impact:
  regression_risk: "HIGH - изменения могут сломать thinking без обнаружения"
  validation: "NONE - нет подтверждения, что thinking работает"
  production_confidence: "LOW - недостаточная уверенность"
```

---

## 📊 Детальный Gap Analysis

### Gap 1: API Incompatibility 🚨

```yaml
gap_id: "CRITICAL-001"
severity: "🚨 CRITICAL"
priority: "P0"

problem:
  current: "Использует thinkingBudget (Gemini 2.5 API)"
  required: "Должен использовать thinkingLevel (Gemini 3 API)"
  compatibility: "ZERO - параметры взаимоисключающие"

affected_models:
  - "gemini-3-flash"
  - "gemini-3-pro-high"
  - "gemini-3-pro-low"

affected_protocols:
  - "OpenAI (/v1/chat/completions)"
  - "Claude (/v1/messages)"
  - "Gemini Native (direct API)"

user_impact:
  openai_clients: "Thinking не активируется автоматически для Flash"
  claude_clients: "Thinking может работать нестабильно"
  gemini_clients: "Thinking НЕ РАБОТАЕТ (API rejection)"
  production_risk: "HIGH"

estimated_fix_effort: "2-3 дня"
```

### Gap 2: Flash Auto-Injection Exclusion ⚠️

```yaml
gap_id: "IMPL-002"
severity: "⚠️ MEDIUM"
priority: "P1"

problem:
  description: "Flash исключен из OpenAI auto-injection"
  location: "openai/request.rs:247-250"
  current_pattern: "ends_with('-high') || ends_with('-low') || contains('-pro')"
  flash_match: false

root_cause:
  likely_intentional: true
  reason: "Текущий код использует неправильный API, поэтому Flash намеренно исключен"
  should_fix_after: "Gap 1 (API fix)"

user_impact:
  openai_sdk_users: "Не получают thinking автоматически"
  workaround: "Нужно явно указывать thinking в запросе"
  production_impact: "MEDIUM"

recommended_fix:
  new_pattern: "model.starts_with('gemini-3') && !model.contains('image')"
  includes: ["gemini-3-flash", "gemini-3-pro-high", "gemini-3-pro-low"]
  excludes: ["gemini-3-pro-image"]

estimated_fix_effort: "< 1 дня (после Gap 1)"
```

### Gap 3: Missing Test Coverage ⚠️

```yaml
gap_id: "TEST-001"
severity: "⚠️ MEDIUM"
priority: "P1"

problem:
  description: "Нет тестов для thinking mode Flash"
  current_coverage: "2 теста (basic routing, without thinking)"
  missing_coverage: "5 критических тестов"

missing_tests:
  test_1:
    name: "test_gemini_3_flash_thinking_request"
    validates: "Thinking config injection и API format"
    criticality: "HIGH"

  test_2:
    name: "test_gemini_3_flash_budget_limits"
    validates: "Budget clamping (max 32000)"
    criticality: "MEDIUM"

  test_3:
    name: "test_gemini_3_flash_level_mapping"
    validates: "Budget → Level conversion logic"
    criticality: "HIGH"

  test_4:
    name: "test_gemini_3_flash_medium_level"
    validates: "MEDIUM level (Flash exclusive feature)"
    criticality: "HIGH"

  test_5:
    name: "test_gemini_3_api_format_validation"
    validates: "Правильность формата API (thinkingLevel)"
    criticality: "CRITICAL"

impact:
  reliability: "Thinking functionality не валидирована"
  regression_risk: "HIGH - изменения могут сломать без обнаружения"
  production_confidence: "LOW"

estimated_fix_effort: "2-3 дня"
```

### Gap 4: Budget-to-Level Mapping ⚠️

```yaml
gap_id: "IMPL-003"
severity: "⚠️ MEDIUM"
priority: "P1"

problem:
  description: "Нет логики конвертации token budget → thinking level"
  depends_on: "Gap 1 (API fix)"

current_behavior:
  claude_protocol: "Клиент отправляет budget_tokens (например, 25000)"
  processing: "Код ограничивает до 32000"
  output: "Отправляет thinkingBudget: 25000 (неправильный API)"

required_behavior:
  input: "budget_tokens: 25000"
  mapping_logic:
    flash:
      "0-4000": "MINIMAL"
      "4001-10000": "LOW"
      "10001-20000": "MEDIUM"  # Flash exclusive!
      "20001+": "HIGH"
    pro_high_low:
      "0-16000": "LOW"
      "16001+": "HIGH"
  output: "thinkingLevel: 'HIGH'"

flash_advantage:
  feature: "MEDIUM level"
  description: "Баланс между качеством и стоимостью"
  availability: "Только Flash, Pro не поддерживает"
  use_case: "Умеренно сложные задачи с оптимизацией стоимости"

estimated_fix_effort: "1-2 дня"
```

### Gap 5: Level-Based Optimization (P2) ℹ️

```yaml
gap_id: "OPT-001"
severity: "ℹ️ LOW"
priority: "P2"

problem:
  description: "Нет адаптивной оптимизации уровней thinking"
  depends_on: "Gaps 1, 3, 4 (API fix, tests, mapping)"

documented_features_not_implemented:
  - "Adaptive level selection based on complexity"
  - "Quality-based retry with level upgrade"
  - "MEDIUM level as default for complex tasks"
  - "Dynamic level adjustment based on feedback"
  - "Cost optimization with MEDIUM level"

implementation_plan:
  component_1:
    name: "Complexity Classifier"
    input: "Request content, message history, tools"
    output: "simple | moderate | complex | critical"
    effort: "3-4 дня"

  component_2:
    name: "Level Selector"
    logic:
      simple: "MINIMAL or LOW"
      moderate: "LOW or MEDIUM (Flash advantage)"
      complex: "MEDIUM (Flash) or HIGH (Pro)"
      critical: "HIGH"
    effort: "2-3 дня"

  component_3:
    name: "Quality Monitor"
    tracks: "Output quality by level"
    adjusts: "Level selection based on feedback"
    effort: "3-4 дня"

  component_4:
    name: "Cost Tracker"
    monitors: "Cost per level, usage distribution"
    optimizes: "Level selection for cost efficiency"
    effort: "2 дня"

expected_benefits:
  cost_savings: "20-30% on complex tasks (use MEDIUM instead of HIGH)"
  quality_improvement: "10-15% on critical tasks (upgrade when needed)"
  user_experience: "Better balance quality/cost"

estimated_total_effort: "1-2 недели (после основного fix)"
```

---

## 🔧 Требуемые изменения

### Phase 1: Critical API Fix (P0) 🚨

**Timeline**: 1-2 недели
**Блокирует**: Все остальные фазы

#### Task 1.1: Gemini 3 Detection Logic

**Файл**: `mappers/openai/request.rs`

**Текущий код** (строки 247-250):
```rust
let is_gemini_3_thinking = mapped_model.contains("gemini-3")
    && (mapped_model.ends_with("-high")
        || mapped_model.ends_with("-low")
        || mapped_model.contains("-pro"));
```

**Исправление**:
```rust
let is_gemini_3_thinking = mapped_model.starts_with("gemini-3")
    && !mapped_model.contains("image");
// Включает: flash, pro-high, pro-low
// Исключает: pro-image (не поддерживает thinking)
```

**Effort**: 30 минут

#### Task 1.2: thinkingLevel API Implementation

**Файл**: `mappers/openai/request.rs`

**Текущий код** (строки 263-272):
```rust
if is_gemini_3_thinking {
    gen_config["thinkingConfig"] = json!({
        "includeThoughts": true,
        "thinkingBudget": 16000
    });
}
```

**Исправление**:
```rust
if is_gemini_3_thinking {
    // Получаем уровень на основе модели
    let thinking_level = determine_thinking_level(&mapped_model, None);

    gen_config["thinkingConfig"] = json!({
        "includeThoughts": true,
        "thinkingLevel": thinking_level
    });
}

fn determine_thinking_level(model: &str, budget: Option<i32>) -> &'static str {
    // Для OpenAI protocol: используем HIGH по умолчанию
    if budget.is_none() {
        return if model.contains("-flash") {
            "MEDIUM"  // Flash default: MEDIUM для баланса
        } else {
            "HIGH"    // Pro default: HIGH для качества
        };
    }

    let budget = budget.unwrap();

    if model.contains("-flash") {
        // Flash: 4 уровня (MINIMAL, LOW, MEDIUM, HIGH)
        match budget {
            0..=4000 => "MINIMAL",
            4001..=10000 => "LOW",
            10001..=20000 => "MEDIUM",
            _ => "HIGH"
        }
    } else {
        // Pro (High/Low): 2 уровня (LOW, HIGH)
        match budget {
            0..=16000 => "LOW",
            _ => "HIGH"
        }
    }
}
```

**Effort**: 4-5 часов

#### Task 1.3: Claude Protocol Budget Mapping

**Файл**: `mappers/claude/request.rs`

**Текущий код** (строки 1376-1381):
```rust
else if mapped_model.contains("gemini") {
    budget = budget.min(32000);
}
// Затем budget используется с thinkingBudget API
```

**Исправление**:
```rust
else if mapped_model.contains("gemini-3") {
    budget = budget.min(32000);

    // Конвертируем бюджет в level для Gemini 3
    let thinking_level = determine_thinking_level(&mapped_model, Some(budget));

    gen_config["thinkingConfig"] = json!({
        "includeThoughts": true,
        "thinkingLevel": thinking_level
    });
} else if mapped_model.contains("gemini-2.5") {
    budget = budget.min(32000);

    // Gemini 2.5 использует thinkingBudget
    gen_config["thinkingConfig"] = json!({
        "includeThoughts": true,
        "thinkingBudget": budget
    });
}
```

**Effort**: 3-4 часа

#### Task 1.4: API Format Validation

**Новый файл**: `mappers/common/gemini_api_validator.rs`

**Реализация**:
```rust
pub fn validate_gemini_3_thinking_config(
    model: &str,
    config: &serde_json::Value
) -> Result<(), String> {
    if !model.starts_with("gemini-3") {
        return Ok(());  // Validation только для Gemini 3
    }

    let thinking_config = config.get("thinkingConfig");
    if thinking_config.is_none() {
        return Ok(());  // Thinking не используется
    }

    let thinking_config = thinking_config.unwrap();

    // Проверка 1: thinkingLevel должен присутствовать
    if thinking_config.get("thinkingLevel").is_none() {
        return Err("Gemini 3 requires 'thinkingLevel', not 'thinkingBudget'".to_string());
    }

    // Проверка 2: thinkingBudget НЕ должен присутствовать
    if thinking_config.get("thinkingBudget").is_some() {
        return Err("Gemini 3 does not support 'thinkingBudget', use 'thinkingLevel'".to_string());
    }

    // Проверка 3: Валидация значения level
    let level = thinking_config["thinkingLevel"].as_str()
        .ok_or("thinkingLevel must be a string")?;

    let valid_levels = if model.contains("-flash") {
        vec!["MINIMAL", "LOW", "MEDIUM", "HIGH"]
    } else {
        vec!["LOW", "HIGH"]
    };

    if !valid_levels.contains(&level) {
        return Err(format!(
            "Invalid thinkingLevel '{}' for {}. Valid: {:?}",
            level, model, valid_levels
        ));
    }

    Ok(())
}
```

**Effort**: 2-3 часа

**Итого Phase 1**: 2-3 рабочих дня

### Phase 2: Feature Parity (P1) ⚠️

**Timeline**: 1 неделя после Phase 1
**Зависит от**: Phase 1 завершен

#### Task 2.1: Flash Auto-Injection

**Файл**: `mappers/openai/request.rs`

Уже исправлено в Task 1.1 (изменение detection logic).

**Effort**: 0 (уже включено)

#### Task 2.2: MEDIUM Level Support

**Validation**: Убедиться, что MEDIUM level правильно обрабатывается только для Flash.

**Тестовые сценарии**:
```yaml
test_scenarios:
  flash_medium:
    model: "gemini-3-flash"
    budget: 15000
    expected_level: "MEDIUM"
    should_work: true

  pro_high_medium:
    model: "gemini-3-pro-high"
    budget: 15000
    expected_level: "HIGH"  # Pro не поддерживает MEDIUM
    should_work: true

  pro_low_medium:
    model: "gemini-3-pro-low"
    budget: 15000
    expected_level: "HIGH"  # Pro не поддерживает MEDIUM
    should_work: true
```

**Effort**: 1 день

#### Task 2.3: Comprehensive Tests

**Файл**: `tests/thinking_models.rs`

**Новые тесты** (5 штук):

##### Test 1: `test_gemini_3_flash_thinking_request`
```rust
#[tokio::test]
async fn test_gemini_3_flash_thinking_request() {
    let request = create_openai_request(
        "gemini-3-flash",
        "Test thinking mode",
        None  // Auto-injection
    );

    let mapped = map_to_gemini_format(&request).await.unwrap();

    // Проверка: thinkingConfig присутствует
    assert!(mapped["generationConfig"]["thinkingConfig"].is_object());

    let thinking_config = &mapped["generationConfig"]["thinkingConfig"];

    // Проверка: использует thinkingLevel, а не thinkingBudget
    assert!(thinking_config.get("thinkingLevel").is_some());
    assert!(thinking_config.get("thinkingBudget").is_none());

    // Проверка: уровень MEDIUM (Flash default)
    assert_eq!(thinking_config["thinkingLevel"], "MEDIUM");
}
```

##### Test 2: `test_gemini_3_flash_budget_limits`
```rust
#[tokio::test]
async fn test_gemini_3_flash_budget_limits() {
    // Test: budget > 32000 ограничивается
    let request = create_claude_request(
        "gemini-3-flash",
        50000  // Превышает лимит
    );

    let mapped = map_to_gemini_format(&request).await.unwrap();
    let level = mapped["generationConfig"]["thinkingConfig"]["thinkingLevel"]
        .as_str().unwrap();

    // Проверка: 50000 → HIGH (после clamping до 32000)
    assert_eq!(level, "HIGH");
}
```

##### Test 3: `test_gemini_3_flash_level_mapping`
```rust
#[tokio::test]
async fn test_gemini_3_flash_level_mapping() {
    let test_cases = vec![
        (2000, "MINIMAL"),
        (5000, "LOW"),
        (15000, "MEDIUM"),
        (25000, "HIGH"),
    ];

    for (budget, expected_level) in test_cases {
        let request = create_claude_request("gemini-3-flash", budget);
        let mapped = map_to_gemini_format(&request).await.unwrap();
        let level = mapped["generationConfig"]["thinkingConfig"]["thinkingLevel"]
            .as_str().unwrap();

        assert_eq!(
            level, expected_level,
            "Budget {} should map to {}",
            budget, expected_level
        );
    }
}
```

##### Test 4: `test_gemini_3_flash_medium_level`
```rust
#[tokio::test]
async fn test_gemini_3_flash_medium_level() {
    // Test: MEDIUM доступен для Flash
    let flash_request = create_claude_request("gemini-3-flash", 15000);
    let flash_mapped = map_to_gemini_format(&flash_request).await.unwrap();
    assert_eq!(
        flash_mapped["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "MEDIUM"
    );

    // Test: MEDIUM НЕ доступен для Pro (должен быть HIGH)
    let pro_request = create_claude_request("gemini-3-pro-high", 15000);
    let pro_mapped = map_to_gemini_format(&pro_request).await.unwrap();
    assert_eq!(
        pro_mapped["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "HIGH"  // Pro не поддерживает MEDIUM
    );
}
```

##### Test 5: `test_gemini_3_api_format_validation`
```rust
#[tokio::test]
async fn test_gemini_3_api_format_validation() {
    let request = create_openai_request("gemini-3-flash", "Test", None);
    let mapped = map_to_gemini_format(&request).await.unwrap();

    // Критическая проверка: правильный формат API
    let validation_result = validate_gemini_3_thinking_config(
        "gemini-3-flash",
        &mapped["generationConfig"]
    );

    assert!(validation_result.is_ok(), "API format validation failed");

    // Дополнительная проверка: точно не использует thinkingBudget
    let thinking_config = &mapped["generationConfig"]["thinkingConfig"];
    assert!(
        thinking_config.get("thinkingBudget").is_none(),
        "Gemini 3 should not use thinkingBudget"
    );
}
```

**Effort**: 2-3 дня

#### Task 2.4: Documentation Update

**Файлы для обновления**:
1. `gemini-3-flash-workflow.md` - API migration notes
2. `gemini-3-flash-thinking-workflow.md` - Level-based examples
3. `gemini-3-flash-COMPARISON.md` - Update compliance after fix
4. **Новый**: `GEMINI-3-API-MIGRATION-GUIDE.md`

**Содержание Migration Guide**:
```markdown
# Gemini 3 API Migration Guide

## Изменения API

### thinkingBudget → thinkingLevel

Gemini 3.x использует НОВЫЙ API для thinking mode.

#### До (Gemini 2.5):
{
  "thinkingConfig": {
    "includeThoughts": true,
    "thinkingBudget": 16000
  }
}

#### После (Gemini 3):
{
  "thinkingConfig": {
    "includeThoughts": true,
    "thinkingLevel": "HIGH"
  }
}

## Mapping Logic

### Flash (4 уровня)
- 0-4000 tokens → MINIMAL
- 4001-10000 tokens → LOW
- 10001-20000 tokens → MEDIUM (Flash exclusive!)
- 20001+ tokens → HIGH

### Pro High/Low (2 уровня)
- 0-16000 tokens → LOW
- 16001+ tokens → HIGH

## Breaking Changes
- thinkingBudget больше не поддерживается
- Точный контроль токенов заменен на уровни качества
- MEDIUM level доступен только для Flash

## Client Impact
- OpenAI SDK: Прозрачно (auto-injection работает)
- Claude SDK: Прозрачно (budget конвертируется автоматически)
- Gemini Native: Требуется обновление клиентского кода
```

**Effort**: 1-2 дня

**Итого Phase 2**: 1 неделя

### Phase 3: Optimization (P2) ℹ️

**Timeline**: 2-3 недели после Phase 2
**Зависит от**: Phase 1 и 2 завершены
**Приоритет**: LOW (можно отложить)

#### Task 3.1: Adaptive Level Selection

**Компоненты**:
1. Request Complexity Analyzer
2. Intelligent Level Selector
3. Quality Feedback Loop

**Effort**: 1-2 недели

#### Task 3.2: Cost Optimization

**Features**:
- Level distribution monitoring
- Cost per level tracking
- Automatic MEDIUM preference for Flash
- Quality threshold validation

**Effort**: 1 неделя

**Итого Phase 3**: 2-3 недели (опционально)

---

## 🎯 Epic-010 Story Breakdown

### Phase 1 Stories (P0)

#### Story-010-01: Gemini 3 API Migration 🚨
```yaml
priority: P0 (CRITICAL)
effort: 5 story points (2-3 дня)
team: Backend
depends_on: []

description: >
  Migrate from Gemini 2.5 thinkingBudget API to Gemini 3 thinkingLevel API
  for all Gemini 3 models (Flash, Pro High, Pro Low).

tasks:
  - Update Gemini 3 detection logic (Task 1.1)
  - Implement thinkingLevel API (Task 1.2)
  - Add budget-to-level mapping (Task 1.3)
  - Implement API validation (Task 1.4)

acceptance_criteria:
  - "Gemini 3 Flash uses thinkingLevel API"
  - "Gemini 3 Pro High uses thinkingLevel API"
  - "Gemini 3 Pro Low uses thinkingLevel API"
  - "Gemini 2.5 continues using thinkingBudget"
  - "API validation catches format errors"
  - "No breaking changes for existing clients"

files_to_modify:
  - "mappers/openai/request.rs"
  - "mappers/claude/request.rs"
  - "mappers/common/gemini_api_validator.rs" (new)

validation:
  - Manual API testing with Google
  - Format validation tests pass
  - No regression in Gemini 2.5
```

#### Story-010-02: Budget-to-Level Mapping Logic 🚨
```yaml
priority: P0 (CRITICAL)
effort: 3 story points (1-2 дня)
team: Backend
depends_on: ["Story-010-01"]

description: >
  Implement intelligent mapping from token budgets to thinking levels,
  supporting Flash's unique MEDIUM level.

tasks:
  - Implement determine_thinking_level function
  - Add Flash-specific MEDIUM level support
  - Add Pro-specific LOW/HIGH mapping
  - Validate level selection logic

acceptance_criteria:
  - "Flash: 4 levels (MINIMAL, LOW, MEDIUM, HIGH)"
  - "Pro: 2 levels (LOW, HIGH)"
  - "Budget ranges map correctly to levels"
  - "MEDIUM level only available for Flash"
  - "Default level: MEDIUM for Flash, HIGH for Pro"

validation:
  - Unit tests for all budget ranges
  - Integration tests for protocol conversion
```

### Phase 2 Stories (P1)

#### Story-010-03: Comprehensive Test Coverage ⚠️
```yaml
priority: P1 (HIGH)
effort: 5 story points (2-3 дня)
team: QA + Backend
depends_on: ["Story-010-01", "Story-010-02"]

description: >
  Add comprehensive test coverage for Gemini 3 Flash thinking mode,
  including all levels and API format validation.

tasks:
  - Add test_gemini_3_flash_thinking_request
  - Add test_gemini_3_flash_budget_limits
  - Add test_gemini_3_flash_level_mapping
  - Add test_gemini_3_flash_medium_level
  - Add test_gemini_3_api_format_validation

acceptance_criteria:
  - "All 5 new tests pass"
  - "Test coverage ≥90% for thinking logic"
  - "API format validation working"
  - "No regression in existing tests"
  - "CI/CD integration complete"

deliverables:
  - "5 new test cases"
  - "Test documentation"
  - "Coverage report"
```

#### Story-010-04: Flash OpenAI Auto-Injection ⚠️
```yaml
priority: P1 (HIGH)
effort: 2 story points (< 1 день)
team: Backend
depends_on: ["Story-010-01"]

description: >
  Enable automatic thinking injection for Flash in OpenAI protocol,
  previously excluded due to API incompatibility.

tasks:
  - Verify detection logic includes Flash
  - Test auto-injection for Flash
  - Validate MEDIUM level as default

acceptance_criteria:
  - "Flash gets auto-injection in OpenAI protocol"
  - "Default level: MEDIUM for Flash"
  - "No explicit thinking config required"
  - "Compatible with all OpenAI SDK clients"

validation:
  - Integration test with OpenAI SDK
  - Default level validation
```

#### Story-010-05: Documentation & Migration Guide ⚠️
```yaml
priority: P1 (HIGH)
effort: 3 story points (1-2 дня)
team: Tech Writer + Backend
depends_on: ["Story-010-01", "Story-010-02", "Story-010-03"]

description: >
  Update all documentation and create comprehensive migration guide
  for Gemini 3 API changes.

tasks:
  - Update workflow documentation
  - Update COMPARISON file
  - Create GEMINI-3-API-MIGRATION-GUIDE.md
  - Add code examples
  - Update integration guides

acceptance_criteria:
  - "All workflow docs updated"
  - "Migration guide complete"
  - "Code examples for all protocols"
  - "Breaking changes documented"
  - "Client impact assessment included"

deliverables:
  - "Updated workflow docs"
  - "Migration guide"
  - "Integration examples"
  - "API comparison matrix"
```

### Phase 3 Stories (P2) - OPTIONAL

#### Story-010-06: Adaptive Level Selection (P2) ℹ️
```yaml
priority: P2 (MEDIUM)
effort: 8 story points (1-2 недели)
team: Backend + ML
depends_on: ["Story-010-01", "Story-010-02", "Story-010-03"]

description: >
  Implement intelligent level selection based on request complexity,
  optimizing for cost and quality.

components:
  - Complexity analyzer
  - Level selector
  - Quality monitor

acceptance_criteria:
  - "Complexity classification working"
  - "Intelligent level selection"
  - "Cost savings ≥20% on complex tasks"
  - "Quality maintained or improved"

effort: "1-2 недели"
can_defer: true
```

#### Story-010-07: Cost & Quality Monitoring (P2) ℹ️
```yaml
priority: P2 (MEDIUM)
effort: 5 story points (1 неделя)
team: Backend + DevOps
depends_on: ["Story-010-06"]

description: >
  Add comprehensive monitoring for level distribution, cost per level,
  and quality metrics.

deliverables:
  - Level distribution dashboard
  - Cost per level tracking
  - Quality metrics by level
  - Optimization recommendations

effort: "1 неделя"
can_defer: true
```

---

## 📈 Impact Analysis

### Compliance Impact

```yaml
current_state:
  compliance: "68.8% (22/32 features)"
  p0_compliance: "37.5% (3/8 P0 features)"
  critical_gaps: 3

after_phase_1:
  compliance: "~75% (24/32 features)"
  p0_compliance: "100% (8/8 P0 features)" ✅
  critical_gaps: 0 ✅

after_phase_2:
  compliance: "~85% (27/32 features)"
  p1_compliance: "100% (12/12 P1 features)" ✅
  production_ready: true ✅

after_phase_3:
  compliance: "~95% (30/32 features)"
  full_optimization: true
  advanced_features: true
```

### User Impact

```yaml
openai_clients:
  before: "Thinking не работает для Flash (auto-injection excluded)"
  after_p1: "Thinking работает автоматически" ✅
  improvement: "MAJOR"

claude_clients:
  before: "Thinking нестабильно (wrong API)"
  after_p1: "Thinking работает стабильно" ✅
  improvement: "MAJOR"

gemini_native_clients:
  before: "Thinking НЕ РАБОТАЕТ (API rejection)"
  after_p1: "Thinking работает корректно" ✅
  improvement: "CRITICAL"
```

### Production Risk

```yaml
current_production_risk:
  level: "🚨 HIGH"
  reasons:
    - "Wrong API may cause errors"
    - "Unpredictable behavior"
    - "No test coverage"
  recommendation: "DO NOT USE in production"

after_phase_1:
  level: "🟡 MEDIUM"
  reasons:
    - "API correct but limited testing"
    - "Basic functionality validated"
  recommendation: "USE with caution"

after_phase_2:
  level: "🟢 LOW"
  reasons:
    - "Full test coverage"
    - "API validated"
    - "Documentation complete"
  recommendation: "PRODUCTION READY" ✅
```

---

## 🚦 Рекомендации

### Immediate Action (P0)

```yaml
recommendation: "DEFER Epic-010 до завершения API migration"

rationale:
  - "Критическая проблема с API блокирует thinking mode"
  - "HIGH production risk"
  - "Требует 2-3 недели на исправление"

alternative_path:
  1. "Выполнить Phase 1 (API fix) в отдельном sprint"
  2. "Затем Epic-010 с Phase 2+3"
  3. "Или включить в Strategic Review для Q2"

estimated_total_effort:
  phase_1_critical: "2-3 дня"
  phase_2_parity: "1 неделя"
  phase_3_optional: "2-3 недели"
  total_minimum: "~2 недели (P0+P1)"
  total_complete: "4-6 недель (P0+P1+P2)"
```

### Strategic Considerations

```yaml
gemini_3_series_status:
  after_epic_007: "gemini-3-pro-image: 100%" ✅
  after_epic_009: "gemini-3-pro-low: 100%" ✅
  after_epic_010_p2: "gemini-3-flash: ~85%"

series_completion:
  without_epic_010: "67% (2/3 models 100%)"
  with_epic_010_p1: "67% (but all critical gaps closed)"
  with_epic_010_p2: "100% (full series complete)"

recommendation:
  q1: "Focus on Epic-007/008/009 + Strategic Review"
  q2: "Epic-010 (with all 3 phases) for complete Gemini 3 series"
```

---

## 📞 Вопросы для команды Reverse Engineering

### Critical Questions

1. **API Compatibility**:
   - Подтверждает ли эталонное приложение использование `thinkingLevel` для Gemini 3?
   - Есть ли примеры запросов с правильным форматом API?
   - Как эталонное приложение обрабатывает MEDIUM level для Flash?

2. **Level Mapping Logic**:
   - Какая логика конвертации budget → level используется в эталоне?
   - Подтверждены ли границы диапазонов (0-4000, 4001-10000, и т.д.)?
   - Есть ли особые случаи или edge cases в mapping logic?

3. **MEDIUM Level Support**:
   - Точно ли MEDIUM level эксклюзивен для Flash?
   - Как Pro High/Low обрабатывают запросы с budget 10001-20000?
   - Есть ли различия в стоимости между MEDIUM и HIGH для Flash?

4. **Auto-Injection Pattern**:
   - Какой pattern detection используется для auto-injection?
   - Почему Flash был исключен (намеренно или ошибка)?
   - Должен ли Flash получать auto-injection после API fix?

5. **Error Handling**:
   - Как Google API реагирует на неправильный формат (thinkingBudget для Gemini 3)?
   - Есть ли graceful degradation или hard failure?
   - Какие error codes возвращаются при API mismatch?

### Validation Requests

```yaml
request_1:
  title: "Правильный формат API для Gemini 3 Flash thinking"
  need: "Пример реального запроса из эталонного приложения"
  format: "JSON с thinkingConfig"

request_2:
  title: "Budget-to-Level mapping logic"
  need: "Код или спецификация mapping ranges"
  details: "Особенно для MEDIUM level"

request_3:
  title: "MEDIUM level стоимость и производительность"
  need: "Данные о стоимости и качестве MEDIUM vs LOW/HIGH"
  use_case: "Обоснование для рекомендации MEDIUM как default"

request_4:
  title: "Edge cases и special handling"
  need: "Особые случаи, которые не описаны в документации"
  examples: "Budget = 0, budget > 32000, invalid levels, и т.д."
```

---

## 📊 Summary

```yaml
epic_010_status:
  current_compliance: "68.8%"
  critical_issue: "🚨 API Incompatibility (uses wrong API for Gemini 3)"
  production_risk: "🚨 HIGH"
  recommendation: "❌ DEFER to Q2"

required_fixes:
  phase_1_critical:
    effort: "2-3 дня"
    priority: "P0"
    blocks: "All thinking functionality"

  phase_2_parity:
    effort: "1 неделя"
    priority: "P1"
    delivers: "Production readiness"

  phase_3_optimization:
    effort: "2-3 недели"
    priority: "P2"
    delivers: "Full feature set"

minimum_viable_epic:
  timeline: "~2 недели (Phase 1 + Phase 2)"
  compliance: "~85%"
  production_ready: true
  stories: 5

questions_for_re_team: 5
validation_requests: 4
```

---

**Документ подготовлен**: 2026-01-11
**Для обсуждения с**: Команда Reverse Engineering
**Статус**: ✅ ГОТОВ к обсуждению
**Следующий шаг**: Валидация технических деталей с RE командой
