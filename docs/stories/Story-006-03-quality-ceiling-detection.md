# Story-006-03: Quality Ceiling Detection - Smart Model Upgrade Recommendations

**Story ID**: Story-006-03
**Epic**: [Epic-006](../epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md) - Gemini 2.5 Flash Lite Thinking - Optimizations & Intelligence
**Priority**: P3 (Low) - Quality Optimization
**Estimated Effort**: 3 hours
**Type**: CODE (Backend - Quality Monitor)
**Status**: To Do
**Created**: 2026-01-11
**Owner**: Backend Dev

**Sequential Position**: Story #3 of 6 (Smart Features Phase)
**Depends On**: Story-006-01 (Live API Validation) - BLOCKED UNTIL COMPLETE
**Blocks**: Story-006-06 (Documentation) - Needs feature to document
**Can Run Parallel**: Story-006-02 (Adaptive Budget) - Independent feature

---

## User Story

**As a** quality-focused API user using gemini-2.5-flash-lite-thinking
**I want** automatic detection when I hit the model's quality ceiling
**So that** I know when to upgrade to gemini-2.5-flash-thinking for better results

---

## Context

### Current Situation

**No Quality Ceiling Awareness** (Suboptimal Model Usage):

```yaml
current_problem:
  scenario: "User requests gemini-2.5-flash-lite-thinking with budget=24576 (max)"
  expectation: "Maximum possible quality from lite model"
  reality: "Quality plateaus at ~1.4 level, won't improve further"
  missed_opportunity: "Flash base (no thinking) provides 1.5 quality"

quality_levels_explained:
  lite_base_no_thinking:
    model: "gemini-2.5-flash-lite"
    thinking_budget: 0
    quality_level: "1.0 (baseline)"
    cost: "Lowest"

  lite_max_thinking:
    model: "gemini-2.5-flash-lite-thinking"
    thinking_budget: 24576  # Maximum allowed
    quality_level: "1.4 (ceiling)"
    cost: "Medium"
    limitation: "Cannot improve beyond 1.4 even with more budget"

  flash_base_no_thinking:
    model: "gemini-2.5-flash-thinking"
    thinking_budget: 0
    quality_level: "1.5 (better than lite max)"
    cost: "Medium-High"
    advantage: "Better quality WITHOUT thinking overhead"

  flash_with_thinking:
    model: "gemini-2.5-flash-thinking"
    thinking_budget: "1-24576"
    quality_level: "1.5-2.0+"
    cost: "High"
    advantage: "Highest possible quality"

ceiling_problem:
  user_confusion: "Why isn't quality improving with more thinking budget?"
  wasted_resources: "Using 24576 budget when flash base (0 budget) is better"
  no_guidance: "System doesn't inform user about better options"
```

**Example Inefficiency**:
```json
// User maxes out lite thinking, but doesn't realize ceiling hit
{
  "model": "gemini-2.5-flash-lite-thinking",
  "messages": [{"role": "user", "content": "Complex algorithm design task"}],
  "thinking": {"type": "enabled", "budget_tokens": 24576}
}
// Response quality: 1.4 (ceiling)
// Better alternative: gemini-2.5-flash-thinking with budget=0 → quality 1.5
// User could get better quality for LESS cost (no thinking tokens)
```

**User Impact**:
- Users don't know when they've hit quality ceiling
- Continue increasing budget thinking it will help
- Miss opportunity to upgrade to better model
- Suboptimal quality-to-cost ratio

### Expected Behavior After Story

**Intelligent Quality Ceiling Detection** (Proactive Guidance):

```yaml
ceiling_detection_system:
  monitors:
    - Thinking budget approaching maximum (≥20000)
    - Response quality score (if available from model)
    - User dissatisfaction signals (repeated requests, low feedback)

  detection_triggers:
    trigger_1_high_budget:
      condition: "budget ≥ 20000 (approaching 24576 max)"
      reasoning: "User requesting near-maximum thinking"
      action: "Analyze if quality ceiling likely reached"

    trigger_2_quality_plateau:
      condition: "Quality score < expected for budget"
      reasoning: "Model not improving proportionally to budget"
      action: "Flag potential ceiling hit"

    trigger_3_repeated_requests:
      condition: "Same/similar prompt with increasing budgets"
      reasoning: "User trying to improve quality"
      action: "Recommend model upgrade"

  recommendation_system:
    when_ceiling_detected:
      response_header: "X-Quality-Ceiling-Detected: true"
      upgrade_suggestion: "X-Recommended-Model: gemini-2.5-flash-thinking"
      explanation: "X-Ceiling-Reason: lite-max-thinking-reached"

    guidance_message:
      en: "Quality ceiling reached for lite model. Consider upgrading to gemini-2.5-flash-thinking for better results."
      zh: "Lite 模型已达质量上限。建议升级到 gemini-2.5-flash-thinking 以获得更好效果。"

automatic_actions:
  log_ceiling_event:
    level: "INFO"
    message: "Quality ceiling detected for gemini-2.5-flash-lite-thinking"
    data:
      budget: 24576
      estimated_quality: "1.4 (ceiling)"
      recommended_model: "gemini-2.5-flash-thinking"
      estimated_improvement: "+0.1 to +0.6 quality levels"

  telemetry_tracking:
    metric: "quality_ceiling_detections"
    dimensions:
      - budget_level
      - task_complexity
      - upgrade_acceptance_rate

user_control_preserved:
  recommendation_only: "Never force model change"
  user_decides: "User chooses whether to upgrade"
  transparency: "Clear explanation of why recommendation made"
```

