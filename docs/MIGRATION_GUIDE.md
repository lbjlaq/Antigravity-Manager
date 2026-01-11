# Gemini 2.5 → Gemini 3.x Migration Guide

**Version**: 3.5.0
**Last Updated**: 2026-01-11
**Epic**: Epic-011 - Gemini 3 API Migration

---

## Overview

Gemini 3.x introduces a new thinking API format that replaces integer token budgets with discrete enum levels. This guide helps you migrate from Gemini 2.5 to Gemini 3.x models.

---

## What Changed

### API Format

**Gemini 2.5** (Old):
```json
{
  "generationConfig": {
    "thinkingConfig": {
      "includeThoughts": true,
      "thinkingBudget": 16000
    }
  }
}
```

**Gemini 3.x** (New):
```json
{
  "generationConfig": {
    "thinkingConfig": {
      "includeThoughts": true,
      "thinkingLevel": "HIGH"
    }
  }
}
```

### Key Differences

| Aspect | Gemini 2.5 | Gemini 3.x |
|--------|-----------|-----------|
| **Parameter** | `thinkingBudget` (integer) | `thinkingLevel` (enum) |
| **Values** | 0-32000 tokens | MINIMAL/LOW/MEDIUM/HIGH |
| **Control** | Fine-grained (any value) | Discrete levels (2-4) |
| **Auto-Injection** | No | Yes (MEDIUM for Flash, HIGH for Pro) |
| **Model Names** | `-thinking` suffix required | No suffix needed |

---

## Migration Strategies

### Strategy 1: Automatic Migration (Recommended)

**No code changes needed** - Our proxy handles conversion automatically.

**Before (Gemini 2.5)**:
```python
from openai import OpenAI

client = OpenAI(
    api_key="sk-antigravity",
    base_url="http://127.0.0.1:8045/v1"
)

response = client.chat.completions.create(
    model="gemini-2.5-flash-thinking",
    messages=[{"role": "user", "content": "Analyze this"}],
    extra_body={"thinking_budget": 16000}
)
```

**After (Gemini 3.x)**:
```python
# Just change the model name - proxy handles the rest
response = client.chat.completions.create(
    model="gemini-3-flash",
    messages=[{"role": "user", "content": "Analyze this"}]
    # thinking_budget optional - auto-injected by default
)
```

**What Happens**:
1. Proxy detects Gemini 3.x model
2. Auto-injects default thinking level (MEDIUM for Flash)
3. Converts to correct API format
4. Sends to Google API
5. Returns response in your requested format

---

### Strategy 2: Explicit Budget Control

Keep using budget values - proxy converts to appropriate levels.

**Before (Gemini 2.5)**:
```python
response = client.chat.completions.create(
    model="gemini-2.5-flash-thinking",
    messages=[...],
    extra_body={"thinking_budget": 24000}
)
```

**After (Gemini 3.x)**:
```python
response = client.chat.completions.create(
    model="gemini-3-flash",
    messages=[...],
    extra_body={"thinking_budget": 24000}  # Maps to HIGH level
)
```

**Budget Mapping**:
- 0-4000 → MINIMAL (Flash only)
- 4001-10000 → LOW
- 10001-20000 → MEDIUM (Flash only)
- 20001+ → HIGH

---

### Strategy 3: Direct Level Control

Use enum levels directly for maximum control.

```python
# Custom implementation using native Gemini API
import requests

response = requests.post(
    "https://generativelanguage.googleapis.com/v1/models/gemini-3-flash:generateContent",
    json={
        "contents": [{"role": "user", "parts": [{"text": "Analyze this"}]}],
        "generationConfig": {
            "thinkingConfig": {
                "includeThoughts": True,
                "thinkingLevel": "MEDIUM"
            }
        }
    },
    params={"key": "YOUR_API_KEY"}
)
```

---

## Model Migration

### Model Name Changes

| Gemini 2.5 | Gemini 3.x | Notes |
|-----------|-----------|-------|
| gemini-2.5-flash-thinking | gemini-3-flash | Thinking auto-injected |
| gemini-2.5-pro-thinking | gemini-3-pro-high | Default to HIGH level |
| gemini-2.5-flash | gemini-3-flash | No thinking by default |
| gemini-2.5-pro | gemini-3-pro-high | No thinking by default |

