/**
 * Model Family normalization, grouping and pricing resolution utilities.
 * Unifies thinking-tier model variants (-high, -medium, -low, -tiered, etc.)
 * under their canonical model family for cleaner visualization and metrics.
 */

export interface ModelPricingRule {
    input: number; // $ per 1M tokens
    output: number; // $ per 1M tokens
    cached: number; // $ per 1M tokens
}

export interface ModelTokenStats {
    model: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_reasoning_tokens?: number;
    total_tokens: number;
    request_count: number;
    avg_ttft_ms?: number;
}

export interface ModelVariantTokenStats {
    model: string;
    tierLabel: string;
    isBase: boolean;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_reasoning_tokens?: number;
    total_tokens: number;
    request_count: number;
    avg_ttft_ms?: number;
    cost: number;
}

export interface ModelFamilyTokenStats {
    familyId: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_reasoning_tokens?: number;
    total_tokens: number;
    request_count: number;
    avg_ttft_ms?: number;
    cost: number;
    variants: ModelVariantTokenStats[];
}

export const TIER_SUFFIXES = [
    '-tiered-xhigh',
    '-tiered-high',
    '-tiered-medium',
    '-tiered-low',
    '-extra-low',
    '-tiered',
    '-sonar',
    '-xhigh',
    '-high',
    '-medium',
    '-low',
    '-thinking',
] as const;

/**
 * Strips known thinking-tier suffixes to extract canonical model family ID.
 * e.g. "gemini-3.8-flash-high" -> "gemini-3.8-flash"
 *      "claude-opus-5-5-medium" -> "claude-opus-5-5"
 */
export function getModelFamilyId(modelId: string): string {
    const id = modelId.toLowerCase().trim();
    for (const suffix of TIER_SUFFIXES) {
        if (id.endsWith(suffix)) {
            return id.slice(0, -suffix.length);
        }
    }
    return id;
}

/**
 * Returns human-friendly display label for a specific tier variant.
 */
export function getTierLabel(modelId: string, familyId?: string): string {
    const id = modelId.toLowerCase().trim();
    const fam = familyId ? familyId.toLowerCase().trim() : getModelFamilyId(id);

    if (id === fam) return 'Default (无后缀)';
    if (id.endsWith('-tiered-xhigh')) return 'Tiered X-High';
    if (id.endsWith('-tiered-high')) return 'Tiered High';
    if (id.endsWith('-tiered-medium')) return 'Tiered Medium';
    if (id.endsWith('-tiered-low')) return 'Tiered Low';
    if (id.endsWith('-sonar')) return 'Sonar';
    if (id.endsWith('-extra-low')) return 'Extra-Low';
    if (id.endsWith('-xhigh')) return 'X-High';
    if (id.endsWith('-high')) return 'High';
    if (id.endsWith('-medium')) return 'Medium';
    if (id.endsWith('-low')) return 'Low';
    if (id.endsWith('-tiered')) return 'Tiered';
    if (id.endsWith('-thinking')) return 'Thinking';

    const rem = id.replace(fam, '').replace(/^[-_]/, '');
    return rem ? rem.toUpperCase() : 'Default';
}

/**
 * Standard badge styling matching the application design system.
 */
export function getTierBadgeClass(tierLabel: string): string {
    if (tierLabel.includes('Default') || tierLabel.includes('无后缀')) {
        return 'bg-gray-100 text-gray-700 border-gray-200/90 dark:bg-base-200 dark:text-gray-300 dark:border-base-300';
    }
    if (tierLabel.includes('Tiered')) {
        return 'bg-cyan-50 text-cyan-700 border-cyan-200/90 dark:bg-cyan-950/40 dark:text-cyan-300 dark:border-cyan-800/60';
    }
    if (tierLabel.includes('Sonar')) {
        return 'bg-teal-50 text-teal-700 border-teal-200/90 dark:bg-teal-950/40 dark:text-teal-300 dark:border-teal-800/60';
    }
    if (tierLabel.includes('X-High')) {
        return 'bg-purple-50 text-purple-700 border-purple-200/90 dark:bg-purple-950/40 dark:text-purple-300 dark:border-purple-800/60';
    }
    if (tierLabel.includes('High')) {
        return 'bg-emerald-50 text-emerald-700 border-emerald-200/90 dark:bg-emerald-950/40 dark:text-emerald-300 dark:border-emerald-800/60';
    }
    if (tierLabel.includes('Medium')) {
        return 'bg-blue-50 text-blue-700 border-blue-200/90 dark:bg-blue-950/40 dark:text-blue-300 dark:border-blue-800/60';
    }
    if (tierLabel.includes('Low') && !tierLabel.includes('Extra-Low')) {
        return 'bg-amber-50 text-amber-700 border-amber-200/90 dark:bg-amber-950/40 dark:text-amber-300 dark:border-amber-800/60';
    }
    if (tierLabel.includes('Extra-Low')) {
        return 'bg-orange-50 text-orange-700 border-orange-200/90 dark:bg-orange-950/40 dark:text-orange-300 dark:border-orange-800/60';
    }
    return 'bg-indigo-50 text-indigo-700 border-indigo-200/90 dark:bg-indigo-950/40 dark:text-indigo-300 dark:border-indigo-800/60';
}