**Example with Detection**:
```bash
# Request with high budget
curl -X POST /v1/messages \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemini-2.5-flash-lite-thinking",
    "messages": [{"role": "user", "content": "Complex task..."}],
    "thinking": {"type": "enabled", "budget_tokens": 24000}
  }'

# Response includes ceiling detection headers
HTTP/1.1 200 OK
X-Quality-Ceiling-Detected: true
X-Recommended-Model: gemini-2.5-flash-thinking
X-Ceiling-Reason: lite-max-thinking-approaching
X-Estimated-Improvement: +0.1-to-0.6-quality-levels
Content-Type: application/json

{
  "model": "gemini-2.5-flash-lite-thinking",
  "content": [...],
  "usage": {
    "thinking_tokens": 22000,
    ...
  }
}

# User sees headers, decides to retry with flash thinking
# Next request:
{
  "model": "gemini-2.5-flash-thinking",  // Upgraded
  "thinking": {"budget_tokens": 5000}    // Less thinking needed
}
// Quality: 1.6 (better than lite's 1.4 ceiling)
// Cost: Lower (5000 thinking vs 24000)
```

### Gap Analysis

**Source**: gemini-2.5-flash-lite-thinking-COMPARISON.md (P3 Gap #2)

```yaml
gap: "No automatic quality ceiling detection or upgrade guidance"
priority: P3
current_state:
  ceiling_awareness: "None - users don't know ceiling exists"
  upgrade_guidance: "Manual - users must discover better models"
  quality_feedback: "No quality-to-cost optimization hints"

target_state:
  ceiling_awareness: "Automatic detection when approaching/at ceiling"
  upgrade_guidance: "Proactive recommendations via response headers"
  quality_feedback: "Clear explanation of quality limits"

why_valuable:
  better_model_utilization: "Users choose optimal model for their needs"
  quality_optimization: "Achieve best quality-to-cost ratio"
  user_education: "Learn about model hierarchy and limitations"
  cost_optimization: "Avoid wasting budget on ceiling-limited model"

effort: "MEDIUM (3 hours)"
priority_rationale: "P3 (enhancement), but high value for quality-focused users"
```

**Quality Hierarchy**:
```yaml
model_tier_understanding:
  tier_1_lite:
    quality_range: "1.0 - 1.4"
    ceiling: "1.4 (with 24576 thinking)"
    best_for: "Simple to moderate tasks, cost-sensitive"

  tier_2_flash:
    quality_range: "1.5 - 2.0"
    ceiling: "2.0 (with 24576 thinking)"
    best_for: "Complex tasks, quality-focused"

  tier_3_pro:
    quality_range: "2.0 - 3.0"
    ceiling: "3.0 (with 32000 thinking)"
    best_for: "Critical tasks, maximum quality required"

upgrade_paths:
  from_lite_ceiling:
    current: "gemini-2.5-flash-lite-thinking @ 24576 budget"
    recommended: "gemini-2.5-flash-thinking @ 0-10000 budget"
    benefit: "+0.1 to +0.6 quality, potentially lower cost"

  from_flash_ceiling:
    current: "gemini-2.5-flash-thinking @ 24576 budget"
    recommended: "gemini-3-pro-high-thinking @ 0-15000 budget"
    benefit: "+0.5 to +1.0 quality, higher thinking budget available"
```

---

## Reference Documentation

### Primary Sources

1. **Epic-006 Document**: `docs/epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md`
   - Story-006-03 definition (lines 110-127)
   - Quality ceiling concept (lines 439-462)

2. **COMPARISON Document**: `docs/antigravity/workflows/models/gemini/gemini-2.5-flash-lite-thinking-COMPARISON.md`
   - P3 Gap #2: Quality Ceiling Detection (lines TBD)

3. **Model Hierarchy**: `docs/comparison/MASTER-MODELS-TABLE.md`
   - Gemini 2.5 model family
   - Thinking budget limits by model

### Code References

**Quality Monitoring Points**:
- `src-tauri/src/proxy/mappers/claude/request.rs:1412-1514` - Thinking config (budget detection)
- `src-tauri/src/proxy/mappers/claude/response.rs` - Response processing (quality assessment)
- `src-tauri/src/proxy/handlers/claude.rs` - Response headers (add ceiling detection headers)

**Integration Points**:
- Budget detection (existing - Story-006-01 validated 24576 limit)
- Complexity classification (Story-006-02 - reuse for context)
- Telemetry system (Story-006-02 - extend for ceiling tracking)

### Related Concepts

**Similar Intelligent Features**:
- Auto-stream conversion (proactive optimization)
- Rate limit retry with account upgrade (intelligent fallback)
- Adaptive budget adjustment (Story-006-02 - smart resource allocation)

---

## Technical Details

### Implementation Architecture

**Three-Component System**:

1. **Ceiling Detector**: Analyze request/response to identify ceiling scenarios
2. **Recommendation Engine**: Generate upgrade suggestions based on ceiling detection
3. **Header Injector**: Add ceiling detection headers to response

### Component 1: Ceiling Detector (1.5 hours)

**Objective**: Detect when user is approaching or at quality ceiling

**Detection Algorithm**:

```rust
// src-tauri/src/proxy/quality/ceiling_detector.rs (NEW FILE)

use crate::models::{ClaudeRequest, ClaudeResponse};
use crate::proxy::mappers::claude::complexity_classifier::ComplexityLevel;

/// Quality ceiling detection result
#[derive(Debug, Clone)]
pub struct CeilingDetection {
    pub ceiling_detected: bool,
    pub reason: CeilingReason,
    pub recommended_model: Option<String>,
    pub estimated_improvement: Option<QualityImprovement>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CeilingReason {
    LiteMaxThinkingApproaching,  // Budget ≥20000, approaching 24576 max
    LiteMaxThinkingReached,      // Budget = 24576, at absolute max
    QualityPlateauDetected,      // Quality not improving proportionally
    None,                        // No ceiling detected
}

#[derive(Debug, Clone)]
pub struct QualityImprovement {
    pub min_gain: f32,  // e.g., 0.1
    pub max_gain: f32,  // e.g., 0.6
}

/// Quality ceiling detector
pub struct CeilingDetector;

impl CeilingDetector {
    /// Detect if request/response indicates quality ceiling
    pub fn detect(
        request: &ClaudeRequest,
        response: Option<&ClaudeResponse>,
        complexity: ComplexityLevel,
    ) -> CeilingDetection {
        // Only applies to gemini-2.5-flash-lite-thinking
        if !request.model.contains("gemini-2.5-flash-lite-thinking") {
            return CeilingDetection {
                ceiling_detected: false,
                reason: CeilingReason::None,
                recommended_model: None,
                estimated_improvement: None,
            };
        }

        // Check if high thinking budget requested
        let thinking_budget = request
            .thinking
            .as_ref()
            .and_then(|t| t.budget_tokens)
            .unwrap_or(0);

        // Detection heuristic
        let ceiling_reason = if thinking_budget >= 24576 {
            // At absolute maximum
            CeilingReason::LiteMaxThinkingReached
        } else if thinking_budget >= 20000 {
            // Approaching maximum (80%+ of 24576)
            CeilingReason::LiteMaxThinkingApproaching
        } else if thinking_budget >= 16000 && complexity == ComplexityLevel::Deep {
            // Deep complexity with high budget - likely needs more than lite offers
            CeilingReason::LiteMaxThinkingApproaching
        } else {
            CeilingReason::None
        };

        if ceiling_reason == CeilingReason::None {
            return CeilingDetection {
                ceiling_detected: false,
                reason: CeilingReason::None,
                recommended_model: None,
                estimated_improvement: None,
            };
        }

        // Ceiling detected - generate recommendation
        CeilingDetection {
            ceiling_detected: true,
            reason: ceiling_reason.clone(),
            recommended_model: Some("gemini-2.5-flash-thinking".to_string()),
            estimated_improvement: Some(Self::estimate_improvement(
                thinking_budget,
                complexity,
            )),
        }
    }

    /// Estimate quality improvement if upgrading to flash thinking
    fn estimate_improvement(
        lite_budget: u32,
        complexity: ComplexityLevel,
    ) -> QualityImprovement {
        // Quality improvement depends on task complexity
        match complexity {
            ComplexityLevel::Simple => QualityImprovement {
                min_gain: 0.05,
                max_gain: 0.15,  // Simple tasks: minimal improvement
            },
            ComplexityLevel::Moderate => QualityImprovement {
                min_gain: 0.1,
                max_gain: 0.3,  // Moderate tasks: moderate improvement
            },
            ComplexityLevel::Complex => QualityImprovement {
                min_gain: 0.2,
                max_gain: 0.5,  // Complex tasks: significant improvement
            },
            ComplexityLevel::Deep => QualityImprovement {
                min_gain: 0.3,
                max_gain: 0.7,  // Deep tasks: major improvement
            },
        }
    }

    /// Get recommended thinking budget for upgraded model
    pub fn recommended_flash_budget(
        lite_budget: u32,
        complexity: ComplexityLevel,
    ) -> u32 {
        // Flash thinking can achieve lite's max quality with less thinking
        // Recommend 30-50% of lite budget, minimum 2000
        let recommended = (lite_budget as f32 * 0.4) as u32;
        recommended.max(2000).min(15000)
    }
}
```

**Detection Logic**:
```yaml
scenario_1_max_budget:
  condition: "budget = 24576"
  reason: "LiteMaxThinkingReached"
  confidence: "100% - at absolute ceiling"
  action: "Strong recommendation to upgrade"

scenario_2_approaching_max:
  condition: "budget ≥ 20000 (80%+ of max)"
  reason: "LiteMaxThinkingApproaching"
  confidence: "90% - very likely near ceiling"
  action: "Moderate recommendation to upgrade"

scenario_3_deep_complexity:
  condition: "budget ≥ 16000 AND complexity = Deep"
  reason: "LiteMaxThinkingApproaching"
  confidence: "80% - deep task needs more than lite offers"
  action: "Moderate recommendation to upgrade"

scenario_4_below_thresholds:
  condition: "budget < 16000 OR complexity < Deep"
  reason: "None"
  confidence: "N/A - not near ceiling"
  action: "No recommendation"
```

---

### Component 2: Recommendation Engine (1 hour)

**Objective**: Generate actionable upgrade recommendations

**Implementation**:

```rust
// Add to ceiling_detector.rs

impl CeilingDetector {
    /// Generate user-facing recommendation message
    pub fn generate_recommendation(
        detection: &CeilingDetection,
        locale: &str,  // "en" or "zh"
    ) -> Option<String> {
        if !detection.ceiling_detected {
            return None;
        }

        let recommended_model = detection.recommended_model.as_ref()?;

        let improvement_str = if let Some(improvement) = &detection.estimated_improvement {
            format!(
                "{:.1}-{:.1}",
                improvement.min_gain, improvement.max_gain
            )
        } else {
            "0.1-0.6".to_string()
        };

        let message = match locale {
            "zh" => format!(
                "检测到 Lite 模型质量上限。建议升级到 {} 以提升 {} 个质量级别。",
                recommended_model, improvement_str
            ),
            _ => format!(
                "Quality ceiling detected for Lite model. Consider upgrading to {} for {improvement_str} quality level improvement.",
                recommended_model
            ),
        };

        Some(message)
    }

    /// Generate response headers for ceiling detection
    pub fn generate_headers(detection: &CeilingDetection) -> Vec<(String, String)> {
        if !detection.ceiling_detected {
            return vec![];
        }

        let mut headers = vec![
            (
                "X-Quality-Ceiling-Detected".to_string(),
                "true".to_string(),
            ),
            (
                "X-Ceiling-Reason".to_string(),
                format!("{:?}", detection.reason),
            ),
        ];

        if let Some(model) = &detection.recommended_model {
            headers.push((
                "X-Recommended-Model".to_string(),
                model.clone(),
            ));
        }

        if let Some(improvement) = &detection.estimated_improvement {
            headers.push((
                "X-Estimated-Improvement".to_string(),
                format!("{:.1}-to-{:.1}-quality-levels", improvement.min_gain, improvement.max_gain),
            ));
        }

        headers
    }
}
```

**Recommendation Guidelines**:
```yaml
recommendation_strength:
  strong:
    trigger: "budget = 24576 (absolute max)"
    message: "Quality ceiling REACHED. Upgrading to flash-thinking HIGHLY RECOMMENDED."
    header: "X-Recommendation-Strength: high"

  moderate:
    trigger: "budget ≥ 20000 OR (budget ≥ 16000 AND deep complexity)"
    message: "Approaching quality ceiling. Consider upgrading to flash-thinking for better results."
    header: "X-Recommendation-Strength: moderate"

  informational:
    trigger: "budget ≥ 12000 AND complex/deep"
    message: "FYI: flash-thinking available if higher quality needed."
    header: "X-Recommendation-Strength: info"
```

---

### Component 3: Header Injector (30 minutes)

**Objective**: Add ceiling detection headers to HTTP response

**File**: `src-tauri/src/proxy/handlers/claude.rs`

**Integration Point**: After response received, before returning to client

**Code Changes**:

```rust
// src-tauri/src/proxy/handlers/claude.rs

use crate::proxy::quality::ceiling_detector::CeilingDetector;

pub async fn handle_claude_request(
    req: ClaudeRequest,
    config: RequestConfig,
) -> Result<(ClaudeResponse, HeaderMap), ProxyError> {
    // ... existing request processing ...

    // Transform request
    let (upstream_body, violations) = transform_claude_request_in(&req, &project).await?;

    // Execute upstream request
    let response = upstream_client.execute(upstream_body).await?;

    // Transform response
    let claude_response = transform_claude_response(&response)?;

    // 🆕 Detect quality ceiling (Story-006-03)
    let complexity = ComplexityClassifier::classify(&req.messages);
    let ceiling_detection = CeilingDetector::detect(&req, Some(&claude_response), complexity);

    // 🆕 Generate response headers
    let mut response_headers = HeaderMap::new();

    // Add standard headers
    response_headers.insert("Content-Type", "application/json".parse().unwrap());

    // 🆕 Add ceiling detection headers if detected
    if ceiling_detection.ceiling_detected {
        for (key, value) in CeilingDetector::generate_headers(&ceiling_detection) {
            response_headers.insert(
                HeaderName::from_bytes(key.as_bytes()).unwrap(),
                HeaderValue::from_str(&value).unwrap(),
            );
        }

        // 🆕 Log ceiling event
        tracing::info!(
            "Quality ceiling detected: model={}, budget={:?}, reason={:?}, recommended={}",
            req.model,
            req.thinking.as_ref().and_then(|t| t.budget_tokens),
            ceiling_detection.reason,
            ceiling_detection.recommended_model.as_deref().unwrap_or("N/A")
        );

        // 🆕 Telemetry: Track ceiling detection
        telemetry::record_quality_ceiling_detection(
            &project,
            &req.model,
            ceiling_detection.reason,
            ceiling_detection.recommended_model.as_deref(),
        );
    }

    Ok((claude_response, response_headers))
}
```

**Header Examples**:
```yaml
no_ceiling_detected:
  headers:
    Content-Type: "application/json"
  # No X-Quality-Ceiling-* headers

ceiling_approaching:
  headers:
    Content-Type: "application/json"
    X-Quality-Ceiling-Detected: "true"
    X-Ceiling-Reason: "LiteMaxThinkingApproaching"
    X-Recommended-Model: "gemini-2.5-flash-thinking"
    X-Estimated-Improvement: "0.2-to-0.5-quality-levels"
    X-Recommendation-Strength: "moderate"

ceiling_reached:
  headers:
    Content-Type: "application/json"
    X-Quality-Ceiling-Detected: "true"
    X-Ceiling-Reason: "LiteMaxThinkingReached"
    X-Recommended-Model: "gemini-2.5-flash-thinking"
    X-Estimated-Improvement: "0.3-to-0.7-quality-levels"
    X-Recommendation-Strength: "high"
```

---

## Acceptance Criteria

### AC-1: Ceiling Detector Implemented

**Requirement**: Detect quality ceiling scenarios with ≥90% accuracy

**Verification**:
```yaml
test_method: "Unit tests with ceiling scenarios"

test_scenarios:
  scenario_1_max_budget:
    request:
      model: "gemini-2.5-flash-lite-thinking"
      thinking_budget: 24576
      complexity: "Complex"
    expected:
      ceiling_detected: true
      reason: "LiteMaxThinkingReached"
      confidence: "100%"

  scenario_2_approaching_max:
    request:
      model: "gemini-2.5-flash-lite-thinking"
      thinking_budget: 22000
      complexity: "Moderate"
    expected:
      ceiling_detected: true
      reason: "LiteMaxThinkingApproaching"
      confidence: "90%"

  scenario_3_deep_complexity:
    request:
      model: "gemini-2.5-flash-lite-thinking"
      thinking_budget: 18000
      complexity: "Deep"
    expected:
      ceiling_detected: true
      reason: "LiteMaxThinkingApproaching"
      confidence: "80%"

  scenario_4_below_threshold:
    request:
      model: "gemini-2.5-flash-lite-thinking"
      thinking_budget: 10000
      complexity: "Moderate"
    expected:
      ceiling_detected: false
      reason: "None"

  scenario_5_different_model:
    request:
      model: "gemini-2.5-flash-thinking"
      thinking_budget: 24576
      complexity: "Complex"
    expected:
      ceiling_detected: false  # Flash has higher ceiling
      reason: "None"

accuracy_target: "≥90% (9/10 scenarios correct)"
```

**Pass Criteria**:
- ✅ Ceiling detected for scenarios 1-3 (max, approaching, deep complexity)
- ✅ No false positives for scenarios 4-5 (below threshold, different model)
- ✅ Correct reason classification
- ✅ Appropriate confidence levels

---

### AC-2: Upgrade Recommendations Generated

**Requirement**: Actionable upgrade recommendations with quality improvement estimates

**Verification**:
```yaml
test_method: "Unit tests for recommendation generation"

test_cases:
  case_1_simple_task:
    input:
      ceiling_detected: true
      complexity: "Simple"
      lite_budget: 24576
    expected:
      recommended_model: "gemini-2.5-flash-thinking"
      recommended_budget: ~9830  # 40% of 24576
      estimated_improvement: "0.05-0.15"

  case_2_moderate_task:
    input:
      ceiling_detected: true
      complexity: "Moderate"
      lite_budget: 22000
    expected:
      recommended_model: "gemini-2.5-flash-thinking"
      recommended_budget: ~8800  # 40% of 22000
      estimated_improvement: "0.1-0.3"

  case_3_complex_task:
    input:
      ceiling_detected: true
      complexity: "Complex"
      lite_budget: 20000
    expected:
      recommended_model: "gemini-2.5-flash-thinking"
      recommended_budget: ~8000  # 40% of 20000
      estimated_improvement: "0.2-0.5"

  case_4_deep_task:
    input:
      ceiling_detected: true
      complexity: "Deep"
      lite_budget: 18000
    expected:
      recommended_model: "gemini-2.5-flash-thinking"
      recommended_budget: ~7200  # 40% of 18000
      estimated_improvement: "0.3-0.7"

recommendation_message:
  en: "Quality ceiling detected for Lite model. Consider upgrading to gemini-2.5-flash-thinking for {improvement} quality level improvement."
  zh: "检测到 Lite 模型质量上限。建议升级到 gemini-2.5-flash-thinking 以提升 {improvement} 个质量级别。"
```

**Pass Criteria**:
- ✅ Correct model recommendation (gemini-2.5-flash-thinking)
- ✅ Reasonable budget recommendation (30-50% of lite budget)
- ✅ Appropriate quality improvement estimate by complexity
- ✅ Clear, actionable recommendation messages (EN + ZH)

---

### AC-3: Response Headers Added

**Requirement**: Ceiling detection headers present in HTTP response when ceiling detected

**Verification**:
```yaml
test_method: "Integration test - HTTP response inspection"

test_1_no_ceiling:
  request:
    model: "gemini-2.5-flash-lite-thinking"
    thinking_budget: 5000
  expected_headers:
    Content-Type: "application/json"
    # No X-Quality-Ceiling-* headers
  verification: "Ceiling headers absent"

test_2_ceiling_approaching:
  request:
    model: "gemini-2.5-flash-lite-thinking"
    thinking_budget: 22000
  expected_headers:
    Content-Type: "application/json"
    X-Quality-Ceiling-Detected: "true"
    X-Ceiling-Reason: "LiteMaxThinkingApproaching"
    X-Recommended-Model: "gemini-2.5-flash-thinking"
    X-Estimated-Improvement: "[0.1-0.7]-quality-levels"
  verification: "All ceiling headers present"

test_3_ceiling_reached:
  request:
    model: "gemini-2.5-flash-lite-thinking"
    thinking_budget: 24576
  expected_headers:
    Content-Type: "application/json"
    X-Quality-Ceiling-Detected: "true"
    X-Ceiling-Reason: "LiteMaxThinkingReached"
    X-Recommended-Model: "gemini-2.5-flash-thinking"
    X-Estimated-Improvement: "[0.1-0.7]-quality-levels"
  verification: "All ceiling headers present with max budget reason"
```

**Pass Criteria**:
- ✅ Headers present when ceiling detected
- ✅ Headers absent when no ceiling detected
- ✅ Correct header values (reason, model, improvement)
- ✅ Headers parseable by HTTP clients

---

### AC-4: Ceiling Events Logged

**Requirement**: Informative logging when ceiling detected

**Verification**:
```yaml
test_method: "Log inspection during integration tests"

expected_log_format:
  level: "INFO"
  message: "Quality ceiling detected: model={model}, budget={budget}, reason={reason}, recommended={recommended}"

example_log:
  timestamp: "2026-01-11T10:30:45Z"
  level: "INFO"
  message: "Quality ceiling detected: model=gemini-2.5-flash-lite-thinking, budget=24576, reason=LiteMaxThinkingReached, recommended=gemini-2.5-flash-thinking"
  metadata:
    project: "claude-code"
    request_id: "req_abc123"

log_validation:
  - Contains model name
  - Contains thinking budget value
  - Contains ceiling reason
  - Contains recommended upgrade model
  - Level is INFO (not DEBUG or WARN)
```

**Pass Criteria**:
- ✅ Ceiling events logged at INFO level
- ✅ Logs include all key information (model, budget, reason, recommendation)
- ✅ Logs searchable (e.g., grep "Quality ceiling detected")
- ✅ No false positive logs (only when ceiling actually detected)

---

### AC-5: Telemetry Tracking

**Requirement**: Ceiling detection events tracked for analytics

**Verification**:
```yaml
telemetry_metrics:
  metric_name: "quality_ceiling_detections"
  dimensions:
    - project_name
    - model_name
    - ceiling_reason
    - recommended_model
    - complexity_level

sample_telemetry_data:
  project: "claude-code"
  model: "gemini-2.5-flash-lite-thinking"
  ceiling_reason: "LiteMaxThinkingReached"
  recommended_model: "gemini-2.5-flash-thinking"
  complexity: "Deep"
  timestamp: "2026-01-11T10:30:45Z"

analytics_queries:
  ceiling_detection_rate:
    query: "SELECT COUNT(*) WHERE ceiling_detected = true GROUP BY model"
    purpose: "Track how often users hit ceiling"

  upgrade_acceptance_rate:
    query: "SELECT COUNT(*) WHERE recommended_model used in next request"
    purpose: "Measure recommendation effectiveness"

  complexity_distribution:
    query: "SELECT complexity, COUNT(*) WHERE ceiling_detected GROUP BY complexity"
    purpose: "Understand which tasks hit ceiling"
```

**Pass Criteria**:
- ✅ Telemetry module records ceiling detections
- ✅ All dimensions captured (project, model, reason, complexity)
- ✅ Data queryable via telemetry API
- ✅ Optional: Upgrade acceptance tracking (if user follows recommendation)

---

## Implementation Steps

### Phase 1: Ceiling Detector (1.5 hours)

**Step 1.1**: Create quality module directory
```bash
mkdir -p src-tauri/src/proxy/quality
touch src-tauri/src/proxy/quality/mod.rs
touch src-tauri/src/proxy/quality/ceiling_detector.rs
```

**Step 1.2**: Implement CeilingDetection structs
- `CeilingDetection`
- `CeilingReason` enum
- `QualityImprovement`

**Step 1.3**: Implement CeilingDetector
- `detect()` method (main detection logic)
- `estimate_improvement()` method
- `recommended_flash_budget()` method

**Step 1.4**: Write unit tests
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_detect_max_budget() { /* scenario 1 */ }

    #[test]
    fn test_detect_approaching_max() { /* scenario 2 */ }

    #[test]
    fn test_detect_deep_complexity() { /* scenario 3 */ }

    #[test]
    fn test_no_detection_below_threshold() { /* scenario 4 */ }

    #[test]
    fn test_no_detection_different_model() { /* scenario 5 */ }
}
```

---

### Phase 2: Recommendation Engine (1 hour)

**Step 2.1**: Implement recommendation generation
- `generate_recommendation()` method (EN + ZH messages)
- `generate_headers()` method

**Step 2.2**: Write recommendation tests
```rust
#[test]
fn test_recommendations_by_complexity() {
    // Test all 4 complexity levels
    // Verify improvement estimates
    // Verify budget recommendations
}
```

**Step 2.3**: Test header generation
```rust
#[test]
fn test_header_generation() {
    // Verify header keys and values
    // Test with/without ceiling detection
}
```

---

### Phase 3: Integration (30 minutes)

**Step 3.1**: Import ceiling detector in handlers/claude.rs
```rust
use crate::proxy::quality::ceiling_detector::CeilingDetector;
```

**Step 3.2**: Add ceiling detection call
- After response received
- Before returning to client
- Add headers to HTTP response

**Step 3.3**: Add logging
- INFO level for ceiling events
- Include all key information

**Step 3.4**: Add telemetry call
```rust
telemetry::record_quality_ceiling_detection(
    &project,
    &req.model,
    ceiling_detection.reason,
    ceiling_detection.recommended_model.as_deref(),
);
```

---

## Definition of Done

Story-006-03 is **COMPLETE** when:

### Code Requirements
- ✅ CeilingDetector implemented (ceiling_detector.rs)
- ✅ Detection algorithm with 3 trigger scenarios
- ✅ Recommendation engine (messages + headers)
- ✅ Integration in claude.rs handler
- ✅ Code formatted (`cargo fmt`) and linted (`cargo clippy`)

### Testing Requirements
- ✅ Unit tests: ≥8 tests (5 detection + 3 recommendation)
- ✅ Integration tests: 3 tests (no ceiling, approaching, reached)
- ✅ Detection accuracy: ≥90% (9/10 scenarios correct)
- ✅ All tests passing (`cargo test`)

### Logging Requirements
- ✅ Ceiling events logged at INFO level
- ✅ Logs include model, budget, reason, recommendation
- ✅ Searchable log format

### Telemetry Requirements
- ✅ Ceiling detections tracked
- ✅ All dimensions captured (project, model, reason, complexity)
- ✅ Data queryable via API (optional)

### Documentation Requirements
- ✅ Code comments explain detection logic
- ✅ Story status updated to "✅ IMPLEMENTED"
- ✅ User documentation (defer to Story-006-06)

---

## Dependencies

### Upstream Dependencies (Must Complete)
- ✅ Story-006-01: Live API Validation (confirms 24576 limit)

### Downstream Dependencies (Will Benefit)
- ⏳ Story-006-05: Quality Metrics Dashboard (displays ceiling detections)
- ⏳ Story-006-06: Documentation (documents ceiling concept)

### Parallel Work (Can Run Concurrently)
- ⏳ Story-006-02: Adaptive Budget (independent feature, shared complexity classifier)
- ⏳ Story-006-04: Budget Analytics Dashboard (independent dashboard)

---

## Risks & Mitigations

### Risk 1: False Positive Detection 🟡

**Risk**: System detects ceiling when user hasn't actually hit it

**Probability**: MEDIUM (25%)

**Impact**: LOW - Annoying recommendations, but not harmful

**Mitigation**:
```yaml
prevention:
  conservative_thresholds: "Budget ≥20000 (80%+ of max) before detecting"
  complexity_context: "Consider task complexity in detection"
  no_forced_changes: "Recommendations only, never force upgrade"

