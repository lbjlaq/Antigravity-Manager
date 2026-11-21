import type { ModelPriorities, ModelPriorityGroup, ModelQuota } from '../types/account';

export type EffectiveModelPrioritySource = 'exact' | 'series' | 'family' | 'default';

export interface EffectiveModelPriorityDetail {
    priority: number;
    source: EffectiveModelPrioritySource;
    matchedKey?: string;
}

export interface ModelPriorityOption {
    key: string;
    label: string;
    scope: 'series' | 'exact';
    family: ModelPriorityGroup;
    variantCount: number;
}

export const MAX_MODEL_PRIORITY_KEY_LEN = 128;
export const MAX_MODEL_PRIORITY_ENTRIES = 256;

const MODEL_PRIORITY_KEY_REGEX = /^[a-z0-9][a-z0-9._/:@-]{0,127}$/;

const TIER_SUFFIXES = [
    '-extra-low',
    '-xhigh',
    '-high',
    '-medium',
    '-low',
    '-thinking',
    '-tiered',
    '@default',
] as const;

const IMAGE_VARIANT_SUFFIXES = [
    '-preview',
    '-2k',
    '-4k',
    '-16x9',
    '-9x16',
    '-4x3',
    '-3x4',
    '-1x1',
    '-21x9',
    '-3x2',
    '-2x3',
    '-4x5',
    '-5x4',
] as const;

export function isValidPriorityNumber(value: unknown): value is number {
    return typeof value === 'number' && Number.isInteger(value) && value >= 1 && value <= 100;
}

export function normalizeModelPriorityKey(rawKey: string): string {
    return rawKey.trim().toLowerCase();
}

export function isValidModelPriorityKey(rawKey: string): boolean {
    const key = normalizeModelPriorityKey(rawKey);
    if (!key || key.length > MAX_MODEL_PRIORITY_KEY_LEN) {
        return false;
    }
    if (key.includes('..') || key.endsWith('/')) {
        return false;
    }
    return MODEL_PRIORITY_KEY_REGEX.test(key);
}

export function isCoarseModelPriorityGroup(key: string): key is ModelPriorityGroup {
    return key === 'gemini' || key === 'claude';
}

export function resolveModelPriorityGroup(modelId: string): ModelPriorityGroup {
    const raw = modelId.trim().toLowerCase();
    if (raw === 'claude' || raw.includes('claude') || raw.startsWith('gpt')) {
        return 'claude';
    }
    return 'gemini';
}

export function stripAllTierSuffixes(modelId: string): string[] {
    const chain: string[] = [];
    let cur = modelId.trim().toLowerCase();
    for (let i = 0; i < 4; i++) {
        let next = cur;
        for (const suffix of TIER_SUFFIXES) {
            if (cur.endsWith(suffix) && cur.length > suffix.length) {
                next = cur.slice(0, -suffix.length);
                break;
            }
        }
        if (!next || next === cur) {
            break;
        }
        chain.push(next);
        cur = next;
    }
    return chain;
}

export function stripImageVariantSuffix(modelId: string): string {
    const lower = modelId.trim().toLowerCase();
    if (!lower.includes('image')) {
        return lower;
    }
    let cur = lower;
    for (let i = 0; i < 4; i++) {
        let matched = false;
        for (const suffix of IMAGE_VARIANT_SUFFIXES) {
            if (cur.endsWith(suffix) && cur.length > suffix.length) {
                cur = cur.slice(0, -suffix.length);
                matched = true;
                break;
            }
        }
        if (!matched) break;
    }
    return cur;
}

export function getCanonicalModelSeriesId(modelId: string): string {
    const lower = modelId.trim().toLowerCase();
    const chain = stripAllTierSuffixes(lower);
    if (chain.length > 0) {
        return chain[chain.length - 1];
    }
    return stripImageVariantSuffix(lower);
}

function getNormalizedOverrideMap(modelPriorities?: ModelPriorities): Record<string, number> {
    const map: Record<string, number> = {};
    if (!modelPriorities) return map;
    for (const [rawKey, value] of Object.entries(modelPriorities)) {
        if (!isValidPriorityNumber(value)) continue;
        const key = normalizeModelPriorityKey(rawKey);
        if (isValidModelPriorityKey(key)) {
            map[key] = value;
        }
    }
    return map;
}

