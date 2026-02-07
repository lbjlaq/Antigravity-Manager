//! Main Claude messages request handler. (Release v5.0.7)

use axum::{
    body::Body,
    extract::{Json, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};
use tracing::{debug, error, info};
use rand::Rng;

use super::compression::apply_progressive_compression;
use super::response::{
    build_compression_failed_error, build_context_too_long_error, build_exhausted_retry_error,
    build_invalid_request_error, build_service_unavailable_error, build_transform_error,
};
use super::retry::{get_thinking_retry_delay, handle_thinking_signature_error, is_context_too_long_error, is_thinking_signature_error};
use crate::proxy::config::DebugLoggingConfig;
use crate::proxy::debug_logger;
use crate::proxy::handlers::common::{apply_retry_strategy, determine_retry_strategy, should_rotate_account, RetryStrategy};
use crate::proxy::handlers::claude::background::{detect_background_task_type, select_background_model};
use crate::proxy::handlers::claude::warmup::{create_warmup_response, is_warmup_request};
use crate::proxy::mappers::claude::{
    clean_cache_control_from_messages, close_tool_loop_for_thinking, create_claude_sse_stream,
    filter_invalid_thinking_blocks_with_family, merge_consecutive_messages,
    transform_claude_request_in, transform_response,
};
use crate::proxy::mappers::context_manager::ContextManager;
use crate::proxy::server::AppState;
use axum::http::HeaderMap;
use std::sync::atomic::Ordering;

const MAX_RETRY_ATTEMPTS: usize = 3;

/// Result type for streaming response that can signal retry needed
enum StreamingResult {
    Success(Response),
    RetryNeeded(String), // Contains error message
}

/// Handle Claude messages request.
pub async fn handle_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let original_body = body.clone();

    tracing::debug!(
        "handle_messages called. Body JSON len: {}",
        body.to_string().len()
    );

    // Generate random Trace ID
    let trace_id: String = rand::Rng::sample_iter(rand::thread_rng(), &rand::distributions::Alphanumeric)
        .take(6)
        .map(char::from)
        .collect::<String>()
        .to_lowercase();
    let debug_cfg = state.debug_logging.read().await.clone();

    // Decide whether to use z.ai or Google flow
    let zai = state.zai.read().await.clone();
    let zai_enabled = zai.enabled && !matches!(zai.dispatch_mode, crate::proxy::ZaiDispatchMode::Off);
    let google_accounts = state.token_manager.len();

    // Parse request
    let mut request: crate::proxy::mappers::claude::models::ClaudeRequest = match serde_json::from_value(body) {
        Ok(r) => r,
        Err(e) => {
            return build_invalid_request_error(format!("Invalid request body: {}", e));
        }
    };

    if debug_logger::is_enabled(&debug_cfg) {
        let original_payload = json!({
            "kind": "original_request",
            "protocol": "anthropic",
            "trace_id": trace_id,
            "original_model": request.model,
            "request": original_body,
        });
        debug_logger::write_debug_payload(&debug_cfg, Some(&trace_id), "original_request", &original_payload).await;
    }

    // Normalize model name for quota protection check
    let normalized_model = crate::proxy::common::model_mapping::normalize_to_standard_id(&request.model)
        .unwrap_or_else(|| request.model.clone());

    let use_zai = determine_provider(&state, &zai, zai_enabled, google_accounts, &normalized_model, &trace_id, &request.model).await;

    // Clean cache_control and merge messages
    clean_cache_control_from_messages(&mut request.messages);
    merge_consecutive_messages(&mut request.messages);

    // Get model family for signature validation
    let target_family = if use_zai {
        Some("claude")
    } else {
        let mapped_model = crate::proxy::common::model_mapping::map_claude_model_to_gemini(&request.model);
        if mapped_model.contains("gemini") {
            Some("gemini")
        } else {
            Some("claude")
        }
    };

    // Filter invalid thinking blocks
    filter_invalid_thinking_blocks_with_family(&mut request.messages, target_family);

    // Recover from broken tool loops
    if state.experimental.read().await.enable_tool_loop_recovery {
        close_tool_loop_for_thinking(&mut request.messages);
    }

    // Intercept warmup requests
    if is_warmup_request(&request) {
        info!("[{}] Intercepting Warmup request, returning mock response", trace_id);
        return create_warmup_response(&request, request.stream);
    }

    if use_zai {
        return handle_zai_request(&state, &headers, &request, &trace_id).await;
    }

    // Google Flow
    handle_google_flow(state, request, trace_id, debug_cfg).await
}

