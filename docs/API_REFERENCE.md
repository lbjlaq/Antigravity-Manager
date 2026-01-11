# API Reference - Gemini 3 Thinking Support

**Version**: 3.5.0
**Last Updated**: 2026-01-11
**Purpose**: User-facing API documentation for Gemini 3 thinking features

---

## Overview

This document provides API reference for using Gemini 3 thinking capabilities through Antigravity Tools proxy.

---

## Supported Models

| Model | Thinking Support | API Format | Levels | Auto-Injection |
|-------|------------------|------------|--------|----------------|
| gemini-3-flash | ✅ Auto-injected | thinkingLevel | 4 (MINIMAL/LOW/MEDIUM/HIGH) | ✅ Yes (MEDIUM) |
| gemini-3-pro-high | ✅ Auto-injected | thinkingLevel | 2 (LOW/HIGH) | ✅ Yes (HIGH) |
| gemini-3-pro-low | ✅ Auto-injected | thinkingLevel | 2 (LOW/HIGH) | ✅ Yes (HIGH) |
| gemini-2.5-flash-thinking | ✅ Explicit opt-in | thinkingBudget | Integer (0-32000) | ❌ No |
| gemini-2.5-pro-thinking | ✅ Explicit opt-in | thinkingBudget | Integer (0-32000) | ❌ No |

---

## Gemini 3 Thinking API

### Flash Models (4 Levels)

**Budget Mapping**:
- **MINIMAL** (0-4000 tokens): Basic reasoning, fastest response
- **LOW** (4001-10000 tokens): Standard reasoning, cost-optimized
- **MEDIUM** (10001-20000 tokens): Balanced reasoning (Flash exclusive, default)
- **HIGH** (20001+ tokens): Deep reasoning, maximum quality

**Example Request (OpenAI format)**:
```bash
curl -X POST http://localhost:8045/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer sk-antigravity" \
  -d '{
    "model": "gemini-3-flash",
    "messages": [
      {"role": "user", "content": "Solve this complex problem"}
    ],
    "thinking_budget": 15000
  }'
```

**Proxy Auto-Converts**:
- Client sends: `thinking_budget: 15000`
- Proxy converts: `thinkingLevel: "MEDIUM"` (15000 is in 10001-20000 range)
- Upstream receives: Correct Gemini 3 API format

**Response**:
```json
{
  "id": "chatcmpl-...",
  "object": "chat.completion",
  "choices": [{
    "message": {
      "role": "assistant",
      "content": "The solution is...",
      "reasoning_content": "Let me analyze this problem..."
    }
  }]
}
```

---

### Pro Models (2 Levels)

**Budget Mapping**:
- **LOW** (0-16000 tokens): Efficient reasoning, faster response
- **HIGH** (16001+ tokens): Maximum reasoning, comprehensive analysis

**Example Request (Claude format)**:
```bash
curl -X POST http://localhost:8045/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: sk-antigravity" \
  -d '{
    "model": "gemini-3-pro-high",
    "messages": [
      {"role": "user", "content": "Analyze this architecture"}
    ],
    "thinking": {
      "type": "enabled",
      "budget": 20000
    }
  }'
```

**Proxy Auto-Converts**:
- Client sends: `thinking.budget: 20000`
- Proxy converts: `thinkingLevel: "HIGH"` (20000 > 16000)
- Upstream receives: Correct Gemini 3 API format

**Response**:
```json
{
  "id": "msg_...",
  "type": "message",
  "content": [
    {
      "type": "thinking",
      "thinking": "Let me analyze the architecture..."
    },
    {
      "type": "text",
      "text": "The architecture has..."
    }
  ]
}
```

---

### Auto-Injection

**Flash and Pro models automatically inject thinking**. You don't need to specify anything:

