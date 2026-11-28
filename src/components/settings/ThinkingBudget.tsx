import { useState, useEffect } from "react";
import { useTranslation, Trans } from "react-i18next";
import { Save, Check, Layers, HelpCircle, HardDrive, Trash2, AlertTriangle, ArrowRight, RotateCcw, Sliders } from "lucide-react";
import { request } from "../../utils/request";
import { showToast } from "../common/ToastContainer";
import {
    ThinkingBudgetConfig,
    ThinkingModeMappingConfig,
    ThinkingModeSourceKey,
    ThinkingModeTargetValue,
} from "../../types/config";

const DEFAULT_MODE_MAPPING_RULES: Record<ThinkingModeSourceKey, ThinkingModeTargetValue> = {
    default: "auto",
    off: "off",
    minimal: "minimal",
    low: "low",
    medium: "medium",
    high: "high",
    xhigh: "xhigh",
    max: "xhigh",
};

const SOURCE_MODE_ITEMS: Array<{
    key: ThinkingModeSourceKey;
    labelKey: string;
    labelDefault: string;
    hintKey: string;
    hintDefault: string;
}> = [
    {
        key: "default",
        labelKey: "proxy.config.thinking_budget.mode_source_default",
        labelDefault: "缺省 / Auto",
        hintKey: "proxy.config.thinking_budget.mode_source_default_hint",
        hintDefault: "未传或 default / adaptive",
    },
    {
        key: "off",
        labelKey: "proxy.config.thinking_budget.mode_source_off",
        labelDefault: "关闭 / Off",
        hintKey: "proxy.config.thinking_budget.mode_source_off_hint",
        hintDefault: "disabled / off / none / budget=0",
    },
    {
        key: "minimal",
        labelKey: "proxy.config.thinking_budget.mode_source_minimal",
        labelDefault: "Minimal",
        hintKey: "proxy.config.thinking_budget.mode_source_minimal_hint",
        hintDefault: "minimal / min / extra_low",
    },
    {
        key: "low",
        labelKey: "proxy.config.thinking_budget.mode_source_low",
        labelDefault: "Low",
        hintKey: "proxy.config.thinking_budget.mode_source_low_hint",
        hintDefault: "low / 1",
    },
    {
        key: "medium",
        labelKey: "proxy.config.thinking_budget.mode_source_medium",
        labelDefault: "Medium",
        hintKey: "proxy.config.thinking_budget.mode_source_medium_hint",
        hintDefault: "medium / normal / 2",
    },
    {
        key: "high",
        labelKey: "proxy.config.thinking_budget.mode_source_high",
        labelDefault: "High",
        hintKey: "proxy.config.thinking_budget.mode_source_high_hint",
        hintDefault: "high / 3 / 4",
    },
    {
        key: "xhigh",
        labelKey: "proxy.config.thinking_budget.mode_source_xhigh",
        labelDefault: "XHigh",
        hintKey: "proxy.config.thinking_budget.mode_source_xhigh_hint",
        hintDefault: "xhigh / x-high / 100",
    },
    {
        key: "max",
        labelKey: "proxy.config.thinking_budget.mode_source_max",
        labelDefault: "Max",
        hintKey: "proxy.config.thinking_budget.mode_source_max_hint",
        hintDefault: "max / extreme (Claude Code Max)",
    },
];

const TARGET_MODE_OPTIONS: Array<{
    value: ThinkingModeTargetValue;
    labelKey: string;
    labelDefault: string;
}> = [
    { value: "auto", labelKey: "proxy.config.thinking_budget.mode_target_auto", labelDefault: "跟随模型默认 (Auto)" },
    { value: "off", labelKey: "proxy.config.thinking_budget.mode_target_off", labelDefault: "关闭思考 (Off)" },
    { value: "minimal", labelKey: "proxy.config.thinking_budget.mode_target_minimal", labelDefault: "极低 (MINIMAL)" },
    { value: "low", labelKey: "proxy.config.thinking_budget.mode_target_low", labelDefault: "低 (LOW)" },
    { value: "medium", labelKey: "proxy.config.thinking_budget.mode_target_medium", labelDefault: "中 (MEDIUM)" },
    { value: "high", labelKey: "proxy.config.thinking_budget.mode_target_high", labelDefault: "高 (HIGH)" },
    { value: "xhigh", labelKey: "proxy.config.thinking_budget.mode_target_xhigh", labelDefault: "极高 (XHIGH / 100)" },
];

function normalizeModeMappingConfig(config?: ThinkingBudgetConfig): ThinkingModeMappingConfig {
    const raw = config?.mode_mapping;
    const rules: Record<string, ThinkingModeTargetValue> = { ...DEFAULT_MODE_MAPPING_RULES };
    if (raw?.rules) {
        for (const [k, v] of Object.entries(raw.rules)) {
            if (k in DEFAULT_MODE_MAPPING_RULES && typeof v === "string") {
                rules[k] = v as ThinkingModeTargetValue;
            }
        }
    }
    return {
        enabled: raw?.enabled ?? true,
        override_suffixed_models: raw?.override_suffixed_models ?? false,
        rules,
    };
}

interface ThinkingBudgetProps {
    config?: ThinkingBudgetConfig;
    onChange: (config: ThinkingBudgetConfig) => void;
    onSave?: () => Promise<void> | void;
    thinkingStoreEnabled?: boolean;
    onThinkingStoreChange?: (enabled: boolean) => void;
    thinkingMaxMemoryTurns?: number;
    onThinkingMaxMemoryTurnsChange?: (turns: number) => void;
    thinkingRetentionDays?: number;
    onThinkingRetentionDaysChange?: (days: number) => void;
    globalCompactionStripEnabled?: boolean;
    onGlobalCompactionStripChange?: (enabled: boolean) => void;
}

interface ConcurrencyGuidePreset {
    key: string;
    icon: string;
    titleKey: string;
    titleDefault: string;
    descKey: string;
    descDefault: string;
}

