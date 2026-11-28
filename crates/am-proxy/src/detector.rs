use crate::platform;
use crate::types::{ProxyNode, ProxyProtocol, ProxySource};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// 常见本地 VPN / 代理客户端默认端口表
const KNOWN_LOCAL_PROXIES: &[(&str, u16, ProxyProtocol)] = &[
    ("Shadowrocket", 1082, ProxyProtocol::Http),
    ("Clash / Mihomo", 7890, ProxyProtocol::Http),
    ("Clash Verge", 7897, ProxyProtocol::Http),
    ("Clash SOCKS", 7891, ProxyProtocol::Socks5),
    ("v2rayN / Nekoray HTTP", 10809, ProxyProtocol::Http),
    ("v2rayN / Nekoray SOCKS", 10808, ProxyProtocol::Socks5),
    ("Surge HTTP", 6152, ProxyProtocol::Http),
    ("Surge SOCKS", 6153, ProxyProtocol::Socks5),
    ("Shadowsocks / Xray", 1080, ProxyProtocol::Socks5),
    ("ShadowsocksX-NG", 1086, ProxyProtocol::Socks5),
    ("Sing-box / Nekobox", 2080, ProxyProtocol::Http),
    ("Local HTTP Proxy", 8080, ProxyProtocol::Http),
];

/// 探测单个端口的 TCP 连通性与延迟（毫秒）
pub async fn probe_tcp_latency(host: &str, port: u16, timeout_ms: u64) -> Option<u64> {
    let start = Instant::now();
    let addr = format!("{}:{}", host, port);
    match timeout(Duration::from_millis(timeout_ms), TcpStream::connect(&addr)).await {
        Ok(Ok(_stream)) => Some(start.elapsed().as_millis() as u64),
        _ => None,
    }
}

/// 自动探测本机所有可用的系统代理、运行中的 VPN 进程监听端口与常见默认端口
pub async fn detect_local_proxies() -> Vec<ProxyNode> {
    let mut candidates = platform::detect_system_proxies();
    candidates.extend(platform::detect_listening_proxy_processes());

    for &(name, port, protocol) in KNOWN_LOCAL_PROXIES {
        candidates.push(ProxyNode {
            name: name.to_string(),
            host: "127.0.0.1".to_string(),
            port,
            protocol,
            username: None,
            password: None,
            latency_ms: None,
            source: ProxySource::AutoDetected,
        });
    }

    // 去重（优先保留系统代理标记）
    let mut seen = HashSet::new();
    let mut unique_candidates = Vec::new();
    for node in candidates {
        let key = (node.host.clone(), node.port);
        if seen.insert(key) {
            unique_candidates.push(node);
        }
    }

    // 并发探测端口存活与延迟
    let mut handles = Vec::with_capacity(unique_candidates.len());
    for mut node in unique_candidates {
        handles.push(tokio::spawn(async move {
            if let Some(latency) = probe_tcp_latency(&node.host, node.port, 350).await {
                node.latency_ms = Some(latency);
                Some(node)
            } else {
                None
            }
        }));
    }

    let mut available = Vec::new();
    for h in handles {
        if let Ok(Some(node)) = h.await {
            available.push(node);
        }
    }

    // 系统代理优先，其次按延迟升序
    available.sort_by(|a, b| {
        let a_sys = matches!(a.source, ProxySource::System);
        let b_sys = matches!(b.source, ProxySource::System);
        b_sys.cmp(&a_sys).then_with(|| {
            a.latency_ms
                .unwrap_or(u64::MAX)
                .cmp(&b.latency_ms.unwrap_or(u64::MAX))
        })
    });

    available
}
