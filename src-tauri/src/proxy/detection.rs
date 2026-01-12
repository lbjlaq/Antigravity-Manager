//! Detection event tracking and alerting
//!
//! Part 1: Event logging infrastructure (Story-024-04 Part 1)
//! Part 2: Alerts and dashboard (Story-024-04 Part 2)
//!
//! Monitors and logs detection attempts, triggers alerts based on
//! configurable thresholds, and provides detection statistics.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::Emitter;

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
    /// Request missing ideType: ANTIGRAVITY marker
    IdeTypeMissing,
    /// apiProvider doesn't match upstream API
    ApiProviderMismatch,
    /// Static user-agent detected (rotation disabled)
    UserAgentStatic,
    /// 429 rate limit error from upstream
    RateLimit429,
    /// 401 authentication error (possible token block)
    AuthError401,
    /// 403 forbidden (account blocked)
    Blocked403,
}

/// Detection event with full context
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
    /// Create a new detection event
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

    /// Add user-agent information to the event
    pub fn with_user_agent(mut self, user_agent: String) -> Self {
        self.user_agent = Some(user_agent);
        self
    }

    /// Add upstream status code to the event
    pub fn with_upstream_status(mut self, status: u16) -> Self {
        self.upstream_status = Some(status);
        self
    }
}

/// Alert threshold configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertThreshold {
    /// Number of events to trigger alert
    pub count: usize,
    /// Time window in minutes
    pub window_minutes: i64,
    /// Severity level
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

/// Detection monitor for tracking and analyzing events
pub struct DetectionMonitor {
    /// In-memory event storage
    events: Arc<Mutex<Vec<DetectionEvent>>>,
    /// Alert threshold configurations per event type
    thresholds: HashMap<DetectionEventType, AlertThreshold>,
    /// Optional callback for alert notifications
    alert_callback: Option<Arc<dyn Fn(&DetectionEvent) + Send + Sync>>,
}

impl DetectionMonitor {
    /// Create a new detection monitor with configured thresholds
    pub fn new(thresholds: HashMap<DetectionEventType, AlertThreshold>) -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            thresholds,
            alert_callback: None,
        }
    }

    /// Set alert callback function (for Part 2)
    pub fn set_alert_callback<F>(&mut self, callback: F)
    where
        F: Fn(&DetectionEvent) + Send + Sync + 'static,
    {
        self.alert_callback = Some(Arc::new(callback));
    }

    /// Record a detection event
    pub fn record_event(&self, event: DetectionEvent) {
        // Structured logging for detection events
        tracing::warn!(
            category = "detection",
            event_type = ?event.event_type,
            severity = ?event.severity,
            account = %event.account_id,
            model = %event.model_id,
            request_id = %event.request_id,
            upstream_status = ?event.upstream_status,
            "Detection event recorded"
        );

        // Store event in memory
        let mut events = self.events.lock().unwrap();
        events.push(event.clone());

        // Check if alert should be triggered
        if self.should_alert(&event) {
            if let Some(callback) = &self.alert_callback {
                callback(&event);
            }
        }
    }

    /// Check if alert threshold is exceeded
    fn should_alert(&self, event: &DetectionEvent) -> bool {
        let threshold = match self.thresholds.get(&event.event_type) {
            Some(t) => t,
            None => return false,
        };

        let events = self.events.lock().unwrap();
        let window_start = Utc::now() - Duration::minutes(threshold.window_minutes);

        let recent_count = events
            .iter()
            .filter(|e| e.event_type == event.event_type && e.timestamp >= window_start)
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
        let events_24h = events.iter().filter(|e| e.timestamp >= last_24h).count();
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

    /// Get recent events (for dashboard)
    pub fn get_recent_events(&self, limit: usize) -> Vec<DetectionEvent> {
        let events = self.events.lock().unwrap();
        events.iter().rev().take(limit).cloned().collect()
    }

    /// Clear old events (keep last 30 days)
    pub fn cleanup_old_events(&self) {
        let cutoff = Utc::now() - Duration::days(30);
        let mut events = self.events.lock().unwrap();
        events.retain(|e| e.timestamp >= cutoff);
    }

    /// Get total event count
    pub fn event_count(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    /// Clear all events (for testing)
    #[cfg(test)]
    pub fn clear_events(&self) {
        self.events.lock().unwrap().clear();
    }
}

/// Detection statistics for monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionStatistics {
    pub total_events: usize,
    pub events_by_type: HashMap<DetectionEventType, usize>,
    pub events_by_severity: HashMap<Severity, usize>,
    pub events_by_account: HashMap<String, usize>,
    pub events_by_model: HashMap<String, usize>,
    pub hourly_rate: f64,
}

