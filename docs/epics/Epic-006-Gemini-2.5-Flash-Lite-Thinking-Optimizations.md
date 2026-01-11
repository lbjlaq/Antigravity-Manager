# Epic-006: Gemini 2.5 Flash Lite Thinking - Intelligence & Analytics

> **🚨 EPIC BLOCKED - 2026-01-11** ❌
>
> **Reason**: `gemini-2.5-flash-lite` **does NOT support thinking mode** at Google API level
> **Evidence**: Direct API testing (11 accounts, 95% confidence)
> **Decision**: Epic cannot proceed - fundamental assumption invalid
> **Details**: See `docs/qa/story-006-01-GATE.md`
> **Next Action**: Proceed to Epic-007 (Gemini 3 Pro Image)

---

**Model**: `gemini-2.5-flash-lite-thinking`
**Priority**: ~~MEDIUM~~ **BLOCKED**
**Total Effort**: ~~12 hours~~ **1 hour invested (validation only)**
**Stories**: ~~6~~ **1 completed (validation)**
**Target Compliance**: ~~91.2% → 100%~~ **N/A (model lacks thinking)**
**Status**: ❌ **BLOCKED** (was: PENDING)

---

## 📋 Executive Summary

### Mission Statement

Enhance `gemini-2.5-flash-lite-thinking` from **pattern-based inference (95% confidence)** to **validated production deployment (100%)** with intelligent optimization features that maximize cost efficiency while maintaining quality standards.

### Current State Analysis

**Reference**: `gemini-2.5-flash-lite-thinking-COMPARISON.md`

```yaml
current_compliance: 91.2%
implementation_status: ✅ PRODUCTION-READY (with caveats)
confidence_level: 95% (pattern-based inference)

compliance_breakdown:
  P0_critical: "0 gaps (100%)"
  P1_high: "0 gaps (100%)"
  P2_medium: "0 gaps (100%)"
  P3_low: "4 gaps (80%)"

implementation_method:
  discovery: "Pattern matching code analysis"
  logic: "gemini-2.5-flash-lite contains 'gemini-2.5-flash' → 24576 budget"
  explicit_reference: "❌ No (inferred behavior)"
  validation_status: "⏳ Pending live API test"
```

### Why This Epic Matters

**1. Cost Optimization Champion**:
- Lowest cost thinking mode (60-70% cheaper than Flash thinking)
- Highest cost/quality ratio for simple tasks
- Critical for high-volume budget-conscious applications

