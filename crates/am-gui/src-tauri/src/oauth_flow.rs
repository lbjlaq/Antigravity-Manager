use am_core::auth::OAuthClient;
use am_core::types::Account;
use am_core::AntigravityManager;
use std::sync::{Mutex, OnceLock};
use tauri::{Emitter, Url};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};

struct OAuthFlowState {
    auth_url: String,
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

const OAUTH_LISTENER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);
const OAUTH_STREAM_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

fn oauth_success_html() -> &'static str {
    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n\
    <!DOCTYPE html><html><head><meta charset='utf-8'><title>Antigravity Manager</title></head>\
    <body style='font-family:-apple-system,BlinkMacSystemFont,sans-serif;background:#09090b;color:#f4f4f5;text-align:center;padding:60px 20px;'>\
    <div style='display:inline-block;padding:32px 48px;border-radius:20px;background:#18181b;border:1px solid #27272a;box-shadow:0 12px 30px rgba(0,0,0,0.5);'>\
    <div style='font-size:40px;margin-bottom:16px;'>✅</div>\
    <h2 style='margin:0 0 8px 0;font-size:20px;'>授权成功 / Authorization Successful</h2>\
    <p style='color:#a1a1aa;font-size:13px;margin:0;'>Antigravity Manager 已安全同步凭证，您可以关闭此浏览器标签页。</p>\
    </div><script>setTimeout(function(){window.close();},2000);</script></body></html>"
}

fn oauth_fail_html() -> &'static str {
    "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n\
    <!DOCTYPE html><html><head><meta charset='utf-8'><title>Antigravity Manager</title></head>\
    <body style='font-family:-apple-system,BlinkMacSystemFont,sans-serif;background:#09090b;color:#f4f4f5;text-align:center;padding:60px 20px;'>\
    <div style='display:inline-block;padding:32px 48px;border-radius:20px;background:#18181b;border:1px solid #27272a;'>\
    <div style='font-size:40px;margin-bottom:16px;'>❌</div>\
    <h2 style='margin:0 0 8px 0;font-size:20px;'>授权失败 / Authorization Failed</h2>\
    <p style='color:#a1a1aa;font-size:13px;margin:0;'>Failed to obtain Authorization Code. Please return to the app and try again.</p>\
    </div></body></html>"
}

async fn serve_oauth_callback_listener(
    listener: TcpListener,
    expected_state: String,
    tx: mpsc::Sender<Result<String, String>>,
    cancel_tx: watch::Sender<bool>,
    mut rx: watch::Receiver<bool>,
    app_handle: Option<tauri::AppHandle>,
) {
    let deadline = tokio::time::Instant::now() + OAUTH_LISTENER_TIMEOUT;
    loop {
        let stream = tokio::select! {
            _ = tokio::time::sleep_until(deadline) => {
                if let Ok(mut state) = get_oauth_flow_state().lock() {
                    if state.as_ref().is_some_and(|s| s.state == expected_state) {
                        *state = None;
                    }
                }
                let _ = tx.try_send(Err("OAuth callback listener timed out waiting for authorization".to_string()));
                let _ = cancel_tx.send(true);
                break;
            }
            _ = rx.changed() => {
                break;
            }
            res = listener.accept() => {
                match res {
                    Ok((stream, _)) => stream,
                    Err(_) => {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        continue;
                    }
                }
            }
        };

        let mut stream = stream;
        let mut buffer = [0u8; 4096];
        let bytes_read =
            match tokio::time::timeout(OAUTH_STREAM_READ_TIMEOUT, stream.read(&mut buffer)).await {
                Ok(Ok(n)) => n,
                Ok(Err(_)) | Err(_) => continue,
            };
        if bytes_read == 0 {
            continue;
        }

        let request = String::from_utf8_lossy(&buffer[..bytes_read]);
        let parsed_url = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|path| Url::parse(&format!("http://localhost{}", path)).ok());

        let (req_path, code, received_state, oauth_error) = match parsed_url {
            Some(url) => {
                let req_path = url.path().to_string();
                let mut code = None;
                let mut state = None;
                let mut oauth_error = None;
                for (k, v) in url.query_pairs() {
                    match k.as_ref() {
                        "code" => code = Some(v.to_string()),
                        "state" => state = Some(v.to_string()),
                        "error" => oauth_error = Some(v.to_string()),
                        _ => {}
                    }
                }
                (req_path, code, state, oauth_error)
            }
            None => (String::new(), None, None, None),
        };

        if req_path != "/oauth-callback" {
            let _ = stream
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .await;
            let _ = stream.flush().await;
            continue;
        }

        let state_valid = received_state.as_deref() == Some(expected_state.as_str());

        if let Some(err_val) = oauth_error {
            let _ = stream.write_all(oauth_fail_html().as_bytes()).await;
            let _ = stream.flush().await;
            if !state_valid {
                continue;
            }
            if let Some(h) = app_handle {
                let _ = h.emit("oauth-callback-received", ());
            }
            let _ = tx.send(Err(format!("OAuth error: {}", err_val))).await;
            let _ = cancel_tx.send(true);
            break;
        }

        match (code, state_valid) {
            (Some(code), true) => {
                let _ = stream.write_all(oauth_success_html().as_bytes()).await;
                let _ = stream.flush().await;
                if let Some(h) = app_handle {
                    let _ = h.emit("oauth-callback-received", ());
                }
                let _ = tx.send(Ok(code)).await;
                let _ = cancel_tx.send(true);
                break;
            }
            _ => {
                let _ = stream.write_all(oauth_fail_html().as_bytes()).await;
                let _ = stream.flush().await;
                continue;
            }
        }
    }
}

