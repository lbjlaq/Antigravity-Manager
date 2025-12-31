use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures::StreamExt;
use tokio::time::Duration;

use crate::proxy::server::AppState;

fn build_client(
    upstream_proxy: crate::proxy::config::UpstreamProxyConfig,
    timeout_secs: u64,
) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(5)));

    if upstream_proxy.enabled && !upstream_proxy.url.is_empty() {
        let proxy = reqwest::Proxy::all(&upstream_proxy.url)
            .map_err(|e| format!("Invalid upstream proxy url: {}", e))?;
        builder = builder.proxy(proxy);
    }

    builder.build().map_err(|e| format!("Failed to build HTTP client: {}", e))
}

fn copy_passthrough_headers(incoming: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (k, v) in incoming.iter() {
        let key = k.as_str().to_ascii_lowercase();
        match key.as_str() {
            "content-type" | "accept" | "user-agent" => {
                out.insert(k.clone(), v.clone());
            }
            _ => {}
        }
    }
    out
}

async fn forward_mcp(
    state: &AppState,
    incoming_headers: HeaderMap,
    method: Method,
    upstream_url: &str,
    body: Body,
) -> Response {
    let zai = state.zai.read().await.clone();
    if !zai.enabled || zai.api_key.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "z.ai is not configured").into_response();
    }

    if !zai.mcp.enabled {
        return StatusCode::NOT_FOUND.into_response();
    }

    let upstream_proxy = state.upstream_proxy.read().await.clone();
    let client = match build_client(upstream_proxy, state.request_timeout) {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };

    let collected = match to_bytes(body, 100 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Failed to read request body: {}", e),
            )
                .into_response();
        }
    };

    let mut headers = copy_passthrough_headers(&incoming_headers);
    if let Ok(v) = HeaderValue::from_str(&format!("Bearer {}", zai.api_key)) {
        headers.insert(header::AUTHORIZATION, v);
    }

    let req = client
        .request(method, upstream_url)
        .headers(headers)
        .body(collected);

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

    let stream = resp.bytes_stream().map(|chunk| match chunk {
        Ok(b) => Ok::<Bytes, std::io::Error>(b),
        Err(e) => Ok(Bytes::from(format!("Upstream stream error: {}", e))),
    });

    out.body(Body::from_stream(stream)).unwrap_or_else(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to build response").into_response()
    })
}

pub async fn handle_web_search_prime(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    body: Body,
) -> Response {
    let zai = state.zai.read().await.clone();
    if !zai.mcp.web_search_enabled {
        return StatusCode::NOT_FOUND.into_response();
    }
    drop(zai);

    forward_mcp(
        &state,
        headers,
        method,
        "https://api.z.ai/api/mcp/web_search_prime/mcp",
        body,
    )
    .await
}

pub async fn handle_web_reader(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    body: Body,
) -> Response {
    let zai = state.zai.read().await.clone();
    if !zai.mcp.web_reader_enabled {
        return StatusCode::NOT_FOUND.into_response();
    }
    drop(zai);

    forward_mcp(
        &state,
        headers,
        method,
        "https://api.z.ai/api/mcp/web_reader/mcp",
        body,
    )
    .await
}
