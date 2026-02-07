// File: src/entities/config/types.ts
// Configuration domain types

export interface UpstreamProxyConfig {
  enabled: boolean;
  url: string;
}

export interface ProxyConfig {
  enabled: boolean;
  allow_lan_access?: boolean;
  auth_mode?: "off" | "strict" | "all_except_health" | "auto";
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
}

export interface DebugLoggingConfig {
  enabled: boolean;
  output_dir?: string;
}

export type SchedulingMode =
  | "CacheFirst"
  | "Balance"
  | "PerformanceFirst"
  | "Selected"
  | "P2C";

export interface StickySessionConfig {
  mode: SchedulingMode;
  max_wait_seconds: number;
  selected_accounts: string[];
  selected_models: Record<string, string[]>;
  strict_selected: boolean;
}

export type ZaiDispatchMode = "off" | "exclusive" | "pooled" | "fallback";

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
  threshold_percentage: number;
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
  antigravity_executable?: string;
  antigravity_args?: string[];
  auto_launch?: boolean;
  auto_check_update?: boolean;
  update_check_interval?: number;
  accounts_page_size?: number;
  debug_console_enabled?: boolean;
  show_proxy_selected_badge?: boolean;
  scheduled_warmup: ScheduledWarmupConfig;
  quota_protection: QuotaProtectionConfig;
  pinned_quota_models: PinnedQuotaModelsConfig;
  circuit_breaker: CircuitBreakerConfig;
  validation_block_minutes?: number;
  proxy: ProxyConfig;
}

export type TunnelMode = "quick" | "auth";

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
