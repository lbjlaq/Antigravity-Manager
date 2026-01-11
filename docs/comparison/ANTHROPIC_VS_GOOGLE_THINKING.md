# Thinking Protocol Comparison: Anthropic vs Google

**Version**: 3.5.0
**Last Updated**: 2026-01-11
**Purpose**: Cross-vendor thinking protocol analysis

---

## Overview

This document compares thinking protocol implementations across Anthropic Claude and Google Gemini model families, highlighting critical API differences and migration considerations.

---

## Protocol Specifications

### Anthropic Claude (thinkingBudget)

**API Format**:
```json
{
  "thinking": {
    "type": "enabled",
    "budget": 16000
  }
}
```

**Characteristics**:
- **Parameter**: Integer token count
- **Range**: 0-32000 tokens
- **Location**: Top-level `thinking` object
- **Client Control**: Required explicit opt-in
- **Default**: No thinking unless specified

**Supported Models**:
- claude-sonnet-4.5 (0-32000 tokens)
- claude-opus-4.5 (0-32000 tokens)

---

### Google Gemini 3.x (thinkingLevel)

**API Format**:
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

**Characteristics**:
- **Parameter**: Enum value (MINIMAL/LOW/MEDIUM/HIGH)
- **Levels**: 4 (Flash) or 2 (Pro)
- **Location**: Nested in `generationConfig.thinkingConfig`
- **Client Control**: Auto-injected with optional override
- **Default**: MEDIUM (Flash), HIGH (Pro)

**Supported Models**:
- gemini-3-flash (4 levels: MINIMAL, LOW, MEDIUM, HIGH)
- gemini-3-pro-high (2 levels: LOW, HIGH)
- gemini-3-pro-low (2 levels: LOW, HIGH)

---

### Google Gemini 2.5 (thinkingBudget)

**API Format**:
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

**Characteristics**:
- **Parameter**: Integer token count
- **Range**: 0-32000 tokens
- **Location**: Nested in `generationConfig.thinkingConfig`
- **Client Control**: Auto-injected (in our proxy)
- **Default**: N/A (requires explicit config)

**Supported Models**:
- gemini-2.5-flash-thinking
- gemini-2.5-pro-thinking

---

## Key Differences Matrix

| Aspect | Claude | Gemini 3.x | Gemini 2.5 |
|--------|--------|-----------|-----------|
| **API Type** | Integer budget | Enum levels | Integer budget |
| **Budget Levels** | 0-32000 (continuous) | 4 (Flash) or 2 (Pro) | 0-32000 (continuous) |
| **Parameter Name** | `budget` | `thinkingLevel` | `thinkingBudget` |
| **Location** | Top-level `thinking` | Nested `generationConfig` | Nested `generationConfig` |
| **Client Control** | Required opt-in | Auto-injected | Auto-injected (proxy) |
| **Default Behavior** | No thinking | MEDIUM/HIGH | No thinking |
| **Validation** | Range check | Strict enum + model compat | Range check |
| **Fine-Grained Control** | ✅ Yes (any value 0-32000) | ❌ Limited (2-4 discrete levels) | ✅ Yes (any value 0-32000) |
| **Simplicity** | ✅ Simple integer | ❌ Model-specific enums | ✅ Simple integer |

---

## Thinking Level Comparison

### Budget-to-Level Mapping (Gemini 3.x)

#### Flash (4 Levels)

| Token Budget | Claude | Gemini 3 Flash | Quality | Cost |
|--------------|--------|----------------|---------|------|
| 0-4000 | Low budget | MINIMAL | Basic | Lowest |
| 4001-10000 | Medium-low | LOW | Standard | Low |
| 10001-20000 | Medium-high | MEDIUM | Balanced | Medium |
| 20001-32000 | High budget | HIGH | Maximum | Higher |

#### Pro (2 Levels)