async fn determine_provider(
    state: &AppState,
    zai: &crate::proxy::ZaiConfig,
    zai_enabled: bool,
    google_accounts: usize,
    normalized_model: &str,
    trace_id: &str,
    original_model: &str,
) -> bool {
    if !zai_enabled {
        return false;
    }

    match zai.dispatch_mode {
        crate::proxy::ZaiDispatchMode::Off => false,
        crate::proxy::ZaiDispatchMode::Exclusive => true,
        crate::proxy::ZaiDispatchMode::Fallback => {
            if google_accounts == 0 {
                info!("[{}] No Google accounts available, using fallback provider", trace_id);
                true
            } else {
                let has_available = state.token_manager.has_available_account("claude", normalized_model).await;
                if !has_available {
                    info!(
                        "[{}] All Google accounts unavailable for {}, using fallback provider",
                        trace_id, original_model
                    );
                }
                !has_available
            }
        }
        crate::proxy::ZaiDispatchMode::Pooled => {
            let total = google_accounts.saturating_add(1).max(1);
            let slot = state.provider_rr.fetch_add(1, Ordering::Relaxed) % total;
            slot == 0
        }
    }
}

async fn handle_zai_request(
    state: &AppState,
    headers: &HeaderMap,
    request: &crate::proxy::mappers::claude::models::ClaudeRequest,
    _trace_id: &str,
) -> Response {
    let new_body = match serde_json::to_value(request) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Failed to serialize fixed request for z.ai: {}", e);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    crate::proxy::providers::zai_anthropic::forward_anthropic_json(
        state,
        axum::http::Method::POST,
        "/v1/messages",
        headers,
        new_body,
        request.messages.len(),
    )
    .await
}

