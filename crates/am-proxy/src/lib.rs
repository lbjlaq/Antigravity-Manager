pub mod detector;
pub mod launcher;
pub mod platform;
pub mod relay;
pub mod types;

pub use detector::{detect_local_proxies, probe_tcp_latency};
pub use launcher::{
    build_proxy_env, get_app_runtime_status, get_ide_settings_proxy, ide_user_settings_path,
    launch_with_proxy, resolve_target_binary, sync_ide_proxy_settings, AppRuntimeStatus,
    DEFAULT_NO_PROXY,
};
pub use platform::{detect_system_proxies, install_shell_hook, uninstall_shell_hook};
pub use relay::{RelayServer, DEFAULT_RELAY_PORT};
pub use types::{ProxyError, ProxyNode, ProxyProtocol, ProxySource, TargetApp};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_proxy_url() {
        let node = ProxyNode::parse_url("http://127.0.0.1:7890", None).unwrap();
        assert_eq!(node.protocol, ProxyProtocol::Http);
        assert_eq!(node.host, "127.0.0.1");
        assert_eq!(node.port, 7890);

        let socks = ProxyNode::parse_url("socks5://127.0.0.1:10808", Some("v2ray".into())).unwrap();
        assert_eq!(socks.protocol, ProxyProtocol::Socks5);
        assert_eq!(socks.name, "v2ray");
    }

    #[tokio::test]
    async fn test_relay_hot_switch() {
        let relay = RelayServer::start("127.0.0.1:0", None).await.unwrap();
        assert!(relay.current_upstream().await.is_none());

        let node = ProxyNode::parse_url("http://127.0.0.1:7890", None).unwrap();
        relay.switch_upstream(Some(node.clone())).await;
        assert_eq!(relay.current_upstream().await, Some(node));

        relay.switch_upstream(None).await;
        assert!(relay.current_upstream().await.is_none());
    }
}