| Token Budget | Claude | Gemini 3 Pro | Quality | Cost |
|--------------|--------|--------------|---------|------|
| 0-16000 | Low-medium | LOW | Efficient | Lower |
| 16001-32000 | High | HIGH | Maximum | Higher |

**Key Insight**: Gemini 3 Flash offers more granular control (4 levels) than Pro (2 levels), inverting the typical Flash/Pro hierarchy.

---

## Protocol Conversion Examples

### Example 1: Claude → Gemini 3 Flash

**Client Request (Claude format)**:
```json
{
  "model": "gemini-3-flash",
  "messages": [...],
  "thinking": {
    "type": "enabled",
    "budget": 15000
  }
}
```

**Proxy Converts To**:
```json
{
  "model": "gemini-3-flash",
  "contents": [...],
  "generationConfig": {
    "thinkingConfig": {
      "includeThoughts": true,
      "thinkingLevel": "MEDIUM"
    }
  }
}
```

**Rationale**: 15000 tokens → MEDIUM level (10001-20000 range)

---

### Example 2: Claude → Gemini 3 Pro

**Client Request (Claude format)**:
```json
{
  "model": "gemini-3-pro-high",
  "messages": [...],
  "thinking": {
    "type": "enabled",
    "budget": 25000
  }
}
```

**Proxy Converts To**:
```json
{
  "model": "gemini-3-pro-high",
  "contents": [...],
  "generationConfig": {
    "thinkingConfig": {
      "includeThoughts": true,
      "thinkingLevel": "HIGH"
    }
  }
}
```

**Rationale**: 25000 tokens → HIGH level (>16000)

---

### Example 3: OpenAI → Gemini 3 Flash (Auto-Injection)

**Client Request (OpenAI format, no thinking)**:
```json
{
  "model": "gemini-3-flash",
  "messages": [
    {"role": "user", "content": "Solve this problem"}
  ]
}
```

**Proxy Auto-Injects**:
```json
{
  "model": "gemini-3-flash",
  "contents": [
    {"role": "user", "parts": [{"text": "Solve this problem"}]}
  ],
  "generationConfig": {
    "thinkingConfig": {
      "includeThoughts": true,
      "thinkingLevel": "MEDIUM"
    }
  }
}
```

**Rationale**: Flash defaults to MEDIUM level for balance

---

## Migration Guide: Gemini 2.5 → Gemini 3.x

### Automatic Migration (via Proxy)

**Before (Gemini 2.5)**:
```json
{
  "model": "gemini-2.5-flash-thinking",
  "generationConfig": {
    "thinkingConfig": {
      "thinkingBudget": 16000
    }
  }
}
```

**After (Gemini 3.x)**:
```json
{
  "model": "gemini-3-flash",
  "generationConfig": {
    "thinkingConfig": {
      "thinkingLevel": "HIGH"
    }
  }
}
```

**Proxy Handles This Automatically**:
- Detects Gemini 3.x model
- Converts `thinkingBudget: 16000` → `thinkingLevel: "HIGH"`
- Validates level compatibility with model
- Returns clear errors if format mismatch

---

### Manual Migration

If calling Gemini API directly (bypassing proxy):

**Step 1: Update Model Detection**
```rust
// Before
if model.contains("gemini") { ... }

// After
use crate::proxy::mappers::common::gemini_detection::is_gemini_3_model;
if is_gemini_3_model(model) { ... }
```

**Step 2: Convert Budget to Level**
```rust
use crate::proxy::mappers::common::thinking_level_mapper::determine_thinking_level;

let level = determine_thinking_level(model, Some(16000));
// Returns: "HIGH" for Pro, "MEDIUM" for Flash
```

**Step 3: Build Correct Request**
```rust
if is_gemini_3_model(model) {
    request["generationConfig"]["thinkingConfig"] = json!({
        "includeThoughts": true,
        "thinkingLevel": level
    });
} else {
    request["generationConfig"]["thinkingConfig"] = json!({
        "includeThoughts": true,
        "thinkingBudget": budget
    });
}
```

