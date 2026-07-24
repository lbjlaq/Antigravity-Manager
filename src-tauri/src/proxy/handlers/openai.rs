// OpenAI Handler
use axum::{
    extract::Json, extract::State, http::StatusCode, response::IntoResponse, response::Response,
};
use base64::Engine as _;
use bytes::Bytes;
use serde_json::{json, Value};
use tracing::{debug, error, info}; // Import Engine trait for encode method

use crate::proxy::mappers::openai::{
    transform_openai_request, transform_openai_response, OpenAIMessage, OpenAIRequest,
};
// use crate::proxy::upstream::client::UpstreamClient; // 通过 state 获取
use crate::proxy::debug_logger;
use crate::proxy::server::AppState;
use crate::proxy::upstream::client::mask_email;

const MAX_RETRY_ATTEMPTS: usize = 3;
const CODEX_VISIBLE_THOUGHT_MESSAGE_PREFIX: &str = "msg_thought_";
use super::common::{
    apply_retry_strategy, determine_retry_strategy, should_rotate_account, RetryStrategy,
};
use crate::modules::account;
use crate::proxy::common::client_adapter::CLIENT_ADAPTERS; // [NEW] Adapter Registry
use crate::proxy::session_manager::SessionManager;
use axum::http::HeaderMap;
use std::collections::VecDeque;
use tokio::time::Duration;

/// Return true only when a streamed chunk contains an actual error event.
///
/// Responses lifecycle envelopes legitimately contain `"error": null`, and
/// assistant text may also mention the word "error". A substring search would
/// misclassify both as failures and rotate otherwise healthy accounts.
fn stream_chunk_has_error_event(bytes: &[u8]) -> bool {
    String::from_utf8_lossy(bytes).lines().any(|line| {
        let Some(data) = line.trim_start().strip_prefix("data:") else {
            return false;
        };
        let Ok(payload) = serde_json::from_str::<Value>(data.trim()) else {
            return false;
        };

        matches!(
            payload.get("type").and_then(Value::as_str),
            Some("error" | "response.failed")
        ) || payload.get("error").is_some_and(|error| !error.is_null())
    })
}

/// Visible Codex commentary is part of the local transcript, not Gemini
/// conversation history. Codex omits output item IDs when it replays a task, so
/// `phase=commentary` is the durable discriminator. The text-prefix fallback
/// heals tasks written by builds that accidentally finalized a thought blob as
/// a normal answer.
fn is_codex_transcript_only_assistant_message(item: &Value, text: &str) -> bool {
    if item.get("type").and_then(Value::as_str) != Some("message")
        || item.get("role").and_then(Value::as_str) != Some("assistant")
    {
        return false;
    }

    item.get("phase").and_then(Value::as_str) == Some("commentary")
        || item
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id.starts_with(CODEX_VISIBLE_THOUGHT_MESSAGE_PREFIX))
        || text.trim_start().starts_with("**Thinking**")
}

#[cfg(test)]
mod stream_peek_tests {
    use super::is_codex_transcript_only_assistant_message;
    use super::stream_chunk_has_error_event;
    use serde_json::json;

    #[test]
    fn responses_created_with_null_error_is_not_an_error_event() {
        let chunk = br#"event: response.created
data: {"type":"response.created","response":{"status":"in_progress","error":null}}

"#;
        assert!(!stream_chunk_has_error_event(chunk));
    }

    #[test]
    fn response_failed_is_an_error_event() {
        let chunk = br#"event: response.failed
data: {"type":"response.failed","response":{"status":"failed","error":{"code":"upstream_error"}}}

"#;
        assert!(stream_chunk_has_error_event(chunk));
    }

    #[test]
    fn legacy_top_level_error_is_an_error_event() {
        let chunk = br#"data: {"error":{"message":"quota exceeded"}}

"#;
        assert!(stream_chunk_has_error_event(chunk));
    }

    #[test]
    fn normal_text_containing_error_is_not_an_error_event() {
        let chunk = br#"data: {"type":"response.output_text.delta","delta":"The JSON key is called \"error\"."}

"#;
        assert!(!stream_chunk_has_error_event(chunk));
    }

    #[test]
    fn identifies_codex_transcript_only_assistant_messages() {
        let thought = json!({
            "type": "message",
            "id": "msg_thought_abc_0",
            "role": "assistant",
            "phase": "commentary",
            "content": [{"type": "output_text", "text": "thinking"}],
        });
        let normal_commentary = json!({
            "type": "message",
            "role": "assistant",
            "phase": "commentary",
            "content": [{"type": "output_text", "text": "progress"}],
        });
        let contaminated_final = json!({
            "type": "message",
            "role": "assistant",
            "phase": "final_answer",
            "content": [{"type": "output_text", "text": "**Thinking**\n\nlegacy thought"}],
        });
        let clean_final = json!({
            "type": "message",
            "role": "assistant",
            "phase": "final_answer",
            "content": [{"type": "output_text", "text": "done"}],
        });

        assert!(is_codex_transcript_only_assistant_message(
            &thought, "thinking"
        ));
        assert!(is_codex_transcript_only_assistant_message(
            &normal_commentary,
            "progress"
        ));
        assert!(is_codex_transcript_only_assistant_message(
            &contaminated_final,
            "**Thinking**\n\nlegacy thought"
        ));
        assert!(!is_codex_transcript_only_assistant_message(
            &clean_final,
            "done"
        ));
    }
}

#[cfg(test)]
mod variant_tests {
    use crate::proxy::common::variant_mapping;
    use crate::proxy::mappers::openai::models::ThinkingConfig;

    #[test]
    fn openai_opus_preserves_client_budget_when_present() {
        let client_budget = Some(32_768);
        let spec = variant_mapping::resolve("claude-opus-4-6-thinking", client_budget)
            .expect("Claude Opus 4.6 thinking must resolve");
        let request_thinking = ThinkingConfig {
            thinking_type: Some("enabled".to_string()),
            budget_tokens: Some(spec.effective_thinking_budget(client_budget)),
            effort: None,
        };

        assert_eq!(request_thinking.budget_tokens, client_budget);
    }

    #[test]
    fn openai_opus_falls_back_to_spec_budget_when_client_budget_is_absent() {
        let client_budget = None;
        let spec = variant_mapping::resolve("claude-opus-4-6-thinking", client_budget)
            .expect("Claude Opus 4.6 thinking must resolve");
        let request_thinking = ThinkingConfig {
            thinking_type: Some("enabled".to_string()),
            budget_tokens: Some(spec.effective_thinking_budget(client_budget)),
            effort: None,
        };

        assert_eq!(request_thinking.budget_tokens, Some(1_024));
    }
}

fn compact_apply_patch_failure_output(
    output: String,
    seen: &mut std::collections::HashSet<String>,
    distinct_count: &mut usize,
) -> String {
    if !output.contains("apply_patch verification failed")
        && !output.contains("Failed to find expected lines")
        && !output.contains("Failed to find context")
        && !output.contains("Expected update hunk")
    {
        return output;
    }

    let fingerprint = output.lines().take(8).collect::<Vec<_>>().join("\n");
    if !seen.insert(fingerprint) {
        return "[Repeated apply_patch failure omitted: the same error was already provided earlier in this request.]".to_string();
    }

    *distinct_count += 1;
    if *distinct_count > 6 {
        return "[Additional apply_patch failure omitted to avoid a retry loop. Produce a fresh V4A patch from current file contents instead of repeating previous failed patches.]".to_string();
    }

    output
}

