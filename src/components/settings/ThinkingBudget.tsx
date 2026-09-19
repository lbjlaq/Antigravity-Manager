import { useState, useRef, useEffect } from "react";
import { useTranslation, Trans } from "react-i18next";
import { Save, Check, ChevronDown, Layers, HelpCircle, HardDrive, Trash2, AlertTriangle } from "lucide-react";
import { request } from "../../utils/request";
import { showToast } from "../common/ToastContainer";
import {
    ThinkingBudgetConfig,
    ThinkingControlSource,
    ThinkingBudgetMode,
} from "../../types/config";

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

const DEFAULT_CONFIG: ThinkingBudgetConfig = {
    control_source: "gateway",
    flash_mode: "custom",
    flash_low: 1024,
    flash_medium: 4096,
    flash_high: 16384,
    flash_tiered: -1,

    pro_mode: "custom",
    pro_low: 1001,
    pro_high: 10001,

    claude_mode: "custom",
    claude_budget: 16384,
    claude_low: 1024,
    claude_medium: 4096,
    claude_high: 16384,

    mode: "custom",
    custom_value: 24576,
    custom_low: 1024,
    custom_medium: 4096,
    custom_high: 16384,
    custom_tiered: -1,
};

type BudgetFieldKey =
    | "flash_low"
    | "flash_medium"
    | "flash_high"
    | "flash_tiered"
    | "pro_low"
    | "pro_high"
    | "claude_budget"
    | "claude_low"
    | "claude_medium"
    | "claude_high";

const BUDGET_DEFAULTS: Record<BudgetFieldKey, number> = {
    flash_low: 1024,
    flash_medium: 4096,
    flash_high: 16384,
    flash_tiered: -1,
    pro_low: 1001,
    pro_high: 10001,
    claude_budget: 16384,
    claude_low: 1024,
    claude_medium: 4096,
    claude_high: 16384,
};

