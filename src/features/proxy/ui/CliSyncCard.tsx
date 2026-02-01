// File: src/features/proxy/ui/CliSyncCard.tsx
// CLI Sync Card for syncing proxy settings to CLI tools

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
    X
} from 'lucide-react';
import { copyToClipboard } from '@/shared/lib';
import { invoke } from '@/shared/api';
import { showToast, ModalDialog } from '@/shared/ui';
import { cn } from '@/shared/lib';

interface CliSyncCardProps {
    proxyUrl: string;
    apiKey: string;
    className?: string;
}

type CliAppType = 'Claude' | 'Codex' | 'Gemini';

interface CliStatus {
    installed: boolean;
    version: string | null;
    is_synced: boolean;
    has_backup: boolean;
    current_base_url: string | null;
    files: string[];
}

export const CliSyncCard = ({ proxyUrl, apiKey, className }: CliSyncCardProps) => {
    const { t } = useTranslation();
    const [statuses, setStatuses] = useState<Record<CliAppType, CliStatus | null>>({
        Claude: null,
        Codex: null,
        Gemini: null
    });
    const [loading, setLoading] = useState<Record<CliAppType, boolean>>({
        Claude: false,
        Codex: false,
        Gemini: false
    });
    const [syncing, setSyncing] = useState<Record<CliAppType, boolean>>({
        Claude: false,
        Codex: false,
        Gemini: false
    });
    const [viewingConfig, setViewingConfig] = useState<{
        app: CliAppType,
        content: string,
        fileName: string,
        allFiles: string[]
    } | null>(null);
    const [restoreConfirmApp, setRestoreConfirmApp] = useState<CliAppType | null>(null);
    const [syncConfirmApp, setSyncConfirmApp] = useState<CliAppType | null>(null);

    const getFormattedProxyUrl = useCallback((app: CliAppType) => {
        if (!proxyUrl) return '';
        const base = proxyUrl.trimEnd().replace(/\/+$/, '');
        if (app === 'Codex') {
            return base.endsWith('/v1') ? base : `${base}/v1`;
        }
        return base.replace(/\/v1$/, '');
    }, [proxyUrl]);

    const checkStatus = useCallback(async (app: CliAppType) => {
        setLoading(prev => ({ ...prev, [app]: true }));
        try {
            const formattedUrl = getFormattedProxyUrl(app);
            const status = await invoke<CliStatus>('get_cli_sync_status', {
                appType: app,
                proxyUrl: formattedUrl
            });
            setStatuses(prev => ({ ...prev, [app]: status }));
        } catch (error) {
            console.error(`Failed to check ${app} status:`, error);
        } finally {
            setLoading(prev => ({ ...prev, [app]: false }));
        }
    }, [getFormattedProxyUrl]);

    const handleSync = (app: CliAppType) => {
        setSyncConfirmApp(app);
    };

    const executeSync = async () => {
        const app = syncConfirmApp;
        if (!app) return;
        setSyncConfirmApp(null);

        if (!proxyUrl || !apiKey) {
            showToast(t('proxy.cli_sync.toast.config_missing', { defaultValue: 'Please generate API Key and start service first' }), 'error');
            return;
        }

        setSyncing(prev => ({ ...prev, [app]: true }));
        try {
            const formattedUrl = getFormattedProxyUrl(app);
            await invoke('execute_cli_sync', {
                appType: app,
                proxyUrl: formattedUrl,
                apiKey: apiKey
            });
            showToast(t('proxy.cli_sync.toast.sync_success', { name: app }), 'success');
            await checkStatus(app);
        } catch (error: any) {
            showToast(t('proxy.cli_sync.toast.sync_error', { name: app, error: error.toString() }), 'error');
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
            await invoke('execute_cli_restore', { appType: app });
            showToast(t('common.success'), 'success');
            await checkStatus(app);
        } catch (error: any) {
            showToast(error.toString(), 'error');
        } finally {
            setSyncing(prev => ({ ...prev, [app]: false }));
        }
    };

    const handleViewConfig = async (app: CliAppType, fileName?: string) => {
        try {
            const status = statuses[app];
            if (!status) return;

            const targetFile = fileName || status.files[0];
            const content = await invoke<string>('get_cli_config_content', {
                appType: app,
                fileName: targetFile
            });
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

                    {!isAppLoading && status?.installed && (
                        <div className={cn(
                            "inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-[10px] font-bold tracking-wide transition-all h-6 shrink-0 whitespace-nowrap shadow-sm",
                            status.is_synced
                                ? "bg-gradient-to-r from-green-500 to-emerald-600 text-white"
                                : "bg-amber-100 dark:bg-amber-900/30 text-amber-600 dark:text-amber-500 border border-amber-200/50 dark:border-amber-800/30"
                        )}>
                            {status.is_synced ? (
                                <><CheckCircle2 size={12} className="shrink-0" /> {t('proxy.cli_sync.status.synced', { defaultValue: 'Synced' })}</>
                            ) : (
                                <><AlertCircle size={12} className="shrink-0" /> {t('proxy.cli_sync.status.not_synced', { defaultValue: 'Not Synced' })}</>
                            )}
                        </div>
                    )}
                </div>

                <div className="mt-auto space-y-3">
                    <div className="p-2.5 bg-gray-50/80 dark:bg-gray-900/40 rounded-lg border border-dashed border-gray-200 dark:border-white/10">
                        <div className="flex justify-between items-start mb-1">
                            <div className="text-[9px] text-gray-400 dark:text-gray-500 uppercase font-bold tracking-wider">{t('proxy.cli_sync.status.current_base_url')}</div>
                        </div>
                        <div className="text-[10px] font-mono truncate text-gray-500 dark:text-gray-400 italic">
                            {status?.current_base_url || '---'}
                        </div>
                    </div>

                    <div className="flex items-center gap-2">
                        {status?.installed && (
                            <>
                                <button
                                    onClick={() => handleViewConfig(app)}
                                    className="btn btn-sm btn-square btn-ghost border border-gray-200 dark:border-white/10 text-gray-500 hover:text-blue-500 hover:bg-white dark:hover:bg-gray-700"
                                    title={t('proxy.cli_sync.btn_view')}
                                >
                                    <Eye size={16} />
                                </button>
                                <button
                                    onClick={() => handleRestore(app)}
                                    className="btn btn-sm btn-square btn-ghost border border-gray-200 dark:border-white/10 text-gray-500 hover:text-orange-500 hover:bg-white dark:hover:bg-gray-700"
                                    title={status.has_backup ? t('proxy.cli_sync.btn_restore_backup') : t('proxy.cli_sync.btn_restore')}
                                >
                                    <RotateCcw size={16} />
                                </button>
                            </>
                        )}
                        <button
                            onClick={() => handleSync(app)}
                            disabled={!status?.installed || isAppSyncing || isAppLoading}
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

            <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                {renderCliItem('Claude', <CodeXml size={20} className="text-purple-500" />, 'Claude Code')}
                {renderCliItem('Codex', <Cpu size={20} className="text-blue-500" />, 'Codex AI')}
                {renderCliItem('Gemini', <Globe size={20} className="text-green-500" />, 'Gemini CLI')}
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

            <ModalDialog
                isOpen={!!syncConfirmApp}
                title={t('proxy.cli_sync.sync_confirm_title')}
                message={syncConfirmApp ? t('proxy.cli_sync.sync_confirm_message', { name: syncConfirmApp }) : ''}
                onConfirm={executeSync}
                onCancel={() => setSyncConfirmApp(null)}
                isDestructive={true}
            />
        </div>
    );
};
