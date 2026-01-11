# Story-006-05: Quality Metrics Dashboard - Quality Score Tracking

**Story ID**: Story-006-05
**Epic**: [Epic-006](../epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md) - Gemini 2.5 Flash Lite Thinking - Optimizations & Intelligence
**Priority**: P3 (Low) - Quality Analytics
**Estimated Effort**: 2 hours
**Type**: CODE (Backend + Frontend)
**Status**: To Do
**Created**: 2026-01-11
**Owner**: Full-Stack Dev (Backend 0.5h + Frontend 1.5h)

**Sequential Position**: Story #5 of 6 (Analytics Phase)
**Depends On**: Story-006-03 (Quality Ceiling Detection) - Provides quality context
**Blocks**: Story-006-06 (Documentation) - Dashboard to document
**Can Run Parallel**: Story-006-04 (Budget Analytics) - Independent dashboard

---

## User Story

**As a** developer monitoring gemini-2.5-flash-lite-thinking response quality
**I want** a dashboard tracking quality scores and trends over time
**So that** I can measure quality improvements and identify degradation patterns

---

## Context

### Current Situation

**No Quality Tracking** (Blind to Quality Issues):

```yaml
current_monitoring:
  metrics_available:
    - Response times (latency)
    - Token usage (efficiency)
    - Error rates (reliability)

  metrics_missing:
    - Quality scores (user satisfaction)
    - Quality trends (improving/degrading)
    - Feedback collection (user input)
    - Ceiling detection frequency (quality limits hit)

problems:
  no_visibility: "Can't measure if responses are good quality"
  no_trends: "Can't see if quality improving or degrading"
  no_feedback_loop: "No way to collect user quality assessments"
  no_ceiling_tracking: "Don't know how often quality ceiling hit"

example_blind_spots:
  - "Is Flash Lite quality sufficient for our tasks?"
  - "Has quality improved since enabling adaptive budgets?"
  - "Which tasks result in low quality scores?"
  - "How often do we hit the quality ceiling?"
```

**Missing Feedback Mechanism**:
- Users can't rate response quality
- No way to flag poor responses
- Can't correlate quality with budget/complexity
- No actionable insights from quality data

### Expected Behavior After Story

**Comprehensive Quality Monitoring** (Data-Driven Quality Management):

```yaml
new_dashboard_widget:
  title: "Flash Lite Thinking - Quality Metrics"
  location: "src/components/proxy/FlashLiteQualityMetrics.tsx"

  visualizations:
    quality_score_trend:
      chart_type: "Line chart"
      x_axis: "Time (hourly/daily)"
      y_axis: "Average quality score (1-5)"
      purpose: "Track quality trends over time"

    quality_distribution:
      chart_type: "Bar chart"
      x_axis: "Quality scores (1-5 stars)"
      y_axis: "Response count"
      purpose: "Show distribution of quality ratings"

    quality_by_complexity:
      chart_type: "Grouped bar chart"
      groups: "SIMPLE, MODERATE, COMPLEX, DEEP"
      metrics: "Average quality score per complexity"
      purpose: "Identify which task types have quality issues"

    ceiling_detection_frequency:
      chart_type: "Stat card"
      metric: "% of requests hitting quality ceiling"
      threshold: "Red if >20%, Yellow if 10-20%, Green if <10%"
      purpose: "Monitor ceiling hit rate"

  feedback_collection:
    rating_system:
      scale: "1-5 stars"
      prompt: "Rate the quality of this response"
      optional_comment: "What could be improved?"

    api_endpoint:
      path: "/api/proxy/feedback"
      method: "POST"
      body: "{ request_id, quality_score, comment }"

  summary_metrics:
    average_quality_score: "4.2 / 5.0"
    quality_trend: "+0.3 (improving)"
    low_quality_rate: "8% (score ≤2)"
    ceiling_hit_rate: "12% (acceptable)"
```

**Quality Scoring System**:
```yaml
score_1_very_poor:
  description: "Incorrect, unhelpful, or nonsensical"
  indicators:
    - Factually wrong
    - Off-topic response
    - Incomplete reasoning

score_2_poor:
  description: "Partially correct but significant issues"
  indicators:
    - Major gaps in reasoning
    - Unclear explanations
    - Important details missing

score_3_acceptable:
  description: "Correct but not great"
  indicators:
    - Correct but minimal
    - Generic response
    - Lacks depth

score_4_good:
  description: "Good quality, useful response"
  indicators:
    - Clear reasoning
    - Helpful explanations
    - Mostly complete

score_5_excellent:
  description: "Outstanding, comprehensive response"
  indicators:
    - Thorough reasoning
    - Clear and insightful
    - Exceeds expectations
```

