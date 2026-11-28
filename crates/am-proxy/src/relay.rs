use crate::types::{ProxyError, ProxyNode, ProxyProtocol};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, RwLock};
use tracing::{debug, warn};

/// 默认本地热切换中继端口
pub const DEFAULT_RELAY_PORT: u16 = 17890;

/// 本地固定端口代理中继服务
///
/// 同端口自动识别 HTTP/HTTPS CONNECT 与 SOCKS5 协议，
/// 支持运行时热切换上游代理节点（或切回直连），无需重启 Antigravity。
pub struct RelayServer {
    local_addr: SocketAddr,
    upstream: Arc<RwLock<Option<ProxyNode>>>,
    shutdown_tx: watch::Sender<bool>,
}

impl RelayServer {
    /// 启动本地中继服务
    pub async fn start(
        bind_addr: &str,
        initial_upstream: Option<ProxyNode>,
    ) -> Result<Self, ProxyError> {
        let listener = TcpListener::bind(bind_addr).await?;
        let local_addr = listener.local_addr()?;
        let upstream = Arc::new(RwLock::new(initial_upstream));
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

        let upstream_clone = Arc::clone(&upstream);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            debug!("am-proxy relay shutting down");
                            break;
                        }
                    }
                    accept_res = listener.accept() => {
                        match accept_res {
                            Ok((stream, _peer)) => {
                                let up = Arc::clone(&upstream_clone);
                                tokio::spawn(async move {
                                    if let Err(e) = handle_client(stream, up).await {
                                        debug!("relay client error: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                warn!("relay accept error: {}", e);
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            local_addr,
            upstream,
            shutdown_tx,
        })
    }

    /// 获取本地监听地址
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// 获取本地 HTTP 代理 URL
    pub fn local_http_url(&self) -> String {
        format!("http://{}", self.local_addr)
    }

    /// 运行时热切换上游节点（传 `None` 切换为直连模式）
    pub async fn switch_upstream(&self, node: Option<ProxyNode>) {
        let mut guard = self.upstream.write().await;
        *guard = node;
    }

    /// 读取当前生效的上游节点
    pub async fn current_upstream(&self) -> Option<ProxyNode> {
        self.upstream.read().await.clone()
    }

    /// 停止本地中继服务
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

impl Drop for RelayServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

async fn handle_client(
    client: TcpStream,
    upstream_lock: Arc<RwLock<Option<ProxyNode>>>,
) -> Result<(), ProxyError> {
    let mut peek_buf = [0u8; 1];
    let n = client.peek(&mut peek_buf).await?;
    if n == 0 {
        return Ok(());
    }

    let upstream = upstream_lock.read().await.clone();

    if peek_buf[0] == 0x05 {
        handle_socks5(client, upstream).await
    } else {
        handle_http(client, upstream).await
    }
}

/// 处理本地 HTTP / HTTPS CONNECT 请求
async fn handle_http(mut client: TcpStream, upstream: Option<ProxyNode>) -> Result<(), ProxyError> {
    let mut buf = vec![0u8; 8192];
    let n = client.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }
    let req_head = String::from_utf8_lossy(&buf[..n]);
    let first_line = req_head.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(ProxyError::Protocol("Malformed HTTP request line".into()));
    }

    let method = parts[0];
    let target = parts[1];

    if method.eq_ignore_ascii_case("CONNECT") {
        // HTTPS 隧道请求: CONNECT host:port HTTP/1.1
        let (host, port) = parse_host_port(target, 443)?;
        let mut server = connect_via_upstream(&host, port, upstream.as_ref()).await?;
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
    } else {
        // 普通 HTTP 代理请求
        match upstream {
            Some(ref node) if node.protocol == ProxyProtocol::Http => {
                // 直接透传给上游 HTTP 代理
                let mut server = TcpStream::connect(format!("{}:{}", node.host, node.port)).await?;
                server.write_all(&buf[..n]).await?;
                let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
            }
            _ => {
                // 解析 URL 目标并通过 SOCKS5 或直连转发
                let parsed = url::Url::parse(target)
                    .map_err(|e| ProxyError::Protocol(format!("Invalid HTTP proxy URL: {}", e)))?;
                let host = parsed
                    .host_str()
                    .ok_or_else(|| ProxyError::Protocol("Missing host in HTTP request".into()))?;
                let port = parsed.port_or_known_default().unwrap_or(80);
                let mut server = connect_via_upstream(host, port, upstream.as_ref()).await?;
                server.write_all(&buf[..n]).await?;
                let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
            }
        }
    }

    Ok(())
}

/// 处理本地 SOCKS5 请求（无认证）
async fn handle_socks5(
    mut client: TcpStream,
    upstream: Option<ProxyNode>,
) -> Result<(), ProxyError> {
    let mut header = [0u8; 2];
    client.read_exact(&mut header).await?;
    let nmethods = header[1] as usize;
    let mut methods = vec![0u8; nmethods];
    client.read_exact(&mut methods).await?;

    // 回复：无需认证
    client.write_all(&[0x05, 0x00]).await?;

    // 读取 CONNECT 请求头
    let mut req = [0u8; 4];
    client.read_exact(&mut req).await?;
    if req[1] != 0x01 {
        client
            .write_all(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await?;
        return Err(ProxyError::Protocol(
            "Only SOCKS5 CONNECT is supported".into(),
        ));
    }

    let host = match req[3] {
        0x01 => {
            let mut ip = [0u8; 4];
            client.read_exact(&mut ip).await?;
            std::net::Ipv4Addr::from(ip).to_string()
        }
        0x03 => {
            let mut len = [0u8; 1];
            client.read_exact(&mut len).await?;
            let mut domain = vec![0u8; len[0] as usize];
            client.read_exact(&mut domain).await?;
            String::from_utf8_lossy(&domain).to_string()
        }
        0x04 => {
            let mut ip = [0u8; 16];
            client.read_exact(&mut ip).await?;
            std::net::Ipv6Addr::from(ip).to_string()
        }
        other => {
            return Err(ProxyError::Protocol(format!(
                "Unsupported SOCKS5 ATYP: {}",
                other
            )))
        }
    };

    let mut port_buf = [0u8; 2];
    client.read_exact(&mut port_buf).await?;
    let port = u16::from_be_bytes(port_buf);

    let mut server = match connect_via_upstream(&host, port, upstream.as_ref()).await {
        Ok(s) => s,
        Err(e) => {
            let _ = client
                .write_all(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await;
            return Err(e);
        }
    };

    // 回复连接成功
    client
        .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
    Ok(())
}

/// 通过指定上游代理（HTTP / SOCKS5）或直连建立到目标 host:port 的 TCP 隧道
async fn connect_via_upstream(
    target_host: &str,
    target_port: u16,
    upstream: Option<&ProxyNode>,
) -> Result<TcpStream, ProxyError> {
    match upstream {
        None => Ok(TcpStream::connect(format!("{}:{}", target_host, target_port)).await?),
        Some(node) => match node.protocol {
            ProxyProtocol::Http => {
                let mut stream = TcpStream::connect(format!("{}:{}", node.host, node.port)).await?;
                let connect_req = format!(
                    "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n\r\n",
                    target_host, target_port, target_host, target_port
                );
                stream.write_all(connect_req.as_bytes()).await?;
                let mut resp = [0u8; 1024];
                let n = stream.read(&mut resp).await?;
                let resp_str = String::from_utf8_lossy(&resp[..n]);
                if !resp_str.contains(" 200 ") {
                    return Err(ProxyError::Protocol(format!(
                        "Upstream HTTP CONNECT failed: {}",
                        resp_str.lines().next().unwrap_or("")
                    )));
                }
                Ok(stream)
            }
            ProxyProtocol::Socks5 => {
                let mut stream = TcpStream::connect(format!("{}:{}", node.host, node.port)).await?;
                // 握手（无认证）
                stream.write_all(&[0x05, 0x01, 0x00]).await?;
                let mut resp = [0u8; 2];
                stream.read_exact(&mut resp).await?;
                if resp[0] != 0x05 || resp[1] != 0x00 {
                    return Err(ProxyError::Protocol("Upstream SOCKS5 auth rejected".into()));
                }
                // CONNECT 请求（域名模式 0x03）
                let host_bytes = target_host.as_bytes();
                let mut req = Vec::with_capacity(7 + host_bytes.len());
                req.extend_from_slice(&[0x05, 0x01, 0x00, 0x03, host_bytes.len() as u8]);
                req.extend_from_slice(host_bytes);
                req.extend_from_slice(&target_port.to_be_bytes());
                stream.write_all(&req).await?;

                let mut head = [0u8; 4];
                stream.read_exact(&mut head).await?;
                if head[1] != 0x00 {
                    return Err(ProxyError::Protocol(format!(
                        "Upstream SOCKS5 connect failed, code={}",
                        head[1]
                    )));
                }
                // 消耗剩余绑定地址字节
                match head[3] {
                    0x01 => {
                        let mut skip = [0u8; 6];
                        stream.read_exact(&mut skip).await?;
                    }
                    0x03 => {
                        let mut len = [0u8; 1];
                        stream.read_exact(&mut len).await?;
                        let mut skip = vec![0u8; len[0] as usize + 2];
                        stream.read_exact(&mut skip).await?;
                    }
                    0x04 => {
                        let mut skip = [0u8; 18];
                        stream.read_exact(&mut skip).await?;
                    }
                    _ => {}
                }
                Ok(stream)
            }
        },
    }
}

fn parse_host_port(raw: &str, default_port: u16) -> Result<(String, u16), ProxyError> {
    if let Some((h, p)) = raw.rsplit_once(':') {
        let port = p
            .parse::<u16>()
            .map_err(|_| ProxyError::Protocol(format!("Invalid port in {}", raw)))?;
        Ok((h.trim_matches(['[', ']']).to_string(), port))
    } else {
        Ok((raw.to_string(), default_port))
    }
}
