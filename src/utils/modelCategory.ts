/**
 * 模型分类工具函数（无 React / icons 依赖，可在 Node 环境直接导入）
 */

export type ModelCategory = 'gemini-pro' | 'gemini-flash' | 'gemini-pro-image' | 'gemini-flash-image' | 'claude' | 'other';

export function categorizeModel(name: string): ModelCategory {
    const n = name.trim().toLowerCase();
    const isGemini = n.startsWith('gemini-');
    const isImage = (isGemini && n.includes('image')) || n.startsWith('image') || n.startsWith('imagen');
    if (isImage) return n.includes('flash') ? 'gemini-flash-image' : 'gemini-pro-image';
    if (isGemini && n.includes('flash')) return 'gemini-flash';
    if (isGemini && n.includes('pro')) return 'gemini-pro';
    if (n.includes('claude') || n.includes('opus') || n.includes('sonnet') || n.includes('haiku')) return 'claude';
    return 'other';
}

export interface ModelDisplayNameInput {
    name: string;
    display_name?: string;
}

const DEFAULT_MODEL_LABELS: Record<string, string> = {
    'gemini-pro-agent': 'gemini-3.1-pro-high',
    'gemini-3.1-pro-high': 'gemini-3.1-pro-high',
    'gemini-3.1-pro': 'gemini-3.1-pro',
    'gemini-3.1-pro-low': 'gemini-3.1-pro-low',
    'gemini-2.5-pro': 'gemini-2.5-pro',
    'gemini-3.8-flash': 'gemini-3.8-flash',
    'gemini-3.7-flash': 'gemini-3.7-flash',
    'gemini-3.5-flash': 'gemini-3.5-flash',
    'gemini-3-flash': 'gemini-3-flash',
    'gemini-2.5-flash': 'gemini-2.5-flash',
    'gemini-3.1-flash-image': 'gemini-3.1-flash-image',
    'gemini-3-pro-image': 'gemini-3-pro-image',
    'claude-sonnet-4-6': 'claude-sonnet-4-6',
    'claude-opus-4-6-thinking': 'claude-opus-4-6-thinking',
    'claude-sonnet-4-5': 'claude-sonnet-4-5',
    'claude-haiku-4-5': 'claude-haiku-4-5',
};

export function getModelDisplayName(
    model: ModelDisplayNameInput | null | undefined,
    fallback?: string,
): string {
    if (model) {
        if (model.name) {
            return DEFAULT_MODEL_LABELS[model.name] || model.name;
        }
        if (model.display_name) return model.display_name;
    }
    return fallback ?? '';
}

/**
 * 按优先级查找配额模型：先精确匹配首选名，再按类别 fallback。
 */
export function findQuotaModel<T extends { name: string }>(
    models: T[] | undefined,
    category: ModelCategory,
): T | undefined {
    if (!models || models.length === 0) return undefined;
    const preferred: Partial<Record<ModelCategory, string[]>> = {
        'gemini-pro': ['gemini-pro-agent', 'gemini-3.1-pro-high', 'gemini-3.1-pro', 'gemini-3.1-pro-low', 'gemini-2.5-pro'],
        'gemini-flash': ['gemini-3-flash-agent', 'gemini-3-flash', 'gemini-3.5-flash'],
        'claude': ['claude-sonnet-4-6', 'claude-opus-4-6-thinking'],
    };
    const names = preferred[category];
    if (names) {
        for (const name of names) {
            const found = models.find(m => m.name === name);
            if (found) return found;
        }
    }
    return models.find(m => categorizeModel(m.name) === category);
}

export function getModelProtectionKey(name: string): string | null {
    switch (categorizeModel(name)) {
        case 'gemini-flash': return 'gemini-3-flash';
        case 'gemini-pro': return 'gemini-3-pro-high';
        case 'gemini-flash-image': return 'gemini-3.1-flash-image';
        case 'gemini-pro-image': return 'gemini-3-pro-image';
        case 'claude': return 'claude';
        default: return null;
    }
}

/**
 * 在任意图片类别中查找第一个实际模型。
 * 用于让新旧 image selector 共享同一配额槽位。
 */
export function findImageQuotaModel<T extends { name: string }>(
    models: T[] | undefined,
): T | undefined {
    if (!models || models.length === 0) return undefined;
    return models.find(m => {
        const c = categorizeModel(m.name);
        return c === 'gemini-flash-image' || c === 'gemini-pro-image';
    });
}

/** 账号管理 pin 列表缺省图像选择器时补入代表 Image，与仪表盘对齐。 */
export const DEFAULT_IMAGE_PIN_SELECTOR = 'gemini-3.1-flash-image';

export function ensurePinnedImageSelector(selectorIds: string[] | undefined): string[] {
    const pinned = selectorIds ? [...selectorIds] : [];
    const hasImage = pinned.some(id => {
        const category = categorizeModel(id);
        return category === 'gemini-flash-image' || category === 'gemini-pro-image';
    });
    if (hasImage) return pinned;
    pinned.push(DEFAULT_IMAGE_PIN_SELECTOR);
    return pinned;
}

export interface QuotaModelSelection<T> {
    selectorId: string;
    selectionKey: string;
    model: T | undefined;
}

export function resolveQuotaModels<T extends { name: string }>(
    models: T[] | undefined,
    selectorIds: string[],
): QuotaModelSelection<T>[] {
    const seen = new Set<string>();
    const results: QuotaModelSelection<T>[] = [];

    for (const selectorId of selectorIds) {
        const normalizedId = selectorId.trim().toLowerCase();
        const category = categorizeModel(normalizedId);

        const isImage = category === 'gemini-pro-image' || category === 'gemini-flash-image';

        // Exact-match first: a pinned id that names a real quota model must render
        // that model, not collapse into its category slot.
        const exact = !isImage
            ? models?.find(m => m.name.trim().toLowerCase() === normalizedId)
            : undefined;
        if (exact) {
            const selectionKey = `model:${normalizedId}`;
            if (seen.has(selectionKey)) continue;
            seen.add(selectionKey);
            results.push({ selectorId, selectionKey, model: exact });
            continue;
        }

        const selectionKey = isImage
            ? 'category:gemini-image'
            : category === 'other'
                ? `model:${normalizedId}`
                : `category:${category}`;

        if (seen.has(selectionKey)) continue;
        seen.add(selectionKey);

        const model = isImage
            ? findImageQuotaModel(models)
            : category === 'other'
                ? models?.find(m => m.name.trim().toLowerCase() === normalizedId)
                : findQuotaModel(models, category);

        results.push({ selectorId, selectionKey, model });
    }
    return results;
}
