use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde_json::Value;
use std::sync::Arc;

// =========================================================================
// 端点健康度追踪器 (EndpointHealthTracker)
// =========================================================================

#[derive(Clone)]
pub struct EndpointHealthTracker {
    failures: Arc<DashMap<String, std::time::Instant>>,
    cooldown: std::time::Duration,
}

static GLOBAL_ENDPOINT_HEALTH: Lazy<EndpointHealthTracker> = Lazy::new(EndpointHealthTracker::default);

pub fn get_global_endpoint_health() -> &'static EndpointHealthTracker {
    &GLOBAL_ENDPOINT_HEALTH
}

impl Default for EndpointHealthTracker {
    fn default() -> Self {
        Self::new(std::time::Duration::from_secs(30))
    }
}

pub fn is_sandbox_endpoint(host: &str) -> bool {
    host.contains("sandbox")
}

impl EndpointHealthTracker {
    pub fn new(cooldown: std::time::Duration) -> Self {
        Self {
            failures: Arc::new(DashMap::new()),
            cooldown,
        }
    }

    pub fn mark_failure(&self, endpoint: &str) {
        let host = extract_endpoint_host(endpoint);
        self.failures.insert(host.to_string(), std::time::Instant::now());
    }

    pub fn mark_success(&self, endpoint: &str) {
        let host = extract_endpoint_host(endpoint);
        self.failures.remove(host);
    }

    pub fn is_healthy(&self, endpoint: &str) -> bool {
        let host = extract_endpoint_host(endpoint);
        let cd = if is_sandbox_endpoint(host) {
            self.cooldown.max(std::time::Duration::from_secs(300))
        } else {
            self.cooldown
        };
        if let Some(entry) = self.failures.get(host) {
            if entry.elapsed() >= cd {
                drop(entry);
                self.failures.remove(host);
                true
            } else {
                false
            }
        } else {
            true
        }
    }

    pub fn get_ordered_endpoints<'a>(&self, endpoints: &'a [&'a str]) -> Vec<&'a str> {
        let mut healthy_primary = Vec::new();
        let mut unhealthy_primary = Vec::new();
        let mut healthy_sandbox = Vec::new();
        let mut unhealthy_sandbox = Vec::new();

        for &ep in endpoints {
            let host = extract_endpoint_host(ep);
            let is_sandbox = is_sandbox_endpoint(host);
            let cd = if is_sandbox {
                self.cooldown.max(std::time::Duration::from_secs(300))
            } else {
                self.cooldown
            };

            let mut fail_time = None;
            if let Some(entry) = self.failures.get(host) {
                let last_fail = *entry.value();
                if last_fail.elapsed() >= cd {
                    drop(entry);
                    self.failures.remove(host);
                } else {
                    fail_time = Some(last_fail);
                }
            }

            match (is_sandbox, fail_time) {
                (false, None) => healthy_primary.push(ep),
                (false, Some(t)) => unhealthy_primary.push((ep, t)),
                (true, None) => healthy_sandbox.push(ep),
                (true, Some(t)) => unhealthy_sandbox.push((ep, t)),
            }
        }

        unhealthy_primary.sort_by_key(|&(_, time)| time);
        unhealthy_sandbox.sort_by_key(|&(_, time)| time);

        let mut result = healthy_primary;
        for (ep, _) in unhealthy_primary {
            result.push(ep);
        }
        result.extend(healthy_sandbox);
        for (ep, _) in unhealthy_sandbox {
            result.push(ep);
        }

        result
    }
}

pub const V1_INTERNAL_BASE_URL_FALLBACKS: [&str; 2] = [
    "https://daily-cloudcode-pa.googleapis.com",
    "https://daily-cloudcode-pa.sandbox.googleapis.com",
];

pub fn extract_endpoint_host(endpoint: &str) -> &str {
    if let Some(stripped) = endpoint.strip_prefix("https://") {
        stripped.split('/').next().unwrap_or(endpoint)
    } else if let Some(stripped) = endpoint.strip_prefix("http://") {
        stripped.split('/').next().unwrap_or(endpoint)
    } else {
        endpoint.split('/').next().unwrap_or(endpoint)
    }
}

pub fn should_try_next_endpoint_for_url(status: rquest::StatusCode, _url: &str) -> bool {
    status.is_server_error()
        || status == rquest::StatusCode::TOO_MANY_REQUESTS
        || status == rquest::StatusCode::UNAUTHORIZED
        || status == rquest::StatusCode::FORBIDDEN
}

// =========================================================================
// 项目解析 (Fetch Project ID)
// =========================================================================

const LOAD_PROJECT_ENDPOINTS: [&str; 3] = [
    "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist",
    "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
];

pub async fn fetch_project_id(access_token: &str) -> Result<String, String> {
    fetch_project_id_for_account(access_token, None).await
}

pub async fn fetch_project_id_for_account(
    access_token: &str,
    _account_id: Option<&str>,
) -> Result<String, String> {
    let request_body = serde_json::json!({
        "metadata": {
            "ideType": "ANTIGRAVITY"
        }
    });

    let client = crate::utils::http::get_standard_client();
    let health_tracker = get_global_endpoint_health();
    let ordered_endpoints = health_tracker.get_ordered_endpoints(&LOAD_PROJECT_ENDPOINTS);
    let mut last_error = String::new();

    for (idx, url) in ordered_endpoints.iter().enumerate() {
        let response = client
            .post(*url)
            .timeout(std::time::Duration::from_secs(12))
            .bearer_auth(access_token)
            .header(
                "User-Agent",
                crate::constants::current_egress_user_agent().as_str(),
            )
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await;

        match response {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    health_tracker.mark_success(url);
                    let data: Value = resp
                        .json()
                        .await
                        .map_err(|e| format!("解析响应失败: {}", e))?;

                    if let Some(project_id) =
                        data.get("cloudaicompanionProject").and_then(|v| v.as_str())
                    {
                        if idx > 0 {
                            tracing::info!(
                                "loadCodeAssist fetch_project_id fallback succeeded at endpoint #{} ({})",
                                idx + 1,
                                url
                            );
                        }
                        return Ok(project_id.to_string());
                    } else {
                        return Err("账号无资格获取官方 cloudaicompanionProject".to_string());
                    }
                } else {
                    if should_try_next_endpoint_for_url(status, url) {
                        health_tracker.mark_failure(url);
                    }
                    let body = resp.text().await.unwrap_or_default();
                    last_error = format!("loadCodeAssist [{}] 返回错误 {}: {}", url, status, body);
                    continue;
                }
            }
            Err(e) => {
                health_tracker.mark_failure(url);
                last_error = format!("loadCodeAssist [{}] 请求失败: {}", url, e);
                continue;
            }
        }
    }

    Err(if last_error.is_empty() {
        "所有 loadCodeAssist 端点均无法访问".to_string()
    } else {
        last_error
    })
}

