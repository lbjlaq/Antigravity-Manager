// File: src-tauri/src/proxy/middleware/ip_filter.rs
//! IP filtering middleware for Axum.
//! Implements blacklist/whitelist checking with CIDR support.

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::modules::security_db;
use crate::proxy::config::SecurityMonitorConfig;

/// Shared security configuration state
pub type SecurityState = Arc<RwLock<SecurityMonitorConfig>>;

/// Blocked response payload
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BlockedResponse {
    error: String,
    reason: String,
    blocked_until: Option<i64>,
    ip: String,
}

/// Extract client IP from request (supports X-Forwarded-For, X-Real-IP)
pub fn extract_client_ip(req: &Request<Body>, connect_info: Option<&ConnectInfo<SocketAddr>>) -> String {
    // Priority 1: X-Forwarded-For header (first IP in chain)
    if let Some(forwarded) = req.headers().get("x-forwarded-for") {
        if let Ok(value) = forwarded.to_str() {
            if let Some(first_ip) = value.split(',').next() {
                let ip = first_ip.trim();
                if !ip.is_empty() {
                    return ip.to_string();
                }
            }
        }
    }

    // Priority 2: X-Real-IP header
    if let Some(real_ip) = req.headers().get("x-real-ip") {
        if let Ok(ip) = real_ip.to_str() {
            let ip = ip.trim();
            if !ip.is_empty() {
                return ip.to_string();
            }
        }
    }

    // Priority 3: Connection info (direct connection)
    if let Some(info) = connect_info {
        return info.0.ip().to_string();
    }

    // Fallback
    "unknown".to_string()
}

/// IP filter middleware
/// Note: ConnectInfo may not be available when using manual hyper server,
/// so we make it optional and fall back to header-based IP extraction.
pub async fn ip_filter_middleware(
    connect_info: Option<ConnectInfo<SocketAddr>>,
    security_state: axum::extract::Extension<SecurityState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let client_ip = extract_client_ip(&req, connect_info.as_ref());
    let path = req.uri().path().to_string();
    let method = req.method().to_string();

    // Read config values we need
    let (blacklist_enabled, whitelist_enabled, strict_mode, access_log_enabled) = {
        let config = security_state.read().await;
        (
            config.blacklist.enabled,
            config.whitelist.enabled,
            config.whitelist.strict_mode,
            config.access_log.enabled,
        )
    };

    // Skip filtering if both blacklist and whitelist are disabled
    if !blacklist_enabled && !whitelist_enabled {
        return next.run(req).await;
    }

    // Check whitelist first (if enabled and has priority)
    if whitelist_enabled {
        let is_whitelisted = security_db::is_ip_in_whitelist(&client_ip).unwrap_or(false);

        if is_whitelisted {
            // Whitelisted IPs bypass all checks
            if access_log_enabled {
                log_access(&client_ip, &path, &method, 200, false, None);
            }
            return next.run(req).await;
        }

        // Strict mode: only whitelisted IPs allowed
        if strict_mode {
            log_access(&client_ip, &path, &method, 403, true, Some("IP not in whitelist (strict mode)"));

            tracing::warn!(
                "[Security] Blocked request from non-whitelisted IP: {} (strict mode)",
                client_ip
            );

            return create_blocked_response(
                &client_ip,
                "IP not in whitelist",
                None,
            );
        }
    }

    // Check blacklist
    if blacklist_enabled {
        if let Ok(Some(entry)) = security_db::get_blacklist_entry_for_ip(&client_ip) {
            let reason = if entry.reason.is_empty() {
                "IP is blacklisted".to_string()
            } else {
                entry.reason.clone()
            };

            log_access(&client_ip, &path, &method, 403, true, Some(&reason));

            tracing::warn!(
                "[Security] Blocked request from blacklisted IP: {} (reason: {})",
                client_ip,
                reason
            );

            return create_blocked_response(&client_ip, &reason, entry.expires_at);
        }
    }

    // IP passed all checks
    let response = next.run(req).await;
    
    // Log successful access if enabled
    if access_log_enabled {
        let status = response.status().as_u16() as i32;
        log_access(&client_ip, &path, &method, status, false, None);
    }

    response
}

/// Create a blocked response with JSON payload
fn create_blocked_response(ip: &str, reason: &str, blocked_until: Option<i64>) -> Response {
    let body = BlockedResponse {
        error: "Forbidden".to_string(),
        reason: reason.to_string(),
        blocked_until,
        ip: ip.to_string(),
    };

    (StatusCode::FORBIDDEN, Json(body)).into_response()
}

/// Log access to database (synchronous, fire-and-forget)
fn log_access(
    ip: &str,
    path: &str,
    method: &str,
    status: i32,
    blocked: bool,
    reason: Option<&str>,
) {
    let _ = security_db::log_access(ip, path, method, status, blocked, reason, None);
}

/// Create security state from config
pub fn create_security_state(config: &crate::proxy::config::ProxyConfig) -> SecurityState {
    Arc::new(RwLock::new(config.security_monitor.clone()))
}

/// Update security state (for hot-reload)
pub async fn update_security_state(state: &SecurityState, config: &crate::proxy::config::ProxyConfig) {
    let mut guard = state.write().await;
    *guard = config.security_monitor.clone();
    tracing::debug!("[Security] Configuration hot-reloaded");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    #[test]
    fn test_extract_client_ip_direct() {
        let req = Request::builder()
            .uri("/test")
            .body(Body::empty())
            .expect("Failed to build request");

        let addr: SocketAddr = "192.168.1.100:12345".parse().expect("Failed to parse addr");
        let ip = extract_client_ip(&req, Some(&ConnectInfo(addr)));
        assert_eq!(ip, "192.168.1.100");
    }

    #[test]
    fn test_extract_client_ip_forwarded() {
        let req = Request::builder()
            .uri("/test")
            .header("x-forwarded-for", "10.0.0.1, 192.168.1.1")
            .body(Body::empty())
            .expect("Failed to build request");

        let addr: SocketAddr = "127.0.0.1:12345".parse().expect("Failed to parse addr");
        let ip = extract_client_ip(&req, Some(&ConnectInfo(addr)));
        assert_eq!(ip, "10.0.0.1");
    }

    #[test]
    fn test_extract_client_ip_real_ip() {
        let req = Request::builder()
            .uri("/test")
            .header("x-real-ip", "10.0.0.50")
            .body(Body::empty())
            .expect("Failed to build request");

        let addr: SocketAddr = "127.0.0.1:12345".parse().expect("Failed to parse addr");
        let ip = extract_client_ip(&req, Some(&ConnectInfo(addr)));
        assert_eq!(ip, "10.0.0.50");
    }
}