fn codex_ledger_from_body(
    body: &Value,
) -> (
    Option<crate::proxy::mappers::openai::interaction_ledger::InteractionLedger>,
    VecDeque<String>,
) {
    let ledger = crate::proxy::mappers::openai::interaction_ledger::build_codex_interaction_ledger(
        body.get("input"),
        body.get("instructions").and_then(|v| v.as_str()),
        body.get("session_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        body.get("previous_response_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    );

    let mut markers = VecDeque::new();
    if let Some(ledger) = &ledger {
        for step in ledger.turns.iter().flat_map(|turn| turn.steps.iter()) {
            if step.raw_item.get("type").and_then(|v| v.as_str()) != Some("instructions") {
                markers.push_back(
                    crate::proxy::mappers::openai::interaction_ledger::step_marker(step),
                );
            }
        }
    }

    (ledger, markers)
}

fn strip_codex_step_markers(content: &str) -> String {
    let mut cleaned = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[codex-turn:")
            && trimmed.contains(" step:")
            && trimmed.contains(" type:")
            && trimmed.ends_with(']')
        {
            continue;
        }
        cleaned.push(line);
    }
    cleaned.join("\n").trim().to_string()
}

fn prefix_with_step_marker(_marker: Option<String>, content: String) -> String {
    // Step markers are useful in debug ledgers, but must not be visible to
    // Gemini. If they enter the prompt, the model learns to emit them as text.
    strip_codex_step_markers(&content)
}

pub async fn handle_chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap, // [CHANGED] Extract headers
    Json(mut body): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // [NEW] Check for Image Model Redirection
    let model_name = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    // [FIX] Only redirect non-native image aliases (dall-e / midjourney) to the
    // images-generations shim. Native Gemini image models (gemini-3-pro-image*) must
    // flow through the normal pipeline (transform_openai_request -> resolve_request_config),
    // which correctly sets requestType=image_gen, imageConfig (size/aspect ratio), sessionId,
    // structured requestId and per-account dynamic model resolution — matching the official
    // Antigravity client. The old shim dropped `size` and built a divergent upstream body,
    // which caused image generation to silently fail for gemini-3-pro-image.
    if (model_name.contains("image")
        || model_name.contains("dall-e")
        || model_name.contains("midjourney"))
        && !model_name.contains("gemini")
    {
        tracing::info!(
            "[ChatRedirection] Redirecting model {} to image generations",
            model_name
        );
        return intercept_chat_to_image(state, body, &model_name).await;
    }

    // [FIX] 保存原始请求体的完整副本，用于日志记录
    // 这确保了即使结构体定义遗漏字段，日志也能完整记录所有参数
    let original_body = body.clone();

    // [NEW] 自动检测并转换 Responses 格式
    // 如果请求包含 instructions 或 input 但没有 messages，则认为是 Responses 格式
    let is_responses_format = !body.get("messages").is_some()
        && (body.get("instructions").is_some() || body.get("input").is_some());

    if is_responses_format {
        debug!("Detected Responses API format, converting to Chat Completions format");

        // 转换 instructions 为 system message
        if let Some(instructions) = body.get("instructions").and_then(|v| v.as_str()) {
            if !instructions.is_empty() {
                let system_msg = json!({
                    "role": "system",
                    "content": instructions
                });

                // 初始化 messages 数组
                if !body.get("messages").is_some() {
                    body["messages"] = json!([]);
                }

                // 将 system message 插入到开头
                if let Some(messages) = body.get_mut("messages").and_then(|v| v.as_array_mut()) {
                    messages.insert(0, system_msg);
                }
            }
        }

        // 转换 input 为 user message（如果存在）
        if let Some(input) = body.get("input") {
            let user_msg = if input.is_string() {
                json!({
                    "role": "user",
                    "content": input.as_str().unwrap_or("")
                })
            } else {
                // input 是数组格式，暂时简化处理
                json!({
                    "role": "user",
                    "content": input.to_string()
                })
            };

            if let Some(messages) = body.get_mut("messages").and_then(|v| v.as_array_mut()) {
                messages.push(user_msg);
            }
        }

        if let Some(obj) = body.as_object_mut() {
            obj.remove("instructions");
        }
    }

    let normalized_interaction_ledger = body.get("_interaction_ledger").cloned();
    let mut openai_req: OpenAIRequest = serde_json::from_value(body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid request: {}", e)))?;

    // Safety: Ensure messages is not empty
    if openai_req.messages.is_empty() {
        debug!("Received request with empty messages, injecting fallback...");
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
                refusal: None,
            });
    }

    let trace_id = format!("req_{}", chrono::Utc::now().timestamp_subsec_millis());
    info!(
        "[{}] OpenAI Chat Request: {} | {} messages | stream: {}",
        trace_id,
        openai_req.model,
        openai_req.messages.len(),
        openai_req.stream
    );
    let debug_cfg = state.debug_logging.read().await.clone();

    let mut force_rotate = false;

    if debug_logger::is_enabled(&debug_cfg) {
        if let Some(ledger) = normalized_interaction_ledger {
            let payload = json!({
                "kind": "normalized_interaction_ledger",
                "protocol": "openai",
                "trace_id": trace_id.clone(),
                "original_model": openai_req.model.clone(),
                "interaction_ledger": ledger,
            });
            debug_logger::write_exchange_payload(
                &debug_cfg,
                Some(&trace_id),
                "normalized_interaction_ledger",
                &payload,
            )
            .await;
        }

        // [FIX] 使用原始 body 副本记录日志，确保不丢失任何字段
        let original_payload = json!({
            "kind": "original_request",
            "protocol": "openai",
            "trace_id": trace_id,
            "original_model": openai_req.model,
            "request": original_body,  // 使用原始请求体，不是结构体序列化
        });
        debug_logger::write_exchange_payload(
            &debug_cfg,
            Some(&trace_id),
            "original_request",
            &original_payload,
        )
        .await;
    }

    // [NEW] Detect Client Adapter
    let client_adapter = CLIENT_ADAPTERS
        .iter()
        .find(|a| a.matches(&headers))
        .cloned();
    if client_adapter.is_some() {
        debug!("[{}] Client Adapter detected", trace_id);
    }

    // [Variant] Resolve canonical model + variant → real model + real params.
    // Replace the client's model/thinking/max_tokens with verified real values so the
    // forwarded request matches the expected upstream format. OpenCode encodes the variant as
    // thinking.budget_tokens; we infer the tier from its magnitude.
    let client_budget = openai_req.thinking.as_ref().and_then(|t| t.budget_tokens);
    if let Some(spec) =
        crate::proxy::common::variant_mapping::resolve(&openai_req.model, client_budget)
    {
        tracing::info!(
            "[{}] [Variant] canonical='{}' budget_hint={:?} -> real_model='{}' budget={} maxOut={}",
            trace_id,
            openai_req.model,
            client_budget,
            spec.id,
            spec.thinking_budget,
            spec.max_output_tokens
        );
        openai_req.model = spec.id.to_string();
        if spec.thinking_budget == 0 {
            // Non-thinking checkpoint model (e.g. gemini-3.1-flash-lite): disable thinking
            // AND strip tools/tool_choice — per upstream spec §3 checkpoint requests carry
            // no tools.
            openai_req.thinking = None;
            openai_req.tools = None;
            openai_req.tool_choice = None;
        } else {
            openai_req.thinking = Some(crate::proxy::mappers::openai::models::ThinkingConfig {
                thinking_type: Some("enabled".to_string()),
                budget_tokens: Some(spec.effective_thinking_budget(client_budget)),
                effort: None,
            });
        }
        openai_req.max_tokens = Some(spec.max_output_tokens);
    }

    let client_tool_names =
        crate::proxy::mappers::openai::request::extract_client_tool_names(&openai_req.tools);

    // 1. 获取 UpstreamClient (Clone handle)
    let upstream = state.upstream.clone();
    let token_manager = state.token_manager;
    let pool_size = token_manager.len();
    // [FIX] Ensure max_attempts is at least 2 to allow for internal retries
    let max_attempts = MAX_RETRY_ATTEMPTS.min(pool_size.saturating_add(1)).max(2);

    let mut last_error = String::new();
    let mut last_email: Option<String> = None;

    // 2. 模型路由解析 (移到循环外以支持在所有路径返回 X-Mapped-Model)
    let mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
        &openai_req.model,
        &*state.custom_mapping.read().await,
    );

    for attempt in 0..max_attempts {
        // 将 OpenAI 工具转为 Value 数组以便探测联网
        let tools_val: Option<Vec<Value>> = openai_req
            .tools
            .as_ref()
            .map(|list| list.iter().cloned().collect());
        let config = crate::proxy::mappers::common_utils::resolve_request_config(
            &openai_req.model,
            &mapped_model,
            &tools_val,
            None, // size (not used in handler, transform_openai_request handles it)
            None, // quality
            None, // image_size
            None, // body
        );

        // 3. 提取 SessionId (粘性指纹)
        let session_id = SessionManager::extract_openai_session_id(&openai_req);

        // 4. 获取 Token (使用准确的 request_type)
        // 关键：在重试尝试时根据 force_rotate 决定是否轮换账号
        let (access_token, project_id, email, account_id, _wait_ms) = match token_manager
            .get_token(
                &config.request_type,
                force_rotate,
                Some(&session_id),
                &mapped_model,
            )
            .await
        {
            Ok(t) => t,
            Err(e) => {
                // [FIX] Attach headers to error response for logging visibility
                let headers = [("X-Mapped-Model", mapped_model.as_str())];
                return Ok((
                    StatusCode::SERVICE_UNAVAILABLE,
                    headers,
                    format!("Token error: {}", e),
                )
                    .into_response());
            }
        };

        // [NEW v4.1.29] 获取完整 Token 对象用于动态规格查询
        let proxy_token = token_manager.get_token_by_id(&account_id);
        let mapped_model = token_manager
            .resolve_dynamic_model_for_account(&account_id, &mapped_model)
            .await;

        last_email = Some(email.clone());
        info!("✓ Using account: {} (type: {})", email, config.request_type);

        // 4. 转换请求 (返回内容包含 session_id, message_count, prefix_hash)
        let (gemini_body, session_id, message_count, _prefix_hash) = transform_openai_request(
            &openai_req,
            &project_id,
            &mapped_model,
            proxy_token.as_ref(),
        );
        let gemini_body_for_debug = gemini_body.clone();

        if debug_logger::is_enabled(&debug_cfg) {
            let payload = json!({
                "kind": "v1internal_request",
                "protocol": "openai",
                "trace_id": trace_id,
                "original_model": openai_req.model,
                "mapped_model": mapped_model,
                "request_type": config.request_type,
                "attempt": attempt,
                "v1internal_request": gemini_body.clone(),
            });
            debug_logger::write_exchange_payload(
                &debug_cfg,
                Some(&trace_id),
                "v1internal_request",
                &payload,
            )
            .await;
        }

        // [New] 打印转换后的报文 (Gemini Body) 供调试
        if let Ok(body_json) = serde_json::to_string_pretty(&gemini_body) {
            debug!("[OpenAI-Request] Transformed Gemini Body:\n{}", body_json);
        }

        // 5. 发送请求
        let client_wants_stream = openai_req.stream;
        let force_stream_internally = !client_wants_stream;
        let actual_stream = client_wants_stream || force_stream_internally;

        if force_stream_internally {
            debug!(
                "[{}] 🔄 Auto-converting non-stream request to stream for better quota",
                trace_id
            );
        }

        let method = if actual_stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        let query_string = if actual_stream { Some("alt=sse") } else { None };

        // [FIX #1522] Inject Anthropic Beta Headers for Claude models (OpenAI path)
        let mut extra_headers = std::collections::HashMap::new();
        if mapped_model.to_lowercase().contains("claude") {
            extra_headers.insert(
                "anthropic-beta".to_string(),
                "claude-code-20250219".to_string(),
            );
            tracing::debug!(
                "[{}] Injected Anthropic beta headers for Claude model (via OpenAI)",
                trace_id
            );
        }

        let call_result = match upstream
            .call_v1_internal_with_headers(
                method,
                &access_token,
                gemini_body,
                query_string,
                extra_headers.clone(),
                Some(account_id.as_str()),
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_error = e.clone();
                debug!(
                    "OpenAI Request failed on attempt {}/{}: {}",
                    attempt + 1,
                    max_attempts,
                    e
                );
                continue;
            }
        };

        // [NEW] 记录端点降级日志到 debug 文件
        if !call_result.fallback_attempts.is_empty() && debug_logger::is_enabled(&debug_cfg) {
            let fallback_entries: Vec<Value> = call_result
                .fallback_attempts
                .iter()
                .map(|a| {
                    json!({
                        "endpoint_url": a.endpoint_url,
                        "status": a.status,
                        "error": a.error,
                    })
                })
                .collect();
            let payload = json!({
                "kind": "endpoint_fallback",
                "protocol": "openai",
                "trace_id": trace_id,
                "original_model": openai_req.model,
                "mapped_model": mapped_model,
                "attempt": attempt,
                "account": mask_email(&email),
                "fallback_attempts": fallback_entries,
            });
            debug_logger::write_debug_payload(
                &debug_cfg,
                Some(&trace_id),
                "endpoint_fallback",
                &payload,
            )
            .await;
        }

        let response = call_result.response;
        // [NEW] 提取实际请求的上游端点 URL，用于日志记录和排查
        let upstream_url = response.url().to_string();
        let status = response.status();
        if status.is_success() {
            // 5. 处理流式 vs 非流式
            if actual_stream {
                use axum::body::Body;
                use axum::response::Response;
                use futures::StreamExt;

                let meta = json!({
                    "protocol": "openai",
                    "trace_id": trace_id,
                    "original_model": openai_req.model,
                    "mapped_model": mapped_model,
                    "request_type": config.request_type,
                    "attempt": attempt,
                    "status": status.as_u16(),
                    "upstream_url": upstream_url,
                });
                let gemini_stream = debug_logger::wrap_stream_with_debug(
                    Box::pin(response.bytes_stream()),
                    debug_cfg.clone(),
                    trace_id.clone(),
                    "upstream_response",
                    meta,
                );

                // [P1 FIX] Enhanced Peek logic to handle heartbeats and slow start
                // Pre-read until we find meaningful content, skip heartbeats
                use crate::proxy::mappers::openai::streaming::create_openai_sse_stream;
                let mut openai_stream = create_openai_sse_stream(
                    gemini_stream,
                    openai_req.model.clone(),
                    session_id,
                    message_count,
                    Some(client_tool_names.clone()),
                );

                let mut first_data_chunk = None;
                let mut retry_this_account = false;

                // Loop to skip heartbeats during peek
                loop {
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(300),
                        openai_stream.next(),
                    )
                    .await
                    {
                        Ok(Some(Ok(bytes))) => {
                            if bytes.is_empty() {
                                continue;
                            }

                            let text = String::from_utf8_lossy(&bytes);
                            // Skip SSE comments/pings (heartbeats)
                            if text.trim().starts_with(":") || text.trim().starts_with("data: :") {
                                tracing::debug!("[OpenAI] Skipping peek heartbeat");
                                continue;
                            }

                            // Check for error events
                            if stream_chunk_has_error_event(&bytes) {
                                tracing::warn!("[OpenAI] Error detected during peek, retrying...");
                                last_error = "Error event during peek".to_string();
                                retry_this_account = true;
                                break;
                            }

                            // We found real data!
                            first_data_chunk = Some(bytes);
                            break;
                        }
                        Ok(Some(Err(e))) => {
                            tracing::warn!("[OpenAI] Stream error during peek: {}, retrying...", e);
                            last_error = format!("Stream error during peek: {}", e);
                            retry_this_account = true;
                            break;
                        }
                        Ok(None) => {
                            tracing::warn!(
                                "[OpenAI] Stream ended during peek (Empty Response), retrying..."
                            );
                            last_error = "Empty response stream during peek".to_string();
                            retry_this_account = true;
                            break;
                        }
                        Err(_) => {
                            tracing::warn!("[OpenAI] First chunk timeout after 300s, retrying...");
                            last_error = "First chunk timeout".to_string();
                            retry_this_account = true;
                            break;
                        }
                    }
                }

                if retry_this_account {
                    continue; // Rotate to next account
                }

                // Combine first chunk with remaining stream
                let combined_stream =
                    futures::stream::once(
                        async move { Ok::<Bytes, String>(first_data_chunk.unwrap()) },
                    )
                    .chain(openai_stream);

                // [NEW] 针对 OpenAI 流增加 300 秒空闲超时保护
                let combined_stream = async_stream::stream! {
                    let mut s = Box::pin(combined_stream);

                    loop {
                        match tokio::time::timeout(std::time::Duration::from_secs(300), s.next()).await {
                            Ok(Some(item)) => yield item,
                            Ok(None) => break,
                            Err(_) => {
                                tracing::error!("[OpenAI-SSE] Idle timeout after 300s, terminating stream");
                                yield Ok::<Bytes, String>(Bytes::from("data: [DONE]\n\n"));
                                break;
                            }
                        }
                    }
                };
                let converted_meta = json!({
                    "protocol": "openai",
                    "trace_id": trace_id,
                    "stage": "converted_codex_response",
                    "original_model": openai_req.model,
                    "mapped_model": mapped_model,
                    "request_type": config.request_type,
                    "attempt": attempt,
                    "status": status.as_u16(),
                    "upstream_url": upstream_url,
                });
                let combined_stream = debug_logger::wrap_stream_with_debug(
                    Box::pin(combined_stream),
                    debug_cfg.clone(),
                    trace_id.clone(),
                    "converted_codex_response",
                    converted_meta,
                );

                if client_wants_stream {
                    // [MULTI-TURN] 保存本次对话的 messages 到 session store（/v1/chat/completions）
                    {
                        let save_msgs = openai_req
                            .messages
                            .iter()
                            .map(|m| {
                                let content_str = match &m.content {
                                    Some(crate::proxy::mappers::openai::OpenAIContent::String(
                                        s,
                                    )) => s.clone(),
                                    _ => String::new(),
                                };
                                json!({"role": m.role, "content": content_str})
                            })
                            .collect::<Vec<_>>();
                        let chat_response_id =
                            format!("chatcmpl-{}", uuid::Uuid::new_v4().simple());
                        let entry = crate::proxy::http_session_store::HttpSessionEntry {
                            input_items: save_msgs,
                            instructions: String::new(),
                            model: openai_req.model.clone(),
                            last_accessed: std::time::Instant::now(),
                        };
                        let rid = chat_response_id.clone();
                        tokio::spawn(async move {
                            crate::proxy::http_session_store::save_session(rid, entry).await;
                        });
                    }
                    // 客户端请求流式，返回 SSE
                    let body = Body::from_stream(combined_stream);
                    return Ok(Response::builder()
                        .header("Content-Type", "text/event-stream")
                        .header("Cache-Control", "no-cache")
                        .header("Connection", "keep-alive")
                        .header("X-Accel-Buffering", "no")
                        .header("X-Account-Email", &email)
                        .header("X-Mapped-Model", &mapped_model)
                        .body(body)
                        .unwrap()
                        .into_response());
                } else {
                    // 客户端请求非流式，但内部强制转为流式
                    // 收集流数据并聚合为 JSON
                    use crate::proxy::mappers::openai::collector::collect_stream_to_json;

                    match collect_stream_to_json(combined_stream).await {
                        Ok(full_response) => {
                            info!("[{}] ✓ Stream collected and converted to JSON", trace_id);
                            if debug_logger::is_enabled(&debug_cfg) {
                                let converted_response = serde_json::to_value(&full_response)
                                    .unwrap_or_else(
                                        |e| json!({ "serialization_error": e.to_string() }),
                                    );
                                let payload = json!({
                                    "kind": "exchange_summary",
                                    "protocol": "openai",
                                    "trace_id": trace_id,
                                    "original_codex_request": original_body,
                                    "gemini_request": gemini_body_for_debug,
                                    "converted_codex_response": converted_response,
                                    "gemini_raw_response_ref": "see upstream_response file with the same trace_id",
                                });
                                debug_logger::write_exchange_payload(
                                    &debug_cfg,
                                    Some(&trace_id),
                                    "exchange_summary",
                                    &payload,
                                )
                                .await;
                            }
                            return Ok((
                                StatusCode::OK,
                                [
                                    ("X-Account-Email", email.as_str()),
                                    ("X-Mapped-Model", mapped_model.as_str()),
                                ],
                                Json(full_response),
                            )
                                .into_response());
                        }
                        Err(e) => {
                            error!("[{}] Stream collection error: {}", trace_id, e);
                            return Ok((
                                StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Stream collection error: {}", e),
                            )
                                .into_response());
                        }
                    }
                }
            }

            let gemini_resp: Value = response
                .json()
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Parse error: {}", e)))?;

            // [CACHE] 从 Gemini 响应中提取缓存信息，关闭反馈循环
            // 兼容两种格式: cachedContentTokenCount (旧), total_cached_tokens (新)
            if let Some(usage) = gemini_resp.get("usageMetadata") {
                let cached = usage
                    .get("total_cached_tokens")
                    .or_else(|| usage.get("cachedContentTokenCount"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if cached > 0 {
                    let cm = crate::proxy::cache_manager::global_cache_manager();
                    cm.record_implicit_hit(&_prefix_hash);
                    // [CACHE] 分层统计日志
                    let stats = cm.get_layer_stats();
                    tracing::info!(
                        "[Cache-Opt] Implicit cache HIT: prefix_hash={} cached_tokens={} | L1(SI): {}/{}, L2(Tools): {}/{}, L3(Prefix): {}/{}",
                        &_prefix_hash[.._prefix_hash.len().min(16)],
                        cached,
                        stats.si_hits, stats.si_total,
                        stats.tools_hits, stats.tools_total,
                        stats.prefix_hits, stats.prefix_total,
                    );
                }
            }

            let openai_response = transform_openai_response(
                &gemini_resp,
                Some(&session_id),
                message_count,
                Some(&client_tool_names),
            );
            if debug_logger::is_enabled(&debug_cfg) {
                let converted_response = serde_json::to_value(&openai_response)
                    .unwrap_or_else(|e| json!({ "serialization_error": e.to_string() }));
                let payload = json!({
                    "kind": "exchange_summary",
                    "protocol": "openai",
                    "trace_id": trace_id,
                    "original_codex_request": original_body,
                    "gemini_request": gemini_body_for_debug,
                    "gemini_raw_response": gemini_resp,
                    "converted_codex_response": converted_response,
                });
                debug_logger::write_exchange_payload(
                    &debug_cfg,
                    Some(&trace_id),
                    "exchange_summary",
                    &payload,
                )
                .await;
            }
            return Ok((
                StatusCode::OK,
                [
                    ("X-Account-Email", email.as_str()),
                    ("X-Mapped-Model", mapped_model.as_str()),
                ],
                Json(openai_response),
            )
                .into_response());
        }

        // 处理特定错误并重试
        let status_code = status.as_u16();
        let _retry_after = response
            .headers()
            .get("Retry-After")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| format!("HTTP {}", status_code));
        last_error = format!("HTTP {}: {}", status_code, error_text);

        // [New] 打印错误报文日志
        tracing::error!(
            "[OpenAI-Upstream] Error Response {}: {}",
            status_code,
            error_text
        );
        if debug_logger::is_enabled(&debug_cfg) {
            let payload = json!({
                "kind": "upstream_response_error",
                "protocol": "openai",
                "trace_id": trace_id,
                "original_model": openai_req.model,
                "mapped_model": mapped_model,
                "request_type": config.request_type,
                "attempt": attempt,
                "status": status_code,
                "upstream_url": upstream_url,
                "account": mask_email(&email),
                "error_text": error_text,
            });
            debug_logger::write_debug_payload(
                &debug_cfg,
                Some(&trace_id),
                "upstream_response_error",
                &payload,
            )
            .await;
        }

        // 确定重试策略
        let strategy = determine_retry_strategy(status_code, &error_text, false);

        // 3. 标记限流状态(用于 UI 显示)
        if status_code == 429 || status_code == 529 || status_code == 503 || status_code == 500 {
            // [FIX] Use async version with model parameter for fine-grained rate limiting
            token_manager
                .mark_rate_limited_async(
                    &email,
                    status_code,
                    _retry_after.as_deref(),
                    &error_text,
                    Some(&mapped_model),
                )
                .await;
        }

        // 执行退避
        if apply_retry_strategy(
            strategy.clone(),
            attempt,
            max_attempts,
            status_code,
            &trace_id,
        )
        .await
        {
            // [NEW] Apply Client Adapter "let_it_crash" strategy
            if let Some(adapter) = &client_adapter {
                if adapter.let_it_crash() && attempt > 0 {
                    // For let_it_crash clients (like opencode), allow maybe 1 retry but then fail fast
                    // to prevent long hangs on UI.
                    tracing::warn!(
                        "[OpenAI] let_it_crash active: Aborting retries after attempt {}",
                        attempt
                    );
                    // Breaking loop to return error immediately
                    // Reuse existing error return logic via loop exit behavior?
                    // Or construct error here?
                    // Let's just break for now, which will trigger the "All accounts exhausted" or last error logic.
                    break;
                }
            }

            // 判断是否需要轮换账号
            if !should_rotate_account(status_code, Some(&strategy)) {
                debug!(
                    "[{}] Keeping same account for status {} (Grace Retry or Server Issue)",
                    trace_id, status_code
                );
                force_rotate = false;
            } else {
                force_rotate = true;
            }

            // 2. [REMOVED] 不再特殊处理 QUOTA_EXHAUSTED，允许账号轮换
            // if error_text.contains("QUOTA_EXHAUSTED") { ... }
            /*
            if error_text.contains("QUOTA_EXHAUSTED") {
                error!(
                    "OpenAI Quota exhausted (429) on account {} attempt {}/{}, stopping to protect pool.",
                    email,
                    attempt + 1,
                    max_attempts
                );
                return Ok((status, [("X-Account-Email", email.as_str()), ("X-Mapped-Model", mapped_model.as_str())], error_text).into_response());
            }
            */

            // 3. 其他限流或服务器过载情况，轮换账号
            tracing::warn!(
                "OpenAI Upstream {} on {} attempt {}/{}, rotating account",
                status_code,
                email,
                attempt + 1,
                max_attempts
            );
            continue;
        }

        // [NEW] 处理 400 错误 (Thinking 签名失效)
        if status_code == 400
            && (error_text.contains("Invalid `signature`")
                || error_text.contains("thinking.signature")
                || error_text.contains("Invalid signature")
                || error_text.contains("Corrupted thought signature"))
        {
            tracing::warn!(
                "[OpenAI] Signature error detected on account {}, retrying without thinking",
                email
            );

            // 追加修复提示词到最后一条用户消息
            if let Some(last_msg) = openai_req.messages.last_mut() {
                if last_msg.role == "user" {
                    let repair_prompt = "\n\n[System Recovery] Your previous output contained an invalid signature. Please regenerate the response without the corrupted signature block.";

                    if let Some(content) = &mut last_msg.content {
                        use crate::proxy::mappers::openai::{OpenAIContent, OpenAIContentBlock};
                        match content {
                            OpenAIContent::String(s) => {
                                s.push_str(repair_prompt);
                            }
                            OpenAIContent::Array(arr) => {
                                arr.push(OpenAIContentBlock::Text {
                                    text: repair_prompt.to_string(),
                                });
                            }
                        }
                        tracing::debug!("[OpenAI] Appended repair prompt to last user message");
                    }
                }
            }

            continue; // 重试
        }

        // 只有 403 (权限/地区限制) 和 401 (认证失效) 触发账号轮换
        if status_code == 403 || status_code == 401 {
            if apply_retry_strategy(
                RetryStrategy::FixedDelay(Duration::from_millis(200)),
                attempt,
                max_attempts,
                status_code,
                &trace_id,
            )
            .await
            {
                continue;
            }
        }

        // 只有 403 (权限/地区限制) 和 401 (认证失效) 触发账号轮换
        if status_code == 403 || status_code == 401 {
            // [NEW] 403 时设置 is_forbidden 状态，避免 Claude Code 会话退出
            if status_code == 403 {
                if let Some(acc_id) = token_manager.get_account_id_by_email(&email) {
                    // Check for VALIDATION_REQUIRED error - temporarily block account
                    if error_text.contains("VALIDATION_REQUIRED")
                        || error_text.contains("verify your account")
                        || error_text.contains("validation_url")
                    {
                        tracing::warn!(
                            "[OpenAI] VALIDATION_REQUIRED detected on account {}, temporarily blocking",
                            email
                        );
                        // Block for 10 minutes (default, configurable via config file)
                        let block_minutes = 10i64;
                        let block_until = chrono::Utc::now().timestamp() + (block_minutes * 60);

                        if let Err(e) = token_manager
                            .set_validation_block_public(&acc_id, block_until, &error_text)
                            .await
                        {
                            tracing::error!("Failed to set validation block: {}", e);
                        }
                    }

                    // 设置 is_forbidden 状态
                    if let Err(e) = token_manager.set_forbidden(&acc_id, &error_text).await {
                        tracing::error!("Failed to set forbidden status: {}", e);
                    }
                }
            }

            if apply_retry_strategy(
                RetryStrategy::FixedDelay(Duration::from_millis(200)),
                attempt,
                max_attempts,
                status_code,
                &trace_id,
            )
            .await
            {
                continue;
            }
        }

        // 404 等由于模型配置或路径错误的 HTTP 异常，直接报错，不进行无效轮换
        error!(
            "OpenAI Upstream non-retryable error {} on account {}: {}",
            status_code, email, error_text
        );
        return Ok((
            status,
            [
                ("X-Account-Email", email.as_str()),
                ("X-Mapped-Model", mapped_model.as_str()),
            ],
            // [FIX] Return JSON error for better client compatibility
            Json(json!({
                "error": {
                    "message": error_text,
                    "type": "upstream_error",
                    "code": status_code
                }
            })),
        )
            .into_response());
    }

    // 所有尝试均失败
    if let Some(email) = last_email {
        Ok((
            StatusCode::TOO_MANY_REQUESTS,
            [("X-Account-Email", email), ("X-Mapped-Model", mapped_model)],
            format!("All accounts exhausted. Last error: {}", last_error),
        )
            .into_response())
    } else {
        Ok((
            StatusCode::TOO_MANY_REQUESTS,
            [("X-Mapped-Model", mapped_model)],
            format!("All accounts exhausted. Last error: {}", last_error),
        )
            .into_response())
    }
}

// --- Codex GUIDANCE PROMPTS ---

