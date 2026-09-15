use axum::{
    body::Body,
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::HashSet;
use tokio::time::Duration;

use crate::proxy::config::{MiniMaxConfig, MiniMaxModelConfig, MiniMaxRegion};
use crate::proxy::server::AppState;

#[derive(Clone, Copy)]
enum MiniMaxProtocol {
    OpenAi,
    Anthropic,
}

pub fn resolve_model(config: &MiniMaxConfig, incoming: &str) -> Option<String> {
    let incoming = incoming.trim();
    let mapped = config
        .model_mapping
        .get(incoming)
        .or_else(|| config.model_mapping.get(&incoming.to_ascii_lowercase()))
        .map(String::as_str)
        .unwrap_or(incoming)
        .trim();
    let candidate = if mapped.to_ascii_lowercase().starts_with("minimax:") {
        mapped.get(8..).unwrap_or_default().trim()
    } else {
        mapped
    };

    config
        .models
        .iter()
        .find(|model| model.model_id.eq_ignore_ascii_case(candidate))
        .map(|model| model.model_id.clone())
}

pub fn should_route(config: &MiniMaxConfig, incoming: &str) -> bool {
    config.enabled && resolve_model(config, incoming).is_some()
}

pub fn model_entries(config: &MiniMaxConfig) -> Vec<Value> {
    if !config.enabled {
        return Vec::new();
    }

    config
        .models
        .iter()
        .map(|model| {
            json!({
                "id": &model.model_id,
                "object": "model",
                "created": 0,
                "owned_by": "minimax",
                "context_window": model.context_window,
                "input_modalities": &model.input_modalities,
                "thinking": &model.thinking,
                "pricing_usd_per_million_tokens": &model.pricing_usd_per_million_tokens,
            })
        })
        .collect()
}

fn selected_base_url(config: &MiniMaxConfig, protocol: MiniMaxProtocol) -> &str {
    match (&config.region, protocol) {
        (MiniMaxRegion::GlobalEn, MiniMaxProtocol::OpenAi) => {
            &config.endpoints.global_openai_base_url
        }
        (MiniMaxRegion::GlobalEn, MiniMaxProtocol::Anthropic) => {
            &config.endpoints.global_anthropic_base_url
        }
        (MiniMaxRegion::CnZh, MiniMaxProtocol::OpenAi) => {
            &config.endpoints.cn_openai_base_url
        }
        (MiniMaxRegion::CnZh, MiniMaxProtocol::Anthropic) => {
            &config.endpoints.cn_anthropic_base_url
        }
    }
}

fn join_base_url(base: &str, path: &str) -> Result<String, String> {
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("MiniMax base URL is empty".to_string());
    }

    let mut path = path.trim();
    if base.ends_with("/v1") && path.starts_with("/v1/") {
        path = path.get(3..).unwrap_or(path);
    }
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
                .map_err(|e| format!("Invalid upstream proxy URL: {}", e))?;
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

    for (key, value) in incoming.iter() {
        match key.as_str().to_ascii_lowercase().as_str() {
            "content-type" | "accept" | "accept-encoding" | "cache-control"
            | "anthropic-version" | "anthropic-beta" | "user-agent" => {
                out.insert(key.clone(), value.clone());
            }
            _ => {}
        }
    }

    out
}

fn collect_content_modalities(value: &Value, modalities: &mut HashSet<String>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_content_modalities(item, modalities);
            }
        }
        Value::Object(map) => {
            if let Some(kind) = map.get("type").and_then(Value::as_str) {
                match kind.to_ascii_lowercase().as_str() {
                    "image" | "image_url" | "input_image" => {
                        modalities.insert("image".to_string());
                    }
                    "video" | "video_url" | "input_video" => {
                        modalities.insert("video".to_string());
                    }
                    "audio" | "audio_url" | "input_audio" => {
                        modalities.insert("audio".to_string());
                    }
                    _ => {}
                }
            }
            if let Some(content) = map.get("content") {
                collect_content_modalities(content, modalities);
            }
        }
        _ => {}
    }
}

