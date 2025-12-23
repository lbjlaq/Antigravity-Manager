//! 反代路由 (OpenAI / Anthropic 协议)

use std::sync::Arc;
use std::collections::HashMap;

use axum::{
    Router,
    routing::{get, post},
    extract::State,
    response::{IntoResponse, Response, sse::{Event, Sse}},
    http::{StatusCode, HeaderMap},
    Json,
};
use futures::stream::StreamExt;
use tokio::sync::Mutex;
use serde_json::json;

use crate::services::AppState;
use crate::proxy::{OpenAIChatRequest, AnthropicChatRequest, GeminiClient};
use crate::error::AppError;

/// 创建反代路由
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // OpenAI 协议
        .route("/v1/chat/completions", post(chat_completions_handler))
        .route("/v1/models", get(list_models_handler))
        // Anthropic 协议
        .route("/v1/messages", post(anthropic_messages_handler))
}

/// 验证 API Key
fn verify_api_key(headers: &HeaderMap, expected_key: &str) -> Result<(), AppError> {
    let auth_header = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    
    let provided_key = auth_header
        .strip_prefix("Bearer ")
        .or_else(|| auth_header.strip_prefix("bearer "))
        .unwrap_or(auth_header);
    
    // 如果 API Key 是默认值或为空，允许所有请求
    if expected_key.is_empty() || expected_key == "sk-antigravity" {
        return Ok(());
    }
    
    if provided_key.is_empty() || provided_key != expected_key {
        return Err(AppError::Unauthorized("无效的 API Key".to_string()));
    }
    
    Ok(())
}

/// OpenAI Chat Completions 处理器
async fn chat_completions_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<OpenAIChatRequest>,
) -> Response {
    // 验证 API Key
    let config = state.config.read().await;
    if let Err(e) = verify_api_key(&headers, &config.proxy.api_key) {
        return e.into_response();
    }
    let timeout = config.proxy.request_timeout;
    let proxy_url = config.proxy.upstream_proxy.clone();
    drop(config);

    // 获取 Token
    let token = match state.token_manager.get_token().await {
        Some(t) => t,
        None => {
            return AppError::Internal("没有可用的账号".to_string()).into_response();
        }
    };

    let project_id = match &token.project_id {
        Some(id) => id.clone(),
        None => {
            return AppError::Internal("账号缺少 project_id".to_string()).into_response();
        }
    };

    // 创建 Gemini 客户端
    let client = GeminiClient::new(timeout, proxy_url.as_deref());

    // 判断是否流式
    let is_stream = request.stream.unwrap_or(true);

    if is_stream {
        // 流式响应
        match client.stream_generate(&request, &token.access_token, &project_id, &token.session_id).await {
            Ok(stream) => {
                let sse_stream = stream.map(|result| {
                    match result {
                        Ok(data) => {
                            if data == "[DONE]" {
                                Ok(Event::default().data("[DONE]"))
                            } else {
                                Ok(Event::default().data(data))
                            }
                        }
                        Err(e) => {
                            tracing::error!("流错误: {}", e);
                            Err(axum::Error::new(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                e,
                            )))
                        }
                    }
                });

                Sse::new(sse_stream)
                    .keep_alive(axum::response::sse::KeepAlive::default())
                    .into_response()
            }
            Err(e) => {
                tracing::error!("流生成失败: {}", e);
                AppError::Upstream(e).into_response()
            }
        }
    } else {
        // 非流式响应
        match client.generate(&request, &token.access_token, &project_id, &token.session_id).await {
            Ok(response) => {
                // 转换为 OpenAI 格式
                let text = response.get("candidates")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("content"))
                    .and_then(|c| c.get("parts"))
                    .and_then(|p| p.get(0))
                    .and_then(|p| p.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");

                let openai_response = json!({
                    "id": "chatcmpl-antigravity",
                    "object": "chat.completion",
                    "created": chrono::Utc::now().timestamp(),
                    "model": request.model,
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": text
                        },
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": 0,
                        "completion_tokens": 0,
                        "total_tokens": 0
                    }
                });

                Json(openai_response).into_response()
            }
            Err(e) => {
                tracing::error!("生成失败: {}", e);
                AppError::Upstream(e).into_response()
            }
        }
    }
}

