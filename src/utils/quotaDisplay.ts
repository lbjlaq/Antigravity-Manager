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
    const isArgon = name.includes('argon');
    const thirdParty = !isArgon && (name.startsWith('claude') || name.startsWith('gpt'));
    const buckets = (groups || []).filter(group => {
        const groupName = (group?.display_name || '').toLowerCase();
        const hasArgonBucket = (group?.buckets || []).some(bucket => bucket?.bucket_id?.toLowerCase().includes('argon'));
        const groupIsArgon = groupName.includes('argon') || hasArgonBucket;
        if (isArgon) return groupIsArgon;
        if (groupIsArgon) return false;

        const isThirdParty = /claude|gpt|3p/.test(groupName)
            || (group?.buckets || []).some(bucket => bucket?.bucket_id?.toLowerCase().includes('3p'));
        return thirdParty ? isThirdParty : name.startsWith('gemini') && !isThirdParty;
    }).flatMap(group => group?.buckets || []);
    const hasGroups = (groups || []).length > 0;
    if (isArgon && hasGroups && buckets.length === 0) {
        return {
            percentage: 0,
            resetTime: undefined,
            isWeeklyConstrained: false,
            weeklyResetTime: undefined,
        };
    }
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
    const isArgon = name.includes('argon');
    const thirdParty = !isArgon && (name.startsWith('claude') || name.startsWith('gpt'));

    const buckets = (groups || []).filter(group => {
        const groupName = (group?.display_name || '').toLowerCase();
        const hasArgonBucket = (group?.buckets || []).some(bucket => bucket?.bucket_id?.toLowerCase().includes('argon'));
        const groupIsArgon = groupName.includes('argon') || hasArgonBucket;
        if (isArgon) return groupIsArgon;
        if (groupIsArgon) return false;

        const isThirdParty = /claude|gpt|3p/.test(groupName)
            || (group?.buckets || []).some(bucket => bucket?.bucket_id?.toLowerCase().includes('3p'));
        return thirdParty ? isThirdParty : name.startsWith('gemini') && !isThirdParty;
    }).flatMap(group => group?.buckets || []);

    const hasGroups = (groups || []).length > 0;
    if (isArgon && hasGroups && buckets.length === 0) {
        return {
            effectivePercentage: 0,
            raw5h: null,
            rawWeekly: null,
            weighted: 0,
            resetTime: undefined,
            isWeeklyConstrained: false,
            isWeeklyExhausted: false,
            is5hCooling: false,
            fiveHourResetTime: undefined,
            weeklyResetTime: undefined,
        };
    }

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

export interface GroupQuotaItem {
    percentage: number;
    resetTime?: string;
    cycleTokens?: number | null;
}

export interface GroupQuotaSummary {
    key: 'gemini' | 'claude' | 'argon';
    labelKey: string;
    defaultLabel: string;
    dynamicLabel?: string;
    supportedModels?: string[];
    fiveHour?: GroupQuotaItem;
    weekly?: GroupQuotaItem;
    isWeeklyConstrained: boolean;
    isWeeklyExhausted: boolean;
}

function isSupportedDisplayModel(name: string): boolean {
    const m = name.trim().toLowerCase();
    if (
        m.includes('-disabled') ||
        m.endsWith('_disabled') ||
        m.startsWith('chat_') ||
        m.startsWith('tab_') ||
        m.includes('internal') ||
        m.includes('-exp') ||
        m.endsWith('-agent') ||
        m === 'gemini-3-flash' ||
        m.includes('2.5') ||
        m.includes('2-5') ||
        m.includes('3.5-flash-low') ||
        m.includes('3.5-flash-extra-low') ||
        m.includes('3.6-flash') ||
        m.includes('3.7-flash')
    ) {
        return false;
    }
    return true;
}

function summarizeSupportedModels(models: ModelQuota[], key: 'gemini' | 'claude' | 'argon'): { dynamicLabel?: string; supportedModels: string[] } {
    const active = models
        .map(m => m.name.trim().toLowerCase())
        .filter(isSupportedDisplayModel);

    if (active.length === 0) {
        return { supportedModels: [] };
    }

    if (key === 'argon') {
        return {
            dynamicLabel: 'Gemini 4 Argon',
            supportedModels: active,
        };
    }

    if (key === 'claude') {
        // 通用动态探测任意代数的 Claude 版本（如 4.6, 5.5, 6.0 等）
        const versions = new Set<string>();
        for (const n of active) {
            if (n.includes('claude')) {
                const match = n.match(/claude-[a-z]+-(\d+)[.-](\d+)/i) || n.match(/claude-(\d+)[.-](\d+)/i);
                if (match) {
                    versions.add(`${match[1]}.${match[2]}`);
                }
            }
        }
        const sortedVersions = Array.from(versions).sort((a, b) => parseFloat(b) - parseFloat(a));
        const hasGpt = active.some(n => n.startsWith('gpt'));
        let dynamicLabel: string | undefined;
        if (sortedVersions.length > 0) {
            const verStr = sortedVersions.join('&');
            dynamicLabel = hasGpt ? `Claude ${verStr} / GPT` : `Claude ${verStr}`;
        }
        return {
            dynamicLabel,
            supportedModels: active,
        };
    }

    // 通用动态探测 Gemini 主版本与小版本分布（如 3.8, 3.1, 4.0 等）
    const versions = new Set<string>();
    for (const n of active) {
        if (n.startsWith('gemini-')) {
            const match = n.match(/gemini-(\d+(?:\.\d+)?)/i);
            if (match) {
                versions.add(match[1]);
            }
        }
    }
    const sortedVersions = Array.from(versions).sort((a, b) => parseFloat(b) - parseFloat(a));
    let dynamicLabel: string | undefined;
    if (sortedVersions.length > 0) {
        dynamicLabel = `Gemini ${sortedVersions.slice(0, 2).join(' / ')}`;
    }
    return {
        dynamicLabel,
        supportedModels: active,
    };
}

