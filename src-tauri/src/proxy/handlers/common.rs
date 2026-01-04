use axum::{extract::State, extract::Json, http::StatusCode, response::IntoResponse, response::Response};
use serde_json::{json, Value};
use crate::proxy::server::AppState;
use crate::proxy::middleware::monitor::X_RESOLVED_MODEL_HEADER;

/// Helper trait to attach resolved model info to responses for monitoring
pub trait WithResolvedModel {
    fn with_resolved_model(self, model: &str) -> Response;
}

impl<T: IntoResponse> WithResolvedModel for T {
    fn with_resolved_model(self, model: &str) -> Response {
        let mut response = self.into_response();
        if let Ok(header_value) = axum::http::HeaderValue::from_str(model) {
            response.headers_mut().insert(X_RESOLVED_MODEL_HEADER, header_value);
        }
        response
    }
}

/// Detects model capabilities and configuration
/// POST /v1/models/detect
pub async fn handle_detect_model(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let model_name = body.get("model").and_then(|v| v.as_str()).unwrap_or("");
    
    if model_name.is_empty() {
        return (StatusCode::BAD_REQUEST, "Missing 'model' field").into_response();
    }

    // 1. Resolve mapping
    let mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
        model_name,
        &*state.custom_mapping.read().await,
        &*state.openai_mapping.read().await,
        &*state.anthropic_mapping.read().await,
        false,  // Common 请求不应用 Claude 家族映射
    );

    // 2. Resolve capabilities
    let config = crate::proxy::mappers::common_utils::resolve_request_config(
        model_name,
        &mapped_model,
        &None // We don't check tools for static capability detection
    );

    // 3. Construct response
    let mut response = json!({
        "model": model_name,
        "mapped_model": mapped_model,
        "type": config.request_type,
        "features": {
            "has_web_search": config.inject_google_search,
            "is_image_gen": config.request_type == "image_gen"
        }
    });

    if let Some(img_conf) = config.image_config {
        if let Some(obj) = response.as_object_mut() {
            obj.insert("config".to_string(), img_conf);
        }
    }

    Json(response).into_response()
}
