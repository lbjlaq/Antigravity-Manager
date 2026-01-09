# Extended Thinking - Model Routing Guide

## Критическая находка (2026-01-09)

**ВАЖНО:** Gemini и Claude модели используют **разные подходы** к Extended Thinking!

---

## 🎯 Ключевое различие

### Claude Models
- ✅ Thinking включается через **суффикс в названии модели**
- Примеры:
  - `claude-opus-4-5-thinking` (С thinking)
  - `claude-opus-4-5` (БЕЗ thinking - НЕ доступен через Google!)
  - `claude-sonnet-4-5-thinking` (С thinking)
  - `claude-sonnet-4-5` (БЕЗ thinking)

**Правило для Claude:**
```
Thinking = часть названия модели в API
```

### Gemini Models
- ✅ Thinking включается через **параметр API `thinkingConfig`**
- ❌ **НЕТ суффикса `-thinking` в названии модели!**
- Примеры:
  - `gemini-3-pro-high` + `thinkingConfig` → thinking включен
  - `gemini-3-pro-high` без `thinkingConfig` → thinking выключен
  - `gemini-3-flash` + `thinkingConfig` → thinking включен

**Правило для Gemini:**
```
Thinking = параметр запроса, НЕ название модели
```

---

## ❌ Распространенная ошибка

### НЕ СУЩЕСТВУЮЩИЕ модели:
```
❌ gemini-3-pro-high-thinking    - возвращает 404 Not Found
❌ gemini-3-pro-low-thinking     - возвращает 404 Not Found
❌ gemini-3-flash-thinking       - возвращает 404 Not Found
```

**Почему 404?**
Google API НЕ распознает модели с суффиксом `-thinking` для Gemini!

### ✅ ПРАВИЛЬНЫЕ модели:
```
✅ gemini-3-pro-high    - существует, thinking через параметр
✅ gemini-3-pro-low     - существует, thinking через параметр
✅ gemini-3-flash       - существует, thinking через параметр
```

---

## 🔧 Реализация в коде

### 1. Model Mapping (`src/proxy/common/model_mapping.rs`)

**ПРАВИЛЬНО:**
```rust
// Gemini - БЕЗ -thinking суффикса!
m.insert("gemini-3-pro", "gemini-3-pro-high");
m.insert("gemini-3-pro-high", "gemini-3-pro-high");
m.insert("gemini-3-pro-low", "gemini-3-pro-low");
m.insert("gemini-3-flash", "gemini-3-flash");

// Fallback
"gemini-3-pro-high"  // БЕЗ -thinking!
```

**НЕПРАВИЛЬНО:**
```rust
// ❌ НЕ ДЕЛАТЬ ТАК:
m.insert("gemini-3-pro", "gemini-3-pro-high-thinking");  // 404 Error!
```

### 2. Thinking Support Detection (`src/proxy/mappers/claude/request.rs`)

**Текущий код (строка 183):**
```rust
let target_model_supports_thinking =
    mapped_model.contains("-thinking")
    || mapped_model.starts_with("claude-");
```

**Проблема:**
Gemini модели (`gemini-3-pro-high`) НЕ проходят эту проверку, поэтому thinking принудительно отключается!

**ИСПРАВЛЕНИЕ (нужно добавить):**
```rust
let target_model_supports_thinking =
    mapped_model.contains("-thinking")
    || mapped_model.starts_with("claude-")
    || mapped_model.starts_with("gemini-");  // ← ДОБАВИТЬ!
```

### 3. Generation Config (`src/proxy/mappers/claude/request.rs:952-979`)

Код уже правильный! Он добавляет `thinkingConfig` в параметры запроса:
```rust
if thinking.type_ == "enabled" && is_thinking_enabled {
    config["thinkingConfig"] = json!({
        "includeThoughts": true,
        "thinkingBudget": budget  // Clamped to model limits
    });
}
```

---

## 📊 Success Rate после исправления

### До исправления:
```
gemini-3-pro-high-thinking: 24 успеха / 282 ошибки = 7.8% ❌
```

### После исправления (ожидается):
```
gemini-3-pro-high: ~90%+ success rate ✅
```

---

## 🎯 Итоговая таблица роутинга

| Входящая модель | Роутится в | Thinking? | Как включается |
|----------------|------------|-----------|----------------|
| **Claude** ||||
| `claude-opus-4-5` | `claude-opus-4-5-thinking` | ✅ Да | Суффикс в названии |
| `claude-sonnet-4-5` | `claude-sonnet-4-5` | ❌ Нет | Суффикс в названии |
| `claude-sonnet-4-5-thinking` | `claude-sonnet-4-5-thinking` | ✅ Да | Суффикс в названии |
| **Gemini** ||||
| `gemini-3-pro` | `gemini-3-pro-high` | ⚙️ Динамически | Параметр API |
| `gemini-3-pro-high` | `gemini-3-pro-high` | ⚙️ Динамически | Параметр API |
| `gemini-3-pro-low` | `gemini-3-pro-low` | ⚙️ Динамически | Параметр API |
| `gemini-3-flash` | `gemini-3-flash` | ⚙️ Динамически | Параметр API |
| **Haiku** ||||
| `claude-haiku-4-5` | `gemini-3-pro-high` | ⚙️ Динамически | Параметр API |
| **Fallback** ||||
| `unknown-model` | `gemini-3-pro-high` | ⚙️ Динамически | Параметр API |

**⚙️ Динамически** = thinking включается/выключается через параметр `thinkingConfig` в зависимости от запроса клиента

---

