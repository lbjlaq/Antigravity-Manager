# Story-024-04: Detection Monitoring & Alerting (Anti-Detection)

**Epic**: Epic-024 - Anti-Detection Hardening
**Priority**: P1 (HIGH)
**Effort**: 4-5 hours (split across Week 1-2)
**Team**: Team 2 (Multi-Protocol Specialists)
**Assignee**: Dev 2C (Junior Monitoring Specialist)
**Created**: 2026-01-12
**Status**: ✅ COMPLETE - QA PASSED (10/10) - Both Parts Complete
**Dependencies**: Story 024-03 (User-Agent Rotation - for full metrics)

---

## 🎯 Objective

Implement detection event logging, alerting system, and monitoring dashboard to track detection attempts and anti-detection effectiveness. System will capture all detection events with full context, trigger configurable alerts, and display real-time detection statistics.

---

## 📊 Business Context

### Current State
```yaml
problem:
  detection_visibility: "NONE (no detection tracking)"
  alert_system: "NONE (no notifications on detection)"
  monitoring: "NONE (no detection dashboard)"
  severity: "P1 HIGH"

impact:
  blind_spots: "Cannot track detection attempts"
  response_time: "Slow (manual detection review)"
  prevention: "Limited (no early warning system)"
```

### Target State
```yaml
goal:
  detection_logging: "100% (all attempts captured)"
  alert_system: "Real-time notifications"
  monitoring_dashboard: "Detection stats and trends"

success_metrics:
  - "All detection events logged with full context"
  - "Alert thresholds configurable"
  - "Real-time detection dashboard"
  - "Notification system (email/webhook/dashboard)"
```

---

## 🔍 Detection Event Categories

### Detection Event Types
```yaml
detection_categories:
  ideType_missing:
    severity: "CRITICAL"
    description: "Request missing ideType: ANTIGRAVITY marker"
    action: "Block request, log event, alert immediately"

  apiProvider_mismatch:
    severity: "HIGH"
    description: "apiProvider doesn't match upstream API"
    action: "Log event, alert if >3 in 1 hour"

  user_agent_static:
    severity: "MEDIUM"
    description: "Static user-agent detected (rotation disabled)"
    action: "Log event, warn after 100 requests"

  rate_limit_429:
    severity: "HIGH"
    description: "429 rate limit error from upstream"
    action: "Log event, alert if >5 in 1 hour"

  auth_error_401:
    severity: "CRITICAL"
    description: "401 authentication error (possible token block)"
    action: "Block account, log event, alert immediately"

  blocked_403:
    severity: "CRITICAL"
    description: "403 forbidden (account blocked)"
    action: "Disable account, log event, alert immediately"
```

### Event Context Data
```yaml
detection_event_fields:
  timestamp: "ISO 8601 timestamp"
  event_type: "Detection category"
  severity: "CRITICAL|HIGH|MEDIUM|LOW"
  account_id: "Affected account"
  model_id: "Model being used"
  request_id: "Unique request ID"
  user_agent: "User-agent used (if applicable)"
  upstream_response: "Upstream API response (status + body)"
  context: "Additional context (error messages, etc.)"
```

---

## 📋 Acceptance Criteria

### AC1: Detection Event Logging (CRITICAL)
```gherkin
GIVEN detection events occur during proxy operation
WHEN events are detected
THEN all events MUST be logged with full context
AND logs MUST include timestamp, type, severity, account, model
AND logs MUST be structured (JSON format)
AND logs MUST be queryable for analysis
```

**Validation**:
- [ ] `src-tauri/src/modules/logger.rs` updated for detection events
- [ ] Detection events logged to separate file (`detection_events.log`)
- [ ] Structured JSON logging format
- [ ] All 6 event categories implemented
- [ ] Test coverage for logging (10+ tests)

---

### AC2: Alert Thresholds Configuration (CRITICAL)
```gherkin
GIVEN detection events are being logged
WHEN alert thresholds are configured
THEN thresholds MUST be configurable per event type
AND thresholds MUST support count-based rules (e.g., >5 events/hour)
AND thresholds MUST support time-window rules (e.g., 1 hour, 24 hours)
AND configuration MUST be loaded from config file
```