// =========================================================================
// 模型名称规范化与工具函数
// =========================================================================

pub fn strip_tier_suffix(model: &str) -> String {
    const TIER_SUFFIXES: &[&str] = &[
        "-extra-low",
        "-xhigh",
        "-high",
        "-medium",
        "-low",
        "-thinking",
        "-tiered",
        "@default",
    ];
    let lower = model.trim().to_lowercase();
    for suffix in TIER_SUFFIXES {
        if lower.ends_with(suffix) {
            let prefix_len = model.len() - suffix.len();
            return model[..prefix_len].to_string();
        }
    }
    model.trim().to_string()
}

pub fn strip_image_variant_suffix(id: &str) -> String {
    const SUFFIXES: &[&str] = &[
        "-preview",
        "-2k",
        "-4k",
        "-1k",
        "-8k",
        "-16x9",
        "-9x16",
        "-1x1",
        "-3x4",
        "-4x3",
        "-high",
        "-medium",
        "-low",
        "-extra-low",
        "-tiered",
    ];
    let mut base = id.trim().to_lowercase();
    let mut changed = true;
    while changed {
        changed = false;
        for s in SUFFIXES {
            if base.ends_with(s) {
                base.truncate(base.len() - s.len());
                changed = true;
                break;
            }
        }
    }
    base
}

fn is_generic_gemini_role_token(token: &str) -> bool {
    matches!(
        token,
        "pro"
            | "flash"
            | "ultra"
            | "nano"
            | "lite"
            | "image"
            | "agent"
            | "exp"
            | "preview"
            | "disabled"
            | "thinking"
            | "tiered"
            | "models"
            | "model"
            | "wtu"
            | "weekly"
            | "5h"
            | "7d"
            | "hour"
            | "quota"
            | "pool"
            | "default"
            | "standard"
    )
}

fn extract_gemini_codename_with_version(model_name: &str) -> Option<(String, String)> {
    let lower = model_name.trim().to_lowercase();
    let stripped = strip_tier_suffix(&lower);
    if let Some(after_gemini) = stripped.strip_prefix("gemini-") {
        if let Some((ver_part, rest)) = after_gemini.split_once('-') {
            let major_ok = ver_part
                .split('.')
                .next()
                .and_then(|m| m.parse::<u32>().ok())
                .map(|m| m >= 4)
                .unwrap_or(false);
            if major_ok {
                let code = rest.split('-').next().unwrap_or("").trim();
                if !code.is_empty()
                    && code.bytes().any(|b| b.is_ascii_alphabetic())
                    && !is_generic_gemini_role_token(code)
                {
                    return Some((ver_part.to_string(), code.to_string()));
                }
            }
        }
    }
    if lower.contains("argon") || stripped == "gemini-pro-orchestrator-g1-pro" {
        return Some(("4".to_string(), "argon".to_string()));
    }
    None
}

