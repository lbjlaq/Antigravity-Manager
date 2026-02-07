//! Server types and response structures
//!
//! This module contains all shared types used across the admin API handlers.

use crate::proxy::TokenManager;
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Axum application state shared across all handlers
#[derive(Clone)]
pub struct AppState {
    pub token_manager: Arc<TokenManager>,
    pub custom_mapping: Arc<tokio::sync::RwLock<std::collections::HashMap<String, String>>>,
    #[allow(dead_code)]
    pub request_timeout: u64,
    #[allow(dead_code)]
    pub thought_signature_map:
        Arc<tokio::sync::Mutex<std::collections::HashMap<String, String>>>,
    #[allow(dead_code)]
    pub upstream_proxy: Arc<tokio::sync::RwLock<crate::proxy::config::UpstreamProxyConfig>>,
    pub upstream: Arc<crate::proxy::upstream::client::UpstreamClient>,
    pub zai: Arc<RwLock<crate::proxy::ZaiConfig>>,
    pub provider_rr: Arc<AtomicUsize>,
    pub zai_vision_mcp: Arc<crate::proxy::zai_vision_mcp::ZaiVisionMcpState>,
    pub monitor: Arc<crate::proxy::monitor::ProxyMonitor>,
    pub experimental: Arc<RwLock<crate::proxy::config::ExperimentalConfig>>,
    pub debug_logging: Arc<RwLock<crate::proxy::config::DebugLoggingConfig>>,
    pub switching: Arc<RwLock<bool>>,
    pub integration: crate::modules::integration::SystemManager,
    pub account_service: Arc<crate::modules::account_service::AccountService>,
    pub security: Arc<RwLock<crate::proxy::ProxySecurityConfig>>,
    pub cloudflared_state: Arc<crate::commands::cloudflared::CloudflaredState>,
    pub is_running: Arc<RwLock<bool>>,
    pub port: u16,
}

// Implement FromRef for security state extraction in middleware
impl axum::extract::FromRef<AppState> for Arc<RwLock<crate::proxy::ProxySecurityConfig>> {
    fn from_ref(state: &AppState) -> Self {
        state.security.clone()
    }
}

// ============================================================================
// Response Types
// ============================================================================

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Serialize)]
pub struct AccountResponse {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub is_current: bool,
    pub disabled: bool,
    pub disabled_reason: Option<String>,
    pub disabled_at: Option<i64>,
    pub proxy_disabled: bool,
    pub proxy_disabled_reason: Option<String>,
    pub proxy_disabled_at: Option<i64>,
    pub protected_models: Vec<String>,
    pub quota: Option<QuotaResponse>,
    pub device_bound: bool,
    pub last_used: i64,
}

#[derive(Serialize)]
pub struct QuotaResponse {
    pub models: Vec<ModelQuota>,
    pub last_updated: i64,
    pub subscription_tier: Option<String>,
    pub is_forbidden: bool,
}

#[derive(Serialize)]
pub struct ModelQuota {
    pub name: String,
    pub percentage: i32,
    pub reset_time: String,
}

#[derive(Serialize)]
pub struct AccountListResponse {
    pub accounts: Vec<AccountResponse>,
    pub current_account_id: Option<String>,
}

// ============================================================================
// Request Types
// ============================================================================

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddAccountRequest {
    pub refresh_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchRequest {
    pub account_id: String,
}

#[derive(Deserialize)]
pub struct BindDeviceRequest {
    #[serde(default = "default_bind_mode")]
    pub mode: String,
}

fn default_bind_mode() -> String {
    "generate".to_string()
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct LogsFilterQuery {
    #[serde(default)]
    pub filter: String,
    #[serde(default)]
    pub errors_only: bool,
    #[serde(default)]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveConfigWrapper {
    pub config: crate::models::AppConfig,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMappingWrapper {
    pub config: crate::proxy::config::ProxyConfig,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct StatsPeriodQuery {
    pub hours: Option<i64>,
    pub days: Option<i64>,
    pub weeks: Option<i64>,
}

#[derive(Deserialize)]
pub struct BulkDeleteRequest {
    #[serde(rename = "accountIds")]
    pub account_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderRequest {
    pub account_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToggleProxyRequest {
    pub enable: bool,
    pub reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveFileRequest {
    pub path: String,
    pub content: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflaredStartRequest {
    pub config: crate::modules::cloudflared::CloudflaredConfig,
}

#[derive(Deserialize)]
pub struct CustomDbRequest {
    pub path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSyncStatusRequest {
    pub app_type: crate::proxy::cli_sync::CliApp,
    pub proxy_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSyncRequest {
    pub app_type: crate::proxy::cli_sync::CliApp,
    pub proxy_url: String,
    pub api_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliRestoreRequest {
    pub app_type: crate::proxy::cli_sync::CliApp,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliConfigContentRequest {
    pub app_type: crate::proxy::cli_sync::CliApp,
    pub file_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeSyncStatusRequest {
    pub proxy_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeSyncRequest {
    pub proxy_url: String,
    pub api_key: String,
    #[serde(default)]
    pub sync_accounts: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeConfigContentRequest {
    pub file_name: Option<String>,
}

#[derive(Deserialize)]
pub struct SubmitCodeRequest {
    pub code: String,
    pub state: Option<String>,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert Account model to AccountResponse
pub fn to_account_response(
    account: &crate::models::account::Account,
    current_id: &Option<String>,
) -> AccountResponse {
    AccountResponse {
        id: account.id.clone(),
        email: account.email.clone(),
        name: account.name.clone(),
        is_current: current_id.as_ref() == Some(&account.id),
        disabled: account.disabled,
        disabled_reason: account.disabled_reason.clone(),
        disabled_at: account.disabled_at,
        proxy_disabled: account.proxy_disabled,
        proxy_disabled_reason: account.proxy_disabled_reason.clone(),
        proxy_disabled_at: account.proxy_disabled_at,
        protected_models: account.protected_models.iter().cloned().collect(),
        quota: account.quota.as_ref().map(|q| QuotaResponse {
            models: q
                .models
                .iter()
                .map(|m| ModelQuota {
                    name: m.name.clone(),
                    percentage: m.percentage,
                    reset_time: m.reset_time.clone(),
                })
                .collect(),
            last_updated: q.last_updated,
            subscription_tier: q.subscription_tier.clone(),
            is_forbidden: q.is_forbidden,
        }),
        device_bound: account.device_profile.is_some(),
        last_used: account.last_used,
    }
}
