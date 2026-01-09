use serde::{Serialize, Deserialize};
use std::collections::VecDeque;
use tokio::sync::RwLock;
#[cfg(feature = "ui")]
use tauri::Emitter;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyRequestLog {
// ... existing fields ...
    pub id: String,
    pub timestamp: i64,
    pub method: String,
    pub url: String,
    pub status: u16,
    pub duration: u64, // ms
    pub model: Option<String>,        // 客户端请求的模型名
    pub mapped_model: Option<String>, // 实际路由后使用的模型名
    pub account_email: Option<String>,
    pub error: Option<String>,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProxyStats {
    pub total_requests: u64,
    pub success_count: u64,
    pub error_count: u64,
}

pub struct ProxyMonitor {
    pub logs: RwLock<VecDeque<ProxyRequestLog>>,
    pub stats: RwLock<ProxyStats>,
    pub max_logs: usize,
    pub enabled: AtomicBool,
    #[cfg(feature = "ui")]
    app_handle: Option<tauri::AppHandle>,
}

impl ProxyMonitor {
    #[cfg(feature = "ui")]
    pub fn new(max_logs: usize, app_handle: Option<tauri::AppHandle>) -> Self {
        if let Err(e) = crate::modules::proxy_db::init_db() {
            tracing::error!("Failed to initialize proxy DB: {}", e);
        }

        Self {
            logs: RwLock::new(VecDeque::with_capacity(max_logs)),
            stats: RwLock::new(ProxyStats::default()),
            max_logs,
            enabled: AtomicBool::new(false),
            app_handle,
        }
    }
    
    #[cfg(not(feature = "ui"))]
    pub fn new(max_logs: usize, _app_handle: Option<()>) -> Self {
         if let Err(e) = crate::modules::proxy_db::init_db() {
            tracing::error!("Failed to initialize proxy DB: {}", e);
        }

        Self {
            logs: RwLock::new(VecDeque::with_capacity(max_logs)),
            stats: RwLock::new(ProxyStats::default()),
            max_logs,
            enabled: AtomicBool::new(false),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub async fn log_request(&self, log: ProxyRequestLog) {
        if !self.is_enabled() {
            return;
        }
        tracing::info!("[Monitor] Logging request: {} {}", log.method, log.url);
        
        {
            let mut stats = self.stats.write().await;
            stats.total_requests += 1;
            if log.status >= 200 && log.status < 400 {
                stats.success_count += 1;
            } else {
                stats.error_count += 1;
            }
        }

        {
            let mut logs = self.logs.write().await;
            if logs.len() >= self.max_logs {
                logs.pop_back();
            }
            logs.push_front(log.clone());
        }

        let log_to_save = log.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::modules::proxy_db::save_log(&log_to_save) {
                tracing::error!("Failed to save proxy log to DB: {}", e);
            }
        });

        #[cfg(feature = "ui")]
        if let Some(app) = &self.app_handle {
             let _ = app.emit("proxy://request", &log);
        }
    }

    pub async fn get_logs(&self, limit: usize) -> Vec<ProxyRequestLog> {
        match crate::modules::proxy_db::get_logs(limit) {
            Ok(logs) => logs,
            Err(e) => {
                tracing::error!("Failed to get logs from DB: {}", e);
                let logs = self.logs.read().await;
                logs.iter().take(limit).cloned().collect()
            }
        }
    }

    pub async fn get_stats(&self) -> ProxyStats {
        match crate::modules::proxy_db::get_stats() {
            Ok(stats) => stats,
            Err(e) => {
                tracing::error!("Failed to get stats from DB: {}", e);
                self.stats.read().await.clone()
            }
        }
    }
    
    pub async fn clear(&self) {
        let mut logs = self.logs.write().await;
        logs.clear();
        let mut stats = self.stats.write().await;
        *stats = ProxyStats::default();

        if let Err(e) = crate::modules::proxy_db::clear_logs() {
            tracing::error!("Failed to clear logs in DB: {}", e);
        }
    }
}