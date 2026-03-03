import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import {
    Terminal,
    CheckCircle2,
    AlertCircle,
    RefreshCw,
    Cpu,
    Globe,
    CodeXml,
    Loader2,
    Eye,
    RotateCcw,
    Copy,
    X,
    Bot,
    Trash2
} from 'lucide-react';
import { copyToClipboard } from '../../utils/clipboard';
import { request as invoke } from '../../utils/request';
import { isTauri } from '../../utils/env';
import { showToast } from '../common/ToastContainer';
import ModalDialog from '../common/ModalDialog';
import { cn } from '../../utils/cn';
import { DroidSyncModal } from './DroidSyncModal';
import { OpenCodeSyncModal } from './OpenCodeSyncModal';
import { useProxyModels } from '../../hooks/useProxyModels';
import GroupedSelect from '../common/GroupedSelect';

interface CliSyncCardProps {
    proxyUrl: string;
    apiKey: string;
    className?: string;
}

type CliAppType = 'Claude' | 'Codex' | 'Gemini' | 'OpenCode' | 'Droid';

interface CliStatus {
    installed: boolean;
    version: string | null;
    is_synced: boolean;
    has_backup: boolean;
    current_base_url: string | null;
    files: string[];
    synced_count?: number;
}

interface ExportStatus {
    exported: boolean;
    export_dir: string;
    files: { name: string; export_path: string; target_path: string }[];
}

