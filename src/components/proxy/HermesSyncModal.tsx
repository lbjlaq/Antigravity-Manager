import { useState, useEffect, useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { RefreshCw, X, CheckCircle2, AlertTriangle, Globe2, ListChecks } from 'lucide-react';
import { HermesAgent } from '@lobehub/icons';
import { cn } from '../../utils/cn';
import { request as invoke } from '../../utils/request';
import { showToast } from '../common/ToastContainer';
import { useProxyModels } from '../../hooks/useProxyModels';

interface HermesSyncModalProps {
    apiKey: string;
    getFormattedProxyUrl: (app: 'Claude' | 'Codex' | 'JeikCode' | 'GrokBuild' | 'Gemini' | 'OpenCode' | 'Droid' | 'Hermes') => string;
    onClose: () => void;
    onSyncDone: () => void;
}

interface HermesStatus {
    installed: boolean;
    version: string | null;
    is_synced: boolean;
    has_backup: boolean;
    current_base_url: string | null;
    files: string[];
    discover_models: boolean;
    configured_models: string[];
    is_active: boolean;
    default_model: string | null;
}

export function HermesSyncModal({ apiKey, getFormattedProxyUrl, onClose, onSyncDone }: HermesSyncModalProps) {
    const { t } = useTranslation();
    const { models: proxyModels } = useProxyModels();
    const [status, setStatus] = useState<HermesStatus | null>(null);
    const [loading, setLoading] = useState(true);
    const [syncing, setSyncing] = useState(false);
    const [useCustomUrl, setUseCustomUrl] = useState(false);
    const [customBaseUrl, setCustomBaseUrl] = useState('');
    const [limitModels, setLimitModels] = useState(false);
    const [selectedModels, setSelectedModels] = useState<Set<string>>(new Set());
    const [activateProvider, setActivateProvider] = useState(false);
    const [defaultModel, setDefaultModel] = useState('');

    const localProxyUrl = getFormattedProxyUrl('Hermes');
    const effectiveProxyUrl = useCustomUrl ? customBaseUrl.trim() : localProxyUrl;

    const loadStatus = useCallback(async () => {
        setLoading(true);
        try {
            const current = await invoke<HermesStatus>('get_hermes_sync_status', { proxyUrl: localProxyUrl });
            setStatus(current);
            const configuredUrl = current.current_base_url?.trim() || '';
            const isCustom = Boolean(configuredUrl && configuredUrl.replace(/\/+$/, '') !== localProxyUrl.replace(/\/+$/, ''));
            setUseCustomUrl(isCustom);
            setCustomBaseUrl(configuredUrl || localProxyUrl);
            setLimitModels(!current.discover_models);
            setSelectedModels(new Set(current.configured_models));
            setActivateProvider(current.is_active);
            setDefaultModel(current.default_model || current.configured_models[0] || '');
        } catch (error: any) {
            showToast(error.toString(), 'error');
        } finally {
            setLoading(false);
        }
    }, [localProxyUrl]);

    useEffect(() => {
        loadStatus();
    }, [loadStatus]);

    // Keep configured IDs (including aliases) selectable even when they are not
    // included in the local catalog, or opening the dialog changes the default.
    const antigravityModels = useMemo(() => {
        const models: { id: string; name: string; group: string }[] = [...proxyModels];
        const knownIds = new Set(models.map(model => model.id));
        for (const id of [...(status?.configured_models || []), status?.default_model]) {
            if (id && !knownIds.has(id)) {
                models.push({ id, name: id, group: 'Hermes' });
                knownIds.add(id);
            }
        }
        return models;
    }, [proxyModels, status]);
    const groups = useMemo(() => [...new Set(antigravityModels.map(model => model.group))], [antigravityModels]);
    const allSelected = antigravityModels.length > 0 && antigravityModels.every(model => selectedModels.has(model.id));
    const availableDefaultModels = useMemo(
        () => limitModels
            ? antigravityModels.filter(model => selectedModels.has(model.id))
            : antigravityModels,
        [antigravityModels, limitModels, selectedModels]
    );

    useEffect(() => {
        if (loading) return;
        if (availableDefaultModels.length === 0) {
            if (defaultModel) setDefaultModel('');
            return;
        }
        if (!availableDefaultModels.some(model => model.id === defaultModel)) {
            setDefaultModel(availableDefaultModels[0].id);
        }
    }, [availableDefaultModels, defaultModel, loading]);

    const toggleAll = () => {
        setSelectedModels(allSelected ? new Set() : new Set(antigravityModels.map(model => model.id)));
    };

    const toggleModel = (modelId: string) => {
        const next = new Set(selectedModels);
        if (next.has(modelId)) {
            next.delete(modelId);
        } else {
            next.add(modelId);
        }
        setSelectedModels(next);
        if (defaultModel === modelId && !next.has(modelId)) {
            setDefaultModel([...next][0] || '');
        }
    };

    const executeSync = async () => {
        if (!effectiveProxyUrl) {
            showToast(t('proxy.hermes_sync.url_required', { defaultValue: 'Enter the Hermes connection address' }), 'error');
            return;
        }
        if (!apiKey) {
            showToast(t('proxy.cli_sync.toast.config_missing', { defaultValue: 'Please generate an API Key and start the service first' }), 'error');
            return;
        }
        if (limitModels && selectedModels.size === 0) {
            showToast(t('proxy.hermes_sync.models_required', { defaultValue: 'Select at least one model' }), 'error');
            return;
        }
        if (activateProvider && !defaultModel) {
            showToast(t('proxy.hermes_sync.default_model_required', { defaultValue: 'Select a default model' }), 'error');
            return;
        }
        if (activateProvider && !availableDefaultModels.some(model => model.id === defaultModel)) {
            showToast(t('proxy.hermes_sync.default_model_invalid', { defaultValue: 'Select a default model available through Antigravity Manager' }), 'error');
            return;
        }

        setSyncing(true);
        try {
            await invoke('execute_hermes_sync', {
                proxyUrl: effectiveProxyUrl,
                apiKey,
                discoverModels: !limitModels,
                models: limitModels ? [...selectedModels] : [],
                activate: activateProvider,
                defaultModel: activateProvider ? defaultModel : null,
            });
            showToast(t('proxy.hermes_sync.toast.sync_success', { defaultValue: 'Hermes synced successfully' }), 'success');
            onSyncDone();
            onClose();
        } catch (error: any) {
            showToast(error.toString(), 'error');
        } finally {
            setSyncing(false);
        }
    };

    return (
        <div className="fixed inset-0 z-[300] flex items-center justify-center p-4 bg-black/50 backdrop-blur-sm animate-in fade-in duration-200">
            <div className="bg-white dark:bg-base-100 rounded-2xl shadow-2xl border border-gray-200 dark:border-base-300 w-full max-w-2xl max-h-[88vh] overflow-hidden animate-in zoom-in-95 duration-200 flex flex-col">
                <div className="px-5 pt-4 pb-3 shrink-0 border-b border-gray-100 dark:border-base-200">
                    <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2.5">
                            <div className="p-1.5 bg-purple-50 dark:bg-purple-900/20 rounded-lg flex items-center justify-center">
                                <HermesAgent.Avatar size={22} />
                            </div>
                            <div>
                                <h3 className="text-sm font-bold text-gray-900 dark:text-base-content">
                                    {t('proxy.hermes_sync.modal_title', { defaultValue: 'Configure Antigravity Manager in Hermes' })}
                                </h3>
                                <p className="text-[10px] text-gray-400 mt-0.5">$HERMES_HOME/config.yaml → providers.antigravity-manager</p>
                            </div>
                        </div>
                        <button type="button" onClick={onClose} className="p-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-base-300 transition-colors">
                            <X size={16} className="text-gray-400" />
                        </button>
                    </div>
                </div>

                <div className="px-5 py-4 space-y-4 overflow-auto">
                    {loading ? (
                        <div className="flex items-center gap-2 text-xs text-gray-400 py-8 justify-center">
                            <RefreshCw size={14} className="animate-spin" />
                            {t('proxy.cli_sync.status.detecting')}
                        </div>
                    ) : (
                        <>
                            <section className="space-y-2">
                                <div className="flex items-center gap-2 text-[10px] font-bold text-gray-400 uppercase tracking-wider">
                                    <Globe2 size={13} />
                                    {t('proxy.hermes_sync.endpoint_title', { defaultValue: 'Connection address' })}
                                </div>
                                <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                                    <label className={cn("flex items-start gap-2 p-3 rounded-xl border cursor-pointer transition-all", !useCustomUrl ? "border-purple-400 bg-purple-50/50 dark:bg-purple-900/10" : "border-gray-200 dark:border-base-300")}>
                                        <input type="radio" checked={!useCustomUrl} onChange={() => setUseCustomUrl(false)} className="radio radio-xs radio-primary mt-0.5" />
                                        <span className="min-w-0">
                                            <span className="block text-xs font-bold">{t('proxy.hermes_sync.local_endpoint', { defaultValue: 'Local proxy' })}</span>
                                            <span className="block text-[10px] font-mono text-gray-400 truncate mt-0.5">{localProxyUrl}</span>
                                        </span>
                                    </label>
                                    <label className={cn("flex items-start gap-2 p-3 rounded-xl border cursor-pointer transition-all", useCustomUrl ? "border-purple-400 bg-purple-50/50 dark:bg-purple-900/10" : "border-gray-200 dark:border-base-300")}>
                                        <input type="radio" checked={useCustomUrl} onChange={() => setUseCustomUrl(true)} className="radio radio-xs radio-primary mt-0.5" />
                                        <span>
                                            <span className="block text-xs font-bold">{t('proxy.hermes_sync.custom_endpoint', { defaultValue: 'Custom address' })}</span>
                                            <span className="block text-[10px] text-gray-400 mt-0.5">{t('proxy.hermes_sync.custom_endpoint_desc', { defaultValue: 'Docker, LAN or reverse proxy' })}</span>
                                        </span>
                                    </label>
                                </div>
                                {useCustomUrl && (
                                    <input
                                        type="text"
                                        value={customBaseUrl}
                                        onChange={event => setCustomBaseUrl(event.target.value)}
                                        placeholder="http://host.docker.internal:8045/v1"
                                        className="w-full px-3 py-2 text-xs font-mono bg-white dark:bg-base-200 border border-gray-200 dark:border-base-300 rounded-lg focus:ring-1 focus:ring-purple-500 focus:border-purple-500 outline-none"
                                    />
                                )}
                            </section>

                            <section className="space-y-2">
                                <div className="flex items-center gap-2 text-[10px] font-bold text-gray-400 uppercase tracking-wider">
                                    <ListChecks size={13} />
                                    {t('proxy.hermes_sync.models_title', { defaultValue: 'Models available in Hermes' })}
                                </div>
                                <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                                    <label className={cn("flex items-start gap-2 p-3 rounded-xl border cursor-pointer transition-all", !limitModels ? "border-purple-400 bg-purple-50/50 dark:bg-purple-900/10" : "border-gray-200 dark:border-base-300")}>
                                        <input type="radio" checked={!limitModels} onChange={() => setLimitModels(false)} className="radio radio-xs radio-primary mt-0.5" />
                                        <span>
                                            <span className="block text-xs font-bold">{t('proxy.hermes_sync.discover_models', { defaultValue: 'Discover automatically' })}</span>
                                            <span className="block text-[10px] text-gray-400 mt-0.5">{t('proxy.hermes_sync.discover_models_desc', { defaultValue: 'Hermes reads the live model list from /v1/models' })}</span>
                                        </span>
                                    </label>
                                    <label className={cn("flex items-start gap-2 p-3 rounded-xl border cursor-pointer transition-all", limitModels ? "border-purple-400 bg-purple-50/50 dark:bg-purple-900/10" : "border-gray-200 dark:border-base-300")}>
                                        <input type="radio" checked={limitModels} onChange={() => setLimitModels(true)} className="radio radio-xs radio-primary mt-0.5" />
                                        <span>
                                            <span className="block text-xs font-bold">{t('proxy.hermes_sync.selected_models', { defaultValue: 'Only selected models' })}</span>
                                            <span className="block text-[10px] text-gray-400 mt-0.5">{selectedModels.size}/{antigravityModels.length}</span>
                                        </span>
                                    </label>
                                </div>
                                {limitModels && (
                                    <div className="p-3 rounded-xl border border-gray-200 dark:border-base-300 space-y-3 max-h-[28vh] overflow-auto">
                                        <div className="flex justify-end">
                                            <button type="button" onClick={toggleAll} className="text-[10px] text-purple-500 hover:text-purple-600 font-medium">
                                                {allSelected ? t('common.deselect_all', { defaultValue: 'Deselect all' }) : t('common.select_all', { defaultValue: 'Select all' })}
                                            </button>
                                        </div>
                                        {groups.map(group => (
                                            <div key={group}>
                                                <div className="text-[9px] font-bold text-gray-400 uppercase tracking-widest mb-1.5">{group}</div>
                                                <div className="flex flex-wrap gap-1.5">
                                                    {antigravityModels.filter(model => model.group === group).map(model => {
                                                        const selected = selectedModels.has(model.id);
                                                        return (
                                                            <button
                                                                type="button"
                                                                key={model.id}
                                                                onClick={() => toggleModel(model.id)}
                                                                className={cn(
                                                                    "px-2.5 py-1 rounded-md text-[11px] font-medium transition-all border",
                                                                    selected
                                                                        ? "bg-purple-500 text-white border-purple-500"
                                                                        : "bg-gray-50 dark:bg-base-200 text-gray-500 dark:text-gray-400 border-gray-200 dark:border-base-300 hover:border-purple-300"
                                                                )}
                                                            >
                                                                {model.id}
                                                            </button>
                                                        );
                                                    })}
                                                </div>
                                            </div>
                                        ))}
                                    </div>
                                )}
                            </section>

                            <section className="space-y-2">
                                <label className={cn("flex items-start gap-2 p-3 rounded-xl border cursor-pointer transition-all", activateProvider ? "border-purple-400 bg-purple-50/50 dark:bg-purple-900/10" : "border-gray-200 dark:border-base-300")}>
                                    <input type="checkbox" checked={activateProvider} onChange={event => setActivateProvider(event.target.checked)} className="checkbox checkbox-xs checkbox-primary mt-0.5" />
                                    <span>
                                        <span className="block text-xs font-bold">{t('proxy.hermes_sync.activate_provider', { defaultValue: 'Make Antigravity Manager the default Hermes provider' })}</span>
                                        <span className="block text-[10px] text-gray-400 mt-0.5">{t('proxy.hermes_sync.activate_provider_desc', { defaultValue: 'Off by default so your current Hermes provider is not changed.' })}</span>
                                    </span>
                                </label>
                                {activateProvider && (
                                    <div>
                                        <label className="block text-[9px] text-gray-400 uppercase font-bold tracking-wider mb-1">{t('proxy.hermes_sync.default_model', { defaultValue: 'Default model' })}</label>
                                        <select
                                            value={defaultModel}
                                            onChange={event => setDefaultModel(event.target.value)}
                                            className="select select-bordered select-sm w-full text-xs"
                                        >
                                            <option value="" disabled>{t('proxy.hermes_sync.select_default_model', { defaultValue: 'Select a model' })}</option>
                                            {availableDefaultModels.map(model => <option key={model.id} value={model.id}>{model.id}</option>)}
                                        </select>
                                    </div>
                                )}
                            </section>

                            <div className="flex items-start gap-2 p-3 bg-gray-50/80 dark:bg-gray-900/40 rounded-xl border border-dashed border-gray-200 dark:border-white/10 text-[11px] text-gray-500 dark:text-gray-400">
                                {status?.is_synced ? <CheckCircle2 size={14} className="text-green-500 shrink-0 mt-0.5" /> : <AlertTriangle size={14} className="text-amber-500 shrink-0 mt-0.5" />}
                                <div>
                                    <div>{status?.is_synced ? t('proxy.cli_sync.status.synced', { defaultValue: 'Synced' }) : t('proxy.cli_sync.status.not_synced', { defaultValue: 'Not synced' })}</div>
                                    {status?.current_base_url && <div className="font-mono text-[10px] mt-0.5 break-all">{status.current_base_url}</div>}
                                    {status?.is_active && <div className="text-purple-500 mt-0.5">{t('proxy.hermes_sync.currently_active', { defaultValue: 'Currently active in Hermes' })}{status.default_model ? ` — ${status.default_model}` : ''}</div>}
                                </div>
                            </div>
                        </>
                    )}
                </div>

                <div className="px-5 py-3 border-t border-gray-100 dark:border-base-200 flex items-center justify-end gap-2 shrink-0">
                    <button type="button" className="px-3 py-1.5 text-xs text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 rounded-lg hover:bg-gray-100 dark:hover:bg-base-300 transition-colors" onClick={onClose}>
                        {t('common.cancel', { defaultValue: 'Cancel' })}
                    </button>
                    <button
                        type="button"
                        className="px-4 py-1.5 text-xs font-bold rounded-lg transition-all flex items-center gap-1.5 bg-purple-500 hover:bg-purple-600 active:bg-purple-700 text-white shadow-sm disabled:opacity-50"
                        disabled={loading || syncing || !status}
                        onClick={executeSync}
                    >
                        <RefreshCw size={12} className={syncing ? 'animate-spin' : ''} />
                        {t('proxy.hermes_sync.btn_confirm_sync', { defaultValue: 'Save to Hermes' })}
                    </button>
                </div>
            </div>
        </div>
    );
}
