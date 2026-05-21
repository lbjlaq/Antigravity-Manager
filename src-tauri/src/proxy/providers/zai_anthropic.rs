use axum::{
    body::Body,
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::Value;
use tokio::time::Duration;

use crate::proxy::server::AppState;

fn map_model_for_zai(original: &str, state: &crate::proxy::ZaiConfig) -> String {
    let m = original.to_lowercase();
    if let Some(mapped) = state.model_mapping.get(original) {
        return mapped.clone();
    }
    if let Some(mapped) = state.model_mapping.get(&m) {
        return mapped.clone();
    }
    if m.starts_with("zai:") {
        return original[4..].to_string();
    }
    if m.starts_with("glm-") {
        return original.to_string();
    }
    if !m.starts_with("claude-") {
        return original.to_string();
    }
    if m.contains("opus") {
        return state.models.opus.clone();
    }
    if m.contains("haiku") {
        return state.models.haiku.clone();
    }
    state.models.sonnet.clone()
}

fn join_base_url(base: &str, path: &str) -> Result<String, String> {
    let mut url =
        url::Url::parse(base.trim()).map_err(|e| format!("Invalid z.ai base_url: {}", e))?;
    let request_path = path.trim_start_matches('/');
    let base_path = url.path().trim_end_matches('/');

    let target_path = if base_path.ends_with(request_path) {
        base_path.to_string()
    } else if request_path.starts_with("v1/messages") && base_path.ends_with("v1/messages") {
        let suffix = request_path.strip_prefix("v1/messages").unwrap_or_default();
        format!("{}{}", base_path, suffix)
    } else if request_path.starts_with("v1/") && base_path.ends_with("v1") {
        let suffix = request_path.strip_prefix("v1").unwrap_or_default();
        format!("{}{}", base_path, suffix)
    } else if base_path.is_empty() {
        request_path.to_string()
    } else {
        format!("{}/{}", base_path, request_path)
    };

    url.set_path(&format!("/{}", target_path.trim_start_matches('/')));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

fn build_client(
    upstream_proxy: Option<crate::proxy::config::UpstreamProxyConfig>,
    timeout_secs: u64,
) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(5)));

    if let Some(config) = upstream_proxy {
        if config.enabled && !config.url.is_empty() {
            let url = crate::proxy::config::normalize_proxy_url(&config.url);
            let proxy = reqwest::Proxy::all(&url)
                .map_err(|e| format!("Invalid upstream proxy url: {}", e))?;
            builder = builder.proxy(proxy);
        }
    }

    builder
        .tcp_nodelay(true) // [FIX #307] Disable Nagle's algorithm to improve latency for small requests
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))
}

fn copy_passthrough_headers(incoming: &HeaderMap) -> HeaderMap {
    // Only forward a conservative set of headers to avoid leaking the local proxy key or cookies.
    let mut out = HeaderMap::new();

    for (k, v) in incoming.iter() {
        let key = k.as_str().to_ascii_lowercase();
        match key.as_str() {
            "content-type" | "accept" | "anthropic-version" | "user-agent" => {
                out.insert(k.clone(), v.clone());
            }
            // Some clients use this for streaming; safe to pass through.
            "cache-control" => {
                out.insert(k.clone(), v.clone());
            }
            _ => {}
        }
    }

    out.entry("anthropic-version")
        .or_insert(HeaderValue::from_static("2023-06-01"));

    out
}

fn set_zai_auth(headers: &mut HeaderMap, incoming: &HeaderMap, api_key: &str) {
    // Prefer to keep the same auth scheme as the incoming request:
    // - If the client used x-api-key (Anthropic style), replace it.
    // - Else if it used Authorization, replace it with Bearer.
    // - Else default to x-api-key.
    let has_x_api_key = incoming.contains_key("x-api-key");
    let has_auth = incoming.contains_key(header::AUTHORIZATION);

    if has_x_api_key || !has_auth {
        if let Ok(v) = HeaderValue::from_str(api_key) {
            headers.insert("x-api-key", v);
        }
    }

    if has_auth {
        if let Ok(v) = HeaderValue::from_str(&format!("Bearer {}", api_key)) {
            headers.insert(header::AUTHORIZATION, v);
        }
    }
}

/// Recursively remove cache_control from all nested objects/arrays
/// [FIX #290] This is a defensive fix that works regardless of serde annotations
pub fn deep_remove_cache_control(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(v) = map.remove("cache_control") {
                tracing::info!("[ISSUE-744] Deep Cleaning found nested cache_control: {:?}", v);
            }
            for v in map.values_mut() {
                deep_remove_cache_control(v);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                deep_remove_cache_control(v);
            }
        }
        _ => {}
    }
}

