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
use crate::proxy::config::FallbackProviderConfig;

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
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(5)));

    if let Some(config) = upstream_proxy {
        if config.enabled && !config.url.is_empty() {
            let proxy = reqwest::Proxy::all(&config.url)
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
    let mut out = HeaderMap::new();

    for (k, v) in incoming.iter() {
        let key = k.as_str().to_ascii_lowercase();
        match key.as_str() {
            "content-type" | "accept" | "anthropic-version" | "user-agent" => {
                out.insert(k.clone(), v.clone());
            }
            "accept-encoding" | "cache-control" => {
                out.insert(k.clone(), v.clone());
            }
            _ => {}
        }
    }

    out
}

fn set_auth(headers: &mut HeaderMap, api_key: &str) {
    if let Ok(v) = HeaderValue::from_str(&format!("Bearer {}", api_key)) {
        headers.insert(header::AUTHORIZATION, v);
    }
}

fn map_model_for_fallback(original: &str, config: &FallbackProviderConfig) -> String {
    if let Some(mapped) = config.model_mapping.get(original) {
        return mapped.clone();
    }
    original.to_string()
}

pub async fn forward_to_fallback_provider(
    state: &AppState,
    method: Method,
    path: &str,
    incoming_headers: &HeaderMap,
    mut body: Value,
) -> Response {
    let fallback = state.fallback_provider.read().await.clone();
    
    if !fallback.enabled {
        return (StatusCode::BAD_REQUEST, "Fallback provider is disabled").into_response();
    }

    if fallback.api_key.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "Fallback provider api_key is not set").into_response();
    }

    if let Some(model) = body.get("model").and_then(|v| v.as_str()) {
        let mapped = map_model_for_fallback(model, &fallback);
        body["model"] = Value::String(mapped);
    }

    let url = match join_base_url(&fallback.base_url, path) {
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
    set_auth(&mut headers, &fallback.api_key);

    headers
        .entry(header::CONTENT_TYPE)
        .or_insert(HeaderValue::from_static("application/json"));

    let body_bytes = match serde_json::to_vec(&body) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Failed to serialize body for fallback provider: {}", e);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let req = match method {
        Method::POST => client.post(&url),
        Method::GET => client.get(&url),
        Method::PUT => client.put(&url),
        Method::DELETE => client.delete(&url),
        _ => return (StatusCode::METHOD_NOT_ALLOWED, "Unsupported method").into_response(),
    };

    let req = req.headers(headers).body(body_bytes);

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Fallback provider request failed: {}", e);
            return (StatusCode::BAD_GATEWAY, format!("Fallback provider error: {}", e)).into_response();
        }
    };

    let status = resp.status();
    let mut response_headers = HeaderMap::new();
    for (k, v) in resp.headers() {
        response_headers.insert(k.clone(), v.clone());
    }

    let body_bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Failed to read fallback provider response: {}", e);
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };

    let mut response = Response::new(Body::from(body_bytes));
    *response.status_mut() = status;
    *response.headers_mut() = response_headers;

    response
}