```bash
# No thinking config needed
curl -X POST http://localhost:8045/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemini-3-flash",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

This automatically gets:
- **Flash**: `thinkingLevel: "MEDIUM"` (default)
- **Pro**: `thinkingLevel: "HIGH"` (default)

---

### Budget Control

You can control the thinking level via budget parameter:

**OpenAI Format**:
```json
{
  "model": "gemini-3-flash",
  "messages": [...],
  "thinking_budget": 5000
}
```
Auto-converts to `thinkingLevel: "LOW"`

**Claude Format**:
```json
{
  "model": "gemini-3-flash",
  "messages": [...],
  "thinking": {
    "type": "enabled",
    "budget": 5000
  }
}
```
Auto-converts to `thinkingLevel: "LOW"`

---

## Budget-to-Level Mapping Reference

### Flash (4 Levels)

| Budget Range | Level | Quality | Latency | Cost |
|--------------|-------|---------|---------|------|
| 0-4000 | MINIMAL | Basic | Fastest | Lowest |
| 4001-10000 | LOW | Standard | Fast | Low |
| 10001-20000 | MEDIUM | Balanced | Medium | Medium |
| 20001+ | HIGH | Maximum | Slower | Higher |

### Pro (2 Levels)

| Budget Range | Level | Quality | Latency | Cost |
|--------------|-------|---------|---------|------|
| 0-16000 | LOW | Efficient | Fast | Lower |
| 16001+ | HIGH | Maximum | Slower | Higher |

---

## Error Handling

### Invalid API Format

**Error**: Wrong API format for model generation

```json
{
  "error": {
    "message": "Gemini 3.x model 'gemini-3-flash' must use thinkingLevel API, not thinkingBudget",
    "type": "invalid_request_error",
    "code": "gemini_api_mismatch"
  }
}
```

**Cause**: Sending raw Gemini 2.5 API format to Gemini 3.x model

**Solution**: Use proxy auto-conversion by sending budget values instead of raw API

---

### Invalid Level

**Error**: Level not supported by model

```json
{
  "error": {
    "message": "Model 'gemini-3-pro-high' has invalid thinkingLevel: 'MEDIUM'. Valid levels: LOW, HIGH",
    "type": "invalid_request_error",
    "code": "invalid_thinking_level"
  }
}
```

**Cause**: MEDIUM level only supported by Flash, not Pro

**Solution**: Use budget ≤16000 (LOW) or >16000 (HIGH) for Pro models

---

## Protocol Examples

### OpenAI Protocol

**Basic Request**:
```python
from openai import OpenAI

client = OpenAI(
    api_key="sk-antigravity",
    base_url="http://127.0.0.1:8045/v1"
)

response = client.chat.completions.create(
    model="gemini-3-flash",
    messages=[
        {"role": "user", "content": "Solve this problem"}
    ]
)

# Response includes thinking
print(response.choices[0].message.reasoning_content)  # Thinking process
print(response.choices[0].message.content)            # Final answer
```

**With Budget Control**:
```python
response = client.chat.completions.create(
    model="gemini-3-flash",
    messages=[...],
    extra_body={"thinking_budget": 5000}  # Forces LOW level
)
```

---

### Claude Protocol

**Basic Request**:
```python
from anthropic import Anthropic

client = Anthropic(
    api_key="sk-antigravity",
    base_url="http://127.0.0.1:8045"
)

response = client.messages.create(
    model="gemini-3-pro-high",
    messages=[
        {"role": "user", "content": "Analyze this"}
    ]
)

# Response includes thinking blocks
for block in response.content:
    if block.type == "thinking":
        print(block.thinking)  # Thinking process
    elif block.type == "text":
        print(block.text)      # Final answer
```

**With Budget Control**:
```python
response = client.messages.create(
    model="gemini-3-pro-high",
    messages=[...],
    thinking={
        "type": "enabled",
        "budget": 25000  # Forces HIGH level
    }
)
```

---

### Gemini Native Protocol

**Direct API Call** (not recommended, use proxy for auto-conversion):
```bash
curl -X POST "https://generativelanguage.googleapis.com/v1/models/gemini-3-flash:generateContent?key=YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "contents": [
      {"role": "user", "parts": [{"text": "Hello"}]}
    ],
    "generationConfig": {
      "thinkingConfig": {
        "includeThoughts": true,
        "thinkingLevel": "MEDIUM"
      }
    }
  }'
```

---

## Best Practices

### Choosing the Right Level

**MINIMAL (Flash only)**:
- ✅ Simple queries
- ✅ Need fastest response
- ✅ Cost is primary concern
- ❌ Not for complex reasoning

**LOW**:
- ✅ Standard questions
- ✅ Balance speed/quality
- ✅ General-purpose default
- ❌ Not for deep analysis

**MEDIUM (Flash only)**:
- ✅ Moderate complexity
- ✅ Best Flash default
- ✅ Good cost/quality balance
- ❌ Not available for Pro

**HIGH**:
- ✅ Complex reasoning
- ✅ Maximum quality needed
- ✅ Time not critical
- ❌ Higher latency/cost

---

### Cost Optimization

**Use Auto-Injection Defaults**:
- Flash MEDIUM is optimized for most use cases
- Pro HIGH maximizes quality for enterprise

**Override Only When Needed**:
```python
# Low-complexity task - save costs
response = client.chat.completions.create(
    model="gemini-3-flash",
    messages=[...],
    extra_body={"thinking_budget": 3000}  # MINIMAL level
)

