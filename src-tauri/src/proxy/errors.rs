use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

pub(crate) fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }

    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = s[..end].to_string();
    out.push_str("…");
    out
}

pub fn anthropic_error(status: StatusCode, error_type: &'static str, message: impl Into<String>) -> Response {
    let message = message.into();
    (
        status,
        Json(json!({
            "type": "error",
            "error": {
                "type": error_type,
                "message": message
            }
        })),
    )
        .into_response()
}

pub fn summarize_for_log(body_text: &str) -> String {
    let parsed = parse_google_error_body(body_text);
    if let Some((status, message, code)) = parsed {
        if let Some(code) = code {
            return format!("{} (code {}): {}", status, code, truncate_utf8(&message, 400));
        }
        return format!("{}: {}", status, truncate_utf8(&message, 400));
    }

    truncate_utf8(body_text, 400)
}

pub fn map_token_manager_error_to_anthropic(err: &str) -> Response {
    let msg = if is_no_available_accounts_error(err) && err.contains("Token pool is empty") {
        "No accounts available: the account pool is empty. Add/enable accounts in the UI."
            .to_string()
    } else if err.contains("invalid_grant") {
        "No available accounts: OAuth refresh failed (invalid_grant). Re-authorize the affected account(s) in the UI."
            .to_string()
    } else if is_no_available_accounts_error(err) {
        "No available accounts: all accounts are currently unavailable (quota/auth/disabled). Check account status in the UI."
            .to_string()
    } else {
        format!("No available accounts: {}", truncate_utf8(err, 240))
    };

    anthropic_error(StatusCode::SERVICE_UNAVAILABLE, "overloaded_error", msg)
}

pub fn is_no_available_accounts_error(err: &str) -> bool {
    err.contains("Token pool is empty")
        || err.contains("All accounts exhausted")
        || err.contains("All accounts failed")
        || err.contains("No available accounts")
}

pub fn map_google_upstream_error_to_anthropic(
    status: StatusCode,
    body_text: &str,
    trace_id: Option<&str>,
) -> Response {
    let (upstream_status, upstream_message, _code) =
        parse_google_error_body(body_text).unwrap_or_else(|| ("UPSTREAM_ERROR".to_string(), body_text.to_string(), None));

    let mut hint = String::new();
    if let Some(trace_id) = trace_id {
        hint.push_str(&format!(" (trace_id={})", trace_id));
    }

    // Normalize common cases to Anthropic-style error taxonomy.
    match status {
        StatusCode::UNAUTHORIZED => anthropic_error(
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            format!(
                "Upstream authentication failed; the current account may need re-authorization. {}{}",
                truncate_utf8(&upstream_message, 400),
                hint
            ),
        ),
        StatusCode::FORBIDDEN => anthropic_error(
            StatusCode::FORBIDDEN,
            "permission_error",
            format!(
                "Upstream permission denied (region/plan/feature gate). {}{}",
                truncate_utf8(&upstream_message, 400),
                hint
            ),
        ),
        StatusCode::TOO_MANY_REQUESTS => {
            let is_quota_exhausted = upstream_status.contains("QUOTA_EXHAUSTED")
                || upstream_status.contains("RESOURCE_EXHAUSTED")
                || upstream_message.contains("QUOTA_EXHAUSTED")
                || upstream_message.contains("RESOURCE_EXHAUSTED");

            let prefix = if is_quota_exhausted {
                "Upstream quota exhausted."
            } else {
                "Upstream rate limited."
            };

            anthropic_error(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limit_error",
                format!("{} {}{}", prefix, truncate_utf8(&upstream_message, 400), hint),
            )
        }
        StatusCode::BAD_REQUEST => anthropic_error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            format!(
                "Upstream rejected the request (invalid argument). {}{}",
                truncate_utf8(&upstream_message, 400),
                hint
            ),
        ),
        StatusCode::NOT_FOUND => anthropic_error(
            StatusCode::NOT_FOUND,
            "invalid_request_error",
            format!(
                "Upstream endpoint/model not found. {}{}",
                truncate_utf8(&upstream_message, 400),
                hint
            ),
        ),
        s if s.is_server_error() => anthropic_error(
            StatusCode::BAD_GATEWAY,
            "api_error",
            format!(
                "Upstream server error. {}{}",
                truncate_utf8(&upstream_message, 400),
                hint
            ),
        ),
        _ => anthropic_error(
            status,
            "api_error",
            format!("Upstream error. {}{}", truncate_utf8(&upstream_message, 400), hint),
        ),
    }
}

fn parse_google_error_body(body_text: &str) -> Option<(String, String, Option<i64>)> {
    let json_val: Value = serde_json::from_str(body_text).ok()?;
    let err = json_val.get("error")?;

    let status = err
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| err.get("error").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| "UPSTREAM_ERROR".to_string());

    let message = err
        .get("message")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| body_text.to_string());

    let code = err.get("code").and_then(|v| v.as_i64());
    Some((status, message, code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_utf8() {
        let s = "абвгд";
        let t = truncate_utf8(s, 5);
        assert!(t.ends_with('…'));
        assert!(t.len() <= s.len() + 3);
    }

    #[test]
    fn test_parse_google_error_body() {
        let body = r#"{"error":{"code":429,"status":"RESOURCE_EXHAUSTED","message":"QUOTA_EXHAUSTED"}}"#;
        let (status, message, code) = parse_google_error_body(body).expect("parsed");
        assert_eq!(status, "RESOURCE_EXHAUSTED");
        assert_eq!(message, "QUOTA_EXHAUSTED");
        assert_eq!(code, Some(429));
    }

    #[tokio::test]
    async fn test_map_google_upstream_error_to_anthropic_429() {
        let body = r#"{"error":{"code":429,"status":"RESOURCE_EXHAUSTED","message":"QUOTA_EXHAUSTED"}}"#;
        let resp = map_google_upstream_error_to_anthropic(StatusCode::TOO_MANY_REQUESTS, body, Some("t123"));
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

        let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("body bytes");
        let v: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["type"], "error");
        assert_eq!(v["error"]["type"], "rate_limit_error");
        assert!(v["error"]["message"].as_str().unwrap_or_default().contains("trace_id=t123"));
    }

    #[test]
    fn test_map_token_manager_error_invalid_grant() {
        let resp = map_token_manager_error_to_anthropic("Token refresh failed: invalid_grant");
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn test_is_no_available_accounts_error() {
        assert!(is_no_available_accounts_error("Token pool is empty"));
        assert!(is_no_available_accounts_error("All accounts exhausted"));
        assert!(is_no_available_accounts_error("All accounts failed"));
        assert!(is_no_available_accounts_error("No available accounts"));
        assert!(!is_no_available_accounts_error("Some other error"));
    }
}
