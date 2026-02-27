export interface UpstreamProxyConfig {
    enabled: boolean;
    url: string;
}

export interface ProxyConfig {
    enabled: boolean;
    allow_lan_access?: boolean;
    auth_mode?: 'off' | 'strict' | 'all_except_health' | 'auto';
    port: number;
    api_key: string;
    admin_password?: string;
    auto_start: boolean;
    custom_mapping?: Record<string, string>;
    request_timeout: number;
    enable_logging: boolean;
    debug_logging?: DebugLoggingConfig;
    upstream_proxy: UpstreamProxyConfig;
    zai?: ZaiConfig;
    scheduling?: StickySessionConfig;
    experimental?: ExperimentalConfig;
    user_agent_override?: string;
    saved_user_agent?: string;
    thinking_budget?: ThinkingBudgetConfig;
    global_system_prompt?: GlobalSystemPromptConfig;
    image_thinking_mode?: 'enabled' | 'disabled'; // [NEW] 图像思维模式开关
    proxy_pool?: ProxyPoolConfig;
}

// ============================================================================
// Thinking Budget 配置 (控制 AI 深度思考时的 Token 预算)
// ============================================================================

/** Thinking Budget 处理模式 */
export type ThinkingBudgetMode = 'auto' | 'passthrough' | 'custom' | 'adaptive'; // [NEW] 支持自适应模式

/** Thinking Effort 等级 (仅 adaptive 模式) */
export type ThinkingEffort = 'low' | 'medium' | 'high';

/** Thinking Budget 配置 */
export interface ThinkingBudgetConfig {
    /** 模式选择 */
    mode: ThinkingBudgetMode;
    /** 自定义固定值（仅在 mode=custom 时生效），范围 1024-65536 */
    custom_value: number;
    /** 思考强度 (仅在 mode=adaptive 时生效) */
    effort?: ThinkingEffort;
}

// ============================================================================
// 全局系统提示词配置
// ============================================================================

/** 全局系统提示词配置 */
export interface GlobalSystemPromptConfig {
    /** 是否启用 */
    enabled: boolean;
    /** 提示词内容 */
    content: string;
}

export interface DebugLoggingConfig {
    enabled: boolean;
    output_dir?: string;
}

export type SchedulingMode = 'CacheFirst' | 'Balance' | 'PerformanceFirst';

export interface StickySessionConfig {
    mode: SchedulingMode;
    max_wait_seconds: number;
}

export type ZaiDispatchMode = 'off' | 'exclusive' | 'pooled' | 'fallback';

export interface ZaiMcpConfig {
    enabled: boolean;
    web_search_enabled: boolean;
    web_reader_enabled: boolean;
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

export interface ScheduledWarmupConfig {
    enabled: boolean;
    monitored_models: string[];
}

export interface QuotaProtectionConfig {
    enabled: boolean;
    threshold_percentage: number; // 1-99
    monitored_models: string[];
}

export interface PinnedQuotaModelsConfig {
    models: string[];
}

export interface ExperimentalConfig {
    enable_usage_scaling: boolean;
    context_compression_threshold_l1?: number;
    context_compression_threshold_l2?: number;
    context_compression_threshold_l3?: number;
}

export interface CircuitBreakerConfig {
    enabled: boolean;
    backoff_steps: number[];
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
    antigravity_args?: string[]; // [NEW] Antigravity 启动参数
    auto_launch?: boolean; // 开机自动启动
    auto_check_update?: boolean; // 自动检查更新
    update_check_interval?: number; // 更新检查间隔（小时）
    accounts_page_size?: number; // 账号列表每页显示数量,默认 0 表示自动计算
    hidden_menu_items?: string[]; // 隐藏的菜单项路径列表
    scheduled_warmup: ScheduledWarmupConfig;
    quota_protection: QuotaProtectionConfig; // [NEW] 配额保护配置
    pinned_quota_models: PinnedQuotaModelsConfig; // [NEW] 配额关注列表
    circuit_breaker: CircuitBreakerConfig; // [NEW] 熔断器配置
    proxy: ProxyConfig;
    cloudflared: CloudflaredConfig; // [NEW] Cloudflared 配置
}

// ============================================================================
// Cloudflared (CF隧道) 类型定义
// ============================================================================

export type TunnelMode = 'quick' | 'auth';

export interface CloudflaredConfig {
    enabled: boolean;
    mode: TunnelMode;
    port: number;
    token?: string;
    use_http2: boolean;
}

export interface CloudflaredStatus {
    installed: boolean;
    version?: string;
    running: boolean;
    url?: string;
    error?: string;
}

// ============================================================================
// 代理池类型定义
// ============================================================================

export interface ProxyAuth {
    username: string;
    password?: string;
}

export interface ProxyEntry {
    id: string;
    name: string;
    url: string;
    auth?: ProxyAuth;
    enabled: boolean;
    priority: number;
    tags: string[];
    max_accounts?: number;
    health_check_url?: string;
    last_check_time?: number;
    is_healthy: boolean;
    latency?: number; // [NEW] 延迟 (毫秒)
}

// export type ProxyPoolMode = 'global' | 'per_account' | 'hybrid'; // [REMOVED]

export type ProxySelectionStrategy = 'round_robin' | 'random' | 'priority' | 'least_connections' | 'weighted_round_robin';

export interface ProxyPoolConfig {
    enabled: boolean;
    // mode: ProxyPoolMode; // [REMOVED]
    proxies: ProxyEntry[];
    health_check_interval: number;
    auto_failover: boolean;
    strategy: ProxySelectionStrategy;
    account_bindings?: Record<string, string>;
}
