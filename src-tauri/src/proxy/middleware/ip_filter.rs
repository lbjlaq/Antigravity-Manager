use crate::modules::security_db;
use crate::proxy::server::AppState;
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

/// IP 黑白名单过滤中间件
pub async fn ip_filter_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    ip_filter_with_security(&state.security, request, next).await
}

fn is_ip_filter_bypassed(method: &axum::http::Method, path: &str) -> bool {
    method == axum::http::Method::OPTIONS || matches!(path, "/health" | "/healthz" | "/api/health")
}

async fn ip_filter_with_security(
    security: &tokio::sync::RwLock<crate::proxy::ProxySecurityConfig>,
    request: Request,
    next: Next,
) -> Response {
    // 放行 CORS 预检与显式健康探针，避免拦截管理端预检与容器探活
    if is_ip_filter_bypassed(request.method(), request.uri().path()) {
        return next.run(request).await;
    }

    // 提取客户端 IP
    let client_ip = extract_client_ip(&request);

    if let Some(ip) = &client_ip {
        // 读取安全配置
        let security_config = security.read().await;

        // 1. 检查白名单 (如果启用白名单模式,只允许白名单 IP)
        if security_config.security_monitor.whitelist.enabled {
            match security_db::is_ip_in_whitelist(ip) {
                Ok(true) => {
                    // 在白名单中,直接放行
                    tracing::debug!("[IP Filter] IP {} is in whitelist, allowing", ip);
                    return next.run(request).await;
                }
                Ok(false) => {
                    // 不在白名单中,且启用了白名单模式,拒绝访问
                    tracing::warn!("[IP Filter] IP {} not in whitelist, blocking", ip);
                    return create_blocked_response(
                        ip,
                        "Access denied. Your IP is not in the whitelist.",
                    );
                }
                Err(e) => {
                    tracing::error!("[IP Filter] Failed to check whitelist: {}", e);
                }
            }
        } else {
            // 白名单优先模式: 如果在白名单中,跳过黑名单检查
            if security_config
                .security_monitor
                .whitelist
                .whitelist_priority
            {
                match security_db::is_ip_in_whitelist(ip) {
                    Ok(true) => {
                        tracing::debug!("[IP Filter] IP {} is in whitelist (priority mode), skipping blacklist check", ip);
                        return next.run(request).await;
                    }
                    Ok(false) => {
                        // 继续检查黑名单
                    }
                    Err(e) => {
                        tracing::error!("[IP Filter] Failed to check whitelist: {}", e);
                    }
                }
            }
        }

        // 2. 检查黑名单
        if security_config.security_monitor.blacklist.enabled {
            match security_db::get_blacklist_entry_for_ip(ip) {
                Ok(Some(entry)) => {
                    tracing::warn!("[IP Filter] IP {} is in blacklist, blocking", ip);

                    // 构建详细的封禁消息
                    let reason = entry
                        .reason
                        .as_deref()
                        .unwrap_or("Malicious activity detected");
                    let ban_type = if let Some(expires_at) = entry.expires_at {
                        let now = chrono::Utc::now().timestamp();
                        let remaining_seconds = expires_at - now;

                        if remaining_seconds > 0 {
                            let hours = remaining_seconds / 3600;
                            let minutes = (remaining_seconds % 3600) / 60;

                            if hours > 24 {
                                let days = hours / 24;
                                format!("Temporary ban. Please try again after {} day(s).", days)
                            } else if hours > 0 {
                                format!("Temporary ban. Please try again after {} hour(s) and {} minute(s).", hours, minutes)
                            } else {
                                format!(
                                    "Temporary ban. Please try again after {} minute(s).",
                                    minutes
                                )
                            }
                        } else {
                            "Temporary ban (expired, will be removed soon).".to_string()
                        }
                    } else {
                        "Permanent ban.".to_string()
                    };

                    let detailed_message =
                        format!("Access denied. Reason: {}. {}", reason, ban_type);

                    // 记录被封禁的访问日志
                    let log = security_db::IpAccessLog {
                        id: uuid::Uuid::new_v4().to_string(),
                        client_ip: ip.clone(),
                        timestamp: chrono::Utc::now().timestamp(),
                        method: Some(request.method().to_string()),
                        path: Some(crate::proxy::payload_audit::redact_uri(
                            &request.uri().to_string(),
                        )),
                        user_agent: request
                            .headers()
                            .get("user-agent")
                            .and_then(|v| v.to_str().ok())
                            .map(|s| s.to_string()),
                        status: Some(403),
                        duration: Some(0),
                        api_key_hash: None,
                        blocked: true,
                        block_reason: Some(format!("IP in blacklist: {}", reason)),
                        username: None,
                    };

                    tokio::spawn(async move {
                        if let Err(e) = security_db::save_ip_access_log(&log) {
                            tracing::error!("[IP Filter] Failed to save blocked access log: {}", e);
                        }
                    });

                    return create_blocked_response(ip, &detailed_message);
                }
                Ok(None) => {
                    // 不在黑名单中,放行
                    tracing::debug!("[IP Filter] IP {} not in blacklist, allowing", ip);
                }
                Err(e) => {
                    tracing::error!("[IP Filter] Failed to check blacklist: {}", e);
                }
            }
        }
    } else {
        tracing::warn!("[IP Filter] Unable to extract client IP from request");
    }

    // 放行请求
    next.run(request).await
}