export const CliSyncCard = ({ proxyUrl, apiKey, className }: CliSyncCardProps) => {
    const { t } = useTranslation();
    const isWebMode = !isTauri();
    const [statuses, setStatuses] = useState<Record<CliAppType, CliStatus | null>>({
        Claude: null,
        Codex: null,
        Gemini: null,
        OpenCode: null,
        Droid: null
    });
    const [loading, setLoading] = useState<Record<CliAppType, boolean>>({
        Claude: false,
        Codex: false,
        Gemini: false,
        OpenCode: false,
        Droid: false
    });
    const [syncing, setSyncing] = useState<Record<CliAppType, boolean>>({
        Claude: false,
        Codex: false,
        Gemini: false,
        OpenCode: false,
        Droid: false
    });
    const [syncAccounts, setSyncAccounts] = useState(false);
    const [droidSyncModal, setDroidSyncModal] = useState(false);
    const [selectedModels, setSelectedModels] = useState<Record<CliAppType, string>>(() => {
        try {
            const saved = localStorage.getItem('antigravity_cli_sync_models');
            if (saved) return { Claude: '', Codex: 'gpt-4o', Gemini: 'gemini-1.5-pro', OpenCode: '', Droid: '', ...JSON.parse(saved) };
        } catch { /* ignore */ }
        return { Claude: '', Codex: 'gpt-4o', Gemini: 'gemini-1.5-pro', OpenCode: '', Droid: '' };
    });
    const [claudeModels, setClaudeModels] = useState(() => {
        try {
            const saved = localStorage.getItem('antigravity_cli_sync_claude_tiers');
            if (saved) return { opus: 'claude-opus-4-6', sonnet: 'claude-sonnet-4-6', haiku: 'claude-haiku-4-5-20251001', ...JSON.parse(saved) };
        } catch { /* ignore */ }
        return { opus: 'claude-opus-4-6', sonnet: 'claude-sonnet-4-6', haiku: 'claude-haiku-4-5-20251001' };
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
    const [exportStatuses, setExportStatuses] = useState<Record<CliAppType, ExportStatus | null>>({
        Claude: null, Codex: null, Gemini: null, OpenCode: null, Droid: null
    });

    useEffect(() => {
        localStorage.setItem('antigravity_cli_sync_models', JSON.stringify(selectedModels));
    }, [selectedModels]);

    useEffect(() => {
        localStorage.setItem('antigravity_cli_sync_claude_tiers', JSON.stringify(claudeModels));
    }, [claudeModels]);

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
        // Codex & OpenCode (OpenAI 协议) 通常需要带 /v1
        if (app === 'Codex' || app === 'OpenCode') {
            return base.endsWith('/v1') ? base : `${base}/v1`;
        }
        // Claude 和 Gemini 的 SDK 通常会自动处理版本路径或不需要 /v1
        return base.replace(/\/v1$/, '');
    }, [proxyUrl]);

    const checkStatus = useCallback(async (app: CliAppType) => {
        setLoading(prev => ({ ...prev, [app]: true }));
        try {
            if (isWebMode) {
                // Web/Docker mode: check export status instead of CLI installation
                let command: string;
                let params: Record<string, unknown>;
                if (app === 'Droid') {
                    command = 'get_droid_export_status';
                    params = {};
                } else if (app === 'OpenCode') {
                    command = 'get_opencode_export_status';
                    params = {};
                } else {
                    command = 'get_cli_export_status';
                    params = { appType: app };
                }
                const exportStatus = await invoke<ExportStatus>(command, params);
                setExportStatuses(prev => ({ ...prev, [app]: exportStatus }));
                // Set a synthetic CliStatus so existing UI logic works
                setStatuses(prev => ({
                    ...prev,
                    [app]: {
                        installed: true, // Always "available" in web mode
                        version: null,
                        is_synced: exportStatus.exported,
                        has_backup: false,
                        current_base_url: exportStatus.exported ? exportStatus.export_dir : null,
                        files: exportStatus.files.map(f => f.name),
                    }
                }));
            } else {
                // Desktop/Tauri mode: original behavior
                const formattedUrl = getFormattedProxyUrl(app);
                let command: string;
                let params: Record<string, unknown>;
                if (app === 'Droid') {
                    command = 'get_droid_sync_status';
                    params = { proxyUrl: formattedUrl };
                } else if (app === 'OpenCode') {
                    command = 'get_opencode_sync_status';
                    params = { proxyUrl: formattedUrl };
                } else {
                    command = 'get_cli_sync_status';
                    params = { appType: app, proxyUrl: formattedUrl };
                }

                const status = await invoke<CliStatus>(command, params);
                setStatuses(prev => ({ ...prev, [app]: status }));
            }
        } catch (error) {
            console.error(`Failed to check ${app} status:`, error);
        } finally {
            setLoading(prev => ({ ...prev, [app]: false }));
        }
    }, [getFormattedProxyUrl, isWebMode]);

    const handleSync = async (app: CliAppType) => {
        if (isWebMode) {
            // Web mode: export directly without modals
            setSyncConfirmApp(app);
            return;
        }
        if (app === 'Droid') {
            setDroidSyncModal(true);
            return;
        }
        if (app === 'OpenCode') {
            setOpenCodeSyncModal(true);
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

        setSyncing(prev => ({ ...prev, [app]: true }));
        try {
            const formattedUrl = getFormattedProxyUrl(app);

            if (isWebMode) {
                // Web/Docker mode: export configs to one-click-sync directory
                let command: string;
                let params: Record<string, unknown>;
                if (app === 'Droid') {
                    command = 'export_droid_config';
                    params = { proxyUrl: formattedUrl, apiKey };
                } else if (app === 'OpenCode') {
                    command = 'export_opencode_config';
                    params = { proxyUrl: formattedUrl, apiKey };
                } else {
                    command = 'export_cli_config';
                    params = app === 'Claude'
                        ? { appType: app, proxyUrl: formattedUrl, apiKey, claudeModels }
                        : { appType: app, proxyUrl: formattedUrl, apiKey, model: selectedModels[app] };
                }

                const result = await invoke<{ export_dir: string; files: { name: string }[] }>(command, params);
                showToast(
                    t('proxy.cli_sync.toast.export_success', {
                        name: app,
                        path: result.export_dir,
                        defaultValue: `${app} configs exported to ${result.export_dir}`
                    }),
                    'success'
                );
                await checkStatus(app);
            } else {
                // Desktop/Tauri mode: original sync behavior
                const command = app === 'OpenCode' ? 'execute_opencode_sync' : 'execute_cli_sync';
                const params = app === 'OpenCode'
                    ? { proxyUrl: formattedUrl, apiKey: apiKey, syncAccounts: syncAccounts }
                    : app === 'Claude'
                        ? { appType: app, proxyUrl: formattedUrl, apiKey: apiKey, claudeModels }
                        : { appType: app, proxyUrl: formattedUrl, apiKey: apiKey, model: selectedModels[app] };

                await invoke(command, params);
                showToast(t(app === 'OpenCode' ? 'proxy.opencode_sync.toast.sync_success' : 'proxy.cli_sync.toast.sync_success', { name: app, defaultValue: `${app} synced successfully` }), 'success');
                await checkStatus(app);
            }
        } catch (error: any) {
            const msgKey = isWebMode ? 'proxy.cli_sync.toast.export_error' : (app === 'OpenCode' ? 'proxy.opencode_sync.toast.sync_error' : 'proxy.cli_sync.toast.sync_error');
            showToast(t(msgKey, { name: app, error: error.toString(), defaultValue: `${isWebMode ? 'Export' : 'Sync'} failed: ${error.toString()}` }), 'error');
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
            const command = app === 'Droid' ? 'execute_droid_restore' : app === 'OpenCode' ? 'execute_opencode_restore' : 'execute_cli_restore';
            const params = (app === 'Droid' || app === 'OpenCode') ? {} : { appType: app };
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
            const formattedUrl = getFormattedProxyUrl(app);
            await invoke('execute_opencode_clear', { proxyUrl: formattedUrl, clearLegacy: true });
            showToast(t('proxy.opencode_sync.toast.clear_success', { defaultValue: 'OpenCode cleared successfully' }), 'success');
            await checkStatus(app);
        } catch (error: any) {
            showToast(t('proxy.opencode_sync.toast.clear_error', { defaultValue: `Clear failed: ${error.toString()}` }), 'error');
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
        checkStatus('Gemini');
        checkStatus('OpenCode');
        checkStatus('Droid');
    }, [checkStatus]);

    const renderCliItem = (app: CliAppType, icon: React.ReactNode, name: string) => {
        const status = statuses[app];
        const isAppLoading = loading[app];
        const isAppSyncing = syncing[app];

        return (
            <div className="flex flex-col bg-white/50 dark:bg-gray-800/40 rounded-xl border border-gray-100 dark:border-white/5 p-4 shadow-sm hover:shadow-lg hover:border-blue-200/50 dark:hover:border-blue-500/30 transition-all duration-300 group">
                <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-y-3 gap-x-2 mb-4">
                    <div className="flex items-center gap-3 min-w-0">
                        <div className="p-2.5 bg-gray-50 dark:bg-base-300 rounded-lg shrink-0 group-hover:scale-110 transition-transform duration-300">
                            {icon}
                        </div>
                        <div className="min-w-0">
                            <h4 className="text-sm font-bold text-gray-900 dark:text-gray-100 leading-tight truncate">
                                {t('proxy.cli_sync.card_title', { name })}
                            </h4>
                            <div className="mt-1 flex items-center gap-1.5 overflow-hidden">
                                {isAppLoading ? (
                                    <div className="flex items-center gap-1 text-[10px] text-gray-400">
                                        <Loader2 size={10} className="animate-spin" />
                                        {t('proxy.cli_sync.status.detecting')}
                                    </div>
                                ) : isWebMode ? (
                                    <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-indigo-50 dark:bg-indigo-900/20 text-indigo-600 dark:text-indigo-400 font-bold whitespace-nowrap">
                                        {t('proxy.cli_sync.status.export_mode', { defaultValue: 'Export Mode' })}
                                    </span>
                                ) : status?.installed ? (
                                    <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-blue-50 dark:bg-blue-900/20 text-blue-600 dark:text-blue-400 font-bold whitespace-nowrap">
                                        {t('proxy.cli_sync.status.installed', { version: status.version })}
                                    </span>
                                ) : (
                                    <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-gray-100 dark:bg-gray-800 text-gray-400 font-medium whitespace-nowrap">
                                        {t('proxy.cli_sync.status.not_installed')}
                                    </span>
                                )}
                            </div>
                        </div>
                    </div>

                    {/* Show Sync/Export Status */}
                    {!isAppLoading && (isWebMode || status?.installed || app === 'OpenCode' && status?.current_base_url) && (
                        <div className={cn(
                            "inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-[10px] font-bold tracking-wide transition-all h-6 shrink-0 whitespace-nowrap shadow-sm",
                            status?.is_synced
                                ? "bg-gradient-to-r from-green-500 to-emerald-600 text-white"
                                : "bg-amber-100 dark:bg-amber-900/30 text-amber-600 dark:text-amber-500 border border-amber-200/50 dark:border-amber-800/30"
                        )}>
                            {status?.is_synced ? (
                                <><CheckCircle2 size={12} className="shrink-0" /> {isWebMode
                                    ? t('proxy.cli_sync.status.exported', { defaultValue: 'Exported' })
                                    : t('proxy.cli_sync.status.synced', { defaultValue: '已同步' })
                                }</>
                            ) : (
                                <><AlertCircle size={12} className="shrink-0" /> {isWebMode
                                    ? t('proxy.cli_sync.status.not_exported', { defaultValue: 'Not Exported' })
                                    : t('proxy.cli_sync.status.not_synced', { defaultValue: '未同步' })
                                }</>
                            )}
                        </div>
                    )}
                </div>

                <div className="mt-auto space-y-3">
                    <div className="p-2.5 bg-gray-50/80 dark:bg-gray-900/40 rounded-lg border border-dashed border-gray-200 dark:border-white/10">
                        <div className="flex justify-between items-start mb-1">
                            <div className="text-[9px] text-gray-400 dark:text-gray-500 uppercase font-bold tracking-wider">
                                {isWebMode
                                    ? t('proxy.cli_sync.status.export_dir', { defaultValue: 'Export Directory' })
                                    : t('proxy.cli_sync.status.current_base_url')
                                }
                            </div>
                        </div>
                        <div className="text-[10px] font-mono truncate text-gray-500 dark:text-gray-400 italic">
                            {isWebMode
                                ? (exportStatuses[app]?.export_dir || t('proxy.cli_sync.status.not_exported_yet', { defaultValue: 'Not exported yet' }))
                                : (status?.current_base_url || '---')
                            }
                        </div>
                    </div>

                    {/* Claude: 3-tier model selection (opus/sonnet/haiku) */}
                    {(status?.installed || isWebMode) && app === 'Claude' && (
                        <div className="space-y-1.5">
                            <div className="text-[9px] text-gray-400 dark:text-gray-500 uppercase font-bold tracking-wider px-1">
                                {t('proxy.cli_sync.model_tiers', { defaultValue: 'Model Tiers' })}
                            </div>
                            {(['opus', 'sonnet', 'haiku'] as const).map((tier) => (
                                <div key={tier} className="flex items-center gap-2">
                                    <span className="text-[9px] font-bold text-gray-400 dark:text-gray-500 w-12 text-right uppercase">{tier}</span>
                                    <GroupedSelect
                                        value={claudeModels[tier]}
                                        onChange={(val) => setClaudeModels((prev: typeof claudeModels) => ({ ...prev, [tier]: val }))}
                                        options={modelOptions}
                                        className="flex-1 !h-7 !text-[10px] !rounded-lg"
                                        allowCustomInput={true}
                                    />
                                </div>
                            ))}
                        </div>
                    )}

                    {/* Codex, Gemini: single model selection */}
                    {(status?.installed || isWebMode) && (app === 'Codex' || app === 'Gemini') && (
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

                    {/* Web mode: export usage instructions */}
                    {isWebMode && exportStatuses[app]?.exported && (
                        <div className="p-2 bg-blue-50/80 dark:bg-blue-900/20 rounded-lg border border-blue-200/50 dark:border-blue-800/30">
                            <div className="text-[9px] text-blue-600 dark:text-blue-400 leading-relaxed">
                                {app === 'Claude' && t('proxy.cli_sync.export_hint_claude', {
                                    defaultValue: 'Merge the exported env section into your existing ~/.claude/settings.json'
                                })}
                                {app === 'Codex' && t('proxy.cli_sync.export_hint_codex', {
                                    defaultValue: 'Copy exported files to ~/.codex/'
                                })}
                                {app === 'Gemini' && t('proxy.cli_sync.export_hint_gemini', {
                                    defaultValue: 'Copy exported settings.json to ~/.gemini/'
                                })}
                                {app === 'OpenCode' && t('proxy.cli_sync.export_hint_opencode', {
                                    defaultValue: 'Copy exported config.json to ~/.opencode/'
                                })}
                                {app === 'Droid' && t('proxy.cli_sync.export_hint_droid', {
                                    defaultValue: 'Copy exported settings.json to ~/.factory/'
                                })}
                            </div>
                        </div>
                    )}

                    {/* OpenCode 独有的账号同步选项 - Allow even if not installed, hide in web mode */}
                    {app === 'OpenCode' && !isWebMode && (
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
                        {/* View/Restore/Clear buttons — desktop mode only */}
                        {!isWebMode && (status?.installed || app === 'OpenCode') && (
                            <>
                                {(app !== 'OpenCode' || status?.is_synced) && (
                                    <button
                                        onClick={() => handleViewConfig(app)}
                                        className="p-1 text-gray-400 hover:text-blue-500 hover:bg-blue-50 dark:hover:bg-blue-900/20 rounded transition-colors"
                                        title={t(app === 'OpenCode' ? 'proxy.opencode_sync.btn_view' : 'proxy.cli_sync.btn_view', { defaultValue: 'View Config' })}
                                    >
                                        <Eye size={14} />
                                    </button>
                                )}
                                <button
                                    onClick={() => handleRestore(app)}
                                    className="p-1 text-gray-400 hover:text-orange-500 hover:bg-orange-50 dark:hover:bg-orange-900/20 rounded transition-colors"
                                    title={t(app === 'OpenCode' ? 'proxy.opencode_sync.btn_restore' : 'proxy.cli_sync.btn_restore', { defaultValue: 'Restore' })}
                                >
                                    <RotateCcw size={14} />
                                </button>
                                {app === 'OpenCode' && (
                                    <button
                                        onClick={() => handleClear(app)}
                                        className="p-1 text-gray-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 rounded transition-colors"
                                        title={t('proxy.opencode_sync.btn_clear', { defaultValue: 'Clear' })}
                                    >
                                        <Trash2 size={14} />
                                    </button>
                                )}
                            </>
                        )}
                        <button
                            onClick={() => handleSync(app)}
                            disabled={(!isWebMode && app !== 'OpenCode' && !status?.installed) || isAppSyncing || isAppLoading}
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
                            {isWebMode
                                ? t('proxy.cli_sync.btn_export', { defaultValue: 'Export' })
                                : t('proxy.cli_sync.btn_sync')
                            }
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
                    {isWebMode
                        ? t('proxy.cli_sync.subtitle_web', { defaultValue: 'Generate drop-in configs for your CLI tools' })
                        : t('proxy.cli_sync.subtitle')
                    }
                </p>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
                {renderCliItem('Claude', <CodeXml size={20} className="text-purple-500" />, 'Claude Code')}
                {renderCliItem('Codex', <Cpu size={20} className="text-blue-500" />, 'Codex AI')}
                {renderCliItem('Gemini', <Globe size={20} className="text-green-500" />, 'Gemini CLI')}
                {renderCliItem('OpenCode', <CodeXml size={20} className="text-blue-500" />, 'OpenCode')}
                {renderCliItem('Droid', <Bot size={20} className="text-orange-500" />, 'Droid')}
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

            {/* Sync/Export confirm dialog */}
            <ModalDialog
                isOpen={!!syncConfirmApp}
                title={isWebMode
                    ? t('proxy.cli_sync.export_confirm_title', { defaultValue: 'Export Configuration' })
                    : t('proxy.cli_sync.sync_confirm_title')
                }
                message={syncConfirmApp
                    ? (isWebMode
                        ? t('proxy.cli_sync.export_confirm_message', {
                            name: syncConfirmApp,
                            defaultValue: `Export ${syncConfirmApp} config files to the one-click-sync directory?`
                        })
                        : t('proxy.cli_sync.sync_confirm_message', { name: syncConfirmApp }))
                    : ''
                }
                onConfirm={executeSync}
                onCancel={() => setSyncConfirmApp(null)}
                isDestructive={!isWebMode}
            />

            {/* Clear 确认弹窗 - 仅 OpenCode */}
            <ModalDialog
                isOpen={!!clearConfirmApp}
                title={t('proxy.opencode_sync.clear_confirm_title', { defaultValue: 'Clear OpenCode Configuration' })}
                message={t('proxy.opencode_sync.clear_confirm_message', { defaultValue: 'This will clear all OpenCode configurations including legacy settings. Are you sure?' })}
                onConfirm={executeClear}
                onCancel={() => setClearConfirmApp(null)}
                isDestructive={true}
            />

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
                    onClose={() => setOpenCodeSyncModal(false)}
                    onSyncDone={() => checkStatus('OpenCode')}
                />
            )}
        </div>
    );
};


export default CliSyncCard;
