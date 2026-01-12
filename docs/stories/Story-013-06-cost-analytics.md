# Story-013-06: Cost Analytics Dashboard

**Epic**: Epic-013 (Gemini 3 Flash Optimization)
**Дата создания**: 2026-01-12
**Статус**: 📋 READY FOR DEVELOPMENT
**Приоритет**: P1 (High - Required for 95% Compliance)
**Оценка**: 2-3 дня

---

## 🎯 User Story

**As a** Product Manager / DevOps Engineer
**I want** level distribution monitoring and cost-per-level analytics
**So that** I can optimize costs, validate Flash MEDIUM usage, and make data-driven decisions

**Context**: Flash has 4 thinking levels (MINIMAL/LOW/MEDIUM/HIGH) with different cost multipliers. We need analytics to track usage patterns, optimize costs, and validate that MEDIUM level (Flash-exclusive) provides value.

---

## 📋 Background

### Current State

**No Analytics**: Usage data exists in logs but no aggregation or analysis

```yaml
current_limitations:
  level_distribution: "Unknown - which levels are used most?"
  cost_per_level: "Unknown - what does MEDIUM actually cost?"
  model_comparison: "Unknown - Flash vs Pro cost difference?"
  trend_analysis: "Unknown - usage patterns over time?"
```

### Business Impact

```yaml
without_analytics:
  problems:
    - "Can't justify Flash MEDIUM investment"
    - "Can't optimize cost (don't know where money goes)"
    - "Can't validate 4-level advantage"
    - "Can't make data-driven pricing decisions"

with_analytics:
  benefits:
    - "Optimize: Use MEDIUM for 70% of requests (saves 30% vs HIGH)"
    - "Validate: MEDIUM quality acceptable for most use cases"
    - "Decide: Flash ROI vs Pro (cost vs capability)"
    - "Monitor: Detect cost anomalies (unexpected HIGH usage)"
```

### Gap Reference

**From**: `gemini-3-flash-COMPARISON.md:308-350` (Gap 4: OPT-001)

```yaml
optimization_gap:
  category: "Cost Monitoring & Analytics"
  description: "Level distribution tracking and cost analysis"

  current_monitoring:
    granularity: "Model-level (Flash vs Pro)"
    visibility: "Basic usage counts"

  desired_monitoring:
    granularity: "Level-specific (MINIMAL/LOW/MEDIUM/HIGH)"
    visibility: "Cost breakdown, trends, optimization opportunities"
    integration: "Dashboard with actionable insights"
```

---

## 🔧 Technical Details

### Analytics Architecture

**Data Flow**:
```
Request Processing
  ↓
Structured Logging (Story-013-04)
  ↓
Log Aggregation (collect level, cost, latency)
  ↓
Analytics Module (compute metrics)
  ↓
Dashboard / API (present insights)
```

### Key Metrics to Track

#### 1. Level Distribution

**Metric**: Percentage of requests by thinking level

```yaml
metric_definition:
  name: "Level Distribution"
  calculation: "COUNT(level) / TOTAL_REQUESTS * 100"
  dimensions:
    - "by level (MINIMAL/LOW/MEDIUM/HIGH)"
    - "by model (Flash vs Pro)"
    - "by time period (hourly/daily/weekly)"

example_output:
  flash_distribution:
    MINIMAL: "5%"
    LOW: "25%"
    MEDIUM: "50%"  # ← Flash exclusive!
    HIGH: "20%"

  pro_distribution:
    LOW: "60%"
    HIGH: "40%"
    MEDIUM: "0%"  # ← Pro doesn't support
```

#### 2. Cost Per Level

**Metric**: Estimated cost by thinking level

