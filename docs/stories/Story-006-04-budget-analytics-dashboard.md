# Story-006-04: Budget Analytics Dashboard - Lite-Specific Budget Tracking

**Story ID**: Story-006-04
**Epic**: [Epic-006](../epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md) - Gemini 2.5 Flash Lite Thinking - Optimizations & Intelligence
**Priority**: P3 (Low) - Analytics Enhancement
**Estimated Effort**: 2 hours
**Type**: CODE (Backend + Frontend)
**Status**: To Do
**Created**: 2026-01-11
**Owner**: Full-Stack Dev (Backend 0.5h + Frontend 1.5h)

**Sequential Position**: Story #4 of 6 (Analytics Phase)
**Depends On**: Story-006-02 (Adaptive Budget) - Uses telemetry data
**Blocks**: Story-006-06 (Documentation) - Dashboard to document
**Can Run Parallel**: Story-006-05 (Quality Metrics) - Independent dashboard

---

## User Story

**As a** developer monitoring gemini-2.5-flash-lite-thinking usage
**I want** a dedicated dashboard showing budget distribution and efficiency metrics
**So that** I can optimize costs and identify budget waste patterns

---

## Context

### Current Situation

**Generic Monitoring** (No Lite-Specific Insights):

```yaml
current_dashboard:
  location: "src/pages/Monitor.tsx"
  metrics:
    - Total requests
    - Response times
    - Token usage (generic)
    - Error rates

  limitations:
    no_budget_distribution: "Can't see how budgets are distributed"
    no_efficiency_metrics: "Can't measure allocated vs actual usage"
    no_waste_detection: "Can't identify over-provisioning"
    no_lite_focus: "Generic metrics, not lite-specific"

example_questions_cant_answer:
  - "What's the most common thinking budget users allocate?"
  - "How much budget is wasted (allocated but unused)?"
  - "What's the P50/P95 budget usage?"
  - "Are users over-provisioning budgets?"
  - "Which complexity levels waste most budget?"
```

**Missing Insights**:
- No visibility into budget distribution (histogram)
- Can't identify waste patterns (allocated > actual)
- No percentile metrics (P50/P95)
- Can't correlate complexity with efficiency

### Expected Behavior After Story

**Lite-Specific Budget Analytics** (Data-Driven Optimization):

```yaml
new_dashboard_widget:
  title: "Flash Lite Thinking - Budget Analytics"
  location: "src/components/proxy/FlashLiteBudgetAnalytics.tsx"

  visualizations:
    budget_distribution_histogram:
      chart_type: "Bar chart"
      x_axis: "Budget ranges (0-2K, 2-8K, 8-16K, 16-24K)"
      y_axis: "Request count"
      purpose: "Show most common budget allocations"

    efficiency_by_complexity:
      chart_type: "Grouped bar chart"
      groups: "SIMPLE, MODERATE, COMPLEX, DEEP"
      metrics: "Allocated budget vs Actual usage"
      purpose: "Identify which complexity levels waste most budget"

    waste_detection_pie:
      chart_type: "Pie chart"
      segments:
        - "Efficient (80-100% usage)"
        - "Acceptable (60-80% usage)"
        - "Wasteful (<60% usage)"
      purpose: "Visualize overall efficiency distribution"

  summary_metrics:
    average_budget_allocated: "~5214 tokens"
    average_budget_actual: "~3950 tokens"
    overall_efficiency: "76%"
    p50_budget: "5000 tokens"
    p95_budget: "18000 tokens"
    cost_savings_vs_baseline: "44.5%"

  insights:
    waste_identification:
      example: "40% of requests allocate >10K but use <5K (potential waste)"
    complexity_recommendations:
      example: "SIMPLE tasks: reduce budget from 8K to 1.5K (save 81%)"
    adaptive_impact:
      example: "Adaptive budgeting reduced waste by 34 percentage points"
```

**Dashboard Features**:
```yaml
filtering:
  by_date_range: "Last 24h, 7d, 30d"
  by_complexity: "SIMPLE, MODERATE, COMPLEX, DEEP"
  by_adaptive_mode: "Adaptive vs Explicit budget"

metrics:
  totals:
    - Total requests
    - Total thinking tokens allocated
    - Total thinking tokens used
    - Overall efficiency %

  percentiles:
    - P50 budget (median)
    - P75 budget
    - P90 budget
    - P95 budget
    - P99 budget

  efficiency_breakdown:
    - By complexity level
    - By adaptive vs explicit
    - By time period

  waste_analysis:
    - Requests with >50% waste
    - Average waste by complexity
    - Cost of waste (tokens)
```

