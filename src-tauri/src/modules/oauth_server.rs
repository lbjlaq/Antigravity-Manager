use crate::modules::oauth;
use std::sync::{Mutex, OnceLock};
use tauri::Url;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::sync::watch;

struct OAuthFlowState {
    auth_url: String,
    #[allow(dead_code)]
    redirect_uri: String,
    state: String,
    client_key: String,
    cancel_tx: watch::Sender<bool>,
    code_tx: mpsc::Sender<Result<String, String>>,
    code_rx: Option<mpsc::Receiver<Result<String, String>>>,
}

static OAUTH_FLOW_STATE: OnceLock<Mutex<Option<OAuthFlowState>>> = OnceLock::new();

fn get_oauth_flow_state() -> &'static Mutex<Option<OAuthFlowState>> {
    OAUTH_FLOW_STATE.get_or_init(|| Mutex::new(None))
}

fn oauth_success_html() -> &'static str {
    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
    <html>\
    <body style='font-family: sans-serif; text-align: center; padding: 50px;'>\
    <h1 style='color: green;'>✅ Authorization Successful!</h1>\
    <p>You can close this window and return to the application.</p>\
    <script>setTimeout(function() { window.close(); }, 2000);</script>\
    </body>\
    </html>"
}

fn oauth_fail_html() -> &'static str {
    "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
    <html>\
    <body style='font-family: sans-serif; text-align: center; padding: 50px;'>\
    <h1 style='color: red;'>❌ Authorization Failed</h1>\
    <p>Failed to obtain Authorization Code. Please return to the app and try again.</p>\
    </body>\
    </html>"
}