export default function ThinkingBudget({
    config = DEFAULT_CONFIG,
    onChange,
    onSave,
    thinkingStoreEnabled = true,
    onThinkingStoreChange,
    thinkingMaxMemoryTurns = 600,
    onThinkingMaxMemoryTurnsChange,
    thinkingRetentionDays = 15,
    onThinkingRetentionDaysChange,
}: ThinkingBudgetProps) {
    const { t } = useTranslation();
    const [isSaving, setIsSaving] = useState(false);
    const [isSaved, setIsSaved] = useState(false);
    const [showClaudeAdvanced, setShowClaudeAdvanced] = useState(false);
    const [isClearingThinking, setIsClearingThinking] = useState(false);
    const [showClearThinkingConfirm, setShowClearThinkingConfirm] = useState(false);

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

    const currentConfig: ThinkingBudgetConfig = {
        ...DEFAULT_CONFIG,
        ...config,
    };

    // 预算输入框本地编辑文本状态，允许用户清空退格为 "" 或输入负号 "-"
    const [inputValues, setInputValues] = useState<Record<string, string>>(() => {
        const init: Record<string, string> = {};
        for (const [key, defaultVal] of Object.entries(BUDGET_DEFAULTS)) {
            const val = (config as any)?.[key];
            init[key] = val !== undefined && val !== null ? String(val) : String(defaultVal);
        }
        return init;
    });

    // 外部配置实质性更新时同步（保留正在编辑的空值状态）
    const lastConfigRef = useRef(config);
    useEffect(() => {
        if (config && config !== lastConfigRef.current) {
            lastConfigRef.current = config;
            setInputValues((prev) => {
                const next = { ...prev };
                for (const key of Object.keys(BUDGET_DEFAULTS)) {
                    const val = (config as any)?.[key];
                    if (val !== undefined && val !== null) {
                        const parsed = parseInt(prev[key], 10);
                        if (parsed !== val && prev[key] !== "" && prev[key] !== "-") {
                            next[key] = String(val);
                        }
                    }
                }
                return next;
            });
        }
    }, [config]);

    const handleControlSourceChange = (source: ThinkingControlSource) => {
        onChange({
            ...currentConfig,
            control_source: source,
        });
    };

    const handleFlashModeChange = (mode: ThinkingBudgetMode) => {
        onChange({
            ...currentConfig,
            flash_mode: mode,
        });
    };

    const handleProModeChange = (mode: ThinkingBudgetMode) => {
        onChange({
            ...currentConfig,
            pro_mode: mode,
        });
    };

    const handleClaudeModeChange = (mode: ThinkingBudgetMode) => {
        onChange({
            ...currentConfig,
            claude_mode: mode,
        });
    };

    // 输入框变更处理：允许清空为 ""，允许 "-"，不自动补 -1
    const handleInputChange = (field: BudgetFieldKey, rawVal: string) => {
        if (rawVal !== "" && rawVal !== "-" && !/^-?\d+$/.test(rawVal)) {
            return;
        }
        setInputValues((prev) => ({
            ...prev,
            [field]: rawVal,
        }));

        // 如果是合法完整数字，实时同步给配置对象；空值时不写入，留待最后保存阶段回填默认值
        const trimmed = rawVal.trim();
        if (trimmed !== "" && trimmed !== "-") {
            const parsed = parseInt(trimmed, 10);
            if (!isNaN(parsed)) {
                onChange({
                    ...currentConfig,
                    [field]: parsed,
                });
            }
        }
    };

    // 最后保存配置阶段：对处于清空/非法状态的预算输入框自动回填为默认值
    const handleSave = async () => {
        const nextInputs = { ...inputValues };
        const nextConfig: ThinkingBudgetConfig = { ...currentConfig };

        for (const [key, defaultVal] of Object.entries(BUDGET_DEFAULTS)) {
            const valStr = (nextInputs[key] ?? "").trim();
            let finalVal: number;
            if (valStr === "" || valStr === "-" || isNaN(parseInt(valStr, 10))) {
                finalVal = defaultVal;
                nextInputs[key] = String(defaultVal);
            } else {
                finalVal = parseInt(valStr, 10);
                nextInputs[key] = String(finalVal);
            }
            (nextConfig as any)[key] = finalVal;
        }

        // 回填到输入框界面显示
        setInputValues(nextInputs);
        // 同步给父组件配置
        onChange(nextConfig);

        if (onSave) {
            setIsSaving(true);
            try {
                await onSave();
                setIsSaved(true);
                setTimeout(() => setIsSaved(false), 2000);
            } finally {
                setIsSaving(false);
            }
        }
    };

    const getPresetTooltip = (val: number): string => {
        if (val === 32768) {
            return t("proxy.config.thinking_budget.tooltip_32768", {
                defaultValue: "32,768 Tokens：适合复杂编程与深度架构任务",
            });
        }
        if (val === 16384 || val === 16000) {
            return t("proxy.config.thinking_budget.tooltip_16384", {
                defaultValue: "1.6w 档位 (16,384 Tokens)：适合复杂 Agent 任务",
            });
        }
        if (val === 8192) {
            return t("proxy.config.thinking_budget.tooltip_8192", {
                defaultValue: "8,192 Tokens：适合中等复杂度多步推理与代码排错",
            });
        }
        if (val === 4096) {
            return t("proxy.config.thinking_budget.tooltip_4096", {
                defaultValue: "4,096 Tokens：标准平衡档，兼顾思考质量与响应速度",
            });
        }
        if (val === 2048) {
            return t("proxy.config.thinking_budget.tooltip_2048", {
                defaultValue: "2,048 Tokens：日常轻量推理档",
            });
        }
        if (val === 1024) {
            return t("proxy.config.thinking_budget.tooltip_1024", {
                defaultValue: "1,024 Tokens：轻量极速思考，消耗极低",
            });
        }
        if (val === 65536) {
            return t("proxy.config.thinking_budget.tooltip_65536", {
                defaultValue: "65,536 Tokens：超长深度思考链极限档",
            });
        }
        if (val === 1001) {
            return t("proxy.config.thinking_budget.tooltip_1001", {
                defaultValue: "1,001 Tokens：Google 官方 3.1 Pro 低思考基准档",
            });
        }
        if (val === 10001) {
            return t("proxy.config.thinking_budget.tooltip_10001", {
                defaultValue: "10,001 Tokens：Google 官方 3.1 Pro 深度推理主力档",
            });
        }
        if (val === -1) {
            return t("proxy.config.thinking_budget.tooltip_adaptive", {
                defaultValue: "自适应 (-1)：完全由上游模型根据问题难度自动分配",
            });
        }
        return `${val.toLocaleString()} Tokens`;
    };

    const renderPresetButtons = (
        field: BudgetFieldKey,
        presets: number[],
        color: "blue" | "purple" = "blue"
    ) => {
        const currentRaw = inputValues[field];
        const currentVal =
            currentRaw !== "" && currentRaw !== "-"
                ? parseInt(currentRaw, 10)
                : (currentConfig as any)[field];

        const activeBg =
            color === "purple"
                ? "bg-purple-600 hover:bg-purple-500"
                : "bg-blue-600 hover:bg-blue-500";
        const hoverText =
            color === "purple"
                ? "hover:text-purple-600 hover:border-purple-400 dark:hover:text-purple-300"
                : "hover:text-blue-600 hover:border-blue-400 dark:hover:text-blue-300";

        return (
            <div className="flex items-center gap-1 mt-1.5 flex-wrap">
                {presets.map((val) => (
                    <button
                        key={val}
                        type="button"
                        title={getPresetTooltip(val)}
                        onClick={() => {
                            setInputValues((prev) => ({ ...prev, [field]: String(val) }));
                            onChange({ ...currentConfig, [field]: val });
                        }}
                        className={`px-1.5 py-0.5 rounded text-[10px] font-mono font-semibold transition-all cursor-pointer ${
                            currentVal === val
                                ? `${activeBg} text-white shadow-xs`
                                : `bg-gray-100 dark:bg-base-300/80 border border-gray-200 dark:border-base-300 text-gray-700 dark:text-gray-300 ${hoverText}`
                        }`}
                    >
                        {val === -1
                            ? t("proxy.config.thinking_budget.preset_adaptive", {
                                  defaultValue: "自适应 (-1)",
                              })
                            : val.toLocaleString()}
                    </button>
                ))}
            </div>
        );
    };

    const controlSource = currentConfig.control_source || "gateway";

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
                                        value={thinkingMaxMemoryTurns}
                                        onChange={(e) => {
                                            const val = parseInt(e.target.value, 10);
                                            if (!isNaN(val)) {
                                                onThinkingMaxMemoryTurnsChange(Math.max(10, Math.min(10000, val)));
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
                                        value={thinkingRetentionDays}
                                        onChange={(e) => {
                                            const val = parseInt(e.target.value, 10);
                                            if (!isNaN(val)) {
                                                onThinkingRetentionDaysChange(Math.max(1, Math.min(365, val)));
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

            {/* 顶级大选择：控制权归属 */}
            <div className="space-y-2">
                <label className="text-xs font-semibold text-gray-800 dark:text-gray-200">
                    {t("proxy.config.thinking_budget.control_source_label", {
                        defaultValue: "控制权归属 (Top-Level Authority)",
                    })}
                </label>
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                    {/* 网关权威控制 */}
                    <button
                        type="button"
                        onClick={() => handleControlSourceChange("gateway")}
                        className={`text-left p-3.5 rounded-xl border transition-all cursor-pointer relative ${
                            controlSource === "gateway"
                                ? "border-blue-500 bg-blue-50/70 dark:bg-blue-900/20 text-gray-900 dark:text-white shadow-xs ring-1 ring-blue-500/40"
                                : "border-gray-200 dark:border-base-300 bg-white dark:bg-base-200 text-gray-700 dark:text-gray-300 hover:border-gray-300 dark:hover:border-base-300"
                        }`}
                    >
                        <div className="flex items-center justify-between mb-1.5">
                            <span className="text-xs font-bold flex items-center gap-1.5 text-gray-900 dark:text-white">
                                <span
                                    className={`w-2.5 h-2.5 rounded-full ${
                                        controlSource === "gateway"
                                            ? "bg-blue-500"
                                            : "bg-gray-300 dark:bg-base-300"
                                    }`}
                                />
                                {t("proxy.config.thinking_budget.source_gateway", {
                                    defaultValue: "网关权威控制",
                                })}
                            </span>
                            <span className="text-[10px] px-2 py-0.5 rounded-full font-bold bg-emerald-100 dark:bg-emerald-950/60 text-emerald-700 dark:text-emerald-300 border border-emerald-200 dark:border-emerald-800/60">
                                {t("proxy.config.thinking_budget.recommended_tag", {
                                    defaultValue: "首选 / 推荐",
                                })}
                            </span>
                        </div>
                        <p className="text-xs text-gray-500 dark:text-gray-400 leading-relaxed">
                            {t("proxy.config.thinking_budget.source_gateway_desc", {
                                defaultValue:
                                    "网关层统一掌控思维链预算与启发式档位，消除不同客户端参数差异，杜绝由于超额预算或格式不当引发的官方 400 报错。",
                            })}
                        </p>
                    </button>

                    {/* 客户端直接控制 */}
                    <button
                        type="button"
                        onClick={() => handleControlSourceChange("client")}
                        className={`text-left p-3.5 rounded-xl border transition-all cursor-pointer relative ${
                            controlSource === "client"
                                ? "border-amber-500 bg-amber-50/70 dark:bg-amber-900/20 text-gray-900 dark:text-white shadow-xs ring-1 ring-amber-500/40"
                                : "border-gray-200 dark:border-base-300 bg-white dark:bg-base-200 text-gray-700 dark:text-gray-300 hover:border-gray-300 dark:hover:border-base-300"
                        }`}
                    >
                        <div className="flex items-center justify-between mb-1.5">
                            <span className="text-xs font-bold flex items-center gap-1.5 text-gray-900 dark:text-white">
                                <span
                                    className={`w-2.5 h-2.5 rounded-full ${
                                        controlSource === "client"
                                            ? "bg-amber-500"
                                            : "bg-gray-300 dark:bg-base-300"
                                    }`}
                                />
                                {t("proxy.config.thinking_budget.source_client", {
                                    defaultValue: "客户端直接控制",
                                })}
                            </span>
                            <span className="text-[10px] px-2 py-0.5 rounded-full font-bold bg-rose-100 dark:bg-rose-950/60 text-rose-700 dark:text-rose-300 border border-rose-200 dark:border-rose-800/60">
                                {t("proxy.config.thinking_budget.danger_tag", {
                                    defaultValue: "危险 / 不推荐",
                                })}
                            </span>
                        </div>
                        <p className="text-xs text-gray-500 dark:text-gray-400 leading-relaxed">
                            {t("proxy.config.thinking_budget.source_client_desc", {
                                defaultValue:
                                    "归一化后直接提取客户端上送的预算并透传。若客户端未传则由上游自适应。不恰当的预算可能直接导致 Google 报错。",
                            })}
                        </p>
                    </button>
                </div>
            </div>

            {/* 客户端控制警示框 */}
            {controlSource === "client" && (
                <div className="p-3.5 bg-rose-50/80 dark:bg-rose-900/20 border border-rose-200 dark:border-rose-800/40 rounded-xl text-xs text-rose-800 dark:text-rose-200 flex items-start gap-2.5">
                    <svg
                        className="w-4 h-4 text-rose-500 shrink-0 mt-0.5"
                        fill="none"
                        stroke="currentColor"
                        viewBox="0 0 24 24"
                    >
                        <path
                            strokeLinecap="round"
                            strokeLinejoin="round"
                            strokeWidth="2"
                            d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"
                        />
                    </svg>
                    <div className="space-y-1 text-[11px] leading-relaxed">
                        <span className="font-semibold">
                            {t("proxy.config.thinking_budget.client_warning_title", {
                                defaultValue: "危险警告：已切换为客户端直接控制",
                            })}
                        </span>
                        <p>
                            {t("proxy.config.thinking_budget.client_warning_desc", {
                                defaultValue:
                                    "不同客户端插件（如 Cline, Roo, Cherry 等）上送的 thinking.budget_tokens 或 effort 各不相同。若客户端指定了超过 Google API 支持的数值（如 Flash 超过 24576、或 Pro 传入非标准数值），Google 将直接返回 400 Bad Request 错误中断生成。",
                            })}
                        </p>
                    </div>
                </div>
            )}

            {/* 网关权威控制下的三大模型分类配置 */}
            {controlSource === "gateway" && (
                <div className="space-y-4 pt-1">
                    {/* 1. Gemini Flash 系列 */}
                    <div className="border border-gray-200 dark:border-base-200 rounded-xl p-4 bg-white dark:bg-base-100 space-y-3.5 shadow-2xs">
                        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 border-b border-gray-100 dark:border-base-200 pb-2.5">
                            <div>
                                <span className="text-xs font-bold text-gray-900 dark:text-white">
                                    {t("proxy.config.thinking_budget.flash_family_title", {
                                        defaultValue: "Gemini Flash ≥ 3.0 系列 (如 3-flash, 3.7-flash, 3.8-flash, tiered 等)",
                                    })}
                                </span>
                                <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                                    {t("proxy.config.thinking_budget.flash_family_desc", {
                                        defaultValue: "控制 Gemini ≥ 3.0 的 Flash 思考模型四大档位预算（< 3.0 如 2.5-flash 为传统非思考模型，网关自动剥离思考）",
                                    })}
                                </p>
                            </div>
                            <div className="flex items-center gap-3">
                                <label className="inline-flex items-center gap-1.5 cursor-pointer text-xs font-medium text-gray-700 dark:text-gray-300">
                                    <input
                                        type="radio"
                                        name="flash_mode"
                                        checked={currentConfig.flash_mode === "default"}
                                        onChange={() => handleFlashModeChange("default")}
                                        className="text-blue-600 focus:ring-blue-500"
                                    />
                                    {t("proxy.config.thinking_budget.mode_default", {
                                        defaultValue: "默认模式 (官方自适应)",
                                    })}
                                </label>
                                <label className="inline-flex items-center gap-1.5 cursor-pointer text-xs font-medium text-gray-700 dark:text-gray-300">
                                    <input
                                        type="radio"
                                        name="flash_mode"
                                        checked={currentConfig.flash_mode !== "default"}
                                        onChange={() => handleFlashModeChange("custom")}
                                        className="text-blue-600 focus:ring-blue-500"
                                    />
                                    {t("proxy.config.thinking_budget.mode_custom", {
                                        defaultValue: "自定义思考预算模式 (推荐)",
                                    })}
                                </label>
                            </div>
                        </div>

                        {currentConfig.flash_mode !== "default" ? (
                            <div className="space-y-3">
                                <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3">
                                    <div>
                                        <label className="block text-xs font-semibold text-gray-700 dark:text-gray-300 mb-1">
                                            {t("proxy.config.thinking_budget.tier_low", {
                                                defaultValue: "Low 档位",
                                            })}
                                        </label>
                                        <input
                                            type="text"
                                            inputMode="numeric"
                                            placeholder="1024"
                                            value={inputValues.flash_low ?? ""}
                                            onChange={(e) =>
                                                handleInputChange("flash_low", e.target.value)
                                            }
                                            className="w-full px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-xs font-mono font-semibold text-gray-900 dark:text-white focus:ring-2 focus:ring-blue-500/30 focus:border-blue-500"
                                        />
                                        {renderPresetButtons("flash_low", [1024, 2048])}
                                        <p className="text-[11px] text-gray-500 dark:text-gray-400 mt-1">
                                            {t("proxy.config.thinking_budget.flash_low_hint", {
                                                defaultValue: "极低推理 (默认 1024)",
                                            })}
                                        </p>
                                    </div>
                                    <div>
                                        <label className="block text-xs font-semibold text-gray-700 dark:text-gray-300 mb-1">
                                            {t("proxy.config.thinking_budget.tier_medium", {
                                                defaultValue: "Medium 档位",
                                            })}
                                        </label>
                                        <input
                                            type="text"
                                            inputMode="numeric"
                                            placeholder="4096"
                                            value={inputValues.flash_medium ?? ""}
                                            onChange={(e) =>
                                                handleInputChange("flash_medium", e.target.value)
                                            }
                                            className="w-full px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-xs font-mono font-semibold text-gray-900 dark:text-white focus:ring-2 focus:ring-blue-500/30 focus:border-blue-500"
                                        />
                                        {renderPresetButtons("flash_medium", [4096, 8192])}
                                        <p className="text-[11px] text-gray-500 dark:text-gray-400 mt-1">
                                            {t("proxy.config.thinking_budget.flash_medium_hint", {
                                                defaultValue: "标准平衡档 (默认 4096)",
                                            })}
                                        </p>
                                    </div>
                                    <div>
                                        <label className="block text-xs font-semibold text-gray-700 dark:text-gray-300 mb-1">
                                            {t("proxy.config.thinking_budget.tier_high", {
                                                defaultValue: "High 档位",
                                            })}
                                        </label>
                                        <input
                                            type="text"
                                            inputMode="numeric"
                                            placeholder="16384"
                                            value={inputValues.flash_high ?? ""}
                                            onChange={(e) =>
                                                handleInputChange("flash_high", e.target.value)
                                            }
                                            className="w-full px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-xs font-mono font-semibold text-gray-900 dark:text-white focus:ring-2 focus:ring-blue-500/30 focus:border-blue-500"
                                        />
                                        {renderPresetButtons("flash_high", [16384, 32768])}
                                        <p className="text-[11px] text-gray-500 dark:text-gray-400 mt-1">
                                            {t("proxy.config.thinking_budget.flash_high_hint", {
                                                defaultValue: "深度推理档 (默认 16384)",
                                            })}
                                        </p>
                                    </div>
                                    <div>
                                        <label className="block text-xs font-semibold text-gray-700 dark:text-gray-300 mb-1">
                                            {t("proxy.config.thinking_budget.tier_tiered", {
                                                defaultValue: "Tiered (自由模型)",
                                            })}
                                        </label>
                                        <input
                                            type="text"
                                            inputMode="numeric"
                                            placeholder="-1"
                                            value={inputValues.flash_tiered ?? ""}
                                            onChange={(e) =>
                                                handleInputChange("flash_tiered", e.target.value)
                                            }
                                            className="w-full px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-xs font-mono font-semibold text-gray-900 dark:text-white focus:ring-2 focus:ring-blue-500/30 focus:border-blue-500"
                                        />
                                        {renderPresetButtons("flash_tiered", [-1, 2048, 8192, 32768])}
                                        <p className="text-[11px] text-gray-500 dark:text-gray-400 mt-1">
                                            {t("proxy.config.thinking_budget.flash_tiered_hint", {
                                                defaultValue: "自适应/自由强度 (默认 -1)",
                                            })}
                                        </p>
                                    </div>
                                </div>
                                <div className="p-3 bg-blue-50/70 dark:bg-blue-900/20 border border-blue-200/80 dark:border-blue-800/40 rounded-xl text-xs text-blue-800 dark:text-blue-200 leading-relaxed">
                                    {t("proxy.config.thinking_budget.tiered_desc", {
                                        defaultValue:
                                            "💡 Tiered 自由模型说明：该模型专为谷歌动态自适应打造。设置为 -1 时，模型根据问题难度自动分配思考量；若指定具体数值（如 30000+），可突破 High 档位解锁 Max 极限思考。",
                                    })}
                                </div>
                            </div>
                        ) : (
                            <p className="text-xs text-gray-500 dark:text-gray-400 italic">
                                {t("proxy.config.thinking_budget.flash_default_hint", {
                                    defaultValue:
                                        "已启用官方默认模式：完全透传官方模型 ID，不注入 thinkingBudget，完全由 Google 上游模型自主决定思考行为。",
                                })}
                            </p>
                        )}
                    </div>

                    {/* 2. Gemini Pro 系列 */}
                    <div className="border border-gray-200 dark:border-base-200 rounded-xl p-4 bg-white dark:bg-base-100 space-y-3.5 shadow-2xs">
                        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 border-b border-gray-100 dark:border-base-200 pb-2.5">
                            <div>
                                <span className="text-xs font-bold text-gray-900 dark:text-white">
                                    {t("proxy.config.thinking_budget.pro_family_title", {
                                        defaultValue: "Gemini Pro ≥ 3.0 系列 (如 3-pro, 3.1-pro 等)",
                                    })}
                                </span>
                                <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                                    {t("proxy.config.thinking_budget.pro_family_desc", {
                                        defaultValue:
                                            "Google 官方 Gemini ≥ 3.0 Pro 体系仅提供 Low 与 High 两个档位。未指定 effort 或 medium 将自动走 High 保证 Pro 深度思考。",
                                    })}
                                </p>
                            </div>
                            <div className="flex items-center gap-3">
                                <label className="inline-flex items-center gap-1.5 cursor-pointer text-xs font-medium text-gray-700 dark:text-gray-300">
                                    <input
                                        type="radio"
                                        name="pro_mode"
                                        checked={currentConfig.pro_mode === "default"}
                                        onChange={() => handleProModeChange("default")}
                                        className="text-blue-600 focus:ring-blue-500"
                                    />
                                    {t("proxy.config.thinking_budget.mode_default", {
                                        defaultValue: "默认模式 (官方自适应)",
                                    })}
                                </label>
                                <label className="inline-flex items-center gap-1.5 cursor-pointer text-xs font-medium text-gray-700 dark:text-gray-300">
                                    <input
                                        type="radio"
                                        name="pro_mode"
                                        checked={currentConfig.pro_mode !== "default"}
                                        onChange={() => handleProModeChange("custom")}
                                        className="text-blue-600 focus:ring-blue-500"
                                    />
                                    {t("proxy.config.thinking_budget.mode_custom", {
                                        defaultValue: "自定义思考预算模式 (推荐)",
                                    })}
                                </label>
                            </div>
                        </div>

                        {currentConfig.pro_mode !== "default" ? (
                            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                <div>
                                    <label className="block text-xs font-semibold text-gray-700 dark:text-gray-300 mb-1">
                                        {t("proxy.config.thinking_budget.tier_low", {
                                            defaultValue: "Low 档位",
                                        })}
                                    </label>
                                    <input
                                        type="text"
                                        inputMode="numeric"
                                        placeholder="1001"
                                        value={inputValues.pro_low ?? ""}
                                        onChange={(e) =>
                                            handleInputChange("pro_low", e.target.value)
                                        }
                                        className="w-full px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-xs font-mono font-semibold text-gray-900 dark:text-white focus:ring-2 focus:ring-blue-500/30 focus:border-blue-500"
                                    />
                                    {renderPresetButtons("pro_low", [1001, 1024, 2048, 4096])}
                                    <p className="text-[11px] text-gray-500 dark:text-gray-400 mt-1">
                                        {t("proxy.config.thinking_budget.pro_low_hint", {
                                            defaultValue: "低思考档位 (默认 1001，填 -1 则走官方自适应)",
                                        })}
                                    </p>
                                </div>
                                <div>
                                    <label className="block text-xs font-semibold text-gray-700 dark:text-gray-300 mb-1">
                                        {t("proxy.config.thinking_budget.tier_pro_high", {
                                            defaultValue: "High 档位 (默认/主力档位)",
                                        })}
                                    </label>
                                    <input
                                        type="text"
                                        inputMode="numeric"
                                        placeholder="10001"
                                        value={inputValues.pro_high ?? ""}
                                        onChange={(e) =>
                                            handleInputChange("pro_high", e.target.value)
                                        }
                                        className="w-full px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-xs font-mono font-semibold text-gray-900 dark:text-white focus:ring-2 focus:ring-blue-500/30 focus:border-blue-500"
                                    />
                                    {renderPresetButtons("pro_high", [10001, 16384, 32768])}
                                    <p className="text-[11px] text-gray-500 dark:text-gray-400 mt-1">
                                        {t("proxy.config.thinking_budget.pro_high_hint", {
                                            defaultValue: "深度思考档位 (默认 10001，填 -1 则走官方自适应)",
                                        })}
                                    </p>
                                </div>
                            </div>
                        ) : (
                            <p className="text-xs text-gray-500 dark:text-gray-400 italic">
                                {t("proxy.config.thinking_budget.pro_default_hint", {
                                    defaultValue:
                                        "已启用官方默认模式：Gemini Pro 系列将由 Google 官方动态确定思考强度。",
                                })}
                            </p>
                        )}
                    </div>

                    {/* 3. Claude 系列 */}
                    <div className="border border-gray-200 dark:border-base-200 rounded-xl p-4 bg-white dark:bg-base-100 space-y-3.5 shadow-2xs">
                        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 border-b border-gray-100 dark:border-base-200 pb-2.5">
                            <div>
                                <span className="text-xs font-bold text-gray-900 dark:text-white">
                                    {t("proxy.config.thinking_budget.claude_family_title", {
                                        defaultValue: "Claude 系列思考模型 (如 claude-3-7-sonnet-thinking 等)",
                                    })}
                                </span>
                                <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                                    {t("proxy.config.thinking_budget.claude_family_desc", {
                                        defaultValue:
                                            "调控 Claude 思考模型的 Token 预算（官方模型列表仅区分 -thinking 与普通模型，此项专用于思考模型）",
                                    })}
                                </p>
                            </div>
                            <div className="flex items-center gap-3">
                                <label className="inline-flex items-center gap-1.5 cursor-pointer text-xs font-medium text-gray-700 dark:text-gray-300">
                                    <input
                                        type="radio"
                                        name="claude_mode"
                                        checked={currentConfig.claude_mode === "default"}
                                        onChange={() => handleClaudeModeChange("default")}
                                        className="text-blue-600 focus:ring-blue-500"
                                    />
                                    {t("proxy.config.thinking_budget.mode_default", {
                                        defaultValue: "默认模式 (官方规范)",
                                    })}
                                </label>
                                <label className="inline-flex items-center gap-1.5 cursor-pointer text-xs font-medium text-gray-700 dark:text-gray-300">
                                    <input
                                        type="radio"
                                        name="claude_mode"
                                        checked={currentConfig.claude_mode !== "default"}
                                        onChange={() => handleClaudeModeChange("custom")}
                                        className="text-blue-600 focus:ring-blue-500"
                                    />
                                    {t("proxy.config.thinking_budget.mode_custom", {
                                        defaultValue: "自定义思考预算模式 (推荐)",
                                    })}
                                </label>
                            </div>
                        </div>

                        {currentConfig.claude_mode !== "default" ? (
                            <div className="space-y-3.5">
                                {/* 核心统一预算输入 */}
                                <div className="p-3.5 bg-purple-50/70 dark:bg-purple-900/20 border border-purple-200/80 dark:border-purple-800/40 rounded-xl space-y-2.5 shadow-2xs">
                                    <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2">
                                        <div>
                                            <label className="block text-xs font-bold text-gray-900 dark:text-white">
                                                {t("proxy.config.thinking_budget.claude_budget_title", {
                                                    defaultValue: "Claude 思考模型预算 (Tokens)",
                                                })}
                                            </label>
                                            <p className="text-xs text-gray-600 dark:text-gray-300 mt-0.5">
                                                {t("proxy.config.thinking_budget.claude_budget_desc", {
                                                    defaultValue: "客户端调用思考模型（如 claude-3-7-sonnet-thinking）时的核心 Token 预算（填 -1 则由官方自适应）",
                                                })}
                                            </p>
                                        </div>
                                        <div className="flex items-center gap-1.5 flex-wrap">
                                            {[1024, 2048, 4096, 8192, 16384, 32768, 65536, -1].map((val) => {
                                                const currentBudgetNum = inputValues.claude_budget !== "" && inputValues.claude_budget !== "-"
                                                    ? parseInt(inputValues.claude_budget, 10)
                                                    : currentConfig.claude_budget;
                                                return (
                                                    <button
                                                        key={val}
                                                        type="button"
                                                        title={getPresetTooltip(val)}
                                                        onClick={() => {
                                                            setInputValues((prev) => ({ ...prev, claude_budget: String(val) }));
                                                            onChange({ ...currentConfig, claude_budget: val });
                                                        }}
                                                        className={`px-2.5 py-1 rounded-lg text-xs font-mono font-semibold transition-all cursor-pointer ${
                                                            currentBudgetNum === val
                                                                ? "bg-purple-600 hover:bg-purple-500 text-white shadow-xs"
                                                                : "bg-white dark:bg-base-200 border border-gray-300 dark:border-base-300 text-gray-700 dark:text-gray-200 hover:border-purple-400 hover:text-purple-600 dark:hover:text-purple-300"
                                                        }`}
                                                    >
                                                        {val === -1 ? t("proxy.config.thinking_budget.preset_adaptive", { defaultValue: "自适应 (-1)" }) : `${val.toLocaleString()}`}
                                                    </button>
                                                );
                                            })}
                                        </div>
                                    </div>

                                    <div className="flex items-center gap-3">
                                        <input
                                            type="text"
                                            inputMode="numeric"
                                            placeholder="16384"
                                            value={inputValues.claude_budget ?? ""}
                                            onChange={(e) =>
                                                handleInputChange("claude_budget", e.target.value)
                                            }
                                            className="w-48 px-3 py-1.5 border border-purple-300 dark:border-purple-800/80 rounded-lg bg-white dark:bg-base-200 text-xs font-mono font-bold text-purple-700 dark:text-purple-300 focus:ring-2 focus:ring-purple-500/30 focus:border-purple-500 shadow-2xs"
                                        />
                                        <span className="text-xs font-medium text-gray-600 dark:text-gray-300">
                                            {t("proxy.config.thinking_budget.claude_budget_current", {
                                                defaultValue: "推荐默认值: 16,384 Tokens (兼顾思考深度与响应速率)",
                                            })}
                                        </span>
                                    </div>
                                </div>

                                {/* 高级：客户端 Effort 档位映射 */}
                                <div>
                                    <button
                                        type="button"
                                        onClick={() => setShowClaudeAdvanced((prev) => !prev)}
                                        className="inline-flex items-center gap-1.5 text-xs font-semibold text-purple-600 dark:text-purple-400 hover:underline cursor-pointer select-none"
                                    >
                                        <ChevronDown size={14} className={`transition-transform duration-200 ${showClaudeAdvanced ? "" : "-rotate-90"}`} />
                                        <span>
                                            {showClaudeAdvanced
                                                ? t("proxy.config.thinking_budget.claude_hide_advanced", { defaultValue: "收起客户端 Effort 档位映射" })
                                                : t("proxy.config.thinking_budget.claude_show_advanced", { defaultValue: "展开高级设置：客户端 Effort 档位映射 (low / medium / high)" })}
                                        </span>
                                    </button>

                                    {showClaudeAdvanced && (
                                        <div className="mt-2.5 p-3.5 bg-gray-50 dark:bg-base-200 border border-gray-200 dark:border-base-300 rounded-xl space-y-2.5 shadow-2xs">
                                            <p className="text-xs text-gray-600 dark:text-gray-300 leading-relaxed">
                                                {t("proxy.config.thinking_budget.claude_effort_mapping_desc", {
                                                    defaultValue: "当部分商业客户端（如 Cline, Roo 等）在请求头或参数中附带 reasoning_effort 时，按以下预算映射：",
                                                })}
                                            </p>
                                            <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                                                <div>
                                                    <label className="block text-xs font-semibold text-gray-700 dark:text-gray-200 mb-1">
                                                        {t("proxy.config.thinking_budget.tier_low", { defaultValue: "Low 档位" })}
                                                    </label>
                                                    <input
                                                        type="text"
                                                        inputMode="numeric"
                                                        placeholder="1024"
                                                        value={inputValues.claude_low ?? ""}
                                                        onChange={(e) => handleInputChange("claude_low", e.target.value)}
                                                        className="w-full px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-xs font-mono font-semibold text-gray-900 dark:text-white focus:ring-2 focus:ring-purple-500/30 focus:border-purple-500"
                                                    />
                                                    {renderPresetButtons("claude_low", [1024, 2048], "purple")}
                                                    <p className="text-[11px] text-gray-500 dark:text-gray-400 mt-1">
                                                        {t("proxy.config.thinking_budget.claude_low_hint", { defaultValue: "轻量思考 (默认 1024)" })}
                                                    </p>
                                                </div>
                                                <div>
                                                    <label className="block text-xs font-semibold text-gray-700 dark:text-gray-200 mb-1">
                                                        {t("proxy.config.thinking_budget.tier_medium", { defaultValue: "Medium 档位" })}
                                                    </label>
                                                    <input
                                                        type="text"
                                                        inputMode="numeric"
                                                        placeholder="4096"
                                                        value={inputValues.claude_medium ?? ""}
                                                        onChange={(e) => handleInputChange("claude_medium", e.target.value)}
                                                        className="w-full px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-xs font-mono font-semibold text-gray-900 dark:text-white focus:ring-2 focus:ring-purple-500/30 focus:border-purple-500"
                                                    />
                                                    {renderPresetButtons("claude_medium", [4096, 8192], "purple")}
                                                    <p className="text-[11px] text-gray-500 dark:text-gray-400 mt-1">
                                                        {t("proxy.config.thinking_budget.claude_medium_hint", { defaultValue: "日常平衡档 (默认 4096)" })}
                                                    </p>
                                                </div>
                                                <div>
                                                    <label className="block text-xs font-semibold text-gray-700 dark:text-gray-200 mb-1">
                                                        {t("proxy.config.thinking_budget.tier_high", { defaultValue: "High 档位" })}
                                                    </label>
                                                    <input
                                                        type="text"
                                                        inputMode="numeric"
                                                        placeholder="16384"
                                                        value={inputValues.claude_high ?? ""}
                                                        onChange={(e) => handleInputChange("claude_high", e.target.value)}
                                                        className="w-full px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-xs font-mono font-semibold text-gray-900 dark:text-white focus:ring-2 focus:ring-purple-500/30 focus:border-purple-500"
                                                    />
                                                    {renderPresetButtons("claude_high", [16384, 32768], "purple")}
                                                    <p className="text-[11px] text-gray-500 dark:text-gray-400 mt-1">
                                                        {t("proxy.config.thinking_budget.claude_high_hint", { defaultValue: "深度思考档 (默认 16384)" })}
                                                    </p>
                                                </div>
                                            </div>
                                        </div>
                                    )}
                                </div>

                                {/* 全宽背景说明条 */}
                                <div className="w-full p-3 bg-purple-50/70 dark:bg-purple-900/20 border border-purple-200/80 dark:border-purple-800/40 rounded-xl text-xs text-purple-800 dark:text-purple-200 leading-relaxed">
                                    {t("proxy.config.thinking_budget.claude_thinking_note", {
                                        defaultValue:
                                            "💡 Claude 思考模型说明：Claude 系列在模型列表中仅提供思考版模型（如 claude-3-7-sonnet-thinking）与普通版，未拆分 low/medium/high 后缀。此处设置的值将作为调用 Claude 思考模型时的统一深度思考预算。",
                                    })}
                                </div>
                            </div>
                        ) : (
                            <p className="text-xs text-gray-500 dark:text-gray-400 italic">
                                {t("proxy.config.thinking_budget.claude_default_hint", {
                                    defaultValue:
                                        "已启用官方默认模式：完全按官方协议规范透传，不注入额外自定义预算限制。",
                                })}
                            </p>
                        )}
                    </div>
                </div>
            )}

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
