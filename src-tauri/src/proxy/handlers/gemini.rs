// Gemini Handler
use axum::{extract::State, extract::{Json, Path}, http::StatusCode, response::IntoResponse};
use serde_json::{json, Value};
use tracing::{debug, error};

use crate::proxy::mappers::gemini::{wrap_request, unwrap_response};
use crate::proxy::server::AppState;
 
const MAX_RETRY_ATTEMPTS: usize = 3;
 
/// 处理 generateContent 和 streamGenerateContent
/// 路径参数: model_name, method (e.g. "gemini-pro", "generateContent")
pub async fn handle_generate(
    State(state): State<AppState>,
    Path(model_action): Path<String>,
    Json(body): Json<Value>
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // 解析 model:method
    let (model_name, method) = if let Some((m, action)) = model_action.rsplit_once(':') {
        (m.to_string(), action.to_string())
    } else {
        (model_action, "generateContent".to_string())
    };

    crate::modules::logger::log_info(&format!("Received Gemini request: {}/{}", model_name, method));

    // 1. 验证方法
    if method != "generateContent" && method != "streamGenerateContent" {
        return Err((StatusCode::BAD_REQUEST, format!("Unsupported method: {}", method)));
    }
    let is_stream = method == "streamGenerateContent";

    // 2. 获取 UpstreamClient 和 TokenManager
    let upstream = state.upstream.clone();
    let token_manager = state.token_manager;
    let pool_size = token_manager.len();
    let max_attempts = MAX_RETRY_ATTEMPTS.min(pool_size).max(1);
    
    let mut last_error = String::new();

    for attempt in 0..max_attempts {
        // 3. 模型路由与配置解析
        let mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
            &model_name,
            &*state.custom_mapping.read().await,
            &*state.openai_mapping.read().await,
            &*state.anthropic_mapping.read().await,
        );
        // 提取 tools 列表以进行联网探测 (Gemini 风格可能是嵌套的)
        let tools_val: Option<Vec<Value>> = body.get("tools").and_then(|t| t.as_array()).map(|arr| {
            let mut flattened = Vec::new();
            for tool_entry in arr {
                if let Some(decls) = tool_entry.get("functionDeclarations").and_then(|v| v.as_array()) {
                    flattened.extend(decls.iter().cloned());
                } else {
                    flattened.push(tool_entry.clone());
                }
            }
            flattened
        });

        let config = crate::proxy::mappers::common_utils::resolve_request_config(&model_name, &mapped_model, &tools_val);

        // 4. 获取 Token (使用准确的 request_type)
        // 关键：在重试尝试 (attempt > 0) 时强制轮换账号
        let (access_token, project_id, email) =
            match token_manager.get_token(&config.request_type, attempt > 0, None).await {
            Ok(t) => t,
            Err(e) => {
                return Err((StatusCode::SERVICE_UNAVAILABLE, format!("Token error: {}", e)));
            }
        };

        tracing::info!("Using account: {} for request (type: {})", email, config.request_type);

        // 5. 包装请求 (project injection)
        let wrapped_body = wrap_request(&body, &project_id, &mapped_model);

        // 5. 上游调用
        let query_string = if is_stream { Some("alt=sse") } else { None };
        let upstream_method = if is_stream { "streamGenerateContent" } else { "generateContent" };

        let response = match upstream
            .call_v1_internal(upstream_method, &access_token, wrapped_body, query_string)
            .await {
                Ok(r) => r,
                Err(e) => {
                    last_error = e.clone();
                    tracing::warn!("Gemini Request failed on attempt {}/{}: {}", attempt + 1, max_attempts, e);
                    continue;
                }
            };

        let status = response.status();
        if status.is_success() {
            // 6. 响应处理
            if is_stream {
                use axum::body::Body;
                use axum::response::Response;
                use bytes::{Bytes, BytesMut};
                use futures::StreamExt;
                
                let mut response_stream = response.bytes_stream();
                let mut buffer = BytesMut::new();

                let stream = async_stream::stream! {
                    while let Some(item) = response_stream.next().await {
                        match item {
                            Ok(bytes) => {
                                debug!("[Gemini-SSE] Received chunk: {} bytes", bytes.len());
                                buffer.extend_from_slice(&bytes);
                                while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                                    let line_raw = buffer.split_to(pos + 1);
                                    if let Ok(line_str) = std::str::from_utf8(&line_raw) {
                                        let line = line_str.trim();
                                        if line.is_empty() { continue; }
                                        
                                        if line.starts_with("data: ") {
                                            let json_part = line.trim_start_matches("data: ").trim();
                                            if json_part == "[DONE]" {
                                                yield Ok::<Bytes, String>(Bytes::from("data: [DONE]\n\n"));
                                                continue;
                                            }
                                            
                                            match serde_json::from_str::<Value>(json_part) {
                                                Ok(mut json) => {
                                                    // Unwrap v1internal response wrapper
                                                    if let Some(inner) = json.get_mut("response").map(|v| v.take()) {
                                                        let new_line = format!("data: {}\n\n", serde_json::to_string(&inner).unwrap_or_default());
                                                        yield Ok::<Bytes, String>(Bytes::from(new_line));
                                                    } else {
                                                        yield Ok::<Bytes, String>(Bytes::from(format!("data: {}\n\n", serde_json::to_string(&json).unwrap_or_default())));
                                                    }
                                                }
                                                Err(e) => {
                                                    debug!("[Gemini-SSE] JSON parse error: {}, passing raw line", e);
                                                    yield Ok::<Bytes, String>(Bytes::from(format!("{}\n\n", line)));
                                                }
                                            }
                                        } else {
                                            // Non-data lines (comments, etc.)
                                            yield Ok::<Bytes, String>(Bytes::from(format!("{}\n\n", line)));
                                        }
                                    } else {
                                        // Non-UTF8 data? Just pass it through or skip
                                        debug!("[Gemini-SSE] Non-UTF8 line encountered");
                                        yield Ok::<Bytes, String>(line_raw.freeze());
                                    }
                                }
                            }
                            Err(e) => {
                                error!("[Gemini-SSE] Connection error: {}", e);
                                yield Err(format!("Stream error: {}", e));
                            }
                        }
                    }
                };
                
                let body = Body::from_stream(stream);
                return Ok(Response::builder()
                    .header("Content-Type", "text/event-stream")
                    .header("Cache-Control", "no-cache")
                    .header("Connection", "keep-alive")
                    .body(body)
                    .unwrap()
                    .into_response());
            }

            let gemini_resp: Value = response
                .json()
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Parse error: {}", e)))?;

            let unwrapped = unwrap_response(&gemini_resp);
            return Ok(Json(unwrapped).into_response());
        }

        // 处理错误并重试
        let status_code = status.as_u16();
        let error_text = response.text().await.unwrap_or_default();
        last_error = format!("HTTP {}: {}", status_code, error_text);
 
        // 只有 429 (限流), 403 (权限/地区限制) 和 401 (认证失效) 触发账号轮换
        if status_code == 429 || status_code == 403 || status_code == 401 {
            // 只有明确包含 "QUOTA_EXHAUSTED" 才停止，避免误判上游的频率限制提示 (如 "check quota")
            if status_code == 429 && error_text.contains("QUOTA_EXHAUSTED") {
                error!("Gemini Quota exhausted (429) on attempt {}/{}, stopping to protect pool.", attempt + 1, max_attempts);
                return Err((status, error_text));
            }

            tracing::warn!("Gemini Upstream {} on attempt {}/{}, rotating account", status_code, attempt + 1, max_attempts);
            continue;
        }
 
        // 404 等由于模型配置或路径错误的 HTTP 异常，直接报错，不进行无效轮换
        error!("Gemini Upstream non-retryable error {}: {}", status_code, error_text);
        return Err((status, error_text));
    }

    Ok((StatusCode::TOO_MANY_REQUESTS, format!("All accounts exhausted. Last error: {}", last_error)).into_response())
}