### Gap Analysis

**Source**: gemini-2.5-flash-lite-thinking-COMPARISON.md (P3 Gap #3)

```yaml
gap: "No lite-specific budget analytics dashboard"
priority: P3
current_state:
  analytics: "Generic token usage metrics"
  visualization: "Total tokens only, no distribution"
  insights: "None - raw data without interpretation"

target_state:
  analytics: "Lite-specific budget distribution and efficiency"
  visualization: "Histograms, efficiency charts, waste detection"
  insights: "Actionable recommendations for optimization"

why_valuable:
  data_driven_decisions: "Identify optimization opportunities"
  waste_reduction: "Detect and eliminate over-provisioning"
  cost_visibility: "Understand where thinking tokens spent"
  adaptive_validation: "Measure adaptive budget effectiveness"

effort: "LOW (2 hours - 0.5h backend + 1.5h frontend)"
priority_rationale: "P3 (enhancement), but enables cost optimization"
```

---

## Reference Documentation

### Primary Sources

1. **Epic-006 Document**: `docs/epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md`
   - Story-006-04 definition (lines 129-145)
   - Analytics requirements

2. **Story-006-02**: Adaptive Budget Adjustment
   - Telemetry data source
   - Efficiency metrics calculation
   - Complexity classification

3. **Existing Monitor**: `src/pages/Monitor.tsx`
   - Current monitoring structure
   - Integration patterns

### Code References

**Backend - Telemetry**:
- `src-tauri/src/proxy/telemetry.rs` - Data source (from Story-006-02)
- `src-tauri/src/proxy/handlers/telemetry.rs` - API endpoints

**Frontend - Dashboard**:
- `src/pages/Monitor.tsx` - Main monitoring page
- `src/components/proxy/` - Dashboard components directory
- `src/stores/networkMonitorStore.ts` - Monitoring state

**Visualization Libraries**:
- Chart.js or Recharts for React charts
- DaisyUI + Tailwind CSS for styling

---

## Technical Details

### Implementation Architecture

**Two-Component System**:

1. **Backend API**: Aggregate and serve budget analytics data
2. **Frontend Widget**: Visualize budget distribution and efficiency

### Component 1: Backend API (30 minutes)

**Objective**: Provide budget analytics data via REST API

**New API Endpoint**:

```rust
// src-tauri/src/proxy/handlers/telemetry.rs

use crate::proxy::telemetry;
use axum::{Json, extract::Query};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct BudgetAnalyticsQuery {
    /// Filter by date range (hours)
    #[serde(default = "default_hours")]
    hours: u32,

    /// Filter by complexity level
    complexity: Option<String>,

    /// Filter by adaptive vs explicit budget
    adaptive_only: Option<bool>,
}

fn default_hours() -> u32 { 24 }

#[derive(Serialize)]
pub struct BudgetAnalyticsResponse {
    /// Summary metrics
    pub summary: BudgetSummary,

    /// Budget distribution histogram
    pub distribution: Vec<BudgetBucket>,

    /// Efficiency by complexity level
    pub efficiency_by_complexity: Vec<ComplexityEfficiency>,

    /// Waste analysis
    pub waste_analysis: WasteAnalysis,

    /// Percentile metrics
    pub percentiles: PercentileMetrics,
}

#[derive(Serialize)]
pub struct BudgetSummary {
    pub total_requests: u32,
    pub total_allocated: u64,
    pub total_actual: u64,
    pub overall_efficiency: f32,  // %
    pub cost_savings_vs_baseline: f32,  // %
}

#[derive(Serialize)]
pub struct BudgetBucket {
    pub range: String,  // e.g., "0-2000"
    pub count: u32,
    pub avg_allocated: u32,
    pub avg_actual: u32,
}

#[derive(Serialize)]
pub struct ComplexityEfficiency {
    pub complexity: String,  // "Simple", "Moderate", "Complex", "Deep"
    pub request_count: u32,
    pub avg_allocated: u32,
    pub avg_actual: u32,
    pub efficiency: f32,  // %
}

#[derive(Serialize)]
pub struct WasteAnalysis {
    pub efficient_requests: u32,     // 80-100% usage
    pub acceptable_requests: u32,    // 60-80% usage
    pub wasteful_requests: u32,      // <60% usage
    pub total_waste_tokens: u64,
}

#[derive(Serialize)]
pub struct PercentileMetrics {
    pub p50: u32,  // Median
    pub p75: u32,
    pub p90: u32,
    pub p95: u32,
    pub p99: u32,
}

/// Get budget analytics for Flash Lite Thinking
#[get("/api/proxy/telemetry/budget-analytics")]
pub async fn get_budget_analytics(
    Query(query): Query<BudgetAnalyticsQuery>,
) -> Json<BudgetAnalyticsResponse> {
    // Fetch data from telemetry system (Story-006-02)
    let telemetry_data = telemetry::get_flash_lite_budget_data(
        query.hours,
        query.complexity.as_deref(),
        query.adaptive_only,
    );

    // Calculate summary metrics
    let summary = BudgetSummary {
        total_requests: telemetry_data.request_count(),
        total_allocated: telemetry_data.total_allocated_budget(),
        total_actual: telemetry_data.total_actual_usage(),
        overall_efficiency: telemetry_data.efficiency_percent(),
        cost_savings_vs_baseline: telemetry_data.savings_vs_baseline(),
    };

    // Generate budget distribution histogram
    let distribution = vec![
        BudgetBucket {
            range: "0-2000".to_string(),
            count: telemetry_data.count_in_range(0, 2000),
            avg_allocated: telemetry_data.avg_allocated_in_range(0, 2000),
            avg_actual: telemetry_data.avg_actual_in_range(0, 2000),
        },
        BudgetBucket {
            range: "2000-8000".to_string(),
            count: telemetry_data.count_in_range(2000, 8000),
            avg_allocated: telemetry_data.avg_allocated_in_range(2000, 8000),
            avg_actual: telemetry_data.avg_actual_in_range(2000, 8000),
        },
        BudgetBucket {
            range: "8000-16000".to_string(),
            count: telemetry_data.count_in_range(8000, 16000),
            avg_allocated: telemetry_data.avg_allocated_in_range(8000, 16000),
            avg_actual: telemetry_data.avg_actual_in_range(8000, 16000),
        },
        BudgetBucket {
            range: "16000-24576".to_string(),
            count: telemetry_data.count_in_range(16000, 24576),
            avg_allocated: telemetry_data.avg_allocated_in_range(16000, 24576),
            avg_actual: telemetry_data.avg_actual_in_range(16000, 24576),
        },
    ];

    // Efficiency by complexity
    let efficiency_by_complexity = vec!["Simple", "Moderate", "Complex", "Deep"]
        .iter()
        .map(|&complexity| ComplexityEfficiency {
            complexity: complexity.to_string(),
            request_count: telemetry_data.count_by_complexity(complexity),
            avg_allocated: telemetry_data.avg_allocated_by_complexity(complexity),
            avg_actual: telemetry_data.avg_actual_by_complexity(complexity),
            efficiency: telemetry_data.efficiency_by_complexity(complexity),
        })
        .collect();

    // Waste analysis
    let waste_analysis = WasteAnalysis {
        efficient_requests: telemetry_data.count_by_efficiency_range(0.8, 1.0),
        acceptable_requests: telemetry_data.count_by_efficiency_range(0.6, 0.8),
        wasteful_requests: telemetry_data.count_by_efficiency_range(0.0, 0.6),
        total_waste_tokens: telemetry_data.total_waste_tokens(),
    };

    // Percentile metrics
    let percentiles = PercentileMetrics {
        p50: telemetry_data.percentile(50),
        p75: telemetry_data.percentile(75),
        p90: telemetry_data.percentile(90),
        p95: telemetry_data.percentile(95),
        p99: telemetry_data.percentile(99),
    };

    Json(BudgetAnalyticsResponse {
        summary,
        distribution,
        efficiency_by_complexity,
        waste_analysis,
        percentiles,
    })
}
```

**Telemetry Data Extensions** (if needed):

```rust
// src-tauri/src/proxy/telemetry.rs

impl AdaptiveBudgetStats {
    /// Get data filtered by time range and complexity
    pub fn get_flash_lite_budget_data(
        hours: u32,
        complexity_filter: Option<&str>,
        adaptive_only: Option<bool>,
    ) -> FilteredBudgetData {
        // Filter data by parameters
        // Return aggregated statistics
    }

    /// Calculate percentile value
    pub fn percentile(&self, p: u8) -> u32 {
        // Sort budget values
        // Return value at Pth percentile
    }

    /// Count requests in budget range
    pub fn count_in_range(&self, min: u32, max: u32) -> u32 {
        // Count requests where min ≤ budget < max
    }

    // ... other helper methods for analytics
}
```

---

### Component 2: Frontend Dashboard Widget (1.5 hours)

**Objective**: Visualize budget analytics in user-friendly dashboard

**New Component**:

```typescript
// src/components/proxy/FlashLiteBudgetAnalytics.tsx

import React, { useEffect, useState } from 'react';
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  PieChart,
  Pie,
  Cell,
} from 'recharts';
import { invoke } from '@tauri-apps/api/core';

interface BudgetAnalyticsData {
  summary: BudgetSummary;
  distribution: BudgetBucket[];
  efficiency_by_complexity: ComplexityEfficiency[];
  waste_analysis: WasteAnalysis;
  percentiles: PercentileMetrics;
}

interface BudgetSummary {
  total_requests: number;
  total_allocated: number;
  total_actual: number;
  overall_efficiency: number;
  cost_savings_vs_baseline: number;
}

// ... other interfaces matching Rust structs

const FlashLiteBudgetAnalytics: React.FC = () => {
  const [data, setData] = useState<BudgetAnalyticsData | null>(null);
  const [timeRange, setTimeRange] = useState(24); // hours
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetchAnalytics();
  }, [timeRange]);

  const fetchAnalytics = async () => {
    setLoading(true);
    try {
      // Call backend API via Tauri
      const response = await invoke<BudgetAnalyticsData>(
        'get_budget_analytics',
        { hours: timeRange }
      );
      setData(response);
    } catch (error) {
      console.error('Failed to fetch budget analytics:', error);
    } finally {
      setLoading(false);
    }
  };

  if (loading) return <div>Loading analytics...</div>;
  if (!data) return <div>No data available</div>;

  return (
    <div className="flash-lite-budget-analytics p-4">
      <h2 className="text-2xl font-bold mb-4">
        Flash Lite Thinking - Budget Analytics
      </h2>

      {/* Summary Cards */}
      <div className="grid grid-cols-1 md:grid-cols-4 gap-4 mb-6">
        <SummaryCard
          title="Total Requests"
          value={data.summary.total_requests}
        />
        <SummaryCard
          title="Overall Efficiency"
          value={`${data.summary.overall_efficiency.toFixed(1)}%`}
        />
        <SummaryCard
          title="Cost Savings"
          value={`${data.summary.cost_savings_vs_baseline.toFixed(1)}%`}
          highlight
        />
        <SummaryCard
          title="P95 Budget"
          value={data.percentiles.p95}
        />
      </div>

      {/* Budget Distribution Histogram */}
      <div className="mb-6 bg-base-200 p-4 rounded-lg">
        <h3 className="text-xl font-semibold mb-3">Budget Distribution</h3>
        <BarChart width={800} height={300} data={data.distribution}>
          <CartesianGrid strokeDasharray="3 3" />
          <XAxis dataKey="range" />
          <YAxis />
          <Tooltip />
          <Legend />
          <Bar dataKey="count" fill="#8884d8" name="Requests" />
        </BarChart>
      </div>

      {/* Efficiency by Complexity */}
      <div className="mb-6 bg-base-200 p-4 rounded-lg">
        <h3 className="text-xl font-semibold mb-3">
          Efficiency by Complexity Level
        </h3>
        <BarChart
          width={800}
          height={300}
          data={data.efficiency_by_complexity}
        >
          <CartesianGrid strokeDasharray="3 3" />
          <XAxis dataKey="complexity" />
          <YAxis />
          <Tooltip />
          <Legend />
          <Bar dataKey="avg_allocated" fill="#82ca9d" name="Allocated" />
          <Bar dataKey="avg_actual" fill="#8884d8" name="Actual" />
        </BarChart>
      </div>

      {/* Waste Analysis Pie Chart */}
      <div className="mb-6 bg-base-200 p-4 rounded-lg">
        <h3 className="text-xl font-semibold mb-3">Waste Analysis</h3>
        <PieChart width={400} height={300}>
          <Pie
            data={[
              {
                name: 'Efficient (80-100%)',
                value: data.waste_analysis.efficient_requests,
              },
              {
                name: 'Acceptable (60-80%)',
                value: data.waste_analysis.acceptable_requests,
              },
              {
                name: 'Wasteful (<60%)',
                value: data.waste_analysis.wasteful_requests,
              },
            ]}
            cx={200}
            cy={150}
            labelLine={false}
            label={renderCustomLabel}
            outerRadius={80}
            fill="#8884d8"
            dataKey="value"
          >
            <Cell fill="#00C49F" />
            <Cell fill="#FFBB28" />
            <Cell fill="#FF8042" />
          </Pie>
          <Tooltip />
        </PieChart>
      </div>

      {/* Percentile Metrics Table */}
      <div className="bg-base-200 p-4 rounded-lg">
        <h3 className="text-xl font-semibold mb-3">Budget Percentiles</h3>
        <table className="table w-full">
          <thead>
            <tr>
              <th>Percentile</th>
              <th>Budget (tokens)</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td>P50 (Median)</td>
              <td>{data.percentiles.p50}</td>
            </tr>
            <tr>
              <td>P75</td>
              <td>{data.percentiles.p75}</td>
            </tr>
            <tr>
              <td>P90</td>
              <td>{data.percentiles.p90}</td>
            </tr>
            <tr>
              <td>P95</td>
              <td>{data.percentiles.p95}</td>
            </tr>
            <tr>
              <td>P99</td>
              <td>{data.percentiles.p99}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  );
};

// Helper component for summary cards
const SummaryCard: React.FC<{
  title: string;
  value: string | number;
  highlight?: boolean;
}> = ({ title, value, highlight }) => (
  <div className={`stat ${highlight ? 'bg-primary text-primary-content' : 'bg-base-200'}`}>
    <div className="stat-title">{title}</div>
    <div className="stat-value text-2xl">{value}</div>
  </div>
);

const renderCustomLabel = ({
  cx,
  cy,
  midAngle,
  innerRadius,
  outerRadius,
  percent,
}: any) => {
  const radius = innerRadius + (outerRadius - innerRadius) * 0.5;
  const x = cx + radius * Math.cos(-midAngle * (Math.PI / 180));
  const y = cy + radius * Math.sin(-midAngle * (Math.PI / 180));

  return (
    <text
      x={x}
      y={y}
      fill="white"
      textAnchor={x > cx ? 'start' : 'end'}
      dominantBaseline="central"
    >
      {`${(percent * 100).toFixed(0)}%`}
    </text>
  );
};

export default FlashLiteBudgetAnalytics;
```

**Integration into Monitor Page**:

```typescript
// src/pages/Monitor.tsx

import FlashLiteBudgetAnalytics from '@/components/proxy/FlashLiteBudgetAnalytics';

const Monitor: React.FC = () => {
  return (
    <div className="monitor-page p-4">
      <h1 className="text-3xl font-bold mb-6">API Proxy Monitor</h1>

      {/* Existing monitoring sections */}
      <RequestStats />
      <ResponseTimes />

      {/* 🆕 Flash Lite Budget Analytics (Story-006-04) */}
      <FlashLiteBudgetAnalytics />

      {/* Other monitoring widgets */}
    </div>
  );
};
```

---

## Acceptance Criteria

### AC-1: Backend API Endpoint Implemented

**Requirement**: `/api/proxy/telemetry/budget-analytics` returns comprehensive budget data

**Verification**:
```yaml
test_method: "API integration test"

test_request:
  method: GET
  url: "/api/proxy/telemetry/budget-analytics?hours=24"

expected_response:
  status: 200
  content_type: "application/json"
  structure:
    summary:
      total_requests: number
      total_allocated: number
      total_actual: number
      overall_efficiency: number
      cost_savings_vs_baseline: number

    distribution:
      - range: string
        count: number
        avg_allocated: number
        avg_actual: number

    efficiency_by_complexity:
      - complexity: string
        request_count: number
        avg_allocated: number
        avg_actual: number
        efficiency: number

    waste_analysis:
      efficient_requests: number
      acceptable_requests: number
      wasteful_requests: number
      total_waste_tokens: number

    percentiles:
      p50: number
      p75: number
      p90: number
      p95: number
      p99: number

validation:
  - All fields present
  - Numbers non-negative
  - Percentiles ordered (P50 < P75 < P90 < P95 < P99)
  - Efficiency percentages 0-100
```

**Pass Criteria**:
- ✅ API endpoint responds with 200 OK
- ✅ JSON structure matches specification
- ✅ All metrics calculated correctly
- ✅ Query parameters work (hours, complexity, adaptive_only)

---

### AC-2: Budget Distribution Histogram

**Requirement**: Histogram showing request count across 4 budget ranges

**Verification**:
```yaml
visualization_component: "BarChart in FlashLiteBudgetAnalytics"

budget_ranges:
  - "0-2000"     # SIMPLE tasks
  - "2000-8000"  # MODERATE tasks
  - "8000-16000" # COMPLEX tasks
  - "16000-24576" # DEEP tasks

x_axis: "Budget ranges"
y_axis: "Request count"

expected_insights:
  - Most common budget range visible
  - Distribution shows usage patterns
  - Can identify over/under-provisioning trends
```

**Pass Criteria**:
- ✅ Histogram displays 4 budget buckets
- ✅ Request counts accurate
- ✅ Chart interactive (tooltips on hover)
- ✅ Responsive design (mobile + desktop)

---

### AC-3: Efficiency by Complexity Chart

**Requirement**: Grouped bar chart showing allocated vs actual usage by complexity level

**Verification**:
```yaml
visualization_component: "Grouped BarChart"

complexity_levels:
  - Simple
  - Moderate
  - Complex
  - Deep

bars_per_level:
  allocated_budget: "Green bar - avg allocated"
  actual_usage: "Blue bar - avg actual"

expected_insights:
  - Identify which complexity levels waste most budget
  - See efficiency by task type
  - Validate adaptive budget effectiveness
```

**Pass Criteria**:
- ✅ Chart shows 4 complexity levels
- ✅ Two bars per level (allocated vs actual)
- ✅ Legend clear (Allocated vs Actual)
- ✅ Tooltip shows exact values

---

### AC-4: Waste Analysis Pie Chart

**Requirement**: Pie chart categorizing requests by efficiency (efficient/acceptable/wasteful)

**Verification**:
```yaml
visualization_component: "PieChart with 3 segments"

segments:
  efficient:
    definition: "80-100% usage"
    color: "Green"
    target: ">60% of requests"

  acceptable:
    definition: "60-80% usage"
    color: "Yellow"
    target: "20-30% of requests"

  wasteful:
    definition: "<60% usage"
    color: "Red"
    target: "<20% of requests"

expected_insights:
  - Overall waste level visible at glance
  - Target: ≥60% efficient requests
  - Identify if waste reduction needed
```

**Pass Criteria**:
- ✅ Pie chart shows 3 segments
- ✅ Percentages displayed on segments
- ✅ Colors match efficiency levels (green/yellow/red)
- ✅ Legend clear

---

### AC-5: Summary Metrics Displayed

**Requirement**: Key metrics visible in summary cards

**Verification**:
```yaml
summary_cards:
  card_1_total_requests:
    value: "Total request count"
    format: "Number"

  card_2_overall_efficiency:
    value: "overall_efficiency %"
    format: "XX.X%"

  card_3_cost_savings:
    value: "cost_savings_vs_baseline %"
    format: "XX.X%"
    highlight: true  # Primary color background

  card_4_p95_budget:
    value: "P95 budget percentile"
    format: "XXXXX tokens"

layout: "4-column grid on desktop, stack on mobile"
```

**Pass Criteria**:
- ✅ 4 summary cards displayed
- ✅ Values formatted correctly
- ✅ Cost savings highlighted (primary color)
- ✅ Responsive layout

---

## Implementation Steps

### Phase 1: Backend API (30 minutes)

**Step 1.1**: Extend telemetry module
- Add filtering methods (by date, complexity, adaptive)
- Add aggregation methods (percentiles, averages, distributions)

**Step 1.2**: Create API endpoint
- `get_budget_analytics()` handler
- Query parameter parsing
- Response struct serialization

**Step 1.3**: Test API
```bash
# Manual test
curl "http://localhost:8045/api/proxy/telemetry/budget-analytics?hours=24"

# Expected: JSON response with all analytics data
```

---

### Phase 2: Frontend Component (1.5 hours)

**Step 2.1**: Create component file
```bash
touch src/components/proxy/FlashLiteBudgetAnalytics.tsx
```

**Step 2.2**: Implement data fetching
- `useEffect` to fetch on mount
- `useState` for data and loading state
- Error handling

**Step 2.3**: Implement visualizations
- Budget distribution histogram (BarChart)
- Efficiency by complexity (Grouped BarChart)
- Waste analysis pie chart (PieChart)
- Percentile metrics table

**Step 2.4**: Add summary cards
- Total requests
- Overall efficiency
- Cost savings (highlighted)
- P95 budget

**Step 2.5**: Style with DaisyUI + Tailwind
- Responsive grid layout
- Card styling
- Chart colors

**Step 2.6**: Integrate into Monitor page
```typescript
import FlashLiteBudgetAnalytics from '@/components/proxy/FlashLiteBudgetAnalytics';
```

---

## Definition of Done

Story-006-04 is **COMPLETE** when:

### Backend Requirements
- ✅ API endpoint `/api/proxy/telemetry/budget-analytics` implemented
- ✅ Query parameters work (hours, complexity, adaptive_only)
- ✅ Response includes all 5 sections (summary, distribution, efficiency, waste, percentiles)
- ✅ Data calculations accurate

### Frontend Requirements
- ✅ FlashLiteBudgetAnalytics component created
- ✅ 4 visualizations implemented (histogram, efficiency chart, pie chart, table)
- ✅ 4 summary cards displayed
- ✅ Component integrated into Monitor page
- ✅ Responsive design (mobile + desktop)

### Quality Requirements
- ✅ Charts interactive (tooltips, legends)
- ✅ Data updates on time range change
- ✅ Loading state handled
- ✅ Error handling implemented

### Documentation Requirements
- ✅ Component commented
- ✅ API endpoint documented
- ✅ Story status updated to "✅ IMPLEMENTED"

---

## Dependencies

### Upstream Dependencies (Must Complete)
- ✅ Story-006-02: Adaptive Budget (provides telemetry data)

### Downstream Dependencies (Will Benefit)
- ⏳ Story-006-06: Documentation (dashboard to document)

### Parallel Work (Can Run Concurrently)
- ⏳ Story-006-03: Quality Ceiling Detection (independent feature)
- ⏳ Story-006-05: Quality Metrics Dashboard (independent dashboard)

---

## Risks & Mitigations

### Risk 1: Insufficient Telemetry Data 🟡

**Risk**: Not enough data collected to generate meaningful analytics

**Probability**: MEDIUM (30%)

**Impact**: MEDIUM - Empty charts, no insights

**Mitigation**:
```yaml
prevention:
  - Ensure Story-006-02 telemetry fully implemented
  - Seed test data for development
  - Handle empty state gracefully

if_insufficient_data:
  ui_fallback:
    - Display "Not enough data" message
    - Show placeholder charts
    - Suggest waiting for more requests

  data_collection:
    - Verify telemetry recording working
    - Check time range (extend if needed)
    - Ensure model filter correct (lite-thinking only)
```

---

### Risk 2: Chart Performance with Large Datasets 🟢

**Risk**: Slow rendering with thousands of data points

**Probability**: LOW (15%)

**Impact**: LOW - Laggy UI, but functional

**Mitigation**:
```yaml
optimization:
  - Aggregate data in backend (already done)
  - Limit chart data points (<100 per chart)
  - Use pagination for detailed views

if_performance_issues:
  - Add chart loading indicators
  - Implement data sampling
  - Add "Show more" button for details
```

---

### Risk 3: Mobile Layout Challenges 🟢

**Risk**: Charts too small or unreadable on mobile

**Probability**: MEDIUM (25%)

**Impact**: LOW - UX issue, not blocking

**Mitigation**:
```yaml
responsive_design:
  desktop: "4-column grid, full-size charts"
  tablet: "2-column grid, medium charts"
  mobile: "1-column stack, compact charts"

if_layout_issues:
  - Increase chart height on mobile
  - Use horizontal scrolling for tables
  - Simplify visualizations (fewer data points)
```

---

## Testing Strategy

### Test Pyramid

```yaml
backend_tests:
  count: 2
  focus:
    - API endpoint returns valid JSON
    - Query parameters filter correctly
  coverage: "telemetry.rs API handler"

frontend_tests:
  count: 3
  focus:
    - Component renders without crash
    - Charts display with mock data
    - Summary cards show correct values
  coverage: "FlashLiteBudgetAnalytics.tsx"

integration_tests:
  count: 1
  focus: "End-to-end data flow (API → UI)"
  coverage: "Full analytics pipeline"
```

### Test Execution Plan

**Backend Tests**:
```bash
cargo test test_budget_analytics_api
cargo test test_budget_analytics_filtering
```

**Frontend Tests**:
```bash
npm test FlashLiteBudgetAnalytics.test.tsx
```

**Manual Testing**:
1. Generate test data (multiple requests with varying budgets)
2. Open Monitor page
3. Verify all charts display
4. Check summary cards
5. Test time range filtering
6. Verify mobile responsiveness

---

## Success Metrics

### Visualization Metrics

```yaml
chart_quality:
  clarity: "≥90% users understand charts"
  interactivity: "Tooltips work on all charts"
  responsiveness: "Renders correctly on all screen sizes"

data_accuracy:
  calculations_correct: "100% (verified via tests)"
  real_time_updates: "Data refreshes on time range change"
```

### User Value Metrics

```yaml
actionable_insights:
  waste_identification: "Users identify over-provisioned budgets"
  optimization_opportunities: "Users reduce average budget by 10-20%"
  efficiency_tracking: "Users monitor efficiency improvements"

adoption:
  dashboard_usage: "≥50% of users view analytics weekly"
  optimization_actions: "≥20% adjust budgets based on insights"
```

---

## Documentation Updates

### Files to Create

**1. Frontend Component**: `src/components/proxy/FlashLiteBudgetAnalytics.tsx`
- Dashboard widget with 4 visualizations

**2. Backend Endpoint**: `src-tauri/src/proxy/handlers/telemetry.rs`
- `get_budget_analytics()` API handler

### Files to Update

**1. Telemetry Module**: `src-tauri/src/proxy/telemetry.rs`
- Add filtering and aggregation methods

**2. Monitor Page**: `src/pages/Monitor.tsx`
- Integrate FlashLiteBudgetAnalytics component

**3. This Story**: `docs/stories/Story-006-04-budget-analytics-dashboard.md`
- Add screenshots of dashboard
- Update status to ✅ IMPLEMENTED

**4. Epic-006**: `docs/epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md`
- Update Story-006-04 status
- Update progress (3/6 → 4/6)

---

## Cross-References

### Related Stories

**Epic-006** (This Epic):
- Story-006-02: Adaptive Budget (provides telemetry data)
- Story-006-05: Quality Metrics Dashboard (similar dashboard pattern)
- Story-006-06: Documentation (dashboard to document)

### Related Features

**Existing Dashboards**:
- Monitor page (main monitoring)
- Request stats (generic metrics)
- Response times (performance)

---

## Implementation Notes

### Chart Library Selection

**Recommended**: Recharts
- React-native integration
- Responsive by default
- TypeScript support
- Composable components

**Alternative**: Chart.js with react-chartjs-2
- More chart types
- Better performance for large datasets
- Steeper learning curve

### Color Scheme

```yaml
budget_allocated: "#82ca9d"  # Green
budget_actual: "#8884d8"     # Blue
efficient: "#00C49F"         # Bright green
acceptable: "#FFBB28"        # Yellow
wasteful: "#FF8042"          # Red
```

### Data Refresh Strategy

```yaml
initial_load:
  - Fetch data on component mount
  - Show loading spinner

time_range_change:
  - Re-fetch data with new hours parameter
  - Transition smoothly (no flash)

auto_refresh:
  - Optional: Poll every 60 seconds
  - Only if dashboard visible
```

### Common Pitfalls

**Avoid**:
- ❌ Fetching data on every render (use useEffect with dependencies)
- ❌ Not handling empty state (show placeholder message)
- ❌ Charts too large for mobile (use responsive sizing)
- ❌ Missing error handling (API may fail)

**Ensure**:
- ✅ Data fetching on mount and time range change only
- ✅ Empty state handled gracefully
- ✅ Responsive chart sizing
- ✅ Error boundaries for chart crashes

---

## Questions & Answers

### Q1: Should dashboard support real-time updates?

**A**: Optional - polling every 60 seconds:
- Auto-refresh when dashboard visible
- Pause when user switches tabs
- Manual refresh button as fallback

### Q2: What if user has no Flash Lite requests?

**A**: Show empty state with explanation:
- Message: "No gemini-2.5-flash-lite-thinking requests yet"
- Suggestion: "Make requests to see analytics"
- Hide charts, show placeholder

### Q3: Should we support CSV export?

**A**: Not in this story - future enhancement:
- Current: Visual dashboard only
- Future: Add "Export CSV" button
- Includes all raw data for external analysis

### Q4: How much data should we retain?

**A**: Backend decision (not frontend concern):
- Suggest: 30 days of detailed data
- Aggregated summaries for older data
- Configurable retention policy

### Q5: Should dashboard filter by project?

**A**: Not in this story - single project view:
- Current: All lite-thinking requests
- Future: Add project filter dropdown

---

## Story Status

**Current Status**: To Do
**Next Story**: Story-006-05 (Quality Metrics Dashboard) - Can run in parallel
**Epic Progress**: 3/6 stories complete (50%) - Stories 001-003 complete

**Depends On**: Story-006-02 (Adaptive Budget) - Telemetry data source
**Can Run Parallel**: Story-006-03 (Quality Ceiling), Story-006-05 (Quality Metrics)

---

**Created**: 2026-01-11
**Last Updated**: 2026-01-11
**Story Points**: 2
**Estimated Hours**: 2 (0.5h backend + 1.5h frontend)
**User Value**: Cost visibility, waste identification, data-driven optimization
