import { Gemini, Claude, OpenAI } from '@lobehub/icons';

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
    Icon: React.ComponentType<any>;
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
    'gemini-3.8-flash': {
        label: 'Gemini 3.8 Flash',
        shortLabel: 'G3.8 Flash',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash'],
    },
    'gemini-3.8-flash-tiered': {
        label: 'Gemini 3.8 Flash',
        shortLabel: 'G3.8 Flash',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash', 'tiered'],
    },
    'gemini-3.8-flash-high': {
        label: 'Gemini 3.8 Flash (High)',
        shortLabel: 'G3.8 High',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash', 'high'],
    },
    'gemini-3.8-flash-medium': {
        label: 'Gemini 3.8 Flash (Medium)',
        shortLabel: 'G3.8 Med',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash', 'medium'],
    },
    'gemini-3.8-flash-low': {
        label: 'Gemini 3.8 Flash (Low)',
        shortLabel: 'G3.8 Low',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash', 'low'],
    },
    'gemini-3.7-flash-high': {
        label: 'Gemini 3.7 Flash (High)',
        shortLabel: 'G3.7 High',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash', 'high'],
    },
    'gemini-3.7-flash-medium': {
        label: 'Gemini 3.7 Flash (Medium)',
        shortLabel: 'G3.7 Med',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash', 'medium'],
    },
    'gemini-3.7-flash-low': {
        label: 'Gemini 3.7 Flash (Low)',
        shortLabel: 'G3.7 Low',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash', 'low'],
    },
    'gemini-3.7-flash-tiered': {
        label: 'Gemini 3.7 Flash (Tiered)',
        shortLabel: 'G3.7 Tiered',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash', 'tiered'],
    },
    // Gemini 3.6 系列
    'gemini-3.6-flash': {
        label: 'Gemini 3.6 Flash',
        shortLabel: 'G3.6 Flash',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash'],
    },
    'gemini-3.6-flash-tiered': {
        label: 'Gemini 3.6 Flash (Tiered)',
        shortLabel: 'G3.6 Tiered',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash', 'tiered'],
    },
    'gemini-3.6-flash-high': {
        label: 'Gemini 3.6 Flash (High)',
        shortLabel: 'G3.6 High',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash', 'high'],
    },
    'gemini-3.6-flash-medium': {
        label: 'Gemini 3.6 Flash (Medium)',
        shortLabel: 'G3.6 Med',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash', 'medium'],
    },
    'gemini-3.6-flash-low': {
        label: 'Gemini 3.6 Flash (Low)',
        shortLabel: 'G3.6 Low',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash', 'low'],
    },

    // Gemini 3.5 系列
    'gemini-3.5-flash': {
        label: 'Gemini 3.5 Flash',
        shortLabel: 'G3.5 Flash',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash'],
    },
    'gemini-3.5-flash-low': {
        label: 'Gemini 3.5 Flash (Medium)',
        shortLabel: 'G3.5 Med',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash', 'medium'],
    },
    'gemini-3.5-flash-extra-low': {
        label: 'Gemini 3.5 Flash (Low)',
        shortLabel: 'G3.5 Low',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_preview',
        i18nDescKey: 'proxy.model.flash_preview',
        group: 'Gemini 3',
        tags: ['flash', 'low'],
    },

    // Gemini 3.1 Pro & 图像
    'gemini-3.1-pro': {
        label: 'Gemini 3.1 Pro',
        shortLabel: 'G3.1 Pro',
        protectedKey: 'gemini-pro',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.pro',
        i18nDescKey: 'proxy.model.pro',
        group: 'Gemini 3',
        tags: ['pro'],
    },
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
    'gemini-3.1-flash-lite': {
        label: 'Gemini 3.1 Flash Lite',
        shortLabel: 'G3.1 Lite',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_lite',
        i18nDescKey: 'proxy.model.flash_lite',
        group: 'Gemini 3',
        tags: ['flash', 'lite'],
    },
    'gemini-3.1-flash-image': {
        label: 'Gemini 3.1 Flash Image',
        shortLabel: 'G3.1 Image',
        protectedKey: 'gemini-3.1-flash-image',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.pro_image',
        i18nDescKey: 'proxy.model.pro_image_1_1',
        group: 'Gemini 3',
        tags: ['image', 'flash'],
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
    'claude-opus-4-6': {
        label: 'Claude Opus 4.6',
        shortLabel: 'Claude Opus 4.6',
        protectedKey: 'claude',
        Icon: Claude.Color,
        i18nKey: 'proxy.model.claude_opus',
        i18nDescKey: 'proxy.model.claude_opus',
        group: 'Claude',
        tags: ['opus'],
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

    // 官方 Agent 与预览模型
    'gemini-pro-agent': {
        label: 'Gemini Pro Agent',
        shortLabel: 'Pro Agent',
        protectedKey: 'gemini-pro',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.pro_agent',
        i18nDescKey: 'proxy.model.pro_agent',
        group: 'Gemini 3',
        tags: ['pro', 'agent'],
    },
    'gemini-3-flash-agent': {
        label: 'Gemini 3 Flash Agent',
        shortLabel: 'Flash Agent',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.flash_agent',
        i18nDescKey: 'proxy.model.flash_agent',
        group: 'Gemini 3',
        tags: ['flash', 'agent'],
    },
    'tab_flash_lite_preview': {
        label: 'Tab Flash Lite Preview',
        shortLabel: 'Tab Flash Lite',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.tab_flash_lite',
        i18nDescKey: 'proxy.model.tab_flash_lite',
        group: 'Gemini 3',
        tags: ['flash', 'preview'],
    },
    'tab_jump_flash_lite_preview': {
        label: 'Tab Jump Flash Lite Preview',
        shortLabel: 'Tab Jump Lite',
        protectedKey: 'gemini-flash',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.tab_jump_flash_lite',
        i18nDescKey: 'proxy.model.tab_jump_flash_lite',
        group: 'Gemini 3',
        tags: ['flash', 'preview'],
    },

    // OpenAI / Outros modelos
    'gpt-oss-120b-medium': {
        label: 'GPT-OSS 120B (Medium)',
        shortLabel: 'GPT-OSS',
        protectedKey: 'gpt-oss',
        Icon: OpenAI.Avatar,
        i18nKey: 'proxy.model.gpt_oss',
        i18nDescKey: 'proxy.model.gpt_oss',
        group: 'Other',
        tags: ['openai'],
    },
    'gemini-5h': {
        label: 'Gemini 5h Limit',
        shortLabel: 'Gemini 5h',
        protectedKey: 'gemini-5h',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.gemini_5h',
        i18nDescKey: 'proxy.model.gemini_5h',
        group: 'Quota Groups',
    },
    'gemini-weekly': {
        label: 'Gemini Weekly Limit',
        shortLabel: 'Gemini 周额度',
        protectedKey: 'gemini-weekly',
        Icon: Gemini.Color,
        i18nKey: 'proxy.model.gemini_weekly',
        i18nDescKey: 'proxy.model.gemini_weekly',
        group: 'Quota Groups',
    },
    '3p-5h': {
        label: 'Claude/GPT 5h Limit',
        shortLabel: 'Claude/GPT 5h',
        protectedKey: '3p-5h',
        Icon: Claude.Color,
        i18nKey: 'proxy.model.3p_5h',
        i18nDescKey: 'proxy.model.3p_5h',
        group: 'Quota Groups',
    },
    '3p-weekly': {
        label: 'Claude/GPT Weekly Limit',
        shortLabel: 'Claude/GPT 周额度',
        protectedKey: '3p-weekly',
        Icon: Claude.Color,
        i18nKey: 'proxy.model.3p_weekly',
        i18nDescKey: 'proxy.model.3p_weekly',
        group: 'Quota Groups',
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

// ── 模型版本解析与排序（实现在 src/utils/modelSort.ts，此处统一 re-export 保证向后兼容）───

export {
    CANONICAL_GROUP_ORDER,
    QUOTA_BUCKET_ORDER,
    extractModelVersion,
    getGroupPriority,
    inferModelGroup,
    getModelTierPriority,
    compareModelsDesc,
    sortModels,
} from '../utils/modelSort';

// ── 模型分类与保护键（实现在 src/utils/modelCategory.ts，此处只 re-export）───

export {
    categorizeModel,
    getModelProtectionKey,
    getModelDisplayName,
    findQuotaModel,
    findImageQuotaModel,
    ensurePinnedImageSelector,
    DEFAULT_IMAGE_PIN_SELECTOR,
    resolveQuotaModels,
    type ModelCategory,
    type QuotaModelSelection,
} from '../utils/modelCategory';