if_false_positives_>10%:
  action_1: "Raise detection threshold (20000 → 22000)"
  action_2: "Add complexity requirement (only Complex/Deep tasks)"
  action_3: "Make detection opt-in via config flag"
```

---

### Risk 2: Quality Improvement Estimates Inaccurate 🟡

**Risk**: Estimated improvement (e.g., +0.3-0.7) doesn't match reality

**Probability**: MEDIUM (40%)

**Impact**: LOW - User expectations vs reality mismatch

**Mitigation**:
```yaml
conservative_estimates:
  use_ranges: "0.1-0.7 (wide range to set expectations)"
  disclaimer: "Estimated improvement may vary by task"

if_estimates_consistently_wrong:
  action_1: "Widen ranges (e.g., 0.1-1.0)"
  action_2: "Add 'typical' vs 'maximum' improvement"
  action_3: "Remove specific numbers, use qualitative ('significant improvement')"
```

---

### Risk 3: Recommended Budget Suboptimal 🟡

**Risk**: Recommended flash budget (40% of lite budget) not ideal

**Probability**: MEDIUM (30%)

**Impact**: LOW - Suboptimal cost/quality, but not breaking

**Mitigation**:
```yaml
tuning_approach:
  start_conservative: "40% of lite budget (safe default)"
  monitor_telemetry: "Track actual flash usage after upgrade"
  adjust_percentage: "Tune 30-50% range based on data"

