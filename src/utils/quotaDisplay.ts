import type { ModelQuota, QuotaGroup } from '../types/account';

export type DashboardQuotaView = 'weighted' | '5h' | 'weekly';

export interface ConstrainedQuotaResult {
    /** 最终在当前所选模式下应展示的有效百分比 (0-100) */
    effectivePercentage: number;
    /** 原始 5H 滑动窗口配额百分比 (0-100, 或 null) */
    raw5h: number | null;
    /** 原始 7 天周配额百分比 (0-100, 或 null) */
    rawWeekly: number | null;
    /** 综合短板/加权基准值 (0-100) */
    weighted: number;
    /** 依据当前模式和约束状态计算出的建议重置时间 */
    resetTime?: string;
    /** 是否受到周配额短板压制 (例如 5H 本有 80% 但周配额仅剩 30%，上限被压至 30%) */
    isWeeklyConstrained: boolean;
    /** 是否因周配额见底 (<=0.1%) 导致彻底熔断归零 */
    isWeeklyExhausted: boolean;
    /** 是否处于 5H 瞬时冷却态 (周配额虽有，但当前 5H 窗口打满归零) */
    is5hCooling: boolean;
    /** 5H 窗口专属重置时间 */
    fiveHourResetTime?: string;
    /** 7天周配额专属重置时间 */
    weeklyResetTime?: string;
}

// An exhausted weekly window takes precedence; protection still uses the backend quota.
export function getModelQuotaDisplay(modelId: string, model: ModelQuota | undefined, groups: QuotaGroup[] = []) {
    const name = modelId.toLowerCase();
    const thirdParty = name.startsWith('claude') || name.startsWith('gpt');
    const buckets = (groups || []).filter(group => {
        const groupName = (group?.display_name || '').toLowerCase();
        const isThirdParty = /claude|gpt|3p/.test(groupName)
            || (group?.buckets || []).some(bucket => bucket?.bucket_id?.toLowerCase().includes('3p'));
        return thirdParty ? isThirdParty : name.startsWith('gemini') && !isThirdParty;
    }).flatMap(group => group?.buckets || []);
    const fiveHour = buckets.filter(bucket => /5h|hour/i.test(`${bucket.window} ${bucket.bucket_id}`))
        .reduce<(typeof buckets)[number] | undefined>((chosen, bucket) =>
            !chosen || bucket.remaining_fraction < chosen.remaining_fraction ? bucket : chosen, undefined);
    const weekly = buckets.filter(bucket => /week|7d/i.test(`${bucket.window} ${bucket.bucket_id}`)
        && bucket.remaining_fraction <= 0.001 && Date.parse(bucket.reset_time) > Date.now())
        .reduce<(typeof buckets)[number] | undefined>((chosen, bucket) =>
            !chosen || Date.parse(bucket.reset_time) > Date.parse(chosen.reset_time) ? bucket : chosen, undefined);
    return {
        percentage: weekly ? 0 : fiveHour ? Math.round(fiveHour.remaining_fraction * 100) : (model?.percentage ?? 0),
        resetTime: weekly?.reset_time || fiveHour?.reset_time || model?.reset_time,
        isWeeklyConstrained: !!weekly,
        weeklyResetTime: weekly?.reset_time,
    };
}

/**
 * 计算模型在指定视角 (综合加权 / 5H滚动 / 7天周配额) 下的双向木桶效应约束配额
 */
export function getModelConstrainedQuota(
    modelId: string,
    model: ModelQuota | undefined,
    groups: QuotaGroup[] = [],
    view: DashboardQuotaView = 'weighted'
): ConstrainedQuotaResult {
    const name = modelId.toLowerCase();
    const thirdParty = name.startsWith('claude') || name.startsWith('gpt');

    const buckets = (groups || []).filter(group => {
        const groupName = (group?.display_name || '').toLowerCase();
        const isThirdParty = /claude|gpt|3p/.test(groupName)
            || (group?.buckets || []).some(bucket => bucket?.bucket_id?.toLowerCase().includes('3p'));
        return thirdParty ? isThirdParty : name.startsWith('gemini') && !isThirdParty;
    }).flatMap(group => group?.buckets || []);

    const fiveHourBucket = buckets.filter(bucket => /5h|hour/i.test(`${bucket?.window || ''} ${bucket?.bucket_id || ''}`))
        .reduce<(typeof buckets)[number] | undefined>((chosen, bucket) =>
            !chosen || bucket.remaining_fraction < chosen.remaining_fraction ? bucket : chosen, undefined);

    const weeklyBucket = buckets.filter(bucket => /week|7d/i.test(`${bucket?.window || ''} ${bucket?.bucket_id || ''}`))
        .reduce<(typeof buckets)[number] | undefined>((chosen, bucket) =>
            !chosen || bucket.remaining_fraction < chosen.remaining_fraction ? bucket : chosen, undefined);

    // 1. 提取原始值
    const raw5h = fiveHourBucket && typeof fiveHourBucket.remaining_fraction === 'number'
        ? Math.round(fiveHourBucket.remaining_fraction * 100)
        : (typeof model?.percentage === 'number' ? model.percentage : null);

    const rawWeekly = weeklyBucket && typeof weeklyBucket.remaining_fraction === 'number'
        ? Math.round(weeklyBucket.remaining_fraction * 100)
        : null;

    const fiveHourResetTime = fiveHourBucket?.reset_time || model?.reset_time;
    const weeklyResetTime = weeklyBucket?.reset_time;

    // 2. 状态判定
    const isWeeklyExhausted = rawWeekly !== null && rawWeekly <= 0;
    const isWeeklyConstrained = isWeeklyExhausted || (rawWeekly !== null && raw5h !== null && rawWeekly < raw5h);
    const is5hCooling = raw5h !== null && raw5h <= 0 && (rawWeekly === null || rawWeekly > 0);

    // 3. 计算综合短板基准
    let weighted = 0;
    if (isWeeklyExhausted) {
        weighted = 0;
    } else if (raw5h !== null && rawWeekly !== null) {
        weighted = Math.min(raw5h, rawWeekly);
    } else if (rawWeekly !== null) {
        weighted = rawWeekly;
    } else if (raw5h !== null) {
        weighted = raw5h;
    }

    // 4. 根据当前模式实施双向木桶约束
    let effectivePercentage = weighted;
    let resetTime = fiveHourResetTime || weeklyResetTime;

    if (view === '5h') {
        // 5H 模式：展示真实 5H 滚动配额；若周配额耗尽 (<=0) 则触发熔断归零，若周配额偏紧则通过 isWeeklyConstrained 标识提示
        if (isWeeklyExhausted) {
            effectivePercentage = 0;
            resetTime = weeklyResetTime || fiveHourResetTime;
        } else {
            effectivePercentage = raw5h ?? rawWeekly ?? 0;
            resetTime = fiveHourResetTime || weeklyResetTime;
        }
    } else if (view === 'weekly') {
        // 周配额模式：展示宏观周配额，但如果 5H 耗尽进入冷却，提示即时受限
        effectivePercentage = rawWeekly ?? raw5h ?? 0;
        resetTime = weeklyResetTime || fiveHourResetTime;
    } else {
        // 综合模式：木桶短板有效值
        effectivePercentage = weighted;
        resetTime = isWeeklyExhausted ? weeklyResetTime : (fiveHourResetTime || weeklyResetTime);
    }

    return {
        effectivePercentage,
        raw5h,
        rawWeekly,
        weighted,
        resetTime,
        isWeeklyConstrained,
        isWeeklyExhausted,
        is5hCooling,
        fiveHourResetTime,
        weeklyResetTime,
    };
}