async fn ensure_oauth_flow_prepared(
    app_handle: Option<tauri::AppHandle>,
    requested_client_key: Option<String>,
) -> Result<String, String> {
    let resolved_requested_key = requested_client_key.as_deref().map(|k| {
        OAuthClient::resolve_client_credentials(Some(k))
            .0
            .to_string()
    });

    if let Ok(mut state) = get_oauth_flow_state().lock() {
        if let Some(s) = state.as_mut() {
            if let Some(ref requested_key) = resolved_requested_key {
                if s.client_key != *requested_key {
                    let _ = s.cancel_tx.send(true);
                    *state = None;
                }
            }
        }
    }

    if let Ok(mut state) = get_oauth_flow_state().lock() {
        if let Some(s) = state.as_mut() {
            if s.code_rx.is_some() {
                return Ok(s.auth_url.clone());
            } else {
                let _ = s.cancel_tx.send(true);
                *state = None;
            }
        }
    }

    let mut ipv4_listener: Option<TcpListener> = None;
    let mut ipv6_listener: Option<TcpListener> = None;

    let port: u16;
    match TcpListener::bind("[::1]:0").await {
        Ok(l6) => {
            port = l6
                .local_addr()
                .map_err(|e| format!("failed_to_get_local_port: {}", e))?
                .port();
            ipv6_listener = Some(l6);
            if let Ok(l4) = TcpListener::bind(format!("127.0.0.1:{}", port)).await {
                ipv4_listener = Some(l4);
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
            if let Ok(l6) = TcpListener::bind(format!("[::1]:{}", port)).await {
                ipv6_listener = Some(l6);
            }
        }
    }

    let redirect_uri = if ipv4_listener.is_some() && ipv6_listener.is_some() {
        format!("http://localhost:{}/oauth-callback", port)
    } else if ipv4_listener.is_some() {
        format!("http://127.0.0.1:{}/oauth-callback", port)
    } else {
        format!("http://[::1]:{}/oauth-callback", port)
    };

    let state_str = uuid::Uuid::new_v4().to_string();
    let (auth_url, resolved_client_key) = OAuthClient::build_auth_url_with_client(
        &redirect_uri,
        &state_str,
        requested_client_key.as_deref(),
    );

    let (cancel_tx, cancel_rx) = watch::channel(false);
    let (code_tx, code_rx) = mpsc::channel::<Result<String, String>>(1);

    if let Some(l4) = ipv4_listener {
        tokio::spawn(serve_oauth_callback_listener(
            l4,
            state_str.clone(),
            code_tx.clone(),
            cancel_tx.clone(),
            cancel_rx.clone(),
            app_handle.clone(),
        ));
    }

    if let Some(l6) = ipv6_listener {
        tokio::spawn(serve_oauth_callback_listener(
            l6,
            state_str.clone(),
            code_tx.clone(),
            cancel_tx.clone(),
            cancel_rx,
            app_handle.clone(),
        ));
    }

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

    if let Some(h) = app_handle {
        let _ = h.emit("oauth-url-generated", &auth_url);
    }

    Ok(auth_url)
}

pub async fn prepare_oauth_url(
    app_handle: Option<tauri::AppHandle>,
    oauth_client_key: Option<String>,
) -> Result<String, String> {
    ensure_oauth_flow_prepared(app_handle, oauth_client_key).await
}

pub fn cancel_oauth_flow() {
    if let Ok(mut state) = get_oauth_flow_state().lock() {
        if let Some(s) = state.take() {
            let _ = s.cancel_tx.send(true);
        }
    }
}

async fn finish_flow_with_manager(
    manager: &AntigravityManager,
    open_browser: bool,
    app_handle: Option<tauri::AppHandle>,
    oauth_client_key: Option<String>,
) -> Result<Account, String> {
    let auth_url = ensure_oauth_flow_prepared(app_handle, oauth_client_key).await?;

    if open_browser {
        open::that(&auth_url).map_err(|e| format!("failed_to_open_browser: {}", e))?;
    }

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
        Some(Err(e)) => {
            cancel_oauth_flow();
            return Err(e);
        }
        None => {
            cancel_oauth_flow();
            return Err("OAuth flow channel closed unexpectedly".to_string());
        }
    };

    cancel_oauth_flow();

    manager
        .complete_oauth_code_exchange(&code, &redirect_uri, Some(&client_key))
        .await
        .map_err(|e| e.to_string())
}

