use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{OnceLock, RwLock};

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";

const OAUTH_CLIENTS_ENV: &str = "ANTIGRAVITY_OAUTH_CLIENTS";
const ACTIVE_OAUTH_CLIENT_ENV: &str = "ANTIGRAVITY_OAUTH_CLIENT_KEY";
const DEFAULT_OAUTH_CLIENT_KEY: &str = "antigravity_enterprise";
const DEFAULT_OAUTH_CLIENT_ID_ENV: &str = "ANTIGRAVITY_OAUTH_CLIENT_ID";
const DEFAULT_OAUTH_CLIENT_SECRET_ENV: &str = "ANTIGRAVITY_OAUTH_CLIENT_SECRET";
const DEFAULT_OAUTH_CLIENT_LABEL: &str = "Antigravity Enterprise";

#[derive(Debug, Clone)]
struct OAuthClientConfig {
    key: String,
    label: String,
    client_id: String,
    client_secret: String,
    is_builtin: bool,
}

#[derive(Debug, Clone)]
struct OAuthClientRegistry {
    clients: Vec<OAuthClientConfig>,
    active_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClientDescriptor {
    pub key: String,
    pub label: String,
    pub client_id: String,
    pub is_active: bool,
    pub is_builtin: bool,
}

static OAUTH_CLIENT_REGISTRY: OnceLock<RwLock<OAuthClientRegistry>> = OnceLock::new();

fn normalize_client_key(key: &str) -> String {
    key.trim().to_ascii_lowercase()
}

fn read_non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn load_default_client_from_env() -> Option<OAuthClientConfig> {
    let client_id = read_non_empty_env(DEFAULT_OAUTH_CLIENT_ID_ENV);
    let client_secret = read_non_empty_env(DEFAULT_OAUTH_CLIENT_SECRET_ENV);

    match (client_id, client_secret) {
        (Some(client_id), Some(client_secret)) => Some(OAuthClientConfig {
            key: normalize_client_key(DEFAULT_OAUTH_CLIENT_KEY),
            label: DEFAULT_OAUTH_CLIENT_LABEL.to_string(),
            client_id,
            client_secret,
            is_builtin: true,
        }),
        (None, None) => {
            crate::modules::logger::log_warn(&format!(
                "Default OAuth client not configured. Set {} and {} to enable built-in OAuth.",
                DEFAULT_OAUTH_CLIENT_ID_ENV, DEFAULT_OAUTH_CLIENT_SECRET_ENV
            ));
            None
        }
        _ => {
            crate::modules::logger::log_warn(&format!(
                "Default OAuth client configuration is incomplete. Both {} and {} are required.",
                DEFAULT_OAUTH_CLIENT_ID_ENV, DEFAULT_OAUTH_CLIENT_SECRET_ENV
            ));
            None
        }
    }
}

fn build_registry() -> OAuthClientRegistry {
    let mut clients: Vec<OAuthClientConfig> = Vec::new();

    if let Some(default_client) = load_default_client_from_env() {
        clients.push(default_client);
    }

    if let Ok(raw_extra_clients) = std::env::var(OAUTH_CLIENTS_ENV) {
        for entry in raw_extra_clients.split(';') {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Expected format: key|client_id|client_secret|optional_label
            let parts: Vec<&str> = trimmed.split('|').map(|v| v.trim()).collect();
            if parts.len() < 3 {
                crate::modules::logger::log_warn(&format!(
                    "Ignored invalid OAuth client entry in {}: {}",
                    OAUTH_CLIENTS_ENV, trimmed
                ));
                continue;
            }

            let key = normalize_client_key(parts[0]);
            if key.is_empty() || parts[1].is_empty() || parts[2].is_empty() {
                crate::modules::logger::log_warn(&format!(
                    "Ignored incomplete OAuth client entry in {}: {}",
                    OAUTH_CLIENTS_ENV, trimmed
                ));
                continue;
            }

            let label = if parts.len() >= 4 && !parts[3].is_empty() {
                parts[3].to_string()
            } else {
                key.clone()
            };

            let custom_client = OAuthClientConfig {
                key: key.clone(),
                label,
                client_id: parts[1].to_string(),
                client_secret: parts[2].to_string(),
                is_builtin: false,
            };

            if let Some(existing_index) = clients.iter().position(|c| c.key == key) {
                clients[existing_index] = custom_client;
                crate::modules::logger::log_info(&format!(
                    "OAuth client '{}' overridden by {}",
                    key, OAUTH_CLIENTS_ENV
                ));
            } else {
                clients.push(custom_client);
                crate::modules::logger::log_info(&format!(
                    "OAuth client '{}' loaded from {}",
                    key, OAUTH_CLIENTS_ENV
                ));
            }
        }
    }

    let mut active_key = std::env::var(ACTIVE_OAUTH_CLIENT_ENV)
        .ok()
        .map(|v| normalize_client_key(&v))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| normalize_client_key(DEFAULT_OAUTH_CLIENT_KEY));