async fn ensure_oauth_flow_prepared(
    app_handle: Option<tauri::AppHandle>,
    requested_client_key: Option<String>,
) -> Result<String, String> {
    if let Ok(mut state) = get_oauth_flow_state().lock() {
        if let Some(s) = state.as_mut() {
            if let Some(requested_key) = requested_client_key.as_ref() {
                if s.client_key != requested_key.to_ascii_lowercase() {
                    let _ = s.cancel_tx.send(true);
                    *state = None;
                }
            }
        }
    }

    // Return URL if flow already exists and is still "fresh" (receiver hasn't been taken)
    if let Ok(mut state) = get_oauth_flow_state().lock() {
        if let Some(s) = state.as_mut() {
            if s.code_rx.is_some() {
                return Ok(s.auth_url.clone());
            } else {
                // Flow is already "in progress" (rx taken), but user requested a NEW one.
                // Force cancel the old one to allow a new attempt.
                let _ = s.cancel_tx.send(true);
                *state = None;
            }
        }
    }

    // Create loopback listeners.
    // Some browsers resolve `localhost` to IPv6 (::1). To avoid "localhost refused connection",
    // we try to listen on BOTH IPv6 and IPv4 with the same port when possible.
    let mut ipv4_listener: Option<TcpListener> = None;
    let mut ipv6_listener: Option<TcpListener> = None;

    // Prefer creating one listener on an ephemeral port first, then bind the other stack to same port.
    // If both are available -> use `http://localhost:<port>` as redirect URI.
    // If only one is available -> use an explicit IP to force correct stack.
    let port: u16;
    match TcpListener::bind("[::1]:0").await {
        Ok(l6) => {
            port = l6
                .local_addr()
                .map_err(|e| format!("failed_to_get_local_port: {}", e))?
                .port();
            ipv6_listener = Some(l6);

            match TcpListener::bind(format!("127.0.0.1:{}", port)).await {
                Ok(l4) => ipv4_listener = Some(l4),
                Err(e) => {
                    crate::modules::logger::log_warn(&format!(
                        "failed_to_bind_ipv4_callback_port_127_0_0_1:{} (will only listen on IPv6): {}",
                        port, e
                    ));
                }
            }
        }
        Err(_) => {
            let l4 = TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|e| format!("failed_to_bind_local_port: {}", e))?;
            port = l4
                .local_addr()
                .map_err(|e| format!("failed_to_get_local_port: {}", e))?
                .port();
            ipv4_listener = Some(l4);

            match TcpListener::bind(format!("[::1]:{}", port)).await {
                Ok(l6) => ipv6_listener = Some(l6),
                Err(e) => {
                    crate::modules::logger::log_warn(&format!(
                        "failed_to_bind_ipv6_callback_port_::1:{} (will only listen on IPv4): {}",
                        port, e
                    ));
                }
            }
        }
    }

    let has_ipv4 = ipv4_listener.is_some();
    let has_ipv6 = ipv6_listener.is_some();

    let redirect_uri = if has_ipv4 && has_ipv6 {
        format!("http://localhost:{}/oauth-callback", port)
    } else if has_ipv4 {
        format!("http://127.0.0.1:{}/oauth-callback", port)
    } else {
        format!("http://[::1]:{}/oauth-callback", port)
    };

    let state_str = uuid::Uuid::new_v4().to_string();
    let (auth_url, resolved_client_key) = oauth::get_auth_url_with_client(
        &redirect_uri,
        &state_str,
        requested_client_key.as_deref(),
    )?;

    // Cancellation signal (supports multiple consumers)
    let (cancel_tx, cancel_rx) = watch::channel(false);
    // Use mpsc instead of oneshot to allow multiple senders (listener OR manual input)
    let (code_tx, code_rx) = mpsc::channel::<Result<String, String>>(1);

    // Start listeners immediately: even if the user authorizes before clicking "Start OAuth",
    // the browser can still hit our callback and finish the flow.
    let app_handle_for_tasks = app_handle.clone();

    if let Some(l4) = ipv4_listener {
        let tx = code_tx.clone();
        let mut rx = cancel_rx.clone();
        let app_handle = app_handle_for_tasks.clone();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = tokio::select! {
                res = l4.accept() => res.map_err(|e| format!("failed_to_accept_connection: {}", e)),
                _ = rx.changed() => Err("OAuth cancelled".to_string()),
            } {
                // Reuse the existing parsing/response code by constructing a temporary listener task
                // that sends into the shared mpsc channel.
                let mut buffer = [0u8; 4096];
                let bytes_read = stream.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..bytes_read]);

                // [FIX #931/850/778] More robust parsing and detailed logging
                let query_params = request
                    .lines()
                    .next()
                    .and_then(|line| {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            Some(parts[1])
                        } else {
                            None
                        }
                    })
                    .and_then(|path| {
                        // Use a dummy base for parsing; redirect_uri is already set to localhost
                        Url::parse(&format!("http://localhost{}", path)).ok()
                    })
                    .map(|url| {
                        let mut code = None;
                        let mut state = None;
                        for (k, v) in url.query_pairs() {
                            if k == "code" {
                                code = Some(v.to_string());
                            } else if k == "state" {
                                state = Some(v.to_string());
                            }
                        }
                        (code, state)
                    });

                let (code, received_state) = match query_params {
                    Some((c, s)) => (c, s),
                    None => (None, None),
                };

                if code.is_none() && bytes_read > 0 {
                    crate::modules::logger::log_error(&format!(
                        "OAuth callback failed to parse code. Raw request (first 512 bytes): {}",
                        &request.chars().take(512).collect::<String>()
                    ));
                }

                // Verify state
                let state_valid = {
                    if let Ok(lock) = get_oauth_flow_state().lock() {
                        if let Some(s) = lock.as_ref() {
                            received_state.as_ref() == Some(&s.state)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };

                let (result, response_html) = match (code, state_valid) {
                    (Some(code), true) => {
                        crate::modules::logger::log_info(
                            "Successfully captured OAuth code from IPv4 listener",
                        );
                        (Ok(code), oauth_success_html())
                    }
                    (Some(_), false) => {
                        crate::modules::logger::log_error(
                            "OAuth callback state mismatch (CSRF protection)",
                        );
                        (Err("OAuth state mismatch".to_string()), oauth_fail_html())
                    }
                    (None, _) => (
                        Err("Failed to get Authorization Code in callback".to_string()),
                        oauth_fail_html(),
                    ),
                };

                let _ = stream.write_all(response_html.as_bytes()).await;
                let _ = stream.flush().await;

                if let Some(h) = app_handle {
                    use tauri::Emitter;
                    let _ = h.emit("oauth-callback-received", ());
                }
                let _ = tx.send(result).await;
            }
        });
    }

    if let Some(l6) = ipv6_listener {
        let tx = code_tx.clone();
        let mut rx = cancel_rx;
        let app_handle = app_handle_for_tasks;
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = tokio::select! {
                res = l6.accept() => res.map_err(|e| format!("failed_to_accept_connection: {}", e)),
                _ = rx.changed() => Err("OAuth cancelled".to_string()),
            } {
                let mut buffer = [0u8; 4096];
                let bytes_read = stream.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..bytes_read]);

                let query_params = request
                    .lines()
                    .next()
                    .and_then(|line| {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            Some(parts[1])
                        } else {
                            None
                        }
                    })
                    .and_then(|path| Url::parse(&format!("http://localhost{}", path)).ok())
                    .map(|url| {
                        let mut code = None;
                        let mut state = None;
                        for (k, v) in url.query_pairs() {
                            if k == "code" {
                                code = Some(v.to_string());
                            } else if k == "state" {
                                state = Some(v.to_string());
                            }
                        }
                        (code, state)
                    });

                let (code, received_state) = match query_params {
                    Some((c, s)) => (c, s),
                    None => (None, None),
                };

                if code.is_none() && bytes_read > 0 {
                    crate::modules::logger::log_error(&format!(
                        "OAuth callback failed to parse code (IPv6). Raw request: {}",
                        &request.chars().take(512).collect::<String>()
                    ));
                }

                // Verify state
                let state_valid = {
                    if let Ok(lock) = get_oauth_flow_state().lock() {
                        if let Some(s) = lock.as_ref() {
                            received_state.as_ref() == Some(&s.state)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };

                let (result, response_html) = match (code, state_valid) {
                    (Some(code), true) => {
                        crate::modules::logger::log_info(
                            "Successfully captured OAuth code from IPv6 listener",
                        );
                        (Ok(code), oauth_success_html())
                    }
                    (Some(_), false) => {
                        crate::modules::logger::log_error(
                            "OAuth callback state mismatch (IPv6 CSRF protection)",
                        );
                        (Err("OAuth state mismatch".to_string()), oauth_fail_html())
                    }
                    (None, _) => (
                        Err("Failed to get Authorization Code in callback".to_string()),
                        oauth_fail_html(),
                    ),
                };

                let _ = stream.write_all(response_html.as_bytes()).await;
                let _ = stream.flush().await;

                if let Some(h) = app_handle {
                    use tauri::Emitter;
                    let _ = h.emit("oauth-callback-received", ());
                }
                let _ = tx.send(result).await;
            }
        });
    }

    // Save state
    if let Ok(mut state) = get_oauth_flow_state().lock() {
        *state = Some(OAuthFlowState {
            auth_url: auth_url.clone(),
            redirect_uri,
            state: state_str,
            client_key: resolved_client_key,
            cancel_tx,
            code_tx,
            code_rx: Some(code_rx),
        });
    }

    // Send event to frontend (for display/copying link)
    if let Some(h) = app_handle {
        use tauri::Emitter;
        let _ = h.emit("oauth-url-generated", &auth_url);
    }

    Ok(auth_url)
}

