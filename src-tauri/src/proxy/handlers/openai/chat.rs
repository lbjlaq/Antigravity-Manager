// OpenAI Chat Completions Handler
// POST /v1/chat/completions

use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use bytes::Bytes;
use serde_json::{json, Value};
use tracing::{debug, error, info};

use crate::proxy::debug_logger;
use crate::proxy::mappers::openai::{
    transform_openai_request, transform_openai_response, OpenAIRequest,
};
use crate::proxy::server::AppState;
use crate::proxy::session_manager::SessionManager;
use super::super::common::{
    apply_retry_strategy, determine_retry_strategy, should_rotate_account, RetryStrategy,
};
use tokio::time::Duration;

const MAX_RETRY_ATTEMPTS: usize = 3;

pub async fn handle_chat_completions(
    State(state): State<AppState>,
    Json(mut body): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Save original request body for logging
    let original_body = body.clone();

    // Auto-detect and convert Responses format
    let is_responses_format = !body.get("messages").is_some()
        && (body.get("instructions").is_some() || body.get("input").is_some());

    if is_responses_format {
        debug!("Detected Responses API format, converting to Chat Completions format");

        // Convert instructions to system message
        if let Some(instructions) = body.get("instructions").and_then(|v| v.as_str()) {
            if !instructions.is_empty() {
                let system_msg = json!({
                    "role": "system",
                    "content": instructions
                });

                if !body.get("messages").is_some() {
                    body["messages"] = json!([]);
                }

                if let Some(messages) = body.get_mut("messages").and_then(|v| v.as_array_mut()) {
                    messages.insert(0, system_msg);
                }
            }
        }

        // Convert input to user message
        if let Some(input) = body.get("input") {
            let user_msg = if input.is_string() {
                json!({
                    "role": "user",
                    "content": input.as_str().unwrap_or("")
                })
            } else {
                json!({
                    "role": "user",
                    "content": input.to_string()
                })
            };

            if let Some(messages) = body.get_mut("messages").and_then(|v| v.as_array_mut()) {
                messages.push(user_msg);
            }
        }
    }

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
            });
    }

    let trace_id = format!("req_{}", chrono::Utc::now().timestamp_subsec_millis());
    info!(
        "[{}] OpenAI Chat Request: {} | {} messages | stream: {}",
        trace_id, openai_req.model, openai_req.messages.len(), openai_req.stream
    );
    let debug_cfg = state.debug_logging.read().await.clone();
    if debug_logger::is_enabled(&debug_cfg) {
        let original_payload = json!({
            "kind": "original_request",
            "protocol": "openai",
            "trace_id": trace_id,
            "original_model": openai_req.model,
            "request": original_body,
        });
        debug_logger::write_debug_payload(&debug_cfg, Some(&trace_id), "original_request", &original_payload).await;
    }

    // 1. Get UpstreamClient
    let upstream = state.upstream.clone();
    let token_manager = state.token_manager;
    let pool_size = token_manager.len();
    let max_attempts = MAX_RETRY_ATTEMPTS.min(pool_size.saturating_add(1)).max(2);

    let mut last_error = String::new();
    let mut last_email: Option<String> = None;

    // 2. Model routing
    let mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
        &openai_req.model,
        &*state.custom_mapping.read().await,
    );

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

        // 3. Extract SessionId
        let session_id = SessionManager::extract_openai_session_id(&openai_req);

        // 4. Get Token
        let token_lease = match token_manager
            .get_token(
                &config.request_type,
                attempt > 0,
                Some(&session_id),
                &mapped_model,
            )
            .await
        {
            Ok(t) => t,
            Err(e) => {
                let headers = [("X-Mapped-Model", mapped_model.as_str())];
                return Ok((
                    StatusCode::SERVICE_UNAVAILABLE,
                    headers,
                    format!("Token error: {}", e),
                )
                    .into_response());
            }
        };

        let access_token = token_lease.access_token.clone();
        let project_id = token_lease.project_id.clone();
        let email = token_lease.email.clone();

        last_email = Some(email.clone());
        info!("✓ Using account: {} (type: {})", email, config.request_type);

        // 4. Transform request
        let gemini_body = transform_openai_request(&openai_req, &project_id, &mapped_model);

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
            debug_logger::write_debug_payload(&debug_cfg, Some(&trace_id), "v1internal_request", &payload).await;
        }

        if let Ok(body_json) = serde_json::to_string_pretty(&gemini_body) {
            debug!("[OpenAI-Request] Transformed Gemini Body:\n{}", body_json);
        }

        // 5. Send request
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

        let response = match upstream
            .call_v1_internal(method, &access_token, gemini_body, query_string)
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

        let status = response.status();
        if status.is_success() {
            token_manager.mark_account_success(&email, Some(&mapped_model));

            if actual_stream {
                use crate::proxy::mappers::openai::streaming::create_openai_sse_stream;
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
                });
                let gemini_stream = debug_logger::wrap_reqwest_stream_with_debug(
                    Box::pin(response.bytes_stream()),
                    debug_cfg.clone(),
                    trace_id.clone(),
                    "upstream_response",
                    meta,
                );

                let mut openai_stream =
                    create_openai_sse_stream(
                        gemini_stream,
                        openai_req.model.clone(),
                        session_id.clone(),
                        openai_req.messages.len(),
                    );

                let mut first_data_chunk = None;
                let mut retry_this_account = false;

                // Peek loop to skip heartbeats
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
                            if text.trim().starts_with(":") || text.trim().starts_with("data: :") {
                                tracing::debug!("[OpenAI] Skipping peek heartbeat");
                                continue;
                            }

                            if text.contains("\"error\"") {
                                tracing::warn!("[OpenAI] Error detected during peek, retrying...");
                                last_error = "Error event during peek".to_string();
                                retry_this_account = true;
                                break;
                            }

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
                            tracing::warn!(
                                "[OpenAI] Timeout waiting for first data (60s), retrying..."
                            );
                            last_error = "Timeout waiting for first data".to_string();
                            retry_this_account = true;
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            break;
                        }
                    }
                }

                if retry_this_account {
                    continue;
                }

                let combined_stream =
                    futures::stream::once(
                        async move { Ok::<Bytes, String>(first_data_chunk.unwrap()) },
                    )
                    .chain(openai_stream);

                if client_wants_stream {
                    let body = Body::from_stream(combined_stream);
                    return Ok(Response::builder()
                        .header("Content-Type", "text/event-stream")
                        .header("Cache-Control", "no-cache")
                        .header("Connection", "keep-alive")
                        .header("X-Accel-Buffering", "no")
                        .header("X-Account-Email", &email)
                        .header("X-Mapped-Model", &mapped_model)
                        .body(body)
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to build value: {}", e)))?
                        .into_response());
                } else {
                    use crate::proxy::mappers::openai::collector::collect_stream_to_json;

                    match collect_stream_to_json(Box::pin(combined_stream)).await {
                        Ok(full_response) => {
                            info!("[{}] ✓ Stream collected and converted to JSON", trace_id);
                            crate::proxy::SignatureCache::global()
                                .delete_session_signature(&session_id);
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

            let openai_response = transform_openai_response(&gemini_resp);
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

        // Handle errors and retry
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
                "error_text": error_text,
            });
            debug_logger::write_debug_payload(&debug_cfg, Some(&trace_id), "upstream_response_error", &payload).await;
        }

        // Mark rate limited status
        if status_code == 429 || status_code == 529 || status_code == 503 || status_code == 500 {
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

        // Circuit Breaker Reporting
        if status_code == 402 || status_code == 429 || status_code == 401 {
             token_manager.report_account_failure(&token_lease.account_id, status_code, &error_text);
        }

        // Handle 400 error (Thinking signature failure)
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

            continue;
        }

        // 403/401 trigger account rotation
        if status_code == 403 || status_code == 401 {
            if status_code == 403 {
                // Refined 403 classification
                if let Some(acc_id) = token_manager.get_account_id_by_email(&email) {
                    if is_validation_required_error(&error_text) {
                        tracing::warn!(
                            "[OpenAI] VALIDATION_REQUIRED detected on account {}, temporarily blocking",
                            email
                        );
                        let block_minutes = 10i64;
                        let block_until = chrono::Utc::now().timestamp() + (block_minutes * 60);
                        
                        if let Err(e) = token_manager.set_validation_block_public(&acc_id, block_until, &error_text).await {
                            tracing::error!("Failed to set validation block: {}", e);
                        }
                    } else if is_permanent_forbidden_error(&error_text) {
                        tracing::warn!(
                            "[OpenAI] Permanent 403 detected on account {}, marking as forbidden",
                            email
                        );
                        if let Err(e) = token_manager.set_forbidden(&acc_id, &error_text).await {
                            tracing::error!("Failed to set forbidden status: {}", e);
                        }
                    } else {
                        tracing::warn!(
                            "[OpenAI] Transient/unknown 403 on account {}, rotating without permanent forbid",
                            email
                        );
                    }
                }
            }

            if attempt + 1 < max_attempts {
                let _ = apply_retry_strategy(
                RetryStrategy::FixedDelay(Duration::from_millis(200)),
                attempt,
                max_attempts,
                status_code,
                &trace_id,
            )
                .await;
                continue;
            }
        }

        let strategy = determine_retry_strategy(status_code, &error_text, false);
        if attempt + 1 < max_attempts
            && apply_retry_strategy(strategy, attempt, max_attempts, status_code, &trace_id).await
        {
            if !should_rotate_account(status_code) {
                debug!(
                    "[{}] Keeping same account for status {} (server-side issue)",
                    trace_id, status_code
                );
            }

            tracing::warn!(
                "OpenAI Upstream {} on {} attempt {}/{}, rotating account",
                status_code,
                email,
                attempt + 1,
                max_attempts
            );
            continue;
        }

        // Non-retryable error
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
            error_text,
        )
            .into_response());
    }

    // All attempts failed
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

fn is_validation_required_error(error_text: &str) -> bool {
    let lower = error_text.to_ascii_lowercase();
    lower.contains("validation_required")
        || lower.contains("verify your account")
        || lower.contains("validation_url")
}

fn is_permanent_forbidden_error(error_text: &str) -> bool {
    let lower = error_text.to_ascii_lowercase();

    // Account-level hard failures (safe to mark forbidden)
    lower.contains("account disabled")
        || lower.contains("account suspended")
        || lower.contains("account has been blocked")
        || lower.contains("account banned")
        || (lower.contains("policy") && lower.contains("violation"))
}