    if !clients.iter().any(|c| c.key == active_key) {
        active_key = clients
            .first()
            .map(|c| c.key.clone())
            .unwrap_or_else(|| normalize_client_key(DEFAULT_OAUTH_CLIENT_KEY));
    }

    OAuthClientRegistry {
        clients,
        active_key,
    }
}

fn oauth_registry() -> &'static RwLock<OAuthClientRegistry> {
    OAUTH_CLIENT_REGISTRY.get_or_init(|| RwLock::new(build_registry()))
}

fn get_client_by_key<'a>(
    clients: &'a [OAuthClientConfig],
    client_key: &str,
) -> Option<&'a OAuthClientConfig> {
    let normalized = normalize_client_key(client_key);
    clients.iter().find(|c| c.key == normalized)
}

fn active_or_first_client(registry: &OAuthClientRegistry) -> Option<OAuthClientConfig> {
    if let Some(active) = get_client_by_key(&registry.clients, &registry.active_key) {
        return Some(active.clone());
    }
    registry.clients.first().cloned()
}

fn select_auth_client(client_key: Option<&str>) -> Result<OAuthClientConfig, String> {
    let registry_guard = oauth_registry().read().map_err(|e| e.to_string())?;
    let registry = &*registry_guard;

    if registry.clients.is_empty() {
        return Err("No OAuth clients configured".to_string());
    }

    if let Some(key) = client_key {
        if let Some(client) = get_client_by_key(&registry.clients, key) {
            return Ok(client.clone());
        }
        return Err(format!("Unknown OAuth client key: {}", key));
    }

    active_or_first_client(registry).ok_or_else(|| "No OAuth clients configured".to_string())
}

fn get_candidate_clients(preferred_client_key: Option<&str>) -> Vec<OAuthClientConfig> {
    let registry_guard = match oauth_registry().read() {
        Ok(guard) => guard,
        Err(_) => return vec![],
    };
    let registry = &*registry_guard;

    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    let mut push_candidate = |client: &OAuthClientConfig| {
        if seen.insert(client.key.clone()) {
            candidates.push(client.clone());
        }
    };

    if let Some(preferred_key) = preferred_client_key {
        if let Some(preferred) = get_client_by_key(&registry.clients, preferred_key) {
            push_candidate(preferred);
        } else {
            crate::modules::logger::log_warn(&format!(
                "Preferred OAuth client '{}' not found; fallback to active client list",
                preferred_key
            ));
        }
    }

    if let Some(active) = get_client_by_key(&registry.clients, &registry.active_key) {
        push_candidate(active);
    }

    for client in &registry.clients {
        push_candidate(client);
    }

    candidates
}

fn is_client_mismatch_error(status: reqwest::StatusCode, error_text: &str) -> bool {
    let text = error_text.to_ascii_lowercase();
    status == reqwest::StatusCode::BAD_REQUEST
        || status == reqwest::StatusCode::UNAUTHORIZED
        || status == reqwest::StatusCode::FORBIDDEN
        || text.contains("unauthorized_client")
        || text.contains("invalid_client")
}

pub fn list_oauth_clients() -> Result<Vec<OAuthClientDescriptor>, String> {
    let registry_guard = oauth_registry().read().map_err(|e| e.to_string())?;
    let registry = &*registry_guard;

    Ok(registry
        .clients
        .iter()
        .map(|client| OAuthClientDescriptor {
            key: client.key.clone(),
            label: client.label.clone(),
            client_id: client.client_id.clone(),
            is_active: client.key == registry.active_key,
            is_builtin: client.is_builtin,
        })
        .collect())
}

pub fn get_active_oauth_client_key() -> Result<String, String> {
    let registry_guard = oauth_registry().read().map_err(|e| e.to_string())?;
    Ok(registry_guard.active_key.clone())
}

