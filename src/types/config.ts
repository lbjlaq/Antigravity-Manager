export interface UpstreamProxyConfig {
    enabled: boolean;
    url: string;
}

export interface ProxyConfig {
    enabled: boolean;
    allow_lan_access?: boolean;
    auth_mode?: 'off' | 'strict' | 'all_except_health' | 'auto';
    access_log_enabled?: boolean;
    port: number;
    api_key: string;
    auto_start: boolean;
    anthropic_mapping?: Record<string, string>;
    openai_mapping?: Record<string, string>;
    custom_mapping?: Record<string, string>;
    request_timeout: number;
    enable_logging: boolean;
    upstream_proxy: UpstreamProxyConfig;
    zai?: ZaiConfig;
}

export type ZaiDispatchMode = 'off' | 'exclusive' | 'pooled' | 'fallback';

export interface ZaiMcpConfig {
    enabled: boolean;
    web_search_enabled: boolean;
    web_reader_enabled: boolean;
    zread_enabled: boolean;
    api_key_override?: string;
    web_reader_url_normalization?: 'off' | 'strip_tracking_query' | 'strip_query';
    vision_enabled: boolean;
}

export interface ZaiModelDefaults {
    opus: string;
    sonnet: string;
    haiku: string;
}

export interface ZaiConfig {
    enabled: boolean;
    base_url: string;
    api_key: string;
    dispatch_mode: ZaiDispatchMode;
    model_mapping?: Record<string, string>;
    models: ZaiModelDefaults;
    mcp: ZaiMcpConfig;
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