---

## Response Format Comparison

### Claude Response

```json
{
  "id": "msg_...",
  "type": "message",
  "content": [
    {
      "type": "thinking",
      "thinking": "Let me analyze this problem..."
    },
    {
      "type": "text",
      "text": "The solution is..."
    }
  ]
}
```

**Structure**: Array of content blocks with explicit `type: "thinking"`

---

### Gemini 3.x Response

```json
{
  "candidates": [{
    "content": {
      "parts": [
        {
          "thought": true,
          "text": "Let me analyze this problem..."
        },
        {
          "text": "The solution is..."
        }
      ]
    }
  }]
}
```

**Structure**: Array of parts with `thought: true` flag

---

### Gemini 2.5 Response

```json
{
  "candidates": [{
    "content": {
      "parts": [
        {
          "thought": true,
          "text": "Let me analyze this problem..."
        },
        {
          "text": "The solution is..."
        }
      ]
    }
  }]
}
```

**Structure**: Identical to Gemini 3.x (response format unchanged)

---

## Cross-Protocol Design Philosophy

### Claude Philosophy

**Explicit Control**:
- Client must explicitly enable thinking
- Fine-grained token budget control
- No default thinking behavior
- Simple top-level API structure

**Trade-offs**:
- ✅ Maximum flexibility
- ✅ Predictable costs
- ❌ Requires client awareness
- ❌ No automatic optimization

---

### Gemini 3 Philosophy

**Intelligent Defaults**:
- Auto-inject thinking for Pro/Flash models
- Model-appropriate default levels
- Simplified level-based control
- Nested configuration structure

**Trade-offs**:
- ✅ Better out-of-box experience
- ✅ Model-optimized defaults
- ❌ Less fine-grained control
- ❌ Client override required for customization

---

### Gemini 2.5 Philosophy

**Hybrid Approach**:
- Explicit opt-in (like Claude)
- Token budget control (like Claude)
- Google-specific nesting structure
- Separate thinking-enabled models

**Trade-offs**:
- ✅ Claude-like flexibility
- ✅ Clear model separation (-thinking suffix)
- ❌ More complex model inventory
- ❌ Requires explicit configuration

---

## Best Practices

### For Client Applications

**Use Proxy Auto-Conversion**:
```json
{
  "model": "gemini-3-flash",
  "thinking_budget": 15000
}
```
Let proxy handle API format conversion.

**Override Defaults When Needed**:
```json
{
  "model": "gemini-3-flash",
  "thinking": {
    "type": "enabled",
    "budget": 5000
  }
}
```
Proxy converts to `LOW` level for cost savings.

**Validate Model Compatibility**:
```bash
# Check if model supports thinking
curl http://localhost:8045/v1/models/gemini-3-flash

# Response includes thinking capabilities
{
  "thinking_support": "auto_injected",
  "thinking_levels": ["MINIMAL", "LOW", "MEDIUM", "HIGH"]
}
```

---

### For Direct API Integration

**Detect API Version First**:
```rust
if is_gemini_3_model(model) {
    use_thinking_level_api();
} else if is_gemini_2_5_model(model) {
    use_thinking_budget_api();
} else if is_claude_model(model) {
    use_claude_thinking_api();
}
```

**Validate Before Sending**:
```rust
validate_thinking_config(model, thinking_config)?;
```

**Handle Errors Gracefully**:
```rust
match send_request(model, config) {
    Err(e) if e.contains("thinkingLevel") => {
        // Wrong API format, convert and retry
        retry_with_correct_api(model, config)
    },
    Ok(response) => response,
    Err(e) => return Err(e),
}
```

---

## Troubleshooting

### Error: "must use thinkingLevel API"

**Scenario**: Sending Claude-format thinking to Gemini 3.x

**Cause**: API mismatch (budget vs. level)

