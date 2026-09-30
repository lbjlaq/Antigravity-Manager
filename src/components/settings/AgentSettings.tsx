import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Save, Check, Bot, Laptop, ShieldCheck, Zap } from "lucide-react";
import { ExperimentalConfig } from "../../types/config";
import { showToast } from "../common/ToastContainer";

interface AgentSettingsProps {
    experimentalConfig?: ExperimentalConfig;
    onChange: (updates: Partial<ExperimentalConfig>) => void;
    onSave?: () => Promise<void> | void;
}

export const AgentSettings: React.FC<AgentSettingsProps> = ({
    experimentalConfig,
    onChange,
    onSave,
}) => {
    const { t } = useTranslation();
    const [isSaving, setIsSaving] = useState(false);
    const [savedSuccessfully, setSavedSuccessfully] = useState(false);

    const isEnabled = experimentalConfig?.enable_cowork_auto_compact ?? false;
    const threshold = experimentalConfig?.cowork_compact_threshold ?? 200000;

    const handleSave = async () => {
        setIsSaving(true);
        try {
            if (onSave) {
                await onSave();
            }
            setSavedSuccessfully(true);
            showToast(t("common.success", { defaultValue: "特定 Agent 设置已保存" }), "success");
            setTimeout(() => setSavedSuccessfully(false), 2000);
        } catch (error: any) {
            console.error("保存特定 Agent 设置失败:", error);
            showToast(t("common.error", { defaultValue: "保存失败" }), "error");
        } finally {
            setIsSaving(false);
        }
    };

    return (
        <div className="space-y-5 text-gray-800 dark:text-gray-200">
            {/* 顶栏说明提示 */}
            <div className="p-3.5 bg-blue-50/70 dark:bg-blue-950/20 rounded-xl border border-blue-200/70 dark:border-blue-800/40 text-xs leading-relaxed space-y-1">
                <div className="font-semibold text-blue-900 dark:text-blue-300 flex items-center gap-1.5">
                    <Bot size={14} className="text-blue-600 dark:text-blue-400" />
                    {t("proxy.agent_settings.banner_title", { defaultValue: "特定 Agent 专属特性与优化治理" })}
                </div>
                <p className="text-gray-600 dark:text-gray-400">
                    {t("proxy.agent_settings.banner_desc", {
                        defaultValue: "针对主流智能体（Claude Desktop、Claude Code、Cursor、Cline 等）的客户端特异性机制与沙箱限制，提供非侵入式的网关级调度协同。默认保持纯净线缆，仅在启用后对指定 Agent 流量精准介入。"
                    })}
                </p>
            </div>

            {/* 板块 1: Claude Desktop 板块 */}
            <div className="p-4 bg-gray-50/70 dark:bg-base-200/60 rounded-xl border border-gray-200/70 dark:border-base-300 space-y-4">
                {/* 标题栏 */}
                <div className="flex items-center justify-between pb-3 border-b border-gray-200/60 dark:border-base-300/60">
                    <div className="flex items-center gap-2">
                        <Laptop size={16} className="text-cyan-500" />
                        <span className="text-sm font-bold text-gray-900 dark:text-white">
                            {t("proxy.agent_settings.claude_desktop.title", { defaultValue: "Claude Desktop 系列" })}
                        </span>
                        <span className="text-[10px] px-2 py-0.5 rounded-full font-medium bg-cyan-100 dark:bg-cyan-900/40 text-cyan-700 dark:text-cyan-300">
                            {t("proxy.agent_settings.claude_desktop.tag", { defaultValue: "Cowork 模式专属" })}
                        </span>
                    </div>
                </div>

                {/* Cowork 模式自动响应式自愈压缩开关 */}
                <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 pb-3 border-b border-gray-200/60 dark:border-base-300/60">
                    <div className="space-y-0.5 max-w-xl">
                        <div className="text-xs font-semibold text-gray-900 dark:text-white flex items-center gap-1.5">
                            <Zap size={14} className="text-amber-500" />
                            {t("proxy.agent_settings.claude_desktop.auto_compact_title", {
                                defaultValue: "Cowork 模式自动响应式压缩自愈 (Auto Reactive Compact)"
                            })}
                        </div>
                        <p className="text-[11px] text-gray-500 dark:text-gray-400 leading-normal">
                            {t("proxy.agent_settings.claude_desktop.auto_compact_desc", {
                                defaultValue: "针对 Cowork 模式无法敲击 /compact 且沙箱旁路本地配置、高频全屏截屏导致上下文迅速膨胀至 200k+（单次 JSON 达 20~36MB）引发几十秒卡顿的顽疾。开启后，网关在后台实行双重确权校验（严格识别 Cowork 会话并对压缩总结请求无条件豁免放行），就地触发客户端内置 Summarizer 自动打上 compact_boundary 折叠历史与截图，恢复秒级极速响应。默认关闭以完整享受 Gemini 100万 Token 超长上下文。"
                            })}
                        </p>
                    </div>
                    <label className="relative inline-flex items-center cursor-pointer select-none shrink-0">
                        <input
                            type="checkbox"
                            checked={isEnabled}
                            onChange={(e) => onChange({ enable_cowork_auto_compact: e.target.checked })}
                            className="sr-only peer"
                        />
                        <div className="w-11 h-6 bg-gray-200 peer-focus:outline-hidden rounded-full peer dark:bg-gray-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all dark:border-gray-600 peer-checked:bg-cyan-600"></div>
                    </label>
                </div>

                {/* 阈值编辑区 (仅开启时可用) */}
                <div className={`space-y-3 transition-all duration-200 ${isEnabled ? "opacity-100" : "opacity-50 pointer-events-none"}`}>
                    <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2">
                        <div className="space-y-0.5">
                            <label className="text-xs font-semibold text-gray-800 dark:text-gray-200">
                                {t("proxy.agent_settings.claude_desktop.threshold_label", {
                                    defaultValue: "压缩触发门限 (Tokens)"
                                })}
                            </label>
                            <p className="text-[11px] text-gray-500 dark:text-gray-400">
                                {t("proxy.agent_settings.claude_desktop.threshold_desc", {
                                    defaultValue: "当 Cowork 会话预估 Token 水位达到此数值时触发自愈信号。全屏截图按官方规格精准折算（~1600 tokens/张）。"
                                })}
                            </p>
                        </div>
                        <div className="flex items-center gap-2">
                            <input
                                type="number"
                                min={50000}
                                max={500000}
                                step={10000}
                                value={threshold}
                                onChange={(e) => {
                                    const val = parseInt(e.target.value, 10);
                                    if (!isNaN(val)) {
                                        onChange({ cowork_compact_threshold: Math.max(50000, val) });
                                    }
                                }}
                                className="w-32 px-3 py-1.5 text-xs text-right font-mono bg-white dark:bg-base-100 border border-gray-300 dark:border-base-300 rounded-lg focus:outline-hidden focus:ring-1 focus:ring-cyan-500"
                            />
                            <span className="text-xs text-gray-500 dark:text-gray-400">Tokens</span>
                        </div>
                    </div>

                    {/* 快捷推荐预设胶囊 */}
                    <div className="flex flex-wrap items-center gap-1.5 pt-1">
                        <span className="text-[11px] text-gray-400 dark:text-gray-500 mr-1">快捷预设:</span>
                        {[
                            { label: "150,000 (极速)", val: 150000 },
                            { label: "180,000 (敏捷)", val: 180000 },
                            { label: "200,000 (官方推荐)", val: 200000 },
                            { label: "250,000 (宽裕)", val: 250000 },
                        ].map((preset) => (
                            <button
                                key={preset.val}
                                type="button"
                                onClick={() => onChange({ cowork_compact_threshold: preset.val })}
                                className={`text-[11px] px-2.5 py-1 rounded-md font-medium transition-all ${
                                    threshold === preset.val
                                        ? "bg-cyan-500 text-white shadow-xs"
                                        : "bg-white dark:bg-base-100 text-gray-600 dark:text-gray-400 border border-gray-200 dark:border-base-300 hover:border-cyan-300 dark:hover:border-cyan-700"
                                }`}
                            >
                                {preset.label}
                            </button>
                        ))}
                    </div>

                    {/* 双重校验安全保障提示 */}
                    <div className="flex items-start gap-2 p-2.5 bg-amber-50/60 dark:bg-amber-950/20 rounded-lg border border-amber-200/60 dark:border-amber-800/30 text-[11px] text-amber-800 dark:text-amber-300">
                        <ShieldCheck size={14} className="shrink-0 mt-0.5 text-amber-600 dark:text-amber-400" />
                        <span>
                            {t("proxy.agent_settings.claude_desktop.safety_note", {
                                defaultValue: "后台双重安全校验：① 仅对 tools 显式包含 mcp__cowork 的桌面会话生效，普通 CLI / Cursor 绝对零误伤；② 对携带 x-stainless-helper: compaction 或 Prompt 包含官方压缩签名的总结请求无条件放行，绝对杜绝卡死会话。"
                            })}
                        </span>
                    </div>
                </div>
            </div>

            {/* 保存按钮 */}
            {onSave && (
                <div className="flex justify-end pt-2">
                    <button
                        onClick={handleSave}
                        disabled={isSaving}
                        className={`btn btn-sm text-xs gap-1.5 font-medium transition-all ${
                            savedSuccessfully
                                ? "bg-green-600 hover:bg-green-700 text-white"
                                : "bg-cyan-600 hover:bg-cyan-700 text-white shadow-xs"
                        }`}
                    >
                        {savedSuccessfully ? (
                            <>
                                <Check size={14} />
                                {t("common.saved", { defaultValue: "已保存" })}
                            </>
                        ) : (
                            <>
                                <Save size={14} />
                                {isSaving ? t("common.saving", { defaultValue: "保存中..." }) : t("common.save", { defaultValue: "保存设置" })}
                            </>
                        )}
                    </button>
                </div>
            )}
        </div>
    );
};

export default AgentSettings;