fn request_modalities(body: &Value) -> HashSet<String> {
    let mut modalities = HashSet::from(["text".to_string()]);
    for field in ["messages", "input"] {
        if let Some(value) = body.get(field) {
            collect_content_modalities(value, &mut modalities);
        }
    }
    modalities
}

fn validate_modalities(model: &MiniMaxModelConfig, body: &Value) -> Result<(), String> {
    let requested = request_modalities(body);
    let allowed: HashSet<_> = model
        .input_modalities
        .iter()
        .map(|modality| modality.to_ascii_lowercase())
        .collect();
    let unsupported: Vec<_> = requested.difference(&allowed).cloned().collect();

    if unsupported.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} does not support input modalities: {}",
            model.model_id,
            unsupported.join(", ")
        ))
    }
}

fn normalize_request(config: &MiniMaxConfig, mut body: Value) -> Result<Value, String> {
    let incoming = body
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| "MiniMax requests require a model".to_string())?;
    let model_id = resolve_model(config, incoming)
        .ok_or_else(|| format!("Unsupported MiniMax model: {}", incoming))?;
    let model = config
        .models
        .iter()
        .find(|model| model.model_id == model_id)
        .ok_or_else(|| format!("Missing MiniMax metadata for {}", model_id))?;

    validate_modalities(model, &body)?;

    if model.thinking.iter().any(|mode| mode == "always_on")
        && body
            .get("thinking")
            .and_then(|thinking| thinking.get("type"))
            .and_then(Value::as_str)
            .is_some_and(|kind| kind.eq_ignore_ascii_case("disabled"))
    {
        return Err(format!("{} does not support disabled thinking", model_id));
    }

    if model.thinking.iter().any(|mode| mode == "adaptive") {
        if let Some(thinking) = body.get_mut("thinking").and_then(Value::as_object_mut) {
            if thinking
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.eq_ignore_ascii_case("enabled"))
            {
                thinking.insert("type".to_string(), Value::String("adaptive".to_string()));
                thinking.remove("budget_tokens");
                thinking.remove("budgetTokens");
                thinking.remove("budget");
            }
        }
    }

    body["model"] = Value::String(model_id);
    Ok(body)
}

fn error_response(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "type": "invalid_request_error",
                "message": message.into(),
            }
        })),
    )
        .into_response()
}

async fn forward_json(
    state: &AppState,
    protocol: MiniMaxProtocol,
    method: Method,
    path: &str,
    incoming_headers: &HeaderMap,
    body: Value,
) -> Response {
    let config = state.minimax.read().await.clone();
    if !config.enabled {
        return error_response(StatusCode::BAD_REQUEST, "MiniMax is disabled");
    }
    if config.api_key.trim().is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "MiniMax API key is not set");
    }

    let body = match normalize_request(&config, body) {
        Ok(body) => body,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    let url = match join_base_url(selected_base_url(&config, protocol), path) {
        Ok(url) => url,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    let upstream_proxy = state.upstream_proxy.read().await.clone();
    let client = match build_client(Some(upstream_proxy), state.request_timeout) {
        Ok(client) => client,
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };

    let mut headers = copy_passthrough_headers(incoming_headers);
    let auth = match HeaderValue::from_str(&format!("Bearer {}", config.api_key)) {
        Ok(value) => value,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "Invalid MiniMax API key"),
    };
    headers.insert(header::AUTHORIZATION, auth);
    headers
        .entry(header::CONTENT_TYPE)
        .or_insert(HeaderValue::from_static("application/json"));

    let body = match serde_json::to_vec(&body) {
        Ok(body) => body,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to encode MiniMax request: {}", error),
            )
        }
    };
    let response = match client
        .request(method, url)
        .headers(headers)
        .body(body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                format!("MiniMax upstream request failed: {}", error),
            )
        }
    };

    let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut outgoing = Response::builder().status(status);
    if let Some(content_type) = response.headers().get(header::CONTENT_TYPE) {
        outgoing = outgoing.header(header::CONTENT_TYPE, content_type.clone());
    }
    let stream = response.bytes_stream().map(|chunk| match chunk {
        Ok(bytes) => Ok::<Bytes, std::io::Error>(bytes),
        Err(error) => Ok(Bytes::from(format!("MiniMax upstream stream error: {}", error))),
    });

    outgoing.body(Body::from_stream(stream)).unwrap_or_else(|_| {
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to build MiniMax response",
        )
    })
}