/// Pre-generate OAuth URL (does not open browser, does not block waiting for callback)
pub async fn prepare_oauth_url(
    app_handle: Option<tauri::AppHandle>,
    oauth_client_key: Option<String>,
) -> Result<String, String> {
    ensure_oauth_flow_prepared(app_handle, oauth_client_key).await
}

/// Cancel current OAuth flow
pub fn cancel_oauth_flow() {
    if let Ok(mut state) = get_oauth_flow_state().lock() {
        if let Some(s) = state.take() {
            let _ = s.cancel_tx.send(true);
            crate::modules::logger::log_info("Sent OAuth cancellation signal");
        }
    }
}

/// Start OAuth flow and wait for callback, then exchange token
pub async fn start_oauth_flow(
    app_handle: Option<tauri::AppHandle>,
    oauth_client_key: Option<String>,
) -> Result<oauth::TokenResponse, String> {
    // Ensure URL + listener are ready (this way if the user authorizes first, it won't get stuck)
    let auth_url = ensure_oauth_flow_prepared(app_handle.clone(), oauth_client_key).await?;

    if let Some(h) = app_handle {
        // Open default browser
        use tauri_plugin_opener::OpenerExt;
        h.opener()
            .open_url(&auth_url, None::<String>)
            .map_err(|e| format!("failed_to_open_browser: {}", e))?;
    }

    // Take code_rx to wait for it
    let (mut code_rx, redirect_uri, client_key) = {
        let mut lock = get_oauth_flow_state()
            .lock()
            .map_err(|_| "OAuth state lock corrupted".to_string())?;
        let Some(state) = lock.as_mut() else {
            return Err("OAuth state does not exist".to_string());
        };
        let rx = state
            .code_rx
            .take()
            .ok_or_else(|| "OAuth authorization already in progress".to_string())?;
        (rx, state.redirect_uri.clone(), state.client_key.clone())
    };

    // Wait for code (if user has already authorized, this returns immediately)
    // For mpsc, we use recv()
    let code = match code_rx.recv().await {
        Some(Ok(code)) => code,
        Some(Err(e)) => return Err(e),
        None => return Err("OAuth flow channel closed unexpectedly".to_string()),
    };

    // Clean up flow state (release cancel_tx, etc.)
    if let Ok(mut lock) = get_oauth_flow_state().lock() {
        *lock = None;
    }

    oauth::exchange_code_with_client(&code, &redirect_uri, Some(&client_key)).await
}

