# Claude Sonnet/Opus 429 Error Fix - Summary

## 🔍 Проблема

**Симптомы:**
- Antigravity Manager получает 429 ошибку при запросах к Claude Sonnet/Opus моделям
- Оригинальное приложение Google Antigravity работает без проблем с теми же аккаунтами
- Квоты не исчерпаны - проблема в детектировании запросов

**Причина:**
Google различает запросы по User-Agent и блокирует устаревшие версии.

## 🎯 Решение

### Изменения

**Файл:** `src-tauri/src/proxy/upstream/client.rs`

**До:**
```rust
.user_agent("antigravity/1.11.9 windows/amd64")
```

**После:**
```rust
// Updated to 1.13.3 to match current Google Antigravity version
let user_agent = std::env::var("CLAUDE_USER_AGENT")
    .unwrap_or_else(|_| "antigravity/1.13.3 darwin/arm64".to_string());
.user_agent(user_agent)
```

### Ключевые изменения:

1. **Обновлена версия:** `1.11.9` → `1.13.3`
2. **Обновлена платформа:** `windows/amd64` → `darwin/arm64` (для macOS)
3. **Добавлена гибкость:** Можно переопределить через environment variable `CLAUDE_USER_AGENT`
4. **Добавлено логирование:** Вывод используемого User-Agent в логи

### Референсная информация

Версия оригинального Google Antigravity:
```
Antigravity Version: 1.13.3
VSCode OSS Version: 1.104.0
Commit: 94f91bc110994badc7c086033db813077a5226af
Date: 2025-12-19T21:03:14.401Z
Electron: 37.3.1
Chromium: 138.0.7204.235
Node.js: 22.18.0
OS: Darwin arm64 25.1.0
Language Server CL: 846830895
```

## 🧪 Тестирование

### Шаг 1: Rebuild приложения

```bash
# В корневой директории проекта
cargo build --release

# Или для development
cargo build
```

### Шаг 2: Запуск приложения

```bash
# Запустить Tauri приложение
npm run tauri dev
```

### Шаг 3: Тестирование Claude моделей

1. Откройте Antigravity Manager
2. Перейдите на вкладку "API Proxy"
3. Убедитесь что прокси сервер запущен
4. Сделайте тестовый запрос к Claude Sonnet или Opus

**Пример curl запроса:**
```bash
curl -X POST http://127.0.0.1:8045/v1/messages \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer sk-antigravity" \
  -d '{
    "model": "claude-sonnet-4-5",
    "messages": [
      {
        "role": "user",
        "content": "Say hello"
      }
    ],
    "max_tokens": 100
  }'
```

**Ожидаемый результат:**
- HTTP 200 OK
- Ответ от Claude модели
- В логах: `🔧 UpstreamClient User-Agent: antigravity/1.13.3 darwin/arm64`

### Шаг 4: Проверка логов

```bash
# Логи находятся в:
# macOS: ~/Library/Application Support/com.lbjlaq.antigravity-tools/logs/

# Проверить последние логи:
tail -f ~/Library/Application\ Support/com.lbjlaq.antigravity-tools/logs/antigravity_*.log

# Искать строки с User-Agent:
grep "User-Agent" ~/Library/Application\ Support/com.lbjlaq.antigravity-tools/logs/antigravity_*.log
```

## 🔧 Advanced Testing (опционально)

### Тестирование разных User-Agent вариантов

```bash
# Тест 1: Darwin (macOS)
export CLAUDE_USER_AGENT="antigravity/1.13.3 darwin/arm64"
npm run tauri dev

# Тест 2: Windows
export CLAUDE_USER_AGENT="antigravity/1.13.3 windows/amd64"
npm run tauri dev

# Тест 3: Linux
export CLAUDE_USER_AGENT="antigravity/1.13.3 linux/amd64"
npm run tauri dev
```

### Автоматизированное тестирование

```bash
# Сделать скрипт исполняемым
chmod +x scripts/test_claude_variants.sh

# Запустить автоматизированные тесты
./scripts/test_claude_variants.sh
```

## 📊 Ожидаемые результаты

### До исправления:
```
❌ HTTP 429 Too Many Requests
{
  "error": {
    "message": "Quota exceeded for aiplatform.googleapis.com/...",
    "type": "rate_limit_error"
  }
}
```

### После исправления:
```
✅ HTTP 200 OK
{
  "id": "msg_...",
  "type": "message",
  "role": "assistant",
  "content": [
    {
      "type": "text",
      "text": "Hello!"
    }
  ],
  "model": "claude-sonnet-4-5",
  "usage": {...}
}
```

## 🐛 Troubleshooting

### Если все еще получаете 429:

1. **Проверьте версию User-Agent в логах:**
   ```bash
   grep "UpstreamClient User-Agent" <log-file>
   ```
   Должно быть: `1.13.3`

2. **Проверьте платформу:**
   - Для macOS: `darwin/arm64` (M1/M2) или `darwin/amd64` (Intel)
   - Для Windows: `windows/amd64`
   - Для Linux: `linux/amd64`

3. **Попробуйте точное совпадение с оригинальным Antigravity:**
   ```bash
   export CLAUDE_USER_AGENT="antigravity/1.13.3 darwin/arm64"
   ```

4. **Проверьте актуальность версии Google Antigravity:**
   ```bash
   # В оригинальном Antigravity:
   # Help → About
   # Проверьте версию и обновите CLAUDE_USER_AGENT если нужно
   ```

## 📝 Дополнительные файлы

Созданные файлы для отладки:
- `src-tauri/src/proxy/upstream/debug_variants.rs` - Варианты для тестирования
- `scripts/test_claude_variants.sh` - Автоматизированный тест-скрипт
- `CLAUDE_FIX_SUMMARY.md` - Этот документ

## 🔄 Следующие шаги

1. ✅ Обновлен User-Agent на 1.13.3
2. ✅ Добавлена поддержка environment variable
3. ✅ Добавлено логирование
4. ⏳ Тестирование с реальными Claude моделями
5. ⏳ Подтверждение исправления

## 📞 Обратная связь

Если исправление сработало или нужна дополнительная отладка, проверьте:
- Версию используемого User-Agent в логах
- HTTP статус код ответа
- Полное тело ответа от Google API

---

**Версия документа:** 1.0
**Дата:** 2026-01-09
**Статус:** Готово к тестированию
