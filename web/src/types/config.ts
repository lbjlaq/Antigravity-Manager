export interface UpstreamProxyConfig {
    enabled: boolean;
    url: string;
}

export interface ProxyConfig {
    enabled?: boolean;
    port: number;
    apiKey: string;
    autoStart: boolean;
    modelMapping: Record<string, string>;
    requestTimeout: number;
    upstreamProxy?: UpstreamProxyConfig;
}

export interface AppConfig {
    language: string;
    theme: string;
    autoRefresh?: boolean;
    refreshInterval?: number;
    autoSync?: boolean;
    syncInterval?: number;
    defaultExportPath?: string;
    proxy: ProxyConfig;
}

