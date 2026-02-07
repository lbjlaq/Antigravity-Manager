pub mod account;
pub mod quota;
pub mod config;
pub mod logger;
pub mod db;
pub mod process;
pub mod version;
pub mod oauth;
pub mod oauth_server;
pub mod migration;
pub mod tray;
pub mod i18n;
pub mod proxy_db;
pub mod device;
pub mod update_checker;
pub mod scheduler;
pub mod token_stats;
pub mod cloudflared;
pub mod integration;
pub mod account_service;
pub mod http_api;
pub mod cache; // [NEW] Antigravity cache clearing module
pub mod log_bridge; // [NEW] Debug console log bridge
pub mod security_db; // [NEW] IP security management (blacklist/whitelist)

use crate::models;

// Re-export commonly used functions to the top level of the modules namespace for easy external calling
pub use account::*;
#[allow(unused_imports)]
pub use quota::*;
pub use config::*;
#[allow(unused_imports)]
pub use logger::*;
// pub use device::*;

pub async fn fetch_quota(access_token: &str, email: &str) -> crate::error::AppResult<(models::QuotaData, Option<String>)> {
    quota::fetch_quota(access_token, email).await
}
