export interface Account {
    id: string;
    email: string;
    name?: string;
    priority?: number;
    token: TokenData;
    device_profile?: DeviceProfile;
    device_history?: DeviceProfileVersion[];
    quota?: QuotaData;
    disabled?: boolean;
    disabled_reason?: string;
    disabled_at?: number;
    proxy_disabled?: boolean;
    proxy_disabled_reason?: string;
    proxy_disabled_at?: number;
    protected_models?: string[];
    live_limited_models?: Record<string, LiveLimitStatus>;
    custom_label?: string;  // 用户自定义标签
    validation_blocked?: boolean;
    validation_blocked_until?: number;
    validation_blocked_reason?: string;
    validation_url?: string;
    created_at: number;
    last_used: number;
}

export interface LiveLimitStatus {
    model: string;
    status: number;
    reason: string;
    until: number;
    detected_at: number;
    message?: string;
}

export interface TokenData {
    access_token: string;
    refresh_token: string;
    expires_in: number;
    expiry_timestamp: number;
    token_type: string;
    email?: string;
}

export interface QuotaData {
    models: ModelQuota[];
    last_updated: number;
    is_forbidden?: boolean;
    forbidden_reason?: string;
    subscription_tier?: string;  // 订阅类型: FREE/PRO/ULTRA
    model_forwarding_rules?: Record<string, string>; // 废弃模型转发表
    quota_groups?: QuotaGroup[]; // 按模型组的配额摘要 (weekly + 5h 双窗口)
}

export interface ModelQuota {
    name: string;
    percentage: number;
    reset_time: string;
    display_name?: string;
    supports_images?: boolean;
    supports_thinking?: boolean;
    thinking_budget?: number;
    recommended?: boolean;
    max_tokens?: number;
    max_output_tokens?: number;
    supported_mime_types?: Record<string, boolean>;
}

/** 单个配额桶 (weekly / 5h) */
export interface QuotaBucket {
    bucket_id: string;
    window: string;  // "weekly" | "5h"
    remaining_fraction: number;
    reset_time: string;
    cycle_tokens?: number; // 本实例在该周配额周期记录的 input + output Token
    display_name?: string;
    description?: string;
}

/** 模型组配额 (如 Gemini Models / Claude and GPT models) */
export interface QuotaGroup {
    display_name: string;
    description?: string;
    buckets?: QuotaBucket[];
}

export interface DeviceProfile {
    machine_id: string;
    mac_machine_id: string;
    dev_device_id: string;
    sqm_id: string;
}

export interface DeviceProfileVersion {
    id: string;
    created_at: number;
    label: string;
    profile: DeviceProfile;
    is_current?: boolean;
}

/**
 * 解析账号的订阅等级 ('ultra' | 'pro' | 'free')
 *
 * 只依据 `quota.subscription_tier` —— 即后端 `loadCodeAssist` 回填的权威值。
 * 关键词表与 Rust 侧 `normalize_subscription_tier` 保持严格一致，避免前后端判定分叉。
 *
 * ⚠️ 不要恢复「模型列表兜底」：实测 `fetchAvailableModels` 对免费号与 Pro 号返回
 * **完全相同的全量目录**（都含 claude-* / gpt-*，且 remainingFraction 恒为 1），
 * 用它推断会把所有免费号判成 Pro —— 这正是「free 账号被标记成 pro」的根因。
 */
export function getAccountTier(account: { quota?: QuotaData | null }): 'ultra' | 'pro' | 'free' {
    const rawTier = account.quota?.subscription_tier?.trim().toLowerCase();
    if (!rawTier) return 'free';

    // Ultra 档（"helium" 是 Ultra 的内部代号：GDP_HELIUM / GOOGLE_ONE_HELIUM）
    if (rawTier.includes('ultra') || rawTier.includes('helium')) return 'ultra';
    // 免费档：free-tier / "Antigravity Starter Quota"
    if (rawTier.includes('free') || rawTier.includes('starter')) return 'free';
    // 付费档：g1-pro-tier / standard-tier / "Google AI Pro" / premium / advanced
    if (
        rawTier.includes('pro') ||
        rawTier.includes('premium') ||
        rawTier.includes('advanced') ||
        rawTier.includes('standard')
    ) {
        return 'pro';
    }

    // 未识别：按最低档处理，等待后端回填权威值（绝不猜成 pro）
    return 'free';
}

/**
 * 订阅等级的展示文案。所有徽标都应使用它，避免直接把后端的原始字符串
 * （如 "Antigravity Starter Quota"）渲染到界面上。
 */
export function getTierLabel(tier: 'ultra' | 'pro' | 'free'): string {
    return tier.toUpperCase();
}
