# Product Owner Prompt: Claude 4.5 Sonnet - Compliance Implementation

**Дата**: 2026-01-10
**Модель**: claude-4.5-sonnet (Standard, No Thinking)
**Статус**: Ready for Epic/Story Creation

---

## Контекст проекта

Детальный анализ текущей реализации API Proxy для модели **claude-4.5-sonnet** (WITHOUT thinking mode) и сравнение с оригинальным workflow из **Google Antigravity v1.13.3**.

**Цель**: Достичь 100% compliance с оригинальным Antigravity.

**Текущий compliance score**: ~75-80%

**Key Difference from Thinking Version**: Проще и быстрее (5 gaps vs 8, 9 hours vs 17.5 hours), большинство изменений shared.

---

## Документация

### Reverse Engineering (Ожидаемое поведение)
**Файл**: `docs/antigravity/workflows/models/claude/claude-4-5-sonnet-workflow.md`

Workflow для claude-4.5-sonnet из Google Antigravity v1.13.3:
- Model ID: 333 (not 334)
- API Provider: ANTHROPIC_VERTEX (26)
- No thinking mode
- Standard code generation

### Current Implementation
**Файл**: `docs/comparison/claude/claude-4-5-sonnet/current-implementation.md`

Current implementation (700+ lines):
- Request/response flow
- Tool use implementation
- Error handling
- Known issues (6 identified)

### Comparison Analysis
**Файл**: `docs/comparison/claude/claude-4-5-sonnet/current-implementation.md` (section "⚖️ Detailed Comparison")

Detailed comparison:
- 5 gaps (vs 8 for thinking)
- Implementation can be shared with thinking version
- Simpler (no thinking blocks, no signatures)

---

## Ключевые находки

### Critical Issues (P0) - 2 gaps, 5 часов

#### 1. Model Provider Information (3 часа) **[SHARED]**
**Detection Risk**: HIGH

**Missing**: `modelId: 333`, `apiProvider: 26`, `modelProvider: 3`

**Impact**: Requests не содержат provider routing информацию

**Note**: **Shared implementation with claude-4.5-sonnet-thinking** - same code, different model ID (333 vs 334)

**Reference**: Comparison doc, section 1

---

#### 2. Request Metadata - ideType: ANTIGRAVITY (2 часа) **[SHARED]**
**Detection Risk**: CRITICAL

**Missing**: `ideType: "ANTIGRAVITY"`, `ideVersion`, `platform`, `architecture`

**Impact**: PRIMARY anti-detection marker отсутствует

**Note**: **Shared implementation with claude-4.5-sonnet-thinking**

**Reference**: Comparison doc, section 2

---

### Medium Priority (P2) - 2 gaps, 3 часа

#### 3. Tool Configuration Modes (2 часа) **[SHARED]**
**Current**: Hardcoded `mode: "VALIDATED"`

**Missing**: Support for AUTO, ANY, NONE modes

**Note**: **Shared implementation with thinking version**

**Reference**: Comparison doc, section 3

---

#### 4. Grounding Configuration (1 час) **[SHARED]**
**Missing**: `geminiSettings`, `recitationPolicy`

**Impact**: Missing anti-plagiarism protection

**Note**: **Shared implementation with thinking version**

**Reference**: Comparison doc, section 4

---

### Low Priority (P3) - 1 gap, 1 час

#### 5. Extended Session Metadata (1 час) **[SHARED]**
**Missing**: `workspace_id`, `cloudaicompanion_project`

**Note**: **Shared implementation with thinking version**

**Reference**: Comparison doc, section 5

---

## Implementation Roadmap

### Phase 1: Critical Compliance (P0)
**Duration**: 5 hours
**Goal**: Anti-detection compliance

**Stories**:
1. Model Provider Information - modelId: 333 (3h) **[SHARED]**
2. Request Metadata - ideType: ANTIGRAVITY (2h) **[SHARED]**

**Success Criteria**:
- All requests include modelId: 333, apiProvider: 26, modelProvider: 3
- ideType: ANTIGRAVITY present in metadata
- Zero detection failures

**Note**: Implement together with thinking version to share code

---

### Phase 2: Feature Parity (P2)
**Duration**: 3 hours

**Stories**:
1. Flexible Tool Configuration Modes (2h) **[SHARED]**
2. Grounding Configuration (1h) **[SHARED]**

**Success Criteria**:
- All tool modes supported
- Recitation policy configured

---

### Phase 3: Enhancement (P3)
**Duration**: 1 hour

**Stories**:
1. Extended Session Metadata (1h) **[SHARED]**

**Success Criteria**:
- workspace_id and project fields supported

---

## Требования к Эпикам и Историям

### Epic Structure

**Title**: `[EPIC] Claude 4.5 Sonnet - [Phase Name] Compliance`

**Description Template**:
```
**Goal**: [Phase goal]
**Priority**: P0/P2/P3
**Duration**: [hours]
**Model**: claude-4.5-sonnet (NO thinking)
**Shared with**: claude-4.5-sonnet-thinking

**Stories**:
- [List]

**Success Criteria**:
[From roadmap]

**Implementation Note**:
Most changes shared with claude-4.5-sonnet-thinking.
Implement together to maximize code reuse.

**References**:
- RE Doc: docs/antigravity/workflows/models/claude/claude-4.5-sonnet-workflow.md
- Current Implementation: docs/comparison/claude/claude-4-5-sonnet/current-implementation.md
- Comparison: docs/comparison/claude/claude-4-5-sonnet/current-implementation.md#detailed-comparison
```

---

### User Story Structure

**Title Format**: `[P1-CRITICAL-XX] [Story Name] (claude-4.5-sonnet)`