function tierPriority(v: ModelVariantTokenStats): number {
    if (v.isBase) return 0;
    const l = v.tierLabel.toLowerCase();
    if (l.includes('tiered')) {
        if (l.includes('x-high')) return 6;
        if (l.includes('high')) return 7;
        return 8;
    }
    if (l.includes('sonar')) return 9;
    if (l.includes('x-high')) return 1;
    if (l.includes('high')) return 2;
    if (l.includes('medium')) return 3;
    if (l.includes('low') && !l.includes('extra-low')) return 4;
    if (l.includes('extra-low')) return 5;
    if (l.includes('thinking')) return 10;
    return 11;
}

/**
 * Aggregates fragmented raw model stats into clean model family groups.
 */
export function groupModelTokenStatsByFamily(
    stats: ModelTokenStats[],
    calculateCost: (m: ModelTokenStats) => number
): ModelFamilyTokenStats[] {
    const familyMap = new Map<string, {
        familyId: string;
        total_input_tokens: number;
        total_output_tokens: number;
        total_cached_tokens: number;
        total_reasoning_tokens: number;
        total_tokens: number;
        request_count: number;
        cost: number;
        weightedTtftSum: number;
        ttftWeightTotal: number;
        variants: ModelVariantTokenStats[];
    }>();

    for (const item of stats) {
        const famId = getModelFamilyId(item.model);
        const isBase = item.model.toLowerCase().trim() === famId.toLowerCase().trim();
        const tierLabel = getTierLabel(item.model, famId);
        const cost = calculateCost(item);

        const variant: ModelVariantTokenStats = {
            model: item.model,
            tierLabel,
            isBase,
            total_input_tokens: item.total_input_tokens,
            total_output_tokens: item.total_output_tokens,
            total_cached_tokens: item.total_cached_tokens,
            total_reasoning_tokens: item.total_reasoning_tokens || 0,
            total_tokens: item.total_tokens,
            request_count: item.request_count,
            avg_ttft_ms: item.avg_ttft_ms,
            cost,
        };

        if (!familyMap.has(famId)) {
            familyMap.set(famId, {
                familyId: famId,
                total_input_tokens: 0,
                total_output_tokens: 0,
                total_cached_tokens: 0,
                total_reasoning_tokens: 0,
                total_tokens: 0,
                request_count: 0,
                cost: 0,
                weightedTtftSum: 0,
                ttftWeightTotal: 0,
                variants: [],
            });
        }

        const fam = familyMap.get(famId)!;
        fam.total_input_tokens += item.total_input_tokens;
        fam.total_output_tokens += item.total_output_tokens;
        fam.total_cached_tokens += item.total_cached_tokens;
        fam.total_reasoning_tokens += (item.total_reasoning_tokens || 0);
        fam.total_tokens += item.total_tokens;
        fam.request_count += item.request_count;
        fam.cost += cost;

        if (item.avg_ttft_ms && item.avg_ttft_ms > 0 && item.request_count > 0) {
            fam.weightedTtftSum += item.avg_ttft_ms * item.request_count;
            fam.ttftWeightTotal += item.request_count;
        }

        fam.variants.push(variant);
    }

    const result: ModelFamilyTokenStats[] = [];
    for (const fam of familyMap.values()) {
        fam.variants.sort((a, b) => tierPriority(a) - tierPriority(b));
        const avg_ttft_ms = fam.ttftWeightTotal > 0
            ? Math.round((fam.weightedTtftSum / fam.ttftWeightTotal) * 10) / 10
            : undefined;

        result.push({
            familyId: fam.familyId,
            total_input_tokens: fam.total_input_tokens,
            total_output_tokens: fam.total_output_tokens,
            total_cached_tokens: fam.total_cached_tokens,
            total_reasoning_tokens: fam.total_reasoning_tokens,
            total_tokens: fam.total_tokens,
            request_count: fam.request_count,
            avg_ttft_ms,
            cost: fam.cost,
            variants: fam.variants,
        });
    }

    // Sort families by total_tokens DESC
    result.sort((a, b) => b.total_tokens - a.total_tokens);
    return result;
}

