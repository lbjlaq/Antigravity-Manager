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

fn map_model_for_orcarouter(original: &str, state: &crate::proxy::OrcaRouterConfig) -> String {
    let m = original.to_lowercase();
    if let Some(mapped) = state.model_mapping.get(original) {
        return mapped.clone();
    }
    if let Some(mapped) = state.model_mapping.get(&m) {
        return mapped.clone();
    }
    // Already an OrcaRouter / vendor-qualified id — pass through unchanged.
    if m.starts_with("orcarouter:") {
        return original[11..].to_string();
    }
    if m.starts_with("anthropic/") || m.starts_with("openai/") || m.starts_with("deepseek/") {
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
    let base = base.trim_end_matches('/');
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    };
    Ok(format!("{}{}", base, path))
}

fn build_client(
    upstream_proxy: Option<crate::proxy::config::UpstreamProxyConfig>,
    timeout_secs: u64,
) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(timeout_secs.max(5)));

    if let Some(config) = upstream_proxy {
        if config.enabled && !config.url.is_empty() {
            let url = crate::proxy::config::normalize_proxy_url(&config.url);
            let proxy = reqwest::Proxy::all(&url)
                .map_err(|e| format!("Invalid upstream proxy url: {}", e))?;
            builder = builder.proxy(proxy);
        }
    }

    builder
        .tcp_nodelay(true)
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
            // Some clients use these for streaming; safe to pass through.
            "accept-encoding" | "cache-control" => {
                out.insert(k.clone(), v.clone());
            }
            _ => {}
        }
    }

    out
}

fn set_orcarouter_auth(headers: &mut HeaderMap, incoming: &HeaderMap, api_key: &str) {
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

/// Recursively remove cache_control from all nested objects/arrays.
/// Mirrors `zai_anthropic::deep_remove_cache_control` so upstream Anthropic
/// APIs are not tripped by "Extra inputs are not permitted".
pub fn deep_remove_cache_control(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("cache_control");
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

/// Forward an Anthropic-protocol request to the OrcaRouter gateway.
///
/// This is an optional named provider (mirrors the z.ai passthrough): when
/// `proxy.orcarouter` is enabled and its dispatch mode is active, Anthropic
/// requests (`/v1/messages`, `/v1/messages/count_tokens`) are forwarded to
/// the OrcaRouter gateway base URL (default `https://api.orcarouter.ai`).
pub async fn forward_anthropic_json(
    state: &AppState,
    method: Method,
    path: &str,
    incoming_headers: &HeaderMap,
    mut body: Value,
    message_count: usize,
) -> Response {
    let cfg = state.orcarouter.read().await.clone();
    if !cfg.enabled || cfg.dispatch_mode == crate::proxy::OrcaRouterDispatchMode::Off {
        return (StatusCode::BAD_REQUEST, "OrcaRouter is disabled").into_response();
    }

    if cfg.api_key.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "OrcaRouter api_key is not set").into_response();
    }

    if let Some(model) = body.get("model").and_then(|v| v.as_str()) {
        let mapped = map_model_for_orcarouter(model, &cfg);
        body["model"] = Value::String(mapped.clone());

        // [FIX] Caching for OrcaRouter (to support thinking-filter)
        if let Some(sig) = body
            .get("thinking")
            .and_then(|t| t.get("signature"))
            .and_then(|s| s.as_str())
        {
            crate::proxy::SignatureCache::global().cache_session_signature(
                "orcarouter-session",
                sig.to_string(),
                message_count,
            );
            crate::proxy::SignatureCache::global().cache_thinking_family(sig.to_string(), mapped);
        }
    }

    let url = match join_base_url(&cfg.base_url, path) {
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
    set_orcarouter_auth(&mut headers, incoming_headers, &cfg.api_key);

    // Ensure JSON content type.
    headers
        .entry(header::CONTENT_TYPE)
        .or_insert(HeaderValue::from_static("application/json"));

    deep_remove_cache_control(&mut body);

    let body_bytes = serde_json::to_vec(&body).unwrap_or_default();
    let body_len = body_bytes.len();

    tracing::debug!(
        "Forwarding request to OrcaRouter (len: {} bytes): {}",
        body_len,
        url
    );

    let req = client
        .request(method, &url)
        .headers(headers)
        .body(body_bytes);

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
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to build response",
        )
            .into_response()
    })
}