async fn handle_google_flow(
    state: AppState,
    request: crate::proxy::mappers::claude::models::ClaudeRequest,
    trace_id: String,
    debug_cfg: DebugLoggingConfig,
) -> Response {
    let experimental = state.experimental.read().await;
    let scaling_enabled = experimental.enable_usage_scaling;
    let threshold_l1 = experimental.context_compression_threshold_l1;
    let threshold_l2 = experimental.context_compression_threshold_l2;
    let threshold_l3 = experimental.context_compression_threshold_l3;
    drop(experimental);

    log_request_details(&request, &trace_id);

    let upstream = state.upstream.clone();
    let mut request_for_body = request.clone();
    let token_manager = state.token_manager.clone();

    let pool_size = token_manager.len();
    let max_attempts = MAX_RETRY_ATTEMPTS.min(pool_size.saturating_add(1)).max(2);

    let mut last_error = String::new();
    let mut retried_without_thinking = false;
    let mut last_email: Option<String> = None;
    let mut last_mapped_model: Option<String> = None;
    let mut last_status = StatusCode::SERVICE_UNAVAILABLE;

    for attempt in 0..max_attempts {
        let mut mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
            &request_for_body.model,
            &*state.custom_mapping.read().await,
        );
        last_mapped_model = Some(mapped_model.clone());

        let tools_val: Option<Vec<Value>> = request_for_body.tools.as_ref().map(|list| {
            list.iter()
                .map(|t| serde_json::to_value(t).unwrap_or(json!({})))
                .collect()
        });

        let config = crate::proxy::mappers::common_utils::resolve_request_config(
            &request_for_body.model,
            &mapped_model,
            &tools_val,
            request.size.as_deref(),
            request.quality.as_deref(),
        );

        let session_id_str = crate::proxy::session_manager::SessionManager::extract_session_id(&request_for_body);
        let session_id = Some(session_id_str.as_str());

        let force_rotate_token = attempt > 0;
        let mut token_lease_result = Err("Initial".to_string());
        // [FIX] Retry loop for token acquisition to handle transient pool exhaustion
        for token_attempt in 0..3 {
            token_lease_result = token_manager
                .get_token(&config.request_type, force_rotate_token, session_id, &config.final_model)
                .await;
            
            if token_lease_result.is_ok() {
                break;
            }
            if token_attempt < 2 {
                // [FIX] Quick jitter (100-500ms) for token acquisition races
                let delay = rand::thread_rng().gen_range(100..500);
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }
        }

        let token_lease = match token_lease_result {
            Ok(t) => t,
            Err(e) => {
                let safe_message = if e.contains("invalid_grant") {
                    "OAuth refresh failed (invalid_grant): refresh_token likely revoked/expired".to_string()
                } else {
                    e
                };
                return build_service_unavailable_error(safe_message, &mapped_model);
            }
        };

        let access_token = token_lease.access_token.clone();
        let project_id = token_lease.project_id.clone();
        let email = token_lease.email.clone();

        last_email = Some(email.clone());
        info!("Using account: {} (type: {})", email, config.request_type);

        // Background task detection
        let background_task_type = detect_background_task_type(&request_for_body);
        let mut request_with_mapped = request_for_body.clone();

        if let Some(task_type) = background_task_type {
            let virtual_model_id = select_background_model(task_type);
            let resolved_model = crate::proxy::common::model_mapping::resolve_model_route(
                virtual_model_id,
                &*state.custom_mapping.read().await,
            );

            info!(
                "[{}][AUTO] Detected background task ({:?}), redirect: {} -> {}",
                trace_id, task_type, mapped_model, resolved_model
            );

            mapped_model = resolved_model.clone();
            request_with_mapped.model = resolved_model;
            request_with_mapped.tools = None;
            request_with_mapped.thinking = None;

            ContextManager::purify_history(
                &mut request_with_mapped.messages,
                crate::proxy::mappers::context_manager::PurificationStrategy::Aggressive,
            );
        }

        // Progressive compression
        let mut is_purified = false;
        let mut raw_estimated;

        if !retried_without_thinking && scaling_enabled {
            match apply_progressive_compression(
                request_with_mapped.clone(),
                &trace_id,
                &mapped_model,
                threshold_l1,
                threshold_l2,
                threshold_l3,
                &token_manager,
            )
            .await
            {
                Ok(result) => {
                    request_with_mapped = result.request;
                    is_purified = result.is_purified;
                    raw_estimated = if !is_purified {
                        ContextManager::estimate_token_usage(&request_with_mapped)
                    } else {
                        0
                    };
                }
                Err(e) => {
                    return build_compression_failed_error(e);
                }
            }
        } else {
            raw_estimated = ContextManager::estimate_token_usage(&request_with_mapped);
        }

        request_with_mapped.model = mapped_model.clone();

        let gemini_body = match transform_claude_request_in(&request_with_mapped, &project_id, retried_without_thinking) {
            Ok(b) => {
                debug!("[{}] Transformed Gemini Body: {}", trace_id, serde_json::to_string_pretty(&b).unwrap_or_default());
                b
            }
            Err(e) => {
                return build_transform_error(e, &request_with_mapped.model, &email);
            }
        };

        if debug_logger::is_enabled(&debug_cfg) {
            let payload = json!({
                "kind": "v1internal_request",
                "protocol": "anthropic",
                "trace_id": trace_id,
                "original_model": request.model,
                "mapped_model": request_with_mapped.model,
                "request_type": config.request_type,
                "attempt": attempt,
                "v1internal_request": gemini_body.clone(),
            });
            debug_logger::write_debug_payload(&debug_cfg, Some(&trace_id), "v1internal_request", &payload).await;
        }

        // Upstream call
        let client_wants_stream = request.stream;
        let force_stream_internally = !client_wants_stream;
        let actual_stream = client_wants_stream || force_stream_internally;

        if force_stream_internally {
            info!("[{}] Auto-converting non-stream request to stream", trace_id);
        }

        let method = if actual_stream { "streamGenerateContent" } else { "generateContent" };
        let query = if actual_stream { Some("alt=sse") } else { None };

        let mut extra_headers = std::collections::HashMap::new();
        if request_with_mapped.thinking.is_some() && request_with_mapped.tools.is_some() {
            extra_headers.insert("anthropic-beta".to_string(), "interleaved-thinking-2025-05-14".to_string());
        }

        let response = match upstream
            .call_v1_internal_with_headers(method, &access_token, gemini_body, query, extra_headers)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_error = e.clone();
                debug!("Request failed on attempt {}/{}: {}", attempt + 1, max_attempts, e);
                
                // [FIX] Medium jitter (1-3s) for network errors
                let delay = rand::thread_rng().gen_range(1000..3000);
                debug!("Network error, waiting {}ms...", delay);
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                
                continue;
            }
        };

        let status = response.status();
        last_status = status;

        if status.is_success() {
            token_manager.mark_account_success(&email, Some(&request_with_mapped.model));
            let context_limit = crate::proxy::mappers::claude::utils::get_context_limit_for_model(&request_with_mapped.model);

            if actual_stream {
                // [FIX] Handle streaming with retry capability
                match handle_streaming_response(
                    response,
                    &state,
                    &request,
                    &request_with_mapped,
                    &trace_id,
                    &email,
                    &session_id_str,
                    is_purified,
                    scaling_enabled,
                    context_limit,
                    raw_estimated,
                    debug_cfg.clone(),
                    client_wants_stream,
                    config.request_type.clone(),
                    attempt,
                )
                .await
                {
                    StreamingResult::Success(resp) => return resp,
                    StreamingResult::RetryNeeded(err) => {
                        last_error = err;
                        // [FIX] Medium-Heavy jitter (2-4s) for streaming interruptions
                        let delay = rand::thread_rng().gen_range(2000..4000);
                        debug!("Streaming retry needed, waiting {}ms...", delay);
                        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                        continue; // Retry with different account
                    }
                }
            } else {
                return handle_non_streaming_response(
                    response,
                    &request_with_mapped,
                    &trace_id,
                    &email,
                    session_id,
                    scaling_enabled,
                    context_limit,
                )
                .await;
            }
        }

        // Error handling
        let status_code = status.as_u16();
        last_status = status;

        // [FIX] Extract Retry-After header BEFORE consuming response body
        let retry_after = response.headers()
            .get("Retry-After")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        let error_text = response.text().await.unwrap_or_else(|_| format!("HTTP {}", status_code));
        last_error = format!("HTTP {}: {}", status_code, error_text);
        debug!("[{}] Upstream Error Response: {}", trace_id, error_text);

        // [FIX] Debug logging for upstream response errors
        if debug_logger::is_enabled(&debug_cfg) {
            let payload = json!({
                "kind": "upstream_response_error",
                "protocol": "anthropic",
                "trace_id": trace_id,
                "original_model": request.model,
                "mapped_model": request_with_mapped.model,
                "request_type": config.request_type,
                "attempt": attempt,
                "status": status_code,
                "error_text": error_text,
            });
            debug_logger::write_debug_payload(&debug_cfg, Some(&trace_id), "upstream_response_error", &payload).await;
        }

        // Handle rate limiting
        if status_code == 429 || status_code == 529 || status_code == 503 || status_code == 500 {
            token_manager.mark_rate_limited_async(&email, status_code, retry_after.as_deref(), &error_text, Some(&request_with_mapped.model)).await;
        }

        if status_code == 403 {
            if let Some(acc_id) = token_manager.get_account_id_by_email(&email) {
                if is_validation_required_error(&error_text) {
                    let block_minutes = crate::modules::config::load_app_config()
                        .map(|cfg| cfg.validation_block_minutes as i64)
                        .unwrap_or(10);
                    let block_until = chrono::Utc::now().timestamp() + (block_minutes * 60);

                    tracing::warn!(
                        "[{}] Claude VALIDATION_REQUIRED on {}, blocking for {} minutes",
                        trace_id,
                        email,
                        block_minutes
                    );

                    if let Err(e) = token_manager
                        .set_validation_block_public(&acc_id, block_until, &error_text)
                        .await
                    {
                        tracing::error!("Failed to set validation block for {}: {}", email, e);
                    }
                } else if is_permanent_forbidden_error(&error_text) {
                    tracing::warn!(
                        "[{}] Claude permanent 403 on {}, marking forbidden",
                        trace_id,
                        email
                    );

                    if let Err(e) = token_manager.set_forbidden(&acc_id, &error_text).await {
                        tracing::error!("Failed to set forbidden for {}: {}", email, e);
                    }
                } else {
                    tracing::warn!(
                        "[{}] Claude transient/unknown 403 on {}, rotate without permanent forbid",
                        trace_id,
                        email
                    );
                }
            }
        }

        // [FIX] Don't block account in Circuit Breaker for 429 (handled by Smart Rate Limiter)
        if status_code == 402 || status_code == 401 {
            token_manager.report_account_failure(&token_lease.account_id, status_code, &error_text);
        }

        // Handle thinking signature error
        if status_code == 400 && !retried_without_thinking && is_thinking_signature_error(&error_text) {
            retried_without_thinking = true;
            tracing::warn!("[{}] Thinking signature error, retrying without thinking blocks", trace_id);
            handle_thinking_signature_error(&mut request_for_body, &trace_id);

            if apply_retry_strategy(
                RetryStrategy::FixedDelay(get_thinking_retry_delay()),
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

        // Handle context too long
        if status_code == 400 && is_context_too_long_error(&error_text) {
            return build_context_too_long_error(&email);
        }

        let strategy = determine_retry_strategy(status_code, &error_text, retried_without_thinking);

        if apply_retry_strategy(strategy, attempt, max_attempts, status_code, &trace_id).await {
            if status_code == 429 {
                token_manager.report_429_penalty(&token_lease.account_id);
            }

            if !should_rotate_account(status_code) {
                debug!("[{}] Keeping same account for status {}", trace_id, status_code);
            }
            continue;
        } else {
            error!("[{}] Non-retryable error {}: {}", trace_id, status_code, error_text);
            return (status, [("X-Account-Email", email.as_str())], error_text).into_response();
        }
    }

    build_exhausted_retry_error(
        last_status,
        &last_error,
        max_attempts,
        last_email.as_deref(),
        last_mapped_model.as_deref(),
    )
}

