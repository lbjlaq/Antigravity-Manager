import { useTranslation } from "react-i18next";
import { GlobalSystemPromptConfig } from "../../types/config";

interface GlobalSystemPromptProps {
    config: GlobalSystemPromptConfig;
    onChange: (config: GlobalSystemPromptConfig) => void;
}

const DEFAULT_CONFIG: GlobalSystemPromptConfig = {
    enabled: false,
    content: '',
    include_default_prompt: true,
};

export default function GlobalSystemPrompt({
    config = DEFAULT_CONFIG,
    onChange,
}: GlobalSystemPromptProps) {
    const { t } = useTranslation();
    const effectiveConfig = { ...DEFAULT_CONFIG, ...config };

    return (
        <div className="space-y-3">
            {/* 标题区域 (Compact) */}
            <div className="flex items-center justify-between gap-3 bg-purple-50/30 dark:bg-purple-900/5 border border-purple-100/50 dark:border-purple-800/20 rounded-lg px-4 py-3">
                <div className="space-y-0.5">
                    <h4 className="font-bold text-sm text-gray-900 dark:text-gray-100">
                        {t("settings.global_system_prompt.title", { defaultValue: "全局系统提示词 (Global System Prompt)" })}
                    </h4>
                    <p className="text-[10px] text-gray-500 dark:text-gray-400">
                        {t("settings.global_system_prompt.hint", { defaultValue: "自动注入所有请求的 systemInstruction" })}
                    </p>
                </div>

                <div className="flex items-center gap-3">
                    <span className={`text-[10px] font-medium ${effectiveConfig.enabled ? 'text-purple-600 dark:text-purple-400' : 'text-gray-400'}`}>
                        {effectiveConfig.enabled ? t("common.enabled", { defaultValue: "已启用" }) : t("common.disabled", { defaultValue: "已禁用" })}
                    </span>
                    <label className="relative inline-flex items-center cursor-pointer shrink-0">
                        <input
                            type="checkbox"
                            checked={effectiveConfig.enabled}
                            onChange={(e) => onChange({ ...effectiveConfig, enabled: e.target.checked })}
                            className="sr-only peer"
                        />
                        <div className="w-9 h-5 bg-gray-200 peer-focus:outline-none rounded-full peer dark:bg-gray-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all dark:after:border-gray-600 peer-checked:bg-purple-600"></div>
                    </label>
                </div>
            </div>

            <div className="flex items-center justify-between gap-3 bg-white dark:bg-base-100 border border-gray-200 dark:border-gray-700 rounded-lg px-4 py-3">
                <div className="space-y-0.5">
                    <h5 className="font-semibold text-xs text-gray-800 dark:text-gray-200">
                        {t("settings.global_system_prompt.default_prompt_label", { defaultValue: "内置 Antigravity 身份提示词" })}
                    </h5>
                    <p className="text-[10px] text-gray-500 dark:text-gray-400">
                        {t("settings.global_system_prompt.default_prompt_hint", { defaultValue: "控制是否自动注入默认编码助手身份" })}
                    </p>
                </div>
                <label className="relative inline-flex items-center cursor-pointer shrink-0">
                    <input
                        type="checkbox"
                        checked={effectiveConfig.include_default_prompt}
                        onChange={(e) => onChange({ ...effectiveConfig, include_default_prompt: e.target.checked })}
                        className="sr-only peer"
                    />
                    <div className="w-9 h-5 bg-gray-200 peer-focus:outline-none rounded-full peer dark:bg-gray-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all dark:after:border-gray-600 peer-checked:bg-purple-600"></div>
                </label>
            </div>

            {/* 编辑区域 (仅在启用时显示) */}
            {effectiveConfig.enabled && (
                <div className="space-y-3">
                    <textarea
                        value={effectiveConfig.content}
                        onChange={(e) => onChange({ ...effectiveConfig, content: e.target.value })}
                        placeholder={t("settings.global_system_prompt.placeholder", {
                            defaultValue: "输入全局系统提示词...\n例如：你是一位资深的全栈开发工程师，擅长 React 和 Rust。请使用简体中文回复。",
                        })}
                        rows={6}
                        className="w-full bg-white dark:bg-base-100 border border-gray-200 dark:border-gray-700 rounded-lg px-4 py-3 text-sm focus:ring-2 focus:ring-purple-500/20 outline-none transition-all resize-y min-h-[120px]"
                    />
                    <div className="flex items-center justify-between">
                        <p className="text-xs text-gray-400 dark:text-gray-500">
                            {t("settings.global_system_prompt.char_count", {
                                defaultValue: "{{count}} 字符",
                                count: effectiveConfig.content.length,
                            })}
                        </p>
                    </div>
                    {effectiveConfig.content.length > 2000 && (
                        <div className="bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700/30 rounded-lg p-3">
                            <p className="text-xs text-amber-700 dark:text-amber-400">
                                {t("settings.global_system_prompt.long_prompt_warning", {
                                    defaultValue: "提示词较长（超过 2000 字符），可能会占用较多的上下文窗口空间，影响模型可用的对话长度。",
                                })}
                            </p>
                        </div>
                    )}
                </div>
            )}
        </div>
    );
}
