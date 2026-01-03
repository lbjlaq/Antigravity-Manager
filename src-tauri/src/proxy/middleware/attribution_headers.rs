use axum::{
    extract::{Request, State},
    http::HeaderValue,
    middleware::Next,
    response::Response,
};

use crate::proxy::observability::RequestAttribution;
use crate::proxy::privacy::anonymize_id_ascii;
use crate::proxy::server::AppState;

const HDR_PROVIDER: &str = "x-antigravity-provider";
const HDR_MODEL: &str = "x-antigravity-model";
const HDR_ACCOUNT: &str = "x-antigravity-account";

pub async fn attribution_headers_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;

    let enabled = { *state.response_attribution_headers.read().await };
    if !enabled {
        return response;
    }

    let attr = match response.extensions().get::<RequestAttribution>() {
        Some(v) => v.clone(),
        None => return response,
    };

    if !response.headers().contains_key(HDR_PROVIDER) {
        if let Ok(v) = HeaderValue::from_str(&attr.provider) {
            response.headers_mut().insert(HDR_PROVIDER, v);
        }
    }

    if !response.headers().contains_key(HDR_MODEL) {
        if let Some(model) = attr.resolved_model.as_ref() {
            if let Ok(v) = HeaderValue::from_str(model) {
                response.headers_mut().insert(HDR_MODEL, v);
            }
        }
    }

    if !response.headers().contains_key(HDR_ACCOUNT) {
        if let Some(account_id) = attr.account_id.as_ref() {
            let v = anonymize_id_ascii(account_id);
            if let Ok(h) = HeaderValue::from_str(&v) {
                response.headers_mut().insert(HDR_ACCOUNT, h);
            }
        }
    }

    response
}
