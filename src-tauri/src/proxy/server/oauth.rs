//! OAuth callback handlers
//!
//! Handles OAuth callback processing and Web-based OAuth flow.

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, Json},
};
use tracing::error;

use crate::proxy::server::types::{AppState, ErrorResponse};

// ============================================================================
// OAuth Types
// ============================================================================

#[derive(serde::Deserialize)]
pub struct OAuthParams {
    pub code: String,
    pub state: Option<String>,
    #[allow(dead_code)]
    pub scope: Option<String>,
}

// ============================================================================
// OAuth Callback Handler
// ============================================================================

pub async fn handle_oauth_callback(
    Query(params): Query<OAuthParams>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Html<String>, StatusCode> {
    let code = params.code;

    // Exchange token
    let port = state.security.read().await.port;
    let host = headers.get("host").and_then(|h| h.to_str().ok());
    let proto = headers.get("x-forwarded-proto").and_then(|h| h.to_str().ok());
    let redirect_uri = get_oauth_redirect_uri(port, host, proto);

    match state.token_manager.exchange_code(&code, &redirect_uri).await {
        Ok(refresh_token) => {
            match state.token_manager.get_user_info(&refresh_token).await {
                Ok(user_info) => {
                    let email = user_info.email;
                    if let Err(e) = state.token_manager.add_account(&email, &refresh_token).await {
                        error!("Failed to add account: {}", e);
                        return Ok(Html(format!(
                            r#"<html><body><h1>Authorization Failed</h1><p>Failed to save account: {}</p></body></html>"#,
                            e
                        )));
                    }
                }
                Err(e) => {
                    error!("Failed to get user info: {}", e);
                    return Ok(Html(format!(
                        r#"<html><body><h1>Authorization Failed</h1><p>Failed to get user info: {}</p></body></html>"#,
                        e
                    )));
                }
            }

            // Success HTML
            Ok(Html(format!(r#"
                <!DOCTYPE html>
                <html>
                <head>
                    <title>Authorization Successful</title>
                    <style>
                        body {{ font-family: system-ui, -apple-system, sans-serif; display: flex; flex-direction: column; align-items: center; justify-content: center; min-height: 100vh; margin: 0; background-color: #f9fafb; padding: 20px; box-sizing: border-box; }}
                        .card {{ background: white; padding: 2rem; border-radius: 1.5rem; box-shadow: 0 10px 25px -5px rgb(0 0 0 / 0.1); text-align: center; max-width: 500px; width: 100%; }}
                        .icon {{ font-size: 3rem; margin-bottom: 1rem; }}
                        h1 {{ color: #059669; margin: 0 0 1rem 0; font-size: 1.5rem; }}
                        p {{ color: #4b5563; line-height: 1.5; margin-bottom: 1.5rem; }}
                        .fallback-box {{ background-color: #f3f4f6; padding: 1.25rem; border-radius: 1rem; border: 1px dashed #d1d5db; text-align: left; margin-top: 1.5rem; }}
                        .fallback-title {{ font-weight: 600; font-size: 0.875rem; color: #1f2937; margin-bottom: 0.5rem; display: block; }}
                        .fallback-text {{ font-size: 0.75rem; color: #6b7280; margin-bottom: 1rem; display: block; }}
                        .copy-btn {{ width: 100%; padding: 0.75rem; background-color: #3b82f6; color: white; border: none; border-radius: 0.75rem; font-weight: 500; cursor: pointer; transition: background-color 0.2s; }}
                        .copy-btn:hover {{ background-color: #2563eb; }}
                    </style>
                </head>
                <body>
                    <div class="card">
                        <div class="icon">OK</div>
                        <h1>Authorization Successful</h1>
                        <p>You can close this window now. The application should refresh automatically.</p>
                        
                        <div class="fallback-box">
                            <span class="fallback-title">Did it not refresh?</span>
                            <span class="fallback-text">If the application is running in a container or remote environment, you may need to manually copy the link below:</span>
                            <button onclick="copyUrl()" class="copy-btn" id="copyBtn">Copy Completion Link</button>
                        </div>
                    </div>
                    <script>
                        // 1. Notify opener if exists
                        if (window.opener) {{
                            window.opener.postMessage({{
                                type: 'oauth-success',
                                message: 'login success'
                            }}, '*');
                        }}

                        // 2. Copy URL functionality
                        function copyUrl() {{
                            navigator.clipboard.writeText(window.location.href).then(() => {{
                                const btn = document.getElementById('copyBtn');
                                const originalText = btn.innerText;
                                btn.innerText = 'Link Copied!';
                                btn.style.backgroundColor = '#059669';
                                setTimeout(() => {{
                                    btn.innerText = originalText;
                                    btn.style.backgroundColor = '#3b82f6';
                                }}, 2000);
                            }});
                        }}
                    </script>
                </body>
                </html>
            "#)))
        }
        Err(e) => {
            error!("OAuth exchange failed: {}", e);
            Ok(Html(format!(
                r#"<html><body><h1>Authorization Failed</h1><p>Error: {}</p></body></html>"#,
                e
            )))
        }
    }
}

// ============================================================================
// Web OAuth URL Preparation
// ============================================================================

pub async fn prepare_oauth_url_web(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let port = state.security.read().await.port;
    let host = headers.get("host").and_then(|h| h.to_str().ok());
    let proto = headers.get("x-forwarded-proto").and_then(|h| h.to_str().ok());
    let redirect_uri = get_oauth_redirect_uri(port, host, proto);

    let state_str = uuid::Uuid::new_v4().to_string();

    // Initialize OAuth flow state and background handler
    let (auth_url, mut code_rx) =
        crate::modules::oauth_server::prepare_oauth_flow_manually(redirect_uri.clone(), state_str.clone())
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse { error: e }),
                )
            })?;

    // Start background task to handle callback/manual code submission
    let token_manager = state.token_manager.clone();
    let redirect_uri_clone = redirect_uri.clone();
    tokio::spawn(async move {
        match code_rx.recv().await {
            Some(Ok(code)) => {
                crate::modules::logger::log_info(
                    "Consuming manually submitted OAuth code in background",
                );
                // Simplified backend flow for web callback
                match crate::modules::oauth::exchange_code(&code, &redirect_uri_clone).await {
                    Ok(token_resp) => {
                        // Success! Now add/upsert account
                        if let Some(refresh_token) = &token_resp.refresh_token {
                            match token_manager.get_user_info(refresh_token).await {
                                Ok(user_info) => {
                                    if let Err(e) = token_manager
                                        .add_account(&user_info.email, refresh_token)
                                        .await
                                    {
                                        crate::modules::logger::log_error(&format!(
                                            "Failed to save account in background OAuth: {}",
                                            e
                                        ));
                                    } else {
                                        crate::modules::logger::log_info(&format!(
                                            "Successfully added account {} via background OAuth",
                                            user_info.email
                                        ));
                                    }
                                }
                                Err(e) => {
                                    crate::modules::logger::log_error(&format!(
                                        "Failed to fetch user info in background OAuth: {}",
                                        e
                                    ));
                                }
                            }
                        } else {
                            crate::modules::logger::log_error(
                                "Background OAuth error: Google did not return a refresh_token.",
                            );
                        }
                    }
                    Err(e) => {
                        crate::modules::logger::log_error(&format!(
                            "Background OAuth exchange failed: {}",
                            e
                        ));
                    }
                }
            }
            Some(Err(e)) => {
                crate::modules::logger::log_error(&format!("Background OAuth flow error: {}", e));
            }
            None => {
                crate::modules::logger::log_info("Background OAuth flow channel closed");
            }
        }
    });

    Ok(Json(serde_json::json!({
        "url": auth_url,
        "state": state_str
    })))
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Get OAuth redirect URI
/// Forces localhost to bypass Google 2.0 policy restrictions on IP addresses and non-HTTPS.
/// Only uses external address when ABV_PUBLIC_URL is explicitly set (e.g., user configured HTTPS domain).
pub fn get_oauth_redirect_uri(port: u16, _host: Option<&str>, _proto: Option<&str>) -> String {
    if let Ok(public_url) = std::env::var("ABV_PUBLIC_URL") {
        let base = public_url.trim_end_matches('/');
        format!("{}/auth/callback", base)
    } else {
        // Force localhost. For remote deployments, users can complete auth via fallback feature.
        format!("http://localhost:{}/auth/callback", port)
    }
}
