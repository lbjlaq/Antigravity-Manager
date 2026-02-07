// OpenAI Images Handlers
// POST /v1/images/generations - Image generation
// POST /v1/images/edits - Image editing

use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use base64::Engine as _;
use serde_json::{json, Value};
use tracing::{info, warn};
use tokio::time::Duration;

use crate::proxy::server::AppState;
use super::super::common::{apply_retry_strategy, determine_retry_strategy, RetryStrategy};

const MAX_IMAGE_RETRY_ATTEMPTS: usize = 3;

fn extract_images_from_response(gemini_resp: &Value, response_format: &str) -> Vec<Value> {
    let raw = gemini_resp.get("response").unwrap_or(gemini_resp);
    let mut images: Vec<Value> = Vec::new();

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
                if data.is_empty() {
                    continue;
                }

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
            }
        }
    }

    images
}

async fn execute_image_request_with_retry(
    state: &AppState,
    request_body: &Value,
    mapped_model: &str,
    trace_id: &str,
) -> Result<(Value, String), String> {
    let token_manager = state.token_manager.clone();
    let upstream = state.upstream.clone();
    let pool_size = token_manager.len();
    let max_attempts = MAX_IMAGE_RETRY_ATTEMPTS.min(pool_size.saturating_add(1)).max(2);
    let mut last_error = String::new();

    for attempt in 0..max_attempts {
        let token_lease = token_manager
            .get_token("image_gen", attempt > 0, None, mapped_model)
            .await
            .map_err(|e| format!("Token error: {}", e))?;

        let email = token_lease.email.clone();
        let access_token = token_lease.access_token.clone();
        let project_id = token_lease.project_id.clone();

        let mut effective_body = request_body.clone();
        if let Some(obj) = effective_body.as_object_mut() {
            obj.insert("project".to_string(), Value::String(project_id));
        }

        let response = match upstream
            .call_v1_internal("generateContent", &access_token, effective_body, None)
            .await
        {
            Ok(response) => response,
            Err(e) => {
                last_error = format!("Network error: {}", e);
                if attempt + 1 < max_attempts {
                    warn!(
                        "[Images] Request failed on attempt {}/{}: {}",
                        attempt + 1,
                        max_attempts,
                        e
                    );
                    continue;
                }
                return Err(last_error);
            }
        };

        let status = response.status();
        if status.is_success() {
            let gemini_resp = response
                .json::<Value>()
                .await
                .map_err(|e| format!("Parse error: {}", e))?;
            token_manager.mark_account_success(&email, Some(mapped_model));
            return Ok((gemini_resp, email));
        }

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
        last_error = format!("Upstream error {}: {}", status_code, error_text);

        if matches!(status_code, 429 | 529 | 503 | 500) {
            token_manager
                .mark_rate_limited_async(
                    &email,
                    status_code,
                    retry_after.as_deref(),
                    &error_text,
                    Some(mapped_model),
                )
                .await;
        }

        if status_code == 401 || status_code == 403 {
            if attempt + 1 < max_attempts {
                let _ = apply_retry_strategy(
                    RetryStrategy::FixedDelay(Duration::from_millis(200)),
                    attempt,
                    max_attempts,
                    status_code,
                    trace_id,
                )
                .await;
                continue;
            }
            return Err(last_error);
        }

        let strategy = determine_retry_strategy(status_code, &error_text, false);
        if attempt + 1 < max_attempts
            && apply_retry_strategy(strategy, attempt, max_attempts, status_code, trace_id).await
        {
            continue;
        }

        return Err(last_error);
    }

    Err(format!("All image attempts failed: {}", last_error))
}

/// OpenAI Images API: POST /v1/images/generations
/// Handles image generation requests, converting to Gemini API format
pub async fn handle_images_generations(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // 1. Parse request parameters
    let prompt = body.get("prompt").and_then(|v| v.as_str()).ok_or((
        StatusCode::BAD_REQUEST,
        "Missing 'prompt' field".to_string(),
    ))?;

    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3-pro-image");

    let n = body.get("n").and_then(|v| v.as_u64()).unwrap_or(1) as usize;

    let size = body
        .get("size")
        .and_then(|v| v.as_str())
        .unwrap_or("1024x1024");

    let response_format = body
        .get("response_format")
        .and_then(|v| v.as_str())
        .unwrap_or("b64_json");

    let quality = body
        .get("quality")
        .and_then(|v| v.as_str())
        .unwrap_or("standard");
    let style = body
        .get("style")
        .and_then(|v| v.as_str())
        .unwrap_or("vivid");

    info!(
        "[Images] Received request: model={}, prompt={:.50}..., n={}, size={}, quality={}, style={}",
        model,
        prompt,
        n,
        size,
        quality,
        style
    );

    // 2. Parse image config using common_utils
    let (image_config, _) = crate::proxy::mappers::common_utils::parse_image_config_with_params(
        model,
        Some(size),
        Some(quality),
    );

    // 3. Prompt Enhancement
    let mut final_prompt = prompt.to_string();
    if quality == "hd" {
        final_prompt.push_str(", (high quality, highly detailed, 4k resolution, hdr)");
    }
    match style {
        "vivid" => final_prompt.push_str(", (vivid colors, dramatic lighting, rich details)"),
        "natural" => final_prompt.push_str(", (natural lighting, realistic, photorealistic)"),
        _ => {}
    }

    let trace_id = format!("img_gen_{}", chrono::Utc::now().timestamp_subsec_millis());

    let mut images: Vec<Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut response_email: Option<String> = None;

    for idx in 0..n {
        let gemini_body = json!({
            "project": "bamboo-precept-lgxtn",
            "requestId": format!("agent-{}", uuid::Uuid::new_v4()),
            "model": "gemini-3-pro-image",
            "userAgent": "antigravity",
            "requestType": "image_gen",
            "request": {
                "contents": [{
                    "role": "user",
                    "parts": [{"text": final_prompt.clone()}]
                }],
                "generationConfig": {
                    "candidateCount": 1,
                    "imageConfig": image_config.clone()
                },
                "safetySettings": [
                    { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "OFF" },
                    { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "OFF" },
                    { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "OFF" },
                    { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "OFF" },
                    { "category": "HARM_CATEGORY_CIVIC_INTEGRITY", "threshold": "OFF" }
                ]
            }
        });

        match execute_image_request_with_retry(&state, &gemini_body, "dall-e-3", &trace_id).await {
            Ok((gemini_resp, email)) => {
                if response_email.is_none() {
                    response_email = Some(email);
                }
                let extracted = extract_images_from_response(&gemini_resp, response_format);
                if extracted.is_empty() {
                    errors.push(format!("Task {}: No images generated", idx));
                } else {
                    images.extend(extracted);
                }
            }
            Err(e) => {
                tracing::error!("[Images] Task {} failed after retries: {}", idx, e);
                errors.push(format!("Task {}: {}", idx, e));
            }
        }
    }

    if images.is_empty() {
        let error_msg = if errors.is_empty() {
            "No images generated".to_string()
        } else {
            errors.join("; ")
        };
        tracing::error!("[Images] All {} requests failed. Errors: {}", n, error_msg);
        return Err((StatusCode::BAD_GATEWAY, error_msg));
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
        "[Images] Successfully generated {} out of {} requested image(s)",
        images.len(),
        n
    );

    // 7. Build OpenAI format response
    let openai_response = json!({
        "created": chrono::Utc::now().timestamp(),
        "data": images
    });

    let response_email = response_email.unwrap_or_else(|| "unknown".to_string());

    Ok((
        StatusCode::OK,
        [("X-Account-Email", response_email.as_str())],
        Json(openai_response),
    )
        .into_response())
}