const CONCURRENCY_GUIDE_PRESETS: ConcurrencyGuidePreset[] = [
    {
        key: "1g",
        icon: "🖥️",
        titleKey: "proxy.config.thinking_budget.guide_preset_1g_title",
        titleDefault: "1GB 内存轻量服务器:",
        descKey: "proxy.config.thinking_budget.guide_preset_1g_desc",
        descDefault: "推荐填写 <1>100 ~ 200 轮</1>。几百个并发会话仅消耗约 50MB 内存，极端抗爆。",
    },
    {
        key: "team",
        icon: "👥",
        titleKey: "proxy.config.thinking_budget.guide_preset_team_title",
        titleDefault: "个人 ~ 10 人自用团队:",
        descKey: "proxy.config.thinking_budget.guide_preset_team_desc",
        descDefault: "推荐填写 <1>600 ~ 1000 轮</1>。数千轮历史对话常驻物理内存 0ms 闪电直出。",
    },
    {
        key: "enterprise",
        icon: "🏢",
        titleKey: "proxy.config.thinking_budget.guide_preset_enterprise_title",
        titleDefault: "100 人企业级并发 (2G-4G):",
        descKey: "proxy.config.thinking_budget.guide_preset_enterprise_desc",
        descDefault: "推荐填写 <1>300 ~ 600 轮</1>。95%+ 请求命中 RAM，兼具极致性能与绝对稳健。",
    },
    {
        key: "relay",
        icon: "🌐",
        titleKey: "proxy.config.thinking_budget.guide_preset_relay_title",
        titleDefault: "1K+ 用户公共中转站 (4G-8G):",
        descKey: "proxy.config.thinking_budget.guide_preset_relay_desc",
        descDefault: "推荐填写 <1>150 ~ 300 轮</1>。内存优先倾斜给长连接池，长对话冷历史托付 SQLite。",
    },
    {
        key: "cluster",
        icon: "🚀",
        titleKey: "proxy.config.thinking_budget.guide_preset_cluster_title",
        titleDefault: "1W+ ~ 10W+ 海量并发集群:",
        descKey: "proxy.config.thinking_budget.guide_preset_cluster_desc",
        descDefault: "推荐填写 <1>50 ~ 100 轮</1>。单机无压承载数万并发会话，WAL 高速索引并发无锁秒级响应。",
    },
];