**Description**:
```
**Model**: claude-4.5-sonnet (NO thinking mode)
**Shared Implementation**: Yes/No
**If Shared**: Reference to thinking version story

[Standard user story format]

**Differences from Thinking Version**:
- Model ID: 333 (not 334)
- No thinkingConfig in request
- No thinking block processing
- Simpler implementation

[Rest of story follows claude-4.5-sonnet-thinking template]
```

---

## Example Story: Model Provider Information

**Title**: `[P1-CRITICAL-01] Add Model Provider Information (claude-4.5-sonnet)`

**Model**: claude-4.5-sonnet (NO thinking)
**Shared with**: claude-4.5-sonnet-thinking
**Implementation**: Same code, different model ID constant

**As a**: API Proxy developer
**I want**: Add modelId: 333, apiProvider: 26, modelProvider: 3 to requests
**So that**: Requests are properly routed through Vertex AI

**Key Implementation Detail**:
```rust
fn get_model_id(model_name: &str) -> u32 {
    match model_name {
        "claude-4.5-sonnet" => 333,              // This model
        "claude-4.5-sonnet-thinking" => 334,     // Thinking version
        _ => 0
    }
}
```

**Acceptance Criteria**:
- [ ] `modelId: 333` in all claude-4.5-sonnet requests
- [ ] `apiProvider: 26` in all Claude requests
- [ ] `modelProvider: 3` in all Claude requests
- [ ] Tests verify correct model ID (333, not 334)

**Testing**:
```rust
#[test]
fn test_model_id_sonnet() {
    let req = build_test_request("claude-4.5-sonnet");
    let body = transform_claude_request_in(&req, "project-id").unwrap();

    assert_eq!(body["modelId"], 333);  // Verify NOT 334
}
```

**Effort**: 3 hours (shared with thinking version)
**Story Points**: 3
**Priority**: P0 - CRITICAL

**For complete story template**, see:
`docs/comparison/claude/claude-4-5-sonnet-thinking/product-owner-prompt.md` (Story 1)

---

## Implementation Strategy

### Shared Implementation Benefits

**Why Implement Together**:
1. **Code Reuse**: 90% of code is identical
2. **Efficiency**: 9 hours total instead of 9+9=18 hours
3. **Consistency**: Same patterns across models
4. **Testing**: Shared test infrastructure

**Implementation Approach**:
1. Write helper functions that work for both models
2. Use model name matching for model-specific values
3. Share validation logic
4. Duplicate only model-specific constants

### Example Shared Code

```rust
// Shared constants
const API_PROVIDER_ANTHROPIC_VERTEX: u32 = 26;
const MODEL_PROVIDER_ANTHROPIC: u32 = 3;

// Model-specific constants
const CLAUDE_SONNET_MODEL_ID: u32 = 333;
const CLAUDE_SONNET_THINKING_MODEL_ID: u32 = 334;

// Shared function with model-specific logic
fn get_model_id(model_name: &str) -> u32 {
    match model_name {
        "claude-4.5-sonnet" => CLAUDE_SONNET_MODEL_ID,
        "claude-4.5-sonnet-thinking" => CLAUDE_SONNET_THINKING_MODEL_ID,
        _ => 0
    }
}

// Completely shared function
fn get_api_provider(model_name: &str) -> u32 {
    if model_name.starts_with("claude-") {
        API_PROVIDER_ANTHROPIC_VERTEX
    } else {
        0
    }
}
```

---

## Testing Requirements

### Shared Tests

**Most tests are shared** between sonnet and sonnet-thinking:
- Provider info tests (different model ID assertion)
- Metadata tests (identical)
- Tool config tests (identical)
- Grounding tests (identical)
- Session tests (identical)

### Model-Specific Tests

**Only test thinking-specific features separately**:
- claude-4.5-sonnet: Verify NO thinkingConfig
- claude-4.5-sonnet-thinking: Verify thinkingConfig present

---

## Success Metrics

**Phase 1 Completion**:
- Compliance: 75% → 90%
- Zero detection failures
- All P0 stories deployed

**All Phases Completion**:
- Compliance: 75% → 100%
- Feature parity with Antigravity
- Shared codebase with thinking version

---

## Следующие шаги для Product Owner

### Option A: Separate Epics (Recommended for clarity)

**Create 2 sets of epics**:
1. Claude 4.5 Sonnet (this model)
2. Claude 4.5 Sonnet Thinking

**Tag stories as**: `[SHARED]` or `[MODEL-SPECIFIC]`

**Advantages**:
- Clear separation by model
- Easier tracking
- Flexibility in priorities

### Option B: Combined Epics (Recommended for efficiency)

**Create 1 set of epics** covering both models:
- Epic title: "Claude 4.5 Sonnet Family - Phase 1"
- Stories mention both models
- Implementation done once

**Advantages**:
- Less duplication
- Faster implementation
- Natural code sharing

### Recommended: Option A with [SHARED] Tags

Most clarity + efficient implementation.

---

## Quick Reference

| Aspect | claude-4.5-sonnet | claude-4.5-sonnet-thinking |
|--------|-------------------|----------------------------|
| **Model ID** | 333 | 334 |
| **Thinking Mode** | ❌ No | ✅ Yes |
| **Total Gaps** | 5 | 8 |
| **Total Effort** | 9 hours | 17.5 hours |
| **Shared Changes** | ~90% | ~90% |
| **Stories** | 5 | 8 |

---

**Total Estimated Effort**:
- Separate: 9 hours (this model) + 17.5 hours (thinking) = 26.5 hours
- Combined (shared implementation): ~18-20 hours (30% savings)

**Recommendation**: Implement together, create separate tracking epics/stories

**Questions?** See comparison doc or thinking version prompt for detailed examples.