pub async fn start_oauth_login(
    app_handle: tauri::AppHandle,
    manager: &AntigravityManager,
    oauth_client_key: Option<String>,
) -> Result<Account, String> {
    finish_flow_with_manager(manager, true, Some(app_handle), oauth_client_key).await
}

pub async fn complete_oauth_login(
    app_handle: tauri::AppHandle,
    manager: &AntigravityManager,
) -> Result<Account, String> {
    finish_flow_with_manager(manager, false, Some(app_handle), None).await
}

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

    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let url = Url::parse(trimmed).map_err(|e| format!("Invalid callback URL: {}", e))?;
        if let Some(res) = extract_from_url(&url)? {
            return Ok(res);
        }
        return Err("Callback URL does not contain a 'code' query parameter".to_string());
    }

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

pub async fn submit_oauth_code(
    manager: &AntigravityManager,
    code_input: String,
    state_input: Option<String>,
) -> Result<Account, String> {
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

    let account = match manager
        .complete_oauth_code_exchange(&code, &redirect_uri, Some(&client_key))
        .await
    {
        Ok(acc) => acc,
        Err(e) => {
            cancel_oauth_flow();
            return Err(e.to_string());
        }
    };

    let _ = code_tx.try_send(Ok(code));
    cancel_oauth_flow();

    Ok(account)
}

#[cfg(test)]
mod tests {
    use super::parse_manual_oauth_input;

    #[test]
    fn parses_callback_url_query_and_raw_code() {
        let (code, state) = parse_manual_oauth_input(
            "http://localhost:12345/oauth-callback?state=abc-123&code=4%2F0Atest_code&scope=email",
        )
        .unwrap();
        assert_eq!(code, "4/0Atest_code");
        assert_eq!(state.as_deref(), Some("abc-123"));

        let (code2, state2) = parse_manual_oauth_input("4%2F0Araw_only").unwrap();
        assert_eq!(code2, "4/0Araw_only");
        assert!(state2.is_none());
    }
}