if_recommendations_consistently_off:
  action_1: "Adjust percentage (e.g., 40% → 50%)"
  action_2: "Make percentage complexity-dependent"
  action_3: "Remove specific budget recommendation, let user decide"
```

---

## Testing Strategy

### Test Pyramid

```yaml
unit_tests:
  count: 8
  focus:
    - Ceiling detection (5 scenarios)
    - Recommendation generation (3 complexity levels)
  coverage: "CeilingDetector methods"

integration_tests:
  count: 3
  focus:
    - No ceiling (budget <16000)
    - Ceiling approaching (budget 20000-24000)
    - Ceiling reached (budget 24576)
  coverage: "End-to-end header injection"

telemetry_tests:
  count: 2
  focus:
    - Ceiling detection recorded
    - Metrics queryable
  coverage: "Telemetry integration"
```

### Test Execution Plan

**Pre-Implementation**:
```bash
# Baseline: No ceiling detection tests exist
cargo test | grep ceiling  # Should find 0 tests
```

**Post-Implementation**:
```bash
# Run all ceiling detection tests
cargo test ceiling_detector -- --nocapture

# Expected: 13 tests passing
# - 8 unit tests (5 detection + 3 recommendation)
# - 3 integration tests
# - 2 telemetry tests
```

**Accuracy Validation**:
```bash
# Run detection accuracy test
cargo test test_ceiling_detection_accuracy -- --nocapture

