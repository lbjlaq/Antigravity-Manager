import { Gemini, Claude } from '@lobehub/icons';

/**
 * 模型配置接口
 */
export interface ModelConfig {
    /** 模型完整显示名称 (作为回退或默认展示) */
    label: string;
    /** 模型简短标签 (用于列表/卡片) */
    shortLabel: string;
    /** 保护模型的键名 */
    protectedKey: string;
    /** 模型图标组件 */
    Icon: React.ComponentType<{ size?: number; className?: string }>;
    /** 国际化键名 (用于动态名称) */
    i18nKey: string;
    /** 描述信息键名 (用于详细说明) */
    i18nDescKey: string;
    /** 所属系列/分组 */
    group: string;
    /** 选填标签 (用于筛选) */
    tags?: string[];
}

/**
 * 模型配置映射
 * 键为模型 ID，值为模型配置
 */
export const MODEL_CONFIG: Record<string, ModelConfig> = {
    // Gemini 3.x 系列
    // [Migrate] Gemini 3 Pro High/Low -> Gemini 3.1 Pro High/Low
    'gemini-3.1-pro-high': {
        label: 'Gemini 3.1 Pro High',
        shortLabel: 'G3.1 Pro',
        protectedKey: 'gemini-pro',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.pro_high',
        i18nDescKey: 'proxy.model.pro_high',
        group: 'Gemini 3',
        tags: ['pro', 'high'],
    },
    // Backward-compatible alias
    'gemini-3-pro-high': {
        label: 'Gemini 3.1 Pro High',
        shortLabel: 'G3.1 Pro',
        protectedKey: 'gemini-pro',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.pro_high',
        i18nDescKey: 'proxy.model.pro_high',
        group: 'Gemini 3',
        tags: ['pro', 'high'],
    },
    'gemini-3-flash': {
        label: 'Gemini 3 Flash',
        shortLabel: 'G3 Flash',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash'],
    },
    'gemini-3-pro-image': {
        label: 'Gemini 3 Image',
        shortLabel: 'G3 Image',
        protectedKey: 'gemini-3-pro-image',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.pro_image',
        i18nDescKey: 'proxy.model.pro_image_1_1',
        group: 'Gemini 3',
        tags: ['image'],
    },
    'gemini-3.1-pro-low': {
        label: 'Gemini 3.1 Pro Low',
        shortLabel: 'G3.1 Low',
        protectedKey: 'gemini-pro',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.pro_low',
        i18nDescKey: 'proxy.model.pro_low',
        group: 'Gemini 3',
        tags: ['pro', 'low'],
    },
    // Backward-compatible alias
    'gemini-3-pro-low': {
        label: 'Gemini 3.1 Pro Low',
        shortLabel: 'G3.1 Low',
        protectedKey: 'gemini-pro',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.pro_low',
        i18nDescKey: 'proxy.model.pro_low',
        group: 'Gemini 3',
        tags: ['pro', 'low'],
    },

    // Gemini 2.5 系列
    'gemini-2.5-flash': {
        label: 'Gemini 2.5 Flash',
        shortLabel: 'G2.5 Flash',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.gemini_2_5_flash',
        i18nDescKey: 'proxy.model.gemini_2_5_flash',
        group: 'Gemini 2.5',
        tags: ['flash'],
    },
    'gemini-2.5-flash-lite': {
        label: 'Gemini 2.5 Flash Lite',
        shortLabel: 'G2.5 Lite',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_lite',
        i18nDescKey: 'proxy.model.flash_lite',
        group: 'Gemini 2.5',
        tags: ['flash', 'lite'],
    },
    'gemini-2.5-flash-thinking': {
        label: 'Gemini 2.5 Flash Think',
        shortLabel: 'G2.5 Think',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_thinking',
        i18nDescKey: 'proxy.model.flash_thinking',
        group: 'Gemini 2.5',
        tags: ['flash', 'thinking'],
    },
    'gemini-2.5-pro': {
        label: 'Gemini 2.5 Pro',
        shortLabel: 'G2.5 Pro',
        protectedKey: 'gemini-pro',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.gemini_2_5_pro',
        i18nDescKey: 'proxy.model.gemini_2_5_pro',
        group: 'Gemini 2.5',
        tags: ['pro'],
    },

    // Claude 系列
    'claude-sonnet-4-6': {
        label: 'Claude 4.6',
        shortLabel: 'Claude 4.6',
        protectedKey: 'claude',
        Icon: Claude.Color,
        i18nKey: 'proxy.model.claude_sonnet',
        i18nDescKey: 'proxy.model.claude_sonnet',
        group: 'Claude',
        tags: ['sonnet'],
    },
    'claude-sonnet-4-6-thinking': {
        label: 'Claude 4.6 TK',
        shortLabel: 'Claude 4.6 TK',
        protectedKey: 'claude',
        Icon: Claude.Color,
        i18nKey: 'proxy.model.claude_sonnet_thinking',
        i18nDescKey: 'proxy.model.claude_sonnet_thinking',
        group: 'Claude',
        tags: ['sonnet', 'thinking'],
    },
    'claude-opus-4-6-thinking': {
        label: 'Claude Opus 4.6 TK',
        shortLabel: 'Claude Opus 4.6 TK',
        protectedKey: 'claude',
        Icon: Claude.Color,
        i18nKey: 'proxy.model.claude_opus_thinking',
        i18nDescKey: 'proxy.model.claude_opus_thinking',
        group: 'Claude',
        tags: ['opus', 'thinking'],
    },
};

