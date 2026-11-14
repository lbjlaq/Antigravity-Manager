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
    'gemini-4-carbon': 'gemini-4-carbon',
    'gemini-4-carbon-xhigh': 'gemini-4-carbon-xhigh',
    'gemini-4-carbon-high': 'gemini-4-carbon-high',
    'gemini-4-carbon-medium': 'gemini-4-carbon-medium',
    'gemini-4-carbon-low': 'gemini-4-carbon-low',
    'gemini-4-argon': 'gemini-4-argon',
    'gemini-4-argon-xhigh': 'gemini-4-argon-xhigh',
    'gemini-4-argon-high': 'gemini-4-argon-high',
    'gemini-4-argon-medium': 'gemini-4-argon-medium',
    'gemini-4-argon-low': 'gemini-4-argon-low',
    'gemini-pro-orchestrator-g1-pro': 'gemini-pro-orchestrator-g1-pro',
    'gemini-4-hydrogen': 'gemini-4-hydrogen',
    'gemini-4-hydrogen-xhigh': 'gemini-4-hydrogen-xhigh',
    'gemini-4-hydrogen-high': 'gemini-4-hydrogen-high',
    'gemini-4-hydrogen-medium': 'gemini-4-hydrogen-medium',
    'gemini-4-hydrogen-low': 'gemini-4-hydrogen-low',
    'gemini-3.1-pro-high': 'gemini-3.1-pro-high',
    'gemini-3.1-pro': 'gemini-3.1-pro',
    'gemini-3.1-pro-low': 'gemini-3.1-pro-low',
    'gemini-3.8-flash': 'gemini-3.8-flash',
    'gemini-3.8-flash-high': 'gemini-3.8-flash-high',
    'gemini-3.8-flash-medium': 'gemini-3.8-flash-medium',
    'gemini-3.8-flash-low': 'gemini-3.8-flash-low',
    'gemini-3.8-flash-tiered': 'gemini-3.8-flash-tiered',
    'gemini-3.5-flash-lite': 'gemini-3.5-flash-lite',
    'gemini-3.1-flash-lite': 'gemini-3.1-flash-lite',
    'gemini-3.1-flash-image': 'gemini-3.1-flash-image',
    'gemini-3-pro-image': 'gemini-3-pro-image',
    'claude-opus-5-5-high': 'claude-opus-5-5-high',
    'claude-opus-5-5-medium': 'claude-opus-5-5-medium',
    'claude-opus-5-5-low': 'claude-opus-5-5-low',
    'claude-sonnet-5-5-high': 'claude-sonnet-5-5-high',
    'claude-sonnet-5-5-medium': 'claude-sonnet-5-5-medium',
    'claude-sonnet-5-5-low': 'claude-sonnet-5-5-low',
    'claude-sonnet-4-6': 'claude-sonnet-4-6',
    'claude-opus-4-6-thinking': 'claude-opus-4-6-thinking',
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
        'gemini-pro': ['gemini-3.1-pro-high', 'gemini-3.1-pro', 'gemini-3.1-pro-low'],
        'gemini-flash': ['gemini-3.8-flash-high', 'gemini-3.8-flash', 'gemini-3.8-flash-medium', 'gemini-3.8-flash-low', 'gemini-3.8-flash-tiered', 'gemini-3.5-flash-lite', 'gemini-3.1-flash-lite'],
        'claude': ['claude-opus-5-5-high', 'claude-sonnet-5-5-high', 'claude-sonnet-4-6', 'claude-opus-4-6-thinking'],
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

const GENERIC_G4_ROLE_TOKENS = new Set([
    'pro', 'flash', 'ultra', 'nano', 'lite', 'image', 'agent', 'exp',
    'preview', 'disabled', 'thinking', 'tiered', 'models', 'model',
    'wtu', 'weekly', '5h', '7d', 'hour', 'quota', 'pool', 'default', 'standard',
]);

/** 判断模型是否属于 Gemini 4 Argon（包含命名含 argon 的模型与 Orchestrator Argon 旗舰入口） */
export function isArgonModel(modelName: string): boolean {
    const lower = modelName.trim().toLowerCase();
    return lower.includes('argon') || lower === 'gemini-pro-orchestrator-g1-pro';
}

