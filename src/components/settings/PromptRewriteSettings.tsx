import { useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { Maximize2, Plus, Sparkles, Trash2, X } from "lucide-react";
import {
    PromptRewriteConfig,
    PromptRewriteRule,
    RECOMMENDED_PROMPT_REWRITE_PRESETS,
} from "../../types/config";

interface PromptRewriteSettingsProps {
    config: PromptRewriteConfig;
    onChange: (config: PromptRewriteConfig) => void;
}

const DEFAULT_CONFIG: PromptRewriteConfig = {
    enabled: true,
    normalize_identity: true,
    rules: RECOMMENDED_PROMPT_REWRITE_PRESETS,
};

function createRule(): PromptRewriteRule {
    const id =
        typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
            ? crypto.randomUUID()
            : `rule-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    return { id, pattern: "", replacement: "", enabled: true };
}

export default function PromptRewriteSettings({
    config = DEFAULT_CONFIG,
    onChange,
}: PromptRewriteSettingsProps) {
    const { t } = useTranslation();
    const [editing, setEditing] = useState<PromptRewriteRule | null>(null);
    const rules = config.rules || [];
    const normalizeIdentity = config.normalize_identity !== false;

    const getRuleIssue = (rule: PromptRewriteRule): string | null => {
        const pattern = rule.pattern;
        if (!pattern) {
            return rule.replacement
                ? t("proxy.prompt_rewrite.rule_empty_pattern", {
                      defaultValue: "匹配内容不能为空",
                  })
                : null;
        }
        if (Array.from(pattern).length > 8192) {
            return t("proxy.prompt_rewrite.rule_pattern_too_long", {
                defaultValue: "匹配内容超过 8192 字符，后端会跳过该规则",
            });
        }
        if (Array.from(rule.replacement).length > 16384) {
            return t("proxy.prompt_rewrite.rule_replacement_too_long", {
                defaultValue: "替换内容超过 16384 字符，后端会跳过该规则",
            });
        }
        if (pattern.replace(/\*/g, "").trim() === "") {
            return t("proxy.prompt_rewrite.rule_empty_match", {
                defaultValue:
                    "该规则会匹配空文本，后端会跳过；请补充固定前缀，例如 You are *",
            });
        }
        return null;
    };

    const updateRule = (index: number, patch: Partial<PromptRewriteRule>) => {
        onChange({
            ...config,
            rules: rules.map((rule, i) => (i === index ? { ...rule, ...patch } : rule)),
        });
    };

    const addRule = () => onChange({ ...config, rules: [...rules, createRule()] });

    const removeRule = (index: number) => {
        onChange({ ...config, rules: rules.filter((_, i) => i !== index) });
    };

    const loadRecommendedPresets = () => {
        if (rules.length > 0) {
            const confirmed = window.confirm(
                t("proxy.prompt_rewrite.load_presets_confirm", {
                    defaultValue: "这将在保留现有自定义规则的同时，自动补齐官方推荐的风控预设规则，是否继续？",
                })
            );
            if (!confirmed) return;
        }
        const existingPatterns = new Set(rules.map((r) => r.pattern.trim()));
        const toAdd = RECOMMENDED_PROMPT_REWRITE_PRESETS.filter(
            (p) => !existingPatterns.has(p.pattern.trim())
        );
        onChange({
            ...config,
            rules: [...rules, ...toAdd],
        });
    };

    const applyEditing = () => {
        if (!editing) return;
        const index = rules.findIndex((rule) => rule.id === editing.id);
        if (index >= 0) {
            updateRule(index, {
                pattern: editing.pattern,
                replacement: editing.replacement,
            });
        }
        setEditing(null);
    };

    return (
        <div className="space-y-3">
            {/* 标题与总开关 */}
            <div className="flex items-center justify-between gap-3 bg-amber-50/40 dark:bg-amber-900/5 border border-amber-100/60 dark:border-amber-800/20 rounded-lg px-4 py-3">
                <div className="space-y-0.5">
                    <h4 className="font-bold text-sm text-gray-900 dark:text-gray-100">
                        {t("proxy.prompt_rewrite.title", { defaultValue: "风控提示词清理方案" })}
                    </h4>
                    <p className="text-[10px] text-gray-500 dark:text-gray-400">
                        {t("proxy.prompt_rewrite.subtitle", {
                            defaultValue:
                                "对首个真实 user 提问之前的所有提示词块执行文本替换；不影响用户正文、模型输出与工具声明。",
                        })}
                    </p>
                </div>

                <div className="flex items-center gap-3">
                    <span
                        className={`text-[10px] font-medium ${
                            config.enabled
                                ? "text-amber-600 dark:text-amber-400"
                                : "text-gray-400"
                        }`}
                    >
                        {config.enabled
                            ? t("common.enabled", { defaultValue: "已启用" })
                            : t("common.disabled", { defaultValue: "已禁用" })}
                    </span>
                    <label className="relative inline-flex items-center cursor-pointer shrink-0">
                        <input
                            type="checkbox"
                            checked={config.enabled}
                            onChange={(e) => onChange({ ...config, enabled: e.target.checked })}
                            className="sr-only peer"
                        />
                        <div className="w-9 h-5 bg-gray-200 peer-focus:outline-none rounded-full peer dark:bg-gray-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all dark:after:border-gray-600 peer-checked:bg-amber-500"></div>
                    </label>
                </div>
            </div>

            {/* 内置身份归一化开关（独立于自定义规则开关） */}
            <div className="flex items-center justify-between gap-3 rounded-lg border border-gray-200/80 dark:border-base-300 bg-white dark:bg-base-100 px-4 py-2.5">
                <div className="space-y-0.5">
                    <p className="text-xs font-semibold text-gray-700 dark:text-gray-200">
                        {t("proxy.prompt_rewrite.normalize_identity", {
                            defaultValue: "内置身份归一化",
                        })}
                    </p>
                    <p className="text-[10px] text-gray-500 dark:text-gray-400">
                        {t("proxy.prompt_rewrite.normalize_identity_hint", {
                            defaultValue:
                                "关闭后保留客户端原始身份句，仅执行上面的自定义规则；用于排障或客户端自带合规身份的场景。",
                        })}
                    </p>
                </div>
                <label className="relative inline-flex items-center cursor-pointer shrink-0">
                    <input
                        type="checkbox"
                        checked={normalizeIdentity}
                        onChange={(e) =>
                            onChange({ ...config, normalize_identity: e.target.checked })
                        }
                        className="sr-only peer"
                    />
                    <div className="w-9 h-5 bg-gray-200 peer-focus:outline-none rounded-full peer dark:bg-gray-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all dark:after:border-gray-600 peer-checked:bg-blue-500"></div>
                </label>
            </div>

            {config.enabled && (
                <div className="space-y-2">
                    {rules.length === 0 && (
                        <div className="rounded-lg border border-dashed border-gray-300 dark:border-base-300 p-4 text-center text-xs text-gray-400 dark:text-gray-500">
                            {t("proxy.prompt_rewrite.empty", {
                                defaultValue: "还没有清理规则，点击下方新增。",
                            })}
                        </div>
                    )}

                    {rules.map((rule, index) => {
                        const issue = getRuleIssue(rule);
                        return (
                        <div
                            key={rule.id || index}
                            className="rounded-lg border border-gray-200/80 dark:border-base-300 bg-white dark:bg-base-100 p-2.5"
                        >
                            <div className="flex items-center gap-2">
                                <span className="w-5 shrink-0 text-right text-[10px] font-mono text-gray-400">
                                    {index + 1}.
                                </span>
                                <input
                                    value={rule.pattern}
                                    onChange={(e) =>
                                        updateRule(index, { pattern: e.target.value })
                                    }
                                    placeholder={t("proxy.prompt_rewrite.match_placeholder", {
                                        defaultValue: "匹配内容，例如：You are *",
                                    })}
                                    className="flex-1 min-w-0 px-2.5 py-1.5 border border-gray-300 dark:border-base-300 rounded-md bg-white dark:bg-base-200 font-mono text-xs text-gray-900 dark:text-base-content focus:ring-2 focus:ring-amber-500/20 focus:border-amber-500 outline-none"
                                />
                                <span className="shrink-0 text-[10px] font-bold uppercase tracking-wide text-gray-400">
                                    to
                                </span>
                                <input
                                    value={rule.replacement}
                                    onChange={(e) =>
                                        updateRule(index, { replacement: e.target.value })
                                    }
                                    placeholder={t("proxy.prompt_rewrite.replace_placeholder", {
                                        defaultValue: "留空 = 删除匹配内容",
                                    })}
                                    className="flex-1 min-w-0 px-2.5 py-1.5 border border-gray-300 dark:border-base-300 rounded-md bg-white dark:bg-base-200 font-mono text-xs text-gray-900 dark:text-base-content focus:ring-2 focus:ring-amber-500/20 focus:border-amber-500 outline-none"
                                />
                                <button
                                    type="button"
                                    onClick={() => setEditing({ ...rule })}
                                    title={t("proxy.prompt_rewrite.expand", {
                                        defaultValue: "放大编辑",
                                    })}
                                    className="shrink-0 p-1.5 rounded-md text-gray-400 hover:text-amber-600 hover:bg-amber-50 dark:hover:bg-amber-900/20 transition-colors"
                                >
                                    <Maximize2 size={14} />
                                </button>
                                <button
                                    type="button"
                                    onClick={() => removeRule(index)}
                                    title={t("proxy.prompt_rewrite.delete", {
                                        defaultValue: "删除",
                                    })}
                                    className="shrink-0 p-1.5 rounded-md text-gray-400 hover:text-rose-600 hover:bg-rose-50 dark:hover:bg-rose-900/20 transition-colors"
                                >
                                    <Trash2 size={14} />
                                </button>
                            </div>
                            <p className="mt-1.5 pl-7 text-[10px] text-gray-400 dark:text-gray-500 truncate">
                                {rule.replacement.length === 0
                                    ? t("proxy.prompt_rewrite.hint_empty", {
                                          defaultValue: "替换为留空 = 删除匹配内容",
                                      })
                                    : `→ ${rule.replacement}`}
                            </p>
                            {issue && (
                                <p className="mt-1 pl-7 text-[10px] text-rose-500 dark:text-rose-400">
                                    {issue}
                                </p>
                            )}
                        </div>
                        );
                    })}

                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                        <button
                            type="button"
                            onClick={addRule}
                            className="py-2 rounded-lg border border-dashed border-gray-300 dark:border-base-300 text-xs font-semibold text-gray-500 dark:text-gray-400 hover:border-amber-400 hover:text-amber-600 dark:hover:text-amber-400 hover:bg-amber-50/40 dark:hover:bg-amber-900/10 transition-colors flex items-center justify-center gap-1.5"
                        >
                            <Plus size={14} />
                            {t("proxy.prompt_rewrite.add_rule", {
                                defaultValue: "新增清理规则",
                            })}
                        </button>
                        <button
                            type="button"
                            onClick={loadRecommendedPresets}
                            className="py-2 rounded-lg border border-dashed border-amber-300 dark:border-amber-700/50 text-xs font-semibold text-amber-600 dark:text-amber-400 hover:border-amber-500 hover:bg-amber-50/60 dark:hover:bg-amber-900/20 transition-colors flex items-center justify-center gap-1.5"
                        >
                            <Sparkles size={14} />
                            {t("proxy.prompt_rewrite.load_presets", {
                                defaultValue: "载入官方推荐预设",
                            })}
                        </button>
                    </div>

                    <div className="rounded-lg bg-gray-50/80 dark:bg-base-200/50 border border-gray-100 dark:border-base-300 p-3 space-y-1">
                        <p className="text-[10px] text-gray-500 dark:text-gray-400 whitespace-pre-wrap">
                            {t("proxy.prompt_rewrite.hint_wildcard", {
                                defaultValue:
                                    "支持通配符 *（匹配同一行内任意字符）；跨行请写 \\n。",
                            })}
                        </p>
                        <p className="text-[10px] text-gray-500 dark:text-gray-400 whitespace-pre-wrap">
                            {t("proxy.prompt_rewrite.hint_escape", {
                                defaultValue:
                                    "支持转义：\\n 换行、\\t 制表符、\\\\ 反斜杠、\\* 字面星号。",
                            })}
                        </p>
                        <p className="text-[10px] text-gray-500 dark:text-gray-400">
                            {t("proxy.prompt_rewrite.hint_order", {
                                defaultValue:
                                    "规则按从上到下顺序执行；只作用于首个真实 user 提问前的提示词块，代码块自动豁免。",
                            })}
                        </p>
                    </div>
                </div>
            )}

            {/* 大编辑框：适合粘贴长段提示词 */}
            {editing &&
                createPortal(
                    <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/50 backdrop-blur-sm p-4">
                        <div className="bg-white dark:bg-base-100 rounded-2xl shadow-xl w-full max-w-4xl max-h-[90vh] flex flex-col border border-gray-200 dark:border-base-300">
                            <div className="flex items-center justify-between px-5 py-3 border-b border-gray-100 dark:border-base-300">
                                <h3 className="text-sm font-bold text-gray-900 dark:text-gray-100">
                                    {t("proxy.prompt_rewrite.expand", {
                                        defaultValue: "放大编辑",
                                    })}
                                </h3>
                                <button
                                    type="button"
                                    onClick={() => setEditing(null)}
                                    className="p-1 rounded-md text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-base-200 transition-colors"
                                >
                                    <X size={16} />
                                </button>
                            </div>
                            <div className="p-5 space-y-4 overflow-y-auto">
                                <div>
                                    <label className="block text-xs font-semibold text-gray-700 dark:text-gray-200 mb-1.5">
                                        {t("proxy.prompt_rewrite.match", {
                                            defaultValue: "匹配内容",
                                        })}
                                    </label>
                                    <textarea
                                        value={editing!.pattern}
                                        onChange={(e) =>
                                            setEditing({ ...editing!, pattern: e.target.value })
                                        }
                                        rows={8}
                                        placeholder={t(
                                            "proxy.prompt_rewrite.match_placeholder",
                                            {
                                                defaultValue: "匹配内容，例如：You are *",
                                            },
                                        )}
                                        className="w-full px-3 py-2 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 font-mono text-xs text-gray-900 dark:text-base-content focus:ring-2 focus:ring-amber-500/20 focus:border-amber-500 outline-none resize-y min-h-[140px]"
                                    />
                                </div>
                                <div>
                                    <label className="block text-xs font-semibold text-gray-700 dark:text-gray-200 mb-1.5">
                                        {t("proxy.prompt_rewrite.replace", {
                                            defaultValue: "替换为",
                                        })}
                                    </label>
                                    <textarea
                                        value={editing!.replacement}
                                        onChange={(e) =>
                                            setEditing({
                                                ...editing!,
                                                replacement: e.target.value,
                                            })
                                        }
                                        rows={6}
                                        placeholder={t(
                                            "proxy.prompt_rewrite.replace_placeholder",
                                            {
                                                defaultValue: "留空 = 删除匹配内容",
                                            },
                                        )}
                                        className="w-full px-3 py-2 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 font-mono text-xs text-gray-900 dark:text-base-content focus:ring-2 focus:ring-amber-500/20 focus:border-amber-500 outline-none resize-y min-h-[110px]"
                                    />
                                    <p className="mt-1 text-[10px] text-gray-400 dark:text-gray-500">
                                        {t("proxy.prompt_rewrite.hint_empty", {
                                            defaultValue: "替换为留空 = 删除匹配内容",
                                        })}
                                    </p>
                                </div>
                            </div>
                            <div className="flex justify-end gap-2 px-5 py-3 border-t border-gray-100 dark:border-base-300">
                                <button
                                    type="button"
                                    onClick={() => setEditing(null)}
                                    className="px-3 py-1.5 rounded-lg text-xs font-semibold bg-gray-100 dark:bg-base-200 text-gray-700 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-base-300 transition-colors"
                                >
                                    {t("common.cancel", { defaultValue: "取消" })}
                                </button>
                                <button
                                    type="button"
                                    onClick={applyEditing}
                                    className="px-3 py-1.5 rounded-lg text-xs font-semibold bg-amber-500 hover:bg-amber-600 text-white shadow-xs transition-colors"
                                >
                                    {t("proxy.prompt_rewrite.done", {
                                        defaultValue: "完成",
                                    })}
                                </button>
                            </div>
                        </div>
                    </div>,
                    document.body
                )}
        </div>
    );
}