## 🔄 Automatic Fallback Mechanism (v3.3.20+)

### Проблема: Claude Opus Thinking Timeouts

**Issue #497**: Claude Opus Thinking (`claude-opus-4-5-thinking`) имеет критический уровень зависаний:
- **Success rate**: 6.3% (3 успеха из 48 запросов)
- **Timeout rate**: 93.7% (45 зависших запросов)
- **Причина**: Google API не отвечает на запросы (>30 секунд ожидания)

### Решение: Автоматический Fallback

**Реализовано в версии 3.3.20** (commit `8dd5fc1`):

```rust
// src/proxy/mappers/claude/request.rs:185-200
if mapped_model == "claude-opus-4-5-thinking" {
    let fallback_model = "gemini-3-pro-high";
    tracing::warn!(
        "[Model-Fallback] Claude Opus Thinking unavailable (issue #497).
         Falling back: {} -> {}",
        mapped_model, fallback_model
    );
    mapped_model = fallback_model.to_string();

    // Emit UI notification event
    emit_model_fallback_event(&claude_req.model, &mapped_model)?;
}
```

### Как работает:

1. **Детектирует** запросы к `claude-opus-4-5-thinking`
2. **Переключает** на `gemini-3-pro-high` (100% success rate)
3. **Логирует** warning в консоль с указанием причины
4. **Уведомляет UI** через Tauri event `proxy://model-fallback`

### UI Notifications

**Toast компонент** (`src/App.tsx`):
- Компактное предупреждение (warning type)
- Сообщение: `"{original_model} недоступен, используем {fallback_model}"`
- Автоматическое закрытие через **5 секунд**
- Возможность ручного закрытия

**Event Payload:**
```typescript
{
  original_model: "claude-opus-4-5",
  fallback_model: "gemini-3-pro-high",
  reason: "High timeout rate (93.7%) with Claude Opus Thinking - see issue #497"
}
```

### Преимущества Fallback:

| Метрика | Claude Opus Thinking | Gemini Pro High (fallback) |
|---------|---------------------|----------------------------|
| Success Rate | 6.3% ❌ | 100% ✅ |
| Timeout Rate | 93.7% | 0% |
| Average Response Time | >30s (timeout) | <5s |
| Thinking Support | ✅ Да | ✅ Да (через thinkingConfig) |
| Extended Thinking Budget | 32,000 tokens | 32,000 tokens |

### Лог примеры:

**Успешный fallback:**
```log
[Model-Fallback] Claude Opus Thinking unavailable (issue #497).
  Falling back: claude-opus-4-5-thinking -> gemini-3-pro-high
[Model-Fallback-Event] Emitted UI notification: claude-opus-4-5 -> gemini-3-pro-high
```

**UI Toast:**
```
⚠️ claude-opus-4-5 недоступен, используем gemini-3-pro-high
```

### Отключение Fallback

Если нужно протестировать оригинальный Opus (для debugging):

**Опция 1: Временно закомментировать код**
```rust
// Временно отключить fallback для тестирования
// if mapped_model == "claude-opus-4-5-thinking" { ... }
```

**Опция 2: Использовать прямой запрос к Gemini**
```json
{
  "model": "gemini-3-pro-high",
  "thinking": { "type": "enabled", "budget_tokens": 32000 }
}
```

### Известные ограничения:

1. **Fallback односторонний**: Только Opus → Gemini (не наоборот)
2. **Без ретрай**: Не пытается повторно с Opus после таймаута
3. **Не кэшируется**: Каждый запрос проверяется заново

### Future Improvements:

- [ ] Динамический fallback на основе реального timeout detection
- [ ] Кэширование статуса моделей (available/unavailable)
- [ ] Configurable fallback chain: Opus → Gemini Pro → Sonnet
- [ ] Retry logic с экспоненциальным backoff

---

## 🔍 Debugging Tips

### Проверка успешного запроса с thinking:
```bash
grep "gemini-3-pro-high" logs/app.log | grep -B 5 "thinkingConfig"
```

### Проверка 404 ошибок:
```bash
grep "404 Not Found" logs/app.log | grep -B 20 "gemini.*thinking"
```

### Статистика по моделям:
```bash
grep "Status: 200 OK" logs/app.log -B 25 | grep "model: Some" | sort | uniq -c
```

### Мониторинг Fallback событий:
```bash
# Все fallback события
grep "Model-Fallback" logs/app.log

# Fallback с временными метками
grep "Model-Fallback" logs/app.log | grep -o "^\[.*\].*Model-Fallback.*"

# Подсчет fallback за сессию
grep "Model-Fallback" logs/app.log | wc -l

# UI notification события
grep "Model-Fallback-Event" logs/app.log

# Просмотр последних 10 fallback
grep "Model-Fallback" logs/app.log | tail -10
```

### Проверка успешности fallback:
```bash
# Найти fallback и следующий за ним успешный ответ
grep -A 50 "Model-Fallback.*claude-opus-4-5-thinking" logs/app.log | \
  grep "gemini-3-pro-high" | \
  grep "Stream completed"
```

---

## 📚 References

- Google Cloud Code API: Models don't use `-thinking` suffix
- Claude API через Google: Uses `-thinking` suffix in model name
- Extended Thinking: `thinkingConfig` параметр в `generationConfig`
- Budget limits: Claude (32000), Gemini Flash (24576), Gemini Pro (32000)

---

**Дата находки:** 2026-01-09
**Анализ логов:** 314 успешных / 446 ошибок 404
**Root cause:** Использование несуществующей модели `gemini-3-pro-high-thinking`