```yaml
metric_definition:
  name: "Cost Per Level"
  calculation: "COUNT(level) * COST_MULTIPLIER(level)"

  cost_multipliers:
    MINIMAL: "1.0x base cost"
    LOW: "1.5x base cost"
    MEDIUM: "2.0x base cost"  # Estimated
    HIGH: "3.0x base cost"

example_output:
  daily_cost_breakdown:
    total_cost: "$45.00"
    by_level:
      MINIMAL: "$2.25 (5% of requests, 1.0x multiplier)"
      LOW: "$11.25 (25% of requests, 1.5x multiplier)"
      MEDIUM: "$18.00 (50% of requests, 2.0x multiplier)"
      HIGH: "$13.50 (20% of requests, 3.0x multiplier)"
```

#### 3. Model Comparison

**Metric**: Flash vs Pro cost and capability

```yaml
metric_definition:
  name: "Model Comparison"
  dimensions:
    - "Total cost (Flash vs Pro)"
    - "Request count (Flash vs Pro)"
    - "Average cost per request"
    - "Thinking level availability"

example_output:
  flash:
    total_requests: 10000
    total_cost: "$45.00"
    avg_cost_per_request: "$0.0045"
    unique_capability: "MEDIUM level (50% usage)"

  pro_high:
    total_requests: 5000
    total_cost: "$30.00"
    avg_cost_per_request: "$0.006"
    unique_capability: "Higher quality (but no MEDIUM)"

  insight: "Flash saves $10/day vs Pro for similar workload using MEDIUM"
```

#### 4. Optimization Opportunities

**Metric**: Cost savings from level optimization

```yaml
metric_definition:
  name: "Optimization Opportunities"
  analysis: "Identify requests using HIGH that could use MEDIUM"

example_output:
  current_usage:
    HIGH_usage: "20% of requests"
    estimated_cost: "$13.50/day"

  optimization_scenario:
    move_to_MEDIUM: "50% of current HIGH requests"
    new_HIGH_usage: "10%"
    new_MEDIUM_usage: "60%"
    cost_savings: "$4.50/day (33% reduction in HIGH costs)"

  recommendation: "Test MEDIUM quality for 50% of HIGH use cases"
```

### Implementation Structure

**Module**: `src-tauri/src/proxy/analytics/mod.rs` (new)

```rust
pub mod analytics {
    use std::collections::HashMap;
    use serde::{Serialize, Deserialize};

    #[derive(Serialize, Deserialize, Debug)]
    pub struct LevelStats {
        pub level: String,
        pub count: u64,
        pub percentage: f64,
        pub estimated_cost: f64,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct AnalyticsReport {
        pub period: String,  // "hourly", "daily", "weekly"
        pub total_requests: u64,
        pub total_cost: f64,
        pub level_distribution: Vec<LevelStats>,
        pub model_comparison: HashMap<String, ModelStats>,
    }

    pub struct Analytics {
        // In-memory aggregation (can be replaced with DB later)
        level_counts: HashMap<String, u64>,
        model_counts: HashMap<String, u64>,
    }

    impl Analytics {
        pub fn record_request(&mut self, model: &str, level: &str) {
            let key = format!("{}:{}", model, level);
            *self.level_counts.entry(key).or_insert(0) += 1;
            *self.model_counts.entry(model.to_string()).or_insert(0) += 1;
        }

        pub fn generate_report(&self, period: &str) -> AnalyticsReport {
            // Aggregate data
            // Calculate percentages
            // Estimate costs
            // Return report
        }
    }
}
```

### Integration Points

#### 1. Data Collection (from Story-013-04 logs)

**File**: `src-tauri/src/proxy/mappers/claude/request.rs`

```rust
use crate::proxy::analytics::ANALYTICS;

// After determining thinking level
info!(
    category = "thinking_mapping",
    model = %mapped_model,
    level = %thinking_level,
    "Level determined"
);

// Record for analytics
ANALYTICS.lock().unwrap().record_request(&mapped_model, thinking_level);
```

#### 2. Report Generation API

**File**: `src-tauri/src/commands/analytics.rs` (new)