**Validation**:
- [ ] `src-tauri/src/proxy/config.rs` updated with alert config
- [ ] Alert thresholds configurable per event type
- [ ] Time windows configurable (1m, 5m, 1h, 24h)
- [ ] Count thresholds configurable (1, 3, 5, 10, etc.)
- [ ] Test coverage for threshold logic (10+ tests)

**Configuration Example**:
```json
{
  "detection_alerts": {
    "enabled": true,
    "thresholds": {
      "ideType_missing": {
        "count": 1,
        "window": "1m",
        "severity": "CRITICAL"
      },
      "rate_limit_429": {
        "count": 5,
        "window": "1h",
        "severity": "HIGH"
      },
      "user_agent_static": {
        "count": 100,
        "window": "24h",
        "severity": "MEDIUM"
      }
    }
  }
}
```

---

### AC3: Notification System (HIGH)
```gherkin
GIVEN alert thresholds are exceeded
WHEN alerts are triggered
THEN system MUST send notifications via configured channels
AND channels MUST include: email, webhook, dashboard alerts
AND notifications MUST include event summary and context
AND notifications MUST be rate-limited to prevent spam
```

**Validation**:
- [ ] Email notifications implemented (optional)
- [ ] Webhook notifications implemented
- [ ] Dashboard alerts implemented (in-app)
- [ ] Notification rate limiting (max 10/hour per channel)
- [ ] Test coverage for notifications (5+ tests)

---

### AC4: Monitoring Dashboard (HIGH)
```gherkin
GIVEN detection events are being tracked
WHEN monitoring dashboard is accessed
THEN dashboard MUST show detection statistics
AND statistics MUST include: events/hour, patterns, affected models
AND dashboard MUST show trends (24h, 7d, 30d)
AND dashboard MUST be accessible via frontend UI
```

**Validation**:
- [ ] `src-tauri/src/proxy/monitor.rs` updated with detection stats
- [ ] Dashboard API endpoint implemented
- [ ] Frontend dashboard component created
- [ ] Real-time updates via Tauri events
- [ ] Test coverage for dashboard (5+ tests)

**Dashboard Metrics**:
```yaml
detection_stats:
  total_events: "Count of all detection events"
  events_by_type: "Breakdown by event category"
  events_by_severity: "CRITICAL, HIGH, MEDIUM, LOW counts"
  events_by_account: "Which accounts triggered detection"
  events_by_model: "Which models triggered detection"
  hourly_rate: "Events per hour (last 24h)"
  trend: "Increasing, stable, or decreasing"
```

---

### AC5: Test Coverage (HIGH)
```gherkin
GIVEN detection monitoring implementation
WHEN tests are executed
THEN tests MUST validate all event logging
AND tests MUST validate alert threshold logic
AND tests MUST validate notification delivery
AND tests MUST validate dashboard data accuracy
```

**Test Requirements**:
- [ ] Unit tests: Event logging (10+ tests)
- [ ] Unit tests: Alert thresholds (10+ tests)
- [ ] Integration tests: Notification delivery (5+ tests)
- [ ] Integration tests: Dashboard API (5+ tests)
- [ ] All tests passing (100%)

---

## 🛠️ Implementation Details

### File Structure

```yaml
new_files:
  - "src-tauri/src/proxy/detection.rs"  # Detection event tracking
  - "tests/security/detection_tests.rs"  # Unit tests
  - "tests/security/detection_integration_tests.rs"  # Integration tests
  - "src/components/dashboard/DetectionMonitor.tsx"  # Frontend dashboard

modified_files:
  - "src-tauri/src/modules/logger.rs"  # Add detection logging
  - "src-tauri/src/proxy/monitor.rs"  # Add detection metrics
  - "src-tauri/src/proxy/config.rs"  # Add alert config
  - "src-tauri/src/commands/proxy.rs"  # Add dashboard API
```

---

### Step 1: Detection Event Module