pub fn set_active_oauth_client_key(client_key: &str) -> Result<(), String> {
    let mut registry_guard = oauth_registry().write().map_err(|e| e.to_string())?;
    let normalized = normalize_client_key(client_key);

    if get_client_by_key(&registry_guard.clients, &normalized).is_none() {
        let available = registry_guard
            .clients
            .iter()
            .map(|c| c.key.clone())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "Unknown OAuth client key '{}'. Available: {}",
            client_key, available
        ));
    }

    registry_guard.active_key = normalized.clone();
    crate::modules::logger::log_info(&format!("Active OAuth client switched to '{}'", normalized));
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: i64,
    #[serde(default)]
    pub token_type: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(skip)]
    pub oauth_client_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserInfo {
    pub email: String,
    pub name: Option<String>,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub picture: Option<String>,
}

impl UserInfo {
    /// Get best display name
    pub fn get_display_name(&self) -> Option<String> {
        // Prefer name
        if let Some(name) = &self.name {
            if !name.trim().is_empty() {
                return Some(name.clone());
            }
        }
        
        // If name is empty, combine given_name and family_name
        match (&self.given_name, &self.family_name) {
            (Some(given), Some(family)) => Some(format!("{} {}", given, family)),
            (Some(given), None) => Some(given.clone()),
            (None, Some(family)) => Some(family.clone()),
            (None, None) => None,
        }
    }
}

/// Generate OAuth authorization URL with optional client selection.
/// Returns (auth_url, resolved_client_key).
pub fn get_auth_url_with_client(
    redirect_uri: &str,
    state: &str,
    client_key: Option<&str>,
) -> Result<(String, String), String> {
    let client = select_auth_client(client_key)?;

    let scopes = vec![
        "https://www.googleapis.com/auth/cloud-platform",
        "https://www.googleapis.com/auth/userinfo.email",
        "https://www.googleapis.com/auth/userinfo.profile",
        "https://www.googleapis.com/auth/cclog",
        "https://www.googleapis.com/auth/experimentsandconfigs",
    ]
    .join(" ");

    let params = vec![
        ("client_id", client.client_id.as_str()),
        ("redirect_uri", redirect_uri),
        ("response_type", "code"),
        ("scope", &scopes),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("include_granted_scopes", "true"),
        ("state", state),
    ];
    
    let url = url::Url::parse_with_params(AUTH_URL, &params)
        .map_err(|e| format!("Invalid Auth URL: {}", e))?;
    Ok((url.to_string(), client.key))
}

/// Generate OAuth authorization URL using current active client.
pub fn get_auth_url(redirect_uri: &str, state: &str) -> String {
    get_auth_url_with_client(redirect_uri, state, None)
        .map(|(url, _)| url)
        .expect("Failed to build OAuth URL")
}

async fn exchange_code_once(
    code: &str,
    redirect_uri: &str,
    client_cfg: &OAuthClientConfig,
) -> Result<TokenResponse, (Option<reqwest::StatusCode>, String)> {
    // [PHASE 2] 对于登录行为，尚未有 account_id，使用全局池阶梯逻辑
    let client = if let Some(pool) = crate::proxy::proxy_pool::get_global_proxy_pool() {
        pool.get_effective_standard_client(None, 60).await
    } else {
        crate::utils::http::get_long_standard_client()
    };
    
    let params = [
        ("client_id", client_cfg.client_id.as_str()),
        ("client_secret", client_cfg.client_secret.as_str()),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
    ];

    tracing::debug!(
        "[OAuth] Sending exchange_code request with User-Agent: {}",
        crate::constants::NATIVE_OAUTH_USER_AGENT.as_str()
    );

    let response = client
        .post(TOKEN_URL)
        .header(rquest::header::USER_AGENT, crate::constants::NATIVE_OAUTH_USER_AGENT.as_str())
        .form(&params)
        .send()
        .await
        .map_err(|e| {
            if e.is_connect() || e.is_timeout() {
                (
                    None,
                    format!(
                        "Token exchange request failed: {}. 请检查你的网络代理设置，确保可以稳定连接 Google 服务。",
                        e
                    ),
                )
            } else {
                (None, format!("Token exchange request failed: {}", e))
            }
        })?;

    if response.status().is_success() {
        let mut token_res = response
            .json::<TokenResponse>()
            .await
            .map_err(|e| (None, format!("Token parsing failed: {}", e)))?;
        token_res.oauth_client_key = Some(client_cfg.key.clone());

        // Add detailed logs
        crate::modules::logger::log_info(&format!(
            "Token exchange successful via [{}]! access_token: {}..., refresh_token: {}",
            client_cfg.key,
            &token_res.access_token.chars().take(20).collect::<String>(),
            if token_res.refresh_token.is_some() {
                "✓"
            } else {
                "✗ Missing"
            }
        ));
        
        // Log warning if refresh_token is missing
        if token_res.refresh_token.is_none() {
            crate::modules::logger::log_warn(
                "Warning: Google did not return a refresh_token. Potential reasons:\n\
                 1. User has previously authorized this application\n\
                 2. Need to revoke access in Google Cloud Console and retry\n\
                 3. OAuth parameter configuration issue",
            );
        }
        
        Ok(token_res)
    } else {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        Err((
            Some(status),
            format!("Token exchange failed: {}", error_text),
        ))
    }
}