/**
 * 获取所有模型 ID 列表
 */
export const getAllModelIds = (): string[] => Object.keys(MODEL_CONFIG);

/**
 * 根据模型 ID 获取配置
 */
export const getModelConfig = (modelId: string): ModelConfig | undefined => {
    return MODEL_CONFIG[modelId.toLowerCase()];
};

/**
 * 模型排序权重配置
 * 数字越小，优先级越高
 */
const MODEL_SORT_WEIGHTS = {
    // 系列权重 (第一优先级)
    series: {
        'gemini-3': 100,
        'gemini-2.5': 200,
        'gemini-2': 300,
        'claude': 400,
    },
    // 性能级别权重 (第二优先级)
    tier: {
        'pro': 10,
        'flash': 20,
        'lite': 30,
        'opus': 5,
        'sonnet': 10,
    },
    // 特殊后缀权重 (第三优先级)
    suffix: {
        'thinking': 1,
        'image': 2,
        'high': 0,
        'low': 3,
    }
};

/**
 * 获取模型的排序权重
 */
function getModelSortWeight(modelId: string): number {
    const id = modelId.toLowerCase();
    let weight = 0;

    // 1. 系列权重 (x1000)
    if (id.startsWith('gemini-3')) {
        weight += MODEL_SORT_WEIGHTS.series['gemini-3'] * 1000;
    } else if (id.startsWith('gemini-2.5')) {
        weight += MODEL_SORT_WEIGHTS.series['gemini-2.5'] * 1000;
    } else if (id.startsWith('gemini-2')) {
        weight += MODEL_SORT_WEIGHTS.series['gemini-2'] * 1000;
    } else if (id.startsWith('claude')) {
        weight += MODEL_SORT_WEIGHTS.series['claude'] * 1000;
    }

    // 2. 性能级别权重 (x100)
    if (id.includes('pro')) {
        weight += MODEL_SORT_WEIGHTS.tier['pro'] * 100;
    } else if (id.includes('flash')) {
        weight += MODEL_SORT_WEIGHTS.tier['flash'] * 100;
    } else if (id.includes('lite')) {
        weight += MODEL_SORT_WEIGHTS.tier['lite'] * 100;
    } else if (id.includes('opus')) {
        weight += MODEL_SORT_WEIGHTS.tier['opus'] * 100;
    } else if (id.includes('sonnet')) {
        weight += MODEL_SORT_WEIGHTS.tier['sonnet'] * 100;
    }

    // 3. 特殊后缀权重 (x10)
    if (id.includes('thinking')) {
        weight += MODEL_SORT_WEIGHTS.suffix['thinking'] * 10;
    } else if (id.includes('image')) {
        weight += MODEL_SORT_WEIGHTS.suffix['image'] * 10;
    } else if (id.includes('high')) {
        weight += MODEL_SORT_WEIGHTS.suffix['high'] * 10;
    } else if (id.includes('low')) {
        weight += MODEL_SORT_WEIGHTS.suffix['low'] * 10;
    }

    return weight;
}

/**
 * 对模型列表进行排序
 * @param models 模型列表
 * @returns 排序后的模型列表
 */
export function sortModels<T extends { id: string }>(models: T[]): T[] {
    return [...models].sort((a, b) => {
        const weightA = getModelSortWeight(a.id);
        const weightB = getModelSortWeight(b.id);

        // 按权重升序排序
        if (weightA !== weightB) {
            return weightA - weightB;
        }

        // 权重相同时，按字母顺序排序
        return a.id.localeCompare(b.id);
    });
}
