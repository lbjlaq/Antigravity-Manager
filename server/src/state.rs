//! 应用状态

use std::sync::Arc;
use crate::modules::AccountManager;
use crate::modules::proxy::TokenManager;
use tokio::sync::RwLock;
use std::collections::HashMap;

pub struct AppState {
    pub api_key: String,
    pub account_manager: Arc<AccountManager>,
    // Proxy related
    pub token_manager: Arc<TokenManager>,
    pub anthropic_mapping: Arc<RwLock<HashMap<String, String>>>,
    pub request_timeout: u64,
    pub thought_signature_map: Arc<tokio::sync::Mutex<HashMap<String, String>>>,
    pub upstream_proxy: Arc<RwLock<crate::modules::proxy::config::UpstreamProxyConfig>>,
    pub proxy_enabled: Arc<std::sync::atomic::AtomicBool>,
}

impl AppState {
    pub async fn new(api_key: String) -> Self {
        let account_manager = Arc::new(AccountManager::new().await);
        
        // Proxy initialization
        let data_dir = crate::modules::account::get_data_dir().unwrap_or(std::path::PathBuf::from(".antigravity_tools"));
        tracing::info!("数据目录: {:?}", data_dir);
        let token_manager = Arc::new(TokenManager::new(data_dir.clone()));
        
        // 从 AccountManager 同步账号到 TokenManager
        match account_manager.list_accounts().await {
            Ok(accounts) => {
                tracing::info!("AccountManager 加载了 {} 个账号", accounts.len());
                match token_manager.sync_from_account_manager(&accounts).await {
                    Ok(count) => {
                        tracing::info!("✅ TokenManager 初始化完成，同步了 {} 个账号", count);
                    },
                    Err(e) => {
                        tracing::error!("❌ TokenManager 同步账号失败: {}", e);
                    }
                }
            },
            Err(e) => {
                tracing::error!("❌ 无法从 AccountManager 获取账号列表: {}", e);
            }
        }

        let anthropic_mapping = Arc::new(RwLock::new(HashMap::new()));
        let upstream_proxy = Arc::new(RwLock::new(crate::modules::proxy::config::UpstreamProxyConfig::default()));
        let thought_signature_map = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let proxy_enabled = Arc::new(std::sync::atomic::AtomicBool::new(true)); // 默认开启
        
        // Load initial config if exists
        if let Ok(config) = crate::modules::load_app_config() {
            // config.proxy 是直接结构体，不是 Option
            // 直接赋值
            *anthropic_mapping.write().await = config.proxy.anthropic_mapping;
            *upstream_proxy.write().await = config.proxy.upstream_proxy;
        }

        Self {
            api_key,
            account_manager,
            token_manager,
            anthropic_mapping,
            request_timeout: 120, // Default timeout
            thought_signature_map,
            upstream_proxy,
            proxy_enabled,
        }
    }
}
