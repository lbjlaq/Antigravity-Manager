import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Save, Check, Bot, Laptop, ShieldCheck, Zap, Wrench, RotateCcw, RefreshCw, FolderOpen, HelpCircle, BookOpen, Sparkles, ChevronDown, ChevronUp, AlertTriangle } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { ExperimentalConfig } from "../../types/config";
import { showToast } from "../common/ToastContainer";
import ModalDialog from "../common/ModalDialog";

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
    const isManualCompactEnabled = experimentalConfig?.enable_cowork_manual_compact ?? false;

    // 开启实验性深度归档时的两步确认弹窗状态
    const [isRiskModalOpen, setIsRiskModalOpen] = useState(false);
    const [isGuideModalOpen, setIsGuideModalOpen] = useState(false);

    const handleManualCompactToggle = (checked: boolean) => {
        if (checked) {
            setIsRiskModalOpen(true);
        } else {
            onChange({ enable_cowork_manual_compact: false });
        }
    };

    const handleRiskModalConfirm = () => {
        setIsRiskModalOpen(false);
        setIsGuideModalOpen(true);
    };

    const handleGuideModalConfirm = () => {
        setIsGuideModalOpen(false);
        onChange({ enable_cowork_manual_compact: true });
        showToast("已成功启用 ./compact 深度归档增强", "success");
    };

    const [patchStatus, setPatchStatus] = useState<{
        is_patched: boolean;
        is_patchable: boolean;
        is_8k?: boolean;
        is_legacy?: boolean;
        message: string;
        file_path: string;
        available_installations?: Array<{
            version: string;
            path: string;
            is_patched: boolean;
            is_8k?: boolean;
            size_mb: number;
        }>;
    } | null>(null);
    const [selectedPath, setSelectedPath] = useState<string>("");
    const [customPath, setCustomPath] = useState<string>("");
    const [isCheckingPatch, setIsCheckingPatch] = useState(false);
    const [isPatching, setIsPatching] = useState(false);
    const [isDragging, setIsDragging] = useState(false);
    const [showGuide, setShowGuide] = useState(false);

    // 补丁注入生命周期确认弹窗状态
    const [isConfirmPatchModalOpen, setIsConfirmPatchModalOpen] = useState(false);
    const [isClaudeRunningOnConfirm, setIsClaudeRunningOnConfirm] = useState(false);
    const [isPatchOperationLoading, setIsPatchOperationLoading] = useState(false);

    const activeTargetPath = customPath.trim() || selectedPath.trim() || undefined;

    const handleBrowsePath = async () => {
        try {
            const { open } = await import("@tauri-apps/plugin-dialog");
            const selected = await open({
                title: "选择 Claude.app 应用程序或二进制文件",
                multiple: false,
                directory: false,
            });
            if (selected && typeof selected === "string") {
                setCustomPath(selected);
                handleCheckPatch(selected);
            }
        } catch (err: any) {
            console.warn("打开文件对话框失败，尝试目录模式:", err);
            try {
                const { open } = await import("@tauri-apps/plugin-dialog");
                const selected = await open({
                    title: "选择 Claude.app 应用程序目录",
                    multiple: false,
                    directory: true,
                });
                if (selected && typeof selected === "string") {
                    setCustomPath(selected);
                    handleCheckPatch(selected);
                }
            } catch (innerErr: any) {
                showToast("无法打开文件选择对话框: " + String(innerErr), "error");
            }
        }
    };

    const handleDropFile = (e: React.DragEvent) => {
        e.preventDefault();
        e.stopPropagation();
        setIsDragging(false);

        if (e.dataTransfer.files && e.dataTransfer.files.length > 0) {
            const file = e.dataTransfer.files[0];
            // 在 Tauri / Electron 环境中，file 对象包含原生绝对路径 .path
            const nativePath = (file as any).path || file.name;
            if (nativePath) {
                setCustomPath(nativePath);
                handleCheckPatch(nativePath);
                return;
            }
        }

        // 尝试从纯文本传输中提取路径
        const textData = e.dataTransfer.getData("text/plain");
        if (textData) {
            const cleanPath = textData.trim().replace(/^file:\/\//, "");
            setCustomPath(cleanPath);
            handleCheckPatch(cleanPath);
        }
    };

    const handleCheckPatch = async (pathOverride?: string) => {
        setIsCheckingPatch(true);
        try {
            const p = pathOverride !== undefined ? pathOverride : activeTargetPath;
            const res = await invoke<any>("check_claude_cowork_patch", { filePath: p || null });
            setPatchStatus(res);
            if (!selectedPath && res.available_installations && res.available_installations.length > 0) {
                setSelectedPath(res.available_installations[0].path);
            } else if (!selectedPath && res.file_path) {
                setSelectedPath(res.file_path);
            }
            showToast(res.message, res.is_patched ? "success" : "info");
        } catch (err: any) {
            showToast(String(err), "error");
        } finally {
            setIsCheckingPatch(false);
        }
    };

    // 点击一键注入：先探测 Claude 是否正在运行，再打开对应的确认弹窗
    const handleApplyPatchClick = async () => {
        setIsCheckingPatch(true);
        try {
            const isRunning = await invoke<boolean>("is_claude_desktop_running", {
                filePath: activeTargetPath || null,
            });
            setIsClaudeRunningOnConfirm(isRunning);
            setIsConfirmPatchModalOpen(true);
        } catch (err: any) {
            console.warn("检查 Claude 运行状态失败，默认进入常规注入确认模式:", err);
            setIsClaudeRunningOnConfirm(false);
            setIsConfirmPatchModalOpen(true);
        } finally {
            setIsCheckingPatch(false);
        }
    };

    // 确认注入补丁的执行流程
    const handleConfirmExecutePatch = async () => {
        setIsPatchOperationLoading(true);
        try {
            if (isClaudeRunningOnConfirm) {
                // 1. 如果 Claude 正在运行，先退出
                await invoke("close_claude_desktop", { filePath: activeTargetPath || null });
            }

            // 2. 注入 8k 补丁
            const res = await invoke<string>("apply_claude_cowork_patch", { filePath: activeTargetPath || null });

            if (isClaudeRunningOnConfirm) {
                // 3. 完成后自动重新打开 Claude
                await invoke("launch_claude_desktop", { filePath: activeTargetPath || null });
                showToast("已成功注入 8k 深度归档补丁，并已自动重新打开 Claude！", "success");
            } else {
                showToast(res, "success");
            }

            setIsConfirmPatchModalOpen(false);
            await handleCheckPatch();
        } catch (err: any) {
            showToast(String(err), "error");
        } finally {
            setIsPatchOperationLoading(false);
        }
    };

    const handleRevertPatch = async () => {
        setIsPatching(true);
        try {
            const res = await invoke<string>("revert_claude_cowork_patch", { filePath: activeTargetPath || null });
            showToast(res, "success");
            await handleCheckPatch();
        } catch (err: any) {
            showToast(String(err), "error");
        } finally {
            setIsPatching(false);
        }
    };

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

                {/* ===== 实验性功能：Claude Cowork 会话长寿命压缩治理套件 ===== */}
                <div className="pt-4 border-t border-purple-200/60 dark:border-purple-900/30 space-y-3.5">
                    {/* 头部标题与教学入口 */}
                    <div className="flex flex-col sm:flex-row sm:items-start justify-between gap-3">
                        <div className="space-y-1 max-w-xl">
                            <div className="flex items-center gap-1.5 font-semibold text-xs text-purple-900 dark:text-purple-300">
                                <Sparkles size={14} className="text-purple-600 dark:text-purple-400 shrink-0" />
                                <span>Claude Cowork 深度归档增强 (Deep Compact)</span>
                                <span className="text-[10px] px-1.5 py-0.5 rounded bg-purple-100 dark:bg-purple-950/70 text-purple-700 dark:text-purple-300 border border-purple-300/80 dark:border-purple-800/60 font-medium">
                                    实验性功能
                                </span>
                            </div>
                            <p className="text-[11px] text-gray-500 dark:text-gray-400 leading-normal">
                                针对 Claude Cowork 模式无法通过 <code className="px-1 py-0.5 bg-gray-100 dark:bg-base-300 rounded font-mono text-[10px]">/compact</code> 进行深度归档（被桌面拦截为未知技能且官方强制保留 50% 历史）的底层缺陷。
                                开启后支持在会话中发送 <code className="px-1 py-0.5 bg-gray-100 dark:bg-base-300 rounded font-mono text-[10px]">./compact</code> 穿透触发深度归档，并可配合下方微创补丁注入 8k 深度归档活跃上下文硬预算。后台已与上方自动压缩无缝互锁，绝不撞车。
                            </p>
                        </div>
                        <button
                            type="button"
                            onClick={() => setShowGuide(!showGuide)}
                            className="btn btn-xs bg-purple-50 hover:bg-purple-100 dark:bg-purple-950/40 dark:hover:bg-purple-900/50 text-purple-700 dark:text-purple-300 border border-purple-200 dark:border-purple-800 text-[11px] gap-1 shrink-0 mt-0.5"
                        >
                            <BookOpen size={12} />
                            <span>{showGuide ? "收起使用教程" : "使用教程与原理解析"}</span>
                            {showGuide ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
                        </button>
                    </div>

                    {/* 可折叠使用教程与原理解析卡片 */}
                    {showGuide && (
                        <div className="p-3.5 bg-gradient-to-br from-purple-50/70 to-indigo-50/40 dark:from-purple-950/30 dark:to-base-200/50 rounded-lg border border-purple-200/70 dark:border-purple-800/40 text-[11px] space-y-2 animate-fadeIn">
                            <div className="font-semibold text-purple-900 dark:text-purple-200 flex items-center gap-1.5">
                                <HelpCircle size={13} className="text-purple-600 dark:text-purple-400" />
                                <span>为什么需要打【./compact】？核心机制与操作指南</span>
                            </div>
                            <ol className="list-decimal list-inside space-y-1.5 text-gray-600 dark:text-gray-300 leading-relaxed pl-1">
                                <li>
                                    <strong>为什么不是 <code>/compact</code>？</strong>
                                    Claude Desktop 前端缺少命令解析器，直接敲 <code>/compact</code> 会被前端误包装为未知 Skill 拦截报错；而在前面加上小圆点输入 <strong><code>./compact</code></strong>，前端会将其判定为普通聊天文本放行至网关。
                                </li>
                                <li>
                                    <strong>网关如何智能捕获与协同？</strong>
                                    8045 网关在捕获到 <code>./compact</code> 时，就地向客户端协调回送微创自愈假信号，激活客户端内置的 Summarizer 进行历史深度折叠，并在完成后优雅回显真实节省的 Token 量（如 <code>Compacted conversation · saved 52k tokens</code>）。
                                </li>
                                <li>
                                    <strong>视觉界面保留 vs 底层物理轻量化</strong>：
                                    压缩后，您在桌面 UI 聊天记录中依然能完整向上滚动查看所有历史讨论（对视觉零破坏）；但在底层发送给大模型处理时，前面的数百轮历史已被 100% 浓缩为轻量级摘要，彻底释放 80%~90% 的上下文包袱，恢复秒级响应！
                                </li>
                                <li>
                                    <strong>自动压缩与手动深度归档的智能互锁</strong>：
                                    两者同时开启时，网关在手动 <code>./compact</code> 期间会自动压制自动压缩门限，两者无缝共生、智能互锁，彻底杜绝双重 400 撞车！
                                </li>
                            </ol>
                        </div>
                    )}

                    {/* 开关：启用 ./compact 对话框手动深度归档 */}
                    <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 p-3 bg-white dark:bg-base-100 rounded-lg border border-gray-200/80 dark:border-base-300">
                        <div className="space-y-0.5">
                            <div className="text-xs font-semibold text-gray-900 dark:text-white flex items-center gap-1.5">
                                <Zap size={13} className="text-purple-600 dark:text-purple-400" />
                                <span>启用 ./compact 对话框指令穿透</span>
                            </div>
                            <p className="text-[11px] text-gray-500 dark:text-gray-400">
                                开启后，在 Cowork 任意会话对话框中直接发送 <code className="px-1 py-0.5 bg-gray-100 dark:bg-base-300 rounded font-mono text-[10px]">./compact</code> 即可随时按需触发全量历史折叠压缩。默认关闭。
                            </p>
                        </div>
                        <label className="relative inline-flex items-center cursor-pointer select-none shrink-0">
                            <input
                                type="checkbox"
                                checked={isManualCompactEnabled}
                                onChange={(e) => handleManualCompactToggle(e.target.checked)}
                                className="sr-only peer"
                            />
                            <div className="w-11 h-6 bg-gray-200 peer-focus:outline-hidden rounded-full peer dark:bg-gray-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all dark:border-gray-600 peer-checked:bg-purple-600"></div>
                        </label>
                    </div>

                    {/* 外置微创补丁操作区 (与网关完全解耦) */}
                    <div className="p-3 bg-purple-50/30 dark:bg-purple-950/10 rounded-lg border border-purple-200/60 dark:border-purple-900/30 space-y-2.5">
                        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2">
                            <div className="text-[11px] text-gray-700 dark:text-gray-300 flex items-center gap-1.5">
                                <Wrench size={13} className="text-purple-600 dark:text-purple-400 shrink-0" />
                                <span className="font-medium">客户端二进制微创补丁工具 (仅 macOS)</span>
                            </div>
                            <div className="flex items-center gap-2 shrink-0 flex-wrap">
                                <button
                                    type="button"
                                    onClick={() => handleCheckPatch()}
                                    disabled={isCheckingPatch}
                                    className="btn btn-xs h-7 min-h-[28px] px-3 bg-white dark:bg-base-100 border border-gray-200 dark:border-base-300 hover:border-purple-300 text-gray-700 dark:text-gray-300 text-[11px] gap-1.5 shrink-0 whitespace-nowrap"
                                >
                                    <RefreshCw size={12} className={isCheckingPatch ? "animate-spin text-purple-500" : ""} />
                                    <span>检查补丁状态</span>
                                </button>
                                <button
                                    type="button"
                                    onClick={handleApplyPatchClick}
                                    disabled={isPatching || isCheckingPatch || isPatchOperationLoading}
                                    className="btn btn-xs h-7 min-h-[28px] px-3.5 bg-purple-600 hover:bg-purple-700 text-white text-[11px] gap-1.5 shadow-xs shrink-0 whitespace-nowrap"
                                >
                                    <Wrench size={12} />
                                    <span>一键注入补丁</span>
                                </button>
                                <button
                                    type="button"
                                    onClick={handleRevertPatch}
                                    disabled={isPatching || isPatchOperationLoading}
                                    className="btn btn-xs h-7 min-h-[28px] px-3 bg-gray-200 dark:bg-base-300 hover:bg-gray-300 dark:hover:bg-base-100 text-gray-700 dark:text-gray-300 text-[11px] gap-1.5 shrink-0 whitespace-nowrap"
                                >
                                    <RotateCcw size={12} />
                                    <span>还原原生</span>
                                </button>
                            </div>
                        </div>

                        {/* 目标版本自动发现选择器与自定义路径 */}
                        <div className="space-y-1.5 pt-1 text-[11px]">
                            {patchStatus?.available_installations && patchStatus.available_installations.length > 0 && (
                                <div className="flex flex-col sm:flex-row sm:items-center gap-2">
                                    <span className="text-gray-500 dark:text-gray-400 shrink-0">检测到本机安装:</span>
                                    <select
                                        value={selectedPath}
                                        onChange={(e) => {
                                            setSelectedPath(e.target.value);
                                            setCustomPath("");
                                            handleCheckPatch(e.target.value);
                                        }}
                                        className="select select-xs select-bordered bg-white dark:bg-base-100 text-[11px] font-mono flex-1 truncate"
                                    >
                                        {patchStatus.available_installations.map((inst, idx) => (
                                            <option key={idx} value={inst.path}>
                                                v{inst.version} ({inst.size_mb} MB) {inst.is_8k ? " [8k深度补丁]" : inst.is_patched ? " [旧版补丁需升级]" : " [官方原版]"} - {inst.path}
                                            </option>
                                        ))}
                                    </select>
                                </div>
                            )}

                            <div className="flex flex-col sm:flex-row sm:items-center gap-2">
                                <span className="text-gray-500 dark:text-gray-400 shrink-0">自定义路径:</span>
                                <div
                                    onDragOver={(e) => {
                                        e.preventDefault();
                                        setIsDragging(true);
                                    }}
                                    onDragLeave={() => setIsDragging(false)}
                                    onDrop={handleDropFile}
                                    className={`flex items-center gap-1.5 flex-1 p-0.5 rounded-lg border transition-colors ${
                                        isDragging
                                            ? "border-purple-500 bg-purple-50/50 dark:bg-purple-950/20"
                                            : "border-transparent"
                                    }`}
                                >
                                    <input
                                        type="text"
                                        value={customPath}
                                        placeholder="支持直接将 Claude.app 拖入此处，或点击右侧浏览"
                                        onChange={(e) => setCustomPath(e.target.value)}
                                        onBlur={() => {
                                            if (customPath.trim()) {
                                                handleCheckPatch(customPath.trim());
                                            }
                                        }}
                                        className="input input-xs input-bordered bg-white dark:bg-base-100 text-[11px] font-mono flex-1"
                                    />
                                    <button
                                        type="button"
                                        onClick={handleBrowsePath}
                                        title="浏览选择 Claude.app 目录或可执行文件"
                                        className="btn btn-xs bg-white dark:bg-base-100 border border-gray-200 dark:border-base-300 hover:border-purple-300 text-gray-700 dark:text-gray-300 text-[11px] px-2 gap-1 shrink-0"
                                    >
                                        <FolderOpen size={12} className="text-amber-500" />
                                        <span>浏览...</span>
                                    </button>
                                </div>
                            </div>
                        </div>

                        {patchStatus && (
                            <div className={`p-2 rounded text-[11px] border font-mono ${
                                patchStatus.is_8k
                                    ? "bg-green-50 dark:bg-green-950/20 border-green-200 dark:border-green-800/40 text-green-800 dark:text-green-300"
                                    : patchStatus.is_legacy
                                    ? "bg-amber-50 dark:bg-amber-950/20 border-amber-200 dark:border-amber-800/40 text-amber-800 dark:text-amber-300"
                                    : patchStatus.is_patched
                                    ? "bg-green-50 dark:bg-green-950/20 border-green-200 dark:border-green-800/40 text-green-800 dark:text-green-300"
                                    : "bg-gray-100 dark:bg-base-100 border-gray-200 dark:border-base-300 text-gray-700 dark:text-gray-300"
                            }`}>
                                <div className="font-semibold mb-0.5">{patchStatus.message}</div>
                                <div className="text-[10px] text-gray-500 truncate" title={patchStatus.file_path}>生效路径: {patchStatus.file_path}</div>
                            </div>
                        )}
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
            {/* 弹窗 1: 启用说明与安全边界 (极简清晰版) */}
            <ModalDialog
                isOpen={isRiskModalOpen}
                title="启用 Cowork 压缩增强 (实验性)"
                type="info"
                confirmText="下一步：查看用法"
                cancelText="取消"
                onConfirm={handleRiskModalConfirm}
                onCancel={() => setIsRiskModalOpen(false)}
            >
                <div className="space-y-2.5 text-xs text-gray-600 dark:text-gray-300">
                    <div className="p-2.5 bg-blue-50/70 dark:bg-blue-950/30 rounded-lg border border-blue-200/60 dark:border-blue-800/40 space-y-1">
                        <div className="font-semibold text-blue-900 dark:text-blue-200 flex items-center gap-1.5">
                            <span>✨ 只开开关（完全零风险、零侵入）</span>
                        </div>
                        <div className="text-[11px] text-blue-800/80 dark:text-blue-300/80 leading-relaxed">
                            不修改任何本地软件。开启后即可在会话中敲 <code className="font-mono font-bold px-1 bg-white dark:bg-base-100 rounded">./compact</code> 触发官方原生折半压缩（安全释放 ~40% 上下文）。
                        </div>
                    </div>

                    <div className="p-2.5 bg-purple-50/70 dark:bg-purple-950/30 rounded-lg border border-purple-200/60 dark:border-purple-800/40 space-y-1">
                        <div className="font-semibold text-purple-900 dark:text-purple-200 flex items-center gap-1.5">
                            <span>⚡ 配合补丁（可选，极限深度瘦身）</span>
                        </div>
                        <div className="text-[11px] text-purple-800/80 dark:text-purple-300/80 leading-relaxed">
                            若后续点击注入下方微创补丁，将破除官方 50% 历史残留限制，把上下文活跃消息直接削减到 8k 以下（释放 85%+）。系统自动隔离备份，随时一键还原。
                        </div>
                    </div>
                </div>
            </ModalDialog>

            {/* 弹窗 2: ./compact 极简教学指南 */}
            <ModalDialog
                isOpen={isGuideModalOpen}
                title="使用教学：如何触发压缩"
                type="success"
                confirmText="我已掌握，立即开启"
                cancelText="返回"
                onConfirm={handleGuideModalConfirm}
                onCancel={() => setIsGuideModalOpen(false)}
            >
                <div className="space-y-3 text-xs text-gray-600 dark:text-gray-300">
                    <p className="text-gray-700 dark:text-gray-200">
                        在 Claude Cowork 任意对话框中直接发送：
                    </p>
                    <div className="flex items-center justify-between p-2.5 bg-purple-100/70 dark:bg-purple-950/50 rounded-lg border border-purple-300/70 dark:border-purple-700 font-mono text-sm text-purple-900 dark:text-purple-200 font-bold">
                        <span>./compact</span>
                        <span className="text-[11px] font-normal text-purple-600 dark:text-purple-300">（必须带小圆点）</span>
                    </div>
                    <div className="text-[11px] text-gray-500 dark:text-gray-400 space-y-1 leading-relaxed">
                        <p>• <strong>为什么带点</strong>：因为直接打 <code>/compact</code> 会被桌面拦截，带点可穿透网关。</p>
                        <p>• <strong>聊天记录不丢</strong>：UI 界面历史完整可见，底层自动减负恢复极速响应。</p>
                    </div>
                </div>
            </ModalDialog>

            {/* 弹窗 3: 补丁注入运行状态检测与自动重启生命周期弹窗 */}
            <ModalDialog
                isOpen={isConfirmPatchModalOpen}
                title={isClaudeRunningOnConfirm ? "退出 Claude 并注入" : "是否确认注入"}
                type={isClaudeRunningOnConfirm ? "confirm" : "info"}
                confirmText={isClaudeRunningOnConfirm ? "退出 Claude 并注入" : "确认注入"}
                cancelText="取消"
                isDestructive={isClaudeRunningOnConfirm}
                isLoading={isPatchOperationLoading}
                onConfirm={handleConfirmExecutePatch}
                onCancel={() => !isPatchOperationLoading && setIsConfirmPatchModalOpen(false)}
            >
                {isClaudeRunningOnConfirm ? (
                    <div className="space-y-2.5 text-xs text-gray-600 dark:text-gray-300">
                        <p className="leading-relaxed">
                            检测到 <strong>Claude Desktop</strong> 客户端当前正在运行中。
                        </p>
                        <div className="p-2.5 bg-amber-50 dark:bg-amber-950/30 rounded-lg border border-amber-200 dark:border-amber-800/40 text-amber-800 dark:text-amber-300 space-y-1">
                            <div className="font-semibold flex items-center gap-1.5">
                                <AlertTriangle size={14} className="shrink-0 text-amber-600 dark:text-amber-400" />
                                <span>注入前需退出客户端</span>
                            </div>
                            <p className="text-[11px] leading-relaxed">
                                注入补丁需要先退出正在运行的 Claude 客户端以解除文件锁并加载新二进制。<strong>注入完成后，系统将自动为您重新打开 Claude。</strong>
                            </p>
                        </div>
                    </div>
                ) : (
                    <div className="space-y-2.5 text-xs text-gray-600 dark:text-gray-300">
                        <p className="leading-relaxed">
                            当前未检测到运行中的 Claude Desktop，将直接对目标可执行文件进行微创注入。
                        </p>
                        <div className="p-2.5 bg-purple-50 dark:bg-purple-950/30 rounded-lg border border-purple-200 dark:border-purple-800/40 text-purple-900 dark:text-purple-200 space-y-1">
                            <div className="font-semibold flex items-center gap-1.5">
                                <Sparkles size={14} className="shrink-0 text-purple-600 dark:text-purple-400" />
                                <span>8k 深度归档微创等长补丁</span>
                            </div>
                            <p className="text-[11px] leading-relaxed">
                                保留最新 8k 活跃消息上下文，超出历史 100% 浓缩归档，压缩率突破 60%+。系统将自动创建 .bak 安全备份，支持随时一键还原。
                            </p>
                        </div>
                    </div>
                )}
            </ModalDialog>
        </div>
    );
};

export default AgentSettings;