/// 规范化 IP 地址字符串：
/// - 清理首尾空格及方括号 `[...]`
/// - 剥离客户端端口（例如 `192.168.1.1:54321` 或 `[2409:8a55::1]:8046`）
/// - 将 IPv4 映射的 IPv6 地址（如 `::ffff:192.168.1.1`）还原为原生 IPv4（`192.168.1.1`）
pub fn normalize_ip_str(raw: &str) -> String {
    let trimmed = raw.trim();

    // 优先尝试直接按 IP 地址解析
    let clean = trimmed.trim_matches('[').trim_matches(']');
    if let Ok(ip) = clean.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V6(v6) => {
                if let Some(v4) = v6.to_ipv4_mapped() {
                    v4.to_string()
                } else {
                    v6.to_string()
                }
            }
            std::net::IpAddr::V4(v4) => v4.to_string(),
        };
    }

    // 若带有端口（如 192.168.1.100:12345 或 [::1]:8080），尝试解析为 SocketAddr
    if let Ok(socket_addr) = trimmed.parse::<std::net::SocketAddr>() {
        return match socket_addr.ip() {
            std::net::IpAddr::V6(v6) => {
                if let Some(v4) = v6.to_ipv4_mapped() {
                    v4.to_string()
                } else {
                    v6.to_string()
                }
            }
            std::net::IpAddr::V4(v4) => v4.to_string(),
        };
    }

    // 手动前缀回退检测
    if let Some(stripped) = clean.strip_prefix("::ffff:") {
        if stripped.parse::<std::net::Ipv4Addr>().is_ok() {
            return stripped.to_string();
        }
    }

    clean.to_string()
}

/// 仅根据 TCP 对端地址判断是否为本机回环。
///
/// 不读取 `X-Forwarded-For` / `X-Real-IP`。这两项由客户端或反代填写，
/// 不能用来决定 `/internal/` 是否免鉴权，否则伪造 `127.0.0.1` 即可绕过。
/// 对端地址由 `server.rs` 的 accept 循环写入 `ConnectInfo`。
pub fn is_direct_loopback(request: &Request) -> bool {
    request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| ip_is_loopback(info.0.ip()))
        .unwrap_or(false)
}

fn ip_is_loopback(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback(),
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
    }
}

fn normalize_ip_addr(ip: std::net::IpAddr) -> String {
    match ip {
        std::net::IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                v4.to_string()
            } else {
                v6.to_string()
            }
        }
        std::net::IpAddr::V4(v4) => v4.to_string(),
    }
}

fn trust_forwarded_headers_env() -> bool {
    std::env::var("ABV_TRUST_FORWARDED_HEADERS")
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// 从请求中提取客户端 IP（仅在来自本机回环或显式配置信任反代时采信转发头，并自动规范化 IPv4/IPv6）
pub fn extract_client_ip(request: &Request) -> Option<String> {
    let peer_ip = request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.ip());

    // 只有经由本机反代/隧道 (127.0.0.1 / ::1) 转发，或显式开启 ABV_TRUST_FORWARDED_HEADERS，
    // 才采信 X-Forwarded-For / X-Real-IP；直连 LAN/公网请求严禁凭转发头伪造源 IP。
    // 单元测试中若未挂载 ConnectInfo，保留既有行为以兼容无 Socket 的 Router::oneshot 测试。
    let trust_forwarded =
        peer_ip.map(ip_is_loopback).unwrap_or(true) || trust_forwarded_headers_env();

    if trust_forwarded {
        let raw = request
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim())
            .or_else(|| {
                request
                    .headers()
                    .get("x-real-ip")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.trim())
            });

        if let Some(ip_str) = raw {
            if !ip_str.is_empty() {
                let normalized = normalize_ip_str(ip_str);
                if normalized.parse::<std::net::IpAddr>().is_ok() {
                    return Some(normalized);
                }
            }
        }
    }

    // 回退使用真实 TCP 连接对端 IP
    peer_ip.map(normalize_ip_addr)
}