const APPLY_PATCH_CHAT_PATH_SYSTEM_GUIDANCE_ZH: &str = concat!(
    "[apply_patch chat-path 指引 — 由 codex-app-transfer adapter 注入,因为上游 lark 语法约束在 chat function-call provider 上不可用]\n",
    "\n",
    "**务必使用 `apply_patch` tool 写文件内容** —— 新建文件、单行编辑、整文件重写都一样。**绝不使用 shell `cat <<EOF > file` / `printf '<content>' > file` / `echo '<content>' > file` / 任何 `>` 重定向来写实际文件内容** —— 这样做会绕过 Codex diff UI 和审计 trail。**同样,绝不使用 `sed -i` / `perl -i` / `ed`、或 shell 按行号删除(如 `sed -i 'N,Md' file`)来编辑或删除已有文件内容** —— 就地 shell 编辑器绕过 diff UI,且对多次编辑间的行号漂移很脆弱(按过期行号删会切错、损坏文件)。(新建或空文件用 `*** Add File: <path>` —— 不要用 shell 重定向。)**优先外科式针对性编辑**:要改/替换已有内容时,只发改动那几行的 `-`(旧)和 `+`(新),保持每个 hunk 最小;且**不要**把增删空行作为编辑的一部分,除非空行本身就是改动(空行 `+`/`-` 位置歧义、可能静默 apply 失败)。**删除内容 —— 即便是跨很多行的大段连续块 —— 也用 apply_patch hunk 里的 `-` 行表达,或用 `*** Delete File: <path>` 删整个文件;不要因为块大就改用 `sed`/`python` 按行范围删除。** 对同一文件的多处不相邻编辑可以放进**一次** apply_patch 调用、分成多个 hunk。**不要**整段重新生成再追加,**不要**因为改了一部分就整文件重写。整文件替换(同一 patch 内 `*** Delete File: <path>` + `*** Add File: <path>`、每行前缀 `+`)**仅限**真正需要时:新建全新内容,或几乎每行都不同。\n",
    "\n",
    "调用 `apply_patch` tool 时,遵循以下基于非 OpenAI chat provider 实战观察总结的规则:\n",
    "\n",
    "1. 推荐的 Update File 形式是**最简形态**:仅 `-line`(要删的行,byte-exact)和 `+line`(新行)直接跟在 `*** Update File: <path>` 之后 —— 无 `@@`、无 context 行。",
    "凡是 `-` 行在文件里**唯一**时(简单单行编辑、配置改动、function 签名等绝大多数场景皆是)就用这个形态。例:\n",
    "  *** Update File: src/config.py\n",
    "  -DEBUG = False\n",
    "  +DEBUG = True\n",
    "若 `-` 行单独**有歧义**(同一行文本在文件多处出现),在上方/下方加空格前缀的 context 行(` line`)钉住它。",
    "若 context 行也不足以消歧,再在独立行上加**单端** `@@ <header>` 标记(`@@ class Foo`、`@@ def bar():`、`@@ fn main() {`)。",
    "**绝不加尾随 `@@`**(`@@ <header> @@` 是错的)—— Codex Desktop 的 V4A applier 会把尾随 `@@` 当字面文本,报 `Failed to find context '... @@'`。",
    "深层嵌套消歧时用**多个** `@@` 行各占一行(例如 `@@ class Outer\\n@@ def inner():`),每条都是单端。\n",
    "\n",
    "2. Add File **不用** `@@` 标记、**不用** hunk。`*** Add File: <path>` 之后,新文件**每一行内容**(包括空行,写成单个 `+` 占一行)都前缀 `+`。没 `+` 前缀的原始源码(例如直接写 `def main():`)会触发 `'def main():' is not a valid hunk header` 错误。",
    "但结构标记 `*** Begin Patch` / `*** Add File:` / `*** End Patch` **不是内容,不加前缀**。尤其**绝不给终止符加前缀**(`+*** End Patch` 是错的):带 `+` 的终止符会被当成内容行,在新建文件末尾留下一行字面 `*** End Patch`。\n",
    "\n",
    "3. 每个 `-` 行和空格前缀的 context 行**必须**跟文件 byte-for-byte 一致(同样的前导 whitespace,不能 trim 尾随空格,字符完全相同)。不确定时先用 shell 跑 `cat <path>` 或 `sed -n '1,80p' <path>` 查一下,再用真实字节组 patch。靠猜会触发 `Failed to find context '<your guess>'` 错误。\n",
    "\n",
    "3a. 行前缀是**单字符**,前缀和内容之间**没有空格**:写 `-DEBUG = False`(不是 `- DEBUG = False`)、`+DEBUG = True`(不是 `+ DEBUG = True`),context 行 ` keepme`(单个前导空格)。Codex Desktop V4A applier 可能容忍多余空格,但其它 apply_patch 实现严格 —— 前缀写紧凑。\n",
    "\n",
    "4. **不要**在同一 patch 内对同一路径同时用 `*** Add File: <path>` 和 `*** Update File: <path>`。Update 步骤会在 Add 步骤落盘前读文件,看到空文件后失败。要么 (a) 让 `*** Add File:` 一次性写最终内容,要么 (b) 拆成两个独立的 `apply_patch` 调用。\n",
    "\n",
    "5. 新建或空文件用 `*** Add File: <path>`、每行前缀 `+`(不要用 `*** Update File:`,也不要用 shell 重定向)。\n",
    "\n",
    "6. 多行文件里,**没有**对应 `-` 行的孤立 `+` 行会**追加**在上文 context 之下 —— **不会**替换任何已有行。要修改已有行,**必须**同时包含 `-` 行(删旧内容)和 `+` 行(加新内容)。",
    "空格前缀的 context 行是拿来**跟文件匹配**的、绝不新增 —— 它必须已存在于文件中。要引入全新行,前缀 `+`;把文件里还没有的行写成 context(或不加前缀)会得到一个无实际改动、apply 失败或 `Failed to find context` 的 hunk。\n",
    "\n",
    "7. Update 报 `Failed to find context` 时,说明 `-`/context 行跟文件 byte 对不上 —— 重新 `cat <path>` / `sed -n` 读文件、把这些行改成完全一致,再重试**同一个**针对性 Update。**不要**升级成整文件重写/重新追加,把编辑保持在改动的那几行。",
    "在**一次**回合里对**同一文件**做多处编辑时,每个已应用的 hunk 都会改变文件内容 —— 把相关编辑放进**一个** patch 的多个 hunk,或在多次独立调用之间重新读文件。某个 `-` 行不再匹配,可能是它**已经被删掉**(被前一个 hunk、或本回合更早的编辑)—— 重发同一删除前先确认它还在,别盲目重试。\n",
    "\n",
    "8. `*** Begin Patch` **必须**是 `input` 字符串的字面第一行 —— 不能有前导空格,前面不能有其它内容,绝不能直接写 `*** Add File:` 或任何操作 header。漏了会触发 `invalid patch: The first line of the patch must be '*** Begin Patch'`。\n",
    "\n",
    "9. `*** Update File: <old>` + `*** Move to: <new>` **要求**至少一个 hunk(带 `-`/`+` 行或 `*** End of File` 标记)。空的 Update+Move 块会报 `Update file hunk for path '<old>' is empty`。**纯重命名不改内容**时,在同一 patch 内用 `*** Delete File: <old>` + `*** Add File: <new>`(把原内容每行前缀 `+` 复制过去)。**重命名同时改内容**时,保留 Update+Move 并写真实的 `-`/`+` hunk。\n",
    "\n",
    "10. 编辑 memory 文件(如 `~/.codex/memories/MEMORY.md`)要格外小心:并发进程可能在你上次读它、到你的 patch 落地之间重写该文件。打 patch **前立即** `cat` 该文件,让每个 `-`/context 行都是**当前**文件里存在的行,并用最小唯一锚点(如单个 `@@ <section header>` + 只写你实际改的那几行)。过期的 `-` 行 —— 内容已被并发固化(consolidation)改掉 —— 会报 `Failed to find context`;失败时重新读、按当前字节重建,而不是重试过期 patch。\n",
    "\n",
    "遵循这些规则可以避免 retry 风暴,提升首次尝试的成功率。"
);

const WEB_TOOLS_SYSTEM_GUIDANCE_ZH: &str = "联网获取信息时(实时事实 / 价格 / 文档 / 新闻 / 版本号 / 任何你不确定或可能已过时的内容),**优先用 `web_search` 和 `web_fetch` 工具,不要用 shell 的 curl / wget / python 去抓 URL 或搜索引擎**。本机对外网访问受限,shell 直连通常被防火墙 / 反爬拦截(返回空或 403),会白费多轮尝试、最后只能靠可能过时的记忆作答;而这两个工具经代理(浏览器 TLS 指纹 + headless 渲染)能真正抓到。用法:先 `web_search(query)` 找信息源,再用 `web_fetch(url)` 读该页**完整正文**(返回全文、自己读)。之前抓过的某 URL 若在对话历史里被折叠 / 压缩、需要回看完整原文, 用 `read_url_local(url)` 从本地缓存取回, 不必重新联网。";

const CHINESE_LANGUAGE_DIRECTIVE: &str =
    "**请始终使用简体中文回复用户**(代码、命令、标识符、文件路径等技术内容保持原文,不要翻译)。";

fn tools_register_apply_patch(body: &Value) -> bool {
    let Some(tools) = body.get("tools").and_then(Value::as_array) else {
        return false;
    };
    tools.iter().any(|t| {
        t.get("name").and_then(Value::as_str) == Some("apply_patch")
            && (t.get("type").and_then(Value::as_str) == Some("custom")
                || t.get("type").and_then(Value::as_str) == Some("function"))
    })
}

fn tools_register_web_fetch(body: &Value) -> bool {
    fn entry_is_web_tool(t: &Value) -> bool {
        matches!(
            t.get("name").and_then(Value::as_str),
            Some("web_fetch") | Some("web_search")
        )
    }
    body.get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools.iter().any(|t| {
                if t.get("type").and_then(Value::as_str) == Some("namespace") {
                    t.get("tools")
                        .and_then(Value::as_array)
                        .is_some_and(|inner| inner.iter().any(entry_is_web_tool))
                } else {
                    entry_is_web_tool(t)
                }
            })
        })
        .unwrap_or(false)
}

fn apply_patch_chat_guidance_message() -> Value {
    let content =
        format!("{CHINESE_LANGUAGE_DIRECTIVE}\n\n{APPLY_PATCH_CHAT_PATH_SYSTEM_GUIDANCE_ZH}");
    serde_json::json!({
        "role": "system",
        "content": content,
    })
}

fn web_tools_guidance_message() -> Value {
    serde_json::json!({
        "role": "system",
        "content": WEB_TOOLS_SYSTEM_GUIDANCE_ZH,
    })
}

// --- END Codex GUIDANCE PROMPTS ---

