// OpenAI Legacy Completions Handler
// POST /v1/completions, /v1/responses (Codex style)

use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use serde_json::{json, Value};
use tracing::{debug, info};

use crate::proxy::mappers::openai::{
    transform_openai_request, transform_openai_response, OpenAIRequest,
};
use crate::proxy::server::AppState;
use crate::proxy::session_manager::SessionManager;
use super::super::common::{apply_retry_strategy, determine_retry_strategy};

const MAX_RETRY_ATTEMPTS: usize = 3;

/// Handle Legacy Completions API (/v1/completions)
/// Converts Prompt to Chat Message format, reuses chat completions logic
pub async fn handle_completions(
    State(state): State<AppState>,
    Json(mut body): Json<Value>,
) -> Response {
    debug!(
        "Received /v1/completions or /v1/responses payload: {:?}",
        body
    );

    let is_codex_style = body.get("input").is_some() || body.get("instructions").is_some();

    // 1. Convert Payload to Messages (Shared Chat Format)
    if is_codex_style {
        let instructions = body
            .get("instructions")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
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
                        let call_id = item
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .or_else(|| item.get("id").and_then(|v| v.as_str()))
                            .unwrap_or("unknown");

                        let name = if item_type == "local_shell_call" {
                            "shell"
                        } else if item_type == "web_search_call" {
                            "google_search"
                        } else {
                            item.get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
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
                        let mut image_parts: Vec<Value> = Vec::new();

                        if let Some(parts) = content {
                            for part in parts {
                                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                    text_parts.push(text.to_string());
                                } else if part.get("type").and_then(|v| v.as_str())
                                    == Some("input_image")
                                {
                                    if let Some(image_url) =
                                        part.get("image_url").and_then(|v| v.as_str())
                                    {
                                        image_parts.push(json!({
                                            "type": "image_url",
                                            "image_url": { "url": image_url }
                                        }));
                                        debug!("[Codex] Found input_image: {}", image_url);
                                    }
                                } else if part.get("type").and_then(|v| v.as_str())
                                    == Some("image_url")
                                {
                                    if let Some(url_obj) = part.get("image_url") {
                                        image_parts.push(json!({
                                            "type": "image_url",
                                            "image_url": url_obj.clone()
                                        }));
                                    }
                                }
                            }
                        }

                        if image_parts.is_empty() {
                            messages.push(json!({
                                "role": role,
                                "content": text_parts.join("\n")
                            }));
                        } else {
                            let mut content_blocks: Vec<Value> = Vec::new();
                            if !text_parts.is_empty() {
                                content_blocks.push(json!({
                                    "type": "text",
                                    "text": text_parts.join("\n")
                                }));
                            }
                            content_blocks.extend(image_parts);
                            messages.push(json!({
                                "role": role,
                                "content": content_blocks
                            }));
                        }
                    }
                    "function_call" | "local_shell_call" | "web_search_call" => {
                        let mut name = item
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let mut args_str = item
                            .get("arguments")
                            .and_then(|v| v.as_str())
                            .unwrap_or("{}")
                            .to_string();
                        let call_id = item
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .or_else(|| item.get("id").and_then(|v| v.as_str()))
                            .unwrap_or("unknown");

                        if item_type == "local_shell_call" {
                            name = "shell";
                            if let Some(action) = item.get("action") {
                                if let Some(exec) = action.get("exec") {
                                    let mut args_obj = serde_json::Map::new();
                                    if let Some(cmd) = exec.get("command") {
                                        let cmd_val = if cmd.is_string() {
                                            json!([cmd])
                                        } else {
                                            cmd.clone()
                                        };
                                        args_obj.insert("command".to_string(), cmd_val);
                                    }
                                    if let Some(wd) =
                                        exec.get("working_directory").or(exec.get("workdir"))
                                    {
                                        args_obj.insert("workdir".to_string(), wd.clone());
                                    }
                                    args_str = serde_json::to_string(&args_obj)
                                        .unwrap_or("{}".to_string());
                                }
                            }
                        } else if item_type == "web_search_call" {
                            name = "google_search";
                            if let Some(action) = item.get("action") {
                                let mut args_obj = serde_json::Map::new();
                                if let Some(q) = action.get("query") {
                                    args_obj.insert("query".to_string(), q.clone());
                                }
                                args_str =
                                    serde_json::to_string(&args_obj).unwrap_or("{}".to_string());
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
                        let call_id = item
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let output = item.get("output");
                        let output_str = if let Some(o) = output {
                            if o.is_string() {
                                o.as_str().unwrap().to_string()
                            } else if let Some(content) = o.get("content").and_then(|v| v.as_str())
                            {
                                content.to_string()
                            } else {
                                o.to_string()
                            }
                        } else {
                            "".to_string()
                        };

                        let name = call_id_to_name.get(call_id).cloned().unwrap_or_else(|| {
                            tracing::warn!(
                                "Unknown tool name for call_id {}, defaulting to 'shell'",
                                call_id
                            );
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
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => prompt_val.to_string(),
        };
        let messages = json!([ { "role": "user", "content": prompt_str } ]);
        if let Some(obj) = body.as_object_mut() {
            obj.remove("prompt");
            obj.insert("messages".to_string(), messages);
        }
    }

    // Normalization for Codex fields
    let has_codex_fields = body.get("instructions").is_some() || body.get("input").is_some();
    let already_normalized = body
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false);

    if has_codex_fields && !already_normalized {
        tracing::debug!("[Codex] Performing simple normalization (messages not yet populated)");

        let mut messages = Vec::new();

        if let Some(inst) = body.get("instructions").and_then(|v| v.as_str()) {
            if !inst.is_empty() {
                messages.push(json!({
                    "role": "system",
                    "content": inst
                }));
            }
        }

        if let Some(input) = body.get("input") {
            if let Some(s) = input.as_str() {
                messages.push(json!({
                    "role": "user",
                    "content": s
                }));
            } else if let Some(arr) = input.as_array() {
                let is_message_array = arr
                    .first()
                    .and_then(|v| v.as_object())
                    .map(|obj| obj.contains_key("role"))
                    .unwrap_or(false);

                if is_message_array {
                    for item in arr {
                        messages.push(item.clone());
                    }
                } else {
                    let content = arr
                        .iter()
                        .map(|v| {
                            if let Some(s) = v.as_str() {
                                s.to_string()
                            } else if v.is_object() {
                                v.to_string()
                            } else {
                                "".to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    if !content.is_empty() {
                        messages.push(json!({
                            "role": "user",
                            "content": content
                        }));
                    }
                }
            } else {
                let content = input.to_string();
                if !content.is_empty() {
                    messages.push(json!({
                        "role": "user",
                        "content": content
                    }));
                }
            };
        }

        if let Some(obj) = body.as_object_mut() {
            tracing::debug!(
                "[Codex] Injecting normalized messages: {} messages",
                messages.len()
            );
            obj.insert("messages".to_string(), json!(messages));
        }
    } else if already_normalized {
        tracing::debug!(
            "[Codex] Skipping normalization (messages already populated by first pass)"
        );
    }

    let mut openai_req: OpenAIRequest = match serde_json::from_value(body.clone()) {
        Ok(req) => req,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("Invalid request: {}", e)).into_response();
        }
    };

    // Safety: Inject empty message if needed
    if openai_req.messages.is_empty() {
        openai_req
            .messages
            .push(crate::proxy::mappers::openai::OpenAIMessage {
                role: "user".to_string(),
                content: Some(crate::proxy::mappers::openai::OpenAIContent::String(
                    " ".to_string(),
                )),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            });
    }

    let upstream = state.upstream.clone();
    let token_manager = state.token_manager;
    let pool_size = token_manager.len();
    let max_attempts = MAX_RETRY_ATTEMPTS.min(pool_size.saturating_add(1)).max(2);

    let mut last_error = String::new();
    let mut last_email: Option<String> = None;

    let mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
        &openai_req.model,
        &*state.custom_mapping.read().await,
    );
    let trace_id = format!("req_{}", chrono::Utc::now().timestamp_subsec_millis());

    for attempt in 0..max_attempts {
        let tools_val: Option<Vec<Value>> = openai_req
            .tools
            .as_ref()
            .map(|list| list.iter().cloned().collect());
        let config = crate::proxy::mappers::common_utils::resolve_request_config(
            &openai_req.model,
            &mapped_model,
            &tools_val,
            None,
            None,
        );

        let session_id_str = SessionManager::extract_openai_session_id(&openai_req);
        let session_id = Some(session_id_str.as_str());

        let force_rotate = attempt > 0;

        let token_lease = match token_manager
            .get_token(
                &config.request_type,
                force_rotate,
                session_id,
                &mapped_model,
            )
            .await
        {
            Ok(t) => t,
            Err(e) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    [("X-Mapped-Model", mapped_model)],
                    format!("Token error: {}", e),
                )
                    .into_response()
            }
        };

        let access_token = token_lease.access_token.clone();
        let project_id = token_lease.project_id.clone();
        let email = token_lease.email.clone();

        last_email = Some(email.clone());

        info!("✓ Using account: {} (type: {})", email, config.request_type);

        let gemini_body = transform_openai_request(&openai_req, &project_id, &mapped_model);

        debug!(
            "[Codex-Request] Transformed Gemini Body ({} parts)",
            gemini_body
                .get("contents")
                .and_then(|c| c.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        );

        let client_wants_stream = openai_req.stream;
        let force_stream_internally = !client_wants_stream;
        let list_response = client_wants_stream || force_stream_internally;
        let method = if list_response {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        let query_string = if list_response { Some("alt=sse") } else { None };

        let response = match upstream
            .call_v1_internal(method, &access_token, gemini_body, query_string)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_error = e.clone();
                debug!(
                    "Codex Request failed on attempt {}/{}: {}",
                    attempt + 1,
                    max_attempts,
                    e
                );
                continue;
            }
        };

        let status = response.status();
        if status.is_success() {
            token_manager.mark_account_success(&email, Some(&mapped_model));

            if list_response {
                use axum::body::Body;
                use axum::response::Response;
                use futures::StreamExt;

                let gemini_stream = response.bytes_stream();

                if client_wants_stream {
                    let mut openai_stream = if is_codex_style {
                        use crate::proxy::mappers::openai::streaming::create_codex_sse_stream;
                        create_codex_sse_stream(
                            Box::pin(gemini_stream),
                            openai_req.model.clone(),
                            session_id_str.clone(),
                            openai_req.messages.len(),
                        )
                    } else {
                        use crate::proxy::mappers::openai::streaming::create_legacy_sse_stream;
                        create_legacy_sse_stream(
                            Box::pin(gemini_stream),
                            openai_req.model.clone(),
                            session_id_str.clone(),
                            openai_req.messages.len(),
                        )
                    };

                    let mut first_data_chunk = None;
                    let mut retry_this_account = false;

                    loop {
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(60),
                            openai_stream.next(),
                        )
                        .await
                        {
                            Ok(Some(Ok(bytes))) => {
                                if bytes.is_empty() {
                                    continue;
                                }
                                let text = String::from_utf8_lossy(&bytes);
                                if text.trim().starts_with(":")
                                    || text.trim().starts_with("data: :")
                                {
                                    continue;
                                }
                                if text.contains("\"error\"") {
                                    last_error = "Error event during peek".to_string();
                                    retry_this_account = true;
                                    break;
                                }
                                first_data_chunk = Some(bytes);
                                break;
                            }
                            Ok(Some(Err(e))) => {
                                last_error = format!("Stream error during peek: {}", e);
                                retry_this_account = true;
                                break;
                            }
                            Ok(None) => {
                                last_error = "Empty response stream".to_string();
                                retry_this_account = true;
                                break;
                            }
                            Err(_) => {
                                last_error = "Timeout waiting for first data".to_string();
                                retry_this_account = true;
                                break;
                            }
                        }
                    }

                    if retry_this_account {
                        continue;
                    }

                    let combined_stream = futures::stream::once(async move {
                        Ok::<Bytes, String>(first_data_chunk.unwrap())
                    })
                    .chain(openai_stream);

                    return Response::builder()
                        .header("Content-Type", "text/event-stream")
                        .header("Cache-Control", "no-cache")
                        .header("Connection", "keep-alive")
                        .header("X-Account-Email", &email)
                        .header("X-Mapped-Model", &mapped_model)
                        .body(Body::from_stream(combined_stream))
                        .unwrap()
                        .into_response();
                } else {
                    use crate::proxy::mappers::openai::streaming::create_openai_sse_stream;
                    let mut openai_stream =
                        create_openai_sse_stream(
                            Box::pin(gemini_stream),
                            openai_req.model.clone(),
                            session_id_str.clone(),
                            openai_req.messages.len(),
                        );

                    let mut first_data_chunk = None;
                    let mut retry_this_account = false;
                    loop {
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(60),
                            openai_stream.next(),
                        )
                        .await
                        {
                            Ok(Some(Ok(bytes))) => {
                                if bytes.is_empty() {
                                    continue;
                                }
                                let text = String::from_utf8_lossy(&bytes);
                                if text.trim().starts_with(":")
                                    || text.trim().starts_with("data: :")
                                {
                                    continue;
                                }
                                if text.contains("\"error\"") {
                                    last_error = "Error event in internal stream".to_string();
                                    retry_this_account = true;
                                    break;
                                }
                                first_data_chunk = Some(bytes);
                                break;
                            }
                            Ok(Some(Err(e))) => {
                                last_error = format!("Internal stream error: {}", e);
                                retry_this_account = true;
                                break;
                            }
                            Ok(None) => {
                                last_error = "Empty internal stream".to_string();
                                retry_this_account = true;
                                break;
                            }
                            Err(_) => {
                                last_error = "Timeout peek internal".to_string();
                                retry_this_account = true;
                                break;
                            }
                        }
                    }
                    if retry_this_account {
                        continue;
                    }

                    let combined_stream = futures::stream::once(async move {
                        Ok::<Bytes, String>(first_data_chunk.unwrap())
                    })
                    .chain(openai_stream);

                    use crate::proxy::mappers::openai::collector::collect_stream_to_json;
                    match collect_stream_to_json(Box::pin(combined_stream)).await {
                        Ok(chat_resp) => {
                            crate::proxy::SignatureCache::global()
                                .delete_session_signature(&session_id_str);
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
                                "choices": choices,
                                "usage": chat_resp.usage
                            });

                            return (
                                StatusCode::OK,
                                [
                                    ("X-Account-Email", email.as_str()),
                                    ("X-Mapped-Model", mapped_model.as_str()),
                                ],
                                Json(legacy_resp),
                            )
                                .into_response();
                        }
                        Err(e) => {
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Stream collection error: {}", e),
                            )
                                .into_response();
                        }
                    }
                }
            }

            let gemini_resp: Value = match response.json().await {
                Ok(json) => json,
                Err(e) => {
                    return (
                        StatusCode::BAD_GATEWAY,
                        [("X-Mapped-Model", mapped_model.as_str())],
                        format!("Parse error: {}", e),
                    )
                        .into_response();
                }
            };

            let chat_resp = transform_openai_response(&gemini_resp);

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
                "choices": choices,
                "usage": chat_resp.usage
            });

            return (
                StatusCode::OK,
                [
                    ("X-Account-Email", email.as_str()),
                    ("X-Mapped-Model", mapped_model.as_str()),
                ],
                Json(legacy_resp),
            )
                .into_response();
        }

        // Handle errors and retry
        let status_code = status.as_u16();
        let retry_after = response
            .headers()
            .get("Retry-After")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| format!("HTTP {}", status_code));
        last_error = format!("HTTP {}: {}", status_code, error_text);

        tracing::error!(
            "[Codex-Upstream] Error Response {}: {}",
            status_code,
            error_text
        );

        if status_code == 429 || status_code == 529 || status_code == 503 || status_code == 500 {
            token_manager
                .mark_rate_limited_async(
                    &email,
                    status_code,
                    retry_after.as_deref(),
                    &error_text,
                    Some(&mapped_model),
                )
                .await;
        }

        let strategy = determine_retry_strategy(status_code, &error_text, false);

        if apply_retry_strategy(strategy, attempt, max_attempts, status_code, &trace_id).await {
            continue;
        } else {
            return (
                status,
                [
                    ("X-Account-Email", email.as_str()),
                    ("X-Mapped-Model", mapped_model.as_str()),
                ],
                error_text,
            )
                .into_response();
        }
    }

    // All attempts failed
    if let Some(email) = last_email {
        (
            StatusCode::TOO_MANY_REQUESTS,
            [("X-Account-Email", email), ("X-Mapped-Model", mapped_model)],
            format!("All accounts exhausted. Last error: {}", last_error),
        )
            .into_response()
    } else {
        (
            StatusCode::TOO_MANY_REQUESTS,
            [("X-Mapped-Model", mapped_model)],
            format!("All accounts exhausted. Last error: {}", last_error),
        )
            .into_response()
    }
}