/// 创建被封禁的响应
fn create_blocked_response(ip: &str, message: &str) -> Response {
    let body = serde_json::json!({
        "error": {
            "message": message,
            "type": "ip_blocked",
            "code": "ip_blocked",
            "ip": ip,
        }
    });

    (
        StatusCode::FORBIDDEN,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&body).unwrap_or_else(|_| message.to_string()),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_ip_str() {
        assert_eq!(normalize_ip_str("192.168.1.1"), "192.168.1.1");
        assert_eq!(normalize_ip_str("192.168.1.1:8080"), "192.168.1.1");
        assert_eq!(normalize_ip_str("::ffff:192.168.1.1"), "192.168.1.1");
        assert_eq!(normalize_ip_str("[::ffff:192.168.1.1]:8046"), "192.168.1.1");
        assert_eq!(normalize_ip_str("::1"), "::1");
        assert_eq!(normalize_ip_str("[::1]:8046"), "::1");
        assert_eq!(
            normalize_ip_str("2409:8a55:a21:2e60:2e2:69ff:fe17:95cb"),
            "2409:8a55:a21:2e60:2e2:69ff:fe17:95cb"
        );
    }

    fn request_from(ip: std::net::IpAddr, forwarded_for: Option<&str>) -> Request {
        let mut builder = Request::builder();
        if let Some(xff) = forwarded_for {
            builder = builder.header("x-forwarded-for", xff);
        }
        let mut request = builder.body(axum::body::Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::new(
                ip, 12345,
            )));
        request
    }

    #[test]
    fn direct_loopback_uses_connect_info_only() {
        let v4_loopback: std::net::IpAddr = "127.0.0.1".parse().unwrap();
        let v6_loopback: std::net::IpAddr = "::1".parse().unwrap();
        let mapped_loopback: std::net::IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        let lan: std::net::IpAddr = "192.168.1.10".parse().unwrap();

        assert!(is_direct_loopback(&request_from(v4_loopback, None)));
        assert!(is_direct_loopback(&request_from(v6_loopback, None)));
        assert!(is_direct_loopback(&request_from(mapped_loopback, None)));
        assert!(!is_direct_loopback(&request_from(lan, None)));
        // 伪造转发头不能把公网连接伪装成本机
        assert!(!is_direct_loopback(&request_from(lan, Some("127.0.0.1"))));

        let missing = Request::builder().body(axum::body::Body::empty()).unwrap();
        assert!(!is_direct_loopback(&missing));
    }

    #[test]
    fn non_loopback_peer_cannot_spoof_forwarded_ip() {
        let lan_peer: std::net::IpAddr = "192.168.1.50".parse().unwrap();
        let req = request_from(lan_peer, Some("127.0.0.1, 10.0.0.1"));
        assert_eq!(extract_client_ip(&req).as_deref(), Some("192.168.1.50"));

        let mut x_real_req = Request::builder()
            .header("x-real-ip", "10.0.0.8")
            .body(axum::body::Body::empty())
            .unwrap();
        x_real_req
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::new(
                lan_peer, 44321,
            )));
        assert_eq!(
            extract_client_ip(&x_real_req).as_deref(),
            Some("192.168.1.50")
        );
    }

    #[test]
    fn loopback_peer_accepts_valid_forwarded_ip_and_falls_back_on_malformed() {
        let loopback: std::net::IpAddr = "127.0.0.1".parse().unwrap();
        let req = request_from(loopback, Some("203.0.113.9, 127.0.0.1"));
        assert_eq!(extract_client_ip(&req).as_deref(), Some("203.0.113.9"));

        let malformed = request_from(loopback, Some("not-an-ip"));
        assert_eq!(extract_client_ip(&malformed).as_deref(), Some("127.0.0.1"));
    }

    #[tokio::test]
    async fn ip_filter_blocks_blacklisted_admin_route_and_spoofed_xff() {
        use axum::{routing::get, Router};
        use std::sync::Arc;
        use tokio::sync::RwLock;
        use tower::ServiceExt;

        let _ = security_db::init_db();
        let test_ip = "198.51.100.241";
        if let Ok(entries) = security_db::get_blacklist() {
            for e in entries.into_iter().filter(|e| e.ip_pattern == test_ip) {
                let _ = security_db::remove_from_blacklist(&e.id);
            }
        }
        let added = security_db::add_to_blacklist(test_ip, Some("unit test"), None, "test")
            .expect("blacklist insert");

        let mut sec = crate::proxy::ProxySecurityConfig::from_proxy_config(
            &crate::proxy::ProxyConfig::default(),
        );
        sec.security_monitor.blacklist.enabled = true;
        let security = Arc::new(RwLock::new(sec));

        let admin_routes = Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .route("/accounts", get(|| async { StatusCode::OK }))
            .layer(axum::middleware::from_fn(move |req, next| {
                let sec = Arc::clone(&security);
                async move { ip_filter_with_security(&sec, req, next).await }
            }));
        let app = Router::new().nest("/api", admin_routes);

        let blocked_req = request_from(test_ip.parse().unwrap(), Some("127.0.0.1"));
        let (mut parts, body) = blocked_req.into_parts();
        parts.uri = "/api/accounts".parse().unwrap();
        let resp = app
            .clone()
            .oneshot(Request::from_parts(parts, body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let health_req = request_from(test_ip.parse().unwrap(), None);
        let (mut parts, body) = health_req.into_parts();
        parts.uri = "/api/health".parse().unwrap();
        let resp = app.oneshot(Request::from_parts(parts, body)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let _ = security_db::remove_from_blacklist(&added.id);
    }
}
