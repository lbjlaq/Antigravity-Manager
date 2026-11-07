export type ModelPriorityGroup = 'gemini' | 'claude';

export interface ModelPriorities {
    gemini?: number;
    claude?: number;
}

export interface Account {
    id: string;
    email: string;
    name?: string;
    priority?: number;
    model_priorities?: ModelPriorities;
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
    qos_status?: 'suspicious' | 'jailed' | 'probing' | 'half_open' | null; // 运行时 QoS 状态
    qos_reset_seconds?: number | null; // 运行时 QoS 冷却剩余秒数
    validation_blocked?: boolean;
    validation_blocked_until?: number;
    validation_blocked_reason?: string;
    validation_url?: string;
    /** 上游已确认的账号封禁（如 TOS_VIOLATION），只有生产端点探测恢复后才会清除 */
    banned?: boolean;
    banned_code?: string;
    banned_reason?: string;
    banned_at?: number;
    ban_appeal_url?: string;
    created_at: number;
    last_used: number;
}

export interface ActiveClientsStatus {
    classic_account_id?: string | null;
    classic_email?: string | null;
    ide_account_id?: string | null;
    ide_email?: string | null;
    agy_account_id?: string | null;
    agy_email?: string | null;
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
    project_id?: string;
    is_gcp_tos?: boolean;
    oauth_client_key?: string;
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
export function getAccountTier(account: { quota?: QuotaData | null; token?: { is_gcp_tos?: boolean; oauth_client_key?: string } }): 'ultra' | 'pro' | 'enterprise' | 'free' {
    const rawTier = account.quota?.subscription_tier?.trim().toLowerCase();
    const clientKey = account.token?.oauth_client_key?.trim().toLowerCase();
    const isEnterpriseClient = Boolean(clientKey && (clientKey === 'antigravity_gcp' || clientKey.includes('gcp') || clientKey.includes('vertex')));

    if (rawTier?.includes('enterprise') || rawTier?.startsWith('gcp_') || isEnterpriseClient) {
        return 'enterprise';
    }

    if (!rawTier) return 'free';

    // Ultra 档（"helium" 是 Ultra 的内部代号：GDP_HELIUM / GOOGLE_ONE_HELIUM）
    if (rawTier.includes('ultra') || rawTier.includes('helium')) return 'ultra';
    // 免费档：free-tier / "Antigravity Starter Quota"
    if (rawTier.includes('free') || rawTier.includes('starter')) return 'free';
    // 付费档：g1-pro-tier / "Google AI Pro" / premium / advanced
    if (
        rawTier.includes('pro') ||
        rawTier.includes('premium') ||
        rawTier.includes('advanced')
    ) {
        return 'pro';
    }

    // 未识别：按最低档处理，等待后端回填权威值（绝不猜成 pro）
    return 'free';
}

/**
 * 统一收敛判定：该账号是否具备 Gemini 4 Argon 授权能力
 * 涵盖两种套餐的 Argon 权威判据：
 * 1. Ultra 旗舰套餐 (Google AI Ultra / Helium)：
 *    Ultra 订阅上游全量授权 Argon（纳入高阶特权共享池），即便 quota_groups 未拆分独立桶，也原生享有 Argon 完整权限；
 * 2. Pro 套餐或灰度专属分组 (Google AI Pro)：
 *    上游在真实 quota_groups 中显式下发了 Argon 专属配额组（如 gemini-argon-wtu-weekly），
 *    或（无 groups 离线兜底时）models 列表中明确包含非禁用的 Argon 官方条目；
 * 3. Free / 未授权套餐：
 *    未分配专属分组且非 Ultra，坚决判定为 false（严格防呆隔离，杜绝虚假徽章与卡片）。
 */
export function hasAccountArgonCapability(account: { quota?: QuotaData | null }): boolean {
    const tier = getAccountTier(account);
    const groups = account.quota?.quota_groups || [];
    const models = account.quota?.models || [];
    return checkArgonEligibility(groups, models, tier);
}

/**
 * 依据配额组、模型列表与订阅等级综合校验 Argon 准入资格
 */
export function checkArgonEligibility(
    groups: QuotaGroup[] = [],
    models: ModelQuota[] = [],
    tier?: 'ultra' | 'pro' | 'enterprise' | 'free'
): boolean {
    // 判据 1: Ultra 旗舰套餐 / Enterprise 企业授权（原生全量支持 Argon，共享或内置高阶池）
    if (tier === 'ultra' || tier === 'enterprise') {
        const hasExplicitDisabled = models.some(m => {
            const n = (m?.name || '').toLowerCase();
            return /argon/i.test(n) && (n.includes('-disabled') || n.endsWith('_disabled') || (m as any)?.disabled);
        });
        if (hasExplicitDisabled) return false;
        return true;
    }

    // 判据 2: Pro 套餐或灰度授权（必须在真实 quota_groups 中分配了专属 Argon 分组/桶）
    const hasArgonInGroups = (groups || []).some(group => {
        const groupName = (group?.display_name || '').toLowerCase();
        return /argon/i.test(groupName)
            || (group?.buckets || []).some(b => /argon/i.test(`${b?.bucket_id || ''} ${b?.display_name || ''}`));
    });

    if (hasArgonInGroups) {
        return true;
    }

    // 判据 3: 无 groups 数据时的离线/冷启动兜底（仅当服务端未返回任何 groups，且 models 明确包含非禁用 Argon 模型）
    if (!groups || groups.length === 0) {
        return models.some(m => {
            const n = (m?.name || '').toLowerCase();
            return /argon/i.test(n) && !n.includes('-disabled') && !n.endsWith('_disabled') && !(m as any)?.disabled;
        });
    }

    // 其余情况（如 Free 账号但仅有普通 Gemini 组，无 Argon 专属组）：坚决判定为 false (Fixes #3745)
    return false;
}

/**
 * 订阅等级的展示文案。所有徽标都应使用它，避免直接把后端的原始字符串
 * （如 "Antigravity Starter Quota"）渲染到界面上。
 */
export function getTierLabel(tier: 'ultra' | 'pro' | 'enterprise' | 'free'): string {
    return tier.toUpperCase();
}

export function getEffectiveModelPriority(
    account: { priority?: number; model_priorities?: ModelPriorities },
    group: ModelPriorityGroup
): number {
    const override = account.model_priorities?.[group];
    if (typeof override === 'number' && override >= 1 && override <= 100) {
        return override;
    }
    return typeof account.priority === 'number' && account.priority >= 1 && account.priority <= 100
        ? account.priority
        : 50;
}

export function hasCustomModelPriorities(account: {
    priority?: number;
    model_priorities?: ModelPriorities;
}): boolean {
    if (!account.model_priorities) return false;
    const base = typeof account.priority === 'number' ? account.priority : 50;
    const g = account.model_priorities.gemini;
    const c = account.model_priorities.claude;
    return (typeof g === 'number' && g !== base) || (typeof c === 'number' && c !== base);
}

export interface ImportAccountEntry {
    email?: string;
    refresh_token: string;
}

export interface BatchImportProgress {
    current: number;
    total: number;
    success: number;
    failed: number;
    current_email?: string;
}

export interface BatchImportResult {
    total: number;
    success: number;
    failed: number;
    errors: string[];
}
