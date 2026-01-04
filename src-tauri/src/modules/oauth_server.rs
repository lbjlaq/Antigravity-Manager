use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::sync::watch;
use std::sync::{Mutex, OnceLock};
use tauri::Url;
use crate::modules::oauth;

struct OAuthFlowState {
    auth_url: String,
    redirect_uri: String,
    cancel_tx: watch::Sender<bool>,
    code_rx: Option<oneshot::Receiver<Result<String, String>>>,
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
        <p>You can close this window and return to the app.</p>\
        <script>setTimeout(function() { window.close(); }, 2000);</script>\
    </body>\
    </html>"
}

fn oauth_fail_html() -> &'static str {
    "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
    <html>\
    <body style='font-family: sans-serif; text-align: center; padding: 50px;'>\
        <h1 style='color: red;'>❌ Authorization Failed</h1>\
        <p>Could not obtain authorization code. Please return to the app and try again.</p>\
    </body>\
    </html>"
}

async fn ensure_oauth_flow_prepared(app_handle: &tauri::AppHandle) -> Result<String, String> {
    use tauri::Emitter;

    // 如果已有 flow，直接返回 URL
    if let Ok(state) = get_oauth_flow_state().lock() {
        if let Some(s) = state.as_ref() {
            return Ok(s.auth_url.clone());
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
                .map_err(|e| format!("无法获取本地端口: {}", e))?
                .port();
            ipv6_listener = Some(l6);

            match TcpListener::bind(format!("127.0.0.1:{}", port)).await {
                Ok(l4) => ipv4_listener = Some(l4),
                Err(e) => {
                    crate::modules::logger::log_warn(&format!(
                        "无法绑定 IPv4 回调端口 127.0.0.1:{} (将仅监听 IPv6): {}",
                        port, e
                    ));
                }
            }
        }
        Err(_) => {
            let l4 = TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|e| format!("无法绑定本地端口: {}", e))?;
            port = l4
                .local_addr()
                .map_err(|e| format!("无法获取本地端口: {}", e))?
                .port();
            ipv4_listener = Some(l4);

            match TcpListener::bind(format!("[::1]:{}", port)).await {
                Ok(l6) => ipv6_listener = Some(l6),
                Err(e) => {
                    crate::modules::logger::log_warn(&format!(
                        "无法绑定 IPv6 回调端口 [::1]:{} (将仅监听 IPv4): {}",
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

    let auth_url = oauth::get_auth_url(&redirect_uri);

    // 取消信号（支持多消费者）
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let (code_tx, code_rx) = oneshot::channel::<Result<String, String>>();

    let code_tx = std::sync::Arc::new(tokio::sync::Mutex::new(Some(code_tx)));

    // Start listeners immediately: even if the user authorizes before clicking "Start OAuth",
    // the browser can still hit our callback and finish the flow.
    let app_handle_for_tasks = app_handle.clone();

    if let Some(l4) = ipv4_listener {
        let tx = code_tx.clone();
        let mut rx = cancel_rx.clone();
        let app_handle = app_handle_for_tasks.clone();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = tokio::select! {
                res = l4.accept() => res.map_err(|e| format!("接受连接失败: {}", e)),
                _ = rx.changed() => Err("OAuth cancelled".to_string()),
            } {
                // Reuse the existing parsing/response code by constructing a temporary listener task
                // that sends into the shared oneshot.
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer).await;
                let request = String::from_utf8_lossy(&buffer);
                let code = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .and_then(|path| Url::parse(&format!("http://127.0.0.1:{}{}", port, path)).ok())
                    .and_then(|url| {
                        url.query_pairs()
                            .find(|(k, _)| k == "code")
                            .map(|(_, v)| v.into_owned())
                    });

                let (result, response_html) = match code {
                    Some(code) => (Ok(code), oauth_success_html()),
                    None => (Err("未能在回调中获取 Authorization Code".to_string()), oauth_fail_html()),
                };
                let _ = stream.write_all(response_html.as_bytes()).await;
                let _ = stream.flush().await;

                if let Some(sender) = tx.lock().await.take() {
                    let _ = app_handle.emit("oauth-callback-received", ());
                    let _ = sender.send(result);
                }
            }
        });
    }

    if let Some(l6) = ipv6_listener {
        let tx = code_tx.clone();
        let mut rx = cancel_rx;
        let app_handle = app_handle_for_tasks;
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = tokio::select! {
                res = l6.accept() => res.map_err(|e| format!("接受连接失败: {}", e)),
                _ = rx.changed() => Err("OAuth cancelled".to_string()),
            } {
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer).await;
                let request = String::from_utf8_lossy(&buffer);
                let code = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .and_then(|path| Url::parse(&format!("http://127.0.0.1:{}{}", port, path)).ok())
                    .and_then(|url| {
                        url.query_pairs()
                            .find(|(k, _)| k == "code")
                            .map(|(_, v)| v.into_owned())
                    });

                let (result, response_html) = match code {
                    Some(code) => (Ok(code), oauth_success_html()),
                    None => (Err("未能在回调中获取 Authorization Code".to_string()), oauth_fail_html()),
                };
                let _ = stream.write_all(response_html.as_bytes()).await;
                let _ = stream.flush().await;

                if let Some(sender) = tx.lock().await.take() {
                    let _ = app_handle.emit("oauth-callback-received", ());
                    let _ = sender.send(result);
                }
            }
        });
    }

    // 保存状态
    if let Ok(mut state) = get_oauth_flow_state().lock() {
        *state = Some(OAuthFlowState {
            auth_url: auth_url.clone(),
            redirect_uri,
            cancel_tx,
            code_rx: Some(code_rx),
        });
    }

    // 发送事件给前端（用于展示/复制链接）
    let _ = app_handle.emit("oauth-url-generated", &auth_url);

    Ok(auth_url)
}