pub async fn forward_openai_json(
    state: &AppState,
    method: Method,
    path: &str,
    incoming_headers: &HeaderMap,
    body: Value,
) -> Response {
    forward_json(
        state,
        MiniMaxProtocol::OpenAi,
        method,
        path,
        incoming_headers,
        body,
    )
    .await
}

pub async fn forward_anthropic_json(
    state: &AppState,
    method: Method,
    path: &str,
    incoming_headers: &HeaderMap,
    body: Value,
) -> Response {
    forward_json(
        state,
        MiniMaxProtocol::Anthropic,
        method,
        path,
        incoming_headers,
        body,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_all_regional_protocol_endpoints() {
        let mut config = MiniMaxConfig::default();
        assert_eq!(
            join_base_url(
                selected_base_url(&config, MiniMaxProtocol::OpenAi),
                "/v1/chat/completions"
            )
            .unwrap(),
            "https://api.minimax.io/v1/chat/completions"
        );
        assert_eq!(
            join_base_url(
                selected_base_url(&config, MiniMaxProtocol::Anthropic),
                "/v1/messages"
            )
            .unwrap(),
            "https://api.minimax.io/anthropic/v1/messages"
        );

        config.region = MiniMaxRegion::CnZh;
        assert_eq!(
            join_base_url(
                selected_base_url(&config, MiniMaxProtocol::OpenAi),
                "/v1/chat/completions"
            )
            .unwrap(),
            "https://api.minimaxi.com/v1/chat/completions"
        );
        assert_eq!(
            join_base_url(
                selected_base_url(&config, MiniMaxProtocol::Anthropic),
                "/v1/messages"
            )
            .unwrap(),
            "https://api.minimaxi.com/anthropic/v1/messages"
        );
    }

    #[test]
    fn resolves_exact_prefixed_and_mapped_models() {
        let mut config = MiniMaxConfig::default();
        config
            .model_mapping
            .insert("fast".to_string(), "MiniMax-M2.7".to_string());

        assert_eq!(resolve_model(&config, "minimax-m3").as_deref(), Some("MiniMax-M3"));
        assert_eq!(
            resolve_model(&config, "minimax:MiniMax-M2.7").as_deref(),
            Some("MiniMax-M2.7")
        );
        assert_eq!(resolve_model(&config, "fast").as_deref(), Some("MiniMax-M2.7"));
        assert_eq!(resolve_model(&config, "unknown"), None);
    }

    #[test]
    fn normalizes_adaptive_thinking_and_rejects_disabled_always_on_thinking() {
        let config = MiniMaxConfig::default();
        let adaptive = normalize_request(
            &config,
            json!({
                "model": "MiniMax-M3",
                "messages": [{"role": "user", "content": "hello"}],
                "thinking": {"type": "enabled", "budget_tokens": 4096}
            }),
        )
        .unwrap();
        assert_eq!(adaptive["thinking"]["type"], "adaptive");
        assert!(adaptive["thinking"].get("budget_tokens").is_none());

        let error = normalize_request(
            &config,
            json!({
                "model": "MiniMax-M2.7",
                "messages": [{"role": "user", "content": "hello"}],
                "thinking": {"type": "disabled"}
            }),
        )
        .unwrap_err();
        assert!(error.contains("does not support disabled thinking"));
    }

    #[test]
    fn validates_model_input_modalities() {
        let config = MiniMaxConfig::default();
        normalize_request(
            &config,
            json!({
                "model": "MiniMax-M3",
                "messages": [{
                    "role": "user",
                    "content": [{"type": "video_url", "video_url": {"url": "https://example.test/a.mp4"}}]
                }]
            }),
        )
        .unwrap();

        let error = normalize_request(
            &config,
            json!({
                "model": "MiniMax-M2.7",
                "messages": [{
                    "role": "user",
                    "content": [{"type": "image_url", "image_url": {"url": "https://example.test/a.png"}}]
                }]
            }),
        )
        .unwrap_err();
        assert!(error.contains("image"));
    }
}