**File**: `src-tauri/src/proxy/detection.rs` (NEW)

```rust
//! Detection event tracking and alerting
//!
//! Monitors and logs detection attempts, triggers alerts based on
//! configurable thresholds, and provides detection statistics.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use chrono::{DateTime, Utc, Duration};

/// Detection event severity
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

/// Detection event type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DetectionEventType {
    IdeTypeMissing,
    ApiProviderMismatch,
    UserAgentStatic,
    RateLimit429,
    AuthError401,
    Blocked403,
}

/// Detection event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: DetectionEventType,
    pub severity: Severity,
    pub account_id: String,
    pub model_id: String,
    pub request_id: String,
    pub user_agent: Option<String>,
    pub upstream_status: Option<u16>,
    pub context: String,
}

impl DetectionEvent {
    pub fn new(
        event_type: DetectionEventType,
        severity: Severity,
        account_id: String,
        model_id: String,
        request_id: String,
        context: String,
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            event_type,
            severity,
            account_id,
            model_id,
            request_id,
            user_agent: None,
            upstream_status: None,
            context,
        }
    }

    pub fn with_user_agent(mut self, user_agent: String) -> Self {
        self.user_agent = Some(user_agent);
        self
    }

    pub fn with_upstream_status(mut self, status: u16) -> Self {
        self.upstream_status = Some(status);
        self
    }
}

/// Alert threshold configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertThreshold {
    pub count: usize,
    pub window_minutes: i64,
    pub severity: Severity,
}

impl Default for AlertThreshold {
    fn default() -> Self {
        Self {
            count: 5,
            window_minutes: 60,
            severity: Severity::High,
        }
    }
}

/// Detection monitor
pub struct DetectionMonitor {
    events: Arc<Mutex<Vec<DetectionEvent>>>,
    thresholds: HashMap<DetectionEventType, AlertThreshold>,
    alert_callback: Option<Box<dyn Fn(&DetectionEvent) + Send + Sync>>,
}

impl DetectionMonitor {
    pub fn new(thresholds: HashMap<DetectionEventType, AlertThreshold>) -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            thresholds,
            alert_callback: None,
        }
    }

    /// Set alert callback function
    pub fn set_alert_callback<F>(&mut self, callback: F)
    where
        F: Fn(&DetectionEvent) + Send + Sync + 'static,
    {
        self.alert_callback = Some(Box::new(callback));
    }

    /// Record detection event
    pub fn record_event(&self, event: DetectionEvent) {
        // Log event
        tracing::warn!(
            event_type = ?event.event_type,
            severity = ?event.severity,
            account = %event.account_id,
            model = %event.model_id,
            "Detection event recorded"
        );

        // Store event
        let mut events = self.events.lock().unwrap();
        events.push(event.clone());

        // Check thresholds
        if self.should_alert(&event) {
            if let Some(callback) = &self.alert_callback {
                callback(&event);
            }
        }
    }

    /// Check if alert should be triggered
    fn should_alert(&self, event: &DetectionEvent) -> bool {
        let threshold = match self.thresholds.get(&event.event_type) {
            Some(t) => t,
            None => return false,
        };

        let events = self.events.lock().unwrap();
        let window_start = Utc::now() - Duration::minutes(threshold.window_minutes);

        let recent_count = events
            .iter()
            .filter(|e| {
                e.event_type == event.event_type
                    && e.timestamp >= window_start
            })
            .count();

        recent_count >= threshold.count
    }

    /// Get detection statistics
    pub fn get_statistics(&self) -> DetectionStatistics {
        let events = self.events.lock().unwrap();

        let total_events = events.len();

        let mut by_type: HashMap<DetectionEventType, usize> = HashMap::new();
        let mut by_severity: HashMap<Severity, usize> = HashMap::new();
        let mut by_account: HashMap<String, usize> = HashMap::new();
        let mut by_model: HashMap<String, usize> = HashMap::new();

        for event in events.iter() {
            *by_type.entry(event.event_type).or_insert(0) += 1;
            *by_severity.entry(event.severity).or_insert(0) += 1;
            *by_account.entry(event.account_id.clone()).or_insert(0) += 1;
            *by_model.entry(event.model_id.clone()).or_insert(0) += 1;
        }

        // Calculate hourly rate (last 24h)
        let last_24h = Utc::now() - Duration::hours(24);
        let events_24h = events
            .iter()
            .filter(|e| e.timestamp >= last_24h)
            .count();
        let hourly_rate = events_24h as f64 / 24.0;

        DetectionStatistics {
            total_events,
            events_by_type: by_type,
            events_by_severity: by_severity,
            events_by_account: by_account,
            events_by_model: by_model,
            hourly_rate,
        }
    }

    /// Clear old events (keep last 30 days)
    pub fn cleanup_old_events(&self) {
        let cutoff = Utc::now() - Duration::days(30);
        let mut events = self.events.lock().unwrap();
        events.retain(|e| e.timestamp >= cutoff);
    }
}

/// Detection statistics
#[derive(Debug, Clone, Serialize)]
pub struct DetectionStatistics {
    pub total_events: usize,
    pub events_by_type: HashMap<DetectionEventType, usize>,
    pub events_by_severity: HashMap<Severity, usize>,
    pub events_by_account: HashMap<String, usize>,
    pub events_by_model: HashMap<String, usize>,
    pub hourly_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detection_event_creation() {
        let event = DetectionEvent::new(
            DetectionEventType::IdeTypeMissing,
            Severity::Critical,
            "acc_123".to_string(),
            "claude-4.5-sonnet".to_string(),
            "req_456".to_string(),
            "Missing ideType marker".to_string(),
        );

        assert_eq!(event.event_type, DetectionEventType::IdeTypeMissing);
        assert_eq!(event.severity, Severity::Critical);
    }

    #[test]
    fn test_alert_threshold_default() {
        let threshold = AlertThreshold::default();
        assert_eq!(threshold.count, 5);
        assert_eq!(threshold.window_minutes, 60);
    }

    #[test]
    fn test_detection_monitor_record_event() {
        let thresholds = HashMap::new();
        let monitor = DetectionMonitor::new(thresholds);

        let event = DetectionEvent::new(
            DetectionEventType::RateLimit429,
            Severity::High,
            "acc_123".to_string(),
            "claude-4.5-sonnet".to_string(),
            "req_456".to_string(),
            "Rate limit exceeded".to_string(),
        );

        monitor.record_event(event);

        let stats = monitor.get_statistics();
        assert_eq!(stats.total_events, 1);
    }

    #[test]
    fn test_should_alert_threshold() {
        let mut thresholds = HashMap::new();
        thresholds.insert(
            DetectionEventType::RateLimit429,
            AlertThreshold {
                count: 3,
                window_minutes: 60,
                severity: Severity::High,
            },
        );

        let monitor = DetectionMonitor::new(thresholds);

        // Record 2 events (below threshold)
        for i in 0..2 {
            let event = DetectionEvent::new(
                DetectionEventType::RateLimit429,
                Severity::High,
                "acc_123".to_string(),
                "claude-4.5-sonnet".to_string(),
                format!("req_{}", i),
                "Rate limit".to_string(),
            );
            assert!(!monitor.should_alert(&event));
            monitor.record_event(event);
        }

        // Record 3rd event (should trigger alert)
        let event = DetectionEvent::new(
            DetectionEventType::RateLimit429,
            Severity::High,
            "acc_123".to_string(),
            "claude-4.5-sonnet".to_string(),
            "req_3".to_string(),
            "Rate limit".to_string(),
        );
        assert!(monitor.should_alert(&event));
    }

    #[test]
    fn test_statistics_by_type() {
        let monitor = DetectionMonitor::new(HashMap::new());

        monitor.record_event(DetectionEvent::new(
            DetectionEventType::RateLimit429,
            Severity::High,
            "acc_1".to_string(),
            "model_1".to_string(),
            "req_1".to_string(),
            "".to_string(),
        ));

        monitor.record_event(DetectionEvent::new(
            DetectionEventType::RateLimit429,
            Severity::High,
            "acc_2".to_string(),
            "model_2".to_string(),
            "req_2".to_string(),
            "".to_string(),
        ));

        monitor.record_event(DetectionEvent::new(
            DetectionEventType::IdeTypeMissing,
            Severity::Critical,
            "acc_3".to_string(),
            "model_3".to_string(),
            "req_3".to_string(),
            "".to_string(),
        ));

        let stats = monitor.get_statistics();
        assert_eq!(stats.total_events, 3);
        assert_eq!(stats.events_by_type.get(&DetectionEventType::RateLimit429), Some(&2));
        assert_eq!(stats.events_by_type.get(&DetectionEventType::IdeTypeMissing), Some(&1));
    }

    // ... 5+ more unit tests
}
```