# High-complexity task - maximize quality
response = client.chat.completions.create(
    model="gemini-3-flash",
    messages=[...],
    extra_body={"thinking_budget": 25000}  # HIGH level
)
```

---

### Performance Optimization

**Use Flash for Speed-Sensitive Tasks**:
```python
# Fast response needed
client.chat.completions.create(
    model="gemini-3-flash",  # Faster than Pro
    extra_body={"thinking_budget": 5000}  # LOW level
)
```

**Use Pro for Quality-Critical Tasks**:
```python
# Maximum quality needed
client.chat.completions.create(
    model="gemini-3-pro-high",  # Higher quality
    extra_body={"thinking_budget": 25000}  # HIGH level
)
```

---

## Migration from Gemini 2.5

### Automatic Migration

No code changes needed if using proxy:

**Before (Gemini 2.5)**:
```python
response = client.chat.completions.create(
    model="gemini-2.5-flash-thinking",
    messages=[...],
    extra_body={"thinking_budget": 16000}
)
```

**After (Gemini 3.x)**:
```python
response = client.chat.completions.create(
    model="gemini-3-flash",  # Just change model
    messages=[...]  # thinking_budget optional, auto-injected
)
```

Proxy handles conversion automatically.

---

### Manual Migration

If calling Google API directly:

**Step 1**: Change model name
```python
# Before
model = "gemini-2.5-flash-thinking"

# After
model = "gemini-3-flash"
```

**Step 2**: Update API format
```python
# Before (Gemini 2.5)
config = {
    "thinkingConfig": {
        "includeThoughts": True,
        "thinkingBudget": 16000
    }
}

# After (Gemini 3.x)
config = {
    "thinkingConfig": {
        "includeThoughts": True,
        "thinkingLevel": "HIGH"  # 16000 maps to HIGH for Flash
    }
}
```

---

## Troubleshooting

### No Thinking in Response

**Symptoms**: Response missing `reasoning_content` or `thinking` blocks

**Possible Causes**:
1. Auto-injection disabled in proxy config
2. Model not a Gemini 3.x thinking model
3. Explicit `thinkingLevel: null` sent

**Solution**:
```python
# Explicitly enable thinking
response = client.chat.completions.create(
    model="gemini-3-flash",
    messages=[...],
    extra_body={"thinking_budget": 15000}
)
```

---

### API Format Errors

**Symptoms**: "must use thinkingLevel API" error

**Cause**: Sending raw Gemini 2.5 API to Gemini 3.x model

**Solution**: Let proxy handle conversion, don't send raw `thinkingConfig`

---

### Invalid Level Errors

**Symptoms**: "invalid thinkingLevel: 'MEDIUM'" for Pro

**Cause**: MEDIUM only supported by Flash

**Solution**: Use appropriate budget ranges:
- Pro LOW: 0-16000
- Pro HIGH: 16001+

---

## Advanced Usage

### Custom Level Mapping

Override default budget-to-level mapping in proxy config:

```json
{
  "thinking_level_mapping": {
    "gemini-3-flash": {
      "minimal": [0, 5000],
      "low": [5001, 12000],
      "medium": [12001, 24000],
      "high": [24001, 32000]
    }
  }
}
```

---

### Disable Auto-Injection

To prevent automatic thinking injection:

```json
{
  "auto_inject_thinking": false
}
```

Then thinking only enabled when explicitly requested.

---

## FAQ

**Q: Do I need to change my code for Gemini 3?**

A: No, proxy handles API conversion automatically.

---

**Q: What's the difference between Flash and Pro thinking?**

A: Flash has 4 levels (more granular), Pro has 2 levels (simpler).

---

**Q: Can I use the same budget values for all models?**

A: Yes, proxy automatically maps budgets to appropriate levels per model.

---

**Q: Why does Flash support MEDIUM but Pro doesn't?**

A: Google's design decision - Flash has more granular control.

---

**Q: Is thinking always included in responses?**

A: For Gemini 3.x, yes (auto-injected). For Gemini 2.5, only if explicitly requested.

---

## References

- **Technical Specification**: `docs/technical-specs/GEMINI_API_ANALYSIS.md`
- **Protocol Comparison**: `docs/comparison/ANTHROPIC_VS_GOOGLE_THINKING.md`
- **Migration Guide**: `docs/MIGRATION_GUIDE.md`
- **Developer Guide**: `docs/DEVELOPER_GUIDE.md`

---

**Document Status**: ✅ Complete
**Last Updated**: 2026-01-11 (Epic-011 Story-006)
**Next Review**: User feedback integration