export function getSpecificModelPriorityEntries(
    modelPriorities?: ModelPriorities
): Array<[string, number]> {
    const map = getNormalizedOverrideMap(modelPriorities);
    return Object.entries(map)
        .filter(([key]) => !isCoarseModelPriorityGroup(key))
        .sort(([a], [b]) => a.localeCompare(b));
}

export function resolveEffectiveModelPriorityDetail(
    account: { priority?: number; model_priorities?: ModelPriorities },
    modelId: string
): EffectiveModelPriorityDetail {
    const basePriority = isValidPriorityNumber(account.priority) ? account.priority : 50;
    const overrides = getNormalizedOverrideMap(account.model_priorities);
    const raw = normalizeModelPriorityKey(modelId);
    const family = resolveModelPriorityGroup(raw);

    if (raw && raw !== family && overrides[raw] !== undefined) {
        return { priority: overrides[raw], source: 'exact', matchedKey: raw };
    }

    if (raw) {
        for (const base of stripAllTierSuffixes(raw)) {
            if (base !== family && overrides[base] !== undefined) {
                return { priority: overrides[base], source: 'series', matchedKey: base };
            }
        }
        const imgBase = stripImageVariantSuffix(raw);
        if (imgBase && imgBase !== raw && imgBase !== family && overrides[imgBase] !== undefined) {
            return { priority: overrides[imgBase], source: 'series', matchedKey: imgBase };
        }
    }

    if (overrides[family] !== undefined) {
        return { priority: overrides[family], source: 'family', matchedKey: family };
    }

    return { priority: basePriority, source: 'default' };
}

export function hasAnyCustomModelPriorities(account: {
    priority?: number;
    model_priorities?: ModelPriorities;
}): boolean {
    if (!account.model_priorities) return false;
    const base = isValidPriorityNumber(account.priority) ? account.priority : 50;
    const overrides = getNormalizedOverrideMap(account.model_priorities);
    if (overrides.gemini !== undefined && overrides.gemini !== base) return true;
    if (overrides.claude !== undefined && overrides.claude !== base) return true;
    return Object.keys(overrides).some(key => !isCoarseModelPriorityGroup(key));
}

export function formatAccountPriorityBadge(
    account: { priority?: number; model_priorities?: ModelPriorities },
    priorityLabel: string
): string {
    const base = isValidPriorityNumber(account.priority) ? account.priority : 50;
    const overrides = getNormalizedOverrideMap(account.model_priorities);
    const g = overrides.gemini ?? base;
    const c = overrides.claude ?? base;
    const hasFamilyDiff =
        (overrides.gemini !== undefined && overrides.gemini !== base) ||
        (overrides.claude !== undefined && overrides.claude !== base);
    const specificCount = Object.keys(overrides).filter(k => !isCoarseModelPriorityGroup(k)).length;

    if (!hasFamilyDiff && specificCount === 0) {
        return `${priorityLabel}: ${base}`;
    }
    if (hasFamilyDiff && specificCount === 0) {
        return `${priorityLabel}: G:${g} · C:${c}`;
    }
    if (hasFamilyDiff) {
        return `${priorityLabel}: G:${g} · C:${c} · +${specificCount}`;
    }
    return `${priorityLabel}: ${base} · +${specificCount}`;
}

export function formatAccountPriorityTooltip(
    account: { priority?: number; model_priorities?: ModelPriorities },
    labels: { priority: string; defaultHint: string }
): string {
    if (!hasAnyCustomModelPriorities(account)) {
        return labels.defaultHint;
    }
    const base = isValidPriorityNumber(account.priority) ? account.priority : 50;
    const overrides = getNormalizedOverrideMap(account.model_priorities);
    const g = overrides.gemini ?? base;
    const c = overrides.claude ?? base;
    const specific = getSpecificModelPriorityEntries(overrides);
    const baseLine = `Gemini: ${g} | Claude: ${c} (${labels.priority}: ${base})`;
    if (specific.length === 0) {
        return baseLine;
    }
    const details = specific.map(([key, value]) => `${key}: ${value}`).join(', ');
    return `${baseLine} | ${details}`;
}

export function stripDisplayTierLabel(displayName: string): string {
    return displayName
        .replace(/\s*\((?:X-?High|High|Medium|Low|Extra-Low|Thinking|Tiered)\)\s*$/i, '')
        .trim();
}

