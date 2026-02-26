//! Log Module Bridge - Captures tracing logs and emits them to the frontend via Tauri Events.
//! Uses a global ring buffer that can be attached to Tauri after app initialization.

use parking_lot::RwLock;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use tauri::Emitter;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// Maximum logs to keep in buffer
const MAX_BUFFER_SIZE: usize = 5000;

/// Global flag to enable/disable log bridging
static LOG_BRIDGE_ENABLED: AtomicBool = AtomicBool::new(false);

/// Atomic counter for unique log IDs
static LOG_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Global app handle for emitting events (set once during setup)
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Global log buffer for storing logs before UI connects
static LOG_BUFFER: OnceLock<Arc<RwLock<VecDeque<LogEntry>>>> = OnceLock::new();

fn get_log_buffer() -> &'static Arc<RwLock<VecDeque<LogEntry>>> {
    LOG_BUFFER.get_or_init(|| Arc::new(RwLock::new(VecDeque::with_capacity(MAX_BUFFER_SIZE))))
}

/// Log entry sent to frontend
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: u64,
    pub timestamp: i64,
    pub level: String,
    pub target: String,
    pub message: String,
    pub fields: std::collections::HashMap<String, String>,
}

/// Initialize the log bridge with app handle (call from setup)
pub fn init_log_bridge(app_handle: tauri::AppHandle) {
    let _ = APP_HANDLE.set(app_handle);
    tracing::debug!("[LogBridge] Initialized with app handle");
}

/// Enable log bridging and emit buffered logs
pub fn enable_log_bridge() {
    LOG_BRIDGE_ENABLED.store(true, Ordering::SeqCst);

    // Emit all buffered logs to frontend
    if let Some(handle) = APP_HANDLE.get() {
        let buffer = get_log_buffer().read();
        for entry in buffer.iter() {
            let _ = handle.emit("log-event", entry.clone());
        }
    }

    tracing::info!("[LogBridge] Debug console enabled");
}

/// Disable log bridging
pub fn disable_log_bridge() {
    LOG_BRIDGE_ENABLED.store(false, Ordering::SeqCst);
    tracing::info!("[LogBridge] Debug console disabled");
}

/// Check if log bridging is enabled
pub fn is_log_bridge_enabled() -> bool {
    LOG_BRIDGE_ENABLED.load(Ordering::SeqCst)
}

/// Get all buffered logs
pub fn get_buffered_logs() -> Vec<LogEntry> {
    get_log_buffer().read().iter().cloned().collect()
}

/// Clear log buffer
pub fn clear_log_buffer() {
    get_log_buffer().write().clear();
}

/// Emit accounts://refreshed event to notify the frontend of account state changes
/// This is used by background tasks (e.g. warmup 403 handling) that cannot access AppHandle directly.
pub fn emit_accounts_refreshed() {
    if let Some(handle) = APP_HANDLE.get() {
        let _ = handle.emit("accounts://refreshed", ());
        tracing::debug!("[LogBridge] Emitted accounts://refreshed event to frontend");
    }
}

/// Visitor to extract fields from tracing events
struct FieldVisitor {
    message: Option<String>,
    fields: std::collections::HashMap<String, String>,
}

impl FieldVisitor {
    fn new() -> Self {
        Self {
            message: None,
            fields: std::collections::HashMap::new(),
        }
    }
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let value_str = format!("{:?}", value);
        if field.name() == "message" {
            self.message = Some(value_str.trim_matches('"').to_string());
        } else {
            self.fields.insert(field.name().to_string(), value_str);
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields
                .insert(field.name().to_string(), value.to_string());
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }
}

/// Tracing Layer that bridges logs to buffer and optionally to Tauri frontend
pub struct TauriLogBridgeLayer;

impl TauriLogBridgeLayer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TauriLogBridgeLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for TauriLogBridgeLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        // [FIX] 如果调试控制台未启用，直接跳过所有处理，避免性能损耗
        if !LOG_BRIDGE_ENABLED.load(Ordering::Relaxed) {
            return;
        }

        // Extract metadata
        let metadata = event.metadata();
        let level = match *metadata.level() {
            Level::ERROR => "ERROR",
            Level::WARN => "WARN",
            Level::INFO => "INFO",
            Level::DEBUG => "DEBUG",
            Level::TRACE => "TRACE",
        };

        // Visit fields
        let mut visitor = FieldVisitor::new();
        event.record(&mut visitor);

        // Build message
        let message = visitor.message.unwrap_or_default();

        // Skip empty messages and internal noise
        if message.is_empty() && visitor.fields.is_empty() {
            return;
        }

        // Create log entry
        let entry = LogEntry {
            id: LOG_ID_COUNTER.fetch_add(1, Ordering::SeqCst),
            timestamp: chrono::Utc::now().timestamp_millis(),
            level: level.to_string(),
            target: metadata.target().to_string(),
            message,
            fields: visitor.fields,
        };

        // Add to buffer
        {
            let mut buffer = get_log_buffer().write();
            if buffer.len() >= MAX_BUFFER_SIZE {
                buffer.pop_front();
            }
            buffer.push_back(entry.clone());
        }

        // Emit to frontend
        if let Some(handle) = APP_HANDLE.get() {
            let _ = handle.emit("log-event", entry);
        }
    }
}

// ============================================================================
// Tauri Commands
// ============================================================================

#[tauri::command]
pub fn enable_debug_console() {
    enable_log_bridge();
}

#[tauri::command]
pub fn disable_debug_console() {
    disable_log_bridge();
}

#[tauri::command]
pub fn is_debug_console_enabled() -> bool {
    is_log_bridge_enabled()
}

#[tauri::command]
pub fn get_debug_console_logs() -> Vec<LogEntry> {
    get_buffered_logs()
}

#[tauri::command]
pub fn clear_debug_console_logs() {
    clear_log_buffer();
}