---

### Step 2: Configuration Integration

**File**: `src-tauri/src/proxy/config.rs` (UPDATE)

```rust
use crate::proxy::detection::{AlertThreshold, DetectionEventType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    // ... existing fields

    /// Detection alerts configuration
    #[serde(default)]
    pub detection_alerts: DetectionAlertsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionAlertsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    pub thresholds: HashMap<DetectionEventType, AlertThreshold>,

    #[serde(default)]
    pub notification_channels: NotificationChannels,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotificationChannels {
    pub email: Option<EmailConfig>,
    pub webhook: Option<WebhookConfig>,
    pub dashboard: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    pub smtp_server: String,
    pub smtp_port: u16,
    pub from: String,
    pub to: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub url: String,
    pub method: String,
    pub headers: HashMap<String, String>,
}

impl Default for DetectionAlertsConfig {
    fn default() -> Self {
        let mut thresholds = HashMap::new();

        // Critical: Alert immediately
        thresholds.insert(
            DetectionEventType::IdeTypeMissing,
            AlertThreshold {
                count: 1,
                window_minutes: 1,
                severity: Severity::Critical,
            },
        );

        // High: Alert after 5 in 1 hour
        thresholds.insert(
            DetectionEventType::RateLimit429,
            AlertThreshold {
                count: 5,
                window_minutes: 60,
                severity: Severity::High,
            },
        );

        Self {
            enabled: true,
            thresholds,
            notification_channels: NotificationChannels::default(),
        }
    }
}
```