# Expected output:
# Correct detections: 9/10 (90% accuracy)
# False positives: 0
# False negatives: 1
# Status: PASS (≥90% target)
```

---

## Success Metrics

### Detection Metrics

```yaml
detection_accuracy:
  target: "≥90%"
  actual: "TBD after testing"
  false_positive_rate: "<10%"
  false_negative_rate: "<10%"

ceiling_detection_rate:
  expected: "5-10% of lite-thinking requests"
  rationale: "Most users don't max out budget"
```

### Recommendation Metrics

```yaml
recommendation_quality:
  model_appropriateness: "100% (always flash-thinking)"
  budget_reasonableness: "≥80% (flash budget ≤ lite budget)"
  improvement_accuracy: "Within ±0.2 quality levels"

upgrade_acceptance_rate:
  target: "≥20% of recommendations followed"
  measurement: "User switches to recommended model in next request"
```

### User Experience Metrics

```yaml
clarity:
  header_comprehension: "≥90% developers understand headers"
  message_actionability: "≥85% know what to do after recommendation"

intrusiveness:
  annoyance_rate: "<5% (false positives rare)"
  value_perception: "≥70% find recommendations helpful"
```

---

## Documentation Updates

### Files to Create

**1. Ceiling Detector Module**: `src-tauri/src/proxy/quality/ceiling_detector.rs`
- CeilingDetection structs
- CeilingDetector implementation
- Detection and recommendation logic

**2. Unit Tests**: Add to ceiling_detector.rs
- 8 ceiling detection and recommendation tests

**3. Integration Tests**: Add to handlers/claude.rs tests
- 3 end-to-end header injection tests

### Files to Update

**1. Claude Handler**: `src-tauri/src/proxy/handlers/claude.rs`
- Import CeilingDetector
- Add ceiling detection call (after response, before return)
- Inject headers into HTTP response

**2. Telemetry Module**: `src-tauri/src/proxy/telemetry.rs`
- Add `record_quality_ceiling_detection()` function
- Track ceiling events for analytics

**3. This Story**: `docs/stories/Story-006-03-quality-ceiling-detection.md`
- Add detection accuracy results
- Add recommendation effectiveness data
- Update status to ✅ IMPLEMENTED

**4. Epic-006**: `docs/epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md`
- Update Story-006-03 status
- Update progress (2/6 → 3/6)

---

## Cross-References

### Related Stories

**Epic-006** (This Epic):
- Story-006-01: Live API Validation (BLOCKS this story - confirms 24576 limit)
- Story-006-02: Adaptive Budget (uses complexity classifier - shared component)
- Story-006-05: Quality Metrics Dashboard (displays ceiling detections)
- Story-006-06: Documentation (documents ceiling concept)

### Related Concepts

**Model Quality Hierarchy**:
- Lite tier: 1.0-1.4 quality range
- Flash tier: 1.5-2.0 quality range
- Pro tier: 2.0-3.0 quality range

**Upgrade Paths**:
- Lite → Flash: +0.1 to +0.7 quality improvement
- Flash → Pro: +0.5 to +1.0 quality improvement

---

## Implementation Notes

### Detection Threshold Tuning

**Recommended Starting Values**:
- Max budget threshold: 24576 (100%)
- Approaching threshold: 20000 (81.6%)
- Deep complexity threshold: 16000 (65.1%)

**Tuning Process**:
1. Deploy with conservative thresholds
2. Monitor false positive/negative rates
3. Adjust thresholds based on telemetry
4. Target: <10% false positives, <10% false negatives

### Recommendation Message Best Practices

**Clarity**:
- State current limitation (ceiling reached)
- Suggest specific action (upgrade to X model)
- Quantify benefit (quality improvement estimate)
- Preserve user choice (recommendation, not requirement)

**Localization**:
- Support EN and ZH initially
- Add more locales as needed
- Keep messages concise (1-2 sentences)

### Common Pitfalls

**Avoid**:
- ❌ Forcing model upgrades (always recommendations only)
- ❌ Detecting ceiling for non-lite models (Flash/Pro have higher ceilings)
- ❌ Overly aggressive thresholds (≥10% false positives)
- ❌ Complex quality formulas (heuristic is sufficient)

**Ensure**:
- ✅ Recommendations are actionable
- ✅ Headers parseable by all HTTP clients
- ✅ Logging provides debugging context
- ✅ Telemetry enables optimization

---

## Questions & Answers

### Q1: How do we actually measure quality improvement?

**A**: Initial version uses heuristic estimates, not actual measurement:
- Estimates based on model tier knowledge (Lite < Flash < Pro)
- Complexity-based adjustment (Deep tasks benefit more)
- Future enhancement: Collect user feedback to validate estimates

### Q2: Should we auto-upgrade models for users?

**A**: NO - Always recommendations only, never forced:
- Respect user model selection
- Users may have budget/quota constraints
- Provide information, let user decide
- Future: Could add opt-in auto-upgrade config

### Q3: What about Pro model recommendations?

**A**: Out of scope for this story (Lite → Flash only):
- Story focuses on gemini-2.5-flash-lite-thinking ceiling
- Recommends gemini-2.5-flash-thinking upgrade
- Future story could add Flash → Pro detection

### Q4: How do we handle streaming requests?

**A**: Headers added at end of stream:
- Detection happens after full response received
- Headers included in final stream message
- Identical behavior to non-streaming

### Q5: Should ceiling detection be configurable?

**A**: Not initially - enabled by default:
- Low overhead (<1ms)
- Non-intrusive (headers only)
- High value (quality guidance)
- Future: Add config flag if users request it

---

## Story Status

**Current Status**: To Do
**Next Story**: Story-006-04 (Budget Analytics Dashboard) - Can run in parallel
**Epic Progress**: 2/6 stories complete (33.3%) - Stories 001-002 complete

**Depends On**: Story-006-01 (Live API Validation) - MUST COMPLETE FIRST
**Can Run Parallel**: Story-006-02 (Adaptive Budget) - Independent feature

---

**Created**: 2026-01-11
**Last Updated**: 2026-01-11
**Story Points**: 3
**Estimated Hours**: 3 (1.5h detector + 1h recommendation + 0.5h integration)
**User Value**: Quality optimization guidance, better model utilization
