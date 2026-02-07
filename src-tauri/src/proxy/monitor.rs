use serde::{Serialize, Deserialize};
use std::collections::VecDeque;
use tokio::sync::RwLock;
use tauri::Emitter;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyRequestLog {
    pub id: String,
    pub timestamp: i64,
    pub method: String,
    pub url: String,
    pub status: u16,
    pub duration: u64, // ms
    pub model: Option<String>,        // 客户端请求的模型名
    pub mapped_model: Option<String>, // 实际路由后使用的模型名
    pub account_email: Option<String>,
    pub client_ip: Option<String>,    // 客户端 IP 地址
    pub error: Option<String>,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub protocol: Option<String>,     // 协议类型: "openai", "anthropic", "gemini"
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProxyStats {
    pub total_requests: u64,
    pub success_count: u64,
    pub error_count: u64,
    pub avg_latency: f64, // [NEW] Average latency in ms
}

pub struct ProxyMonitor {
    pub logs: RwLock<VecDeque<ProxyRequestLog>>,
    pub stats: RwLock<ProxyStats>,
    pub max_logs: usize,
    pub enabled: AtomicBool,
    app_handle: Option<tauri::AppHandle>,
}

impl ProxyMonitor {
    pub fn new(max_logs: usize, app_handle: Option<tauri::AppHandle>) -> Self {
        // Initialize DB
        if let Err(e) = crate::modules::proxy_db::init_db() {
            tracing::error!("Failed to initialize proxy DB: {}", e);
        }

        // Auto cleanup old logs (keep last 30 days)
        tokio::spawn(async {
            match crate::modules::proxy_db::cleanup_old_logs(30) {
                Ok(deleted) => {
                    if deleted > 0 {
                        tracing::info!("Auto cleanup: removed {} old logs (>30 days)", deleted);
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to cleanup old logs: {}", e);
                }
            }
        });

        Self {
            logs: RwLock::new(VecDeque::with_capacity(max_logs)),
            stats: RwLock::new(ProxyStats::default()),
            max_logs,
            enabled: AtomicBool::new(false), // Default to disabled
            app_handle,
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub async fn log_request(&self, log: ProxyRequestLog) {
        // [OPTIMIZED] Removed redundant token stats recording here.
        // It is handled asynchronously along with duplicate DB logging below to prevent double-counting.

        if !self.is_enabled() {
            return;
        }
        tracing::info!("[Monitor] Logging request: {} {}", log.method, log.url);
        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_requests += 1;
            if log.status >= 200 && log.status < 400 {
                stats.success_count += 1;
            } else {
                stats.error_count += 1;
            }
        }

        // Add log to memory
        {
            let mut logs = self.logs.write().await;
            if logs.len() >= self.max_logs {
                logs.pop_back();
            }
            logs.push_front(log.clone());
        }

        // Save to DB
        let log_to_save = log.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::modules::proxy_db::save_log(&log_to_save) {
                tracing::error!("Failed to save proxy log to DB: {}", e);
            }

            // [FIX] Sync to Security DB (access_log) so it appears in Security Monitor
            if let Some(ip) = &log_to_save.client_ip {
                if let Err(e) = crate::modules::security_db::log_access(
                    ip,
                    &log_to_save.url,
                    &log_to_save.method,
                    log_to_save.status as i32,
                    false, // Not blocked (this comes from monitor, so it wasn't blocked by IP filter)
                    None,  // No block reason
                    None,  // We don't have UA easily accessible here
                ) {
                    tracing::error!("Failed to save security log: {}", e);
                }
            }
            
            // Record token stats if available
            if let (Some(account), Some(input), Some(output)) = (
                &log_to_save.account_email,
                log_to_save.input_tokens,
                log_to_save.output_tokens,
            ) {
                let model = log_to_save.model.clone().unwrap_or_else(|| "unknown".to_string());
                if let Err(e) = crate::modules::token_stats::record_usage(account, &model, input, output) {
                    tracing::debug!("Failed to record token stats: {}", e);
                }
            }
        });

        // Emit event (send summary only, without body to reduce memory)
        if let Some(app) = &self.app_handle {
            let log_summary = ProxyRequestLog {
                id: log.id.clone(),
                timestamp: log.timestamp,
                method: log.method.clone(),
                url: log.url.clone(),
                status: log.status,
                duration: log.duration,
                model: log.model.clone(),
                mapped_model: log.mapped_model.clone(),
                account_email: log.account_email.clone(),
                client_ip: log.client_ip.clone(),
                error: log.error.clone(),
                request_body: None,  // Don't send body in event
                response_body: None, // Don't send body in event
                input_tokens: log.input_tokens,
                output_tokens: log.output_tokens,
                protocol: log.protocol.clone(),
            };
            let _ = app.emit("proxy://request", &log_summary);
        }
    }

    pub async fn get_logs(&self, limit: usize) -> Vec<ProxyRequestLog> {
        // Try to get from DB first for true history
        let db_result = tokio::task::spawn_blocking(move || {
            crate::modules::proxy_db::get_logs(limit)
        }).await;

        match db_result {
            Ok(Ok(logs)) => logs,
            Ok(Err(e)) => {
                tracing::error!("Failed to get logs from DB: {}", e);
                // Fallback to memory
                let logs = self.logs.read().await;
                logs.iter().take(limit).cloned().collect()
            }
            Err(e) => {
                tracing::error!("Spawn blocking failed for get_logs: {}", e);
                let logs = self.logs.read().await;
                logs.iter().take(limit).cloned().collect()
            }
        }
    }

    pub async fn get_stats(&self) -> ProxyStats {
        let db_result = tokio::task::spawn_blocking(|| {
            crate::modules::proxy_db::get_stats()
        }).await;

        match db_result {
            Ok(Ok(stats)) => stats,
            Ok(Err(e)) => {
                tracing::error!("Failed to get stats from DB: {}", e);
                self.stats.read().await.clone()
            }
            Err(e) => {
                tracing::error!("Spawn blocking failed for get_stats: {}", e);
                self.stats.read().await.clone()
            }
        }
    }
    
    pub async fn get_logs_filtered(
        &self,
        page: usize,
        page_size: usize,
        search_text: Option<String>,
        level: Option<String>,
    ) -> Result<Vec<ProxyRequestLog>, String> {
        let offset = (page.max(1) - 1) * page_size;
        let errors_only = level.as_deref() == Some("error");
        let search = search_text.unwrap_or_default();

        let res = tokio::task::spawn_blocking(move || {
            crate::modules::proxy_db::get_logs_filtered(&search, errors_only, page_size, offset)
        }).await;

        match res {
            Ok(r) => r,
            Err(e) => Err(format!("Spawn blocking failed: {}", e)),
        }
    }
    
    pub async fn clear(&self) {
        let mut logs = self.logs.write().await;
        logs.clear();
        let mut stats = self.stats.write().await;
        *stats = ProxyStats::default();

        let _ = tokio::task::spawn_blocking(|| {
            if let Err(e) = crate::modules::proxy_db::clear_logs() {
                tracing::error!("Failed to clear logs in DB: {}", e);
            }
        }).await;
    }
}