/// Anthropic Messages 处理器
async fn anthropic_messages_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<AnthropicChatRequest>,
) -> Response {
    // 验证 API Key
    let config = state.config.read().await;
    if let Err(e) = verify_api_key(&headers, &config.proxy.api_key) {
        return e.into_response();
    }
    let timeout = config.proxy.request_timeout;
    let proxy_url = config.proxy.upstream_proxy.clone();
    let model_mapping = config.proxy.model_mapping.clone();
    drop(config);

    // 获取 Token
    let token = match state.token_manager.get_token().await {
        Some(t) => t,
        None => {
            return AppError::Internal("没有可用的账号".to_string()).into_response();
        }
    };

    let project_id = match &token.project_id {
        Some(id) => id.clone(),
        None => {
            return AppError::Internal("账号缺少 project_id".to_string()).into_response();
        }
    };

    // 创建 Gemini 客户端
    let client = GeminiClient::new(timeout, proxy_url.as_deref());

    // 签名映射 (用于思维链)
    let signature_map: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));

    // Anthropic 请求强制流式
    match client.stream_generate_anthropic(
        &request,
        &token.access_token,
        &project_id,
        &token.session_id,
        &model_mapping,
        signature_map,
    ).await {
        Ok(stream) => {
            let msg_id = format!("msg_{}", uuid::Uuid::new_v4());
            let model = request.model.clone();
            
            // 构造 Anthropic SSE 事件流
            let sse_stream = stream.map(move |result| {
                match result {
                    Ok(data) => {
                        if data == "[DONE]" {
                            // message_stop 事件
                            let event_data = json!({
                                "type": "message_stop"
                            });
                            Ok(Event::default()
                                .event("message_stop")
                                .data(event_data.to_string()))
                        } else {
                            // 解析 chunk
                            if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(&data) {
                                let text = chunk.get("choices")
                                    .and_then(|c| c.get(0))
                                    .and_then(|c| c.get("delta"))
                                    .and_then(|d| d.get("content"))
                                    .and_then(|c| c.as_str())
                                    .unwrap_or("");

                                if !text.is_empty() {
                                    let event_data = json!({
                                        "type": "content_block_delta",
                                        "index": 0,
                                        "delta": {
                                            "type": "text_delta",
                                            "text": text
                                        }
                                    });
                                    return Ok(Event::default()
                                        .event("content_block_delta")
                                        .data(event_data.to_string()));
                                }
                            }
                            
                            // 空事件
                            Ok(Event::default().data(""))
                        }
                    }
                    Err(e) => {
                        tracing::error!("Anthropic 流错误: {}", e);
                        Err(axum::Error::new(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            e,
                        )))
                    }
                }
            });

            // 先发送 message_start 事件
            let start_event = json!({
                "type": "message_start",
                "message": {
                    "id": msg_id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": model,
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": {
                        "input_tokens": 0,
                        "output_tokens": 0
                    }
                }
            });

            let content_block_start = json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "text",
                    "text": ""
                }
            });

            // 创建前置事件流
            let init_events = futures::stream::iter(vec![
                Ok::<_, axum::Error>(Event::default()
                    .event("message_start")
                    .data(start_event.to_string())),
                Ok(Event::default()
                    .event("content_block_start")
                    .data(content_block_start.to_string())),
            ]);

            // 合并流
            let combined_stream = init_events.chain(sse_stream);

            Sse::new(combined_stream)
                .keep_alive(axum::response::sse::KeepAlive::default())
                .into_response()
        }
        Err(e) => {
            tracing::error!("Anthropic 生成失败: {}", e);
            AppError::Upstream(e).into_response()
        }
    }
}

/// 模型列表处理器
async fn list_models_handler(
    State(state): State<Arc<AppState>>,
) -> Response {
    let config = state.config.read().await;
    let models: Vec<serde_json::Value> = config.proxy.model_mapping.keys()
        .map(|id| {
            json!({
                "id": id,
                "object": "model",
                "created": 1700000000,
                "owned_by": "antigravity"
            })
        })
        .collect();

    Json(json!({
        "object": "list",
        "data": models
    })).into_response()
}
