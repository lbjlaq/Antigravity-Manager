// use std::io::{Read, Write};
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;
use std::sync::{Mutex, OnceLock};
use tauri::Url;
use crate::modules::oauth;

// 全局取消 Token 存储
static CANCELLATION_TOKEN: OnceLock<Mutex<Option<oneshot::Sender<()>>>> = OnceLock::new();

/// 获取取消 Token 的 Mutex
fn get_cancellation_token() -> &'static Mutex<Option<oneshot::Sender<()>>> {
    CANCELLATION_TOKEN.get_or_init(|| Mutex::new(None))
}

/// 取消当前的 OAuth 流程
pub fn cancel_oauth_flow() {
    let mutex = get_cancellation_token();
    if let Ok(mut lock) = mutex.lock() {
        if let Some(tx) = lock.take() {
            let _ = tx.send(());
            crate::modules::logger::log_info("已发送 OAuth 取消信号");
        }
    }
}

/// 启动 OAuth 流程
/// 1. 启动本地服务器监听回调
/// 2. 打开浏览器访问授权页面
/// 3. 等待并捕获 code
/// 4. 交换 token
pub async fn start_oauth_flow(app_handle: tauri::AppHandle) -> Result<oauth::TokenResponse, String> {
    use tauri::Emitter; // 引入 Emitter trait

    // 创建取消通道
    let (tx, rx) = oneshot::channel::<()>();
    
    // 存储发送端
    {
        let mutex = get_cancellation_token();
        if let Ok(mut lock) = mutex.lock() {
            *lock = Some(tx);
        }
    }

    // 1. 启动本地监听器 (绑定到随机端口)
    // 使用 Tokio TcpListener 实现异步中断
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| format!("无法绑定本地端口: {}", e))?;
    let port = listener.local_addr().map_err(|e| format!("无法获取本地端口: {}", e))?.port();
    
    // 构造动态 Redirect URI
    let redirect_uri = format!("http://localhost:{}/oauth-callback", port);
    
    // 2. 获取授权 URL
    let auth_url = oauth::get_auth_url(&redirect_uri);
    
    // 发送事件给前端 (用于复制链接功能)
    // 忽略发送错误，因为这不影响主流程
    let _ = app_handle.emit("oauth-url-generated", &auth_url);
    
    // 3. 打开浏览器 (使用 tauri_plugin_opener)
    use tauri_plugin_opener::OpenerExt;
    app_handle.opener().open_url(&auth_url, None::<String>).map_err(|e| format!("无法打开浏览器: {}", e))?;
    
    // 4. 等待回调 (阻塞接受一个连接)，支持取消
    let (mut stream, _) = tokio::select! {
        res = listener.accept() => {
             res.map_err(|e| format!("接受连接失败: {}", e))?
        }
        _ = rx => {
            return Err("用户取消了授权".to_string());
        }
    };
    
    // 清除取消 token (也可以不做，因为已经被使用了)
    {
        let mutex = get_cancellation_token();
        if let Ok(mut lock) = mutex.lock() {
             *lock = None;
        }
    }
    
    let mut buffer = [0; 1024];
    stream.read(&mut buffer).await.map_err(|e| format!("读取请求失败: {}", e))?;
    
    let request = String::from_utf8_lossy(&buffer);
    
    // 解析请求行获取 code
    // GET /oauth-callback?code=XXXX HTTP/1.1
    let code = if let Some(line) = request.lines().next() {
        if let Some(path) = line.split_whitespace().nth(1) {
            let url = Url::parse(&format!("http://localhost:{}{}", port, path))
                .map_err(|e| format!("URL 解析失败: {}", e))?;
            
            let pairs = url.query_pairs();
            let mut code = None;
            for (key, value) in pairs {
                if key == "code" {
                    code = Some(value.into_owned());
                    break;
                }
            }
            code
        } else {
            None
        }
    } else {
        None
    };
    
    let response_html = if code.is_some() {
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n
        <html>
        <body style='font-family: sans-serif; text-align: center; padding: 50px;'>
            <h1 style='color: green;'>✅ 授权成功!</h1>
            <p>您可以关闭此窗口返回应用。</p>
            <script>setTimeout(function() { window.close(); }, 2000);</script>
        </body>
        </html>"
    } else {
        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n<h1>❌ 授权失败</h1>"
    };
    
    stream.write_all(response_html.as_bytes()).await.unwrap_or(());
    stream.flush().await.unwrap_or(());
    
    let code = code.ok_or("未能在回调中获取 Authorization Code")?;
    
    // 5. 交换 Token
    oauth::exchange_code(&code, &redirect_uri).await
}