pub fn extract_gemini4_codename(model_name: &str) -> Option<String> {
    extract_gemini_codename_with_version(model_name).map(|(_, code)| code)
}

pub fn extract_group_g4_codename(
    display_name_lower: &str,
    bucket_id_lower: &str,
) -> Option<String> {
    if display_name_lower.contains("argon") || bucket_id_lower.contains("argon") {
        return Some("argon".to_string());
    }
    for text in [display_name_lower, bucket_id_lower] {
        let normalized = text.replace('_', "-").replace(' ', "-");
        let parts: Vec<&str> = normalized.split('-').filter(|s| !s.is_empty()).collect();
        for win in parts.windows(3) {
            if win[0] == "gemini" {
                let is_v4_plus = win[1]
                    .split('.')
                    .next()
                    .and_then(|m| m.parse::<u32>().ok())
                    .map(|m| m >= 4)
                    .unwrap_or(false);
                if is_v4_plus
                    && win[2].bytes().any(|b| b.is_ascii_alphabetic())
                    && !is_generic_gemini_role_token(win[2])
                {
                    return Some(win[2].to_string());
                }
                if win[2] == "wtu"
                    && win[1].bytes().any(|b| b.is_ascii_alphabetic())
                    && !is_generic_gemini_role_token(win[1])
                {
                    return Some(win[1].to_string());
                }
            }
        }
    }
    None
}

pub fn extract_group_g4_codename_with_known(
    display_name_lower: &str,
    bucket_id_lower: &str,
    known_codenames: &[String],
) -> Option<String> {
    if let Some(code) = extract_group_g4_codename(display_name_lower, bucket_id_lower) {
        return Some(code);
    }
    known_codenames
        .iter()
        .find(|code| {
            !code.is_empty()
                && (display_name_lower.contains(code.as_str())
                    || bucket_id_lower.contains(code.as_str()))
        })
        .cloned()
}

pub fn normalize_to_standard_id(model_name: &str) -> Option<String> {
    let lower = model_name.to_lowercase();
    if lower.contains("-disabled") || lower.ends_with("_disabled") {
        return None;
    }
    if let Some((ver, code)) = extract_gemini_codename_with_version(&lower) {
        return Some(format!("gemini-{}-{}", ver, code));
    }
    if lower.contains("image") {
        if lower.contains("flash") {
            return Some("gemini-3.1-flash-image".to_string());
        }
        return Some("gemini-3-pro-image".to_string());
    }
    if lower.contains("claude") {
        if lower.contains("opus") {
            return Some("claude-opus".to_string());
        }
        return Some("claude-sonnet".to_string());
    }
    if lower.contains("pro") {
        return Some("gemini-3-pro".to_string());
    }
    if lower.contains("flash") {
        return Some("gemini-3-flash".to_string());
    }
    Some(strip_tier_suffix(&lower))
}

pub fn is_model_compliant_with_baseline(model: &str) -> bool {
    let lower = model.to_lowercase();
    let m = lower.trim();

    if m.contains("-disabled")
        || m.ends_with("_disabled")
        || m.starts_with("chat_")
        || m.starts_with("tab_")
        || m.contains("internal")
        || m.contains("-exp")
        || m.ends_with("-agent")
        || m == "gemini-3-flash"
        || m == "gemini-flash-worker-agy"
    {
        return false;
    }

    if let Some(rest) = m.strip_prefix("gemini-") {
        if let Some((major, _)) = parse_version_tuple(rest) {
            if major < 3 {
                return false;
            }
        }
    }
    true
}

fn parse_version_tuple(rest: &str) -> Option<(u32, u32)> {
    let mut parts = rest.split('-');
    let ver_str = parts.next()?;
    let mut num_parts = ver_str.split('.');
    let major = num_parts.next()?.parse::<u32>().ok()?;
    let minor = num_parts.next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
    Some((major, minor))
}

pub fn is_active_persisted_long_image_limit(
    model_key: &str,
    status: &crate::models::account::LiveLimitStatus,
    now: i64,
) -> bool {
    status.status == 429
        && status.reason == "QuotaExhausted"
        && status.until > now
        && status.until.saturating_sub(status.detected_at) > 3600
        && model_key.to_lowercase().contains("image")
        && status.message.as_deref().is_some_and(|message| {
            message.contains("QUOTA_EXHAUSTED")
                && (message.contains("quotaResetDelay") || message.contains("reset after"))
        })
}
