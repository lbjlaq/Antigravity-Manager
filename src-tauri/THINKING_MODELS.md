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