/// Завершить OAuth flow без открытия браузера.
/// Предполагается, что пользователь открыл ссылку вручную (или ранее была открыта),
/// а мы только ждём callback и обмениваем code на token.
pub async fn complete_oauth_flow(
    app_handle: Option<tauri::AppHandle>,
) -> Result<oauth::TokenResponse, String> {
    // Ensure URL + listeners exist
    let _ = ensure_oauth_flow_prepared(app_handle, None).await?;

    // Take receiver to wait for code
    let (mut code_rx, redirect_uri, client_key) = {
        let mut lock = get_oauth_flow_state()
            .lock()
            .map_err(|_| "OAuth state lock corrupted".to_string())?;
        let Some(state) = lock.as_mut() else {
            return Err("OAuth state does not exist".to_string());
        };
        let rx = state
            .code_rx
            .take()
            .ok_or_else(|| "OAuth authorization already in progress".to_string())?;
        (rx, state.redirect_uri.clone(), state.client_key.clone())
    };

    let code = match code_rx.recv().await {
        Some(Ok(code)) => code,
        Some(Err(e)) => return Err(e),
        None => return Err("OAuth flow channel closed unexpectedly".to_string()),
    };

    if let Ok(mut lock) = get_oauth_flow_state().lock() {
        *lock = None;
    }

    oauth::exchange_code_with_client(&code, &redirect_uri, Some(&client_key)).await
}

/// Snapshot active OAuth flow context `(redirect_uri, state, client_key)` if present.
pub fn get_active_oauth_flow_context() -> Option<(String, String, String)> {
    let lock = get_oauth_flow_state().lock().ok()?;
    let state = lock.as_ref()?;
    Some((
        state.redirect_uri.clone(),
        state.state.clone(),
        state.client_key.clone(),
    ))
}

