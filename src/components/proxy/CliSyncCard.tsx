import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import {
    Terminal,
    Check,
    AlertCircle,
    RefreshCw,
    CodeXml,
    Loader2,
    Eye,
    RotateCcw,
    Copy,
    X,
    Bot,
    Trash2,
    Sparkles
} from 'lucide-react';
import { copyToClipboard } from '../../utils/clipboard';
import { request as invoke } from '../../utils/request';
import { showToast } from '../common/ToastContainer';
import ModalDialog from '../common/ModalDialog';
import { cn } from '../../utils/cn';
import { DroidSyncModal } from './DroidSyncModal';
import { OpenCodeSyncModal } from './OpenCodeSyncModal';
import { HermesSyncModal } from './HermesSyncModal';
import { OpenClawSyncModal } from './OpenClawSyncModal';
import { useProxyModels } from '../../hooks/useProxyModels';
import GroupedSelect from '../common/GroupedSelect';
import { Claude, OpenAI, Gemini, Grok, OpenCode, Github as LobeGithub, HermesAgent, OpenClaw } from '@lobehub/icons';
import { JeikCodeIcon } from '../common/JeikCodeIcon';

interface CliSyncCardProps {
    proxyUrl: string;
    apiKey: string;
    className?: string;
}

type CliAppType = 'Claude' | 'Codex' | 'JeikCode' | 'GrokBuild' | 'Hermes' | 'OpenClaw' | 'Gemini' | 'OpenCode' | 'Droid';

interface CliStatus {
    installed: boolean;
    version: string | null;
    is_synced: boolean;
    has_backup: boolean;
    current_base_url: string | null;
    files: string[];
    synced_count?: number;
}

