use std::sync::Arc;
use tokio::sync::RwLock;
use crate::proxy::{ProxyConfig, TokenManager, AxumServer, ProxySecurityConfig, ZaiDispatchMode};
use crate::proxy::monitor::{ProxyMonitor, ProxyRequestLog, ProxyStats};
use crate::modules::account;
#[cfg(feature = "ui")]
use tauri::AppHandle; // Optional dependency for monitor

/// 反代服务逻辑封装
pub struct ProxyService {
    pub instance: Arc<RwLock<Option<ProxyServiceInstance>>>,
    pub monitor: Arc<RwLock<Option<Arc<ProxyMonitor>>>>,
}

/// 反代服务实例
pub struct ProxyServiceInstance {
    pub config: ProxyConfig,
    pub token_manager: Arc<TokenManager>,
    pub axum_server: AxumServer,
    pub server_handle: tokio::task::JoinHandle<()>,
}

/// 反代服务状态 (DTO)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProxyStatus {
    pub running: bool,
    pub port: u16,
    pub base_url: String,
    pub active_accounts: usize,
}

impl ProxyService {
    pub fn new() -> Self {
        Self {
            instance: Arc::new(RwLock::new(None)),
            monitor: Arc::new(RwLock::new(None)),
        }
    }

    /// 启动服务 (UI Enabled)
    #[cfg(feature = "ui")]
    pub async fn start(
        &self,
        config: ProxyConfig,
        app_handle: Option<AppHandle>,
    ) -> Result<ProxyStatus, String> {
        self._start_common(config, app_handle).await
    }

    /// 启动服务 (Headless)
    #[cfg(not(feature = "ui"))]
    pub async fn start(
        &self,
        config: ProxyConfig,
        _app_handle: Option<()>,
    ) -> Result<ProxyStatus, String> {
        self._start_common(config, None).await
    }
}

// Separate implementation block for UI
#[cfg(feature = "ui")]
impl ProxyService {
    async fn _start_common(
        &self,
        config: ProxyConfig,
        app_handle: Option<AppHandle>,
    ) -> Result<ProxyStatus, String> {
        let instance_lock = self.instance.write().await;
        if instance_lock.is_some() { return Err("服务已在运行中".to_string()); }

        {
            let mut monitor_lock = self.monitor.write().await;
            if monitor_lock.is_none() {
                *monitor_lock = Some(Arc::new(ProxyMonitor::new(1000, app_handle.clone())));
            }
            if let Some(monitor) = monitor_lock.as_ref() {
                monitor.set_enabled(config.enable_logging);
            }
        }
        
        self._finish_start(config, instance_lock).await
    }
}

// Separate implementation block for Headless
#[cfg(not(feature = "ui"))]
impl ProxyService {
    async fn _start_common(
        &self,
        config: ProxyConfig,
        _app_handle: Option<()>,
    ) -> Result<ProxyStatus, String> {
        let instance_lock = self.instance.write().await;
        if instance_lock.is_some() { return Err("服务已在运行中".to_string()); }

        {
            let mut monitor_lock = self.monitor.write().await;
            if monitor_lock.is_none() {
                // Pass None to ProxyMonitor::new which expects Option<()> in headless
                *monitor_lock = Some(Arc::new(ProxyMonitor::new(1000, None)));
            }
            if let Some(monitor) = monitor_lock.as_ref() {
                monitor.set_enabled(config.enable_logging);
            }
        }
        
        self._finish_start(config, instance_lock).await
    }
}