### Feature Comparison

| Feature | Gemini 2.5 | Gemini 3 Flash | Gemini 3 Pro |
|---------|-----------|---------------|--------------|
| **Thinking Levels** | Continuous (0-32000) | 4 levels | 2 levels |
| **Auto-Injection** | No | Yes (MEDIUM) | Yes (HIGH) |
| **MEDIUM Level** | N/A | ✅ Supported | ❌ Not supported |
| **Budget Control** | Direct | Via mapping | Via mapping |
| **Max Tokens** | 32000 | 8192 | 8192 |

---

## Code Migration Examples

### OpenAI Client Migration

**Before (Gemini 2.5)**:
```python
from openai import OpenAI

client = OpenAI(
    api_key="sk-antigravity",
    base_url="http://127.0.0.1:8045/v1"
)

# Explicit thinking model
response = client.chat.completions.create(
    model="gemini-2.5-flash-thinking",
    messages=[
        {"role": "user", "content": "Solve this problem"}
    ],
    extra_body={
        "thinking_budget": 16000
    }
)

# Access thinking
thinking = response.choices[0].message.reasoning_content
answer = response.choices[0].message.content
```

**After (Gemini 3.x)**:
```python
from openai import OpenAI

client = OpenAI(
    api_key="sk-antigravity",
    base_url="http://127.0.0.1:8045/v1"
)

# Auto-injected thinking
response = client.chat.completions.create(
    model="gemini-3-flash",
    messages=[
        {"role": "user", "content": "Solve this problem"}
    ]
    # No thinking_budget needed - auto-injected
)

# Access thinking (same as before)
thinking = response.choices[0].message.reasoning_content
answer = response.choices[0].message.content
```

---

### Claude Client Migration

**Before (Gemini 2.5)**:
```python
from anthropic import Anthropic

client = Anthropic(
    api_key="sk-antigravity",
    base_url="http://127.0.0.1:8045"
)

response = client.messages.create(
    model="gemini-2.5-flash-thinking",
    messages=[
        {"role": "user", "content": "Analyze"}
    ],
    thinking={
        "type": "enabled",
        "budget": 16000
    }
)

# Access thinking
for block in response.content:
    if block.type == "thinking":
        print(block.thinking)
    elif block.type == "text":
        print(block.text)
```

**After (Gemini 3.x)**:
```python
from anthropic import Anthropic

client = Anthropic(
    api_key="sk-antigravity",
    base_url="http://127.0.0.1:8045"
)

response = client.messages.create(
    model="gemini-3-flash",
    messages=[
        {"role": "user", "content": "Analyze"}
    ]
    # thinking config optional - auto-injected
)

# Access thinking (same as before)
for block in response.content:
    if block.type == "thinking":
        print(block.thinking)
    elif block.type == "text":
        print(block.text)
```

---

### cURL Migration

**Before (Gemini 2.5)**:
```bash
curl -X POST http://localhost:8045/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer sk-antigravity" \
  -d '{
    "model": "gemini-2.5-flash-thinking",
    "messages": [
      {"role": "user", "content": "Test"}
    ],
    "thinking_budget": 16000
  }'
```

**After (Gemini 3.x)**:
```bash
curl -X POST http://localhost:8045/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer sk-antigravity" \
  -d '{
    "model": "gemini-3-flash",
    "messages": [
      {"role": "user", "content": "Test"}
    ]
  }'
```

---

## Configuration Migration

### Proxy Configuration

**Before (Gemini 2.5)**:
```json
{
  "model_mapping": {
    "gpt-4": "gemini-2.5-pro-thinking",
    "gpt-3.5-turbo": "gemini-2.5-flash-thinking"
  },
  "default_thinking_budget": 16000
}
```

**After (Gemini 3.x)**:
```json
{
  "model_mapping": {
    "gpt-4": "gemini-3-pro-high",
    "gpt-3.5-turbo": "gemini-3-flash"
  },
  "auto_inject_thinking": true,
  "default_thinking_level": {
    "flash": "MEDIUM",
    "pro": "HIGH"
  }
}
```

---