/// 预生成 OAuth URL (不打开浏览器、不阻塞等待回调)
pub async fn prepare_oauth_url(app_handle: tauri::AppHandle) -> Result<String, String> {
    ensure_oauth_flow_prepared(&app_handle).await
}

/// 取消当前的 OAuth 流程
pub fn cancel_oauth_flow() {
    if let Ok(mut state) = get_oauth_flow_state().lock() {
        if let Some(s) = state.take() {
            let _ = s.cancel_tx.send(true);
            crate::modules::logger::log_info("已发送 OAuth 取消信号");
        }
    }
}

/// 启动 OAuth 流程并等待回调，再交换 token
pub async fn start_oauth_flow(app_handle: tauri::AppHandle) -> Result<oauth::TokenResponse, String> {
    // 确保已准备好 URL + listener（这样即使用户先授权，也不会卡住）
    let auth_url = ensure_oauth_flow_prepared(&app_handle).await?;

    // 打开默认浏览器
    use tauri_plugin_opener::OpenerExt;
    app_handle
        .opener()
        .open_url(&auth_url, None::<String>)
        .map_err(|e| format!("无法打开浏览器: {}", e))?;

    // 取出 code_rx 用于等待
    let (code_rx, redirect_uri) = {
        let mut lock = get_oauth_flow_state()
            .lock()
            .map_err(|_| "OAuth 状态锁被污染".to_string())?;
        let Some(state) = lock.as_mut() else {
            return Err("OAuth 状态不存在".to_string());
        };
        let rx = state
            .code_rx
            .take()
            .ok_or_else(|| "OAuth 授权已在进行中".to_string())?;
        (rx, state.redirect_uri.clone())
    };

    // 等待 code（如果用户已完成授权，此处会立即返回）
    let code = match code_rx.await {
        Ok(Ok(code)) => code,
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err("等待 OAuth 回调失败".to_string()),
    };

    // 清理 flow state（释放 cancel_tx 等）
    if let Ok(mut lock) = get_oauth_flow_state().lock() {
        *lock = None;
    }

    oauth::exchange_code(&code, &redirect_uri).await
}

/// Завершить OAuth flow без открытия браузера.
/// Предполагается, что пользователь открыл ссылку вручную (или ранее была открыта),
/// а мы только ждём callback и обмениваем code на token.
pub async fn complete_oauth_flow(app_handle: tauri::AppHandle) -> Result<oauth::TokenResponse, String> {
    // Ensure URL + listeners exist
    let _ = ensure_oauth_flow_prepared(&app_handle).await?;

    // Take receiver to wait for code
    let (code_rx, redirect_uri) = {
        let mut lock = get_oauth_flow_state()
            .lock()
            .map_err(|_| "OAuth 状态锁被污染".to_string())?;
        let Some(state) = lock.as_mut() else {
            return Err("OAuth 状态不存在".to_string());
        };
        let rx = state
            .code_rx
            .take()
            .ok_or_else(|| "OAuth 授权已在进行中".to_string())?;
        (rx, state.redirect_uri.clone())
    };

    let code = match code_rx.await {
        Ok(Ok(code)) => code,
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err("等待 OAuth 回调失败".to_string()),
    };

    if let Ok(mut lock) = get_oauth_flow_state().lock() {
        *lock = None;
    }

    oauth::exchange_code(&code, &redirect_uri).await
}