/**
 * 提取 Gemini Models、Claude & GPT models 以及 Gemini 4 Argon 的聚合配额
 * @param groups 配额组数据 (quota_groups)
 * @param fallbackModels 兜底模型列表 (models)
 * @param tier 账号订阅类型 ('ultra' | 'pro' | 'free')
 */
export function getGroupQuotaSummaries(
    groups: QuotaGroup[] = [],
    fallbackModels: ModelQuota[] = [],
    tier?: 'ultra' | 'pro' | 'free'
): GroupQuotaSummary[] {
    const isFreeTier = tier === 'free';

    const findGroupBuckets = (target: 'gemini' | 'claude' | 'argon') => {
        return (groups || []).filter(group => {
            const groupName = (group?.display_name || '').toLowerCase();
            const hasArgonBucket = (group?.buckets || []).some(b => /argon/i.test(`${b?.bucket_id || ''} ${b?.display_name || ''}`));
            const isArgon = /argon/i.test(groupName) || hasArgonBucket;
            if (target === 'argon') return isArgon;
            if (isArgon) return false;

            const isThirdParty = /claude|gpt|3p/.test(groupName)
                || (group?.buckets || []).some(bucket => bucket?.bucket_id?.toLowerCase().includes('3p'));
            return target === 'claude' ? isThirdParty : !isThirdParty;
        }).flatMap(group => group?.buckets || []);
    };

    const buildGroup = (key: 'gemini' | 'claude' | 'argon'): GroupQuotaSummary => {
        const buckets = findGroupBuckets(key);

        const fiveHourBucket = buckets.filter(bucket => /5h|hour/i.test(`${bucket?.window || ''} ${bucket?.bucket_id || ''}`))
            .reduce<(typeof buckets)[number] | undefined>((chosen, bucket) =>
                !chosen || bucket.remaining_fraction < chosen.remaining_fraction ? bucket : chosen, undefined);

        const weeklyBucket = buckets.filter(bucket => /week|7d/i.test(`${bucket?.window || ''} ${bucket?.bucket_id || ''}`))
            .reduce<(typeof buckets)[number] | undefined>((chosen, bucket) =>
                !chosen || bucket.remaining_fraction < chosen.remaining_fraction ? bucket : chosen, undefined);

        // 如果服务端没有返回 quota_groups，尝试用 models 作为兜底
        let fallback5h: GroupQuotaItem | undefined = undefined;
        let fallbackWeekly: GroupQuotaItem | undefined = undefined;

        if (fallbackModels.length > 0) {
            const matched = fallbackModels.filter(m => {
                const n = (m?.name || '').toLowerCase();
                if (n.includes('-disabled') || n.endsWith('_disabled') || (m as any)?.disabled) return false;
                const isArgon = n.includes('argon');
                if (key === 'argon') return isArgon;
                if (isArgon) return false;
                return key === 'claude' ? (n.startsWith('claude') || n.startsWith('gpt')) : n.startsWith('gemini');
            });

            if (matched.length > 0) {
                const now = Date.now();
                // 区分 5H 滑动窗口与周配额：如果重置时间在 6 小时之后，说明该时间是周配额时间，绝不能误标为 5H
                const fiveHourModels = matched.filter(m => {
                    if (!m.reset_time) return false;
                    const diffMs = Date.parse(m.reset_time) - now;
                    return diffMs > 0 && diffMs <= 6 * 3600 * 1000;
                });
                const weeklyModels = matched.filter(m => {
                    if (!m.reset_time) return false;
                    const diffMs = Date.parse(m.reset_time) - now;
                    return diffMs > 6 * 3600 * 1000;
                });

                if (fiveHourModels.length > 0) {
                    const min5h = fiveHourModels.reduce((prev, curr) => curr.percentage < prev.percentage ? curr : prev, fiveHourModels[0]);
                    fallback5h = {
                        percentage: min5h.percentage,
                        resetTime: min5h.reset_time,
                    };
                }

                if (weeklyModels.length > 0) {
                    const minWeekly = weeklyModels.reduce((prev, curr) => curr.percentage < prev.percentage ? curr : prev, weeklyModels[0]);
                    fallbackWeekly = {
                        percentage: minWeekly.percentage,
                        resetTime: minWeekly.reset_time,
                    };
                } else if (matched.length > 0 && !fiveHourModels.length) {
                    // 若均无明确时间戳，则作为常规配额
                    const minModel = matched.reduce((prev, curr) => curr.percentage < prev.percentage ? curr : prev, matched[0]);
                    if (isFreeTier || key === 'argon') {
                        fallbackWeekly = {
                            percentage: minModel.percentage,
                            resetTime: minModel.reset_time,
                        };
                    } else {
                        fallback5h = {
                            percentage: minModel.percentage,
                            resetTime: minModel.reset_time,
                        };
                    }
                }
            }
        }

        // 若服务端返回了真实的 quota_groups，绝不能被 fallbackModels 跨组污染（特别是 Argon 仅有官方周配额，无 5H 配额）
        const useFallback = (!groups || groups.length === 0) && key !== 'argon';

        // Free 账号在官方架构下只有周额度，没有 5H 额度
        const raw5h = (!isFreeTier && fiveHourBucket)
            ? Math.round(fiveHourBucket.remaining_fraction * 100)
            : (!isFreeTier && useFallback && fallback5h ? fallback5h.percentage : undefined);

        const rawWeekly = weeklyBucket
            ? Math.round(weeklyBucket.remaining_fraction * 100)
            : (useFallback && fallbackWeekly ? fallbackWeekly.percentage : (isFreeTier && useFallback && fallback5h ? fallback5h.percentage : undefined));

        const weeklyResetTime = weeklyBucket?.reset_time || (useFallback ? fallbackWeekly?.resetTime : undefined) || (isFreeTier && useFallback ? fallback5h?.resetTime : undefined);
        const fiveHourResetTime = isFreeTier ? undefined : (fiveHourBucket?.reset_time || (useFallback ? fallback5h?.resetTime : undefined));

        const effectiveWeekly = rawWeekly;
        const effectiveWeeklyResetTime = weeklyResetTime;

        const isWeeklyExhausted = effectiveWeekly !== undefined && effectiveWeekly <= 0;
        const isWeeklyConstrained = isWeeklyExhausted || (effectiveWeekly !== undefined && raw5h !== undefined && effectiveWeekly < raw5h);

        const labelKey = key === 'argon'
            ? 'accounts.quota_group_argon'
            : (key === 'claude' ? 'accounts.quota_group_claude' : 'accounts.quota_group_gemini');
        const defaultLabel = key === 'argon'
            ? 'Gemini 4 Argon'
            : (key === 'claude' ? 'Claude & GPT Models' : 'Gemini Models');

        const matchedForSummary = fallbackModels.filter(m => {
            const n = (m?.name || '').toLowerCase();
            if (n.includes('-disabled') || n.endsWith('_disabled') || (m as any)?.disabled) return false;
            const isArgon = n.includes('argon');
            if (key === 'argon') return isArgon;
            if (isArgon) return false;
            return key === 'claude' ? (n.startsWith('claude') || n.startsWith('gpt')) : n.startsWith('gemini');
        });
        const { dynamicLabel, supportedModels } = summarizeSupportedModels(matchedForSummary, key);

        return {
            key,
            labelKey,
            defaultLabel,
            dynamicLabel,
            supportedModels,
            fiveHour: raw5h !== undefined ? {
                percentage: isWeeklyExhausted ? 0 : raw5h,
                resetTime: isWeeklyExhausted ? effectiveWeeklyResetTime : fiveHourResetTime,
            } : undefined,
            weekly: effectiveWeekly !== undefined ? {
                percentage: effectiveWeekly,
                resetTime: effectiveWeeklyResetTime,
                cycleTokens: weeklyBucket?.cycle_tokens,
            } : undefined,
            isWeeklyConstrained,
            isWeeklyExhausted,
        };
    };

    const hasArgonInGroups = (groups || []).some(group => {
        const groupName = (group?.display_name || '').toLowerCase();
        return /argon/i.test(groupName)
            || (group?.buckets || []).some(b => /argon/i.test(`${b?.bucket_id || ''} ${b?.display_name || ''}`));
    });

    // 仅在服务端返回的 quota_groups 中包含真实 Argon 分组，或（无 groups 兜底时）包含未禁用的真实 Argon 模型时展示 (Fixes #3745)
    const hasArgon = (groups && groups.length > 0)
        ? hasArgonInGroups
        : (hasArgonInGroups || fallbackModels.some(m => {
            const n = (m?.name || '').toLowerCase();
            return /argon/i.test(n) && !n.includes('-disabled') && !n.endsWith('_disabled') && !(m as any)?.disabled;
        }));

    const summaries: GroupQuotaSummary[] = [
        buildGroup('gemini'),
        buildGroup('claude'),
    ];

    if (hasArgon) {
        summaries.push(buildGroup('argon'));
    }

    return summaries;
}