fn is_validation_required_error(error_text: &str) -> bool {
    let lower = error_text.to_ascii_lowercase();
    lower.contains("validation_required")
        || lower.contains("verify your account")
        || lower.contains("validation_url")
}

fn is_permanent_forbidden_error(error_text: &str) -> bool {
    let lower = error_text.to_ascii_lowercase();
    lower.contains("account disabled")
        || lower.contains("account suspended")
        || lower.contains("account has been blocked")
        || lower.contains("account banned")
        || (lower.contains("policy") && lower.contains("violation"))
}

async fn handle_streaming_response(
    response: reqwest::Response,
    _state: &AppState,
    original_request: &crate::proxy::mappers::claude::models::ClaudeRequest,
    request_with_mapped: &crate::proxy::mappers::claude::models::ClaudeRequest,
    trace_id: &str,
    email: &str,
    session_id_str: &str,
    is_purified: bool,
    scaling_enabled: bool,
    context_limit: u32,
    raw_estimated: u32,
    debug_cfg: DebugLoggingConfig,
    client_wants_stream: bool,
    request_type: String,
    attempt: usize,
) -> StreamingResult {
    let meta = json!({
        "protocol": "anthropic",
        "trace_id": trace_id,
        "original_model": original_request.model,
        "mapped_model": request_with_mapped.model,
        "request_type": request_type,
        "attempt": attempt,
        "status": 200,
    });

    let gemini_stream = debug_logger::wrap_reqwest_stream_with_debug(
        Box::pin(response.bytes_stream()),
        debug_cfg,
        trace_id.to_string(),
        "upstream_response",
        meta,
    );

    let current_message_count = request_with_mapped.messages.len();

    let mut claude_stream = create_claude_sse_stream(
        gemini_stream,
        trace_id.to_string(),
        email.to_string(),
        Some(session_id_str.to_string()),
        scaling_enabled,
        context_limit,
        Some(raw_estimated),
        current_message_count,
    );

    // Peek first chunk
    let first_data_chunk = loop {
        match tokio::time::timeout(std::time::Duration::from_secs(60), claude_stream.next()).await {
            Ok(Some(Ok(bytes))) => {
                if bytes.is_empty() {
                    continue;
                }
                let text = String::from_utf8_lossy(&bytes);
                if text.trim().starts_with(':') {
                    debug!("[{}] Skipping peek heartbeat: {}", trace_id, text.trim());
                    continue;
                }
                break Some(bytes);
            }
            Ok(Some(Err(e))) => {
                // [FIX] Signal retry instead of returning 503
                tracing::warn!("[{}] Stream error during peek: {}, retrying...", trace_id, e);
                return StreamingResult::RetryNeeded(format!("Stream error during peek: {}", e));
            }
            Ok(None) => {
                // [FIX] Signal retry instead of returning 503
                tracing::warn!("[{}] Stream ended during peek (Empty Response), retrying...", trace_id);
                return StreamingResult::RetryNeeded("Empty response stream during peek".to_string());
            }
            Err(_) => {
                // [FIX] Signal retry instead of returning 503
                tracing::warn!("[{}] Timeout waiting for first data (60s), retrying...", trace_id);
                return StreamingResult::RetryNeeded("Timeout waiting for first data".to_string());
            }
        }
    };

    match first_data_chunk {
        Some(bytes) => {
            let combined_stream = Box::pin(
                futures::stream::once(async move { Ok(bytes) }).chain(claude_stream.map(
                    |result| -> Result<Bytes, std::io::Error> {
                        match result {
                            Ok(b) => Ok(b),
                            Err(e) => Ok(Bytes::from(format!("data: {{\"error\":\"{}\"}}\n\n", e))),
                        }
                    },
                )),
            );

            if client_wants_stream {
                StreamingResult::Success(
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "text/event-stream")
                        .header(header::CACHE_CONTROL, "no-cache")
                        .header(header::CONNECTION, "keep-alive")
                        .header("X-Accel-Buffering", "no")
                        .header("X-Account-Email", email)
                        .header("X-Mapped-Model", &request_with_mapped.model)
                        .header("X-Context-Purified", if is_purified { "true" } else { "false" })
                        .body(Body::from_stream(combined_stream))
                        .unwrap()
                )
            } else {
                use crate::proxy::mappers::claude::collect_stream_to_json;

                match collect_stream_to_json(combined_stream).await {
                    Ok(full_response) => {
                        info!("[{}] Stream collected and converted to JSON", trace_id);
                        StreamingResult::Success(
                            Response::builder()
                                .status(StatusCode::OK)
                                .header(header::CONTENT_TYPE, "application/json")
                                .header("X-Account-Email", email)
                                .header("X-Mapped-Model", &request_with_mapped.model)
                                .header("X-Context-Purified", if is_purified { "true" } else { "false" })
                                .body(Body::from(serde_json::to_string(&full_response).unwrap()))
                                .unwrap()
                        )
                    }
                    Err(e) => {
                        StreamingResult::Success(
                            (StatusCode::INTERNAL_SERVER_ERROR, format!("Stream collection error: {}", e)).into_response()
                        )
                    }
                }
            }
        }
        None => {
            // [FIX] Signal retry for empty stream instead of 503
            tracing::warn!("[{}] Stream ended immediately (Empty Response), retrying...", trace_id);
            StreamingResult::RetryNeeded("Empty response stream (None)".to_string())
        }
    }
}