/// Parse manual OAuth input (full callback URL, schemeless callback URL, query string, or raw code).
/// Returns `(decoded_code, extracted_state)`.
pub fn parse_manual_oauth_input(raw_input: &str) -> Result<(String, Option<String>), String> {
    let trimmed = raw_input.trim();
    if trimmed.is_empty() {
        return Err("Authorization code cannot be empty".to_string());
    }

    let extract_from_url = |url: &Url| -> Result<Option<(String, Option<String>)>, String> {
        let mut code = None;
        let mut state = None;
        let mut oauth_error = None;
        for (k, v) in url.query_pairs() {
            match k.as_ref() {
                "code" if !v.trim().is_empty() => code = Some(v.trim().to_string()),
                "state" if !v.trim().is_empty() => state = Some(v.trim().to_string()),
                "error" if !v.trim().is_empty() => oauth_error = Some(v.trim().to_string()),
                _ => {}
            }
        }
        if let Some(err) = oauth_error {
            return Err(format!("OAuth callback returned error: {}", err));
        }
        Ok(code.map(|c| (c, state)))
    };

    // 1. Standard URL starting with http:// or https://
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let url = Url::parse(trimmed).map_err(|e| format!("Invalid callback URL: {}", e))?;
        if let Some(res) = extract_from_url(&url)? {
            return Ok(res);
        }
        return Err("Callback URL does not contain a 'code' query parameter".to_string());
    }

    // 2. Schemeless localhost/127.0.0.1/[::1] URL or query fragment containing "code=" or "error="
    if trimmed.starts_with("localhost:")
        || trimmed.starts_with("127.0.0.1:")
        || trimmed.starts_with("[::1]:")
        || trimmed.contains("code=")
        || trimmed.contains("error=")
    {
        let candidate = if trimmed.starts_with('?') || trimmed.starts_with('/') {
            format!("http://localhost{}", trimmed)
        } else if trimmed.contains("://") {
            trimmed.to_string()
        } else if trimmed.starts_with("localhost")
            || trimmed.starts_with("127.0.0.1")
            || trimmed.starts_with("[::1]")
        {
            format!("http://{}", trimmed)
        } else {
            format!("http://localhost/?{}", trimmed.trim_start_matches('&'))
        };

        if let Ok(url) = Url::parse(&candidate) {
            if let Some(res) = extract_from_url(&url)? {
                return Ok(res);
            }
        }
    }

    // 3. Raw authorization code (possibly URL-encoded like "4%2F0A...")
    let dummy_url = Url::parse(&format!("http://localhost/?code={}", trimmed))
        .map_err(|_| "Invalid authorization code format".to_string())?;
    let decoded_code = dummy_url
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "Authorization code cannot be empty".to_string())?;

    Ok((decoded_code, None))
}

/// Manually submit an OAuth code to complete the flow.
/// Synchronously exchanges the code using the prepared flow's redirect_uri and client_key.
pub async fn submit_oauth_code(
    code_input: String,
    state_input: Option<String>,
) -> Result<oauth::TokenResponse, String> {
    let (code, extracted_state) = parse_manual_oauth_input(&code_input)?;
    let effective_state = state_input
        .filter(|s| !s.trim().is_empty())
        .or(extracted_state);

    let (redirect_uri, client_key, code_tx) = {
        let lock = get_oauth_flow_state().lock().map_err(|e| e.to_string())?;
        if let Some(state) = lock.as_ref() {
            if let Some(ref provided_state) = effective_state {
                if provided_state != &state.state {
                    return Err("OAuth state mismatch (CSRF protection)".to_string());
                }
            }
            (
                state.redirect_uri.clone(),
                state.client_key.clone(),
                state.code_tx.clone(),
            )
        } else {
            return Err(
                "No active OAuth flow found. Please click 'Start OAuth' first.".to_string(),
            );
        }
    };

    crate::modules::logger::log_info(&format!(
        "Exchanging manual OAuth code with redirect_uri: {}, client_key: {}",
        redirect_uri, client_key
    ));

    // Exchange with the exact redirect_uri and client_key stored in the flow.
    // If exchange fails, cancel flow to ensure corrupted/expired state is not reused.
    let token_res =
        match oauth::exchange_code_with_client(&code, &redirect_uri, Some(&client_key)).await {
            Ok(res) => res,
            Err(e) => {
                crate::modules::logger::log_warn(&format!(
                    "OAuth exchange failed during manual submission, cleaning up flow state: {}",
                    e
                ));
                cancel_oauth_flow();
                return Err(e);
            }
        };

    // Also notify code_tx in case a background listener (e.g. Desktop flow) is awaiting code_rx
    let _ = code_tx.try_send(Ok(code));

    // Clean up flow state
    cancel_oauth_flow();

    Ok(token_res)
}

/// Get the active flow's client_key for callback matching
pub fn get_active_flow_client_key(state_param: Option<&str>) -> Option<String> {
    if let Ok(lock) = get_oauth_flow_state().lock() {
        if let Some(s) = lock.as_ref() {
            if let Some(expected_state) = state_param {
                if s.state == expected_state {
                    return Some(s.client_key.clone());
                }
            } else {
                return Some(s.client_key.clone());
            }
        }
    }
    None
}