// Common completion logic
impl ProxyService {
    async fn _finish_start(
        &self, 
        config: ProxyConfig, 
        mut instance_lock: tokio::sync::RwLockWriteGuard<'_, Option<ProxyServiceInstance>>
    ) -> Result<ProxyStatus, String> {
        let monitor = self.monitor.read().await.as_ref().unwrap().clone();
        
        // 2. 初始化 Token 管理器
        let app_data_dir = account::get_data_dir()?;
        // Ensure accounts dir exists
        let _ = account::get_accounts_dir()?;
        
        let token_manager = Arc::new(TokenManager::new(app_data_dir));
        // 同步 UI 传递的调度配置
        token_manager.update_sticky_config(config.scheduling.clone()).await;
        
        // 3. 加载账号
        let active_accounts = token_manager.load_accounts().await
            .map_err(|e| format!("加载账号失败: {}", e))?;
        
        if active_accounts == 0 {
            let zai_enabled = config.zai.enabled
                && !matches!(config.zai.dispatch_mode, ZaiDispatchMode::Off);
            if !zai_enabled {
                return Err("没有可用账号，请先添加账号".to_string());
            }
        }
        
        // 启动 Axum 服务器
        let (axum_server, server_handle) =
            match AxumServer::start(
                config.get_bind_address().to_string(),
                config.port,
                token_manager.clone(),
                config.anthropic_mapping.clone(),
                config.openai_mapping.clone(),
                config.custom_mapping.clone(),
                config.model_strategies.clone(),
                config.request_timeout,
                config.upstream_proxy.clone(),
                ProxySecurityConfig::from_proxy_config(&config),
                config.zai.clone(),
                monitor.clone(),
                config.experimental.clone(),
            ).await {
                Ok((server, handle)) => (server, handle),
                Err(e) => return Err(format!("启动 Axum 服务器失败: {}", e)),
            };
        
        // 创建服务实例
        let instance = ProxyServiceInstance {
            config: config.clone(),
            token_manager: token_manager.clone(), // Clone for ProxyServiceInstance
            axum_server,
            server_handle,
        };
        
        *instance_lock = Some(instance);
        
        // 保存配置到全局 AppConfig (Optional: service maybe shouldn't touch global config file directly? 
        // But for consistency with current behavior, we do it here or let CLI/UI do it. 
        // Let's keep it here for now as it persists the "last running config" state effectively)
        let mut app_config = crate::modules::config::load_app_config().map_err(|e| e)?;
        app_config.proxy = config.clone();
        crate::modules::config::save_app_config(&app_config).map_err(|e| e)?;
        
        Ok(ProxyStatus {
            running: true,
            port: config.port,
            base_url: format!("http://127.0.0.1:{}", config.port),
            active_accounts,
        })
    }
    
    /// 停止服务
    pub async fn stop(&self) -> Result<(), String> {
        let mut instance_lock = self.instance.write().await;
        
        if instance_lock.is_none() {
            // Idempotent: if not running, return Ok or Err? 
            // Original code returns Err.
            return Err("服务未运行".to_string());
        }
        
        // 停止 Axum 服务器
        if let Some(instance) = instance_lock.take() {
            instance.axum_server.stop();
            // 等待服务器任务完成
            instance.server_handle.await.ok();
        }
        
        Ok(())
    }
    
    /// 获取当前状态
    pub async fn get_status(&self) -> ProxyStatus {
        let instance_lock = self.instance.read().await;
        
        match instance_lock.as_ref() {
            Some(instance) => ProxyStatus {
                running: true,
                port: instance.config.port,
                base_url: format!("http://127.0.0.1:{}", instance.config.port),
                active_accounts: instance.token_manager.len(),
            },
            None => ProxyStatus {
                running: false,
                port: 0,
                base_url: String::new(),
                active_accounts: 0,
            },
        }
    }

    /// 获取统计信息
    pub async fn get_stats(&self) -> ProxyStats {
        let monitor_lock = self.monitor.read().await;
        if let Some(monitor) = monitor_lock.as_ref() {
            monitor.get_stats().await
        } else {
            ProxyStats::default()
        }
    }
    
    /// 获取日志
    pub async fn get_logs(&self, limit: usize) -> Vec<ProxyRequestLog> {
         let monitor_lock = self.monitor.read().await;
         if let Some(monitor) = monitor_lock.as_ref() {
             monitor.get_logs(limit).await
         } else {
             Vec::new()
         }
    }

    /// 清理日志
    pub async fn clear_logs(&self) {
        let monitor_lock = self.monitor.read().await;
        if let Some(monitor) = monitor_lock.as_ref() {
            monitor.clear().await;
        }
    }
}