---

### Step 3: Dashboard API Command

**File**: `src-tauri/src/commands/proxy.rs` (UPDATE)

```rust
use crate::proxy::detection::DetectionStatistics;

#[tauri::command]
pub fn get_detection_statistics() -> Result<DetectionStatistics, String> {
    let monitor = get_detection_monitor()?;
    Ok(monitor.get_statistics())
}

#[tauri::command]
pub fn get_recent_detection_events(limit: usize) -> Result<Vec<DetectionEvent>, String> {
    let monitor = get_detection_monitor()?;
    let events = monitor.events.lock().unwrap();

    let recent: Vec<DetectionEvent> = events
        .iter()
        .rev()
        .take(limit)
        .cloned()
        .collect();

    Ok(recent)
}
```

---

### Step 4: Frontend Dashboard Component

**File**: `src/components/dashboard/DetectionMonitor.tsx` (NEW)

```typescript
import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

interface DetectionStatistics {
  total_events: number;
  events_by_type: Record<string, number>;
  events_by_severity: Record<string, number>;
  hourly_rate: number;
}

export const DetectionMonitor: React.FC = () => {
  const [stats, setStats] = useState<DetectionStatistics | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    loadStatistics();

    // Listen for detection events
    const unlisten = listen('detection://event', () => {
      loadStatistics();
    });

    return () => {
      unlisten.then(fn => fn());
    };
  }, []);

  const loadStatistics = async () => {
    try {
      const data = await invoke<DetectionStatistics>('get_detection_statistics');
      setStats(data);
      setLoading(false);
    } catch (error) {
      console.error('Failed to load detection statistics:', error);
    }
  };

  if (loading || !stats) {
    return <div>Loading detection statistics...</div>;
  }

  return (
    <div className="detection-monitor">
      <h2>Detection Monitoring</h2>

      <div className="stats-grid">
        <div className="stat-card">
          <h3>Total Events</h3>
          <p className="stat-value">{stats.total_events}</p>
        </div>

        <div className="stat-card">
          <h3>Hourly Rate</h3>
          <p className="stat-value">{stats.hourly_rate.toFixed(2)}/hour</p>
        </div>
      </div>

      <div className="events-by-type">
        <h3>Events by Type</h3>
        <ul>
          {Object.entries(stats.events_by_type).map(([type, count]) => (
            <li key={type}>
              {type}: {count}
            </li>
          ))}
        </ul>
      </div>

      <div className="events-by-severity">
        <h3>Events by Severity</h3>
        <ul>
          {Object.entries(stats.events_by_severity).map(([severity, count]) => (
            <li key={severity} className={`severity-${severity.toLowerCase()}`}>
              {severity}: {count}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
};
```