// ===== Notification System (Story-024-04 Part 2) =====

/// Notification sender for detection alerts
pub struct NotificationSender {
    webhook_config: Option<crate::proxy::config::WebhookConfig>,
    http_client: reqwest::Client,
}

impl NotificationSender {
    /// Create a new notification sender
    pub fn new(webhook_config: Option<crate::proxy::config::WebhookConfig>) -> Self {
        Self {
            webhook_config,
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    /// Send webhook notification
    pub async fn send_webhook(&self, event: &DetectionEvent) -> Result<(), String> {
        let config = match &self.webhook_config {
            Some(c) => c,
            None => return Ok(()), // No webhook configured
        };

        let payload = serde_json::json!({
            "event_type": event.event_type,
            "severity": event.severity,
            "timestamp": event.timestamp.to_rfc3339(),
            "account_id": event.account_id,
            "model_id": event.model_id,
            "request_id": event.request_id,
            "user_agent": event.user_agent,
            "upstream_status": event.upstream_status,
            "context": event.context,
        });

        let method = match config.method.to_uppercase().as_str() {
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "PATCH" => reqwest::Method::PATCH,
            _ => reqwest::Method::POST,
        };

        let mut request = self.http_client.request(method, &config.url).json(&payload);

        // Add custom headers
        for (key, value) in &config.headers {
            request = request.header(key, value);
        }

        request
            .send()
            .await
            .map_err(|e| format!("Webhook notification failed: {}", e))?;

        tracing::info!(
            category = "detection_alert",
            event_type = ?event.event_type,
            webhook_url = %config.url,
            "Webhook notification sent"
        );

        Ok(())
    }

    /// Send dashboard notification (via Tauri events)
    pub fn send_dashboard_alert(event: &DetectionEvent, app_handle: &tauri::AppHandle) {
        let _ = app_handle.emit("detection://alert", event);

        tracing::info!(
            category = "detection_alert",
            event_type = ?event.event_type,
            "Dashboard alert sent"
        );
    }
}

// Update DetectionMonitor to support notifications
impl DetectionMonitor {
    /// Create a new detection monitor with webhook configuration
    pub fn with_notifications(
        thresholds: HashMap<DetectionEventType, AlertThreshold>,
        webhook_config: Option<crate::proxy::config::WebhookConfig>,
    ) -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            thresholds,
            alert_callback: None,
        }
    }

    /// Send alert notifications (webhook + dashboard)
    pub async fn send_alert_notifications(
        &self,
        event: &DetectionEvent,
        webhook_config: Option<&crate::proxy::config::WebhookConfig>,
        app_handle: Option<&tauri::AppHandle>,
    ) {
        // Send webhook notification
        if let Some(config) = webhook_config {
            let sender = NotificationSender::new(Some(config.clone()));
            if let Err(e) = sender.send_webhook(event).await {
                tracing::error!(
                    category = "detection_alert",
                    error = %e,
                    "Failed to send webhook notification"
                );
            }
        }

        // Send dashboard notification
        if let Some(handle) = app_handle {
            NotificationSender::send_dashboard_alert(event, handle);
        }
    }
}

#[cfg(test)]
mod notification_tests {
    use super::*;

    #[test]
    fn test_notification_sender_creation() {
        let sender = NotificationSender::new(None);
        assert!(sender.webhook_config.is_none());
    }