/// OpenAI Images API: POST /v1/images/edits
/// Handles image editing requests with multipart form data
pub async fn handle_images_edits(
    State(state): State<AppState>,
    mut multipart: axum::extract::Multipart,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    tracing::info!("[Images] Received edit request");

    let mut image_data = None;
    let mut mask_data = None;
    let mut reference_images: Vec<String> = Vec::new();
    let mut prompt = String::new();
    let mut n = 1;
    let mut size = "1024x1024".to_string();
    let mut response_format = "b64_json".to_string();
    let mut model = "gemini-3-pro-image".to_string();
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
            let data = field
                .bytes()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("Image read error: {}", e)))?;
            image_data = Some(base64::engine::general_purpose::STANDARD.encode(data));
        } else if name == "mask" {
            let data = field
                .bytes()
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("Mask read error: {}", e)))?;
            mask_data = Some(base64::engine::general_purpose::STANDARD.encode(data));
        } else if name.starts_with("image") && name != "image_size" {
            // Support image1, image2, etc.
            let data = field.bytes().await.map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("Reference image read error: {}", e),
                )
            })?;
            reference_images.push(base64::engine::general_purpose::STANDARD.encode(data));
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

    // Validation
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

    // 1. Prepare Config
    let size_input = aspect_ratio.as_deref().or(Some(&size));

    let quality_input = match image_size_param.as_deref() {
        Some("4K") => Some("hd"),
        Some("2K") => Some("medium"),
        _ => None,
    };

    let (image_config, _) = crate::proxy::mappers::common_utils::parse_image_config_with_params(
        &model,
        size_input,
        quality_input,
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
    if let Some(data) = image_data {
        contents_parts.push(json!({
            "inlineData": {
                "mimeType": "image/png",
                "data": data
            }
        }));
    }

    // Add Mask (if standard edit)
    if let Some(data) = mask_data {
        contents_parts.push(json!({
            "inlineData": {
                "mimeType": "image/png",
                "data": data
            }
        }));
    }

    // Add Reference Images (Image-to-Image)
    for ref_data in reference_images {
        contents_parts.push(json!({
            "inlineData": {
                "mimeType": "image/jpeg",
                "data": ref_data
            }
        }));
    }

    // 4. Construct Request Body
    let gemini_body = json!({
        "project": "bamboo-precept-lgxtn",
        "requestId": format!("img-edit-{}", uuid::Uuid::new_v4()),
        "model": model,
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
                { "category": "HARM_CATEGORY_CIVIC_INTEGRITY", "threshold": "OFF" },
            ]
        }
    });

    let trace_id = format!("img_edit_{}", chrono::Utc::now().timestamp_subsec_millis());

    // 5. Execute Requests with retry/rotation parity
    let mut images: Vec<Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut response_email: Option<String> = None;

    for idx in 0..n {
        match execute_image_request_with_retry(&state, &gemini_body, "dall-e-3", &trace_id).await {
            Ok((gemini_resp, email)) => {
                if response_email.is_none() {
                    response_email = Some(email);
                }
                let extracted = extract_images_from_response(&gemini_resp, &response_format);
                if extracted.is_empty() {
                    errors.push(format!("Task {}: No images generated", idx));
                } else {
                    images.extend(extracted);
                }
            }
            Err(e) => {
                tracing::error!("[Images] Task {} failed after retries: {}", idx, e);
                errors.push(format!("Task {}: {}", idx, e));
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
        return Err((StatusCode::BAD_GATEWAY, error_msg));
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

    let response_email = response_email.unwrap_or_else(|| "unknown".to_string());

    Ok((
        StatusCode::OK,
        [("X-Account-Email", response_email.as_str())],
        Json(openai_response),
    )
        .into_response())
}