---

## 🧪 Test Strategy

### Unit Tests (10+ tests)
**File**: `tests/security/detection_tests.rs`

```rust
#[test]
fn test_event_creation() { /* ... */ }

#[test]
fn test_alert_threshold_logic() { /* ... */ }

#[test]
fn test_statistics_calculation() { /* ... */ }

#[test]
fn test_event_cleanup() { /* ... */ }

// ... 6+ more unit tests
```

### Integration Tests (10+ tests)
**File**: `tests/security/detection_integration_tests.rs`

```rust
#[tokio::test]
async fn test_detection_event_logging() { /* ... */ }

#[tokio::test]
async fn test_alert_notification_webhook() { /* ... */ }

#[tokio::test]
async fn test_dashboard_api_response() { /* ... */ }

// ... 7+ more integration tests
```

---

## 🔗 Dependencies

### Prerequisite
- ✅ Epic-013 COMPLETE
- 🟡 Story 024-03 (User-Agent Rotation - for full metrics)

### Blocks
- None

### Enables
- ✅ Detection visibility and tracking
- ✅ Early warning system for detection attempts
- ✅ Data-driven anti-detection improvements

---

## 📊 Success Metrics

### Code Metrics
```yaml
files_created: "3 (detection.rs + tests + dashboard)"
files_modified: "4 (logger.rs, monitor.rs, config.rs, proxy.rs)"
event_types: "6 detection categories"
alert_channels: "3 (email, webhook, dashboard)"
test_coverage:
  unit_tests: "10+ tests"
  integration_tests: "10+ tests"
```

### Quality Metrics
```yaml
regression_tests: "398/398 passing"
new_tests: "20+ tests passing (100%)"
code_review: "Dev 2A + Team Lead approval"
```

### Business Metrics
```yaml
detection_visibility: "100% (all events captured)"
alert_response_time: "<1 minute (real-time)"
monitoring_coverage: "100% (dashboard + logs + alerts)"
```

---

## 🎯 Definition of Done

- [ ] **Code Complete**:
  - [ ] detection.rs module implemented
  - [ ] 6 event types implemented
  - [ ] Alert threshold logic complete
  - [ ] Notification system complete
  - [ ] Dashboard API complete
  - [ ] Frontend dashboard component complete

- [ ] **Tests Passing**:
  - [ ] 398/398 regression tests passing
  - [ ] 10+ new unit tests passing (100%)
  - [ ] 10+ new integration tests passing (100%)

- [ ] **Quality Gates**:
  - [ ] Code review approved
  - [ ] Linting clean
  - [ ] Formatted correctly

- [ ] **Documentation**:
  - [ ] Detection monitoring documented
  - [ ] Alert configuration documented
  - [ ] Dashboard usage documented

- [ ] **Deployment Ready**:
  - [ ] Builds successfully
  - [ ] Ready for merge to main

---

**Story Status**: ✅ READY FOR DEVELOPMENT
**Assigned To**: Dev 2C (Junior Monitoring Specialist)
**Start Date**: Week 1-2 (split across epic)
**Estimated Completion**: Week 2 End (4-5 hours total)