pub async fn handle_list_models(State(state): State<AppState>) -> Result<impl IntoResponse, (StatusCode, String)> {
    let model_group = "gemini";
    let (access_token, _, _) = state.token_manager.get_token(model_group, false, None).await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, format!("Token error: {}", e)))?;

    // Fetch from upstream
    let upstream_models = state.upstream.fetch_available_models(&access_token).await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;

    // Transform map to Gemini list format
    let mut models = Vec::new();
    if let Some(obj) = upstream_models.as_object() {
        tracing::info!("Upstream models keys: {:?}", obj.keys());
        for (key, value) in obj {
             let description = value.get("description").and_then(|v| v.as_str()).unwrap_or("");
             let display_name = value.get("displayName").and_then(|v| v.as_str()).unwrap_or(key);
             
             models.push(json!({
                 "name": format!("models/{}", key),
                 "version": "001",
                 "displayName": display_name,
                 "description": description,
                 "inputTokenLimit": 128000,
                 "outputTokenLimit": 8192,
                 "supportedGenerationMethods": ["generateContent", "countTokens"],
                 "temperature": 1.0,
                 "topP": 0.95,
                 "topK": 64
             }));
        }
    }
    
    // Fallback
    if models.is_empty() {
         models.push(json!({
             "name": "models/gemini-2.5-pro", 
             "displayName": "Gemini 2.5 Pro", 
             "supportedGenerationMethods": ["generateContent", "countTokens"]
         }));
    }

    Ok(Json(json!({ "models": models })))
}

pub async fn handle_get_model(Path(model_name): Path<String>) -> impl IntoResponse {
    Json(json!({
        "name": format!("models/{}", model_name),
        "displayName": model_name
    }))
}

pub async fn handle_count_tokens(State(state): State<AppState>, Path(_model_name): Path<String>, Json(_body): Json<Value>) -> Result<impl IntoResponse, (StatusCode, String)> {
    let model_group = "gemini";
    let (_access_token, _project_id, _) = state.token_manager.get_token(model_group, false, None).await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, format!("Token error: {}", e)))?;
    
    Ok(Json(json!({"totalTokens": 0})))
}