/// Exchange authorization code for token using optional preferred client.
/// When preferred/active client mismatches, fallback to other configured clients.
pub async fn exchange_code_with_client(
    code: &str,
    redirect_uri: &str,
    preferred_client_key: Option<&str>,
) -> Result<TokenResponse, String> {
    let candidates = get_candidate_clients(preferred_client_key);
    if candidates.is_empty() {
        return Err("No OAuth clients configured".to_string());
    }

    let mut attempt_errors: Vec<String> = Vec::new();

    for (idx, client_cfg) in candidates.iter().enumerate() {
        match exchange_code_once(code, redirect_uri, client_cfg).await {
            Ok(token_res) => {
                if idx > 0 {
                    crate::modules::logger::log_info(&format!(
                        "OAuth code exchange recovered via fallback client [{}]",
                        client_cfg.key
                    ));
                }
                return Ok(token_res);
            }
            Err((status_opt, err_msg)) => {
                let should_fallback = status_opt
                    .map(|status| is_client_mismatch_error(status, &err_msg))
                    .unwrap_or(false);

                attempt_errors.push(format!("{} => {}", client_cfg.key, err_msg));

                if should_fallback {
                    crate::modules::logger::log_warn(&format!(
                        "OAuth code exchange failed for client [{}], trying next client: {}",
                        client_cfg.key, err_msg
                    ));
                    continue;
                }

                return Err(format!(
                    "Token exchange failed for client [{}]: {}",
                    client_cfg.key, err_msg
                ));
            }
        }
    }

    Err(format!(
        "Token exchange failed for all OAuth clients: {}",
        attempt_errors.join(" | ")
    ))
}

/// Exchange authorization code for token
pub async fn exchange_code(code: &str, redirect_uri: &str) -> Result<TokenResponse, String> {
    exchange_code_with_client(code, redirect_uri, None).await
}

async fn refresh_access_token_once(
    refresh_token: &str,
    account_id: Option<&str>,
    client_cfg: &OAuthClientConfig,
) -> Result<TokenResponse, (Option<reqwest::StatusCode>, String)> {
    // [PHASE 2] 根据 account_id 使用对应的代理
    let client = if let Some(pool) = crate::proxy::proxy_pool::get_global_proxy_pool() {
        pool.get_effective_standard_client(account_id, 60).await
    } else {
        crate::utils::http::get_long_standard_client()
    };
    
    let params = [
        ("client_id", client_cfg.client_id.as_str()),
        ("client_secret", client_cfg.client_secret.as_str()),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];

    // [FIX #1583] 提供更详细的日志，帮助诊断 Docker 环境下的代理问题
    if let Some(id) = account_id {
        crate::modules::logger::log_info(&format!("Refreshing Token for account: {}...", id));
    } else {
        crate::modules::logger::log_info("Refreshing Token for generic request (no account_id)...");
    }
    
    tracing::debug!(
        "[OAuth] Sending refresh_access_token request with User-Agent: {}",
        crate::constants::NATIVE_OAUTH_USER_AGENT.as_str()
    );

    let response = client
        .post(TOKEN_URL)
        .header(rquest::header::USER_AGENT, crate::constants::NATIVE_OAUTH_USER_AGENT.as_str())
        .form(&params)
        .send()
        .await
        .map_err(|e| {
            if e.is_connect() || e.is_timeout() {
                (
                    None,
                    format!(
                        "Refresh request failed: {}. 无法连接 Google 授权服务器，请检查代理设置。",
                        e
                    ),
                )
            } else {
                (None, format!("Refresh request failed: {}", e))
            }
        })?;

    if response.status().is_success() {
        let mut token_data = response
            .json::<TokenResponse>()
            .await
            .map_err(|e| (None, format!("Refresh data parsing failed: {}", e)))?;
        token_data.oauth_client_key = Some(client_cfg.key.clone());

        crate::modules::logger::log_info(&format!(
            "Token refreshed successfully via [{}]! Expires in: {} seconds",
            client_cfg.key, token_data.expires_in
        ));
        Ok(token_data)
    } else {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        Err((Some(status), format!("Refresh failed: {}", error_text)))
    }
}