/// Manually prepare an OAuth flow without starting listeners.
/// Useful for Web/Docker environments where we only need manual code submission.
pub fn prepare_oauth_flow_manually(
    redirect_uri: String,
    oauth_client_key: Option<String>,
) -> Result<String, String> {
    // 1. Check if we can reuse existing flow state
    if let Ok(lock) = get_oauth_flow_state().lock() {
        if let Some(s) = lock.as_ref() {
            let same_client = match oauth_client_key.as_ref() {
                Some(key) => s.client_key == key.to_ascii_lowercase(),
                None => true,
            };
            if same_client && s.redirect_uri == redirect_uri {
                return Ok(s.auth_url.clone());
            }
        }
    }

    // 2. Clear old state if different or fresh flow requested
    if let Ok(mut lock) = get_oauth_flow_state().lock() {
        if let Some(s) = lock.as_mut() {
            let _ = s.cancel_tx.send(true);
            *lock = None;
        }
    }

    let state_str = uuid::Uuid::new_v4().to_string();
    let (auth_url, resolved_client_key) =
        oauth::get_auth_url_with_client(&redirect_uri, &state_str, oauth_client_key.as_deref())?;

    let (cancel_tx, _cancel_rx) = watch::channel(false);
    let (code_tx, code_rx) = mpsc::channel(1);

    if let Ok(mut state) = get_oauth_flow_state().lock() {
        *state = Some(OAuthFlowState {
            auth_url: auth_url.clone(),
            redirect_uri,
            state: state_str,
            client_key: resolved_client_key,
            cancel_tx,
            code_tx,
            code_rx: Some(code_rx),
        });
    }

    Ok(auth_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_manual_oauth_input_full_url() {
        let input = "http://localhost:8045/auth/callback?state=state-123&code=4%2F0AeanS0_test_code&scope=email+profile";
        let (code, state) =
            parse_manual_oauth_input(input).expect("should parse full callback url");
        assert_eq!(code, "4/0AeanS0_test_code");
        assert_eq!(state.as_deref(), Some("state-123"));
    }

    #[test]
    fn test_parse_manual_oauth_input_schemeless_localhost_url() {
        let input = "localhost:8045/auth/callback?code=4%2F0AeanS0_schemeless&state=st-999";
        let (code, state) =
            parse_manual_oauth_input(input).expect("should parse schemeless callback url");
        assert_eq!(code, "4/0AeanS0_schemeless");
        assert_eq!(state.as_deref(), Some("st-999"));
    }

    #[test]
    fn test_parse_manual_oauth_input_raw_encoded_and_decoded_code() {
        let (code1, state1) =
            parse_manual_oauth_input("  4%2F0AeanS0_encoded_raw  ").expect("should decode raw %2F");
        assert_eq!(code1, "4/0AeanS0_encoded_raw");
        assert!(state1.is_none());

        let (code2, state2) =
            parse_manual_oauth_input("4/0AeanS0_plain_raw").expect("should keep plain raw code");
        assert_eq!(code2, "4/0AeanS0_plain_raw");
        assert!(state2.is_none());
    }

    #[test]
    fn test_parse_manual_oauth_input_rejects_error_callback_and_empty() {
        let err = parse_manual_oauth_input(
            "http://localhost:8045/auth/callback?error=access_denied&state=s1",
        )
        .unwrap_err();
        assert!(err.contains("access_denied"));

        assert!(parse_manual_oauth_input("   ").is_err());
    }

    #[test]
    fn test_prepare_oauth_flow_manually_reuse() {
        let redirect_uri = "http://localhost:8045/auth/callback".to_string();
        let url1 = prepare_oauth_flow_manually(redirect_uri.clone(), None)
            .expect("First prepare should succeed");
        let url2 = prepare_oauth_flow_manually(redirect_uri.clone(), None)
            .expect("Second prepare should succeed");

        assert_eq!(
            url1, url2,
            "Subsequent calls with same parameters should reuse the active flow URL"
        );
    }

    #[tokio::test]
    async fn test_submit_oauth_code_state_mismatch_and_cleanup() {
        let redirect_uri = "http://localhost:8045/auth/callback".to_string();
        let _ = prepare_oauth_flow_manually(redirect_uri.clone(), None)
            .expect("prepare should succeed");

        // Submit with deliberately mismatched state
        let err = submit_oauth_code("test_code".to_string(), Some("wrong-state".to_string()))
            .await
            .unwrap_err();
        assert!(err.contains("CSRF protection"));

        // Clean up
        cancel_oauth_flow();
        assert!(get_active_oauth_flow_context().is_none());
    }
}