```rust
#[tauri::command]
pub fn get_analytics_report(period: String) -> Result<AnalyticsReport, String> {
    let analytics = ANALYTICS.lock().unwrap();
    Ok(analytics.generate_report(&period))
}

#[tauri::command]
pub fn get_cost_breakdown(model: String) -> Result<CostBreakdown, String> {
    let analytics = ANALYTICS.lock().unwrap();
    Ok(analytics.get_cost_breakdown(&model))
}
```

#### 3. Dashboard UI

**File**: `src/pages/Analytics.tsx` (new page)

```typescript
import { invoke } from '@tauri-apps/api/core';
import { useEffect, useState } from 'react';
import { PieChart, Pie, Cell, BarChart, Bar, XAxis, YAxis } from 'recharts';

export function AnalyticsPage() {
  const [report, setReport] = useState(null);

  useEffect(() => {
    loadAnalytics();
  }, []);

  async function loadAnalytics() {
    const data = await invoke('get_analytics_report', { period: 'daily' });
    setReport(data);
  }

  return (
    <div className="analytics-dashboard">
      <h1>Cost Analytics Dashboard</h1>

      {/* Level Distribution Pie Chart */}
      <section>
        <h2>Level Distribution</h2>
        <PieChart width={400} height={400}>
          <Pie data={report.level_distribution} dataKey="percentage" nameKey="level" />
        </PieChart>
      </section>

      {/* Cost Breakdown */}
      <section>
        <h2>Cost by Level</h2>
        <BarChart width={600} height={300} data={report.level_distribution}>
          <Bar dataKey="estimated_cost" fill="#8884d8" />
          <XAxis dataKey="level" />
          <YAxis />
        </BarChart>
      </section>

      {/* Optimization Recommendations */}
      <section>
        <h2>Optimization Opportunities</h2>
        {report.recommendations.map(rec => (
          <div className="recommendation-card">
            <h3>{rec.title}</h3>
            <p>{rec.description}</p>
            <p className="savings">Potential savings: ${rec.savings}/day</p>
          </div>
        ))}
      </section>
    </div>
  );
}
```

---

## ✅ Acceptance Criteria

### AC-1: Level Distribution Tracking

```gherkin
GIVEN 100 requests with levels [50 MEDIUM, 30 LOW, 15 HIGH, 5 MINIMAL]
WHEN generating analytics report
THEN level_distribution shows:
  - MEDIUM: 50%
  - LOW: 30%
  - HIGH: 15%
  - MINIMAL: 5%
```

**Verification**:
- ✅ Counts accurate for each level
- ✅ Percentages sum to 100%
- ✅ Updates in real-time (or near real-time)

---

### AC-2: Cost Estimation

```gherkin
GIVEN level_distribution and cost_multipliers
WHEN calculating total cost
THEN estimated_cost is accurate within 10%
```

**Cost Multiplier Assumptions**:
```yaml
multipliers:
  MINIMAL: 1.0x
  LOW: 1.5x
  MEDIUM: 2.0x
  HIGH: 3.0x

base_cost: "$0.001 per request" # Example
```

**Verification**:
- ✅ Cost calculation logic correct
- ✅ Multipliers configurable
- ✅ Total cost matches expected

---

### AC-3: Model Comparison

```gherkin
GIVEN requests for Flash and Pro
WHEN comparing models
THEN report shows:
  - Request count per model
  - Total cost per model
  - Average cost per request
  - Level availability difference
```

**Verification**:
- ✅ Flash shows MEDIUM usage
- ✅ Pro shows 0% MEDIUM usage
- ✅ Cost comparison accurate

---

### AC-4: Dashboard UI

```gherkin
GIVEN analytics data is available
WHEN user opens Analytics page
THEN dashboard displays:
  - Level distribution pie chart
  - Cost breakdown bar chart
  - Model comparison table
  - Optimization recommendations
```

**Verification**:
- ✅ Charts render correctly
- ✅ Data updates when period changes (hourly/daily/weekly)
- ✅ UI is responsive and interactive

---

### AC-5: API Endpoints

```gherkin
GIVEN analytics module is running
WHEN frontend calls analytics APIs
THEN APIs return correct data
```