export default function ThinkingBudget({
    config,
    onChange,
    onSave,
    thinkingStoreEnabled = true,
    onThinkingStoreChange,
    thinkingMaxMemoryTurns = 600,
    onThinkingMaxMemoryTurnsChange,
    thinkingRetentionDays = 15,
    onThinkingRetentionDaysChange,
    globalCompactionStripEnabled = false,
    onGlobalCompactionStripChange,
}: ThinkingBudgetProps) {
    const { t } = useTranslation();
    const [isSaving, setIsSaving] = useState(false);
    const [isSaved, setIsSaved] = useState(false);
    const [isClearingThinking, setIsClearingThinking] = useState(false);
    const [showClearThinkingConfirm, setShowClearThinkingConfirm] = useState(false);
    const [signatureCacheStats, setSignatureCacheStats] = useState<{
        profile?: { name?: string; l1_limit?: number; read_pool_size?: number; retention_days?: number; total_memory_mb?: number; cores?: number };
        stats?: { l1_entries?: number; l1_hits?: number; l2_hits?: number; misses?: number; saves_dropped?: number; pending_deletes?: number; batch_calls?: number };
        thinking?: { sessions?: number; memory_turns?: number; memory_bytes?: number };
    } | null>(null);
    const [isClearingSignatureCache, setIsClearingSignatureCache] = useState(false);

    const loadSignatureCacheStats = async () => {
        try {
            const res = await request<any>("get_signature_cache_stats");
            setSignatureCacheStats(res);
        } catch {
            // 统计失败不影响设置页使用
        }
    };

    useEffect(() => {
        void loadSignatureCacheStats();
    }, []);

    const handleClearSignatureCache = async () => {
        if (!window.confirm(t("proxy.config.thinking_budget.clear_signature_confirm", {
            defaultValue: "确定清空工具签名缓存吗？不会影响请求日志与思考块。",
        }))) {
            return;
        }
        setIsClearingSignatureCache(true);
        try {
            const res = await request<number | { cleared?: number }>("clear_signature_cache");
            const count = typeof res === "number" ? res : res?.cleared ?? 0;
            showToast(
                t("proxy.config.thinking_budget.clear_signature_success", {
                    count,
                    defaultValue: `已清空签名缓存 (${count} 条 L1 记录)`,
                }),
                "success"
            );
            await loadSignatureCacheStats();
        } catch (err: any) {
            showToast(
                t("proxy.config.thinking_budget.clear_signature_error", {
                    error: String(err),
                    defaultValue: `清空签名缓存失败: ${String(err)}`,
                }),
                "error"
            );
        } finally {
            setIsClearingSignatureCache(false);
        }
    };

    const handleClearThinkingStore = async () => {
        setIsClearingThinking(true);
        try {
            const res = await request<number | { deleted?: number }>("clear_thinking_store");
            const count = typeof res === "number" ? res : res?.deleted ?? 0;
            showToast(
                t("proxy.config.thinking_budget.clear_success", {
                    count,
                    defaultValue: `已成功清空思考块存储 (共清理 ${count} 条历史记录)`,
                }),
                "success"
            );
            setShowClearThinkingConfirm(false);
        } catch (err: any) {
            showToast(
                t("proxy.config.thinking_budget.clear_error", {
                    error: String(err),
                    defaultValue: `清空思考块失败: ${String(err)}`,
                }),
                "error"
            );
        } finally {
            setIsClearingThinking(false);
        }
    };

    // 内存思考轮次与持久化天数输入框本地文本状态，允许自由编辑退格，失焦时自动校验归一化
    const [memoryTurnsInput, setMemoryTurnsInput] = useState<string>(() => String(thinkingMaxMemoryTurns));
    const [retentionDaysInput, setRetentionDaysInput] = useState<string>(() => String(thinkingRetentionDays));

    useEffect(() => {
        setMemoryTurnsInput(String(thinkingMaxMemoryTurns));
    }, [thinkingMaxMemoryTurns]);

    useEffect(() => {
        setRetentionDaysInput(String(thinkingRetentionDays));
    }, [thinkingRetentionDays]);

    // 保存：只落盘思考存储/压缩开关等设置（档位已由官方结构体驱动，无需本地校准）
    const handleSave = async () => {
        if (!onSave) return;
        setIsSaving(true);
        try {
            await onSave();
            setIsSaved(true);
            setTimeout(() => setIsSaved(false), 2000);
        } finally {
            setIsSaving(false);
        }
    };

    return (
        <div className="space-y-4">
            {/* Header & Section Title */}
            <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 border-b border-gray-200 dark:border-base-200 pb-2.5">
                <div>
                    <h4 className="text-xs font-bold text-gray-900 dark:text-white flex items-center gap-1.5">
                        <svg
                            className="w-4 h-4 text-purple-500 dark:text-purple-400"
                            fill="none"
                            stroke="currentColor"
                            viewBox="0 0 24 24"
                        >
                            <path
                                strokeLinecap="round"
                                strokeLinejoin="round"
                                strokeWidth="2"
                                d="M9.663 17h4.673M12 3v1m6.364 1.636l-.707.707M21 12h-1M4 12H3m3.343-5.657l-.707-.707m2.828 9.9a5 5 0 117.072 0l-.548.547A3.374 3.374 0 0014 18.469V19a2 2 0 11-4 0v-.531c0-.895-.356-1.754-.988-2.386l-.548-.547z"
                            />
                        </svg>
                        {t("proxy.config.thinking_budget.title", {
                            defaultValue: "思考链预算控制 (Thinking Budget)",
                        })}
                    </h4>
                    <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                        {t("proxy.config.thinking_budget.description", {
                            defaultValue:
                                "跨协议归一化思考参数，统一调控 Gemini 与 Claude 模型的深度思考 Token 预算及自适应行为。",
                        })}
                    </p>
                </div>
            </div>

            {/* 0. 服务端思考块回填 (Server-side Thought & Signature Restoration) */}
            {onThinkingStoreChange && (
                <div className="p-3.5 bg-purple-50/70 dark:bg-purple-900/20 border border-purple-200/80 dark:border-purple-800/40 rounded-xl flex items-center justify-between gap-4 shadow-2xs">
                    <div className="space-y-0.5">
                        <div className="flex items-center gap-2">
                            <span className="text-xs font-bold text-gray-900 dark:text-white">
                                {t("proxy.config.thinking_budget.store_enabled", { defaultValue: "服务端思考块与签名回填" })}
                            </span>
                            <span className="px-2 py-0.5 rounded-full text-[10px] font-bold bg-purple-100 dark:bg-purple-900/40 text-purple-700 dark:text-purple-300 border border-purple-200 dark:border-purple-800/60">
                                {t("proxy.config.thinking_budget.default_on_tag", { defaultValue: "默认开启" })}
                            </span>
                        </div>
                        <p className="text-xs text-gray-600 dark:text-gray-300 max-w-xl leading-relaxed">
                            {t("proxy.config.thinking_budget.store_enabled_desc", {
                                defaultValue: "自动截获并持久化思考过程与加密签名；当商业 Agent（如 Cline, Roo, Claude Code 等）多轮对话未带回思考时由服务端自动补齐，杜绝上游 400 报错或上下文丢失。"
                            })}
                        </p>
                    </div>
                    <input
                        type="checkbox"
                        className="toggle toggle-sm toggle-primary shrink-0"
                        checked={thinkingStoreEnabled}
                        onChange={(e) => onThinkingStoreChange(e.target.checked)}
                    />
                </div>
            )}

            {/* 0.1 全局压缩剥离签名 (Global Compaction Signature Stripping) */}
            {onGlobalCompactionStripChange && (
                <div className="p-3.5 bg-amber-50/70 dark:bg-amber-900/20 border border-amber-200/80 dark:border-amber-800/40 rounded-xl flex items-center justify-between gap-4 shadow-2xs">
                    <div className="space-y-0.5">
                        <div className="flex items-center gap-2">
                            <span className="text-xs font-bold text-gray-900 dark:text-white">
                                {t("proxy.config.thinking_budget.global_compaction_strip", { defaultValue: "全局压缩剥离签名" })}
                            </span>
                            <span className="px-2 py-0.5 rounded-full text-[10px] font-bold bg-amber-100 dark:bg-amber-900/40 text-amber-700 dark:text-amber-300 border border-amber-200 dark:border-amber-800/60">
                                {t("proxy.config.thinking_budget.experimental_tag", { defaultValue: "试验性" })}
                            </span>
                            <span className="px-2 py-0.5 rounded-full text-[10px] font-bold bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-400 border border-gray-200 dark:border-gray-700">
                                {t("proxy.config.thinking_budget.default_off_tag", { defaultValue: "默认关闭" })}
                            </span>
                        </div>
                        <p className="text-xs text-gray-600 dark:text-gray-300 max-w-xl leading-relaxed">
                            {t("proxy.config.thinking_budget.global_compaction_strip_desc", {
                                defaultValue: "默认情况下保持关闭，仅对 Claude Code CLI 与 Desktop (Cowork) 的特定压缩特征触发签名剥离。开启后全局生效（协议无关、客户端无关），自动识别会话上下文回落与就地折叠，隔离失效签名并以官方哨兵锁定。"
                            })}
                        </p>
                    </div>
                    <input
                        type="checkbox"
                        className="toggle toggle-sm toggle-warning shrink-0"
                        checked={globalCompactionStripEnabled}
                        onChange={(e) => onGlobalCompactionStripChange(e.target.checked)}
                    />
                </div>
            )}

            {/* 每轮对话的内存thinking与SQLite存储滑动窗口 */}
            {(onThinkingMaxMemoryTurnsChange || onThinkingRetentionDaysChange) && (
                <div className="p-3.5 bg-blue-50/70 dark:bg-blue-900/20 border border-blue-200/80 dark:border-blue-800/40 rounded-xl space-y-3 shadow-2xs">
                    <div className="flex items-center justify-between flex-wrap gap-2">
                        <div className="flex items-center gap-2">
                            <span className="text-xs font-bold text-gray-900 dark:text-white flex items-center gap-1.5">
                                <Layers size={14} className="text-blue-600 dark:text-blue-400" />
                                {t("proxy.config.thinking_budget.window_settings_title", {
                                    defaultValue: "思考块双层滑动窗口与容量配置 (RAM + SQLite)",
                                })}
                            </span>
                            <span className="px-2 py-0.5 rounded-full text-[10px] font-bold bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300 border border-blue-200 dark:border-blue-800/60">
                                {t("proxy.config.thinking_budget.dual_layer_tag", {
                                    defaultValue: "双层索引穿透",
                                })}
                            </span>
                        </div>

                        {/* 清空思考块按钮与小字提示 */}
                        <div className="flex items-center gap-2">
                            <span className="text-[10px] text-gray-500 dark:text-gray-400 hidden sm:inline-block">
                                {t("proxy.config.thinking_budget.clear_tip", {
                                    defaultValue: "仅当缓存命中异常、版本更新或开发者要求时才删除",
                                })}
                            </span>
                            <button
                                type="button"
                                onClick={() => setShowClearThinkingConfirm(true)}
                                disabled={isClearingThinking}
                                title={t("proxy.config.thinking_budget.clear_btn_tooltip", {
                                    defaultValue: "清空所有思考块缓存与数据库（不影响请求日志）",
                                })}
                                className="btn btn-xs btn-outline btn-error gap-1.5 h-6 min-h-6 px-2.5 text-[11px] font-medium rounded-lg shadow-2xs hover:shadow-xs transition-all"
                            >
                                <Trash2 size={12} className={isClearingThinking ? "animate-spin" : ""} />
                                {t("proxy.config.thinking_budget.clear_btn", {
                                    defaultValue: "清空思考块",
                                })}
                            </button>
                        </div>
                    </div>

                    <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                        {/* 1. 内存常驻思考轮次 (L1 RAM) */}
                        {onThinkingMaxMemoryTurnsChange && (
                            <div className="p-3 bg-white/90 dark:bg-base-100 rounded-lg border border-blue-200/70 dark:border-base-200 shadow-2xs flex items-center justify-between gap-3">
                                <div className="space-y-0.5 min-w-0">
                                    <div className="flex items-center gap-1.5">
                                        <Layers size={13} className="text-blue-600 dark:text-blue-400 shrink-0" />
                                        <span className="text-xs font-bold text-gray-800 dark:text-gray-200 truncate">
                                            {t("proxy.config.thinking_budget.max_memory_turns_label", {
                                                defaultValue: "内存常驻轮次 (RAM 窗口)",
                                            })}
                                        </span>
                                        <span className="text-[10px] px-1.5 py-0.2 rounded font-bold bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300 shrink-0">
                                            {t("proxy.config.thinking_budget.default_600_tag", {
                                                defaultValue: "默认 600",
                                            })}
                                        </span>
                                    </div>
                                    <p className="text-[11px] text-gray-500 dark:text-gray-400 leading-tight">
                                        {t("proxy.config.thinking_budget.max_memory_turns_subdesc", {
                                            defaultValue: "单轮思考约 2 KB；600 轮 ≈ 1.2 MB / 会话。",
                                        })}
                                    </p>
                                </div>
                                <div className="flex items-center gap-1.5 shrink-0">
                                    <input
                                        type="number"
                                        min={10}
                                        max={10000}
                                        step={50}
                                        className="input input-xs input-bordered w-20 text-center font-mono font-bold bg-gray-50 dark:bg-base-200 text-gray-900 dark:text-white"
                                        value={memoryTurnsInput}
                                        onChange={(e) => {
                                            setMemoryTurnsInput(e.target.value);
                                        }}
                                        onBlur={() => {
                                            const val = parseInt(memoryTurnsInput, 10);
                                            if (isNaN(val)) {
                                                setMemoryTurnsInput(String(thinkingMaxMemoryTurns));
                                            } else {
                                                const clamped = Math.max(10, Math.min(10000, val));
                                                setMemoryTurnsInput(String(clamped));
                                                onThinkingMaxMemoryTurnsChange(clamped);
                                            }
                                        }}
                                        onKeyDown={(e) => {
                                            if (e.key === "Enter") {
                                                (e.target as HTMLInputElement).blur();
                                            }
                                        }}
                                    />
                                    <span className="text-xs font-medium text-gray-500">
                                        {t("proxy.config.thinking_budget.turns_unit", { defaultValue: "轮" })}
                                    </span>
                                </div>
                            </div>
                        )}

                        {/* 2. SQLite 思考库保留周期 (L2 Disk) */}
                        {onThinkingRetentionDaysChange && (
                            <div className="p-3 bg-white/90 dark:bg-base-100 rounded-lg border border-purple-200/70 dark:border-base-200 shadow-2xs flex items-center justify-between gap-3">
                                <div className="space-y-0.5 min-w-0">
                                    <div className="flex items-center gap-1.5">
                                        <HardDrive size={13} className="text-purple-600 dark:text-purple-400 shrink-0" />
                                        <span className="text-xs font-bold text-gray-800 dark:text-gray-200 truncate">
                                            {t("proxy.config.experimental.thinking_retention_days_label", {
                                                defaultValue: "思考库保留周期 (SQLite)",
                                            })}
                                        </span>
                                        <span className="text-[10px] px-1.5 py-0.2 rounded font-bold bg-purple-100 dark:bg-purple-900/40 text-purple-700 dark:text-purple-300 shrink-0">
                                            {t("proxy.config.thinking_budget.default_15_days_tag", {
                                                defaultValue: "默认 15天",
                                            })}
                                        </span>
                                    </div>
                                    <p className="text-[11px] text-gray-500 dark:text-gray-400 leading-tight">
                                        {t("proxy.config.thinking_budget.retention_days_subdesc", {
                                            defaultValue: "活跃会话每次请求自动顺延，无请求才过期。",
                                        })}
                                    </p>
                                </div>
                                <div className="flex items-center gap-1.5 shrink-0">
                                    <input
                                        type="number"
                                        min={1}
                                        max={365}
                                        className="input input-xs input-bordered w-20 text-center font-mono font-bold bg-gray-50 dark:bg-base-200 text-gray-900 dark:text-white"
                                        value={retentionDaysInput}
                                        onChange={(e) => {
                                            setRetentionDaysInput(e.target.value);
                                        }}
                                        onBlur={() => {
                                            const val = parseInt(retentionDaysInput, 10);
                                            if (isNaN(val)) {
                                                setRetentionDaysInput(String(thinkingRetentionDays));
                                            } else {
                                                const clamped = Math.max(1, Math.min(365, val));
                                                setRetentionDaysInput(String(clamped));
                                                onThinkingRetentionDaysChange(clamped);
                                            }
                                        }}
                                        onKeyDown={(e) => {
                                            if (e.key === "Enter") {
                                                (e.target as HTMLInputElement).blur();
                                            }
                                        }}
                                    />
                                    <span className="text-xs font-medium text-gray-500">
                                        {t("proxy.config.thinking_budget.days_unit", { defaultValue: "天" })}
                                    </span>
                                </div>
                            </div>
                        )}
                    </div>

                    {/* 签名缓存运行状态（只读）+ 清理入口 */}
                    <div className="p-3 rounded-lg bg-white/80 dark:bg-base-200/80 border border-blue-100 dark:border-blue-900/40 text-xs space-y-2 text-gray-600 dark:text-gray-300">
                        <div className="flex items-center justify-between gap-2 flex-wrap">
                            <div className="flex items-center gap-1.5 font-semibold text-blue-950 dark:text-blue-200">
                                <HardDrive size={14} className="text-blue-500 shrink-0" />
                                <span>
                                    {t("proxy.config.thinking_budget.signature_cache_title", {
                                        defaultValue: "工具签名缓存（自动）",
                                    })}
                                </span>
                                {signatureCacheStats?.profile?.name && (
                                    <span className="px-2 py-0.5 rounded-full text-[10px] font-bold bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300 border border-blue-200 dark:border-blue-800/60">
                                        {signatureCacheStats.profile.name}
                                    </span>
                                )}
                            </div>
                            <button
                                type="button"
                                onClick={handleClearSignatureCache}
                                disabled={isClearingSignatureCache}
                                className="btn btn-xs btn-outline btn-error gap-1.5 h-6 min-h-6 px-2.5 text-[11px] font-medium rounded-lg"
                            >
                                <Trash2 size={12} className={isClearingSignatureCache ? "animate-spin" : ""} />
                                {t("proxy.config.thinking_budget.clear_signature_btn", {
                                    defaultValue: "清空签名缓存",
                                })}
                            </button>
                        </div>
                        <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 text-[11px] pl-5">
                            <div>
                                <span className="text-gray-400">L1</span>{" "}
                                {signatureCacheStats?.stats?.l1_entries ?? "-"} / {signatureCacheStats?.profile?.l1_limit ?? "-"}
                            </div>
                            <div>
                                <span className="text-gray-400">
                                    {t("proxy.config.thinking_budget.signature_cache_hit_rate", {
                                        defaultValue: "命中率",
                                    })}
                                </span>{" "}
                                {(() => {
                                    const s = signatureCacheStats?.stats;
                                    const hits = (s?.l1_hits ?? 0) + (s?.l2_hits ?? 0);
                                    const total = hits + (s?.misses ?? 0);
                                    return total > 0 ? `${Math.round((hits / total) * 100)}%` : "-";
                                })()}
                            </div>
                            <div>
                                <span className="text-gray-400">
                                    {t("proxy.config.thinking_budget.signature_cache_thinking", {
                                        defaultValue: "思考块",
                                    })}
                                </span>{" "}
                                {t("proxy.config.thinking_budget.signature_cache_thinking_value", {
                                    turns: signatureCacheStats?.thinking?.memory_turns ?? "-",
                                    sessions: signatureCacheStats?.thinking?.sessions ?? "-",
                                    defaultValue: "{{turns}} 轮 / {{sessions}} 会话",
                                })}
                            </div>
                            <div>
                                <span className="text-gray-400">
                                    {t("proxy.config.thinking_budget.signature_cache_dropped", {
                                        defaultValue: "丢弃",
                                    })}
                                </span>{" "}
                                {signatureCacheStats?.stats?.saves_dropped ?? 0}
                            </div>
                            <div>
                                <span className="text-gray-400">
                                    {t("proxy.config.thinking_budget.signature_cache_pending", {
                                        defaultValue: "待删除",
                                    })}
                                </span>{" "}
                                {signatureCacheStats?.stats?.pending_deletes ?? 0}
                            </div>
                        </div>
                        <p className="leading-relaxed pl-5 text-[11px] text-gray-500 dark:text-gray-400">
                            {t("proxy.config.thinking_budget.signature_cache_desc", {
                                defaultValue: "按本机 CPU / 内存自动选择缓存档位；SQLite 仅做冷层，批量写入与压缩清理均在后台完成。",
                            })}
                            {t("proxy.config.thinking_budget.signature_cache_machine", {
                                mb: signatureCacheStats?.profile?.total_memory_mb ?? "-",
                                cores: signatureCacheStats?.profile?.cores ?? "-",
                                defaultValue: "本机 {{mb}} MB / {{cores}} 核。",
                            })}
                        </p>
                    </div>

                    {/* 滑动窗口机制与服务器内存配置建议指南 */}
                    <div className="p-3 rounded-lg bg-white/80 dark:bg-base-200/80 border border-blue-100 dark:border-blue-900/40 text-xs space-y-2 text-gray-600 dark:text-gray-300">
                        <div className="flex items-start gap-1.5 font-semibold text-blue-950 dark:text-blue-200">
                            <HelpCircle size={14} className="text-blue-500 shrink-0 mt-0.5" />
                            <span>
                                {t("proxy.config.thinking_budget.window_guide_title", {
                                    defaultValue: "滑动窗口淘汰机制与各并发规模选型建议",
                                })}
                                :
                            </span>
                        </div>
                        <p className="leading-relaxed pl-5 text-[11px] text-gray-500 dark:text-gray-400">
                            {t("proxy.config.thinking_budget.window_guide_desc", {
                                defaultValue:
                                    "超长会话超过此设定轮次时，系统自动执行滑动窗口先进先出（FIFO）淘汰；被淘汰轮次绝不降级为破坏缓存的占位符，而是由本地 SQLite 专属索引（primary_tool_id）在纳秒级精准穿透回捞，保证 Prompt Cache 100% 字节级严格对齐且绝不 OOM。",
                            })}
                        </p>
                        <div className="pl-5 grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-2 text-[11px] pt-1">
                            {CONCURRENCY_GUIDE_PRESETS.map((preset) => (
                                <div
                                    key={preset.key}
                                    className="p-2 rounded bg-blue-50/40 dark:bg-base-300/40 border border-blue-100/60 dark:border-base-300"
                                >
                                    <span className="font-bold text-gray-800 dark:text-gray-200 block">
                                        {preset.icon} {t(preset.titleKey, { defaultValue: preset.titleDefault })}
                                    </span>
                                    <span className="text-gray-500 dark:text-gray-400">
                                        <Trans
                                            i18nKey={preset.descKey}
                                            defaults={preset.descDefault}
                                            components={{ 1: <strong className="text-blue-600 dark:text-blue-400" /> }}
                                        />
                                    </span>
                                </div>
                            ))}
                        </div>
                    </div>
                </div>
            )}

            {/* 思考模式映射 (Cross-Protocol Thinking Mode Mapping) */}
            {(() => {
                const mapping = normalizeModeMappingConfig(config);
                const updateMapping = (patch: Partial<ThinkingModeMappingConfig>) => {
                    const nextMapping: ThinkingModeMappingConfig = {
                        ...mapping,
                        ...patch,
                        rules: patch.rules ? { ...patch.rules } : { ...mapping.rules },
                    };
                    onChange({
                        ...(config || {}),
                        mode_mapping: nextMapping,
                    });
                };
                const handleRuleChange = (sourceKey: ThinkingModeSourceKey, target: ThinkingModeTargetValue) => {
                    updateMapping({
                        rules: {
                            ...mapping.rules,
                            [sourceKey]: target,
                        },
                    });
                };
                const applyPreset = (
                    rules: Record<ThinkingModeSourceKey, ThinkingModeTargetValue>,
                    overrideSuffixed?: boolean
                ) => {
                    updateMapping({
                        enabled: true,
                        override_suffixed_models: overrideSuffixed ?? mapping.override_suffixed_models,
                        rules,
                    });
                };
                const isModified =
                    !mapping.enabled ||
                    mapping.override_suffixed_models ||
                    SOURCE_MODE_ITEMS.some((item) => mapping.rules[item.key] !== DEFAULT_MODE_MAPPING_RULES[item.key]);

                return (
                    <div
                        data-testid="thinking-mode-mapping-panel"
                        className="p-4 bg-blue-50/60 dark:bg-blue-950/20 border border-blue-200/80 dark:border-blue-800/40 rounded-xl space-y-3.5 shadow-2xs"
                    >
                        <div className="flex items-start justify-between gap-4">
                            <div className="space-y-1">
                                <div className="flex flex-wrap items-center gap-2">
                                    <Sliders size={14} className="text-blue-600 dark:text-blue-400 shrink-0" />
                                    <span className="text-xs font-bold text-gray-900 dark:text-white">
                                        {t("proxy.config.thinking_budget.mode_mapping_title", {
                                            defaultValue: "跨协议思考模式映射 (Thinking Mode Mapping)",
                                        })}
                                    </span>
                                    <span
                                        className={`px-2 py-0.5 rounded-full text-[10px] font-bold border ${
                                            mapping.enabled
                                                ? "bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300 border-blue-200 dark:border-blue-800/60"
                                                : "bg-gray-100 dark:bg-base-300 text-gray-500 dark:text-gray-400 border-gray-200 dark:border-base-300"
                                        }`}
                                    >
                                        {mapping.enabled
                                            ? t("proxy.config.thinking_budget.mode_mapping_active", { defaultValue: "单次映射生效中" })
                                            : t("proxy.config.thinking_budget.mode_mapping_disabled", { defaultValue: "已旁路透传" })}
                                    </span>
                                </div>
                                <p className="text-xs text-gray-600 dark:text-gray-300 leading-relaxed">
                                    {t("proxy.config.thinking_budget.mode_mapping_desc", {
                                        defaultValue:
                                            "统一接管 Claude / OpenAI / Responses / Gemini 传入的思考开关与强度（如 output_config.effort、reasoning_effort、thinkingLevel），在模型分档与 thinkingConfig 注入前执行单次映射，不再硬编码把 Max 压回 HIGH。",
                                    })}
                                </p>
                            </div>
                            <input
                                type="checkbox"
                                data-testid="thinking-mode-mapping-enabled"
                                className="toggle toggle-sm toggle-primary shrink-0 mt-0.5"
                                checked={mapping.enabled}
                                onChange={(e) => updateMapping({ enabled: e.target.checked })}
                            />
                        </div>

                        {/* 快捷预设 & 重置 */}
                        <div className="flex flex-wrap items-center justify-between gap-2 pt-0.5">
                            <div className="flex flex-wrap items-center gap-1.5">
                                <span className="text-[11px] font-medium text-gray-500 dark:text-gray-400 mr-1">
                                    {t("proxy.config.thinking_budget.mode_presets_label", { defaultValue: "快捷预设:" })}
                                </span>
                                <button
                                    type="button"
                                    data-testid="thinking-mode-preset-default"
                                    disabled={!mapping.enabled}
                                    onClick={() => applyPreset({ ...DEFAULT_MODE_MAPPING_RULES }, false)}
                                    className="px-2.5 py-1 rounded-lg text-[11px] font-medium bg-white dark:bg-base-100 hover:bg-blue-50 dark:hover:bg-blue-900/30 text-gray-700 dark:text-gray-200 border border-gray-200 dark:border-base-300 transition-colors disabled:opacity-50 cursor-pointer"
                                >
                                    {t("proxy.config.thinking_budget.mode_preset_default", { defaultValue: "官方对齐 (Max→XHigh)" })}
                                </button>
                                <button
                                    type="button"
                                    data-testid="thinking-mode-preset-force-xhigh"
                                    disabled={!mapping.enabled}
                                    onClick={() =>
                                        applyPreset({
                                            default: "xhigh",
                                            off: "low",
                                            minimal: "minimal",
                                            low: "low",
                                            medium: "medium",
                                            high: "high",
                                            xhigh: "xhigh",
                                            max: "xhigh",
                                        })
                                    }
                                    className="px-2.5 py-1 rounded-lg text-[11px] font-medium bg-white dark:bg-base-100 hover:bg-blue-50 dark:hover:bg-blue-900/30 text-gray-700 dark:text-gray-200 border border-gray-200 dark:border-base-300 transition-colors disabled:opacity-50 cursor-pointer"
                                >
                                    {t("proxy.config.thinking_budget.mode_preset_force_xhigh", { defaultValue: "默认拉满 (缺省→XHigh)" })}
                                </button>
                                <button
                                    type="button"
                                    data-testid="thinking-mode-preset-low-cost"
                                    disabled={!mapping.enabled}
                                    onClick={() =>
                                        applyPreset({
                                            default: "low",
                                            off: "off",
                                            minimal: "minimal",
                                            low: "low",
                                            medium: "low",
                                            high: "medium",
                                            xhigh: "high",
                                            max: "high",
                                        })
                                    }
                                    className="px-2.5 py-1 rounded-lg text-[11px] font-medium bg-white dark:bg-base-100 hover:bg-blue-50 dark:hover:bg-blue-900/30 text-gray-700 dark:text-gray-200 border border-gray-200 dark:border-base-300 transition-colors disabled:opacity-50 cursor-pointer"
                                >
                                    {t("proxy.config.thinking_budget.mode_preset_low_cost", { defaultValue: "轻量省额 (降一档)" })}
                                </button>
                            </div>

                            {isModified && (
                                <button
                                    type="button"
                                    data-testid="thinking-mode-reset"
                                    onClick={() =>
                                        updateMapping({
                                            enabled: true,
                                            override_suffixed_models: false,
                                            rules: { ...DEFAULT_MODE_MAPPING_RULES },
                                        })
                                    }
                                    className="inline-flex items-center gap-1 px-2.5 py-1 rounded-lg text-[11px] font-medium text-blue-600 dark:text-blue-400 hover:bg-blue-100/70 dark:hover:bg-blue-900/30 transition-colors cursor-pointer"
                                >
                                    <RotateCcw size={12} />
                                    <span>{t("proxy.config.thinking_budget.mode_reset_btn", { defaultValue: "恢复默认" })}</span>
                                </button>
                            )}
                        </div>

                        {/* 8 行客户端模式 -> 网关目标档位映射矩阵 */}
                        <div className={`grid grid-cols-1 md:grid-cols-2 gap-2.5 ${!mapping.enabled ? "opacity-55 pointer-events-none" : ""}`}>
                            {SOURCE_MODE_ITEMS.map((item) => {
                                const currentTarget = (mapping.rules[item.key] || DEFAULT_MODE_MAPPING_RULES[item.key]) as ThinkingModeTargetValue;
                                const isOverridden = currentTarget !== DEFAULT_MODE_MAPPING_RULES[item.key];
                                return (
                                    <div
                                        key={item.key}
                                        className={`flex items-center justify-between gap-2 px-3 py-2 rounded-lg border transition-colors ${
                                            isOverridden
                                                ? "bg-amber-50/70 dark:bg-amber-950/20 border-amber-300/80 dark:border-amber-700/60"
                                                : "bg-white/90 dark:bg-base-100/80 border-gray-200/80 dark:border-base-300"
                                        }`}
                                    >
                                        <div className="min-w-0">
                                            <div className="flex items-center gap-1.5">
                                                <span className="text-xs font-semibold text-gray-900 dark:text-white truncate">
                                                    {t(item.labelKey, { defaultValue: item.labelDefault })}
                                                </span>
                                            </div>
                                            <div className="text-[10px] font-mono text-gray-500 dark:text-gray-400 truncate">
                                                {t(item.hintKey, { defaultValue: item.hintDefault })}
                                            </div>
                                        </div>

                                        <ArrowRight size={13} className="text-gray-400 dark:text-gray-500 shrink-0" />

                                        <select
                                            data-testid={`thinking-mode-select-${item.key}`}
                                            aria-label={t(item.labelKey, { defaultValue: item.labelDefault })}
                                            disabled={!mapping.enabled}
                                            value={currentTarget}
                                            onChange={(e) => handleRuleChange(item.key, e.target.value as ThinkingModeTargetValue)}
                                            className="select select-bordered select-xs h-7 min-h-7 w-40 shrink-0 bg-white dark:bg-base-200 text-xs font-medium text-gray-900 dark:text-gray-100"
                                        >
                                            {TARGET_MODE_OPTIONS.map((opt) => (
                                                <option key={opt.value} value={opt.value}>
                                                    {t(opt.labelKey, { defaultValue: opt.labelDefault })}
                                                </option>
                                            ))}
                                        </select>
                                    </div>
                                );
                            })}
                        </div>

                        {/* 允许覆盖具名档位后缀开关 */}
                        <div className="p-3 rounded-lg bg-white/80 dark:bg-base-100/70 border border-blue-200/70 dark:border-base-300 flex items-center justify-between gap-4">
                            <div className="space-y-0.5 min-w-0">
                                <div className="flex items-center gap-2">
                                    <span className="text-xs font-semibold text-gray-900 dark:text-white">
                                        {t("proxy.config.thinking_budget.override_suffixed_title", {
                                            defaultValue: "允许映射/客户端强度覆盖具名档位后缀",
                                        })}
                                    </span>
                                    <span className="px-1.5 py-0.5 rounded text-[10px] font-mono bg-gray-100 dark:bg-base-300 text-gray-600 dark:text-gray-300">
                                        -high → -xhigh / -low
                                    </span>
                                </div>
                                <p className="text-[11px] text-gray-500 dark:text-gray-400 leading-relaxed">
                                    {t("proxy.config.thinking_budget.override_suffixed_desc", {
                                        defaultValue:
                                            "默认关闭：仅裸模型（如 gemini-4-hydrogen）按思考强度选档，具名模型（如 -high）锁定后缀档位。开启后，当请求/映射给出显式强度时，将对多档位模型族重新分档，同时自动保护 gpt-oss-120b-medium、claude-opus-4-6-thinking 等单名模型。",
                                    })}
                                </p>
                            </div>
                            <input
                                type="checkbox"
                                data-testid="thinking-mode-override-suffixed"
                                disabled={!mapping.enabled}
                                className="toggle toggle-sm toggle-primary shrink-0"
                                checked={mapping.override_suffixed_models}
                                onChange={(e) => updateMapping({ override_suffixed_models: e.target.checked })}
                            />
                        </div>

                        {/* CLI / Headless 环境变量兼容提示 */}
                        <div className="text-[11px] text-gray-500 dark:text-gray-400 font-mono bg-white/60 dark:bg-base-100/40 px-2.5 py-1.5 rounded-lg border border-blue-100 dark:border-base-300/70">
                            {t("proxy.config.thinking_budget.mode_mapping_env_hint", {
                                defaultValue:
                                    "Headless/ENV: ANTIGRAVITY_THINKING_MODE_MAPPING=\"max=xhigh,low=medium\" · ANTIGRAVITY_THINKING_OVERRIDE_SUFFIXED=true",
                            })}
                        </div>
                    </div>
                );
            })()}

            {/* 底部专属保存操作栏 */}
            {onSave && (
                <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 pt-3.5 border-t border-gray-200 dark:border-base-200">
                    <span className="text-xs text-gray-500 dark:text-gray-400">
                        {t("proxy.config.thinking_budget.save_hint", {
                            defaultValue: "修改思考预算设置后点击保存即可立即生效，配置将即时热更新至当前运行的反代服务。",
                        })}
                    </span>
                    <button
                        type="button"
                        disabled={isSaving}
                        onClick={handleSave}
                        className={`px-4 py-2 rounded-xl text-xs font-semibold flex items-center gap-1.5 shadow-sm transition-all active:scale-95 shrink-0 cursor-pointer ${
                            isSaved
                                ? "bg-emerald-600 hover:bg-emerald-500 text-white"
                                : "bg-blue-600 hover:bg-blue-500 text-white"
                        } ${isSaving ? "opacity-75 cursor-wait" : ""}`}
                    >
                        {isSaved ? (
                            <>
                                <Check size={14} className="text-white" />
                                <span>{t("common.saved", { defaultValue: "已保存生效" })}</span>
                            </>
                        ) : (
                            <>
                                <Save size={14} className={isSaving ? "animate-spin" : ""} />
                                <span>{isSaving ? t("common.saving", { defaultValue: "保存中..." }) : t("proxy.config.thinking_budget.save_btn", { defaultValue: "保存思考设置并热生效" })}</span>
                            </>
                        )}
                    </button>
                </div>
            )}

            {/* 清空思考块二次确认弹窗 */}
            {showClearThinkingConfirm && (
                <div className="modal modal-open">
                    <div className="modal-box max-w-md bg-white dark:bg-base-100 border border-base-300 shadow-2xl p-5">
                        <div className="flex items-start gap-3">
                            <div className="p-2 rounded-full bg-error/10 text-error shrink-0 mt-0.5">
                                <AlertTriangle size={20} />
                            </div>
                            <div className="space-y-2 min-w-0">
                                <h3 className="text-sm font-bold text-gray-900 dark:text-white">
                                    {t("proxy.config.thinking_budget.clear_modal_title", {
                                        defaultValue: "确认清空思考块存储？",
                                    })}
                                </h3>
                                <p className="text-xs text-gray-600 dark:text-gray-300 leading-relaxed whitespace-pre-line">
                                    {t("proxy.config.thinking_budget.clear_modal_desc", {
                                        defaultValue:
                                            "此操作将彻底清空内存常驻轮次 (RAM) 与本地 SQLite 数据库中所有的历史思维链和工具签名记录。\n\n⚠️ 注意：此操作仅清理思考块数据，绝不删除任何反向代理请求日志。",
                                    })}
                                </p>
                                <div className="p-2.5 rounded-lg bg-amber-50 dark:bg-amber-950/30 border border-amber-200 dark:border-amber-800/50 text-[11px] text-amber-800 dark:text-amber-300 leading-relaxed">
                                    {t("proxy.config.thinking_budget.clear_modal_warning", {
                                        defaultValue:
                                            "建议仅在出现缓存命中异常、版本升级或开发者明确要求时执行。清空后新请求将重新建立干净的前缀索引。",
                                    })}
                                </div>
                            </div>
                        </div>

                        <div className="modal-action mt-5 flex justify-end gap-2">
                            <button
                                type="button"
                                className="btn btn-sm btn-ghost"
                                onClick={() => setShowClearThinkingConfirm(false)}
                                disabled={isClearingThinking}
                            >
                                {t("common.cancel", { defaultValue: "取消" })}
                            </button>
                            <button
                                type="button"
                                className="btn btn-sm btn-error text-white gap-1.5"
                                onClick={handleClearThinkingStore}
                                disabled={isClearingThinking}
                            >
                                {isClearingThinking ? (
                                    <span className="loading loading-spinner loading-xs" />
                                ) : (
                                    <Trash2 size={13} />
                                )}
                                {t("proxy.config.thinking_budget.clear_confirm_btn", {
                                    defaultValue: "确认清空",
                                })}
                            </button>
                        </div>
                    </div>
                    <div
                        className="modal-backdrop bg-black/40"
                        onClick={() => !isClearingThinking && setShowClearThinkingConfirm(false)}
                    />
                </div>
            )}
        </div>
    );
}