    #[tokio::test]
    async fn test_send_webhook_no_config() {
        let sender = NotificationSender::new(None);
        let event = DetectionEvent::new(
            DetectionEventType::RateLimit429,
            Severity::High,
            "acc_123".to_string(),
            "model_123".to_string(),
            "req_123".to_string(),
            "Test".to_string(),
        );

        let result = sender.send_webhook(&event).await;
        assert!(result.is_ok()); // Should succeed (no-op)
    }

    #[test]
    fn test_webhook_payload_structure() {
        let event = DetectionEvent::new(
            DetectionEventType::IdeTypeMissing,
            Severity::Critical,
            "acc_123".to_string(),
            "claude-4.5-sonnet".to_string(),
            "req_456".to_string(),
            "Missing marker".to_string(),
        )
        .with_user_agent("Mozilla/5.0".to_string())
        .with_upstream_status(403);

        let payload = serde_json::json!({
            "event_type": event.event_type,
            "severity": event.severity,
            "timestamp": event.timestamp.to_rfc3339(),
            "account_id": event.account_id,
            "model_id": event.model_id,
            "request_id": event.request_id,
            "user_agent": event.user_agent,
            "upstream_status": event.upstream_status,
            "context": event.context,
        });

        assert_eq!(payload["account_id"], "acc_123");
        assert_eq!(payload["upstream_status"], 403);
        assert_eq!(payload["user_agent"], "Mozilla/5.0");
    }
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
        assert_eq!(event.account_id, "acc_123");
        assert_eq!(event.model_id, "claude-4.5-sonnet");
        assert_eq!(event.request_id, "req_456");
        assert_eq!(event.context, "Missing ideType marker");
        assert_eq!(event.user_agent, None);
        assert_eq!(event.upstream_status, None);
    }

    #[test]
    fn test_detection_event_with_user_agent() {
        let event = DetectionEvent::new(
            DetectionEventType::UserAgentStatic,
            Severity::Medium,
            "acc_123".to_string(),
            "gpt-4".to_string(),
            "req_789".to_string(),
            "Static UA detected".to_string(),
        )
        .with_user_agent("Mozilla/5.0".to_string());

        assert_eq!(event.user_agent, Some("Mozilla/5.0".to_string()));
    }

    #[test]
    fn test_detection_event_with_upstream_status() {
        let event = DetectionEvent::new(
            DetectionEventType::RateLimit429,
            Severity::High,
            "acc_123".to_string(),
            "claude-3-opus".to_string(),
            "req_101".to_string(),
            "Rate limit exceeded".to_string(),
        )
        .with_upstream_status(429);

        assert_eq!(event.upstream_status, Some(429));
    }

    #[test]
    fn test_alert_threshold_default() {
        let threshold = AlertThreshold::default();
        assert_eq!(threshold.count, 5);
        assert_eq!(threshold.window_minutes, 60);
        assert_eq!(threshold.severity, Severity::High);
    }

    #[test]
    fn test_detection_monitor_creation() {
        let thresholds = HashMap::new();
        let monitor = DetectionMonitor::new(thresholds);
        assert_eq!(monitor.event_count(), 0);
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
    fn test_should_alert_threshold_not_exceeded() {
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
            monitor.record_event(event);
        }

        let stats = monitor.get_statistics();
        assert_eq!(stats.total_events, 2);
    }

    #[test]
    fn test_should_alert_threshold_exceeded() {
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

        // Record 3 events (should trigger alert)
        for i in 0..3 {
            let event = DetectionEvent::new(
                DetectionEventType::RateLimit429,
                Severity::High,
                "acc_123".to_string(),
                "claude-4.5-sonnet".to_string(),
                format!("req_{}", i),
                "Rate limit".to_string(),
            );
            let should_alert = monitor.should_alert(&event);
            monitor.record_event(event);

            if i == 2 {
                assert!(should_alert);
            }
        }
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
        assert_eq!(
            stats.events_by_type.get(&DetectionEventType::RateLimit429),
            Some(&2)
        );
        assert_eq!(
            stats
                .events_by_type
                .get(&DetectionEventType::IdeTypeMissing),
            Some(&1)
        );
    }

    #[test]
    fn test_statistics_by_severity() {
        let monitor = DetectionMonitor::new(HashMap::new());

        monitor.record_event(DetectionEvent::new(
            DetectionEventType::IdeTypeMissing,
            Severity::Critical,
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
            DetectionEventType::UserAgentStatic,
            Severity::Medium,
            "acc_3".to_string(),
            "model_3".to_string(),
            "req_3".to_string(),
            "".to_string(),
        ));

        let stats = monitor.get_statistics();
        assert_eq!(stats.events_by_severity.get(&Severity::Critical), Some(&1));
        assert_eq!(stats.events_by_severity.get(&Severity::High), Some(&1));
        assert_eq!(stats.events_by_severity.get(&Severity::Medium), Some(&1));
    }

    #[test]
    fn test_statistics_by_account() {
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
            "acc_1".to_string(),
            "model_2".to_string(),
            "req_2".to_string(),
            "".to_string(),
        ));

        monitor.record_event(DetectionEvent::new(
            DetectionEventType::IdeTypeMissing,
            Severity::Critical,
            "acc_2".to_string(),
            "model_3".to_string(),
            "req_3".to_string(),
            "".to_string(),
        ));

        let stats = monitor.get_statistics();
        assert_eq!(stats.events_by_account.get("acc_1"), Some(&2));
        assert_eq!(stats.events_by_account.get("acc_2"), Some(&1));
    }

    #[test]
    fn test_statistics_by_model() {
        let monitor = DetectionMonitor::new(HashMap::new());

        monitor.record_event(DetectionEvent::new(
            DetectionEventType::RateLimit429,
            Severity::High,
            "acc_1".to_string(),
            "claude-4.5-sonnet".to_string(),
            "req_1".to_string(),
            "".to_string(),
        ));

        monitor.record_event(DetectionEvent::new(
            DetectionEventType::RateLimit429,
            Severity::High,
            "acc_2".to_string(),
            "claude-4.5-sonnet".to_string(),
            "req_2".to_string(),
            "".to_string(),
        ));

        monitor.record_event(DetectionEvent::new(
            DetectionEventType::IdeTypeMissing,
            Severity::Critical,
            "acc_3".to_string(),
            "gpt-4".to_string(),
            "req_3".to_string(),
            "".to_string(),
        ));

        let stats = monitor.get_statistics();
        assert_eq!(stats.events_by_model.get("claude-4.5-sonnet"), Some(&2));
        assert_eq!(stats.events_by_model.get("gpt-4"), Some(&1));
    }

    #[test]
    fn test_cleanup_old_events() {
        let monitor = DetectionMonitor::new(HashMap::new());

        // Record event
        monitor.record_event(DetectionEvent::new(
            DetectionEventType::RateLimit429,
            Severity::High,
            "acc_1".to_string(),
            "model_1".to_string(),
            "req_1".to_string(),
            "".to_string(),
        ));

        assert_eq!(monitor.event_count(), 1);

        // Cleanup shouldn't remove recent events
        monitor.cleanup_old_events();
        assert_eq!(monitor.event_count(), 1);
    }

    #[test]
    fn test_get_recent_events() {
        let monitor = DetectionMonitor::new(HashMap::new());

        // Record 5 events
        for i in 0..5 {
            monitor.record_event(DetectionEvent::new(
                DetectionEventType::RateLimit429,
                Severity::High,
                format!("acc_{}", i),
                "model".to_string(),
                format!("req_{}", i),
                "".to_string(),
            ));
        }

        let recent = monitor.get_recent_events(3);
        assert_eq!(recent.len(), 3);

        // Recent events should be in reverse order (newest first)
        assert_eq!(recent[0].account_id, "acc_4");
        assert_eq!(recent[1].account_id, "acc_3");
        assert_eq!(recent[2].account_id, "acc_2");
    }
}