async fn handle_non_streaming_response(
    response: reqwest::Response,
    request_with_mapped: &crate::proxy::mappers::claude::models::ClaudeRequest,
    trace_id: &str,
    email: &str,
    session_id: Option<&str>,
    scaling_enabled: bool,
    context_limit: u32,
) -> Response {
    let bytes = match response.bytes().await {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_GATEWAY, format!("Failed to read body: {}", e)).into_response(),
    };

    let gemini_resp: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_GATEWAY, format!("Parse error: {}", e)).into_response(),
    };

    let raw = gemini_resp.get("response").unwrap_or(&gemini_resp);

    let gemini_response: crate::proxy::mappers::claude::models::GeminiResponse = match serde_json::from_value(raw.clone()) {
        Ok(r) => r,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("Convert error: {}", e)).into_response(),
    };

    let s_id_owned = session_id.map(|s| s.to_string());

    let claude_response = match transform_response(
        &gemini_response,
        scaling_enabled,
        context_limit,
        s_id_owned,
        request_with_mapped.model.clone(),
        request_with_mapped.messages.len(),
    ) {
        Ok(r) => r,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("Transform error: {}", e)).into_response(),
    };

    info!(
        "[{}] Request finished. Model: {}, Tokens: In {}, Out {}",
        trace_id,
        request_with_mapped.model,
        claude_response.usage.input_tokens,
        claude_response.usage.output_tokens
    );

    (
        StatusCode::OK,
        [("X-Account-Email", email), ("X-Mapped-Model", request_with_mapped.model.as_str())],
        Json(claude_response),
    )
        .into_response()
}

fn log_request_details(request: &crate::proxy::mappers::claude::models::ClaudeRequest, trace_id: &str) {
    let meaningful_msg = request
        .messages
        .iter()
        .rev()
        .filter(|m| m.role == "user")
        .find_map(|m| {
            let content = match &m.content {
                crate::proxy::mappers::claude::models::MessageContent::String(s) => s.to_string(),
                crate::proxy::mappers::claude::models::MessageContent::Array(arr) => arr
                    .iter()
                    .filter_map(|block| match block {
                        crate::proxy::mappers::claude::models::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            };

            if content.trim().is_empty() || content.starts_with("Warmup") || content.contains("<system-reminder>") {
                None
            } else {
                Some(content)
            }
        });

    let latest_msg = meaningful_msg.unwrap_or_else(|| "[Complex/Tool Message]".to_string());

    info!(
        "[{}] Claude Request | Model: {} | Stream: {} | Messages: {} | Tools: {}",
        trace_id,
        request.model,
        request.stream,
        request.messages.len(),
        request.tools.is_some()
    );

    debug!("[{}] Content Preview: {:.100}...", trace_id, latest_msg);
}