export function buildModelPriorityPickerOptions(
    models: Array<Pick<ModelQuota, 'name' | 'display_name'>>,
    configuredKeys: Set<string>
): { seriesOptions: ModelPriorityOption[]; exactOptions: ModelPriorityOption[] } {
    const seriesMap = new Map<
        string,
        { label: string; family: ModelPriorityGroup; variants: Set<string> }
    >();
    const exactMap = new Map<string, { label: string; family: ModelPriorityGroup }>();

    for (const model of models) {
        const rawKey = normalizeModelPriorityKey(model.name || '');
        if (!rawKey || !isValidModelPriorityKey(rawKey) || isCoarseModelPriorityGroup(rawKey)) {
            continue;
        }
        const family = resolveModelPriorityGroup(rawKey);
        const seriesKey = getCanonicalModelSeriesId(rawKey);
        const rawLabel = (model.display_name || model.name || rawKey).trim();

        if (!isCoarseModelPriorityGroup(seriesKey)) {
            const existing = seriesMap.get(seriesKey);
            if (existing) {
                existing.variants.add(rawKey);
            } else {
                seriesMap.set(seriesKey, {
                    label: stripDisplayTierLabel(rawLabel) || seriesKey,
                    family,
                    variants: new Set([rawKey]),
                });
            }
        }

        if (rawKey !== seriesKey && !exactMap.has(rawKey)) {
            exactMap.set(rawKey, { label: rawLabel, family });
        }
    }

    const seriesOptions: ModelPriorityOption[] = Array.from(seriesMap.entries())
        .filter(([key]) => !configuredKeys.has(key))
        .map(([key, info]) => ({
            key,
            label: info.label,
            scope: 'series' as const,
            family: info.family,
            variantCount: info.variants.size,
        }))
        .sort((a, b) => a.key.localeCompare(b.key));

    const exactOptions: ModelPriorityOption[] = Array.from(exactMap.entries())
        .filter(([key]) => !configuredKeys.has(key))
        .map(([key, info]) => ({
            key,
            label: info.label,
            scope: 'exact' as const,
            family: info.family,
            variantCount: 1,
        }))
        .sort((a, b) => a.key.localeCompare(b.key));

    return { seriesOptions, exactOptions };
}

/**
 * Build temporary Pedal Mode model priorities:
 * boosts the target family to priority 1 (clearing target-family specific overrides
 * so they cannot shadow the boost), while preserving the other family's priority
 * and all specific model overrides belonging to the other family.
 */
export function buildPedalBoostedModelPriorities(
    origPriority: number,
    origModelPriorities: ModelPriorities | undefined,
    targetFamily: ModelPriorityGroup
): ModelPriorities {
    const otherFamily: ModelPriorityGroup = targetFamily === 'gemini' ? 'claude' : 'gemini';
    const boosted: ModelPriorities = {};
    const normalized = getNormalizedOverrideMap(origModelPriorities);

    for (const [key, value] of Object.entries(normalized)) {
        if (isCoarseModelPriorityGroup(key)) {
            if (key === otherFamily) {
                boosted[key] = value;
            }
            continue;
        }
        if (resolveModelPriorityGroup(key) === otherFamily) {
            boosted[key] = value;
        }
    }

    boosted[targetFamily] = 1;
    if (boosted[otherFamily] === undefined) {
        boosted[otherFamily] = origPriority;
    }

    return boosted;
}

export function normalizeModelPrioritiesPayload(
    modelPriorities: ModelPriorities,
    invalidPriorityMessage: string,
    invalidKeyMessage: (key: string) => string
): Record<string, number> {
    const normalized: Record<string, number> = {};
    const entries = Object.entries(modelPriorities);
    if (entries.length > MAX_MODEL_PRIORITY_ENTRIES) {
        throw new Error(invalidKeyMessage(`>${MAX_MODEL_PRIORITY_ENTRIES}`));
    }
    for (const [rawKey, val] of entries) {
        if (val === undefined || val === null) {
            continue;
        }
        const key = normalizeModelPriorityKey(rawKey);
        if (!isValidModelPriorityKey(key)) {
            throw new Error(invalidKeyMessage(rawKey));
        }
        if (!isValidPriorityNumber(val)) {
            throw new Error(invalidPriorityMessage);
        }
        if (Object.prototype.hasOwnProperty.call(normalized, key)) {
            throw new Error(invalidKeyMessage(rawKey));
        }
        normalized[key] = val;
    }
    return normalized;
}