/**
 * Intelligently resolves pricing rule for any model name by:
 * 1. Exact match
 * 2. Canonical family ID match (stripping thinking tiers like -high, -medium)
 * 3. Dot-to-dash normalization (claude-sonnet-4.6 -> claude-sonnet-4-6)
 * 4. Family prefix & substring pattern matching
 * 5. Fallback to default
 */
export function resolvePricingRule(
    modelKey: string,
    pricing: Record<string, ModelPricingRule>
): ModelPricingRule {
    if (!modelKey) return pricing['default'] || { input: 1.0, output: 5.0, cached: 0.1 };

    const key = modelKey.toLowerCase().trim();

    // 1. 精确匹配
    if (pricing[key]) return pricing[key];

    // 2. 剥除档位后缀匹配模型家族 (如 gemini-3.8-flash-high -> gemini-3.8-flash)
    const famId = getModelFamilyId(key);
    if (pricing[famId]) return pricing[famId];

    // 3. 剥除扩展后缀匹配
    const stripped = key.replace(/-(tiered-xhigh|tiered-high|tiered-medium|tiered-low|tiered|sonar|max|xhigh|high|medium|default|low|extra-low|lite|thinking|online)$/, '');
    if (pricing[stripped]) return pricing[stripped];

    // 4. 点分与破折号互通匹配 (如 claude-sonnet-4.6 与 claude-sonnet-4-6)
    const normalizedKey = key.replace(/\./g, '-');
    const normalizedFamId = famId.replace(/\./g, '-');
    const normalizedStripped = stripped.replace(/\./g, '-');
    if (pricing[normalizedKey]) return pricing[normalizedKey];
    if (pricing[normalizedFamId]) return pricing[normalizedFamId];
    if (pricing[normalizedStripped]) return pricing[normalizedStripped];

    // 5. 常见家族前缀/模式匹配（针对通用系列规则，如 gemini-flash, gemini-pro, claude-sonnet 等）
    for (const [pattern, rule] of Object.entries(pricing)) {
        if (pattern === 'default') continue;
        const normPattern = pattern.toLowerCase().replace(/\./g, '-');

        // 如果 pattern 包含多个词根（如 claude-sonnet），要求全部命中
        if (normPattern.includes('-')) {
            const parts = normPattern.split('-');
            const allMatched = parts.every(p => normalizedKey.includes(p) || normalizedFamId.includes(p));
            if (allMatched) {
                return rule;
            }
        } else if (normalizedKey.includes(normPattern) || normalizedFamId.includes(normPattern)) {
            return rule;
        }
    }

    return pricing['default'] || { input: 1.0, output: 5.0, cached: 0.1 };
}

export interface ModelTrendPoint {
    period: string;
    model_data: Record<string, number>;
}

/**
 * Aggregates trend time points into unified model families to prevent
 * multiple thinking tiers of the same model from cluttering charts.
 */
export function aggregateModelTrendsByFamily(
    modelTrend: ModelTrendPoint[],
    topN: number = 5,
    otherLabel: string = 'Other Models'
): {
    transformedTrend: Record<string, any>[];
    displayModelKeys: string[];
} {
    // 1. Group points by model family
    const familyTrend = modelTrend.map(point => {
        const famData: Record<string, number> = {};
        for (const [m, val] of Object.entries(point.model_data)) {
            const fam = getModelFamilyId(m);
            famData[fam] = (famData[fam] || 0) + val;
        }
        return { period: point.period, model_data: famData };
    });

    // 2. Calculate totals per family to determine top N
    const familyTotals: Record<string, number> = {};
    const familiesSet = new Set<string>();
    for (const point of familyTrend) {
        for (const [fam, val] of Object.entries(point.model_data)) {
            familiesSet.add(fam);
            familyTotals[fam] = (familyTotals[fam] || 0) + val;
        }
    }

    const sortedFamilies = Array.from(familiesSet).sort(
        (a, b) => (familyTotals[b] || 0) - (familyTotals[a] || 0)
    );
    const topFamilies = sortedFamilies.slice(0, topN);
    const hasOther = sortedFamilies.length > topN;
    const displayModelKeys = hasOther ? [...topFamilies, otherLabel] : topFamilies;

    // 3. Transform to recharts row format
    const transformedTrend = familyTrend.map(point => {
        const row: Record<string, any> = { period: point.period };
        let otherSum = 0;
        for (const [fam, val] of Object.entries(point.model_data)) {
            if (topFamilies.includes(fam)) {
                row[fam] = val;
            } else {
                otherSum += val;
            }
        }
        if (hasOther) {
            row[otherLabel] = otherSum;
        }
        return row;
    });

    return { transformedTrend, displayModelKeys };
}

