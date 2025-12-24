export interface UpstreamProxyConfig {
    enabled: boolean;
    url: string;
}

export interface ProxyConfig {
    enabled: boolean;
    port: number;
    api_key: string;
    auto_start: boolean;
    anthropic_mapping?: Record<string, string>;
    request_timeout: number;
    upstream_proxy: UpstreamProxyConfig;
}

export interface AppConfig {
    language: string;
    theme: string;
    auto_refresh: boolean;
    refresh_interval: number;
    auto_sync: boolean;
    sync_interval: number;
    default_export_path?: string;
    proxy: ProxyConfig;
}
