use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast;

/// 日志条目
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub ts_ms: u64,
    pub kind: String,       // "proxy", "system", "error"
    pub level: Option<String>,
    pub message: Option<String>,
    // Proxy specific fields
    pub method: Option<String>,
    pub path: Option<String>,
    pub status: Option<u16>,
    pub duration_ms: Option<u64>,
    pub upstream: Option<String>,
}

impl LogEntry {
    pub fn proxy(method: &str, path: &str, status: u16, duration_ms: u64, upstream: Option<&str>) -> Self {
        Self {
            ts_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            kind: "proxy".to_string(),
            level: None,
            message: None,
            method: Some(method.to_string()),
            path: Some(path.to_string()),
            status: Some(status),
            duration_ms: Some(duration_ms),
            upstream: upstream.map(|s| s.to_string()),
        }
    }

    pub fn system(level: &str, message: &str) -> Self {
        Self {
            ts_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            kind: "system".to_string(),
            level: Some(level.to_string()),
            message: Some(message.to_string()),
            method: None,
            path: None,
            status: None,
            duration_ms: None,
            upstream: None,
        }
    }
}

/// 日志广播器
pub struct LogBroadcaster {
    sender: broadcast::Sender<LogEntry>,
}

impl Default for LogBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

impl LogBroadcaster {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self { sender }
    }

    /// 发送日志
    pub fn send(&self, entry: LogEntry) {
        let _ = self.sender.send(entry);
    }

    /// 订阅日志流
    pub fn subscribe(&self) -> broadcast::Receiver<LogEntry> {
        self.sender.subscribe()
    }
}

/// 全局日志广播实例
static LOG_BROADCASTER: once_cell::sync::Lazy<Arc<LogBroadcaster>> =
    once_cell::sync::Lazy::new(|| Arc::new(LogBroadcaster::new()));

pub fn global_log_broadcaster() -> Arc<LogBroadcaster> {
    LOG_BROADCASTER.clone()
}

/// 便捷函数：发送代理日志
pub fn emit_proxy_log(method: &str, path: &str, status: u16, duration_ms: u64, upstream: Option<&str>) {
    global_log_broadcaster().send(LogEntry::proxy(method, path, status, duration_ms, upstream));
}

/// 便捷函数：发送系统日志
pub fn emit_system_log(level: &str, message: &str) {
    global_log_broadcaster().send(LogEntry::system(level, message));
}