pub async fn forward_anthropic_json(
    state: &AppState,
    method: Method,
    path: &str,
    incoming_headers: &HeaderMap,
    mut body: Value,
    message_count: usize, // [NEW v4.0.0] Pass message count for rewind detection
) -> Response {
    let zai = state.zai.read().await.clone();
    if !zai.enabled || zai.dispatch_mode == crate::proxy::ZaiDispatchMode::Off {
        return (StatusCode::BAD_REQUEST, "z.ai is disabled").into_response();
    }

    if zai.api_key.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "z.ai api_key is not set").into_response();
    }

    if let Some(model) = body.get("model").and_then(|v| v.as_str()) {
        let mapped = map_model_for_zai(model, &zai);
        body["model"] = Value::String(mapped.clone());

        // [FIX] Caching for z.ai (to support thinking-filter)
        if let Some(sig) = body.get("thinking").and_then(|t| t.get("signature")).and_then(|s| s.as_str()) {
            crate::proxy::SignatureCache::global().cache_session_signature(
                "zai-session",
                sig.to_string(),
                message_count
            );
            crate::proxy::SignatureCache::global().cache_thinking_family(sig.to_string(), mapped);
        }
    }

    let url = match join_base_url(&zai.base_url, path) {
        Ok(u) => u,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };

    let timeout_secs = state.request_timeout.max(5);
    let upstream_proxy = state.upstream_proxy.read().await.clone();
    let client = match build_client(Some(upstream_proxy), timeout_secs) {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };

    let mut headers = copy_passthrough_headers(incoming_headers);
    set_zai_auth(&mut headers, incoming_headers, &zai.api_key);

    // Ensure JSON content type.
    headers
        .entry(header::CONTENT_TYPE)
        .or_insert(HeaderValue::from_static("application/json"));

    // [FIX #290] Clean cache_control before sending to Anthropic API
    // This prevents "Extra inputs are not permitted" errors
    if let Some(cc) = body.get("cache_control") {
        tracing::info!("[ISSUE-744] Deep cleaning cache_control from ROOT: {:?}", cc);
    }
    deep_remove_cache_control(&mut body);

    // [FIX #307] Explicitly serialize body to Vec<u8> to ensure Content-Length is set correctly.
    // This avoids "Transfer-Encoding: chunked" for small bodies which caused connection errors.
    let body_bytes = serde_json::to_vec(&body).unwrap_or_default();
    let body_len = body_bytes.len();

    tracing::debug!("Forwarding request to z.ai (len: {} bytes): {}", body_len, url);

    let req = client.request(method, &url)
        .headers(headers)
        .body(body_bytes); // Use .body(Vec<u8>) instead of .json()

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("Upstream request failed: {}", e),
            )
                .into_response();
        }
    };

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

    let mut out = Response::builder().status(status);
    if let Some(ct) = resp.headers().get(header::CONTENT_TYPE) {
        out = out.header(header::CONTENT_TYPE, ct.clone());
    }

    // Stream response body to the client (covers SSE and non-SSE).
    let stream = resp.bytes_stream().map(|chunk| match chunk {
        Ok(b) => Ok::<Bytes, std::io::Error>(b),
        Err(e) => Ok(Bytes::from(format!("Upstream stream error: {}", e))),
    });

    out.body(Body::from_stream(stream)).unwrap_or_else(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to build response").into_response()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_base_url_accepts_shannon_messages_endpoint() {
        let url = join_base_url("https://api.shannon-ai.com/v1/messages", "/v1/messages")
            .expect("valid joined URL");

        assert_eq!(url, "https://api.shannon-ai.com/v1/messages");
    }

    #[test]
    fn join_base_url_accepts_shannon_messages_endpoint_for_count_tokens() {
        let url = join_base_url(
            "https://api.shannon-ai.com/v1/messages",
            "/v1/messages/count_tokens",
        )
        .expect("valid joined URL");

        assert_eq!(url, "https://api.shannon-ai.com/v1/messages/count_tokens");
    }

    #[test]
    fn join_base_url_accepts_shannon_v1_base() {
        let url = join_base_url("https://api.shannon-ai.com/v1", "/v1/messages")
            .expect("valid joined URL");

        assert_eq!(url, "https://api.shannon-ai.com/v1/messages");
    }

    #[test]
    fn join_base_url_accepts_provider_root_base() {
        let url =
            join_base_url("https://api.shannon-ai.com", "/v1/messages").expect("valid joined URL");

        assert_eq!(url, "https://api.shannon-ai.com/v1/messages");
    }

    #[test]
    fn copy_passthrough_headers_injects_default_anthropic_version() {
        let headers = HeaderMap::new();

        let out = copy_passthrough_headers(&headers);

        assert_eq!(
            out.get("anthropic-version").and_then(|v| v.to_str().ok()),
            Some("2023-06-01")
        );
    }

    #[test]
    fn copy_passthrough_headers_preserves_incoming_anthropic_version() {
        let mut headers = HeaderMap::new();
        headers.insert("anthropic-version", HeaderValue::from_static("2024-01-01"));

        let out = copy_passthrough_headers(&headers);

        assert_eq!(
            out.get("anthropic-version").and_then(|v| v.to_str().ok()),
            Some("2024-01-01")
        );
    }

    #[test]
    fn copy_passthrough_headers_does_not_forward_accept_encoding() {
        let mut headers = HeaderMap::new();
        headers.insert("accept-encoding", HeaderValue::from_static("gzip, br"));

        let out = copy_passthrough_headers(&headers);

        assert!(out.get("accept-encoding").is_none());
    }
}