**API Tests**:
| Endpoint | Input | Expected Output |
|----------|-------|-----------------|
| `get_analytics_report("daily")` | period="daily" | AnalyticsReport with 24h data |
| `get_cost_breakdown("gemini-3-flash")` | model="flash" | Cost by level for Flash |
| `get_optimization_opportunities()` | none | List of cost-saving recommendations |

**Verification**:
- ✅ All APIs respond <100ms
- ✅ Data format matches schema
- ✅ Error handling works

---

## 🔍 Implementation Guide

### Step 1: Create Analytics Module

**File**: `src-tauri/src/proxy/analytics/mod.rs`

1. Define data structures (`LevelStats`, `AnalyticsReport`)
2. Implement `Analytics` struct with aggregation logic
3. Add `record_request()` for data collection
4. Add `generate_report()` for report generation

### Step 2: Integrate Data Collection

**Files**: `claude/request.rs`, `openai/request.rs`

1. Import analytics module
2. After determining thinking level, call `ANALYTICS.record_request()`
3. Ensure thread-safe access (use `Mutex` or `RwLock`)

### Step 3: Implement Backend APIs

**File**: `src-tauri/src/commands/analytics.rs`

1. Create Tauri command functions
2. Add to `invoke_handler!` in `lib.rs`
3. Test with manual invocations

### Step 4: Build Dashboard UI

**File**: `src/pages/Analytics.tsx`

1. Create React component
2. Fetch data using `invoke()`
3. Render charts (use recharts or similar)
4. Add period selector (hourly/daily/weekly)
5. Add model filter (Flash/Pro/All)

### Step 5: Add Navigation

**File**: `src/components/layout/Sidebar.tsx`

1. Add "Analytics" link in sidebar
2. Route to `/analytics`

### Step 6: Testing

**Unit Tests** (`analytics/mod.rs`):
```rust
#[test]
fn test_level_distribution_calculation() {
    let mut analytics = Analytics::new();

    // Record 100 requests
    for _ in 0..50 { analytics.record_request("gemini-3-flash", "MEDIUM"); }
    for _ in 0..30 { analytics.record_request("gemini-3-flash", "LOW"); }
    for _ in 0..20 { analytics.record_request("gemini-3-flash", "HIGH"); }

    let report = analytics.generate_report("test");

    assert_eq!(report.total_requests, 100);
    assert_eq!(report.level_distribution[0].percentage, 50.0); // MEDIUM
    assert_eq!(report.level_distribution[1].percentage, 30.0); // LOW
    assert_eq!(report.level_distribution[2].percentage, 20.0); // HIGH
}
```

**Integration Tests**:
1. Send 100 requests with known level distribution
2. Call `get_analytics_report()`
3. Verify report matches expected

---

## 📊 Quality Gates

### QG-1: Data Accuracy

**Verification**:
```bash
# Send 100 known requests
# 50 MEDIUM, 30 LOW, 20 HIGH

# Check report
curl http://localhost:8045/analytics/report

# Expected output:
# {
#   "total_requests": 100,
#   "level_distribution": [
#     {"level": "MEDIUM", "count": 50, "percentage": 50.0},
#     {"level": "LOW", "count": 30, "percentage": 30.0},
#     {"level": "HIGH", "count": 20, "percentage": 20.0}
#   ]
# }
```

**Expected**:
- ✅ Counts accurate (±1)
- ✅ Percentages accurate (±0.1%)
- ✅ Cost estimates accurate (±10%)

---

### QG-2: Performance

**Benchmarks**:
- `record_request()`: <1ms (non-blocking)
- `generate_report()`: <50ms (for 10K requests)
- API response: <100ms

**Expected**:
- ✅ No performance degradation
- ✅ Memory usage reasonable (<50MB for analytics)

---

### QG-3: UI Usability

**User Testing**:
1. Product Manager opens Analytics page
2. Views level distribution
3. Changes period filter (daily → weekly)
4. Exports report (optional feature)

