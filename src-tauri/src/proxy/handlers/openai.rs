// OpenAI Handler
use axum::{extract::State, extract::Json, http::StatusCode, response::IntoResponse};
use serde_json::{json, Value};
use tracing::{debug, error};

use crate::proxy::mappers::openai::{transform_openai_request, transform_openai_response, OpenAIRequest};
// use crate::proxy::upstream::client::UpstreamClient; // 通过 state 获取
use crate::proxy::server::AppState;
 
const MAX_RETRY_ATTEMPTS: usize = 3;
 
pub async fn handle_chat_completions(
    State(state): State<AppState>,
    Json(body): Json<Value>
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut openai_req: OpenAIRequest = serde_json::from_value(body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid request: {}", e)))?;

    // Safety: Ensure messages is not empty
    if openai_req.messages.is_empty() {
        tracing::warn!("Received request with empty messages, injecting fallback...");
        openai_req.messages.push(crate::proxy::mappers::openai::OpenAIMessage {
            role: "user".to_string(),
            content: Some(crate::proxy::mappers::openai::OpenAIContent::String(" ".to_string())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
    }

    debug!("Received OpenAI request for model: {}", openai_req.model);

    // 1. 获取 UpstreamClient (Clone handle)
    let upstream = state.upstream.clone();
    let token_manager = state.token_manager;
    let pool_size = token_manager.len();
    let max_attempts = MAX_RETRY_ATTEMPTS.min(pool_size).max(1);
    
    let mut last_error = String::new();
 
    for attempt in 0..max_attempts {
        // 2. 预解析模型路由与配置
        let mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
            &openai_req.model,
            &*state.custom_mapping.read().await,
            &*state.openai_mapping.read().await,
            &*state.anthropic_mapping.read().await,
        );
        let config = crate::proxy::mappers::common_utils::resolve_request_config(&openai_req.model, &mapped_model);

        // 3. 获取 Token (使用准确的 request_type)
        let (access_token, project_id, email) = match token_manager.get_token(&config.request_type, false).await {
            Ok(t) => t,
            Err(e) => {
                return Err((StatusCode::SERVICE_UNAVAILABLE, format!("Token error: {}", e)));
            }
        };

        tracing::info!("Using account: {} for request (type: {})", email, config.request_type);

        // 4. 转换请求
        let gemini_body = transform_openai_request(&openai_req, &project_id, &mapped_model);

        // 5. 发送请求
        let list_response = openai_req.stream;
        let method = if list_response { "streamGenerateContent" } else { "generateContent" };
        let query_string = if list_response { Some("alt=sse") } else { None };

        let response = match upstream
            .call_v1_internal(method, &access_token, gemini_body, query_string)
            .await {
                Ok(r) => r,
                Err(e) => {
                    last_error = e.clone();
                    tracing::warn!("OpenAI Request failed on attempt {}/{}: {}", attempt + 1, max_attempts, e);
                    continue;
                }
            };

        let status = response.status();
        if status.is_success() {
            // 5. 处理流式 vs 非流式
            if list_response {
                use crate::proxy::mappers::openai::streaming::create_openai_sse_stream;
                use axum::response::Response;
                use axum::body::Body;
                // Removed redundant StreamExt

                let gemini_stream = response.bytes_stream();
                let openai_stream = create_openai_sse_stream(Box::pin(gemini_stream), openai_req.model.clone());
                let body = Body::from_stream(openai_stream);

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

            let openai_response = transform_openai_response(&gemini_resp);
            return Ok(Json(openai_response).into_response());
        }

        // 处理特定错误并重试
        let status_code = status.as_u16();
        let error_text = response.text().await.unwrap_or_default();
        last_error = format!("HTTP {}: {}", status_code, error_text);
 
        // 429 智能处理
        if status_code == 429 {
            // 1. 优先尝试解析 RetryInfo (由 Google Cloud 直接下发)
            if let Some(delay_ms) = crate::proxy::upstream::retry::parse_retry_delay(&error_text) {
                let actual_delay = delay_ms.saturating_add(200).min(10_000);
                tracing::warn!(
                    "OpenAI Upstream 429 on attempt {}/{}, waiting {}ms then retrying",
                    attempt + 1,
                    max_attempts,
                    actual_delay
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(actual_delay)).await;
                continue;
            }

            // 2. 只有明确包含 "QUOTA_EXHAUSTED" 才停止，避免误判频率提示 (如 "check quota")
            if error_text.contains("QUOTA_EXHAUSTED") {
                error!("OpenAI Quota exhausted (429) on attempt {}/{}, stopping to protect pool.", attempt + 1, max_attempts);
                return Err((status, error_text));
            }

            // 3. 其他 429 情况（如无重试指示的频率限制），轮换账号
            tracing::warn!("OpenAI Upstream 429 on attempt {}/{}, rotating account", attempt + 1, max_attempts);
            continue;
        }

        // 只有 403 (权限/地区限制) 和 401 (认证失效) 触发账号轮换
        if status_code == 403 || status_code == 401 {
            tracing::warn!("OpenAI Upstream {} on attempt {}/{}, rotating account", status_code, attempt + 1, max_attempts);
            continue;
        }
 
        // 404 等由于模型配置或路径错误的 HTTP 异常，直接报错，不进行无效轮换
        error!("OpenAI Upstream non-retryable error {}: {}", status_code, error_text);
        return Err((status, error_text));
    }

    // 所有尝试均失败
    Err((StatusCode::TOO_MANY_REQUESTS, format!("All accounts exhausted. Last error: {}", last_error)))
}

/// 处理 Legacy Completions API (/v1/completions)
/// 将 Prompt 转换为 Chat Message 格式，复用 handle_chat_completions
pub async fn handle_completions(
    State(state): State<AppState>,
    Json(mut body): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    tracing::info!("Received /v1/completions or /v1/responses payload: {:?}", body);

    let is_codex_style = body.get("input").is_some() && body.get("instructions").is_some();
    
    // 1. Convert Payload to Messages (Shared Chat Format)
    if is_codex_style {
        let instructions = body.get("instructions").and_then(|v| v.as_str()).unwrap_or_default();
        let input_items = body.get("input").and_then(|v| v.as_array());
        
        let mut messages = Vec::new();
        
        // System Instructions
        if !instructions.is_empty() {
            messages.push(json!({ "role": "system", "content": instructions }));
        }

        let mut call_id_to_name = std::collections::HashMap::new();

        // Pass 1: Build Call ID to Name Map
        if let Some(items) = input_items {
            for item in items {
                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                 match item_type {
                    "function_call" | "local_shell_call" | "web_search_call" => {
                        let call_id = item.get("call_id").and_then(|v| v.as_str())
                                     .or_else(|| item.get("id").and_then(|v| v.as_str()))
                                     .unwrap_or("unknown");
                        
                        let name = if item_type == "local_shell_call" {
                            "shell"
                        } else if item_type == "web_search_call" {
                            "google_search"
                        } else {
                            item.get("name").and_then(|v| v.as_str()).unwrap_or("unknown")
                        };
                        
                        call_id_to_name.insert(call_id.to_string(), name.to_string());
                        tracing::debug!("Mapped call_id {} to name {}", call_id, name);
                    }
                    _ => {}
                }
            }
        }

        // Pass 2: Map Input Items to Messages
        if let Some(items) = input_items {
            for item in items {
                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match item_type {
                    "message" => {
                        let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                        let content = item.get("content").and_then(|v| v.as_array());
                        let mut text_parts = Vec::new();
                        if let Some(parts) = content {
                            for part in parts {
                                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                    text_parts.push(text);
                                }
                            }
                        }
                        messages.push(json!({
                            "role": role,
                            "content": text_parts.join("\n")
                        }));
                    }
                    "function_call" | "local_shell_call" | "web_search_call" => {
                        let mut name = item.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                        let mut args_str = item.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}").to_string();
                        let call_id = item.get("call_id").and_then(|v| v.as_str()).or_else(|| item.get("id").and_then(|v| v.as_str())).unwrap_or("unknown");
                        
                        // Handle native shell calls
                        if item_type == "local_shell_call" {
                            name = "shell";
                            if let Some(action) = item.get("action") {
                                if let Some(exec) = action.get("exec") {
                                    // Map to ShellCommandToolCallParams (string command) or ShellToolCallParams (array command)
                                    // Most LLMs prefer a single string for shell
                                    let mut args_obj = serde_json::Map::new();
                                    if let Some(cmd) = exec.get("command") {
                                        // CRITICAL FIX: The 'shell' tool schema defines 'command' as an ARRAY of strings.
                                        // We MUST pass it as an array, not a joined string, otherwise Gemini rejects with 400 INVALID_ARGUMENT.
                                        let cmd_val = if cmd.is_string() {
                                             json!([cmd]) // Wrap in array
                                        } else {
                                             cmd.clone() // Assume already array
                                        };
                                        args_obj.insert("command".to_string(), cmd_val);
                                    }
                                    if let Some(wd) = exec.get("working_directory").or(exec.get("workdir")) {
                                        args_obj.insert("workdir".to_string(), wd.clone());
                                    }
                                    args_str = serde_json::to_string(&args_obj).unwrap_or("{}".to_string());
                                }
                            }
                        } else if item_type == "web_search_call" {
                            name = "google_search";
                            if let Some(action) = item.get("action") {
                                let mut args_obj = serde_json::Map::new();
                                if let Some(q) = action.get("query") {
                                    args_obj.insert("query".to_string(), q.clone());
                                }
                                args_str = serde_json::to_string(&args_obj).unwrap_or("{}".to_string());
                            }
                        }

                        messages.push(json!({
                            "role": "assistant",
                            "tool_calls": [
                                {
                                    "id": call_id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": args_str
                                    }
                                }
                            ]
                        }));
                    }
                    "function_call_output" | "custom_tool_call_output" => {
                        let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("unknown");
                        let output = item.get("output");
                        let output_str = if let Some(o) = output {
                            if o.is_string() { o.as_str().unwrap().to_string() }
                            else if let Some(content) = o.get("content").and_then(|v| v.as_str()) { content.to_string() }
                            else { o.to_string() }
                        } else { "".to_string() };

                        let name = call_id_to_name.get(call_id).cloned().unwrap_or_else(|| {
                            // Fallback: if unknown and we see function_call_output, it's likely "shell" in this context
                            tracing::warn!("Unknown tool name for call_id {}, defaulting to 'shell'", call_id);
                            "shell".to_string()
                        });

                        messages.push(json!({
                            "role": "tool",
                            "tool_call_id": call_id,
                            "name": name,
                            "content": output_str
                        }));
                    }
                    _ => {}
                }
            }
        }

        if let Some(obj) = body.as_object_mut() {
            obj.insert("messages".to_string(), json!(messages));
        }
    } else if let Some(prompt_val) = body.get("prompt") {
        // Legacy OpenAI Style: prompt -> Chat
        let prompt_str = match prompt_val {
            Value::String(s) => s.clone(),
            Value::Array(arr) => arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join("\n"),
            _ => prompt_val.to_string(),
        };
        let messages = json!([ { "role": "user", "content": prompt_str } ]);
        if let Some(obj) = body.as_object_mut() {
            obj.remove("prompt");
            obj.insert("messages".to_string(), messages);
        }
    }

    // 2. Reuse handle_chat_completions logic (wrapping with custom handler or direct call)
    // Actually, due to SSE handling differences (Codex uses different event format), we replicate the loop here or abstract it.
    // For now, let's replicate the core loop but with Codex specific SSE mapping.

    let mut openai_req: OpenAIRequest = serde_json::from_value(body.clone())
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid request: {}", e)))?;

    // Safety: Inject empty message if needed
    if openai_req.messages.is_empty() {
        openai_req.messages.push(crate::proxy::mappers::openai::OpenAIMessage {
            role: "user".to_string(),
            content: Some(crate::proxy::mappers::openai::OpenAIContent::String(" ".to_string())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
    }

    let upstream = state.upstream.clone();
    let token_manager = state.token_manager;
    let pool_size = token_manager.len();
    let max_attempts = MAX_RETRY_ATTEMPTS.min(pool_size).max(1);
    
    let mut last_error = String::new();

    for _attempt in 0..max_attempts {
        let mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
            &openai_req.model,
            &*state.custom_mapping.read().await,
            &*state.openai_mapping.read().await,
            &*state.anthropic_mapping.read().await,
        );
        let config = crate::proxy::mappers::common_utils::resolve_request_config(&openai_req.model, &mapped_model);

        let (access_token, project_id, email) = match token_manager.get_token(&config.request_type, false).await {
            Ok(t) => t,
            Err(e) => return Err((StatusCode::SERVICE_UNAVAILABLE, format!("Token error: {}", e))),
        };

        tracing::info!("Using account: {} for completions request (type: {})", email, config.request_type);

        let gemini_body = transform_openai_request(&openai_req, &project_id, &mapped_model);
        let list_response = openai_req.stream;
        let method = if list_response { "streamGenerateContent" } else { "generateContent" };
        let query_string = if list_response { Some("alt=sse") } else { None };

        let response = match upstream.call_v1_internal(method, &access_token, gemini_body, query_string).await {
            Ok(r) => r,
            Err(e) => {
                last_error = e.clone();
                continue;
            }
        };

        let status = response.status();
        if status.is_success() {
            if list_response {
                use axum::response::Response;
                use axum::body::Body;

                let gemini_stream = response.bytes_stream();
                let body = if is_codex_style {
                    use crate::proxy::mappers::openai::streaming::create_codex_sse_stream;
                    let s = create_codex_sse_stream(Box::pin(gemini_stream), openai_req.model.clone());
                    Body::from_stream(s)
                } else {
                    use crate::proxy::mappers::openai::streaming::create_legacy_sse_stream;
                    let s = create_legacy_sse_stream(Box::pin(gemini_stream), openai_req.model.clone());
                    Body::from_stream(s)
                };

                return Ok(Response::builder()
                    .header("Content-Type", "text/event-stream")
                    .header("Cache-Control", "no-cache")
                    .header("Connection", "keep-alive")
                    .body(body)
                    .unwrap()
                    .into_response());
            }

            let gemini_resp: Value = response.json().await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Parse error: {}", e)))?;

            let chat_resp = transform_openai_response(&gemini_resp);
            
            // Map Chat Response -> Legacy Completions Response
            let choices = chat_resp.choices.iter().map(|c| {
                json!({
                    "text": match &c.message.content {
                        Some(crate::proxy::mappers::openai::OpenAIContent::String(s)) => s.clone(),
                        _ => "".to_string()
                    },
                    "index": c.index,
                    "logprobs": null,
                    "finish_reason": c.finish_reason
                })
            }).collect::<Vec<_>>();

            let legacy_resp = json!({
                "id": chat_resp.id,
                "object": "text_completion",
                "created": chat_resp.created,
                "model": chat_resp.model,
                "choices": choices
            });

            return Ok(axum::Json(legacy_resp).into_response());
        }

        // Handle errors and retry
        let status_code = status.as_u16();
        let error_text = response.text().await.unwrap_or_default();
        last_error = format!("HTTP {}: {}", status_code, error_text);

        if status_code == 429 || status_code == 403 || status_code == 401 {
            continue;
        }
        return Err((status, error_text));
    }

    Err((StatusCode::TOO_MANY_REQUESTS, format!("All attempts failed. Last error: {}", last_error)))
}

pub async fn handle_list_models() -> impl IntoResponse {
    Json(json!({
        "object": "list",
        "data": [
            {"id": "gpt-4", "object": "model", "created": 1706745600, "owned_by": "openai"},
            {"id": "gpt-3.5-turbo", "object": "model", "created": 1706745600, "owned_by": "openai"},
            {"id": "o1-mini", "object": "model", "created": 1706745600, "owned_by": "openai"}
        ]
    }))
}