/** 从模型名动态提取 Gemini 4+ 专属代号（零硬编码） */
export function extractGemini4Codename(modelName: string): string | null {
    const lower = modelName.trim().toLowerCase();
    if (!lower || lower.includes('-disabled') || lower.endsWith('_disabled')) return null;
    const stripped = lower.replace(/-(?:extra-low|xhigh|high|medium|low|thinking|tiered)$/, '');
    const m = stripped.match(/^gemini-(\d+)(?:\.\d+)?-([a-z0-9]+)/);
    if (m && Number(m[1]) >= 4) {
        const code = m[2];
        if (/[a-z]/.test(code) && !GENERIC_G4_ROLE_TOKENS.has(code)) {
            return code;
        }
    }
    if (isArgonModel(lower)) return 'argon';
    return null;
}

export function isDedicatedG4Group(
    group: { display_name?: string; buckets?: Array<{ bucket_id?: string; display_name?: string | null }> },
    knownCodenames: readonly string[] = []
): boolean {
    const gname = (group.display_name || '').toLowerCase();
    const texts = [
        gname,
        ...(group.buckets || []).flatMap(b => [
            (b.bucket_id || '').toLowerCase(),
            (b.display_name || '').toLowerCase(),
        ]),
    ];
    for (const t of texts) {
        if (!t) continue;
        if (t.includes('argon')) return true;
        if (/(?:^|[\s_-])gemini[\s_-]+([4-9]|\d{2,})(?:\.\d+)?[\s_-]+([a-z][a-z0-9]*)/.test(t)) {
            const m = t.match(/gemini[\s_-]+(?:[4-9]|\d{2,})(?:\.\d+)?[\s_-]+([a-z][a-z0-9]*)/);
            if (m && !GENERIC_G4_ROLE_TOKENS.has(m[1])) return true;
        }
        const wtu = t.match(/gemini[\s_-]+([a-z][a-z0-9]*)[\s_-]+wtu/);
        if (wtu && !GENERIC_G4_ROLE_TOKENS.has(wtu[1])) return true;
        if (knownCodenames.some(code => code && t.includes(code))) return true;
    }
    return false;
}

/** 从 quota_groups 提取通用 Gemini 或 3P(Claude/GPT) 的 5h / weekly 百分比 (0-100)，自动隔离所有 Gemini 4+ 专属组 */
export function extractGroupQuotaPercentage(
    quota: {
        models?: Array<{ name?: string; disabled?: boolean }>;
        quota_groups?: Array<{
            display_name?: string;
            buckets?: Array<{ window?: string; bucket_id?: string; display_name?: string | null; remaining_fraction?: number }>;
        }>;
    } | undefined,
    category: 'gemini' | 'claude',
    targetWindow: '5h' | 'weekly'
): number | null {
    const groups = quota?.quota_groups;
    if (!groups || groups.length === 0) return null;

    const knownCodenames = Array.from(
        new Set(
            (quota?.models || [])
                .filter(m => !m.disabled && m.name)
                .map(m => extractGemini4Codename(m.name!))
                .filter((c): c is string => Boolean(c))
        )
    );

    for (const group of groups) {
        if (isDedicatedG4Group(group, knownCodenames)) continue;
        const name = (group.display_name || '').toLowerCase();
        const isTarget = category === 'claude'
            ? (name.includes('claude') || name.includes('gpt') || name.includes('3p'))
            : (name.includes('gemini') || (!name.includes('claude') && !name.includes('gpt') && !name.includes('3p')));
        if (!isTarget) continue;

        const bucket = group.buckets?.find(b => {
            const win = (b.window || '').toLowerCase();
            const id = (b.bucket_id || '').toLowerCase();
            return targetWindow === 'weekly'
                ? (win.includes('week') || id.includes('week') || win.includes('7d'))
                : (win.includes('5h') || id.includes('5h') || win.includes('hour') || id.includes('hour'));
        });
        if (bucket && typeof bucket.remaining_fraction === 'number') {
            return Math.round(bucket.remaining_fraction * 100);
        }
    }
    return null;
}