**Expected**:
- ✅ Charts load <2 seconds
- ✅ Filters work correctly
- ✅ Data updates without page refresh

---

### QG-4: Business Value Validation

**Validation**:
1. Run for 1 week with production traffic
2. Generate weekly report
3. Identify top cost-saving opportunity
4. Implement optimization
5. Measure actual cost reduction

**Expected**:
- ✅ Analytics identify real optimization (e.g., "50% of HIGH could use MEDIUM")
- ✅ Optimization saves measurable cost (e.g., "15% cost reduction")
- ✅ ROI positive (analytics cost < savings)

---

## 🎯 Success Metrics

```yaml
data_visibility:
  before: "No level-specific insights"
  after: "Real-time level distribution and cost breakdown"
  improvement: "100% visibility into usage patterns"

cost_optimization:
  baseline: "Unoptimized level usage"
  scenario_1: "Move 50% of HIGH to MEDIUM"
  savings: "15-20% cost reduction"
  validation: "Measured via analytics dashboard"

business_decisions:
  enabled_decisions:
    - "Flash vs Pro model selection (ROI analysis)"
    - "MEDIUM level validation (quality vs cost)"
    - "Pricing strategy (cost-based pricing)"
    - "Capacity planning (predict costs)"

compliance_impact:
  flash_compliance: "88% → 95%"
  epic_013_progress: "Phase 3 complete"
```

---

## 🔗 Related Work

**Dependencies**:
- ✅ Story-013-04 (Error Logging) - Provides structured log data

**Enhances**:
- Story-013-01 (MEDIUM Level Tests) - Validates MEDIUM usage via analytics
- Story-013-05 (Caching) - Could add cache hit rate to analytics

**Future Enhancements**:
- Prometheus integration (metrics export)
- Grafana dashboards (advanced visualization)
- Automated cost alerts (PagerDuty)
- Historical trend analysis (time-series DB)

**References**:
- `gemini-3-flash-COMPARISON.md:308-350` (Gap 4: OPT-001)
- Gemini API pricing documentation
- Analytics best practices

---

## 📝 Notes

### Why This Matters for 95% Compliance

```yaml
compliance_impact:
  gap_4_opt_001: "Quality monitoring and optimization"

  before_story_013_06:
    monitoring: "Model-level only (Flash vs Pro)"
    optimization: "Guesswork, no data"
    compliance: "88%"

  after_story_013_06:
    monitoring: "Level-specific (MINIMAL/LOW/MEDIUM/HIGH)"
    optimization: "Data-driven cost management"
    compliance: "95%+"

rationale: "Analytics closes Gap 4 (OPT-001) quality monitoring requirement"
```

### Business Value Examples

**Example 1: Flash MEDIUM Validation**
```yaml
hypothesis: "MEDIUM level sufficient for 70% of use cases"
before_analytics: "Can't validate, using HIGH by default"
with_analytics:
  observation: "MEDIUM usage: 50% of requests"
  quality_feedback: "User satisfaction maintained"
  cost_impact: "30% savings vs all-HIGH usage"
  decision: "✅ MEDIUM validated, recommend as default"
```

**Example 2: Model Selection**
```yaml
question: "Should we use Flash or Pro?"
before_analytics: "Guess based on features"
with_analytics:
  flash_data:
    avg_cost: "$0.0045/request"
    unique_feature: "MEDIUM level (50% usage)"
    total_cost: "$45/day"
  pro_data:
    avg_cost: "$0.006/request"
    quality: "Higher"
    total_cost: "$60/day"
  decision: "✅ Flash for 80% of workload (saves $12/day), Pro for critical 20%"
```

---

**Story Owner**: Backend Developer + Product Manager
**Reviewers**: Tech Lead, Business Analyst
**Estimated Effort**: 2-3 days (16-24 hours)
**Actual Effort**: _TBD after completion_

**Status**: 📋 READY FOR DEVELOPMENT
**Next Step**: Assign to developer, start implementation after Story-013-04 (logging foundation)
