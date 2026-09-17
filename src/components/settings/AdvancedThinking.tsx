import { useTranslation } from "react-i18next";
import { BrainCircuit } from "lucide-react";
import { ProxyConfig } from "../../types/config";
import ThinkingBudget from "./ThinkingBudget";
import GlobalSystemPrompt from "./GlobalSystemPrompt";
import ImageThinkingMode from "./ImageThinkingMode";

interface AdvancedThinkingProps {
    config: ProxyConfig;
    onChange: (config: ProxyConfig) => void;
}

export default function AdvancedThinking({
    config,
    onChange,
}: AdvancedThinkingProps) {
    const { t } = useTranslation();

    return (
        <div className="space-y-4">
            <div className="bg-white dark:bg-base-100 rounded-xl p-4 border border-gray-100 dark:border-base-200 shadow-sm">
                <div className="flex items-center gap-3 mb-4">
                    <div className="p-1.5 bg-indigo-50 dark:bg-indigo-900/20 rounded text-indigo-600 dark:text-indigo-400">
                        <BrainCircuit size={20} />
                    </div>
                    <div>
                        <h3 className="text-base font-bold text-gray-900 dark:text-gray-100 leading-none">
                            {t("settings.advanced_thinking.title", { defaultValue: "高级思维与全局配置" })}
                        </h3>
                        <p className="text-[11px] text-gray-500 dark:text-gray-400 mt-1">
                            {t("settings.advanced_thinking.description", { defaultValue: "集中管理思考能力、图像模式及全局指令。" })}
                        </p>
                    </div>
                </div>

                <div className="space-y-4 divide-y divide-gray-100 dark:divide-gray-800">
                    {/* 0. 服务端思考回填开关 */}
                    <div className="pt-0 flex items-center justify-between gap-4">
                        <div className="space-y-1">
                            <h4 className="text-sm font-bold text-gray-900 dark:text-gray-100">
                                {t("settings.advanced_thinking.store_enabled", { defaultValue: "服务端思考块回填" })}
                            </h4>
                            <p className="text-[11px] text-gray-500 dark:text-gray-400 max-w-lg">
                                {t("settings.advanced_thinking.store_enabled_desc", { defaultValue: "默认开启。自动捕获/回填思考过程与签名；商业 Agent 未带回思考时由服务端补齐。自定义 Agent 可通过 DELETE /v1/thinking/sessions/:id 清理。" })}
                            </p>
                        </div>
                        <input
                            type="checkbox"
                            className="toggle toggle-sm toggle-primary"
                            checked={config.experimental?.thinking_store_enabled !== false}
                            onChange={(e) => onChange({
                                ...config,
                                experimental: {
                                    enable_usage_scaling: config.experimental?.enable_usage_scaling ?? false,
                                    ...config.experimental,
                                    thinking_store_enabled: e.target.checked,
                                },
                            })}
                        />
                    </div>

                    {/* 1. 思考预算 (Thinking Budget) */}
                    <div className="pt-0">
                        <ThinkingBudget
                            config={config.thinking_budget || { mode: 'auto', custom_value: 24576 }}
                            onChange={(newConfig) => onChange({ ...config, thinking_budget: newConfig })}
                        />
                    </div>

                    {/* 2. 图像思维模式 (Image Thinking Mode) */}
                    <div className="pt-4">
                        <ImageThinkingMode
                            value={config.image_thinking_mode || 'enabled'}
                            onChange={(newValue) => onChange({ ...config, image_thinking_mode: newValue })}
                        />
                    </div>

                    {/* 3. 全局系统提示词 (Global System Prompt) */}
                    <div className="pt-4">
                        <GlobalSystemPrompt
                            config={config.global_system_prompt || { enabled: false, content: '' }}
                            onChange={(newConfig) => onChange({ ...config, global_system_prompt: newConfig })}
                        />
                    </div>
                </div>
            </div>
        </div>
    );
}
