import { Gemini, Claude, OpenAI } from '@lobehub/icons';
import type { FC } from 'react';

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type IconComponent = FC<any>;

/**
 * 模型配置接口
 */
export interface ModelConfig {
    /** 显示标签 */
    label: string;
    /** 短标签 */
    shortLabel: string;
    /** 配额保护键 */
    protectedKey: string;
    /** 图标组件 */
    Icon: IconComponent;
    /** i18n 键 */
    i18nKey?: string;
    /** i18n 描述键 */
    i18nDescKey?: string;
    /** 分组 */
    group?: string;
    /** 标签列表 */
    tags?: string[];
}

/**
 * 模型配置映射
 * 键为模型 ID，值为模型配置
 */
export const MODEL_CONFIG: Record<string, ModelConfig> = {
    // Gemini 3.1 系列
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

    // Gemini 3 系列
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

    // Gemini 2.5 系列 (backward compatible)
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

    // Claude 4.6 系列 (注意: Sonnet 用 claude-sonnet-4-6, Opus 用 claude-opus-4-6-thinking)
    'claude-sonnet-4-6': {
        label: 'Claude Sonnet 4.6',
        shortLabel: 'Sonnet 4.6',
        protectedKey: 'claude',
        Icon: Claude.Color,
        i18nKey: 'proxy.model.claude_sonnet',
        i18nDescKey: 'proxy.model.claude_sonnet',
        group: 'Claude',
        tags: ['sonnet'],
    },
    'claude-opus-4-6-thinking': {
        label: 'Claude Opus 4.6 TK',
        shortLabel: 'Opus 4.6 TK',
        protectedKey: 'claude',
        Icon: Claude.Color,
        i18nKey: 'proxy.model.claude_opus_thinking',
        i18nDescKey: 'proxy.model.claude_opus_thinking',
        group: 'Claude',
        tags: ['opus', 'thinking'],
    },

    // GPT-OSS 系列
    'gpt-oss-120b': {
        label: 'GPT-OSS 120B',
        shortLabel: 'OSS 120B',
        protectedKey: 'gpt-oss',
        Icon: OpenAI as unknown as IconComponent,
        i18nKey: 'proxy.model.gpt_oss',
        i18nDescKey: 'proxy.model.gpt_oss',
        group: 'GPT-OSS',
        tags: ['oss'],
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
        'gemini-3.1': 50,
        'gemini-3': 100,
        'gemini-2.5': 200,
        'gemini-2': 300,
        'claude': 400,
        'gpt-oss': 500,
    },
    // 性能级别权重 (第二优先级)
    tier: {
        'pro': 10,
        'flash': 20,
        'lite': 30,
        'opus': 5,
        'sonnet': 10,
        'oss': 15,
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
    if (id.startsWith('gemini-3.1')) {
        weight += MODEL_SORT_WEIGHTS.series['gemini-3.1'] * 1000;
    } else if (id.startsWith('gemini-3')) {
        weight += MODEL_SORT_WEIGHTS.series['gemini-3'] * 1000;
    } else if (id.startsWith('gemini-2.5')) {
        weight += MODEL_SORT_WEIGHTS.series['gemini-2.5'] * 1000;
    } else if (id.startsWith('gemini-2')) {
        weight += MODEL_SORT_WEIGHTS.series['gemini-2'] * 1000;
    } else if (id.startsWith('claude')) {
        weight += MODEL_SORT_WEIGHTS.series['claude'] * 1000;
    } else if (id.startsWith('gpt-oss')) {
        weight += MODEL_SORT_WEIGHTS.series['gpt-oss'] * 1000;
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
    } else if (id.includes('oss')) {
        weight += MODEL_SORT_WEIGHTS.tier['oss'] * 100;
    }

    // 3. 特殊后缀权重
    if (id.includes('thinking')) {
        weight += MODEL_SORT_WEIGHTS.suffix['thinking'];
    } else if (id.includes('image')) {
        weight += MODEL_SORT_WEIGHTS.suffix['image'];
    }

    if (id.endsWith('high')) {
        weight += MODEL_SORT_WEIGHTS.suffix['high'];
    } else if (id.endsWith('low')) {
        weight += MODEL_SORT_WEIGHTS.suffix['low'];
    }

    return weight;
}

/**
 * 获取排序后的模型列表
 */
export const getSortedModelIds = (): string[] => {
    const modelIds = getAllModelIds();
    return modelIds.sort((a, b) => getModelSortWeight(a) - getModelSortWeight(b));
};

/**
 * 对模型数组按权重排序
 * @param models 包含 id 字段的模型数组
 */
export const sortModels = <T extends { id: string }>(models: T[]): T[] => {
    return [...models].sort((a, b) => getModelSortWeight(a.id) - getModelSortWeight(b.id));
};