**2. Intelligence Gap**:
- Manual budget configuration (no complexity-based optimization)
- No automatic quality ceiling detection (users stuck at lite limit)
- Missing analytics (can't optimize without data)

**3. Validation Uncertainty**:
- 95% confidence based on pattern matching (not 100%)
- Need live API test to confirm model existence
- Documentation accurate but unverified

---

## 🎯 Epic Goals

### Primary Objectives

1. **Validate Model Existence** (95% → 100% confidence)
   - Live API testing with thinking configuration
   - Confirm 24576 budget limit
   - Verify pattern-based implementation correct

2. **Implement Intelligent Features** (P3 gaps → production features)
   - Adaptive budget adjustment (auto-optimize costs)
   - Quality ceiling detection (auto-upgrade recommendation)
   - Smart cost/quality optimization

3. **Enable Analytics** (blind operations → data-driven)
   - Lite-specific budget tracking
   - Quality metrics dashboard
   - Cost savings measurement

4. **Complete Documentation** (scattered knowledge → comprehensive guide)
   - Consolidate all features
   - Integration examples
   - Best practices guide

### Success Criteria

**Compliance**:
- [ ] 91.2% → 100% compliance achieved
- [ ] All 4 P3 gaps resolved
- [ ] 95% → 100% confidence (live API validation)

**Intelligence**:
- [ ] Adaptive budget saves ≥20% tokens vs static budget
- [ ] Quality ceiling detected with ≥90% accuracy
- [ ] Auto-upgrade recommendations reduce manual switches by ≥40%

**Analytics**:
- [ ] Budget distribution histogram available
- [ ] Quality metrics tracked per request
- [ ] Cost savings dashboard operational

**Documentation**:
- [ ] Complete configuration guide published
- [ ] All features integrated in examples
- [ ] Best practices documented

---

## 📊 Gap Analysis

### Current Gaps (All P3 - Low Priority)

#### Gap 1: Adaptive Budget Adjustment 🎯

**Priority**: P3 (Optimization)
**Effort**: 3 hours (MEDIUM)
**Impact**: Cost savings 15-25%

```yaml
description: "Automatic thinking budget adjustment based on task complexity"

current_behavior:
  - Users manually set budget (e.g., 12000, 24576)
  - Static budget regardless of task complexity
  - Simple tasks waste tokens, complex tasks under-budgeted

gap:
  - No automatic complexity classification
  - No budget optimization logic
  - No cost/quality balancing

example_waste:
  simple_task:
    query: "What is 2+2?"
    budget: 12000
    actual_needed: ~500
    waste: 11500 tokens (96% waste)

  complex_task:
    query: "Design distributed consensus algorithm"
    budget: 12000
    actual_needed: ~20000
    deficit: -8000 tokens (insufficient, quality degraded)

proposed_solution:
  method: "Classify prompt complexity, auto-adjust budget"
  complexity_levels:
    - SIMPLE (0-2000 tokens)
    - MODERATE (2000-8000 tokens)
    - COMPLEX (8000-16000 tokens)
    - DEEP (16000-24576 tokens)
  implementation: "Request mapper with ML classifier or heuristic rules"

benefit:
  cost_savings: "15-25% average"
  quality_improvement: "Better budget allocation for complex tasks"
  user_experience: "No manual budget tuning required"
```

---

#### Gap 2: Quality Ceiling Detection 🔍

**Priority**: P3 (Enhancement)
**Effort**: 3 hours (MEDIUM)
**Impact**: Better model utilization

```yaml
description: "Automatic upgrade recommendation when lite quality ceiling reached"

current_behavior:
  - Users manually upgrade to Flash thinking
  - No indication when lite reaches quality limit
  - Suboptimal model selection for complex tasks

gap:
  - No automatic quality assessment
  - No upgrade recommendation logic
  - No quality ceiling threshold

quality_ceiling_concept:
  lite_base: "Quality level 1.0 (baseline)"
  lite_max_thinking: "Quality level 1.4 (with 24576 budget)"
  flash_base: "Quality level 1.5 (no thinking)"

  ceiling_threshold: "1.4 (lite + max thinking ≈ flash base)"
  implication: "Further budget increase won't improve quality"

proposed_solution:
  method: "Monitor quality metrics, detect ceiling, recommend upgrade"
  detection_triggers:
    - "Budget ≥ 20000 (approaching max)"
    - "Response quality score < target"
    - "User dissatisfaction signals"
  recommendation: "Suggest: 'Consider upgrading to gemini-2.5-flash-thinking for better quality'"

benefit:
  better_utilization: "Users get optimal model for task"
  cost_awareness: "Avoid wasting budget at ceiling"
  user_guidance: "Proactive model recommendations"
```

---

#### Gap 3: Lite-Specific Budget Analytics 📈

**Priority**: P3 (Analytics)
**Effort**: 2 hours (LOW)
**Impact**: Data-driven optimization

```yaml
description: "Track budget usage distribution for lite thinking"

current_behavior:
  - Generic thinking tracking (same as all models)
  - No lite-specific insights
  - Can't identify optimization opportunities

gap:
  - No budget distribution histogram
  - No average/min/max/percentile metrics
  - No waste detection analytics

proposed_solution:
  method: "Collect and visualize lite-specific budget metrics"
  metrics:
    - "Average budget used: 8234 tokens"
    - "P50 budget: 6000 tokens"
    - "P95 budget: 22000 tokens"
    - "Waste rate: 23% (budget > actual needed)"

  visualization:
    - Histogram: Budget distribution (0-24576)
    - Time series: Budget trends over time
    - Comparison: Lite vs Flash budget efficiency

benefit:
  optimization: "Identify typical budget needs"
  cost_savings: "Detect over-budgeting patterns"
  capacity_planning: "Understand usage patterns"
```

---

#### Gap 4: Quality Score Tracking 📊

**Priority**: P3 (Analytics)
**Effort**: 2 hours (MEDIUM)
**Impact**: Quality measurement

```yaml
description: "Track quality scores for lite thinking responses"

current_behavior:
  - No quality tracking
  - No user satisfaction metrics
  - Blind to quality degradation

gap:
  - No quality measurement system
  - No feedback collection
  - No quality trend analysis

proposed_solution:
  method: "Collect quality metrics, track scores"
  metrics:
    - "Response completeness score (1-5)"
    - "Reasoning quality score (1-5)"
    - "User satisfaction (thumbs up/down)"
    - "Quality ceiling proximity (0-100%)"

  storage:
    - Database: quality_metrics table
    - Fields: request_id, model, budget, quality_score, timestamp

  dashboard:
    - Average quality: 3.8/5
    - Quality trend: Last 7 days
    - Budget vs Quality correlation

benefit:
  visibility: "Understand lite thinking effectiveness"
  optimization: "Data-driven budget recommendations"
  ceiling_detection: "Identify when upgrade needed"
```

---

## 🏗️ Implementation Phases

### Phase 1: Validation & Confidence (Story 001)

**Duration**: 1 hour
**Goal**: 95% → 100% confidence via live API testing

```yaml
Story-006-01: Live API Validation & Model Confirmation
  type: VALIDATION + DOCS
  effort: 1 hour
  deliverables:
    - Live API test results
    - Model existence confirmation
    - Budget limit validation (24576)
    - Pattern-based implementation verification
    - Validation report documentation
```

**Critical Success Factor**: Confirms model exists and behaves as expected

---

### Phase 2: Intelligent Optimization (Stories 002-003)

**Duration**: 6 hours
**Goal**: Smart features for cost/quality optimization

```yaml
Story-006-02: Adaptive Budget Adjustment
  type: CODE (Backend - Request Mapper)
  effort: 3 hours
  deliverables:
    - Complexity classifier (heuristic or ML)
    - Auto-budget adjustment logic
    - 4 complexity levels (SIMPLE, MODERATE, COMPLEX, DEEP)
    - Cost savings measurement
    - Configuration examples

Story-006-03: Quality Ceiling Detection & Upgrade Recommendation
  type: CODE (Backend - Quality Monitor)
  effort: 3 hours
  deliverables:
    - Quality ceiling threshold detection
    - Auto-upgrade recommendation logic
    - Response headers with upgrade suggestions
    - Logging for ceiling events
    - Documentation of ceiling concept
```

**Critical Success Factor**: 15-25% cost savings + better model utilization

---

### Phase 3: Analytics & Monitoring (Stories 004-005)

**Duration**: 4 hours
**Goal**: Data-driven insights for optimization

```yaml
Story-006-04: Lite-Specific Budget Analytics Dashboard
  type: CODE (Backend + Frontend)
  effort: 2 hours
  deliverables:
    - Budget distribution histogram
    - Average/P50/P95 metrics
    - Waste detection analytics
    - Dashboard UI component
    - Database schema updates

Story-006-05: Quality Metrics Tracking & Dashboard
  type: CODE (Backend + Frontend)
  effort: 2 hours
  deliverables:
    - Quality scoring system
    - Feedback collection API
    - Quality trends dashboard
    - Budget vs Quality correlation charts
    - Database schema updates
```

**Critical Success Factor**: Data-driven optimization enabled

---

### Phase 4: Documentation & Integration (Story 006)

**Duration**: 1 hour
**Goal**: Comprehensive documentation consolidating all features

```yaml
Story-006-06: Documentation Consolidation & Best Practices
  type: DOCS
  effort: 1 hour
  deliverables:
    - Complete configuration guide
    - Adaptive budget usage examples
    - Quality ceiling documentation
    - Analytics interpretation guide
    - Best practices for lite thinking
    - Migration guide (when to upgrade to Flash)
```

**Critical Success Factor**: Complete user-facing documentation

---

## 📈 Epic Roadmap

### Sequential Execution Order

```
Phase 1: VALIDATION (BLOCKING)
├─ Story-006-01 (1h) ──┐
                        │
Phase 2: SMART FEATURES │ (After validation confirms model exists)
├─ Story-006-02 (3h) ◄──┤
├─ Story-006-03 (3h) ◄──┘
│
Phase 3: ANALYTICS      │ (After smart features implemented)
├─ Story-006-04 (2h) ◄──┤
├─ Story-006-05 (2h) ◄──┤
│                        │
Phase 4: DOCS           │ (After all features complete)
└─ Story-006-06 (1h) ◄──┘
```

**Parallelization Opportunities**:

**Wave 1** (After validation):
- ✅ Story-006-02 + Story-006-03 can run parallel (independent features)

**Wave 2**:
- ✅ Story-006-04 + Story-006-05 can run parallel (independent dashboards)

**Wave 3**:
- ❌ Story-006-06 must be sequential (depends on all previous)

---

## 🎯 Story Breakdown

### Story-006-01: Live API Validation (1h)

**Type**: VALIDATION + DOCS
**Owner**: Backend Dev
**Priority**: CRITICAL (blocks all other stories)

**Scope**:
- Execute live API request to gemini-2.5-flash-lite-thinking
- Validate thinking budget (1-24576)
- Confirm pattern-based implementation correct
- Document validation results

**Deliverables**:
- Validation test script
- API response evidence
- Validation report (success/failure)
- Updated confidence level in docs

**Acceptance Criteria**:
- [ ] Live API request executes successfully
- [ ] Thinking blocks present in response
- [ ] Budget limit confirmed as 24576
- [ ] Validation documented

---

### Story-006-02: Adaptive Budget Adjustment (3h)

**Type**: CODE (Backend - Request Mapper)
**Owner**: Backend Dev
**Priority**: HIGH (major optimization)

**Scope**:
- Implement complexity classifier
- Auto-adjust budget based on prompt complexity
- Add configuration toggle (enable/disable)
- Measure cost savings

**Deliverables**:
- Complexity classifier function
- Budget adjustment logic
- Unit tests (5+ tests)
- Cost savings metrics

**Acceptance Criteria**:
- [ ] Complexity classifier identifies 4 levels
- [ ] Budget auto-adjusts based on complexity
- [ ] Cost savings ≥15% measured
- [ ] Configuration toggle works
- [ ] Tests pass

---

### Story-006-03: Quality Ceiling Detection (3h)

**Type**: CODE (Backend - Quality Monitor)
**Owner**: Backend Dev
**Priority**: HIGH (quality optimization)

**Scope**:
- Implement quality ceiling threshold detection
- Add upgrade recommendation logic
- Emit response headers with suggestions
- Log ceiling events

**Deliverables**:
- Quality ceiling detection logic
- Upgrade recommendation system
- Response header: `X-Upgrade-Recommended: gemini-2.5-flash-thinking`
- Unit tests (4+ tests)

**Acceptance Criteria**:
- [ ] Ceiling detected when budget ≥20000
- [ ] Upgrade recommendation in response headers
- [ ] Logging shows ceiling events
- [ ] Tests pass

---

### Story-006-04: Budget Analytics Dashboard (2h)

**Type**: CODE (Backend + Frontend)
**Owner**: Frontend Dev
**Priority**: MEDIUM (analytics)

**Scope**:
- Track budget distribution for lite thinking
- Create histogram visualization
- Display average/P50/P95 metrics
- Detect waste patterns

**Deliverables**:
- Database schema (budget_analytics table)
- Backend API for analytics
- Frontend dashboard component
- Histogram chart (D3.js or Chart.js)

**Acceptance Criteria**:
- [ ] Budget distribution histogram displayed
- [ ] Average/P50/P95 metrics shown
- [ ] Waste detection works (budget > actual)
- [ ] Dashboard responsive

---

### Story-006-05: Quality Metrics Dashboard (2h)

**Type**: CODE (Backend + Frontend)
**Owner**: Frontend Dev
**Priority**: MEDIUM (analytics)

**Scope**:
- Implement quality scoring system
- Add feedback collection API
- Create quality trends dashboard
- Show budget vs quality correlation

**Deliverables**:
- Database schema (quality_metrics table)
- Feedback collection API endpoint
- Frontend dashboard component
- Quality trends chart

**Acceptance Criteria**:
- [ ] Quality scores collected (1-5 scale)
- [ ] Feedback API works (thumbs up/down)
- [ ] Quality trends chart displayed
- [ ] Budget vs Quality correlation shown

---

### Story-006-06: Documentation Consolidation (1h)

**Type**: DOCS
**Owner**: Technical Writer
**Priority**: MEDIUM (completeness)

**Scope**:
- Create comprehensive Flash Lite Thinking guide
- Integrate all Epic-006 features
- Document best practices
- Provide migration guide

**Deliverables**:
- `docs/features/flash-lite-thinking-guide.md`
- Configuration examples (6+ examples)
- Best practices section
- When to upgrade guide

**Acceptance Criteria**:
- [ ] All 6 Epic-006 features documented
- [ ] Configuration examples work
- [ ] Best practices clear
- [ ] Migration guide complete

---

## 🔄 Execution Strategy

### Team Assignment (3 Developers)

**Developer A (Backend Specialist)**:
- Story-006-01: Validation (1h)
- Story-006-02: Adaptive Budget (3h)
- Story-006-03: Quality Ceiling (3h)

**Total**: 7 hours

**Developer B (Frontend Specialist)**:
- Story-006-04: Budget Analytics Dashboard (2h)
- Story-006-05: Quality Metrics Dashboard (2h)
- Story-006-06: Documentation (1h)

**Total**: 5 hours

**Developer C (Full-Stack / Reviewer)**:
- Code reviews for all stories
- Support Dev A/B as needed
- Testing and QA

**Total**: Variable (reviews + testing)

---

### Timeline (2 Developers: 3 days, 3 Developers: 2 days)

**Day 1**:
- Dev A: Story-006-01 (1h) ← **BLOCKING**
- Sync point: Validation results
- Dev A: Story-006-02 (3h)
- Dev B: Waiting for validation, then start 006-04 prep

**Day 2**:
- Dev A: Story-006-03 (3h)
- Dev B: Story-006-04 (2h) + Story-006-05 (2h)
- Dev C: Reviews

**Day 3**:
- Dev B: Story-006-06 (1h)
- All: Final QA, testing, integration

---

### Parallelization Matrix

| Wave | Stories | Parallelizable? | Duration |
|------|---------|----------------|----------|
| 1 | 006-01 | ❌ No (blocking) | 1h |
| 2 | 006-02, 006-03 | ✅ Yes | 3h |
| 3 | 006-04, 006-05 | ✅ Yes | 2h |
| 4 | 006-06 | ❌ No (final) | 1h |

**Max Parallelization**: 2 developers working simultaneously in Wave 2 & 3

---

## 📊 Success Metrics

### Compliance Metrics

**Before Epic-006**:
- Overall compliance: 91.2%
- Confidence: 95% (pattern-based)
- P3 gaps: 4

**After Epic-006**:
- Overall compliance: 100%
- Confidence: 100% (validated)
- P3 gaps: 0

---

### Performance Metrics

**Adaptive Budget**:
- Cost savings: ≥15% average
- Waste reduction: ≥20%
- Quality improvement: +10% for complex tasks

**Quality Ceiling**:
- Detection accuracy: ≥90%
- Upgrade recommendations: ≥40% reduce manual switches
- User satisfaction: +15%

**Analytics**:
- Budget tracking coverage: 100% of requests
- Quality metrics coverage: ≥80% of requests (requires user feedback)
- Dashboard load time: <500ms

---

### User Impact Metrics

**Cost Efficiency**:
- Average cost per request: -20% (via adaptive budget)
- Wasted tokens: -25% (via smart budgeting)

**Quality**:
- Appropriate model usage: +30%
- User satisfaction: +15%
- Quality ceiling awareness: 100%

**Visibility**:
- Budget insights: Dashboard available
- Quality trends: Dashboard available
- Upgrade recommendations: Automated

---

## ⚠️ Risks and Mitigation

### Risk 1: Model Doesn't Exist (Validation Failure)

**Probability**: LOW (5% - pattern matching confidence is 95%)
**Impact**: CRITICAL (entire epic blocked)

**Mitigation**:
1. **If validation fails**:
   - Document as "Reserved/Predicted model name"
   - Mark Epic-006 as BLOCKED
   - Pivot to HIGH priority model (gemini-3-pro-image or gemini-2.5-pro-thinking)

2. **If thinking not supported**:
   - Update COMPARISON to remove thinking variant
   - Close Epic-006 as NOT APPLICABLE

3. **If budget different (not 24576)**:
   - Update all stories with correct budget
   - Re-validate COMPARISON document

**Contingency**: Have backup epic ready (gemini-3-pro-image spec)

---

### Risk 2: Complexity Classifier Accuracy

**Probability**: MEDIUM (40% - ML/heuristic may be inaccurate)
**Impact**: MEDIUM (cost savings not achieved)

**Mitigation**:
1. Start with simple heuristic rules (not ML)
2. A/B test with manual budget as baseline
3. Iterate on classifier based on results
4. Provide manual override option

**Acceptance Threshold**: ≥70% accuracy for cost savings

---

### Risk 3: Analytics Dashboard Performance

**Probability**: LOW
**Impact**: MEDIUM (dashboard slow for large datasets)

**Mitigation**:
1. Limit analytics to last 30 days
2. Aggregate data (not raw requests)
3. Lazy loading for charts
4. Database indexing

**Performance Target**: <500ms dashboard load

---

### Risk 4: Quality Metrics Low Adoption

**Probability**: HIGH (60% - users may not provide feedback)
**Impact**: LOW (analytics incomplete)

**Mitigation**:
1. Make feedback optional (not blocking)
2. Use implicit signals (retry rate, response time)
3. Provide incentives for feedback (future enhancement)
4. Accept partial data (≥30% coverage acceptable)

---

## 🔗 Dependencies

### External Dependencies

- **Epic-005**: Thinking activation architecture (reference implementation)
- **Epic-003**: Extended Thinking patterns (Claude reference)

### Technical Dependencies

**Backend**:
- Rust tokio runtime (async/await)
- serde_json (JSON processing)
- tracing (logging)
- Database: SQLite (new tables for analytics)

**Frontend**:
- React 19 (hooks)
- Chart.js or D3.js (visualizations)
- DaisyUI (styling)
- i18n (translations)

### Story Dependencies

```yaml
Story-006-01: BLOCKS ALL (must confirm model exists)
Story-006-02: No dependencies
Story-006-03: No dependencies
Story-006-04: Depends on 006-02 (adaptive budget data source)
Story-006-05: Depends on 006-03 (quality ceiling data source)
Story-006-06: Depends on ALL (consolidates all features)
```

---

## 📝 Documentation Deliverables

### New Documents

1. **`docs/epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md`** (this file)
2. **`docs/stories/Story-006-01-live-api-validation.md`**
3. **`docs/stories/Story-006-02-adaptive-budget-adjustment.md`**
4. **`docs/stories/Story-006-03-quality-ceiling-detection.md`**
5. **`docs/stories/Story-006-04-budget-analytics-dashboard.md`**
6. **`docs/stories/Story-006-05-quality-metrics-dashboard.md`**
7. **`docs/stories/Story-006-06-documentation-consolidation.md`**
8. **`docs/features/flash-lite-thinking-guide.md`** (from Story-006-06)
9. **`docs/validation/gemini-2.5-flash-lite-thinking-validation-report.md`** (from Story-006-01)

**Total**: 9 documents

---

### Updated Documents

1. **`gemini-2.5-flash-lite-thinking-COMPARISON.md`**:
   - Update confidence: 95% → 100%
   - Update compliance: 91.2% → 100%
   - Mark P3 gaps as RESOLVED

2. **`MASTER-MODELS-TABLE.md`**:
   - Update status: TODO → DONE
   - Update COMPARISON: ⏳ → ✅

---

## 🎉 Definition of Done

### Epic Level

- [ ] All 6 stories completed
- [ ] Compliance: 100%
- [ ] Confidence: 100% (validated)
- [ ] All P3 gaps resolved
- [ ] Cost savings ≥15% measured
- [ ] Quality metrics operational
- [ ] Documentation complete

### Production Readiness

- [ ] Live API validation passed
- [ ] Adaptive budget tested (≥70% accuracy)
- [ ] Quality ceiling detection tested
- [ ] Analytics dashboards functional
- [ ] All tests passing
- [ ] Documentation published

### Quality Gates

- [ ] Code review approved (all stories)
- [ ] Test coverage ≥85%
- [ ] Performance acceptable (<500ms dashboard)
- [ ] Security review passed
- [ ] Documentation reviewed

---

## 📚 References

### Primary References

- **COMPARISON**: `docs/antigravity/workflows/models/gemini/gemini-2.5-flash-lite-thinking-COMPARISON.md`
- **MASTER TABLE**: `docs/comparison/MASTER-MODELS-TABLE.md`
- **Epic-005**: Reference for thinking implementation patterns
- **Epic-003**: Reference for extended thinking architecture

### Code References

- **Pattern Matching**: `src-tauri/src/proxy/mappers/claude/request.rs:1440-1442`
- **Budget Clamping**: Same file (Flash thinking logic)
- **Token Manager**: `src-tauri/src/proxy/token_manager.rs`
- **Monitor**: `src-tauri/src/proxy/monitor.rs`

---

## 🔮 Future Enhancements (Post-Epic)

### Phase 2 Optimizations (Backlog)

1. **ML-Based Complexity Classifier** (8 hours)
   - Replace heuristics with ML model
   - Train on historical data
   - Improve accuracy 70% → 90%

2. **Automatic Model Switching** (4 hours)
   - Auto-upgrade to Flash thinking on quality ceiling
   - Transparent model switching
   - Cost impact notifications

3. **Predictive Budget Recommendations** (6 hours)
   - Analyze historical patterns
   - Suggest optimal budgets for similar queries
   - Personalized recommendations

4. **Quality Feedback Loop** (4 hours)
   - User thumbs up/down UI
   - Automatic quality learning
   - Adaptive quality thresholds

**Total Backlog**: 22 hours (future epic)

---

**Epic Created**: 2026-01-11
**Last Updated**: 2026-01-11
**Created By**: Documentation Team
**Status**: PENDING (Ready for story creation)
**Next Step**: Create Story-006-01 (Live API Validation)