### Gap Analysis

**Source**: gemini-2.5-flash-lite-thinking-COMPARISON.md (P3 Gap #4)

```yaml
gap: "No quality score tracking or feedback collection"
priority: P3
current_state:
  quality_visibility: "None - no quality metrics"
  feedback_mechanism: "None - no way to rate responses"
  trend_analysis: "None - no historical quality data"

target_state:
  quality_visibility: "Real-time quality scores and trends"
  feedback_mechanism: "1-5 star rating system with comments"
  trend_analysis: "Quality trends by time, complexity, budget"

why_valuable:
  measure_improvements: "Quantify quality impact of optimizations"
  identify_issues: "Detect quality degradation early"
  validate_assumptions: "Confirm lite model quality sufficient"
  user_satisfaction: "Direct user feedback collection"

effort: "MEDIUM (2 hours - 0.5h backend + 1.5h frontend)"
priority_rationale: "P3 (enhancement), but critical for quality assurance"
```

---

## Reference Documentation

### Primary Sources

1. **Epic-006 Document**: `docs/epics/Epic-006-Gemini-2.5-Flash-Lite-Thinking-Optimizations.md`
   - Story-006-05 definition (lines 147-164)

2. **Story-006-03**: Quality Ceiling Detection
   - Quality levels concept (Lite: 1.0-1.4, Flash: 1.5-2.0)
   - Ceiling detection frequency tracking

3. **Story-006-04**: Budget Analytics Dashboard
   - Dashboard pattern reference
   - Similar visualization approach

### Code References

**Backend**:
- `src-tauri/src/proxy/telemetry.rs` - Add quality metrics
- `src-tauri/src/proxy/handlers/feedback.rs` - New feedback API

**Frontend**:
- `src/pages/Monitor.tsx` - Dashboard integration
- `src/components/proxy/` - Widget location
- Chart libraries (Recharts)

---

## Technical Details

### Implementation Architecture

**Three-Component System**:

1. **Feedback API**: Collect quality ratings and comments
2. **Quality Metrics API**: Aggregate and serve quality data
3. **Frontend Dashboard**: Visualize quality trends and scores

### Component 1: Feedback Collection API (20 minutes)

**Objective**: Allow users to rate response quality

**New API Endpoint**:

```rust
// src-tauri/src/proxy/handlers/feedback.rs (NEW FILE)

use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize)]
pub struct FeedbackRequest {
    /// Unique request ID
    pub request_id: String,

    /// Quality score (1-5)
    #[serde(deserialize_with = "validate_quality_score")]
    pub quality_score: u8,

    /// Optional feedback comment
    pub comment: Option<String>,

    /// Model used
    pub model: String,

    /// Thinking budget (if applicable)
    pub thinking_budget: Option<u32>,
}

#[derive(Serialize)]
pub struct FeedbackResponse {
    pub success: bool,
    pub message: String,
}

fn validate_quality_score<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let score: u8 = serde::Deserialize::deserialize(deserializer)?;
    if score >= 1 && score <= 5 {
        Ok(score)
    } else {
        Err(serde::de::Error::custom("Quality score must be 1-5"))
    }
}

/// Submit quality feedback for a response
#[post("/api/proxy/feedback")]
pub async fn submit_feedback(
    State(telemetry): State<Arc<TelemetryState>>,
    Json(feedback): Json<FeedbackRequest>,
) -> Json<FeedbackResponse> {
    // Store feedback in telemetry system
    telemetry.record_quality_feedback(
        &feedback.request_id,
        &feedback.model,
        feedback.quality_score,
        feedback.thinking_budget,
        feedback.comment.as_deref(),
    );

    tracing::info!(
        "Quality feedback received: request_id={}, model={}, score={}",
        feedback.request_id,
        feedback.model,
        feedback.quality_score
    );

    Json(FeedbackResponse {
        success: true,
        message: "Feedback recorded successfully".to_string(),
    })
}
```

**Feedback Storage**:

```rust
// src-tauri/src/proxy/telemetry.rs

#[derive(Clone)]
pub struct QualityFeedback {
    pub request_id: String,
    pub model: String,
    pub quality_score: u8,
    pub thinking_budget: Option<u32>,
    pub comment: Option<String>,
    pub timestamp: DateTime<Utc>,
}

pub struct TelemetryState {
    quality_feedback: Arc<Mutex<Vec<QualityFeedback>>>,
    // ... other telemetry data
}

impl TelemetryState {
    pub fn record_quality_feedback(
        &self,
        request_id: &str,
        model: &str,
        quality_score: u8,
        thinking_budget: Option<u32>,
        comment: Option<&str>,
    ) {
        let feedback = QualityFeedback {
            request_id: request_id.to_string(),
            model: model.to_string(),
            quality_score,
            thinking_budget,
            comment: comment.map(|s| s.to_string()),
            timestamp: Utc::now(),
        };

        self.quality_feedback.lock().unwrap().push(feedback);
    }
}
```

---

### Component 2: Quality Metrics API (10 minutes)

**Objective**: Provide aggregated quality metrics

**New API Endpoint**:

```rust
// src-tauri/src/proxy/handlers/telemetry.rs

#[derive(Serialize)]
pub struct QualityMetricsResponse {
    pub summary: QualitySummary,
    pub trend: Vec<QualityTrendPoint>,
    pub distribution: Vec<QualityDistributionBucket>,
    pub by_complexity: Vec<ComplexityQuality>,
    pub ceiling_stats: CeilingStats,
}

#[derive(Serialize)]
pub struct QualitySummary {
    pub total_ratings: u32,
    pub average_quality: f32,  // 1.0-5.0
    pub quality_trend: f32,    // Change from previous period
    pub low_quality_rate: f32,  // % with score ≤2
}

#[derive(Serialize)]
pub struct QualityTrendPoint {
    pub timestamp: String,  // ISO 8601
    pub average_quality: f32,
    pub rating_count: u32,
}

#[derive(Serialize)]
pub struct QualityDistributionBucket {
    pub score: u8,  // 1-5
    pub count: u32,
    pub percentage: f32,
}

#[derive(Serialize)]
pub struct ComplexityQuality {
    pub complexity: String,
    pub avg_quality: f32,
    pub rating_count: u32,
}

#[derive(Serialize)]
pub struct CeilingStats {
    pub ceiling_detection_rate: f32,  // %
    pub total_detections: u32,
    pub total_requests: u32,
}

/// Get quality metrics for Flash Lite Thinking
#[get("/api/proxy/telemetry/quality-metrics")]
pub async fn get_quality_metrics(
    Query(query): Query<QualityMetricsQuery>,
    State(telemetry): State<Arc<TelemetryState>>,
) -> Json<QualityMetricsResponse> {
    let feedback_data = telemetry.get_quality_feedback(
        query.hours.unwrap_or(24),
        query.complexity.as_deref(),
    );

    // Calculate summary
    let summary = QualitySummary {
        total_ratings: feedback_data.len() as u32,
        average_quality: feedback_data.average_quality(),
        quality_trend: feedback_data.trend_vs_previous_period(),
        low_quality_rate: feedback_data.low_quality_rate(),
    };

    // Generate quality trend (hourly or daily points)
    let trend = feedback_data.generate_trend_points(query.hours.unwrap_or(24));

    // Quality distribution (1-5 stars)
    let distribution = (1..=5)
        .map(|score| {
            let count = feedback_data.count_by_score(score);
            QualityDistributionBucket {
                score,
                count,
                percentage: (count as f32 / feedback_data.len() as f32) * 100.0,
            }
        })
        .collect();

    // Quality by complexity
    let by_complexity = vec!["Simple", "Moderate", "Complex", "Deep"]
        .iter()
        .map(|&complexity| ComplexityQuality {
            complexity: complexity.to_string(),
            avg_quality: feedback_data.avg_quality_by_complexity(complexity),
            rating_count: feedback_data.count_by_complexity(complexity),
        })
        .collect();

    // Ceiling detection stats (from Story-006-03)
    let ceiling_stats = telemetry.get_ceiling_detection_stats();

    Json(QualityMetricsResponse {
        summary,
        trend,
        distribution,
        by_complexity,
        ceiling_stats,
    })
}
```

---

### Component 3: Frontend Dashboard Widget (1.5 hours)

**Objective**: Visualize quality metrics and collect feedback

**New Component**:

```typescript
// src/components/proxy/FlashLiteQualityMetrics.tsx

import React, { useEffect, useState } from 'react';
import {
  LineChart,
  Line,
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
} from 'recharts';
import { invoke } from '@tauri-apps/api/core';

interface QualityMetricsData {
  summary: QualitySummary;
  trend: QualityTrendPoint[];
  distribution: QualityDistributionBucket[];
  by_complexity: ComplexityQuality[];
  ceiling_stats: CeilingStats;
}

// ... interface definitions matching Rust structs

const FlashLiteQualityMetrics: React.FC = () => {
  const [data, setData] = useState<QualityMetricsData | null>(null);
  const [timeRange, setTimeRange] = useState(24);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetchMetrics();
  }, [timeRange]);

  const fetchMetrics = async () => {
    setLoading(true);
    try {
      const response = await invoke<QualityMetricsData>(
        'get_quality_metrics',
        { hours: timeRange }
      );
      setData(response);
    } catch (error) {
      console.error('Failed to fetch quality metrics:', error);
    } finally {
      setLoading(false);
    }
  };

  if (loading) return <div>Loading quality metrics...</div>;
  if (!data) return <div>No quality data available</div>;

  const trendColor = data.summary.quality_trend >= 0 ? 'text-success' : 'text-error';
  const ceilingColor =
    data.ceiling_stats.ceiling_detection_rate < 10 ? 'text-success' :
    data.ceiling_stats.ceiling_detection_rate < 20 ? 'text-warning' :
    'text-error';

  return (
    <div className="flash-lite-quality-metrics p-4">
      <h2 className="text-2xl font-bold mb-4">
        Flash Lite Thinking - Quality Metrics
      </h2>

      {/* Summary Cards */}
      <div className="grid grid-cols-1 md:grid-cols-4 gap-4 mb-6">
        <SummaryCard
          title="Average Quality"
          value={`${data.summary.average_quality.toFixed(2)} / 5.0`}
          icon="⭐"
        />
        <SummaryCard
          title="Quality Trend"
          value={`${data.summary.quality_trend >= 0 ? '+' : ''}${data.summary.quality_trend.toFixed(2)}`}
          className={trendColor}
          icon={data.summary.quality_trend >= 0 ? '📈' : '📉'}
        />
        <SummaryCard
          title="Low Quality Rate"
          value={`${data.summary.low_quality_rate.toFixed(1)}%`}
          icon="⚠️"
        />
        <SummaryCard
          title="Ceiling Hit Rate"
          value={`${data.ceiling_stats.ceiling_detection_rate.toFixed(1)}%`}
          className={ceilingColor}
          icon="🚀"
        />
      </div>

      {/* Quality Trend Line Chart */}
      <div className="mb-6 bg-base-200 p-4 rounded-lg">
        <h3 className="text-xl font-semibold mb-3">Quality Trend Over Time</h3>
        <LineChart width={800} height={300} data={data.trend}>
          <CartesianGrid strokeDasharray="3 3" />
          <XAxis dataKey="timestamp" />
          <YAxis domain={[1, 5]} />
          <Tooltip />
          <Legend />
          <Line
            type="monotone"
            dataKey="average_quality"
            stroke="#8884d8"
            name="Quality Score"
            strokeWidth={2}
          />
        </LineChart>
      </div>

      {/* Quality Distribution Bar Chart */}
      <div className="mb-6 bg-base-200 p-4 rounded-lg">
        <h3 className="text-xl font-semibold mb-3">Quality Score Distribution</h3>
        <BarChart width={800} height={300} data={data.distribution}>
          <CartesianGrid strokeDasharray="3 3" />
          <XAxis dataKey="score" />
          <YAxis />
          <Tooltip />
          <Legend />
          <Bar dataKey="count" fill="#82ca9d" name="Responses" />
        </BarChart>
      </div>

      {/* Quality by Complexity */}
      <div className="mb-6 bg-base-200 p-4 rounded-lg">
        <h3 className="text-xl font-semibold mb-3">Quality by Task Complexity</h3>
        <BarChart width={800} height={300} data={data.by_complexity}>
          <CartesianGrid strokeDasharray="3 3" />
          <XAxis dataKey="complexity" />
          <YAxis domain={[0, 5]} />
          <Tooltip />
          <Legend />
          <Bar dataKey="avg_quality" fill="#8884d8" name="Avg Quality" />
        </BarChart>
      </div>

      {/* Feedback Collection Form */}
      <div className="bg-base-200 p-4 rounded-lg">
        <h3 className="text-xl font-semibold mb-3">Submit Quality Feedback</h3>
        <FeedbackForm onSubmit={handleFeedbackSubmit} />
      </div>
    </div>
  );
};

const FeedbackForm: React.FC<{ onSubmit: (feedback: any) => void }> = ({
  onSubmit,
}) => {
  const [requestId, setRequestId] = useState('');
  const [qualityScore, setQualityScore] = useState(5);
  const [comment, setComment] = useState('');

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();

    try {
      await invoke('submit_feedback', {
        feedback: {
          request_id: requestId,
          quality_score: qualityScore,
          comment: comment || null,
          model: 'gemini-2.5-flash-lite-thinking',
          thinking_budget: null,
        },
      });

      alert('Feedback submitted successfully!');
      setRequestId('');
      setQualityScore(5);
      setComment('');
    } catch (error) {
      console.error('Failed to submit feedback:', error);
      alert('Failed to submit feedback');
    }
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <div>
        <label className="label">Request ID</label>
        <input
          type="text"
          className="input input-bordered w-full"
          value={requestId}
          onChange={(e) => setRequestId(e.target.value)}
          placeholder="e.g., req_abc123"
          required
        />
      </div>

      <div>
        <label className="label">Quality Score</label>
        <div className="rating rating-lg">
          {[1, 2, 3, 4, 5].map((score) => (
            <input
              key={score}
              type="radio"
              name="quality-score"
              className="mask mask-star-2 bg-orange-400"
              checked={qualityScore === score}
              onChange={() => setQualityScore(score)}
            />
          ))}
        </div>
      </div>

      <div>
        <label className="label">Comment (Optional)</label>
        <textarea
          className="textarea textarea-bordered w-full"
          value={comment}
          onChange={(e) => setComment(e.target.value)}
          placeholder="What could be improved?"
          rows={3}
        />
      </div>

      <button type="submit" className="btn btn-primary">
        Submit Feedback
      </button>
    </form>
  );
};

const SummaryCard: React.FC<{
  title: string;
  value: string;
  icon?: string;
  className?: string;
}> = ({ title, value, icon, className }) => (
  <div className="stat bg-base-200">
    <div className="stat-title">{title}</div>
    <div className={`stat-value text-2xl ${className || ''}`}>
      {icon && <span className="mr-2">{icon}</span>}
      {value}
    </div>
  </div>
);

export default FlashLiteQualityMetrics;
```

---

## Acceptance Criteria

### AC-1: Feedback API Implemented

**Requirement**: `/api/proxy/feedback` accepts quality ratings

**Verification**:
```yaml
test_method: "API integration test"

test_request:
  method: POST
  url: "/api/proxy/feedback"
  body:
    request_id: "req_test123"
    quality_score: 4
    comment: "Good response, clear reasoning"
    model: "gemini-2.5-flash-lite-thinking"
    thinking_budget: 5000

expected_response:
  status: 200
  body:
    success: true
    message: "Feedback recorded successfully"

validation:
  - Quality score validated (1-5 only)
  - Invalid scores rejected (0, 6, etc.)
  - Feedback stored in telemetry
```

**Pass Criteria**:
- ✅ API endpoint accepts POST requests
- ✅ Quality score validation works (1-5 only)
- ✅ Feedback stored and retrievable
- ✅ Request ID logged for correlation

---

### AC-2: Quality Metrics API Implemented

**Requirement**: `/api/proxy/telemetry/quality-metrics` returns aggregated data

**Verification**:
```yaml
test_method: "API integration test"

expected_response_structure:
  summary:
    total_ratings: number
    average_quality: number (1.0-5.0)
    quality_trend: number (±X.XX)
    low_quality_rate: number (%)

  trend:
    - timestamp: string
      average_quality: number
      rating_count: number

  distribution:
    - score: number (1-5)
      count: number
      percentage: number

  by_complexity:
    - complexity: string
      avg_quality: number
      rating_count: number

  ceiling_stats:
    ceiling_detection_rate: number (%)
    total_detections: number
    total_requests: number
```

**Pass Criteria**:
- ✅ API returns valid JSON structure
- ✅ All metrics calculated correctly
- ✅ Query parameters work (hours, complexity filter)
- ✅ Ceiling stats integrated from Story-006-03

---

### AC-3: Quality Trend Visualization

**Requirement**: Line chart showing quality score trend over time

**Verification**:
```yaml
chart_type: "LineChart (Recharts)"
x_axis: "Time (hourly or daily)"
y_axis: "Average quality score (1-5)"
data_points: "One per hour (last 24h) or per day (last 7d)"

expected_insights:
  - Quality improving/degrading visible
  - Can correlate with deployments/changes
  - Rating volume visible
```

**Pass Criteria**:
- ✅ Line chart renders quality trend
- ✅ Tooltip shows exact values
- ✅ Y-axis range 1-5 (quality scale)
- ✅ Time range selectable

---

### AC-4: Quality Distribution Visualization

**Requirement**: Bar chart showing count of each quality score (1-5 stars)

**Verification**:
```yaml
chart_type: "BarChart"
x_axis: "Quality scores (1-5 stars)"
y_axis: "Response count"

expected_distribution:
  - Most responses 4-5 stars (good quality)
  - Few 1-2 star responses (poor quality)
  - Clear visualization of quality spread
```

**Pass Criteria**:
- ✅ Bar chart shows 5 bars (scores 1-5)
- ✅ Tooltip shows count and percentage
- ✅ Responsive design

---

### AC-5: Feedback Collection Form

**Requirement**: User-friendly form to submit quality ratings

**Verification**:
```yaml
form_fields:
  request_id:
    type: "text input"
    required: true
    placeholder: "req_abc123"

  quality_score:
    type: "star rating (1-5)"
    required: true
    ui: "Interactive star selector"

  comment:
    type: "textarea"
    required: false
    placeholder: "What could be improved?"

submission:
  button: "Submit Feedback"
  success: "Alert: Feedback submitted successfully"
  error: "Alert: Failed to submit feedback"
```

**Pass Criteria**:
- ✅ Form renders correctly
- ✅ Star rating selector works
- ✅ Form validation (request ID required, score 1-5)
- ✅ Submission calls feedback API
- ✅ Success/error feedback shown

---

## Implementation Steps

### Phase 1: Backend APIs (30 minutes)

**Step 1.1**: Create feedback handler
```bash
touch src-tauri/src/proxy/handlers/feedback.rs
```

**Step 1.2**: Implement feedback API
- `submit_feedback()` endpoint
- Quality score validation (1-5)
- Feedback storage in telemetry

**Step 1.3**: Extend telemetry module
- `record_quality_feedback()` method
- `get_quality_feedback()` method with filtering
- Quality metrics calculations

**Step 1.4**: Create quality metrics API
- `get_quality_metrics()` endpoint
- Aggregate feedback data
- Generate trend, distribution, complexity breakdowns

---

### Phase 2: Frontend Component (1.5 hours)

**Step 2.1**: Create component file
```bash
touch src/components/proxy/FlashLiteQualityMetrics.tsx
```

**Step 2.2**: Implement data fetching
- Fetch quality metrics from API
- Time range selection
- Loading and error states

**Step 2.3**: Implement visualizations
- Quality trend line chart
- Quality distribution bar chart
- Quality by complexity bar chart
- Summary cards (4 metrics)

**Step 2.4**: Implement feedback form
- Star rating selector
- Request ID input
- Optional comment textarea
- Submit handler

**Step 2.5**: Style with DaisyUI
- Card layouts
- Form styling
- Responsive design

**Step 2.6**: Integrate into Monitor page
```typescript
import FlashLiteQualityMetrics from '@/components/proxy/FlashLiteQualityMetrics';
```

---

## Definition of Done

Story-006-05 is **COMPLETE** when:

### Backend Requirements
- ✅ Feedback API `/api/proxy/feedback` implemented
- ✅ Quality metrics API `/api/proxy/telemetry/quality-metrics` implemented
- ✅ Feedback storage in telemetry
- ✅ Quality metrics calculations (average, trend, distribution)

### Frontend Requirements
- ✅ FlashLiteQualityMetrics component created
- ✅ 3 charts implemented (trend, distribution, by_complexity)
- ✅ 4 summary cards displayed
- ✅ Feedback form working
- ✅ Component integrated into Monitor page

### Quality Requirements
- ✅ Star rating selector works
- ✅ Form validation works
- ✅ Charts interactive (tooltips)
- ✅ Responsive design

### Documentation Requirements
- ✅ API endpoints documented
- ✅ Component commented
- ✅ Story status updated to "✅ IMPLEMENTED"

---

## Dependencies

### Upstream Dependencies (Must Complete)
- ✅ Story-006-03: Quality Ceiling Detection (ceiling stats integration)

### Downstream Dependencies (Will Benefit)
- ⏳ Story-006-06: Documentation (dashboard to document)

### Parallel Work (Can Run Concurrently)
- ⏳ Story-006-04: Budget Analytics Dashboard (independent dashboard)

---

## Risks & Mitigations

### Risk 1: Low Feedback Volume 🟡

**Risk**: Users don't submit quality ratings, dashboard empty

**Probability**: MEDIUM (40%)

**Impact**: MEDIUM - No data to analyze

**Mitigation**:
```yaml
prevention:
  - Make feedback form prominent
  - Add feedback prompt in response
  - Gamify feedback (badges, progress)

if_low_volume:
  - Display "Submit feedback to see metrics"
  - Show sample/demo data
  - Add reminder notifications
```

---

### Risk 2: Quality Scores Biased 🟡

**Risk**: Users only rate poor responses (negativity bias) or excellent responses (positivity bias)

**Probability**: MEDIUM (35%)

**Impact**: LOW - Skewed metrics, but still useful

**Mitigation**:
```yaml
bias_awareness:
  - Display sample size (total ratings)
  - Note potential bias in dashboard
  - Encourage rating all responses

if_biased:
  - Accept bias, note in documentation
  - Focus on trends (improving/degrading)
  - Supplement with automatic quality metrics (future)
```

---

## Testing Strategy

### Test Pyramid

```yaml
backend_tests:
  count: 3
  focus:
    - Feedback API validation (scores 1-5 only)
    - Quality metrics calculation
    - Telemetry integration
  coverage: "feedback.rs, telemetry.rs"

frontend_tests:
  count: 2
  focus:
    - Form validation
    - Chart rendering
  coverage: "FlashLiteQualityMetrics.tsx"

integration_tests:
  count: 1
  focus: "End-to-end feedback submission and display"
```

---

## Success Metrics

### Adoption Metrics

```yaml
feedback_collection:
  target: "≥20% of requests rated"
  measurement: "feedback_count / total_requests"

dashboard_usage:
  target: "≥50% of users view quality metrics weekly"
```

### Quality Insights

```yaml
quality_trends:
  improving: "Quality score increasing over time"
  stable: "Quality score consistent (±0.2)"
  degrading: "Quality score decreasing (alert)"

actionable_insights:
  low_quality_identification: "Detect tasks with score ≤2"
  ceiling_correlation: "Correlate ceiling hits with quality"
```

---

## Documentation Updates

### Files to Create

**1. Feedback Handler**: `src-tauri/src/proxy/handlers/feedback.rs`
**2. Quality Metrics Component**: `src/components/proxy/FlashLiteQualityMetrics.tsx`

### Files to Update

**1. Telemetry Module**: `src-tauri/src/proxy/telemetry.rs`
**2. Monitor Page**: `src/pages/Monitor.tsx`
**3. This Story**: Update status to ✅ IMPLEMENTED
**4. Epic-006**: Update progress (4/6 → 5/6)

---

## Cross-References

### Related Stories

**Epic-006**:
- Story-006-03: Quality Ceiling Detection (ceiling stats source)
- Story-006-04: Budget Analytics (similar dashboard pattern)
- Story-006-06: Documentation (dashboard to document)

---

## Story Status

**Current Status**: To Do
**Next Story**: Story-006-06 (Documentation Consolidation) - Final story
**Epic Progress**: 4/6 stories complete (66.7%)

**Depends On**: Story-006-03 (Quality Ceiling Detection)
**Can Run Parallel**: Story-006-04 (Budget Analytics)

---

**Created**: 2026-01-11
**Last Updated**: 2026-01-11
**Story Points**: 2
**Estimated Hours**: 2 (0.5h backend + 1.5h frontend)
**User Value**: Quality visibility, feedback collection, trend analysis