export const CliSyncCard = ({ proxyUrl, apiKey, className }: CliSyncCardProps) => {
    const { t } = useTranslation();
    const [statuses, setStatuses] = useState<Record<CliAppType, CliStatus | null>>({
        Claude: null,
        Codex: null,
        JeikCode: null,
        GrokBuild: null,
        Gemini: null,
        OpenCode: null,
        Droid: null,
        Hermes: null,
        OpenClaw: null
    });
    const [loading, setLoading] = useState<Record<CliAppType, boolean>>({
        Claude: false,
        Codex: false,
        JeikCode: false,
        GrokBuild: false,
        Gemini: false,
        OpenCode: false,
        Droid: false,
        Hermes: false,
        OpenClaw: false
    });
    const [syncing, setSyncing] = useState<Record<CliAppType, boolean>>({
        Claude: false,
        Codex: false,
        JeikCode: false,
        GrokBuild: false,
        Gemini: false,
        OpenCode: false,
        Droid: false,
        Hermes: false,
        OpenClaw: false
    });
    const [syncAccounts, setSyncAccounts] = useState(false);
    const [droidSyncModal, setDroidSyncModal] = useState(false);
    const [hermesSyncModal, setHermesSyncModal] = useState(false);
    const [openClawSyncModal, setOpenClawSyncModal] = useState(false);
    const [selectedModels, setSelectedModels] = useState<Record<CliAppType, string>>({
        Claude: 'claude-sonnet-4-6',
        Codex: 'gpt-oss-120b-medium',
        JeikCode: 'gemini-3.8-flash-high',
        GrokBuild: 'gemini-3.8-flash-high',
        Gemini: 'gemini-3.1-pro-high',
        OpenCode: '',
        Droid: '',
        Hermes: '',
        OpenClaw: ''
    });
    const [viewingConfig, setViewingConfig] = useState<{
        app: CliAppType,
        content: string,
        fileName: string,
        allFiles: string[]
    } | null>(null);
    const [restoreConfirmApp, setRestoreConfirmApp] = useState<CliAppType | null>(null);
    const [syncConfirmApp, setSyncConfirmApp] = useState<CliAppType | null>(null);
    const [openCodeSyncModal, setOpenCodeSyncModal] = useState(false);
    const [clearConfirmApp, setClearConfirmApp] = useState<CliAppType | null>(null);

    const { models: proxyModels } = useProxyModels();

    const modelOptions = proxyModels.map(m => ({
        value: m.id,
        label: m.name,
        group: m.group || 'General'
    }));

    // 根据不同的 CLI 应用格式化 Proxy URL
    const getFormattedProxyUrl = useCallback((app: CliAppType) => {
        if (!proxyUrl) return '';
        const base = proxyUrl.trimEnd().replace(/\/+$/, '');
        // Codex & OpenCode & JeikCode & GrokBuild & Hermes & OpenClaw (Anthropic / OpenAI / Responses 协议) 通常需要带 /v1
        if (app === 'Codex' || app === 'OpenCode' || app === 'JeikCode' || app === 'GrokBuild' || app === 'Hermes' || app === 'OpenClaw') {
            return base.endsWith('/v1') ? base : `${base}/v1`;
        }
        // Claude 和 Gemini 的 SDK 通常会自动处理版本路径或不需要 /v1
        return base.replace(/\/v1$/, '');
    }, [proxyUrl]);

    const checkStatus = useCallback(async (app: CliAppType) => {
        setLoading(prev => ({ ...prev, [app]: true }));
        try {
            const formattedUrl = getFormattedProxyUrl(app);
            let command: string;
            let params: Record<string, unknown>;
            if (app === 'Droid') {
                command = 'get_droid_sync_status';
                params = { proxyUrl: formattedUrl };
            } else if (app === 'OpenCode') {
                command = 'get_opencode_sync_status';
                params = { proxyUrl: formattedUrl };
            } else if (app === 'Hermes') {
                command = 'get_hermes_sync_status';
                params = { proxyUrl: formattedUrl };
            } else if (app === 'OpenClaw') {
                command = 'get_openclaw_sync_status';
                params = { proxyUrl: formattedUrl };
            } else {
                command = 'get_cli_sync_status';
                params = { appType: app, proxyUrl: formattedUrl };
            }

            const status = await invoke<CliStatus>(command, params);
            setStatuses(prev => ({ ...prev, [app]: status }));
        } catch (error) {
            console.error(`Failed to check ${app} status:`, error);
        } finally {
            setLoading(prev => ({ ...prev, [app]: false }));
        }
    }, [getFormattedProxyUrl]);

    const handleSync = async (app: CliAppType) => {
        if (app === 'Droid') {
            setDroidSyncModal(true);
            return;
        }
        if (app === 'OpenCode') {
            setOpenCodeSyncModal(true);
            return;
        }
        if (app === 'Hermes') {
            setHermesSyncModal(true);
            return;
        }
        if (app === 'OpenClaw') {
            setOpenClawSyncModal(true);
            return;
        }
        setSyncConfirmApp(app);
    };

    const executeSync = async () => {
        const app = syncConfirmApp;
        if (!app) return;
        setSyncConfirmApp(null);

        if (!proxyUrl || !apiKey) {
            showToast(t('proxy.cli_sync.toast.config_missing', { defaultValue: '请先生成 API Key 并启动服务' }), 'error');
            return;
        }

        try {
            const formattedUrl = getFormattedProxyUrl(app);
            const command = app === 'OpenCode' ? 'execute_opencode_sync' : 'execute_cli_sync';
            const params = app === 'OpenCode'
                ? { proxyUrl: formattedUrl, apiKey: apiKey, syncAccounts: syncAccounts }
                : { appType: app, proxyUrl: formattedUrl, apiKey: apiKey, model: selectedModels[app] };

            await invoke(command, params);
            showToast(t(app === 'OpenCode' ? 'proxy.opencode_sync.toast.sync_success' : 'proxy.cli_sync.toast.sync_success', { name: app, defaultValue: `${app} synced successfully` }), 'success');
            await checkStatus(app);
        } catch (error: any) {
            showToast(t(app === 'OpenCode' ? 'proxy.opencode_sync.toast.sync_error' : 'proxy.cli_sync.toast.sync_error', { name: app, error: error.toString(), defaultValue: `Sync failed: ${error.toString()}` }), 'error');
        } finally {
            setSyncing(prev => ({ ...prev, [app]: false }));
        }
    };

    const handleRestore = (app: CliAppType) => {
        setRestoreConfirmApp(app);
    };

    const executeRestore = async () => {
        if (!restoreConfirmApp) return;
        const app = restoreConfirmApp;
        setRestoreConfirmApp(null);

        setSyncing(prev => ({ ...prev, [app]: true }));
        try {
            const command = app === 'Droid' ? 'execute_droid_restore' : app === 'OpenCode' ? 'execute_opencode_restore' : app === 'Hermes' ? 'execute_hermes_restore' : app === 'OpenClaw' ? 'execute_openclaw_restore' : 'execute_cli_restore';
            const params = (app === 'Droid' || app === 'OpenCode' || app === 'Hermes' || app === 'OpenClaw') ? {} : { appType: app };
            await invoke(command, params);
            showToast(t('common.success'), 'success');
            await checkStatus(app);
        } catch (error: any) {
            showToast(error.toString(), 'error');
        } finally {
            setSyncing(prev => ({ ...prev, [app]: false }));
        }
    };

    const handleClear = (app: CliAppType) => {
        setClearConfirmApp(app);
    };

    const executeClear = async () => {
        if (!clearConfirmApp) return;
        const app = clearConfirmApp;
        setClearConfirmApp(null);

        setSyncing(prev => ({ ...prev, [app]: true }));
        try {
            if (app === 'Hermes') {
                await invoke('execute_hermes_clear');
                showToast(t('proxy.hermes_sync.toast.clear_success', { defaultValue: 'Hermes configuration cleared successfully' }), 'success');
            } else if (app === 'OpenClaw') {
                await invoke('execute_openclaw_clear');
                showToast(t('proxy.openclaw_sync.toast.clear_success', { defaultValue: 'OpenClaw configuration cleared successfully' }), 'success');
            } else {
                const formattedUrl = getFormattedProxyUrl(app);
                await invoke('execute_opencode_clear', { proxyUrl: formattedUrl, clearLegacy: true });
                showToast(t('proxy.opencode_sync.toast.clear_success', { defaultValue: 'OpenCode cleared successfully' }), 'success');
            }
            await checkStatus(app);
        } catch (error: any) {
            const toastKey = app === 'Hermes'
                ? 'proxy.hermes_sync.toast.clear_error'
                : app === 'OpenClaw'
                ? 'proxy.openclaw_sync.toast.clear_error'
                : 'proxy.opencode_sync.toast.clear_error';
            showToast(t(toastKey, { error: error.toString(), defaultValue: `Clear failed: ${error.toString()}` }), 'error');
        } finally {
            setSyncing(prev => ({ ...prev, [app]: false }));
        }
    };

    const handleViewConfig = async (app: CliAppType, fileName?: string) => {
        try {
            const status = statuses[app];
            if (!status) return;

            const targetFile = fileName || status.files[0];
            let command: string;
            let params: Record<string, unknown>;
            if (app === 'Droid') {
                command = 'get_droid_config_content';
                params = {};
            } else if (app === 'OpenCode') {
                command = 'get_opencode_config_content';
                params = { request: { fileName: targetFile } };
            } else if (app === 'Hermes') {
                command = 'get_hermes_config_content';
                params = {};
            } else if (app === 'OpenClaw') {
                command = 'get_openclaw_config_content';
                params = {};
            } else {
                command = 'get_cli_config_content';
                params = { appType: app, fileName: targetFile };
            }

            const content = await invoke<string>(command, params);
            setViewingConfig({
                app,
                content,
                fileName: targetFile,
                allFiles: status.files
            });
        } catch (error: any) {
            showToast(error.toString(), 'error');
        }
    };

    useEffect(() => {
        checkStatus('Claude');
        checkStatus('Codex');
        checkStatus('JeikCode');
        checkStatus('GrokBuild');
        checkStatus('Hermes');
        checkStatus('OpenClaw');
        checkStatus('Gemini');
        checkStatus('OpenCode');
        checkStatus('Droid');
    }, [checkStatus]);

    const openExternalUrl = async (url: string) => {
        try {
            const { openUrl } = await import('@tauri-apps/plugin-opener');
            await openUrl(url);
        } catch {
            window.open(url, '_blank', 'noopener,noreferrer');
        }
    };

    const renderCliItem = (app: CliAppType, icon: React.ReactNode, name: string) => {
        const status = statuses[app];
        const isAppLoading = loading[app];
        const isAppSyncing = syncing[app];

        return (
            <div className={cn(
                "flex flex-col rounded-xl border p-4 shadow-sm transition-all duration-300 group",
                app === 'JeikCode'
                    ? "bg-gradient-to-b from-emerald-500/[0.04] via-white/80 to-white/60 dark:from-emerald-950/20 dark:via-gray-800/60 dark:to-gray-800/40 border-emerald-500/30 dark:border-emerald-500/40 hover:border-emerald-400 dark:hover:border-emerald-400 shadow-emerald-500/5 hover:shadow-emerald-500/15 ring-1 ring-emerald-500/10"
                    : "bg-white/50 dark:bg-gray-800/40 border-gray-100 dark:border-white/5 hover:shadow-lg hover:border-blue-200/50 dark:hover:border-blue-500/30"
            )}>
                <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-y-3 gap-x-2 mb-3">
                    <div className="flex items-center gap-3 min-w-0">
                        <div className={cn(
                            "w-10 h-10 shrink-0 flex items-center justify-center transition-transform duration-300 group-hover:scale-105 p-0 rounded-xl overflow-hidden shadow-sm border border-gray-200/60 dark:border-white/10",
                            app === 'JeikCode' && "shadow-md shadow-emerald-500/10 border-emerald-500/30 dark:border-emerald-500/40"
                        )}>
                            {app === 'JeikCode' ? (
                                <JeikCodeIcon size="100%" className="w-full h-full" />
                            ) : (
                                icon
                            )}
                        </div>
                        <div className="min-w-0 flex-1">
                            <h4 className="text-sm font-bold text-gray-900 dark:text-gray-100 leading-tight whitespace-nowrap">
                                {name}
                            </h4>
                            <div className="mt-1 flex items-center gap-1.5 flex-wrap">
                                {isAppLoading ? (
                                    <div className="flex items-center gap-1 text-[10px] text-gray-400">
                                        <Loader2 size={10} className="animate-spin" />
                                        {t('proxy.cli_sync.status.detecting')}
                                    </div>
                                ) : status?.installed ? (
                                    <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-blue-50 dark:bg-blue-900/20 text-blue-600 dark:text-blue-400 font-bold whitespace-nowrap">
                                        {t('proxy.cli_sync.status.installed', { version: status.version })}
                                    </span>
                                ) : (
                                    <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-gray-100 dark:bg-gray-800 text-gray-400 font-medium whitespace-nowrap">
                                        {t('proxy.cli_sync.status.not_installed')}
                                    </span>
                                )}
                                {app === 'JeikCode' && (
                                    <span className="inline-flex items-center gap-1 text-[9px] px-1.5 py-0.5 rounded-full bg-gradient-to-r from-emerald-500/20 to-teal-500/20 text-emerald-600 dark:text-emerald-400 border border-emerald-500/30 font-bold shrink-0 shadow-2xs">
                                        <Sparkles size={10} className="text-emerald-500 animate-spin-once" />
                                        {t('proxy.cli_sync.jeikcode_recommended', { defaultValue: '推荐使用' })}
                                    </span>
                                )}
                            </div>
                        </div>
                    </div>

                    {/* Show Sync Status if installed OR if it's OpenCode/JeikCode/GrokBuild/Hermes/OpenClaw */}
                    {!isAppLoading && (status?.installed || (app === 'OpenCode' || app === 'JeikCode' || app === 'GrokBuild' || app === 'Hermes' || app === 'OpenClaw') && status?.current_base_url) && (
                        <div
                            className={cn(
                                "inline-flex items-center justify-center transition-all shrink-0 whitespace-nowrap shadow-sm",
                                status.is_synced
                                    ? "w-6 h-6 rounded-full bg-gradient-to-tr from-green-500 to-emerald-600 text-white shadow-emerald-500/20"
                                    : "w-6 h-6 rounded-full bg-amber-100 dark:bg-amber-900/40 text-amber-600 dark:text-amber-500 border border-amber-200/60 dark:border-amber-800/40"
                            )}
                            title={status.is_synced ? t('proxy.cli_sync.status.synced', { defaultValue: '已指向本项目' }) : t('proxy.cli_sync.status.not_synced', { defaultValue: '未同步' })}
                        >
                            {status.is_synced ? (
                                <Check size={14} className="stroke-[3]" />
                            ) : (
                                <AlertCircle size={13} />
                            )}
                        </div>
                    )}
                </div>

                {/* JeikCode 专属官方项目推荐卡片（贴满方格，充实饱满，点击跳转 GitHub） */}
                {app === 'JeikCode' && (
                    <div
                        onClick={() => openExternalUrl('https://github.com/jeikl/JeikCode')}
                        className="mb-3 p-3 rounded-xl border border-emerald-500/30 bg-gradient-to-br from-emerald-500/[0.09] via-teal-500/[0.05] to-cyan-500/[0.08] dark:from-emerald-950/40 dark:via-teal-950/25 dark:to-cyan-950/30 hover:border-emerald-400 hover:shadow-md transition-all duration-300 cursor-pointer group/link select-none w-full"
                        title="点击访问 JeikCode 官方 GitHub 仓库 (https://github.com/jeikl/JeikCode)"
                    >
                        {/* 顶栏：Logo + 标题与作者 + 右侧 GitHub 放大图标 */}
                        <div className="flex items-center justify-between gap-2 mb-1.5">
                            <div className="flex items-center gap-1.5 min-w-0">
                                <JeikCodeIcon size={16} />
                                <div className="flex items-baseline gap-1 min-w-0">
                                    <span className="text-[11px] font-black text-emerald-900 dark:text-emerald-200 tracking-tight whitespace-nowrap">
                                        {t('proxy.cli_sync.jeikcode_badge', { defaultValue: 'Best Matched' })}
                                    </span>
                                    <span className="text-[9px] text-emerald-700/80 dark:text-emerald-400/80 font-semibold whitespace-nowrap">
                                        · {t('proxy.cli_sync.jeikcode_author', { defaultValue: 'by Jeikl' })}
                                    </span>
                                </div>
                            </div>
                            <span className="inline-flex items-center text-gray-700 dark:text-gray-300 group-hover/link:text-emerald-500 group-hover/link:scale-110 transition-all shrink-0">
                                <LobeGithub size={16} />
                            </span>
                        </div>

                        {/* 核心标语（独立一行，排版整洁，中英文均不截断） */}
                        <p className="text-[9.5px] text-emerald-800 dark:text-emerald-300 font-medium leading-tight mb-2">
                            {t('proxy.cli_sync.jeikcode_tagline', { defaultValue: '核心维护者深度打造 · 原生网关最佳兼容' })}
                        </p>

                        {/* 关键词高亮标签群（精炼短语，自适应折行，丰满耐看） */}
                        <div className="flex flex-wrap items-center gap-1.5">
                            <span className="inline-flex items-center gap-1 text-[9px] font-bold px-2 py-0.5 rounded-md bg-emerald-500/15 text-emerald-700 dark:text-emerald-300 border border-emerald-500/30 shadow-2xs whitespace-nowrap">
                                ⚡ {t('proxy.cli_sync.jeikcode_tag_cache', { defaultValue: '95%+ 恐怖缓存' })}
                            </span>
                            <span className="inline-flex items-center gap-1 text-[9px] font-bold px-2 py-0.5 rounded-md bg-blue-500/15 text-blue-700 dark:text-blue-300 border border-blue-500/30 shadow-2xs whitespace-nowrap">
                                🗺️ {t('proxy.cli_sync.jeikcode_tag_codegraph', { defaultValue: '代码图谱' })}
                            </span>
                            <span className="inline-flex items-center gap-1 text-[9px] font-bold px-2 py-0.5 rounded-md bg-purple-500/15 text-purple-700 dark:text-purple-300 border border-purple-500/30 shadow-2xs whitespace-nowrap">
                                🚀 {t('proxy.cli_sync.jeikcode_tag_surpass', { defaultValue: '超越 CC / Codex' })}
                            </span>
                            <span className="inline-flex items-center gap-1 text-[9px] font-bold px-2 py-0.5 rounded-md bg-amber-500/15 text-amber-700 dark:text-amber-300 border border-amber-500/30 shadow-2xs whitespace-nowrap">
                                🛠️ {t('proxy.cli_sync.jeikcode_tag_custom', { defaultValue: '自由定制' })}
                            </span>
                        </div>
                    </div>
                )}

                <div className="mt-auto space-y-3">
                    <div className="p-2.5 bg-gray-50/80 dark:bg-gray-900/40 rounded-lg border border-dashed border-gray-200 dark:border-white/10">
                        <div className="flex justify-between items-start mb-1">
                            <div className="text-[9px] text-gray-400 dark:text-gray-500 uppercase font-bold tracking-wider">{t('proxy.cli_sync.status.current_base_url')}</div>
                        </div>
                        <div className="text-[10px] font-mono truncate text-gray-500 dark:text-gray-400 italic">
                            {status?.current_base_url || '---'}
                        </div>
                    </div>

                    {/* Claude, Codex, Gemini, JeikCode, GrokBuild 的模型选择 */}
                    {(status?.installed || app === 'OpenCode' || app === 'JeikCode' || app === 'GrokBuild') && (app === 'Claude' || app === 'Codex' || app === 'Gemini' || app === 'JeikCode' || app === 'GrokBuild') && (
                        <div className="space-y-1">
                            <div className="text-[9px] text-gray-400 dark:text-gray-500 uppercase font-bold tracking-wider px-1">
                                {t('proxy.cli_sync.model_select', { defaultValue: 'Select Model' })}
                            </div>
                            <GroupedSelect
                                value={selectedModels[app]}
                                onChange={(val) => setSelectedModels(prev => ({ ...prev, [app]: val }))}
                                options={modelOptions}
                                className="w-full !h-8 !text-[11px] !rounded-lg"
                                allowCustomInput={true}
                            />
                        </div>
                    )}

                    {/* OpenCode 独有的账号同步选项 - Allow even if not installed */}
                    {app === 'OpenCode' && (
                        <div className="flex items-center gap-2 p-2 bg-gray-50/50 dark:bg-gray-900/20 rounded-lg">
                            <input
                                type="checkbox"
                                id="opencode-sync-accounts"
                                checked={syncAccounts}
                                onChange={(e) => setSyncAccounts(e.target.checked)}
                                className="checkbox checkbox-xs checkbox-primary"
                            />
                            <label htmlFor="opencode-sync-accounts" className="text-[10px] text-gray-600 dark:text-gray-400 cursor-pointer select-none">
                                {t('proxy.opencode_sync.sync_accounts', { defaultValue: 'Sync accounts to antigravity-accounts.json' })}
                            </label>
                        </div>
                    )}

                    <div className="flex items-center gap-2">
                        {(status?.installed || app === 'OpenCode' || app === 'JeikCode' || app === 'GrokBuild' || app === 'Hermes' || app === 'OpenClaw') && (
                            <>
                                {/* 对于 OpenCode、Hermes 与 OpenClaw，如果未同步，则不显示查看按钮（因为文件尚未生成，后端会报错） */}
                                {((app !== 'OpenCode' && app !== 'Hermes' && app !== 'OpenClaw') || status?.is_synced) && (
                                    <button
                                        onClick={() => handleViewConfig(app)}
                                        className="p-1 text-gray-400 hover:text-blue-500 hover:bg-blue-50 dark:hover:bg-blue-900/20 rounded transition-colors"
                                        title={t(app === 'OpenCode' ? 'proxy.opencode_sync.btn_view' : app === 'Hermes' ? 'proxy.hermes_sync.btn_view' : app === 'OpenClaw' ? 'proxy.openclaw_sync.btn_view' : 'proxy.cli_sync.btn_view', { defaultValue: 'View Config' })}
                                    >
                                        <Eye size={14} />
                                    </button>
                                )}
                                <button
                                    onClick={() => handleRestore(app)}
                                    className="p-1 text-gray-400 hover:text-orange-500 hover:bg-orange-50 dark:hover:bg-orange-900/20 rounded transition-colors"
                                    title={t(app === 'OpenCode' ? 'proxy.opencode_sync.btn_restore' : app === 'Hermes' ? 'proxy.hermes_sync.btn_restore' : app === 'OpenClaw' ? 'proxy.openclaw_sync.btn_restore' : 'proxy.cli_sync.btn_restore', { defaultValue: 'Restore' })}
                                >
                                    <RotateCcw size={14} />
                                </button>
                                {/* OpenCode、Hermes 与 OpenClaw 的 Clear 按钮 */}
                                {(app === 'OpenCode' || app === 'Hermes' || app === 'OpenClaw') && (
                                    <button
                                        onClick={() => handleClear(app)}
                                        className="p-1 text-gray-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 rounded transition-colors"
                                        title={t(app === 'Hermes' ? 'proxy.hermes_sync.btn_clear' : app === 'OpenClaw' ? 'proxy.openclaw_sync.btn_clear' : 'proxy.opencode_sync.btn_clear', { defaultValue: 'Clear' })}
                                    >
                                        <Trash2 size={14} />
                                    </button>
                                )}
                            </>
                        )}
                        <button
                            onClick={() => handleSync(app)}
                            disabled={(app !== 'OpenCode' && app !== 'JeikCode' && app !== 'GrokBuild' && app !== 'Hermes' && app !== 'OpenClaw' && !status?.installed) || isAppSyncing || isAppLoading}
                            className={cn(
                                "btn btn-sm flex-1 gap-2 rounded-xl transition-all font-bold shadow-sm",
                                status?.is_synced
                                    ? "btn-ghost border-gray-200 dark:border-base-400 text-gray-500 hover:bg-gray-100"
                                    : "btn-primary hover:shadow-lg shadow-blue-500/20"
                            )}
                        >
                            {isAppSyncing ? (
                                <Loader2 size={14} className="animate-spin" />
                            ) : (
                                <RefreshCw size={14} className={cn(isAppLoading && "animate-spin-once")} />
                            )}
                            {t('proxy.cli_sync.btn_sync')}
                        </button>
                    </div>
                </div>
            </div>
        );
    };

    return (
        <div className={cn("space-y-4", className)}>
            <div className="px-1 flex items-center justify-between">
                <div className="flex items-center gap-2 text-gray-400">
                    <Terminal size={14} />
                    <span className="text-[10px] font-bold uppercase tracking-widest">
                        {t('proxy.cli_sync.title')}
                    </span>
                </div>
                <p className="text-[10px] text-gray-400 dark:text-gray-500 italic">
                    {t('proxy.cli_sync.subtitle')}
                </p>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
                {renderCliItem('JeikCode', <JeikCodeIcon size="100%" className="w-full h-full" />, 'JeikCode')}
                {renderCliItem('Claude', (
                    <div className="w-full h-full bg-[#D97757] flex items-center justify-center text-white">
                        <Claude size={22} className="text-white" />
                    </div>
                ), 'Claude Code')}
                {renderCliItem('Codex', (
                    <div className="w-full h-full bg-[#000000] dark:bg-[#121214] flex items-center justify-center text-white">
                        <OpenAI size={22} className="text-white" />
                    </div>
                ), 'Codex AI')}
                {renderCliItem('OpenCode', (
                    <div className="w-full h-full bg-[#000000] dark:bg-[#121214] flex items-center justify-center text-white">
                        <OpenCode size={20} className="text-white" />
                    </div>
                ), 'OpenCode')}
                {renderCliItem('GrokBuild', (
                    <div className="w-full h-full bg-[#000000] dark:bg-[#121214] flex items-center justify-center text-white">
                        <Grok size={22} className="text-white" />
                    </div>
                ), 'Grok Build')}
                {renderCliItem('Hermes', <HermesAgent.Avatar size={40} shape="square" style={{ width: '100%', height: '100%', borderRadius: 0 }} />, 'Hermes Agent')}
                {renderCliItem('OpenClaw', (
                    <div className="w-full h-full bg-[#18181b] dark:bg-[#121214] flex items-center justify-center text-white">
                        <OpenClaw.Color size={26} />
                    </div>
                ), 'OpenClaw')}
                {renderCliItem('Gemini', (
                    <div className="w-full h-full bg-[#18181b] dark:bg-[#121214] flex items-center justify-center">
                        <Gemini.Color size={24} />
                    </div>
                ), 'Gemini CLI')}
                {renderCliItem('Droid', (
                    <div className="w-full h-full bg-gradient-to-br from-orange-500 via-amber-500 to-orange-600 flex items-center justify-center text-white">
                        <Bot size={22} className="text-white drop-shadow-sm" />
                    </div>
                ), 'Droid')}
            </div>

            {/* Config Viewer Modal */}
            {viewingConfig && (
                <div className="fixed inset-0 z-[300] flex items-center justify-center p-4 bg-black/50 backdrop-blur-sm animate-in fade-in duration-200">
                    <div className="bg-white dark:bg-base-100 rounded-2xl shadow-2xl border border-gray-200 dark:border-base-300 w-full max-w-2xl overflow-hidden animate-in zoom-in-95 duration-200">
                        <div className="px-6 py-4 border-b border-gray-100 dark:border-base-200 flex items-center justify-between bg-gray-50/50 dark:bg-base-200/50">
                            <div>
                                <h3 className="font-bold text-gray-900 dark:text-base-content flex items-center gap-2">
                                    <CodeXml size={18} className="text-blue-500" />
                                    {t('proxy.cli_sync.modal.view_title', { name: viewingConfig.app })}
                                </h3>
                                <div className="mt-2 flex gap-2">
                                    {viewingConfig.allFiles.map(file => (
                                        <button
                                            key={file}
                                            onClick={() => handleViewConfig(viewingConfig.app, file)}
                                            className={cn(
                                                "px-3 py-1 text-[10px] font-bold rounded-lg transition-all border",
                                                viewingConfig.fileName === file
                                                    ? "bg-blue-500 text-white border-blue-500"
                                                    : "bg-white dark:bg-base-300 text-gray-400 border-gray-100 dark:border-base-400 hover:border-blue-200"
                                            )}
                                        >
                                            {file}
                                        </button>
                                    ))}
                                </div>
                            </div>
                            <div className="flex items-center gap-2">
                                <button
                                    onClick={async () => {
                                        const success = await copyToClipboard(viewingConfig.content);
                                        if (success) {
                                            showToast(t('proxy.cli_sync.modal.copy_success'), 'success');
                                        }
                                    }}
                                    className="btn btn-ghost btn-sm hover:bg-blue-50 hover:text-blue-600 dark:hover:bg-blue-900/20"
                                >
                                    <Copy size={16} />
                                </button>
                                <button
                                    onClick={() => setViewingConfig(null)}
                                    className="btn btn-ghost btn-sm hover:bg-red-50 hover:text-red-600 dark:hover:bg-red-900/20"
                                >
                                    <X size={18} />
                                </button>
                            </div>
                        </div>
                        <div className="p-6">
                            <div className="bg-gray-900 rounded-xl p-4 overflow-auto max-h-[50vh] border border-gray-800 shadow-inner">
                                <pre className="text-xs font-mono text-gray-300 leading-relaxed">
                                    {viewingConfig.content}
                                </pre>
                            </div>
                        </div>
                    </div>
                </div>
            )}
            {/* 恢复默认/备份确认弹窗 */}
            <ModalDialog
                isOpen={!!restoreConfirmApp}
                title={statuses[restoreConfirmApp!]?.has_backup
                    ? t('proxy.cli_sync.btn_restore_backup')
                    : t('proxy.cli_sync.btn_restore') || t('proxy.cli_sync.title')}
                message={restoreConfirmApp
                    ? (statuses[restoreConfirmApp!]?.has_backup
                        ? t('proxy.cli_sync.restore_backup_confirm')
                        : t('proxy.cli_sync.restore_confirm', { name: restoreConfirmApp }))
                    : ''}
                onConfirm={executeRestore}
                onCancel={() => setRestoreConfirmApp(null)}
                isDestructive={true}
            />

            {/* 同步配置确认弹窗 (Issue #756) */}
            <ModalDialog
                isOpen={!!syncConfirmApp}
                title={t('proxy.cli_sync.sync_confirm_title')}
                message={syncConfirmApp ? t('proxy.cli_sync.sync_confirm_message', { name: syncConfirmApp }) : ''}
                onConfirm={executeSync}
                onCancel={() => setSyncConfirmApp(null)}
                isDestructive={true}
            />

            {/* Clear 确认弹窗 - OpenCode / Hermes / OpenClaw */}
            <ModalDialog
                isOpen={!!clearConfirmApp}
                title={clearConfirmApp === 'Hermes'
                    ? t('proxy.hermes_sync.clear_confirm_title', { defaultValue: 'Clear Hermes Configuration' })
                    : clearConfirmApp === 'OpenClaw'
                    ? t('proxy.openclaw_sync.clear_confirm_title', { defaultValue: 'Clear OpenClaw Configuration' })
                    : t('proxy.opencode_sync.clear_confirm_title', { defaultValue: 'Clear OpenCode Configuration' })}
                message={clearConfirmApp === 'Hermes'
                    ? t('proxy.hermes_sync.clear_confirm_message', { defaultValue: 'This will remove the Antigravity Manager provider from Hermes. Are you sure?' })
                    : clearConfirmApp === 'OpenClaw'
                    ? t('proxy.openclaw_sync.clear_confirm_message', { defaultValue: 'This will remove the Antigravity Manager provider from OpenClaw. Are you sure?' })
                    : t('proxy.opencode_sync.clear_confirm_message', { defaultValue: 'This will clear all OpenCode configurations including legacy settings. Are you sure?' })}
                onConfirm={executeClear}
                onCancel={() => setClearConfirmApp(null)}
                isDestructive={true}
            />

            {/* OpenClaw 配置与双版本同步弹窗 */}
            {openClawSyncModal && (
                <OpenClawSyncModal
                    apiKey={apiKey}
                    getFormattedProxyUrl={getFormattedProxyUrl}
                    onClose={() => setOpenClawSyncModal(false)}
                    onSyncDone={() => checkStatus('OpenClaw')}
                />
            )}

            {/* Hermes 配置与模型选择弹窗 */}
            {hermesSyncModal && (
                <HermesSyncModal
                    apiKey={apiKey}
                    getFormattedProxyUrl={getFormattedProxyUrl}
                    onClose={() => setHermesSyncModal(false)}
                    onSyncDone={() => checkStatus('Hermes')}
                />
            )}

            {/* Droid 模型添加弹窗 */}
            {droidSyncModal && (
                <DroidSyncModal
                    proxyUrl={proxyUrl}
                    apiKey={apiKey}
                    getFormattedProxyUrl={getFormattedProxyUrl}
                    onClose={() => setDroidSyncModal(false)}
                    onSyncDone={() => checkStatus('Droid')}
                />
            )}

            {/* OpenCode 模型选择弹窗 */}
            {openCodeSyncModal && (
                <OpenCodeSyncModal
                    proxyUrl={proxyUrl}
                    apiKey={apiKey}
                    getFormattedProxyUrl={getFormattedProxyUrl}
                    syncAccounts={syncAccounts}
                    onClose={() => setOpenCodeSyncModal(false)}
                    onSyncDone={() => checkStatus('OpenCode')}
                />
            )}
        </div>
    );
};


export default CliSyncCard;
