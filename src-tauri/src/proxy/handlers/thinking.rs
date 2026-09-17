use axum::{
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;

use crate::proxy::server::AppState;
use crate::proxy::thinking_store::{sanitize_session_id, SessionScope, ThinkingStore};

#[derive(Debug, Deserialize, Default)]
pub struct EndSessionBody {
    #[serde(default)]
    pub session_id: Option<String>,
}

fn json_with_session(
    status: StatusCode,
    client_id: &str,
    body: serde_json::Value,
) -> axum::response::Response {
    let mut headers = HeaderMap::new();
    if let Ok(v) = HeaderValue::from_str(client_id) {
        headers.insert("X-Session-Id", v);
    }
    (status, headers, Json(body)).into_response()
}

fn resolve_end_key(headers: &HeaderMap, body_sid: Option<&str>) -> Result<SessionScope, (StatusCode, String)> {
    let from_body = body_sid
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| sanitize_session_id(s));
    let fallback = from_body.unwrap_or_default();
    if fallback.is_empty()
        && headers.get("x-session-id").is_none()
        && headers.get("x-antigravity-session-id").is_none()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "session_id is required (body.session_id or X-Session-Id header)".to_string(),
        ));
    }
    Ok(SessionScope::from_headers(headers, fallback))
}

/// POST /v1/thinking/end
/// Body: { "session_id": "..." }  (or X-Session-Id header)
pub async fn handle_end_session(
    State(_state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<EndSessionBody>,
) -> impl IntoResponse {
    match resolve_end_key(&headers, body.session_id.as_deref()) {
        Ok(scope) => {
            let result = ThinkingStore::global().end_session(&scope.store_key);
            tracing::info!(
                "[ThinkingStore] Ended session {} (deleted {} turns, {} bytes)",
                scope.client_id,
                result.deleted_turns,
                result.deleted_bytes
            );
            json_with_session(
                StatusCode::OK,
                &scope.client_id,
                json!({
                    "ok": true,
                    "session_id": result.session_id,
                    "deleted_turns": result.deleted_turns,
                    "deleted_bytes": result.deleted_bytes
                }),
            )
        }
        Err((status, msg)) => (
            status,
            Json(json!({
                "ok": false,
                "error": { "message": msg }
            })),
        )
            .into_response(),
    }
}

/// DELETE /v1/thinking/sessions/:session_id
pub async fn handle_delete_session(
    State(_state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match resolve_end_key(&headers, Some(&session_id)) {
        Ok(scope) => {
            let result = ThinkingStore::global().end_session(&scope.store_key);
            json_with_session(
                StatusCode::OK,
                &scope.client_id,
                json!({
                    "ok": true,
                    "session_id": result.session_id,
                    "deleted_turns": result.deleted_turns,
                    "deleted_bytes": result.deleted_bytes
                }),
            )
        }
        Err((status, msg)) => (
            status,
            Json(json!({
                "ok": false,
                "error": { "message": msg }
            })),
        )
            .into_response(),
    }
}

/// GET /v1/thinking/sessions/:session_id
pub async fn handle_session_stats(
    State(_state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match resolve_end_key(&headers, Some(&session_id)) {
        Ok(scope) => {
            let (turns, bytes) = ThinkingStore::global()
                .session_stats(&scope.store_key)
                .unwrap_or((0, 0));
            json_with_session(
                StatusCode::OK,
                &scope.client_id,
                json!({
                    "ok": true,
                    "session_id": scope.client_id,
                    "turns": turns,
                    "bytes": bytes
                }),
            )
        }
        Err((status, msg)) => (
            status,
            Json(json!({
                "ok": false,
                "error": { "message": msg }
            })),
        )
            .into_response(),
    }
}