## Testing Migration

### Update Test Cases

**Before (Gemini 2.5)**:
```rust
#[tokio::test]
async fn test_gemini_thinking() {
    let response = client
        .post("/v1/chat/completions")
        .json(&json!({
            "model": "gemini-2.5-flash-thinking",
            "messages": [...],
            "thinking_budget": 16000
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
}
```

**After (Gemini 3.x)**:
```rust
#[tokio::test]
async fn test_gemini_thinking() {
    let response = client
        .post("/v1/chat/completions")
        .json(&json!({
            "model": "gemini-3-flash",
            "messages": [...]
            // No thinking_budget needed
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    // Verify thinking auto-injected
    let body: Value = response.json().await.unwrap();
    assert!(body["choices"][0]["message"]["reasoning_content"].is_string());
}
```

---

## Troubleshooting

### Issue 1: "must use thinkingLevel API" Error

**Scenario**: Getting API format errors

**Cause**: Sending raw Gemini 2.5 API format to Gemini 3.x model

**Solution**: Use proxy auto-conversion instead of direct API calls

```python
# ❌ Don't do this
requests.post(
    "http://localhost:8045/v1/native/gemini",
    json={
        "model": "gemini-3-flash",
        "generationConfig": {
            "thinkingConfig": {
                "thinkingBudget": 16000  # Wrong API
            }
        }
    }
)

# ✅ Do this instead
client.chat.completions.create(
    model="gemini-3-flash",
    messages=[...],
    extra_body={"thinking_budget": 16000}  # Proxy converts automatically
)
```

---

### Issue 2: No Thinking in Responses

**Scenario**: Responses missing thinking content

**Cause**: Auto-injection disabled or model doesn't support thinking

**Solution**: Verify model name and enable auto-injection

```python
# Verify model supports thinking
response = requests.get("http://localhost:8045/v1/models/gemini-3-flash")
print(response.json()["thinking_support"])  # Should show "auto_injected"

# Explicitly enable thinking
response = client.chat.completions.create(
    model="gemini-3-flash",
    messages=[...],
    extra_body={"thinking_budget": 15000}
)
```

---

### Issue 3: Invalid Level for Pro Models

**Scenario**: "invalid thinkingLevel: 'MEDIUM'" error

**Cause**: MEDIUM level only supported by Flash, not Pro

**Solution**: Use appropriate budget ranges for Pro

```python
# ❌ Don't use MEDIUM-range budgets for Pro
response = client.chat.completions.create(
    model="gemini-3-pro-high",
    messages=[...],
    extra_body={"thinking_budget": 15000}  # Maps to MEDIUM - not supported
)

# ✅ Use LOW or HIGH range budgets
response = client.chat.completions.create(
    model="gemini-3-pro-high",
    messages=[...],
    extra_body={"thinking_budget": 25000}  # Maps to HIGH - supported
)
```

---

### Issue 4: Unexpected Costs

**Scenario**: Higher than expected API costs

**Cause**: Auto-injection defaulting to MEDIUM/HIGH levels

**Solution**: Explicitly set lower budget values

```python
# Control costs with lower budgets
response = client.chat.completions.create(
    model="gemini-3-flash",
    messages=[...],
    extra_body={"thinking_budget": 5000}  # Forces LOW level
)
```

---

## Performance Optimization

### Cost Optimization

**Before (Gemini 2.5)**:
```python
# Fine-grained budget control
budgets = {
    "simple": 8000,
    "moderate": 16000,
    "complex": 24000
}

response = client.chat.completions.create(
    model="gemini-2.5-flash-thinking",
    extra_body={"thinking_budget": budgets[complexity]}
)
```

**After (Gemini 3.x)**:
```python
# Level-based control
levels = {
    "simple": 5000,    # LOW
    "moderate": 15000, # MEDIUM
    "complex": 25000   # HIGH
}

response = client.chat.completions.create(
    model="gemini-3-flash",
    extra_body={"thinking_budget": levels[complexity]}
)
```

---

### Latency Optimization

**Flash vs. Pro Trade-offs**:

| Task | Recommended Model | Thinking Level | Latency |
|------|------------------|----------------|---------|
| Quick queries | gemini-3-flash | LOW (5000) | ~1.2x |
| Standard tasks | gemini-3-flash | MEDIUM (15000) | ~1.5x |
| Complex analysis | gemini-3-flash | HIGH (25000) | ~1.8x |
| Maximum quality | gemini-3-pro-high | HIGH (25000) | ~2.2x |

---

## Rollback Plan

If you need to revert to Gemini 2.5:

```python
# Temporarily switch back to Gemini 2.5
response = client.chat.completions.create(
    model="gemini-2.5-flash-thinking",  # Use -thinking suffix
    messages=[...],
    extra_body={"thinking_budget": 16000}
)
```

**Note**: Gemini 2.5 support maintained for backward compatibility.

---

## Migration Checklist

- [ ] Update model names (remove `-thinking` suffix)
- [ ] Test with default auto-injection (no budget specified)
- [ ] Verify thinking content in responses
- [ ] Update budget values for level mapping
- [ ] Adjust tests for new model names
- [ ] Update configuration files
- [ ] Monitor costs with new level system
- [ ] Document team migration process
- [ ] Train team on new API format
- [ ] Plan gradual rollout
- [ ] Prepare rollback procedures

---

## Best Practices

### 1. Gradual Migration

```python
# Phase 1: Test with new models
test_response = client.chat.completions.create(
    model="gemini-3-flash",
    messages=[test_message]
)

# Phase 2: A/B test quality
compare_responses(gemini_2_5_response, gemini_3_response)

# Phase 3: Full migration
production_response = client.chat.completions.create(
    model="gemini-3-flash",
    messages=[production_message]
)
```

---

### 2. Monitor Quality

```python
# Track thinking quality metrics
def track_quality(response):
    thinking = response.choices[0].message.reasoning_content
    answer = response.choices[0].message.content

    metrics = {
        "thinking_length": len(thinking),
        "answer_quality": evaluate_quality(answer),
        "latency": response.response_time,
        "cost": estimate_cost(response)
    }

    log_metrics(metrics)
```

---

### 3. Cost Tracking

```python
# Monitor API costs
level_costs = {
    "MINIMAL": 0.5,
    "LOW": 1.0,
    "MEDIUM": 1.5,
    "HIGH": 2.0
}

def estimate_cost(model, budget):
    level = map_budget_to_level(model, budget)
    return level_costs[level]
```

---

## FAQ

**Q: Do I need to change my code for Gemini 3?**

A: No, proxy handles API conversion automatically. Just change model names.

---

**Q: What happens to my existing budget values?**

A: They're automatically mapped to appropriate enum levels by the proxy.

---

**Q: Can I still use Gemini 2.5 models?**

A: Yes, Gemini 2.5 support is maintained for backward compatibility.

---

**Q: Why does Flash support MEDIUM but Pro doesn't?**

A: Google's design decision - Flash has more granular control (4 levels vs. 2).

---

**Q: Will migration affect my response quality?**

A: No, quality should be similar or better with appropriate level selection.

---

**Q: How do I test the migration?**

A: Run your existing tests with new model names and verify thinking content is present.

---

## Support Resources

- **Technical Specification**: `docs/technical-specs/GEMINI_API_ANALYSIS.md`
- **API Reference**: `docs/API_REFERENCE.md`
- **Developer Guide**: `docs/DEVELOPER_GUIDE.md`
- **Testing Guide**: `docs/TESTING_GUIDE.md`
- **Protocol Comparison**: `docs/comparison/ANTHROPIC_VS_GOOGLE_THINKING.md`

---

## Timeline Recommendations

**Week 1**: Testing
- Test Gemini 3.x models in development
- Compare quality with Gemini 2.5
- Identify any issues

**Week 2**: Gradual Rollout
- Migrate 10% of traffic to Gemini 3.x
- Monitor metrics and costs
- Adjust as needed

**Week 3**: Full Migration
- Migrate remaining traffic
- Update documentation
- Train team on new API

**Week 4**: Optimization
- Fine-tune level selection
- Optimize costs
- Document lessons learned

---

**Document Status**: ✅ Complete
**Last Updated**: 2026-01-11 (Epic-011 Story-006)
**Next Review**: Post-migration feedback integration
