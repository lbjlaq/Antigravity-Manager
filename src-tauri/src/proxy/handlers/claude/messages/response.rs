//! Response building helpers for Claude messages handler.

use axum::{
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Build error response for invalid request.
pub fn build_invalid_request_error(message: String) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": message
            }
        })),
    )
        .into_response()
}

/// Build error response for service unavailable.
pub fn build_service_unavailable_error(message: String, mapped_model: &str) -> Response {
    let headers = [("X-Mapped-Model", mapped_model)];
    (
        StatusCode::SERVICE_UNAVAILABLE,
        headers,
        Json(json!({
            "type": "error",
            "error": {
                "type": "overloaded_error",
                "message": format!("No available accounts: {}", message)
            }
        })),
    )
        .into_response()
}

/// Build error response for transform error.
pub fn build_transform_error(message: String, model: &str, email: &str) -> Response {
    let headers = [("X-Mapped-Model", model), ("X-Account-Email", email)];
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        headers,
        Json(json!({
            "type": "error",
            "error": {
                "type": "api_error",
                "message": format!("Transform error: {}", message)
            }
        })),
    )
        .into_response()
}

/// Build error response for context too long.
pub fn build_context_too_long_error(email: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        [("X-Account-Email", email)],
        Json(json!({
            "id": "err_prompt_too_long",
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": "Prompt is too long (server-side context limit reached).",
                "suggestion": "Please: 1) Execute '/compact' in Claude Code 2) Reduce conversation history 3) Switch to gemini-1.5-pro (2M context limit)"
            }
        })),
    )
        .into_response()
}

/// Build error response for compression failure.
pub fn build_compression_failed_error(message: String) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": message,
                "suggestion": "Please use /compact or /clear command in Claude Code, or switch to a model with larger context window."
            }
        })),
    )
        .into_response()
}

/// Build final exhausted retry error response.
pub fn build_exhausted_retry_error(
    last_status: StatusCode,
    last_error: &str,
    max_attempts: usize,
    last_email: Option<&str>,
    last_mapped_model: Option<&str>,
) -> Response {
    let mut headers = HeaderMap::new();

    if let Some(email) = last_email {
        if let Ok(v) = header::HeaderValue::from_str(email) {
            headers.insert("X-Account-Email", v);
        }
    }
    if let Some(model) = last_mapped_model {
        if let Ok(v) = header::HeaderValue::from_str(model) {
            headers.insert("X-Mapped-Model", v);
        }
    }

    let error_type = match last_status.as_u16() {
        400 => "invalid_request_error",
        401 => "authentication_error",
        403 => "permission_error",
        429 => "rate_limit_error",
        529 => "overloaded_error",
        _ => "api_error",
    };

    (
        last_status,
        headers,
        Json(json!({
            "type": "error",
            "error": {
                "id": "err_retry_exhausted",
                "type": error_type,
                "message": format!("All {} attempts failed. Last status: {}. Error: {}", max_attempts, last_status, last_error)
            }
        })),
    )
        .into_response()
}

/// Extract error type from status code.
pub fn get_error_type_from_status(status_code: u16) -> &'static str {
    match status_code {
        400 => "invalid_request_error",
        401 => "authentication_error",
        403 => "permission_error",
        429 => "rate_limit_error",
        529 => "overloaded_error",
        _ => "api_error",
    }
}