/// Refresh access_token using refresh_token with optional preferred OAuth client key.
/// If client mismatch occurs, it retries with other configured clients.
pub async fn refresh_access_token_with_client(
    refresh_token: &str,
    account_id: Option<&str>,
    preferred_client_key: Option<&str>,
) -> Result<TokenResponse, String> {
    let candidates = get_candidate_clients(preferred_client_key);
    if candidates.is_empty() {
        return Err("No OAuth clients configured".to_string());
    }

    let mut attempt_errors: Vec<String> = Vec::new();

    for (idx, client_cfg) in candidates.iter().enumerate() {
        match refresh_access_token_once(refresh_token, account_id, client_cfg).await {
            Ok(token_res) => {
                if idx > 0 {
                    crate::modules::logger::log_info(&format!(
                        "Refresh recovered via fallback OAuth client [{}]",
                        client_cfg.key
                    ));
                }
                return Ok(token_res);
            }
            Err((status_opt, err_msg)) => {
                let should_fallback = status_opt
                    .map(|status| is_client_mismatch_error(status, &err_msg))
                    .unwrap_or(false);

                attempt_errors.push(format!("{} => {}", client_cfg.key, err_msg));

                if should_fallback {
                    crate::modules::logger::log_warn(&format!(
                        "Refresh failed for client [{}], trying next client: {}",
                        client_cfg.key, err_msg
                    ));
                    continue;
                }

                return Err(format!(
                    "Refresh failed for client [{}]: {}",
                    client_cfg.key, err_msg
                ));
            }
        }
    }

    Err(format!(
        "Refresh failed for all OAuth clients: {}",
        attempt_errors.join(" | ")
    ))
}

/// Refresh access_token using refresh_token
pub async fn refresh_access_token(
    refresh_token: &str,
    account_id: Option<&str>,
) -> Result<TokenResponse, String> {
    refresh_access_token_with_client(refresh_token, account_id, None).await
}

/// Get user info
pub async fn get_user_info(access_token: &str, account_id: Option<&str>) -> Result<UserInfo, String> {
    let client = if let Some(pool) = crate::proxy::proxy_pool::get_global_proxy_pool() {
        pool.get_effective_client(account_id, 15).await
    } else {
        crate::utils::http::get_client()
    };
    
    let response = client
        .get(USERINFO_URL)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("User info request failed: {}", e))?;

    if response.status().is_success() {
        response.json::<UserInfo>()
            .await
            .map_err(|e| format!("User info parsing failed: {}", e))
    } else {
        let error_text = response.text().await.unwrap_or_default();
        Err(format!("Failed to get user info: {}", error_text))
    }
}

/// Check and refresh Token if needed
/// Returns the latest access_token
pub async fn ensure_fresh_token(
    current_token: &crate::models::TokenData,
    account_id: Option<&str>,
) -> Result<crate::models::TokenData, String> {
    let now = chrono::Local::now().timestamp();
    
    // If no expiry or more than 5 minutes valid, return direct
    if current_token.expiry_timestamp > now + 300 {
        return Ok(current_token.clone());
    }
    
    // Need to refresh
    crate::modules::logger::log_info(&format!("Token expiring soon for account {:?}, refreshing...", account_id));
    let response = refresh_access_token_with_client(
        &current_token.refresh_token,
        account_id,
        current_token.oauth_client_key.as_deref(),
    )
    .await?;
    
    let oauth_client_key = response
        .oauth_client_key
        .clone()
        .or_else(|| current_token.oauth_client_key.clone());

    // Construct new TokenData
    Ok(crate::models::TokenData::new(
        response.access_token,
        current_token.refresh_token.clone(), // refresh_token may not be returned on refresh
        response.expires_in,
        current_token.email.clone(),
        current_token.project_id.clone(), // Keep original project_id
        None,  // session_id will be generated in token_manager
        current_token.is_gcp_tos,
    )
    .with_oauth_client_key(oauth_client_key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_auth_url_contains_state() {
        let redirect_uri = "http://localhost:8080/callback";
        let state = "test-state-123456";
        let url = get_auth_url(redirect_uri, state);
        
        assert!(url.contains("state=test-state-123456"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A8080%2Fcallback"));
        assert!(url.contains("response_type=code"));
    }
}
