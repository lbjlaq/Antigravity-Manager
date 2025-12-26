export interface UpstreamProxyConfig {
    enabled: boolean;
    url: string;
}

export interface ProxyConfig {
    enabled: boolean;
    allow_lan_access?: boolean;
    port: number;
    api_key: string;
    auto_start: boolean;
    anthropic_mapping?: Record<string, string>;
    openai_mapping?: Record<string, string>;
    custom_mapping?: Record<string, string>;
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
    antigravity_executable?: string; // [NEW] 手动指定的反重力程序路径
    auto_launch?: boolean; // 开机自动启动
    accounts_page_size?: number; // 账号列表每页显示数量,默认 0 表示自动计算
    proxy: ProxyConfig;
}