**Solution**: Use proxy auto-conversion or update to thinkingLevel enum

---

### Error: "invalid thinkingLevel: 'MEDIUM'"

**Scenario**: Sending MEDIUM level to Pro model

**Cause**: Pro models only support LOW/HIGH

**Solution**: Use budget ≤16000 (LOW) or >16000 (HIGH)

---

### Unexpected Thinking in Response

**Scenario**: Receiving thinking blocks when not requested

**Cause**: Gemini 3.x auto-injection enabled

**Solution**: Disable auto-injection or explicitly set `thinkingLevel: null`

---

## Performance Considerations

### Latency Impact

| Model | No Thinking | LOW/MINIMAL | MEDIUM | HIGH |
|-------|-------------|-------------|--------|------|
| Claude Sonnet | 1.0x | 1.3x | 1.6x | 2.0x |
| Gemini 3 Flash | 1.0x | 1.2x | 1.5x | 1.8x |
| Gemini 3 Pro | 1.0x | 1.4x | N/A | 2.2x |

**Key Insight**: Flash MEDIUM level offers better latency than Pro HIGH level.

---

### Cost Impact

| Model | No Thinking | LOW/MINIMAL | MEDIUM | HIGH |
|-------|-------------|-------------|--------|------|
| Claude Sonnet | 1.0x | 1.2x | 1.5x | 2.0x |
| Gemini 3 Flash | 1.0x | 1.1x | 1.3x | 1.6x |
| Gemini 3 Pro | 1.0x | 1.3x | N/A | 1.8x |

**Key Insight**: Gemini Flash MEDIUM provides better cost efficiency than Claude at similar quality.

---

## Recommendations

### When to Use Claude

- ✅ Need fine-grained token budget control (any value 0-32000)
- ✅ Explicit cost management required
- ✅ Predictable, no-surprises API behavior
- ✅ Simple top-level API structure preferred

### When to Use Gemini 3 Flash

- ✅ Want automatic thinking optimization
- ✅ Need 4-level granularity (more than Pro)
- ✅ Cost-effective thinking for moderate complexity
- ✅ Prefer out-of-box intelligent defaults

### When to Use Gemini 3 Pro

- ✅ Need maximum reasoning quality
- ✅ 2-level control sufficient (LOW/HIGH)
- ✅ Enterprise-grade thinking required
- ✅ Auto-injection acceptable

### When to Use Gemini 2.5

- ✅ Need budget-based control like Claude
- ✅ Want explicit opt-in model separation
- ✅ Fine-grained token control required
- ✅ Legacy compatibility needed

---

## Future Considerations

### Potential API Evolution

**Gemini 4.x Speculation**:
- Possible unified API (budget + level hybrid)
- Dynamic level adjustment based on complexity
- Cross-model thinking sharing
- Real-time budget consumption tracking

**Claude Evolution**:
- Potential level-based presets
- Cost optimization hints
- Thinking quality feedback

---

## References

### Official Documentation

**Anthropic**:
- Claude API Documentation: https://docs.anthropic.com/claude/docs/thinking
- Thinking Guide: https://docs.anthropic.com/claude/docs/extended-thinking

**Google Gemini 3.x**:
- Thinking API: https://ai.google.dev/gemini-api/docs/thinking
- Gemini 3 Flash: https://blog.google/products/gemini/gemini-3-flash/

**Google Gemini 2.5**:
- Vertex AI Thinking: https://docs.cloud.google.com/vertex-ai/generative-ai/docs/thinking

### Internal Documentation

- `docs/technical-specs/GEMINI_API_ANALYSIS.md`
- Epic-011 implementation stories
- `docs/antigravity/workflows/models/gemini/gemini-3-api-breaking-change-analysis.md`

---

**Document Status**: ✅ Complete
**Last Updated**: 2026-01-11 (Epic-011 Story-006)
**Next Review**: Gemini 4.x API release