/// 处理 Legacy Completions API (/v1/completions)
/// 将 Prompt 转换为 Chat Message 格式，复用 handle_chat_completions
pub async fn handle_completions(
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    State(state): State<AppState>,
    Json(mut body): Json<Value>,
) -> Response {
    debug!(
        "Received /v1/completions or /v1/responses payload: {:?}",
        body
    );
    let original_body = body.clone();
    let debug_cfg = state.debug_logging.read().await.clone();

    // [MULTI-TURN] 支持 previous_response_id 链式历史恢复
    // 当客户端通过 HTTP POST /v1/responses 传入 previous_response_id 时，
    // 从服务器端 session store 取出上一轮的历史，合并到本轮的 input 中
    let previous_response_id = body
        .get("previous_response_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let response_id_for_save = format!("resp-{}", uuid::Uuid::new_v4());
    let http_tool_call_cache: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    if let Some(ref prev_id) = previous_response_id {
        if let Some(session) = crate::proxy::http_session_store::get_session(prev_id).await {
            // 把历史 input items 合并进来
            let existing_input = body
                .get("input")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let merged = crate::proxy::http_session_store::merge_history_with_new_input(
                session.input_items,
                &[],
                &existing_input,
                &http_tool_call_cache,
            );
            let merged_len = merged.len();
            if let Some(obj) = body.as_object_mut() {
                obj.insert("input".to_string(), json!(merged));
                // 从历史 session 继承 instructions（如果本轮没带）
                if !obj.contains_key("instructions") && !session.instructions.is_empty() {
                    obj.insert("instructions".to_string(), json!(session.instructions));
                }
                // 继承 model（如果本轮没带）
                if !obj.contains_key("model") && !session.model.is_empty() {
                    obj.insert("model".to_string(), json!(session.model));
                }
            }
            tracing::debug!(
                "[MultiTurn] Restored session from prev_id={}, {} items in history",
                prev_id,
                merged_len
            );
        }
    }

    let is_codex_style = body.get("input").is_some() || body.get("instructions").is_some();

    // 1. Convert Payload to Messages (Shared Chat Format)
    if is_codex_style {
        let instructions = body
            .get("instructions")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let input_items = body.get("input").and_then(|v| v.as_array());
        let (interaction_ledger, mut step_markers) = codex_ledger_from_body(&body);

        let mut messages = Vec::new();

        // System Instructions
        if !instructions.is_empty() {
            messages.push(json!({ "role": "system", "content": instructions }));
        }

        let mut call_id_to_name = std::collections::HashMap::new();
        let mut skipped_incomplete_custom_call_ids = std::collections::HashSet::new();

        // Pass 1: Build Call ID to Name Map
        if let Some(items) = input_items {
            for item in items {
                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if item_type == "custom_tool_call"
                    && item.get("status").and_then(|v| v.as_str()) == Some("incomplete")
                {
                    if let Some(call_id) = item
                        .get("call_id")
                        .and_then(|v| v.as_str())
                        .or_else(|| item.get("id").and_then(|v| v.as_str()))
                    {
                        skipped_incomplete_custom_call_ids.insert(call_id.to_string());
                    }
                    continue;
                }
                match item_type {
                    "function_call" | "custom_tool_call" | "local_shell_call"
                    | "web_search_call" => {
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

        let mut seen_apply_patch_failures = std::collections::HashSet::new();
        let mut apply_patch_failure_distinct_count = 0usize;

        // Pass 2: Map durable conversation items to Gemini messages. Visible
        // assistant commentary stays in Codex's local transcript and must not
        // be replayed as model history.
        if let Some(items) = input_items {
            for item in items {
                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let step_marker = step_markers.pop_front();
                if item_type == "custom_tool_call"
                    && item.get("status").and_then(|v| v.as_str()) == Some("incomplete")
                {
                    continue;
                }
                match item_type {
                    "message" => {
                        let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                        let content = item.get("content").and_then(|v| v.as_array());
                        let mut text_parts = Vec::new();
                        let mut image_parts: Vec<Value> = Vec::new();

                        if let Some(parts) = content {
                            for part in parts {
                                // 处理文本块
                                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                    text_parts.push(text.to_string());
                                }
                                // [NEW] 处理图像块 (Codex input_image 格式)
                                else if part.get("type").and_then(|v| v.as_str())
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
                                }
                                // [NEW] 兼容标准 OpenAI image_url 格式
                                else if part.get("type").and_then(|v| v.as_str())
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

                        let joined_text = text_parts.join("\n");
                        if is_codex_transcript_only_assistant_message(item, &joined_text) {
                            continue;
                        }

                        // 构造消息内容：如果有图像则使用数组格式
                        if image_parts.is_empty() {
                            let content = prefix_with_step_marker(step_marker, joined_text);
                            let message = json!({
                                "role": role,
                                "content": content
                            });
                            messages.push(message);
                        } else {
                            let mut content_blocks: Vec<Value> = Vec::new();
                            let marker_text = prefix_with_step_marker(step_marker, joined_text);
                            if !marker_text.is_empty() {
                                content_blocks.push(json!({
                                    "type": "text",
                                    "text": marker_text
                                }));
                            }
                            content_blocks.extend(image_parts);
                            let message = json!({
                                "role": role,
                                "content": content_blocks
                            });
                            messages.push(message);
                        }
                    }
                    "function_call" | "custom_tool_call" | "local_shell_call"
                    | "web_search_call" => {
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

                        // Handle native shell calls
                        if item_type == "custom_tool_call" {
                            if let Some(input) = item.get("input").and_then(|v| v.as_str()) {
                                args_str = serde_json::to_string(&json!({ "input": input }))
                                    .unwrap_or_else(|_| "{}".to_string());
                            }
                        } else if item_type == "local_shell_call" {
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

                        let message = json!({
                            "role": "assistant",
                            "content": "",
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
                        });
                        messages.push(message);
                    }
                    "function_call_output" | "custom_tool_call_output" => {
                        let call_id = item
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        if item_type == "custom_tool_call_output"
                            && skipped_incomplete_custom_call_ids.contains(call_id)
                        {
                            tracing::warn!(
                                "Skipping output for incomplete custom tool call {}",
                                call_id
                            );
                            continue;
                        }
                        let output = item.get("output");
                        let mut output_str = if let Some(o) = output {
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

                        let name = if let Some(name) = call_id_to_name.get(call_id).cloned() {
                            name
                        } else if item_type == "custom_tool_call_output" {
                            tracing::warn!(
                                "Skipping orphan custom_tool_call_output for unknown call_id {}",
                                call_id
                            );
                            continue;
                        } else {
                            tracing::warn!(
                                "Unknown function_call_output tool name for call_id {}, defaulting to 'shell'",
                                call_id
                            );
                            "shell".to_string()
                        };

                        if name == "apply_patch" {
                            output_str = compact_apply_patch_failure_output(
                                output_str,
                                &mut seen_apply_patch_failures,
                                &mut apply_patch_failure_distinct_count,
                            );
                        }
                        output_str = prefix_with_step_marker(step_marker, output_str);

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
            if let Some(ledger) = interaction_ledger {
                obj.insert("_interaction_ledger".to_string(), json!(ledger));
            }
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

    // 2. Reuse handle_chat_completions logic (wrapping with custom handler or direct call)
    // Actually, due to SSE handling differences (Codex uses different event format), we replicate the loop here or abstract it.
    // For now, let's replicate the core loop but with Codex specific SSE mapping.

    // [Fix Phase 2] Backport normalization logic from handle_chat_completions
    // Handle "instructions" + "input" (Codex style) -> system + user messages
    // This is critical because `transform_openai_request` expects `messages` to be populated.

    // [FIX] 检查是否已经有 messages (被第一次标准化处理过)
    let has_codex_fields = body.get("instructions").is_some() || body.get("input").is_some();
    let already_normalized = body
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false);

    // 只有在未标准化时才进行简单转换
    if has_codex_fields && !already_normalized {
        tracing::debug!("[Codex] Performing simple normalization (messages not yet populated)");

        let mut messages = Vec::new();

        // instructions -> system message
        if let Some(inst) = body.get("instructions").and_then(|v| v.as_str()) {
            if !inst.is_empty() {
                messages.push(json!({
                    "role": "system",
                    "content": inst
                }));
            }
        }

        // input -> user message (支持对象数组形式的对话历史)
        if let Some(input) = body.get("input") {
            if let Some(s) = input.as_str() {
                messages.push(json!({
                    "role": "user",
                    "content": s
                }));
            } else if let Some(arr) = input.as_array() {
                // 判断是消息对象数组还是简单的内容块/字符串数组
                let is_message_array = arr
                    .first()
                    .and_then(|v| v.as_object())
                    .map(|obj| obj.contains_key("role") || obj.contains_key("type"))
                    .unwrap_or(false);

                if is_message_array {
                    // 深度识别：像处理 messages 一样处理 input 数组，并自动映射 Responses API 的工具流
                    for item in arr {
                        if let Some(obj) = item.as_object() {
                            if let Some(item_type) = obj.get("type").and_then(|v| v.as_str()) {
                                match item_type {
                                    "message" => {
                                        let role = obj
                                            .get("role")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("user");
                                        let content =
                                            obj.get("content").cloned().unwrap_or(json!(""));
                                        messages.push(json!({ "role": role, "content": content }));
                                    }
                                    "function_call" | "custom_tool_call" => {
                                        let call_id = obj
                                            .get("call_id")
                                            .or_else(|| obj.get("id"))
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        let name =
                                            obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                        let mut arguments = obj
                                            .get("arguments")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        if item_type == "custom_tool_call" {
                                            if let Some(input) =
                                                obj.get("input").and_then(|v| v.as_str())
                                            {
                                                arguments = serde_json::to_string(
                                                    &json!({ "input": input }),
                                                )
                                                .unwrap_or_else(|_| "{}".to_string());
                                            }
                                        }
                                        messages.push(json!({
                                            "role": "assistant",
                                            "content": "",
                                            "tool_calls": [{
                                                "id": if call_id.is_empty() { "call_unknown" } else { call_id },
                                                "type": "function",
                                                "function": { "name": name, "arguments": arguments },
                                            }],
                                        }));
                                    }
                                    "function_call_output" | "custom_tool_call_output" => {
                                        let call_id = obj
                                            .get("call_id")
                                            .or_else(|| obj.get("tool_call_id"))
                                            .or_else(|| obj.get("id"))
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        let output_value =
                                            obj.get("output").cloned().unwrap_or(json!(""));
                                        let output_str = if let Some(s) = output_value.as_str() {
                                            s.to_string()
                                        } else {
                                            output_value.to_string()
                                        };
                                        messages.push(json!({
                                            "role": "tool",
                                            "tool_call_id": call_id,
                                            "content": output_str,
                                        }));
                                    }
                                    _ => {
                                        messages.push(item.clone());
                                    }
                                }
                                continue;
                            }
                        }
                        messages.push(item.clone());
                    }
                } else {
                    // 降级处理：传统的字符串或混合内容拼接
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

    // [FIX] 在 openai_req 反序列化之前，从 body 中捕获原始 input 和 instructions
    // 用于后续 session 保存时，保留完整的工具调用历史（而非从 openai_req.messages 重建丢失信息）
    let session_save_input: Vec<serde_json::Value> = body
        .get("input")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let session_save_instructions: String = body
        .get("instructions")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let normalized_interaction_ledger = body.get("_interaction_ledger").cloned();

    if let Some(obj) = body.as_object_mut() {
        obj.remove("instructions");
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
                refusal: None,
            });
    }

    // [NEW v4.2.0] Context Management & Reasoning Replay
    let session_id_str = SessionManager::extract_openai_session_id(&openai_req);

    let client_tool_names =
        crate::proxy::mappers::openai::request::extract_client_tool_names(&openai_req.tools);

    crate::proxy::mappers::context_manager::ContextManager::restore_openai_reasoning_content(
        &mut openai_req.messages,
        &session_id_str,
    );

    let experimental_cfg = state.experimental.read().await;
    let compression_level = if experimental_cfg.compression_level == "disabled" {
        if experimental_cfg.enable_usage_scaling {
            "high".to_string()
        } else {
            "disabled".to_string()
        }
    } else {
        experimental_cfg.compression_level.clone()
    };

    let mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
        &openai_req.model,
        &*state.custom_mapping.read().await,
    );
    let trace_id = format!("req_{}", chrono::Utc::now().timestamp_subsec_millis());
    if debug_logger::is_enabled(&debug_cfg) {
        if let Some(ledger) = normalized_interaction_ledger {
            let payload = json!({
                "kind": "normalized_interaction_ledger",
                "protocol": "openai",
                "trace_id": trace_id.clone(),
                "request_path": uri.path(),
                "original_model": openai_req.model.clone(),
                "interaction_ledger": ledger,
            });
            debug_logger::write_exchange_payload(
                &debug_cfg,
                Some(&trace_id),
                "normalized_interaction_ledger",
                &payload,
            )
            .await;
        }
    }
    let token_manager = state.token_manager.clone();

    let mut compression_applied = false;
    let mut is_purified = false;

    if compression_level == "high" {
        let context_limit = if mapped_model.contains("flash") {
            1_000_000
        } else {
            2_000_000
        };

        let raw_estimated =
            crate::proxy::mappers::context_manager::ContextManager::estimate_openai_token_usage(
                &openai_req,
            );
        let calibrator = crate::proxy::mappers::estimation_calibrator::get_calibrator();
        let mut estimated_usage = calibrator.calibrate(raw_estimated);
        let mut usage_ratio = estimated_usage as f32 / context_limit as f32;

        let threshold_l1 = experimental_cfg.context_compression_threshold_l1;
        let threshold_l2 = experimental_cfg.context_compression_threshold_l2;
        let threshold_l3 = experimental_cfg.context_compression_threshold_l3;

        tracing::info!(
            "[{}] [ContextManager] [OpenAI] Context pressure: {:.1}% (raw: {}, calibrated: {} / {}), Calibration factor: {:.2}",
            trace_id, usage_ratio * 100.0, raw_estimated, estimated_usage, context_limit, calibrator.get_factor()
        );

        // ===== Layer 1: Tool Message Trimming =====
        if usage_ratio > threshold_l1 && !compression_applied {
            if crate::proxy::mappers::context_manager::ContextManager::trim_openai_tool_messages(
                &mut openai_req.messages,
                5,
            ) {
                tracing::info!(
                    "[{}] [Layer-1] [OpenAI] Tool trimming triggered (usage: {:.1}%, threshold: {:.1}%)",
                    trace_id, usage_ratio * 100.0, threshold_l1 * 100.0
                );
                compression_applied = true;

                let new_raw = crate::proxy::mappers::context_manager::ContextManager::estimate_openai_token_usage(&openai_req);
                let new_usage = calibrator.calibrate(new_raw);
                let new_ratio = new_usage as f32 / context_limit as f32;

                tracing::info!(
                    "[{}] [Layer-1] [OpenAI] Compression result: {:.1}% → {:.1}% (saved {} tokens)",
                    trace_id,
                    usage_ratio * 100.0,
                    new_ratio * 100.0,
                    estimated_usage - new_usage
                );

                if new_ratio < 0.7 {
                    estimated_usage = new_usage;
                    usage_ratio = new_ratio;
                } else {
                    usage_ratio = new_ratio;
                    compression_applied = false;
                }
            }
        }

        // ===== Layer 2: Thinking Content Compression =====
        if usage_ratio > threshold_l2 && !compression_applied {
            tracing::info!(
                "[{}] [Layer-2] [OpenAI] Thinking compression triggered (usage: {:.1}%, threshold: {:.1}%)",
                trace_id, usage_ratio * 100.0, threshold_l2 * 100.0
            );

            if crate::proxy::mappers::context_manager::ContextManager::compress_openai_thinking_preserve_signature(
                &mut openai_req.messages,
                4,
            ) {
                is_purified = true;
                compression_applied = true;

                let new_raw = crate::proxy::mappers::context_manager::ContextManager::estimate_openai_token_usage(&openai_req);
                let new_usage = calibrator.calibrate(new_raw);
                let new_ratio = new_usage as f32 / context_limit as f32;

                tracing::info!(
                    "[{}] [Layer-2] [OpenAI] Compression result: {:.1}% → {:.1}% (saved {} tokens)",
                    trace_id, usage_ratio * 100.0, new_ratio * 100.0, estimated_usage - new_usage
                );

                usage_ratio = new_ratio;
            }
        }

        // ===== Layer 3: Fork Conversation + XML Summary =====
        if usage_ratio > threshold_l3 && !compression_applied {
            tracing::info!(
                "[{}] [Layer-3] [OpenAI] Context pressure ({:.1}%) exceeded threshold ({:.1}%), attempting Fork+Summary",
                trace_id, usage_ratio * 100.0, threshold_l3 * 100.0
            );

            let token_manager_clone = token_manager.clone();

            match try_compress_openai_with_summary(
                &openai_req,
                &trace_id,
                &token_manager_clone,
                &session_id_str,
            )
            .await
            {
                Ok(forked_req) => {
                    tracing::info!(
                        "[{}] [Layer-3] [OpenAI] Fork successful: {} → {} messages",
                        trace_id,
                        openai_req.messages.len(),
                        forked_req.messages.len()
                    );

                    openai_req = forked_req;
                    is_purified = false;

                    let new_raw = crate::proxy::mappers::context_manager::ContextManager::estimate_openai_token_usage(&openai_req);
                    let new_usage = calibrator.calibrate(new_raw);
                    let new_ratio = new_usage as f32 / context_limit as f32;

                    tracing::info!(
                        "[{}] [Layer-3] [OpenAI] Compression result: {:.1}% → {:.1}% (saved {} tokens)",
                        trace_id, usage_ratio * 100.0, new_ratio * 100.0, estimated_usage - new_usage
                    );
                }
                Err(e) => {
                    tracing::error!(
                        "[{}] [Layer-3] [OpenAI] Fork+Summary failed: {}, falling back to error response",
                        trace_id, e
                    );
                    return (
                        StatusCode::BAD_REQUEST,
                        format!("Context too long and automatic compression failed: {}", e),
                    )
                        .into_response();
                }
            }
        }
    } else if compression_level != "disabled" {
        if crate::proxy::mappers::context_manager::ContextManager::trim_openai_tool_messages(
            &mut openai_req.messages,
            5,
        ) {
            tracing::info!("[Codex-Context] Trimmed old tool messages to keep last 5 rounds");
        }

        if compression_level == "medium" {
            if crate::proxy::mappers::context_manager::ContextManager::purify_openai_history(
                &mut openai_req.messages,
                crate::proxy::mappers::context_manager::PurificationStrategy::Soft,
            ) {
                tracing::info!("[Codex-Context] Purified older assistant reasoning_content and natural language history");
            }
        }
    }

    let assistant_turn_index = openai_req
        .messages
        .iter()
        .filter(|m| m.role == "assistant")
        .count();

    let upstream = state.upstream.clone();
    let pool_size = token_manager.len();
    // [FIX] Ensure max_attempts is at least 2 to allow for internal retries
    let max_attempts = MAX_RETRY_ATTEMPTS.min(pool_size.saturating_add(1)).max(2);

    let mut last_error = String::new();
    let mut last_email: Option<String> = None;

    if debug_logger::is_enabled(&debug_cfg) {
        let payload = json!({
            "kind": "original_request",
            "protocol": "openai",
            "trace_id": trace_id,
            "request_path": uri.path(),
            "request": original_body,
        });
        debug_logger::write_exchange_payload(
            &debug_cfg,
            Some(&trace_id),
            "original_request",
            &payload,
        )
        .await;
    }

    let mut force_rotate = false;

    for attempt in 0..max_attempts {
        // 3. 模型配置解析
        // 将 OpenAI 工具转为 Value 数组以便探测联网
        let tools_val: Option<Vec<Value>> = openai_req
            .tools
            .as_ref()
            .map(|list| list.iter().cloned().collect());
        let config = crate::proxy::mappers::common_utils::resolve_request_config(
            &openai_req.model,
            &mapped_model,
            &tools_val,
            None, // size
            None, // quality
            None, // image_size
            None, // body
        );

        // 3. 提取 SessionId (复用)
        // [New] 使用 TokenManager 内部逻辑提取 session_id，支持粘性调度
        let session_id_str = SessionManager::extract_openai_session_id(&openai_req);
        let session_id = Some(session_id_str.as_str());

        let (access_token, project_id, email, account_id, _wait_ms) = match token_manager
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

        let mapped_model = token_manager
            .resolve_dynamic_model_for_account(&account_id, &mapped_model)
            .await;

        last_email = Some(email.clone());

        info!("✓ Using account: {} (type: {})", email, config.request_type);

        let proxy_token = token_manager.get_token_by_id(&account_id);
        let (gemini_body, session_id, message_count, _prefix_hash) = transform_openai_request(
            &openai_req,
            &project_id,
            &mapped_model,
            proxy_token.as_ref(),
        );
        let gemini_body_for_debug = gemini_body.clone();
        if debug_logger::is_enabled(&debug_cfg) {
            let payload = json!({
                "kind": "v1internal_request",
                "protocol": "openai",
                "trace_id": trace_id,
                "request_path": uri.path(),
                "original_model": openai_req.model,
                "mapped_model": mapped_model,
                "request_type": config.request_type,
                "attempt": attempt,
                "v1internal_request": gemini_body_for_debug.clone(),
            });
            debug_logger::write_exchange_payload(
                &debug_cfg,
                Some(&trace_id),
                "v1internal_request",
                &payload,
            )
            .await;
        }

        // [DEBUG v4.2.0] Detailed size analysis of Gemini request body
        if let Some(contents) = gemini_body.get("contents").and_then(|c| c.as_array()) {
            let mut sizes = Vec::new();
            for (idx, msg) in contents.iter().enumerate() {
                let role = msg
                    .get("role")
                    .and_then(|r| r.as_str())
                    .unwrap_or("unknown");
                let msg_str = serde_json::to_string(msg).unwrap_or_default();
                sizes.push(format!("msg_{}[{}]: {} chars", idx, role, msg_str.len()));
            }

            let system_instruction_len = gemini_body
                .get("request")
                .and_then(|r| r.get("systemInstruction"))
                .map(|s| serde_json::to_string(s).unwrap_or_default().len())
                .unwrap_or(0);

            let tools_len = gemini_body
                .get("request")
                .and_then(|r| r.get("tools"))
                .map(|t| serde_json::to_string(t).unwrap_or_default().len())
                .unwrap_or(0);

            tracing::info!(
                "[Codex-Token-Analysis] Total parts: {}. SystemInstruction: {} chars, Tools: {} chars. Content sizes: {:?}",
                contents.len(),
                system_instruction_len,
                tools_len,
                sizes
            );
        }

        // [AUTO-CONVERSION] For Legacy/Codex as well
        let client_wants_stream = openai_req.stream;
        let force_stream_internally = !client_wants_stream;
        let list_response = client_wants_stream || force_stream_internally;
        let method = if list_response {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        let query_string = if list_response { Some("alt=sse") } else { None };

        let call_result = match upstream
            .call_v1_internal(
                method,
                &access_token,
                gemini_body,
                query_string,
                Some(account_id.as_str()),
            )
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

        let response = call_result.response;
        let upstream_url = response.url().to_string();
        let status = response.status();
        if status.is_success() {
            // [智能限流] 请求成功，重置该账号的连续失败计数
            token_manager.mark_account_success(&email);

            if list_response {
                use axum::body::Body;
                use axum::response::Response;
                use futures::StreamExt;

                let upstream_meta = json!({
                    "protocol": "openai",
                    "trace_id": trace_id,
                    "request_path": uri.path(),
                    "original_model": openai_req.model,
                    "mapped_model": mapped_model,
                    "request_type": config.request_type,
                    "attempt": attempt,
                    "status": status.as_u16(),
                    "upstream_url": upstream_url,
                });
                let gemini_stream = debug_logger::wrap_stream_with_debug(
                    Box::pin(response.bytes_stream()),
                    debug_cfg.clone(),
                    trace_id.clone(),
                    "upstream_response",
                    upstream_meta,
                );

                // DECISION: Which stream to create?
                // If client wants stream: give them what they asked (Legacy/Codex SSE).
                // If forced stream: use Chat SSE + Collector, because our collector works on Chat format
                // and we already have logic to convert Chat JSON -> Legacy JSON.

                if client_wants_stream {
                    let mut openai_stream = if is_codex_style {
                        use crate::proxy::mappers::openai::streaming::create_codex_sse_stream;
                        create_codex_sse_stream(
                            gemini_stream,
                            openai_req.model.clone(),
                            session_id,
                            message_count,
                            assistant_turn_index,
                        )
                    } else {
                        use crate::proxy::mappers::openai::streaming::create_legacy_sse_stream;
                        create_legacy_sse_stream(
                            gemini_stream,
                            openai_req.model.clone(),
                            session_id,
                            message_count,
                        )
                    };

                    // [P1 FIX] Enhanced Peek logic (Reused from above/standard)
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
                                if stream_chunk_has_error_event(&bytes) {
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
                    let converted_meta = json!({
                        "protocol": "openai",
                        "trace_id": trace_id,
                        "stage": "converted_codex_response",
                        "request_path": uri.path(),
                        "original_model": openai_req.model,
                        "mapped_model": mapped_model,
                        "request_type": config.request_type,
                        "attempt": attempt,
                        "status": status.as_u16(),
                        "upstream_url": upstream_url,
                    });
                    let combined_stream = debug_logger::wrap_stream_with_debug(
                        Box::pin(combined_stream),
                        debug_cfg.clone(),
                        trace_id.clone(),
                        "converted_codex_response",
                        converted_meta,
                    );

                    // [MULTI-TURN][FIX] 保存本次完整 input_items 到 session store
                    // 使用从 body 中提取的原始 input（含文本/工具调用/工具结果全量历史），
                    // 而非从 openai_req.messages 重建（会丢失 tool_calls/tool 角色等信息）
                    {
                        let save_input = session_save_input.clone();
                        let save_instructions = session_save_instructions.clone();
                        let save_model = openai_req.model.clone();
                        let entry = crate::proxy::http_session_store::HttpSessionEntry {
                            input_items: save_input,
                            instructions: save_instructions,
                            model: save_model,
                            last_accessed: std::time::Instant::now(),
                        };
                        let rid = response_id_for_save.clone();
                        tokio::spawn(async move {
                            crate::proxy::http_session_store::save_session(rid, entry).await;
                        });
                    }
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
                    // Forced Stream Internal -> Convert to Legacy JSON
                    // Use CHAT SSE Stream (so Collector can parse it)
                    use crate::proxy::mappers::openai::streaming::create_openai_sse_stream;
                    // Note: We use create_openai_sse_stream regardless of is_codex_style here,
                    // because we just want the content aggregation which chat stream does well.
                    let mut openai_stream = create_openai_sse_stream(
                        gemini_stream,
                        openai_req.model.clone(),
                        session_id,
                        message_count,
                        Some(client_tool_names.clone()),
                    );

                    // Peek Logic (Repeated for safety/correctness on this stream type)
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
                                if stream_chunk_has_error_event(&bytes) {
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
                    let converted_meta = json!({
                        "protocol": "openai",
                        "trace_id": trace_id,
                        "stage": "converted_codex_response",
                        "request_path": uri.path(),
                        "original_model": openai_req.model,
                        "mapped_model": mapped_model,
                        "request_type": config.request_type,
                        "attempt": attempt,
                        "status": status.as_u16(),
                        "upstream_url": upstream_url,
                    });
                    let combined_stream = debug_logger::wrap_stream_with_debug(
                        Box::pin(combined_stream),
                        debug_cfg.clone(),
                        trace_id.clone(),
                        "converted_codex_response",
                        converted_meta,
                    );

                    // Collect
                    use crate::proxy::mappers::openai::collector::collect_stream_to_json;
                    match collect_stream_to_json(combined_stream).await {
                        Ok(chat_resp) => {
                            let is_responses_api = uri.path() == "/v1/responses";

                            if is_responses_api {
                                let mut output = Vec::new();
                                for c in chat_resp.choices.iter() {
                                    let text = match &c.message.content {
                                        Some(
                                            crate::proxy::mappers::openai::OpenAIContent::String(s),
                                        ) => s.clone(),
                                        _ => "".to_string(),
                                    };

                                    let has_content = !text.is_empty();
                                    let has_tools = c.message.tool_calls.is_some()
                                        && !c.message.tool_calls.as_ref().unwrap().is_empty();

                                    if has_content || has_tools {
                                        let mut msg_obj = serde_json::Map::new();
                                        msg_obj.insert("type".to_string(), json!("message"));
                                        msg_obj.insert("role".to_string(), json!("assistant"));

                                        if has_content {
                                            msg_obj.insert("content".to_string(), json!(text));
                                        }
                                        if let Some(tool_calls) = &c.message.tool_calls {
                                            msg_obj.insert(
                                                "tool_calls".to_string(),
                                                json!(tool_calls),
                                            );
                                        }
                                        output.push(serde_json::Value::Object(msg_obj));
                                    }
                                }

                                // Calculate usage if available
                                let usage_value = if let Some(ref usage) = chat_resp.usage {
                                    usage.to_responses_usage_value()
                                } else {
                                    json!({
                                        "input_tokens": 0,
                                        "input_tokens_details": {
                                            "cached_tokens": 0
                                        },
                                        "output_tokens": 0,
                                        "output_tokens_details": {
                                            "reasoning_tokens": 0
                                        },
                                        "total_tokens": 0
                                    })
                                };

                                let resp = json!({
                                    "type": "response",
                                    "id": format!("resp_{}", uuid::Uuid::new_v4().simple()),
                                    "status": "completed",
                                    "output": output,
                                    "usage": usage_value
                                });
                                if debug_logger::is_enabled(&debug_cfg) {
                                    let payload = json!({
                                        "kind": "exchange_summary",
                                        "protocol": "openai",
                                        "trace_id": trace_id,
                                        "request_path": uri.path(),
                                        "original_codex_request": original_body.clone(),
                                        "gemini_request": gemini_body_for_debug.clone(),
                                        "converted_codex_response": resp.clone(),
                                        "gemini_raw_response_ref": "see upstream_response file with the same trace_id",
                                    });
                                    debug_logger::write_exchange_payload(
                                        &debug_cfg,
                                        Some(&trace_id),
                                        "exchange_summary",
                                        &payload,
                                    )
                                    .await;
                                }

                                return (
                                    StatusCode::OK,
                                    [
                                        ("X-Account-Email", email.as_str()),
                                        ("X-Mapped-Model", mapped_model.as_str()),
                                    ],
                                    Json(resp),
                                )
                                    .into_response();
                            }

                            // NOW: Convert Chat Response -> Legacy Response (Same logic as below)
                            let choices = chat_resp
                                .choices
                                .iter()
                                .map(|c| {
                                    let mut text = match &c.message.content {
                                        Some(
                                            crate::proxy::mappers::openai::OpenAIContent::String(s),
                                        ) => s.clone(),
                                        _ => "".to_string(),
                                    };
                                    if let Some(ref reasoning) = c.message.reasoning_content {
                                        if !reasoning.is_empty() {
                                            text = format!("{}\n\n{}", reasoning, text);
                                        }
                                    }
                                    json!({
                                        "text": text,
                                        "index": c.index,
                                        "logprobs": null,
                                        "finish_reason": c.finish_reason
                                    })
                                })
                                .collect::<Vec<_>>();

                            let legacy_resp = json!({
                                "id": chat_resp.id,
                                "object": "text_completion",
                                "created": chat_resp.created,
                                "model": chat_resp.model,
                                "choices": choices,
                                "usage": chat_resp.usage
                            });
                            if debug_logger::is_enabled(&debug_cfg) {
                                let payload = json!({
                                    "kind": "exchange_summary",
                                    "protocol": "openai",
                                    "trace_id": trace_id,
                                    "request_path": uri.path(),
                                    "original_codex_request": original_body.clone(),
                                    "gemini_request": gemini_body_for_debug.clone(),
                                    "converted_codex_response": legacy_resp.clone(),
                                    "gemini_raw_response_ref": "see upstream_response file with the same trace_id",
                                });
                                debug_logger::write_exchange_payload(
                                    &debug_cfg,
                                    Some(&trace_id),
                                    "exchange_summary",
                                    &payload,
                                )
                                .await;
                            }

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

            let chat_resp = transform_openai_response(
                &gemini_resp,
                Some("session-123"),
                1,
                Some(&client_tool_names),
            );

            let is_responses_api = uri.path() == "/v1/responses";

            if is_responses_api {
                let mut output = Vec::new();
                for c in chat_resp.choices.iter() {
                    let text = match &c.message.content {
                        Some(crate::proxy::mappers::openai::OpenAIContent::String(s)) => s.clone(),
                        _ => "".to_string(),
                    };

                    let has_content = !text.is_empty();
                    let has_tools = c.message.tool_calls.is_some()
                        && !c.message.tool_calls.as_ref().unwrap().is_empty();

                    if has_content || has_tools {
                        let mut msg_obj = serde_json::Map::new();
                        msg_obj.insert("type".to_string(), json!("message"));
                        msg_obj.insert("role".to_string(), json!("assistant"));

                        if has_content {
                            msg_obj.insert("content".to_string(), json!(text));
                        }
                        if let Some(tool_calls) = &c.message.tool_calls {
                            msg_obj.insert("tool_calls".to_string(), json!(tool_calls));
                        }
                        output.push(serde_json::Value::Object(msg_obj));
                    }
                }

                // Calculate usage if available
                let usage_value = if let Some(ref usage) = chat_resp.usage {
                    usage.to_responses_usage_value()
                } else {
                    json!({
                        "input_tokens": 0,
                        "input_tokens_details": {
                            "cached_tokens": 0
                        },
                        "output_tokens": 0,
                        "output_tokens_details": {
                            "reasoning_tokens": 0
                        },
                        "total_tokens": 0
                    })
                };

                let resp = json!({
                    "type": "response",
                    "id": format!("resp_{}", uuid::Uuid::new_v4().simple()),
                    "status": "completed",
                    "output": output,
                    "usage": usage_value
                });
                if debug_logger::is_enabled(&debug_cfg) {
                    let payload = json!({
                        "kind": "exchange_summary",
                        "protocol": "openai",
                        "trace_id": trace_id,
                        "request_path": uri.path(),
                        "original_codex_request": original_body.clone(),
                        "gemini_request": gemini_body_for_debug.clone(),
                        "gemini_raw_response": gemini_resp.clone(),
                        "converted_codex_response": resp.clone(),
                    });
                    debug_logger::write_exchange_payload(
                        &debug_cfg,
                        Some(&trace_id),
                        "exchange_summary",
                        &payload,
                    )
                    .await;
                }

                return (
                    StatusCode::OK,
                    [
                        ("X-Account-Email", email.as_str()),
                        ("X-Mapped-Model", mapped_model.as_str()),
                    ],
                    Json(resp),
                )
                    .into_response();
            }

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
                "choices": choices,
                "usage": chat_resp.usage
            });
            if debug_logger::is_enabled(&debug_cfg) {
                let payload = json!({
                    "kind": "exchange_summary",
                    "protocol": "openai",
                    "trace_id": trace_id,
                    "request_path": uri.path(),
                    "original_codex_request": original_body.clone(),
                    "gemini_request": gemini_body_for_debug.clone(),
                    "gemini_raw_response": gemini_resp.clone(),
                    "converted_codex_response": legacy_resp.clone(),
                });
                debug_logger::write_exchange_payload(
                    &debug_cfg,
                    Some(&trace_id),
                    "exchange_summary",
                    &payload,
                )
                .await;
            }

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

        // 3. 标记限流状态(用于 UI 显示)
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

        // 确定重试策略
        // 确定重试策略 (对齐官方 1.5s Grace Window)
        let strategy = determine_retry_strategy(status_code, &error_text, false);

        // 执行退备
        if apply_retry_strategy(
            strategy.clone(),
            attempt,
            max_attempts,
            status_code,
            &trace_id,
        )
        .await
        {
            // 继续重试 (loop 会增加 attempt, 导致 force_rotate=true)
            continue;
        } else {
            // 不可重试
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

    // 所有尝试均失败
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

pub async fn handle_list_models(State(state): State<AppState>) -> impl IntoResponse {
    use crate::proxy::common::model_mapping::get_all_dynamic_models;

    let model_ids = get_all_dynamic_models(&state.custom_mapping, Some(&state.token_manager)).await;

    let data: Vec<_> = model_ids
        .into_iter()
        .map(|id| {
            json!({
                "id": id,
                "object": "model",
                "created": 1706745600,
                "owned_by": "antigravity"
            })
        })
        .collect();

    Json(json!({
        "object": "list",
        "data": data
    }))
}

/// OpenAI Images API: POST /v1/images/generations
/// 处理图像生成请求，转换为 Gemini API 格式
pub async fn handle_chat_redirection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    handle_chat_completions(State(state), headers, Json(body)).await
}

async fn intercept_chat_to_image(
    state: AppState,
    body: Value,
    model_name: &str,
) -> Result<Response, (StatusCode, String)> {
    // 1. Extract prompt from messages
    let mut prompt = String::new();
    if let Some(messages) = body.get("messages").and_then(|v| v.as_array()) {
        for msg in messages {
            if msg.get("role").and_then(|v| v.as_str()) == Some("user") {
                if let Some(content) = msg.get("content") {
                    if let Some(s) = content.as_str() {
                        prompt = s.to_string();
                    } else if let Some(arr) = content.as_array() {
                        for part in arr {
                            if part.get("type").and_then(|v| v.as_str()) == Some("text") {
                                prompt.push_str(
                                    part.get("text").and_then(|v| v.as_str()).unwrap_or(""),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    if prompt.is_empty() {
        prompt = "A beautiful painting".to_string(); // fallback
    }

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // 2. Call internal image generator
    let img_req = json!({
        "prompt": prompt,
        "model": model_name,
        "n": 1,
        "response_format": "url"
    });

    match handle_images_generations_internal(state, img_req).await {
        Ok((email, img_res)) => {
            // Extract URL
            let mut img_markdown = String::new();
            if let Some(data) = img_res.get("data").and_then(|v| v.as_array()) {
                for item in data {
                    if let Some(url) = item.get("url").and_then(|v| v.as_str()) {
                        img_markdown.push_str(&format!("![Generated Image]({})\n\n", url));
                    }
                }
            }

            if img_markdown.is_empty() {
                img_markdown = "Failed to extract image URL from generation result.".to_string();
            }

            // 3. Construct Chat Completion Response
            if is_stream {
                use axum::body::Body;

                let chunk = json!({
                    "id": format!("chatcmpl-img-{}", uuid::Uuid::new_v4()),
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model_name,
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "role": "assistant",
                            "content": img_markdown
                        },
                        "finish_reason": null
                    }]
                });

                let done_chunk = json!({
                    "id": format!("chatcmpl-img-{}", uuid::Uuid::new_v4()),
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model_name,
                    "choices": [{
                        "index": 0,
                        "delta": {},
                        "finish_reason": "stop"
                    }]
                });

                let sse_data = format!(
                    "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                    chunk.to_string(),
                    done_chunk.to_string()
                );

                let body = Body::from(sse_data);
                Ok(Response::builder()
                    .header("Content-Type", "text/event-stream")
                    .header("Cache-Control", "no-cache")
                    .header("X-Account-Email", email)
                    .body(body)
                    .unwrap())
            } else {
                let resp = json!({
                    "id": format!("chatcmpl-img-{}", uuid::Uuid::new_v4()),
                    "object": "chat.completion",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model_name,
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": img_markdown
                        },
                        "finish_reason": "stop"
                    }],
                    "usage": { "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0 }
                });

                Ok((
                    StatusCode::OK,
                    [("X-Account-Email", email.as_str())],
                    Json(resp),
                )
                    .into_response())
            }
        }
        Err((status, msg, _email)) => Err((status, msg)),
    }
}

pub async fn handle_images_generations(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match handle_images_generations_internal(state, body).await {
        Ok((email_header, openai_response)) => Ok((
            StatusCode::OK,
            [("X-Account-Email", email_header.as_str())],
            Json(openai_response),
        )
            .into_response()),
        // Attach the attempted account to error responses too, so the traffic log shows
        // which account the failed (e.g. 502/503) image request used.
        Err((status, msg, email_opt)) => {
            let email = email_opt.unwrap_or_default();
            Ok((status, [("X-Account-Email", email)], msg).into_response())
        }
    }
}

pub async fn handle_images_generations_internal(
    state: AppState,
    body: Value,
) -> Result<(String, Value), (StatusCode, String, Option<String>)> {
    // 1. 解析请求参数
    let prompt = body.get("prompt").and_then(|v| v.as_str()).ok_or((
        StatusCode::BAD_REQUEST,
        "Missing 'prompt' field".to_string(),
        None,
    ))?;

    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3.1-flash-image");

    let n = body.get("n").and_then(|v| v.as_u64()).unwrap_or(1) as usize;

    let size = body.get("size").and_then(|v| v.as_str());

    let response_format = body
        .get("response_format")
        .and_then(|v| v.as_str())
        .unwrap_or("b64_json");

    let quality = body.get("quality").and_then(|v| v.as_str());

    let image_size = body
        .get("image_size")
        .or(body.get("imageSize"))
        .and_then(|v| v.as_str());

    let style = body
        .get("style")
        .and_then(|v| v.as_str())
        .unwrap_or("vivid");

    info!(
        "[Images] Received request: model={}, prompt={:.50}..., n={}, size={}, quality={}, style={}",
        model,
        prompt,
        n,
        size.unwrap_or("auto"),
        quality.unwrap_or("auto"),
        style
    );

    // 2. 使用 common_utils 解析图片配置（统一逻辑，支持动态计算宽高比和 quality 映射）
    let (image_config, clean_model_name) =
        crate::proxy::mappers::common_utils::parse_image_config_with_params(
            model, size, quality, image_size,
        );

    // 3. Prompt Enhancement（保留原有逻辑）
    let mut final_prompt = prompt.to_string();
    if quality == Some("hd") {
        final_prompt.push_str(", (high quality, highly detailed, 4k resolution, hdr)");
    }
    match style {
        "vivid" => final_prompt.push_str(", (vivid colors, dramatic lighting, rich details)"),
        "natural" => final_prompt.push_str(", (natural lighting, realistic, photorealistic)"),
        _ => {}
    }

    // 4. 并发发送请求
    // 注意：不再在外部获取 Token，而是移入 Task 内部并在重试时获取
    let upstream = state.upstream.clone();
    let token_manager = state.token_manager.clone();
    let max_pool_size = token_manager.len();
    let max_attempts = MAX_RETRY_ATTEMPTS
        .min(max_pool_size.saturating_add(1))
        .max(2);

    let mut tasks = Vec::new();

    // Track the last account actually attempted, so error responses (502/503) can be
    // attributed to an account in the traffic log instead of showing "(none)".
    let attempted_account = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));

    for _ in 0..n {
        let upstream = upstream.clone();
        let token_manager = token_manager.clone();
        let final_prompt = final_prompt.clone();
        let image_config = image_config.clone(); // 使用解析后的完整配置
        let _response_format = response_format.to_string();

        let model_to_use = clean_model_name.clone();
        let attempted_account = attempted_account.clone();

        tasks.push(tokio::spawn(async move {
            let mut last_error = String::new();
            let mut force_rotate = false;

            for attempt in 0..max_attempts {
                let (access_token, project_id, email, account_id, _wait_ms) = match token_manager
                    .get_token("image_gen", force_rotate, None, &model_to_use)
                    .await
                {
                    Ok(t) => t,
                    Err(e) => {
                        last_error = format!("Token error: {}", e);
                        if attempt < max_attempts - 1 {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            continue;
                        }
                        break;
                    }
                };
                if let Ok(mut g) = attempted_account.lock() {
                    *g = Some(email.clone());
                }

                // [FIX] Resolve to the account-specific dynamic image model, exactly like the
                // chat (openai.rs:232) and gemini (gemini.rs:155) handlers do. Sending the static
                // alias (e.g. "gemini-3-pro-image") made upstream return 404 "Requested entity was
                // not found" because each account exposes its own concrete image model id.
                let resolved_model = token_manager
                    .resolve_dynamic_model_for_account(&account_id, &model_to_use)
                    .await;

                let gemini_body = json!({
                    "project": project_id,
                    "requestId": format!("agent-{}", uuid::Uuid::new_v4()),
                    "model": resolved_model,
                    "userAgent": "antigravity",
                    "requestType": "image_gen",
                    "request": {
                        "contents": [{
                            "role": "user",
                            "parts": [{"text": final_prompt}]
                        }],
                        "generationConfig": {
                            "candidateCount": 1, // 强制单张
                            "imageConfig": image_config // ✅ 使用完整配置（包含 aspectRatio 和 imageSize）
                        },
                        "safetySettings": [
                            { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "OFF" },
                            { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "OFF" },
                            { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "OFF" },
                            { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "OFF" },
                        ]
                    }
                });

                match upstream
                    .call_v1_internal(
                        "generateContent",
                        &access_token,
                        gemini_body,
                        None,
                        Some(account_id.as_str()),
                    )
                    .await
                {
                    Ok(call_result) => {
                        let response = call_result.response;
                        let status = response.status();
                        if !status.is_success() {
                            let err_text = response.text().await.unwrap_or_default();
                            let status_code = status.as_u16();
                            last_error = format!("Upstream error {}: {}", status, err_text);

                            // 429/500/503: mark limited and rotate to another account
                            if status_code == 429 || status_code == 503 || status_code == 500 {
                                tracing::warn!(
                                    "[Images] Account {} rate limited/error ({}), rotating...",
                                    email,
                                    status_code
                                );
                                token_manager
                                    .mark_rate_limited_async(
                                        &email,
                                        status_code,
                                        None,
                                        &err_text,
                                        Some(model_to_use.as_str()),
                                    )
                                    .await;
                                force_rotate = true;
                                continue; // Retry loop
                            }

                            // [FIX] 403/404 usually mean THIS account lacks the image model or
                            // project access. Rotate to another account instead of failing the
                            // whole request, so an image-capable account can serve it.
                            if (status_code == 403 || status_code == 404)
                                && attempt < max_attempts - 1
                            {
                                tracing::warn!(
                                    "[Images] Account {} returned {} for image gen, rotating to another account",
                                    email,
                                    status_code
                                );
                                force_rotate = true;
                                continue;
                            }

                            // Other errors: return
                            return Err(last_error);
                        }
                        token_manager.mark_account_success(&account_id);
                        token_manager
                            .clear_persisted_live_limit(&account_id, Some(&model_to_use))
                            .await;

                        match response.json::<Value>().await {
                            Ok(json) => return Ok((json, email)),
                            Err(e) => return Err(format!("Parse error: {}", e)),
                        }
                    }
                    Err(e) => {
                        last_error = format!("Network error: {}", e);
                        continue;
                    }
                }
            }

            // All attempts failed
            Err(format!("Max retries exhausted. Last error: {}", last_error))
        }));
    }

    // 5. 收集结果
    let mut images: Vec<Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut used_email: Option<String> = None;

    for (idx, task) in tasks.into_iter().enumerate() {
        match task.await {
            Ok(result) => match result {
                Ok((gemini_resp, email_used)) => {
                    // Capture the email from the first successful task for logging
                    if used_email.is_none() {
                        used_email = Some(email_used);
                    }
                    let raw = gemini_resp.get("response").unwrap_or(&gemini_resp);
                    if let Some(parts) = raw
                        .get("candidates")
                        .and_then(|c| c.get(0))
                        .and_then(|cand| cand.get("content"))
                        .and_then(|content| content.get("parts"))
                        .and_then(|p| p.as_array())
                    {
                        for part in parts {
                            if let Some(img) = part.get("inlineData") {
                                let data = img.get("data").and_then(|v| v.as_str()).unwrap_or("");
                                if !data.is_empty() {
                                    if response_format == "url" {
                                        let mime_type = img
                                            .get("mimeType")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("image/png");
                                        images.push(json!({
                                            "url": format!("data:{};base64,{}", mime_type, data)
                                        }));
                                    } else {
                                        images.push(json!({
                                            "b64_json": data
                                        }));
                                    }
                                    tracing::debug!("[Images] Task {} succeeded", idx);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("[Images] Task {} failed: {}", idx, e);
                    errors.push(e);
                }
            },
            Err(e) => {
                let err_msg = format!("Task join error: {}", e);
                tracing::error!("[Images] Task {} join error: {}", idx, e);
                errors.push(err_msg);
            }
        }
    }

    if images.is_empty() {
        let error_msg = if !errors.is_empty() {
            errors.join("; ")
        } else {
            "No images generated".to_string()
        };
        tracing::error!("[Images] All {} requests failed. Errors: {}", n, error_msg);

        // [FIX] Map upstream status codes correctly instead of forcing 502
        let status = if error_msg.contains("429") || error_msg.contains("Quota exhausted") {
            StatusCode::TOO_MANY_REQUESTS
        } else if error_msg.contains("503") || error_msg.contains("Service Unavailable") {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::BAD_GATEWAY
        };

        let attempted = used_email
            .clone()
            .or_else(|| attempted_account.lock().ok().and_then(|g| g.clone()));
        return Err((status, error_msg, attempted));
    }

    // 部分成功时记录警告
    if !errors.is_empty() {
        tracing::warn!(
            "[Images] Partial success: {} out of {} requests succeeded. Errors: {}",
            images.len(),
            n,
            errors.join("; ")
        );
    }

    tracing::info!(
        "[Images] Successfully generated {} out of {} requested image(s)",
        images.len(),
        n
    );

    // 6. 构建 OpenAI 格式响应
    let openai_response = json!({
        "created": chrono::Utc::now().timestamp(),
        "data": images
    });

    // [FIX] 图像生成成功后触发配额刷新 (Issue #1995)
    tokio::spawn(async move {
        let _ = account::refresh_all_quotas_logic().await;
    });

    let email_header = used_email.unwrap_or_default();
    Ok((email_header, openai_response))
}

pub async fn handle_images_edits(
    State(state): State<AppState>,
    mut multipart: axum::extract::Multipart,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    tracing::info!("[Images] Received edit request");

    let mut image_data: Option<(String, String)> = None;
    let mut mask_data: Option<(String, String)> = None;
    let mut reference_images: Vec<(String, String)> = Vec::new(); // Store (base64 data, mime type) reference images
    let mut prompt = String::new();
    let mut n = 1;
    let mut size = "1024x1024".to_string();
    let mut response_format = "b64_json".to_string();
    let mut model = "gemini-3.1-flash-image".to_string();
    let mut aspect_ratio: Option<String> = None;
    let mut image_size_param: Option<String> = None;
    let mut style: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Multipart error: {}", e)))?
    {
        let name = field.name().unwrap_or("").to_string();

        if name == "image" {
            let mime_type = field
                .content_type()
                .map(|content_type| content_type.to_string())
                .unwrap_or_else(|| "image/png".to_string());
            let data = field
                .bytes()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("Image read error: {}", e)))?;
            image_data = Some((
                base64::engine::general_purpose::STANDARD.encode(data),
                mime_type,
            ));
        } else if name == "mask" {
            let mime_type = field
                .content_type()
                .map(|content_type| content_type.to_string())
                .unwrap_or_else(|| "image/png".to_string());
            let data = field
                .bytes()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("Mask read error: {}", e)))?;
            mask_data = Some((
                base64::engine::general_purpose::STANDARD.encode(data),
                mime_type,
            ));
        } else if name.starts_with("image") && name != "image_size" {
            // Support image1, image2, etc.
            let mime_type = field
                .content_type()
                .map(|content_type| content_type.to_string())
                .unwrap_or_else(|| "image/jpeg".to_string());
            let data = field.bytes().await.map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("Reference image read error: {}", e),
                )
            })?;
            reference_images.push((
                base64::engine::general_purpose::STANDARD.encode(data),
                mime_type,
            ));
        } else if name == "prompt" {
            prompt = field
                .text()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("Prompt read error: {}", e)))?;
        } else if name == "n" {
            if let Ok(val) = field.text().await {
                n = val.parse().unwrap_or(1);
            }
        } else if name == "size" {
            if let Ok(val) = field.text().await {
                size = val;
            }
        } else if name == "image_size" {
            if let Ok(val) = field.text().await {
                image_size_param = Some(val);
            }
        } else if name == "aspect_ratio" {
            if let Ok(val) = field.text().await {
                aspect_ratio = Some(val);
            }
        } else if name == "style" {
            if let Ok(val) = field.text().await {
                style = Some(val);
            }
        } else if name == "response_format" {
            if let Ok(val) = field.text().await {
                response_format = val;
            }
        } else if name == "model" {
            if let Ok(val) = field.text().await {
                if !val.is_empty() {
                    model = val;
                }
            }
        }
    }

    // Validation: Require either 'image' (standard edit) OR 'prompt' (generation)
    // If reference images are present, we treat it as generation with image context
    if prompt.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Missing prompt".to_string()));
    }

    tracing::info!(
        "[Images] Edit/Ref Request: model={}, prompt={}, n={}, size={}, aspect_ratio={:?}, image_size={:?}, style={:?}, refs={}, has_main_image={}",
        model,
        prompt,
        n,
        size,
        aspect_ratio,
        image_size_param,
        style,
        reference_images.len(),
        image_data.is_some()
    );

    // 2. Prepare Config (Aspect Ratio / Size)
    // Priority: aspect_ratio param > size param
    // Priority: image_size param > quality param (derived from model suffix or default)

    // We reuse parse_image_config_with_params but need to adapt the inputs
    let size_input = aspect_ratio.as_deref().or(Some(&size)); // If aspect_ratio is "16:9", it works. If it's just "1:1", it also works.

    // Map 'image_size' (2K) to 'quality' semantics if needed, or pass directly if logic supports
    // common_utils logic: 'hd' -> 4K, 'medium' -> 2K.
    let quality_input = match image_size_param.as_deref() {
        Some("4K") => Some("hd"),
        Some("2K") => Some("medium"),
        _ => None, // Fallback to standard
    };

    let (image_config, clean_model_name) =
        crate::proxy::mappers::common_utils::parse_image_config_with_params(
            &model,
            size_input,
            quality_input,
            image_size_param.as_deref(), // [NEW] Pass direct image_size param
        );

    // 3. Construct Contents
    let mut contents_parts = Vec::new();

    // Add Prompt
    let mut final_prompt = prompt.clone();
    if let Some(s) = style {
        final_prompt.push_str(&format!(", style: {}", s));
    }
    contents_parts.push(json!({
        "text": final_prompt
    }));

    // Add Main Image (if standard edit)
    if let Some((data, mime_type)) = image_data {
        contents_parts.push(json!({
            "inlineData": {
                "mimeType": mime_type,
                "data": data
            }
        }));
    }

    // Add Mask (if standard edit)
    if let Some((data, mime_type)) = mask_data {
        contents_parts.push(json!({
            "inlineData": {
                "mimeType": mime_type,
                "data": data
            }
        }));
    }

    // Add Reference Images (Image-to-Image)
    for (ref_data, mime_type) in reference_images {
        contents_parts.push(json!({
            "inlineData": {
                "mimeType": mime_type,
                "data": ref_data
            }
        }));
    }

    // 4. 并发发送请求
    // 注意：不再在外部获取 Token，而是移入 Task 内部
    let upstream = state.upstream.clone();
    let token_manager = state.token_manager.clone();
    let max_pool_size = token_manager.len();
    let max_attempts = MAX_RETRY_ATTEMPTS
        .min(max_pool_size.saturating_add(1))
        .max(2);

    let mut tasks = Vec::new();
    for _ in 0..n {
        let upstream = upstream.clone();
        let token_manager = token_manager.clone();
        let contents_parts = contents_parts.clone();
        let image_config = image_config.clone();
        let response_format = response_format.clone();
        let model_to_use = clean_model_name.clone();

        tasks.push(tokio::spawn(async move {
            let mut last_error = String::new();

            let mut force_rotate = false;

            for attempt in 0..max_attempts {
                // 4.1 获取 Token
                let (access_token, project_id, email, account_id, _wait_ms) = match token_manager
                    .get_token("image_gen", force_rotate, None, "gemini-3-pro-image")
                    .await
                {
                    Ok(t) => t,
                    Err(e) => {
                        last_error = format!("Token error: {}", e);
                        if attempt < max_attempts - 1 {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            continue;
                        }
                        break;
                    }
                };

                // 4.2 Construct Request Body (Need project_id)
                let gemini_body = json!({
                    "project": project_id,
                    "requestId": format!("img-edit-{}", uuid::Uuid::new_v4()),
                    "model": model_to_use,
                    "userAgent": "antigravity",
                    "requestType": "image_gen",
                    "request": {
                        "contents": [{
                            "role": "user",
                            "parts": contents_parts
                        }],
                        "generationConfig": {
                            "candidateCount": 1,
                            "imageConfig": image_config,
                            "maxOutputTokens": 8192,
                            "stopSequences": [],
                            "temperature": 1.0,
                            "topP": 0.95,
                            "topK": 40
                        },
                        "safetySettings": [
                            { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "OFF" },
                            { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "OFF" },
                            { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "OFF" },
                            { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "OFF" },
                        ]
                    }
                });

                match upstream
                    .call_v1_internal(
                        "generateContent",
                        &access_token,
                        gemini_body,
                        None,
                        Some(account_id.as_str()),
                    )
                    .await
                {
                    Ok(call_result) => {
                        let response = call_result.response;
                        let status = response.status();
                        if !status.is_success() {
                            let err_text = response.text().await.unwrap_or_default();
                            let status_code = status.as_u16();
                            last_error = format!("Upstream error {}: {}", status, err_text);

                            // 429/500/503 等错误进行标记和重试
                            if status_code == 429 || status_code == 503 || status_code == 500 {
                                tracing::warn!(
                                    "[Images] Account {} rate limited/error ({}), rotating...",
                                    email,
                                    status_code
                                );
                                token_manager
                                    .mark_rate_limited_async(
                                        &email,
                                        status_code,
                                        None,
                                        &err_text,
                                        Some(&model_to_use),
                                    )
                                    .await;
                                continue; // Retry loop
                            }
                            return Err(last_error);
                        }
                        token_manager.mark_account_success(&account_id);
                        token_manager
                            .clear_persisted_live_limit(&account_id, Some(&model_to_use))
                            .await;

                        match response.json::<Value>().await {
                            Ok(json) => return Ok((json, response_format.clone(), email)),
                            Err(e) => return Err(format!("Parse error: {}", e)),
                        }
                    }
                    Err(e) => {
                        last_error = format!("Network error: {}", e);
                        continue;
                    }
                }
            }
            Err(format!("Max retries exhausted. Last error: {}", last_error))
        }));
    }

    // 5. Collect Results
    let mut images: Vec<Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut used_email: Option<String> = None;

    for (idx, task) in tasks.into_iter().enumerate() {
        match task.await {
            Ok(result) => match result {
                Ok((gemini_resp, response_format, email_used)) => {
                    if used_email.is_none() {
                        used_email = Some(email_used);
                    }
                    let raw = gemini_resp.get("response").unwrap_or(&gemini_resp);
                    if let Some(parts) = raw
                        .get("candidates")
                        .and_then(|c| c.get(0))
                        .and_then(|cand| cand.get("content"))
                        .and_then(|content| content.get("parts"))
                        .and_then(|p| p.as_array())
                    {
                        for part in parts {
                            if let Some(img) = part.get("inlineData") {
                                let data = img.get("data").and_then(|v| v.as_str()).unwrap_or("");
                                if !data.is_empty() {
                                    if response_format == "url" {
                                        let mime_type = img
                                            .get("mimeType")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("image/png");
                                        images.push(json!({
                                            "url": format!("data:{};base64,{}", mime_type, data)
                                        }));
                                    } else {
                                        images.push(json!({
                                            "b64_json": data
                                        }));
                                    }
                                    tracing::debug!("[Images] Task {} succeeded", idx);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("[Images] Task {} failed: {}", idx, e);
                    errors.push(e);
                }
            },
            Err(e) => {
                let err_msg = format!("Task join error: {}", e);
                tracing::error!("[Images] Task {} join error: {}", idx, e);
                errors.push(err_msg);
            }
        }
    }

    if images.is_empty() {
        let error_msg = if !errors.is_empty() {
            errors.join("; ")
        } else {
            "No images generated".to_string()
        };
        tracing::error!(
            "[Images] All {} edit requests failed. Errors: {}",
            n,
            error_msg
        );
        let status = if error_msg.contains("429") || error_msg.contains("Quota exhausted") {
            StatusCode::TOO_MANY_REQUESTS
        } else if error_msg.contains("503") || error_msg.contains("Service Unavailable") {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::BAD_GATEWAY
        };

        return Err((status, error_msg));
    }

    if !errors.is_empty() {
        tracing::warn!(
            "[Images] Partial success: {} out of {} requests succeeded. Errors: {}",
            images.len(),
            n,
            errors.join("; ")
        );
    }

    tracing::info!(
        "[Images] Successfully generated {} out of {} requested edited image(s)",
        images.len(),
        n
    );

    let openai_response = json!({
        "created": chrono::Utc::now().timestamp(),
        "data": images
    });

    tokio::spawn(async move {
        let _ = account::refresh_all_quotas_logic().await;
    });

    let email_header = used_email.unwrap_or_default();
    Ok((
        StatusCode::OK,
        [
            ("X-Mapped-Model", clean_model_name.as_str()),
            ("X-Account-Email", email_header.as_str()),
        ],
        Json(openai_response),
    )
        .into_response())
}

// ==========================================
// CODE INTEGRATION: Codex WebSocket Handler
// ==========================================

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use futures::{SinkExt, StreamExt};
use uuid::Uuid;

// ==========================================

// CODE INTEGRATION: Global Tool Call Cache

// ==========================================

use std::sync::OnceLock;

use tokio::sync::RwLock as TokioRwLock;

use std::collections::HashMap;

static WEBSOCKET_TOOL_CALL_CACHE: OnceLock<TokioRwLock<HashMap<String, Value>>> = OnceLock::new();

pub fn get_cached_tool_call(call_id: &str) -> Option<Value> {
    if let Some(cache) = WEBSOCKET_TOOL_CALL_CACHE.get() {
        if let Ok(guard) = cache.try_read() {
            return guard.get(call_id).cloned();
        }
    }

    None
}

pub fn insert_cached_tool_call(call_id: String, item: Value) {
    if call_id.is_empty() {
        return;
    }

    let cache = WEBSOCKET_TOOL_CALL_CACHE.get_or_init(|| TokioRwLock::new(HashMap::new()));

    if let Ok(mut guard) = cache.try_write() {
        guard.insert(call_id, item);
    }
}

#[derive(Debug, Clone)]

struct WebsocketSessionState {
    last_request: Option<Value>,
    last_response_output: Value,
    last_response_id: String,
    last_response_pending_tool_call_ids: Vec<String>,
    tool_call_cache: std::collections::HashMap<String, Value>,
}

pub async fn handle_responses_websocket(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_websocket_session(socket, headers, state))
}

async fn handle_websocket_session(mut socket: WebSocket, headers: HeaderMap, state: AppState) {
    tracing::info!("Codex responses websocket: client connected");
    let mut session_state = WebsocketSessionState {
        last_request: None,
        last_response_output: json!([]),
        last_response_id: String::new(),
        last_response_pending_tool_call_ids: Vec::new(),
        tool_call_cache: std::collections::HashMap::new(),
    };

    while let Some(msg_result) = socket.recv().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("responses websocket: read message failed: {:?}", e);
                break;
            }
        };

        let text = match msg {
            Message::Text(t) => t,
            Message::Binary(b) => match String::from_utf8(b) {
                Ok(s) => s,
                Err(_) => continue,
            },
            Message::Close(_) => {
                tracing::info!("responses websocket: client disconnected");
                break;
            }
            _ => continue,
        };

        let payload: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                let error_ev = json!({
                    "type": "error",
                    "error": {
                        "message": format!("Invalid JSON: {}", e),
                        "type": "invalid_request_error"
                    }
                });
                let _ = socket.send(Message::Text(error_ev.to_string())).await;
                continue;
            }
        };
        let ws_trace_id = format!("ws_{}", chrono::Utc::now().timestamp_subsec_millis());
        let debug_cfg = state.debug_logging.read().await.clone();
        if debug_logger::is_enabled(&debug_cfg) {
            let payload_log = json!({
                "kind": "codex_websocket_raw_request",
                "protocol": "codex_websocket",
                "trace_id": ws_trace_id,
                "raw_text": text.clone(),
                "payload": payload.clone(),
            });
            debug_logger::write_exchange_payload(
                &debug_cfg,
                Some(&ws_trace_id),
                "codex_websocket_raw_request",
                &payload_log,
            )
            .await;
        }

        if should_handle_prewarm_locally(&payload, &session_state) {
            let (created, completed) = handle_prewarm_locally(&payload, &mut session_state);
            let _ = socket.send(Message::Text(created.to_string())).await;
            let _ = socket.send(Message::Text(completed.to_string())).await;
            if debug_logger::is_enabled(&debug_cfg) {
                let payload_log = json!({
                    "kind": "codex_websocket_local_response",
                    "protocol": "codex_websocket",
                    "trace_id": ws_trace_id,
                    "events": [created, completed],
                });
                debug_logger::write_exchange_payload(
                    &debug_cfg,
                    Some(&ws_trace_id),
                    "codex_websocket_local_response",
                    &payload_log,
                )
                .await;
            }
            continue;
        }

        let normalized = match normalize_responses_websocket_request(&payload, &mut session_state) {
            Ok(n) => n,
            Err(e) => {
                let error_ev = json!({
                    "type": "error",
                    "error": {
                        "message": e,
                        "type": "invalid_request_error"
                    }
                });
                let _ = socket.send(Message::Text(error_ev.to_string())).await;
                continue;
            }
        };

        let openai_body = convert_codex_to_openai_request(normalized);
        let response_result =
            handle_chat_completions(State(state.clone()), headers.clone(), Json(openai_body)).await;

        let response = match response_result {
            Ok(res) => res.into_response(),
            Err((status, err_msg)) => {
                let error_ev = json!({
                    "type": "error",
                    "error": {
                        "message": err_msg,
                        "type": "server_error",
                        "code": status.as_u16().to_string()
                    }
                });
                let _ = socket.send(Message::Text(error_ev.to_string())).await;
                continue;
            }
        };

        if !response.status().is_success() {
            let error_ev = json!({
                "type": "error",
                "error": {
                    "message": format!("Upstream returned status {}", response.status()),
                    "type": "server_error"
                }
            });
            let _ = socket.send(Message::Text(error_ev.to_string())).await;
            continue;
        }

        let body = response.into_body();
        let mut stream = body.into_data_stream();

        let mut translation_state = TranslationState {
            response_id: format!("resp-{}", &Uuid::new_v4().to_string()[..24]),
            item_id: format!("item-{}", &Uuid::new_v4().to_string()[..16]),
            message_output_index: None,
            next_output_index: 0,
            tool_output_indices: std::collections::HashMap::new(),
            message_item_added: false,
            content_part_added: false,
            accumulated_text: String::new(),
            tool_calls: std::collections::HashMap::new(),
            tool_calls_added: std::collections::HashSet::new(),
        };

        let created_ev = json!({
            "type": "response.created",
            "response": {
                "id": &translation_state.response_id,
                "object": "response",
                "status": "in_progress",
                "output": []
            }
        });
        let mut outgoing_ws_events = Vec::new();
        send_ws_event(&mut socket, &mut outgoing_ws_events, &created_ev).await;

        let mut buffer = bytes::BytesMut::new();
        while let Some(chunk_res) = stream.next().await {
            let chunk = match chunk_res {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("Stream chunk error: {:?}", e);
                    break;
                }
            };
            buffer.extend_from_slice(&chunk);
            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                let line_raw = buffer.split_to(pos + 1);
                if let Ok(line_str) = std::str::from_utf8(&line_raw) {
                    let line = line_str.trim();
                    if line.is_empty() || !line.starts_with("data: ") {
                        continue;
                    }
                    let json_part = line.trim_start_matches("data: ").trim();
                    if json_part == "[DONE]" {
                        break;
                    }
                    if let Ok(chunk_json) = serde_json::from_str::<Value>(json_part) {
                        translate_openai_chunk_to_ws(
                            &chunk_json,
                            &mut translation_state,
                            &mut socket,
                            &mut outgoing_ws_events,
                        )
                        .await;
                    }
                }
            }
        }

        if !buffer.is_empty() {
            if let Ok(line_str) = std::str::from_utf8(&buffer) {
                let line = line_str.trim();
                if line.starts_with("data: ") {
                    let json_part = line.trim_start_matches("data: ").trim();
                    if json_part != "[DONE]" {
                        if let Ok(chunk_json) = serde_json::from_str::<Value>(json_part) {
                            translate_openai_chunk_to_ws(
                                &chunk_json,
                                &mut translation_state,
                                &mut socket,
                                &mut outgoing_ws_events,
                            )
                            .await;
                        }
                    }
                }
            }
        }

        let completed_output = finalize_ws_events(
            &mut translation_state,
            &mut socket,
            &mut session_state,
            &mut outgoing_ws_events,
        )
        .await;
        if debug_logger::is_enabled(&debug_cfg) {
            let payload_log = json!({
                "kind": "codex_websocket_converted_response",
                "protocol": "codex_websocket",
                "trace_id": ws_trace_id,
                "events": outgoing_ws_events,
                "completed_output": completed_output.clone(),
            });
            debug_logger::write_exchange_payload(
                &debug_cfg,
                Some(&ws_trace_id),
                "codex_websocket_converted_response",
                &payload_log,
            )
            .await;
        }

        session_state.last_response_output = completed_output;
        session_state.last_response_id = translation_state.response_id.clone();
        session_state.last_response_pending_tool_call_ids = translation_state
            .tool_calls
            .values()
            .map(|(_, call_id, _, _)| call_id.clone())
            .collect();
    }
}

fn should_handle_prewarm_locally(payload: &Value, state: &WebsocketSessionState) -> bool {
    if state.last_request.is_some() {
        return false;
    }
    let event_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if event_type != "response.create" {
        return false;
    }
    if let Some(generate) = payload.get("generate").and_then(|v| v.as_bool()) {
        if !generate {
            return true;
        }
    }
    false
}

fn handle_prewarm_locally(payload: &Value, state: &mut WebsocketSessionState) -> (Value, Value) {
    let response_id = format!("resp_prewarm_{}", Uuid::new_v4());
    let created_at = chrono::Utc::now().timestamp();
    let model = payload
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let created_ev = json!({
        "type": "response.created",
        "sequence_number": 0,
        "response": {
            "id": &response_id,
            "object": "response",
            "created_at": created_at,
            "status": "in_progress",
            "background": false,
            "error": null,
            "output": [],
            "model": model,
        }
    });

    let completed_ev = json!({
        "type": "response.completed",
        "sequence_number": 1,
        "response": {
            "id": &response_id,
            "object": "response",
            "created_at": created_at,
            "status": "completed",
            "background": false,
            "error": null,
            "output": [],
            "usage": {
                "input_tokens": 0,
                "input_tokens_details": {
                    "cached_tokens": 0
                },
                "output_tokens": 0,
                "output_tokens_details": {
                    "reasoning_tokens": 0
                },
                "total_tokens": 0
            },
            "model": model,
        }
    });

    let mut normalized = payload.clone();
    if let Some(obj) = normalized.as_object_mut() {
        obj.remove("type");
        obj.remove("generate");
    }
    state.last_request = Some(normalized);
    state.last_response_output = json!([]);
    state.last_response_id = response_id;
    state.last_response_pending_tool_call_ids = Vec::new();

    (created_ev, completed_ev)
}

fn normalize_responses_websocket_request(
    payload: &Value,
    state: &mut WebsocketSessionState,
) -> Result<Value, String> {
    let event_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match event_type {
        "response.create" => {
            if state.last_request.is_none() {
                let mut normalized = payload.clone();
                if let Some(obj) = normalized.as_object_mut() {
                    obj.remove("type");
                    obj.insert("stream".to_string(), Value::Bool(true));
                    if !obj.contains_key("input") {
                        obj.insert("input".to_string(), json!([]));
                    }
                }
                let model_name = normalized
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if model_name.is_empty() {
                    return Err("missing model in response.create request".to_string());
                }
                state.last_request = Some(normalized.clone());
                Ok(normalized)
            } else {
                normalize_response_subsequent_request(payload, state)
            }
        }
        "response.append" => normalize_response_subsequent_request(payload, state),
        _ => Err(format!(
            "unsupported websocket request type: {}",
            event_type
        )),
    }
}

fn normalize_response_subsequent_request(
    payload: &Value,
    state: &mut WebsocketSessionState,
) -> Result<Value, String> {
    if state.last_request.is_none() {
        return Err("websocket request received before response.create".to_string());
    }

    // [FIX] 拦截 compaction 和完整历史替换事件
    if should_replace_websocket_transcript(payload) {
        let mut normalized = payload.clone();
        if let Some(obj) = normalized.as_object_mut() {
            obj.remove("type");
            obj.remove("previous_response_id");
            obj.insert("stream".to_string(), Value::Bool(true));
        }
        state.last_request = Some(normalized.clone());
        return Ok(normalized);
    }

    // [FIX] 始终走完整的 merge 逻辑，废弃 transcript replacement 分支
    // 旧逻辑在检测到 function_call/assistant 时直接替换整个历史，导致多轮对话历史丢失
    // 正确做法：last_request.input + last_response_output + new payload.input 全部合并
    let mut merged_input = Vec::new();

    // 1. 上一轮请求的 input（已含此前所有历史）
    if let Some(last_req) = &state.last_request {
        if let Some(arr) = last_req.get("input").and_then(|v| v.as_array()) {
            merged_input.extend(arr.clone());
        }
    }

    // 2. 上一轮 response 的 output items（assistant 回复、工具调用等）
    if let Some(arr) = state.last_response_output.as_array() {
        merged_input.extend(arr.clone());
    }

    // 3. 本轮新的 input items（用户消息、工具调用结果等）
    if let Some(arr) = payload.get("input").and_then(|v| v.as_array()) {
        for item in arr {
            let t = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if t == "compaction" || t == "compaction_summary" {
                continue;
            }
            if t == "function_call_output" || t == "custom_tool_call_output" {
                if let Some(call_id) = item.get("call_id").and_then(|v| v.as_str()) {
                    state
                        .last_response_pending_tool_call_ids
                        .retain(|x| x != call_id);
                }
            }
            merged_input.push(item.clone());
        }
    }

    repair_tool_calls(&mut merged_input, &state.tool_call_cache);

    let deduped = dedupe_function_calls_by_call_id(dedupe_input_items_by_id(merged_input));

    let mut normalized = payload.clone();
    if let Some(obj) = normalized.as_object_mut() {
        obj.remove("type");
        obj.remove("previous_response_id");
        obj.insert("input".to_string(), json!(deduped));
        if !obj.contains_key("model") {
            if let Some(last_req) = &state.last_request {
                if let Some(model) = last_req.get("model") {
                    obj.insert("model".to_string(), model.clone());
                }
            }
        }
        if !obj.contains_key("instructions") {
            if let Some(last_req) = &state.last_request {
                if let Some(instructions) = last_req.get("instructions") {
                    obj.insert("instructions".to_string(), instructions.clone());
                }
            }
        }
        if !obj.contains_key("tools") {
            if let Some(last_req) = &state.last_request {
                if let Some(tools) = last_req.get("tools") {
                    obj.insert("tools".to_string(), tools.clone());
                }
            }
        }
        if !obj.contains_key("tool_choice") {
            if let Some(last_req) = &state.last_request {
                if let Some(tool_choice) = last_req.get("tool_choice") {
                    obj.insert("tool_choice".to_string(), tool_choice.clone());
                }
            }
        }
        obj.insert("stream".to_string(), Value::Bool(true));
    }
    state.last_request = Some(normalized.clone());
    Ok(normalized)
}
#[allow(dead_code)]
fn should_replace_websocket_transcript(payload: &Value) -> bool {
    let previous_response_id = payload
        .get("previous_response_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !previous_response_id.is_empty() {
        return false;
    }
    if let Some(input_array) = payload.get("input").and_then(|v| v.as_array()) {
        for item in input_array {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if item_type == "function_call" || item_type == "custom_tool_call" {
                return true;
            }
            if item_type == "message" {
                let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("");
                if role == "assistant" {
                    return true;
                }
            }
        }
    }
    false
}

#[allow(dead_code)]
fn normalize_response_transcript_replacement(payload: &Value, last_request: &Value) -> Value {
    let mut normalized = payload.clone();
    if let Some(obj) = normalized.as_object_mut() {
        obj.remove("type");
        obj.remove("previous_response_id");
        obj.insert("stream".to_string(), Value::Bool(true));
        if !obj.contains_key("model") {
            if let Some(model) = last_request.get("model") {
                obj.insert("model".to_string(), model.clone());
            }
        }
        if !obj.contains_key("instructions") {
            if let Some(instructions) = last_request.get("instructions") {
                obj.insert("instructions".to_string(), instructions.clone());
            }
        }
    }
    normalized
}

fn dedupe_input_items_by_id(items: Vec<Value>) -> Vec<Value> {
    use std::collections::{HashMap, HashSet};
    let mut referenced_call_ids = HashSet::new();
    for item in &items {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if item_type == "function_call_output" || item_type == "custom_tool_call_output" {
            if let Some(call_id) = item.get("call_id").and_then(|v| v.as_str()) {
                if !call_id.is_empty() {
                    referenced_call_ids.insert(call_id.to_string());
                }
            }
        }
    }

    let mut keep_map: HashMap<String, (usize, bool)> = HashMap::new();
    for (idx, item) in items.iter().enumerate() {
        let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if item_id.is_empty() {
            continue;
        }
        let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
        let is_referenced = !call_id.is_empty() && referenced_call_ids.contains(call_id);
        if let Some(&(existing_idx, existing_referenced)) = keep_map.get(item_id) {
            if is_referenced || !existing_referenced {
                keep_map.insert(item_id.to_string(), (idx, is_referenced));
            }
        } else {
            keep_map.insert(item_id.to_string(), (idx, is_referenced));
        }
    }

    let mut keep_indices = HashSet::new();
    for (_, (idx, _)) in keep_map {
        keep_indices.insert(idx);
    }

    let mut filtered = Vec::new();
    for (idx, item) in items.into_iter().enumerate() {
        let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if !item_id.is_empty() {
            if !keep_indices.contains(&idx) {
                continue;
            }
        }
        filtered.push(item);
    }
    filtered
}

fn dedupe_function_calls_by_call_id(items: Vec<Value>) -> Vec<Value> {
    use std::collections::HashSet;
    let mut seen_call_ids = HashSet::new();
    let mut filtered = Vec::new();
    for item in items {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if item_type == "function_call" || item_type == "custom_tool_call" {
            if let Some(call_id) = item.get("call_id").and_then(|v| v.as_str()) {
                if !call_id.is_empty() {
                    if seen_call_ids.contains(call_id) {
                        continue;
                    }
                    seen_call_ids.insert(call_id.to_string());
                }
            }
        }
        filtered.push(item);
    }
    filtered
}

fn repair_tool_calls(
    input_items: &mut Vec<Value>,
    tool_call_cache: &std::collections::HashMap<String, Value>,
) {
    let mut call_present = std::collections::HashSet::new();
    for item in input_items.iter() {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if item_type == "function_call" || item_type == "custom_tool_call" {
            if let Some(call_id) = item.get("call_id").and_then(|v| v.as_str()) {
                call_present.insert(call_id.to_string());
            }
        }
    }

    let mut new_items = Vec::new();
    let mut inserted = std::collections::HashSet::new();
    for item in input_items.drain(..) {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if item_type == "function_call_output" || item_type == "custom_tool_call_output" {
            if let Some(call_id) = item.get("call_id").and_then(|v| v.as_str()) {
                if !call_id.is_empty()
                    && !call_present.contains(call_id)
                    && !inserted.contains(call_id)
                {
                    if let Some(cached_call) = tool_call_cache.get(call_id) {
                        new_items.push(cached_call.clone());
                        inserted.insert(call_id.to_string());
                    }
                }
            }
        }
        new_items.push(item);
    }
    *input_items = new_items;
}

fn convert_codex_to_openai_request(mut body: Value) -> Value {
    let instructions = body
        .get("instructions")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let input_items = body.get("input").and_then(|v| v.as_array());
    let (interaction_ledger, mut step_markers) = codex_ledger_from_body(&body);

    let mut messages = Vec::new();
    if !instructions.is_empty() {
        messages.push(json!({ "role": "system", "content": instructions }));
    }

    let mut call_id_to_name = std::collections::HashMap::new();
    let mut skipped_incomplete_custom_call_ids = std::collections::HashSet::new();

    if let Some(items) = input_items {
        for item in items {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if item_type == "custom_tool_call"
                && item.get("status").and_then(|v| v.as_str()) == Some("incomplete")
            {
                if let Some(call_id) = item
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .or_else(|| item.get("id").and_then(|v| v.as_str()))
                {
                    skipped_incomplete_custom_call_ids.insert(call_id.to_string());
                }
                continue;
            }
            match item_type {
                "function_call" | "custom_tool_call" | "local_shell_call" | "web_search_call" => {
                    let call_id = item
                        .get("call_id")
                        .and_then(|v| v.as_str())
                        .or_else(|| item.get("id").and_then(|v| v.as_str()))
                        .unwrap_or("unknown");
                    let mut name = item
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    if item_type == "local_shell_call" || name == "local_shell_call" {
                        name = "shell".to_string();
                    } else if item_type == "web_search_call" || name == "web_search_call" {
                        name = "google_search".to_string();
                    }
                    call_id_to_name.insert(call_id.to_string(), name);
                }
                _ => {}
            }
        }
    }

    if let Some(items) = input_items {
        let mut seen_apply_patch_failures = std::collections::HashSet::new();
        let mut apply_patch_failure_distinct_count = 0usize;
        for item in items {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let step_marker = step_markers.pop_front();
            if item_type == "custom_tool_call"
                && item.get("status").and_then(|v| v.as_str()) == Some("incomplete")
            {
                continue;
            }
            match item_type {
                "message" => {
                    let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                    let content = item.get("content").and_then(|v| v.as_array());
                    let mut text_parts = Vec::new();
                    let mut image_parts = Vec::new();

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
                                    image_parts.push(json!({ "type": "image_url", "image_url": { "url": image_url } }));
                                }
                            } else if part.get("type").and_then(|v| v.as_str()) == Some("image_url")
                            {
                                if let Some(url_obj) = part.get("image_url") {
                                    image_parts.push(json!({ "type": "image_url", "image_url": url_obj.clone() }));
                                }
                            }
                        }
                    }

                    if image_parts.is_empty() {
                        let content = prefix_with_step_marker(step_marker, text_parts.join("\n"));
                        messages.push(json!({ "role": role, "content": content }));
                    } else {
                        let mut content_blocks = Vec::new();
                        let marker_text =
                            prefix_with_step_marker(step_marker, text_parts.join("\n"));
                        if !marker_text.is_empty() {
                            content_blocks.push(json!({ "type": "text", "text": marker_text }));
                        }
                        content_blocks.extend(image_parts);
                        messages.push(json!({ "role": role, "content": content_blocks }));
                    }
                }
                "function_call" | "custom_tool_call" | "local_shell_call" | "web_search_call" => {
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

                    if item_type == "custom_tool_call" {
                        if let Some(input) = item.get("input").and_then(|v| v.as_str()) {
                            args_str = serde_json::to_string(&json!({ "input": input }))
                                .unwrap_or_else(|_| "{}".to_string());
                        }
                    } else if item_type == "local_shell_call" || name == "local_shell_call" {
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
                                    .unwrap_or_else(|_| "{}".to_string());
                            }
                        }
                    } else if item_type == "web_search_call" || name == "web_search_call" {
                        name = "google_search";
                        if let Some(action) = item.get("action") {
                            let mut args_obj = serde_json::Map::new();
                            if let Some(q) = action.get("query") {
                                args_obj.insert("query".to_string(), q.clone());
                            }
                            args_str = serde_json::to_string(&args_obj)
                                .unwrap_or_else(|_| "{}".to_string());
                        }
                    }

                    messages.push(json!({
                        "role": "assistant",
                        "content": "",
                        "tool_calls": [{
                            "id": call_id,
                            "type": "function",
                            "function": { "name": name, "arguments": args_str }
                        }]
                    }));
                }
                "function_call_output" | "custom_tool_call_output" => {
                    let call_id = item
                        .get("call_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    if item_type == "custom_tool_call_output"
                        && skipped_incomplete_custom_call_ids.contains(call_id)
                    {
                        tracing::warn!(
                            "Skipping output for incomplete custom tool call {}",
                            call_id
                        );
                        continue;
                    }
                    let output = item.get("output");
                    let mut output_str = if let Some(o) = output {
                        if o.is_string() {
                            o.as_str().unwrap().to_string()
                        } else if let Some(content) = o.get("content").and_then(|v| v.as_str()) {
                            content.to_string()
                        } else {
                            o.to_string()
                        }
                    } else {
                        "".to_string()
                    };

                    let name = match call_id_to_name.get(call_id).cloned().or_else(|| {
                        get_cached_tool_call(call_id).and_then(|v| {
                            v.get("name")
                                .and_then(|n| n.as_str())
                                .map(|s| s.to_string())
                        })
                    }) {
                        Some(name) => name,
                        None if item_type == "custom_tool_call_output" => {
                            tracing::warn!(
                                "Skipping orphan custom_tool_call_output for unknown call_id {}",
                                call_id
                            );
                            continue;
                        }
                        None => "shell".to_string(),
                    };

                    if name == "apply_patch" {
                        output_str = compact_apply_patch_failure_output(
                            output_str,
                            &mut seen_apply_patch_failures,
                            &mut apply_patch_failure_distinct_count,
                        );
                    }
                    output_str = prefix_with_step_marker(step_marker, output_str);

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
        if let Some(ledger) = interaction_ledger {
            obj.insert("_interaction_ledger".to_string(), json!(ledger));
        }
        obj.remove("instructions");
    }
    body
}

struct TranslationState {
    response_id: String,
    item_id: String,
    message_output_index: Option<u32>,
    next_output_index: u32,
    tool_output_indices: std::collections::HashMap<u32, u32>,
    message_item_added: bool,
    content_part_added: bool,
    accumulated_text: String,
    tool_calls: std::collections::HashMap<u32, (String, String, String, String)>,
    tool_calls_added: std::collections::HashSet<u32>,
}

async fn send_ws_event(socket: &mut WebSocket, ws_events: &mut Vec<Value>, event: &Value) {
    ws_events.push(event.clone());
    let _ = socket.send(Message::Text(event.to_string())).await;
}

async fn translate_openai_chunk_to_ws(
    chunk: &Value,
    state: &mut TranslationState,
    socket: &mut WebSocket,
    ws_events: &mut Vec<Value>,
) {
    if let Some(choices) = chunk.get("choices").and_then(|c| c.as_array()) {
        for choice in choices {
            if let Some(delta) = choice.get("delta") {
                if let Some(reasoning) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                    if !reasoning.is_empty() {
                        let message_output_index = match state.message_output_index {
                            Some(idx) => idx,
                            None => {
                                let idx = state.next_output_index;
                                state.next_output_index += 1;
                                state.message_output_index = Some(idx);
                                idx
                            }
                        };
                        let reasoning_ev = json!({
                            "type": "response.reasoning_summary_text.delta",
                            "sequence_number": 0,
                            "item_id": &state.item_id,
                            "output_index": message_output_index,
                            "summary_index": 0,
                            "delta": reasoning
                        });
                        send_ws_event(socket, ws_events, &reasoning_ev).await;

                        if !state.message_item_added {
                            let item_added = json!({
                                "type": "response.output_item.added",
                                "output_index": message_output_index,
                                "item": {
                                    "id": &state.item_id,
                                    "type": "message",
                                    "role": "assistant",
                                    "phase": "commentary",
                                    "status": "in_progress",
                                    "content": []
                                }
                            });
                            send_ws_event(socket, ws_events, &item_added).await;

                            let part_added = json!({
                                "type": "response.content_part.added",
                                "item_id": &state.item_id,
                                "output_index": message_output_index,
                                "content_index": 0,
                                "part": {
                                    "type": "output_text",
                                    "text": ""
                                }
                            });
                            send_ws_event(socket, ws_events, &part_added).await;
                            state.message_item_added = true;
                            state.content_part_added = true;
                        }

                        let delta_ev = json!({
                            "type": "response.output_text.delta",
                            "item_id": &state.item_id,
                            "output_index": message_output_index,
                            "content_index": 0,
                            "delta": reasoning
                        });
                        send_ws_event(socket, ws_events, &delta_ev).await;
                        state.accumulated_text.push_str(reasoning);
                    }
                }

                if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                    if !content.is_empty() {
                        let message_output_index = match state.message_output_index {
                            Some(idx) => idx,
                            None => {
                                let idx = state.next_output_index;
                                state.next_output_index += 1;
                                state.message_output_index = Some(idx);
                                idx
                            }
                        };
                        if !state.message_item_added {
                            let item_added = json!({
                                "type": "response.output_item.added",
                                "output_index": message_output_index,
                                "item": {
                                    "id": &state.item_id,
                                    "type": "message",
                                    "role": "assistant",
                                    "phase": "commentary",
                                    "status": "in_progress",
                                    "content": []
                                }
                            });
                            send_ws_event(socket, ws_events, &item_added).await;

                            let part_added = json!({
                                "type": "response.content_part.added",
                                "item_id": &state.item_id,
                                "output_index": message_output_index,
                                "content_index": 0,
                                "part": {
                                    "type": "output_text",
                                    "text": ""
                                }
                            });
                            send_ws_event(socket, ws_events, &part_added).await;
                            state.message_item_added = true;
                            state.content_part_added = true;
                        }

                        let delta_ev = json!({
                            "type": "response.output_text.delta",
                            "item_id": &state.item_id,
                            "output_index": message_output_index,
                            "content_index": 0,
                            "delta": content
                        });
                        send_ws_event(socket, ws_events, &delta_ev).await;
                        state.accumulated_text.push_str(content);
                    }
                }

                if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tool_calls {
                        let tc_idx = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let tc_id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let tc_name = tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let tc_args = tc
                            .get("function")
                            .and_then(|f| f.get("arguments"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        if !tc_id.is_empty() || !tc_name.is_empty() {
                            let tool_item_id =
                                format!("item-{}", &Uuid::new_v4().to_string()[..16]);
                            let call_id = if tc_id.is_empty() {
                                format!("call_{}", &Uuid::new_v4().to_string()[..16])
                            } else {
                                tc_id.to_string()
                            };
                            state.tool_calls.insert(
                                tc_idx,
                                (
                                    tool_item_id,
                                    call_id.clone(),
                                    tc_name.to_string(),
                                    String::new(),
                                ),
                            );
                            if !tc_name.is_empty() {
                                // 临时插入一个包含 name 的 Value，最终会被 finalize_ws_events 里的完整 Value 覆盖
                                insert_cached_tool_call(call_id, json!({ "name": tc_name }));
                            }
                        }

                        if let Some((tool_item_id, call_id, name, args)) =
                            state.tool_calls.get_mut(&tc_idx)
                        {
                            args.push_str(tc_args);
                            let tool_output_index = match state.tool_output_indices.get(&tc_idx) {
                                Some(idx) => *idx,
                                None => {
                                    let idx = state.next_output_index;
                                    state.next_output_index += 1;
                                    state.tool_output_indices.insert(tc_idx, idx);
                                    idx
                                }
                            };

                            if !state.tool_calls_added.contains(&tc_idx) {
                                let (actual_name, namespace) = split_namespace_tool_name(name);
                                let mut item_obj = serde_json::json!({
                                    "id": tool_item_id,
                                    "type": "function_call",
                                    "status": "in_progress",
                                    "name": actual_name,
                                    "call_id": call_id,
                                    "arguments": ""
                                });
                                if let Some(ns) = namespace {
                                    item_obj["namespace"] = json!(ns);
                                }
                                let tool_added = json!({
                                    "type": "response.output_item.added",
                                    "output_index": tool_output_index,
                                    "item": item_obj
                                });
                                send_ws_event(socket, ws_events, &tool_added).await;
                                state.tool_calls_added.insert(tc_idx);
                            }

                            if !tc_args.is_empty() {
                                let args_delta = json!({
                                    "type": "response.function_call_arguments.delta",
                                    "item_id": tool_item_id,
                                    "output_index": tool_output_index,
                                    "delta": tc_args
                                });
                                send_ws_event(socket, ws_events, &args_delta).await;
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn finalize_ws_events(
    state: &mut TranslationState,
    socket: &mut WebSocket,
    session_state: &mut WebsocketSessionState,
    ws_events: &mut Vec<Value>,
) -> Value {
    let mut output_items = Vec::new();
    let mut tool_keys: Vec<u32> = state.tool_calls.keys().cloned().collect();
    tool_keys.sort();

    for tc_idx in tool_keys {
        if let Some((tool_item_id, call_id, name, args)) = state.tool_calls.get(&tc_idx) {
            let tool_output_index = match state.tool_output_indices.get(&tc_idx) {
                Some(idx) => *idx,
                None => {
                    let idx = state.next_output_index;
                    state.next_output_index += 1;
                    state.tool_output_indices.insert(tc_idx, idx);
                    idx
                }
            };
            let args_done = json!({
                "type": "response.function_call_arguments.done",
                "item_id": tool_item_id,
                "output_index": tool_output_index,
                "arguments": args
            });
            send_ws_event(socket, ws_events, &args_done).await;

            let (actual_name, namespace) = split_namespace_tool_name(name);
            let mut item_obj = serde_json::json!({
                "id": tool_item_id,
                "type": "function_call",
                "status": "completed",
                "name": actual_name,
                "call_id": call_id,
                "arguments": args
            });
            if let Some(ns) = namespace {
                item_obj["namespace"] = json!(ns);
            }

            let tool_done = json!({
                "type": "response.output_item.done",
                "output_index": tool_output_index,
                "item": item_obj
            });
            send_ws_event(socket, ws_events, &tool_done).await;

            let tc_val = item_obj.clone();

            session_state
                .tool_call_cache
                .insert(call_id.clone(), tc_val.clone());
            insert_cached_tool_call(call_id.clone(), tc_val.clone());
            output_items.push(tc_val);
        }
    }

    if state.message_item_added {
        let message_output_index = state.message_output_index.unwrap_or(0);
        let text_done = json!({
            "type": "response.output_text.done",
            "item_id": &state.item_id,
            "output_index": message_output_index,
            "content_index": 0,
            "text": &state.accumulated_text
        });
        send_ws_event(socket, ws_events, &text_done).await;

        let part_done = json!({
            "type": "response.content_part.done",
            "item_id": &state.item_id,
            "output_index": message_output_index,
            "content_index": 0,
            "part": {
                "type": "output_text",
                "text": &state.accumulated_text
            }
        });
        send_ws_event(socket, ws_events, &part_done).await;

        let message_done = json!({
            "type": "response.output_item.done",
            "output_index": message_output_index,
            "item": {
                "id": &state.item_id,
                "type": "message",
                "role": "assistant",
                "phase": "final_answer",
                "status": "completed",
                "content": [{
                    "type": "output_text",
                    "text": &state.accumulated_text
                }]
            }
        });
        send_ws_event(socket, ws_events, &message_done).await;

        output_items.push(json!({
            "id": &state.item_id,
            "type": "message",
            "role": "assistant",
            "phase": "final_answer",
            "status": "completed",
            "content": [{
                "type": "output_text",
                "text": &state.accumulated_text
            }]
        }));
    }

    let completed_ev = json!({
        "type": "response.completed",
        "response": {
            "id": &state.response_id,
            "object": "response",
            "status": "completed",
            "output": output_items
        }
    });
    send_ws_event(socket, ws_events, &completed_ev).await;

    json!(output_items)
}

fn split_namespace_tool_name(qualified_name: &str) -> (String, Option<String>) {
    let name = qualified_name.trim();
    if name.starts_with("mcp__") {
        return (name.to_string(), None);
    }
    if let Some(pos) = name.find("__") {
        if pos > 0 {
            let namespace = name[..pos].to_string();
            let actual_name = name[pos + 2..].to_string();
            return (actual_name, Some(namespace));
        }
    }
    (name.to_string(), None)
}

const INTERNAL_BACKGROUND_TASK: &str = "gemini-2.5-flash-lite";
const CONTEXT_SUMMARY_PROMPT: &str = r#"You are a context compression specialist. Your task is to create a structured XML snapshot of the conversation history.

This snapshot will become the Agent's ONLY memory of the past. All key details, plans, errors, and user instructions MUST be preserved.

First, think through the entire history in a private <scratchpad>. Review the user's overall goal, the agent's actions, tool outputs, file modifications, and any unresolved issues. Identify every piece of information critical for future actions.

After reasoning, generate the final <state_snapshot> XML object. Information must be extremely dense. Omit any irrelevant conversational filler.

The structure MUST be as follows:

<state_snapshot>
  <overall_goal>
    <!-- Describe the user's high-level goal in one concise sentence -->
  </overall_goal>

  <technical_context>
    <!-- Tech stack: frameworks, languages, toolchain, dependency versions -->
  </technical_context>

  <file_system_state>
    <!-- List files that were created, read, modified, or deleted. Note their status -->
  </file_system_state>

  <code_changes>
    <!-- Key code snippets (preserve function signatures and important logic) -->
  </code_changes>

  <debugging_history>
    <!-- List all errors encountered, with stack traces, and how they were fixed -->
  </debugging_history>

  <current_plan>
    <!-- Step-by-step plan. Mark completed steps -->
  </current_plan>

  <user_preferences>
    <!-- User's work preferences for this project (test commands, code style, etc.) -->
  </user_preferences>

  <key_decisions>
    <!-- Critical architectural decisions and design choices -->
  </key_decisions>

  <latest_thinking_signature>
    <!-- [CRITICAL] Preserve the last valid thinking signature -->
    <!-- Format: base64-encoded signature string -->
    <!-- This MUST be copied exactly as-is, no modifications -->
  </latest_thinking_signature>
</state_snapshot>

**IMPORTANT**:
1. Code snippets must be complete, including function signatures and key logic
2. Error messages must be preserved verbatim, including line numbers and stacks
3. File paths must use absolute paths
4. The thinking signature must be copied exactly, no modifications
"#;

async fn call_openai_gemini_sync(
    model: &str,
    request: &OpenAIRequest,
    token_manager: &std::sync::Arc<crate::proxy::TokenManager>,
    trace_id: &str,
) -> Result<String, String> {
    let (access_token, project_id, _, account_id, _wait_ms) = token_manager
        .get_token("gemini", false, None, model)
        .await
        .map_err(|e| format!("Failed to get account: {}", e))?;

    let token_obj = token_manager.get_token_by_id(&account_id);
    let session_id = format!("bg_sid_{}", chrono::Utc::now().timestamp_subsec_millis());
    let (gemini_body, _, _, _) =
        transform_openai_request(request, &project_id, &session_id, token_obj.as_ref());

    let upstream_url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
        model
    );

    debug!("[{}] [OpenAI-BG] Calling Gemini API: {}", trace_id, model);

    let response = reqwest::Client::new()
        .post(&upstream_url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .json(&gemini_body)
        .send()
        .await
        .map_err(|e| format!("API call failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "API returned {}: {}",
            response.status(),
            response.text().await.unwrap_or_default()
        ));
    }

    let gemini_response: Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    gemini_response
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Failed to extract text from response".to_string())
}

async fn try_compress_openai_with_summary(
    original_request: &OpenAIRequest,
    trace_id: &str,
    token_manager: &std::sync::Arc<crate::proxy::TokenManager>,
    session_id_str: &str,
) -> Result<OpenAIRequest, String> {
    info!(
        "[{}] [Layer-3] [OpenAI] Starting context compression with XML summary",
        trace_id
    );

    let last_signature =
        crate::proxy::mappers::context_manager::ContextManager::extract_last_openai_valid_signature(
            session_id_str,
        );

    if let Some(ref sig) = last_signature {
        debug!(
            "[{}] [Layer-3] [OpenAI] Extracted signature (len: {})",
            trace_id,
            sig.len()
        );
    }

    let mut summary_messages = original_request.messages.clone();

    let signature_instruction = if let Some(ref sig) = last_signature {
        format!("\n\n**CRITICAL**: The last thinking signature is:\n```\n{}\n```\nYou MUST include this EXACTLY in the <latest_thinking_signature> section.", sig)
    } else {
        "\n\n**Note**: No thinking signature found in history. Leave <latest_thinking_signature> empty.".to_string()
    };

    summary_messages.push(OpenAIMessage {
        role: "user".to_string(),
        content: Some(
            crate::proxy::mappers::openai::models::OpenAIContent::String(format!(
                "{}{}",
                CONTEXT_SUMMARY_PROMPT, signature_instruction
            )),
        ),
        refusal: None,
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });

    let mut summary_request = original_request.clone();
    summary_request.messages = summary_messages;
    summary_request.model = INTERNAL_BACKGROUND_TASK.to_string();
    summary_request.stream = false;
    summary_request.max_tokens = Some(8000);
    summary_request.temperature = Some(0.3);

    debug!(
        "[{}] [Layer-3] [OpenAI] Calling {} for summary generation",
        trace_id, INTERNAL_BACKGROUND_TASK
    );

    let xml_summary = call_openai_gemini_sync(
        INTERNAL_BACKGROUND_TASK,
        &summary_request,
        token_manager,
        trace_id,
    )
    .await?;

    info!(
        "[{}] [Layer-3] [OpenAI] Generated XML summary (len: {} chars)",
        trace_id,
        xml_summary.len()
    );

    let mut forked_messages = vec![
        OpenAIMessage {
            role: "user".to_string(),
            content: Some(crate::proxy::mappers::openai::models::OpenAIContent::String(format!(
                "Context has been compressed. Here is the structured summary of our conversation history:\n\n{}",
                xml_summary
            ))),
            refusal: None,
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
        OpenAIMessage {
            role: "assistant".to_string(),
            content: Some(crate::proxy::mappers::openai::models::OpenAIContent::String(
                "I have reviewed the compressed context summary. I understand the current state and will continue from here.".to_string()
            )),
            refusal: None,
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ];

    if let Some(last_msg) = original_request.messages.last() {
        if last_msg.role == "user" {
            if !matches!(&last_msg.content, Some(crate::proxy::mappers::openai::models::OpenAIContent::String(s)) if s.contains(CONTEXT_SUMMARY_PROMPT))
            {
                forked_messages.push(last_msg.clone());
            }
        }
    }

    let mut forked_request = original_request.clone();
    forked_request.messages = forked_messages;
    Ok(forked_request)
}
