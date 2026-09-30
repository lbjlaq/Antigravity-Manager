import { useState, useEffect, useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { RefreshCw, X, CheckCircle2, AlertTriangle, Globe2, ListChecks, Layers } from 'lucide-react';
import { OpenClaw } from '@lobehub/icons';
import { cn } from '../../utils/cn';
import { request as invoke } from '../../utils/request';
import { showToast } from '../common/ToastContainer';
import { useProxyModels } from '../../hooks/useProxyModels';

interface OpenClawSyncModalProps {
    apiKey: string;
    getFormattedProxyUrl: (app: any) => string;
    onClose: () => void;
    onSyncDone: () => void;
}

interface OpenClawStatus {
    installed: boolean;
    version: string | null;
    detectedVersionTarget: 'v1' | 'v2' | null;
    isSynced: boolean;
    hasBackup: boolean;
    currentBaseUrl: string | null;
    files: string[];
    configuredModels: string[];
    isActive: boolean;
    defaultModel: string | null;
    syncedVersion: 'v1' | 'v2' | null;
}

export function OpenClawSyncModal({ apiKey, getFormattedProxyUrl, onClose, onSyncDone }: OpenClawSyncModalProps) {
    const { t } = useTranslation();
    const { models: proxyModels } = useProxyModels();
    const [status, setStatus] = useState<OpenClawStatus | null>(null);
    const [loading, setLoading] = useState(true);
    const [syncingVersion, setSyncingVersion] = useState<'v1' | 'v2' | null>(null);
    const [targetVersion, setTargetVersion] = useState<'v1' | 'v2'>('v2');
    const [useCustomUrl, setUseCustomUrl] = useState(false);
    const [customBaseUrl, setCustomBaseUrl] = useState('');
    const [limitModels, setLimitModels] = useState(false);
    const [selectedModels, setSelectedModels] = useState<Set<string>>(new Set());
    const [activateProvider, setActivateProvider] = useState(false);
    const [defaultModel, setDefaultModel] = useState('');

    const localProxyUrl = getFormattedProxyUrl('OpenClaw');
    const effectiveProxyUrl = useCustomUrl ? customBaseUrl.trim() : localProxyUrl;

    const loadStatus = useCallback(async () => {
        setLoading(true);
        try {
            const current = await invoke<OpenClawStatus>('get_openclaw_sync_status', { proxyUrl: localProxyUrl });
            setStatus(current);
            const configuredUrl = current.currentBaseUrl?.trim() || '';
            const isCustom = Boolean(configuredUrl && configuredUrl.replace(/\/+$/, '') !== localProxyUrl.replace(/\/+$/, ''));
            setUseCustomUrl(isCustom);
            setCustomBaseUrl(configuredUrl || localProxyUrl);
            setLimitModels(current.configuredModels.length > 0);
            setSelectedModels(new Set(current.configuredModels));
            setActivateProvider(current.isActive);
            setDefaultModel(current.defaultModel || current.configuredModels[0] || '');

            // 优先采用配置中已同步的版本；若无则采用探测到的版本，默认 v2
            if (current.syncedVersion) {
                setTargetVersion(current.syncedVersion);
            } else if (current.detectedVersionTarget) {
                setTargetVersion(current.detectedVersionTarget);
            } else {
                setTargetVersion('v2');
            }
        } catch (error: any) {
            showToast(error.toString(), 'error');
        } finally {
            setLoading(false);
        }
    }, [localProxyUrl]);

    useEffect(() => {
        loadStatus();
    }, [loadStatus]);

    const antigravityModels = useMemo(() => {
        const models: { id: string; name: string; group: string }[] = [...proxyModels];
        const knownIds = new Set(models.map(m => m.id));
        for (const id of [...(status?.configuredModels || []), status?.defaultModel]) {
            if (id && !knownIds.has(id)) {
                models.push({ id, name: id, group: 'OpenClaw' });
                knownIds.add(id);
            }
        }
        return models;
    }, [proxyModels, status]);

    const groups = useMemo(() => [...new Set(antigravityModels.map(m => m.group))], [antigravityModels]);
    const allSelected = antigravityModels.length > 0 && antigravityModels.every(m => selectedModels.has(m.id));
    const availableDefaultModels = useMemo(
        () => limitModels ? antigravityModels.filter(m => selectedModels.has(m.id)) : antigravityModels,
        [antigravityModels, limitModels, selectedModels]
    );

    useEffect(() => {
        if (loading) return;
        if (availableDefaultModels.length === 0) {
            if (defaultModel) setDefaultModel('');
            return;
        }
        if (!availableDefaultModels.some(m => m.id === defaultModel)) {
            setDefaultModel(availableDefaultModels[0].id);
        }
    }, [availableDefaultModels, defaultModel, loading]);

    const toggleAll = () => {
        setSelectedModels(allSelected ? new Set() : new Set(antigravityModels.map(m => m.id)));
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

    const executeSyncWithVersion = async (version: 'v1' | 'v2') => {
        if (!effectiveProxyUrl) {
            showToast(t('proxy.openclaw_sync.url_required', { defaultValue: '请输入 OpenClaw 连接地址' }), 'error');
            return;
        }
        if (!apiKey) {
            showToast(t('proxy.cli_sync.toast.config_missing', { defaultValue: '请先生成 API Key 并启动服务' }), 'error');
            return;
        }
        if (limitModels && selectedModels.size === 0) {
            showToast(t('proxy.openclaw_sync.models_required', { defaultValue: '请至少选择一个模型' }), 'error');
            return;
        }
        if (activateProvider && !defaultModel) {
            showToast(t('proxy.openclaw_sync.default_model_required', { defaultValue: '请选择默认模型' }), 'error');
            return;
        }

        setSyncingVersion(version);
        try {
            const modelsToSync = limitModels && selectedModels.size > 0
                ? [...selectedModels]
                : antigravityModels.map(m => m.id);

            await invoke('execute_openclaw_sync', {
                proxyUrl: effectiveProxyUrl,
                apiKey,
                targetVersion: version,
                models: modelsToSync,
                activate: activateProvider,
                defaultModel: activateProvider ? defaultModel : null,
            });
            showToast(t('proxy.openclaw_sync.toast.sync_success', {
                version: version === 'v2' ? 'v2.0 (≥ 2026.8.1)' : 'v1.0 (< 2026.8.1)',
                defaultValue: `OpenClaw (${version === 'v2' ? '≥2026.8.1' : '<2026.8.1'}) 配置同步成功`
            }), 'success');
            onSyncDone();
            onClose();
        } catch (error: any) {
            showToast(error.toString(), 'error');
        } finally {
            setSyncingVersion(null);
        }
    };

    return (
        <div className="fixed inset-0 z-[300] flex items-center justify-center p-4 bg-black/50 backdrop-blur-sm animate-in fade-in duration-200">
            <div className="bg-white dark:bg-base-100 rounded-2xl shadow-2xl border border-gray-200 dark:border-base-300 w-full max-w-2xl max-h-[88vh] overflow-hidden animate-in zoom-in-95 duration-200 flex flex-col">
                {/* 顶栏 */}
                <div className="px-5 pt-4 pb-3 shrink-0 border-b border-gray-100 dark:border-base-200">
                    <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2.5">
                            <div className="p-1.5 bg-[#18181b] dark:bg-[#121214] rounded-lg flex items-center justify-center border border-white/10 shadow-sm">
                                <OpenClaw.Color size={22} />
                            </div>
                            <div>
                                <h3 className="text-sm font-bold text-gray-900 dark:text-base-content flex items-center gap-2">
                                    {t('proxy.openclaw_sync.modal_title', { defaultValue: '在 OpenClaw 中配置 Antigravity Manager' })}
                                    {status?.version && (
                                        <span className="text-[10px] px-1.5 py-0.2 rounded bg-red-500/10 text-red-500 border border-red-500/20 font-mono">
                                            v{status.version}
                                        </span>
                                    )}
                                </h3>
                                <p className="text-[10px] text-gray-400 mt-0.5">$OPENCLAW_CONFIG_PATH / ~/.openclaw/openclaw.json → models.providers.antigravity-manager</p>
                            </div>
                        </div>
                        <button type="button" onClick={onClose} className="p-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-base-300 transition-colors">
                            <X size={16} className="text-gray-400" />
                        </button>
                    </div>
                </div>

                {/* 内容区 */}
                <div className="px-5 py-4 space-y-4 overflow-auto">
                    {loading ? (
                        <div className="flex items-center gap-2 text-xs text-gray-400 py-8 justify-center">
                            <RefreshCw size={14} className="animate-spin" />
                            {t('proxy.cli_sync.status.detecting')}
                        </div>
                    ) : (
                        <>
                            {/* 1. 版本规范选择 */}
                            <section className="space-y-2">
                                <div className="flex items-center justify-between">
                                    <div className="flex items-center gap-2 text-[10px] font-bold text-gray-400 uppercase tracking-wider">
                                        <Layers size={13} />
                                        {t('proxy.openclaw_sync.version_title', { defaultValue: 'OpenClaw SDK 版本规范' })}
                                    </div>
                                    {status?.detectedVersionTarget && (
                                        <span className="text-[9px] text-emerald-600 dark:text-emerald-400 bg-emerald-500/10 border border-emerald-500/20 px-1.5 py-0.5 rounded-md font-medium">
                                            {t('proxy.openclaw_sync.detected_hint', {
                                                version: status.version || '',
                                                target: status.detectedVersionTarget === 'v2' ? '≥ 2026.8.1 (v2.0)' : '< 2026.8.1 (v1.0)',
                                                defaultValue: `环境已检测: ${status.detectedVersionTarget === 'v2' ? '≥ 2026.8.1' : '< 2026.8.1'}`
                                            })}
                                        </span>
                                    )}
                                </div>
                                <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                                    <label
                                        onClick={() => setTargetVersion('v2')}
                                        className={cn(
                                            "flex items-start gap-2.5 p-3 rounded-xl border cursor-pointer transition-all",
                                            targetVersion === 'v2'
                                                ? "border-red-500/70 bg-red-500/[0.04] dark:bg-red-500/[0.08] shadow-sm ring-1 ring-red-500/20"
                                                : "border-gray-200 dark:border-base-300 hover:border-gray-300"
                                        )}
                                    >
                                        <input
                                            type="radio"
                                            name="openclaw-version"
                                            checked={targetVersion === 'v2'}
                                            onChange={() => setTargetVersion('v2')}
                                            className="radio radio-xs radio-error mt-0.5"
                                        />
                                        <span className="min-w-0 flex-1">
                                            <span className="flex items-center gap-1.5">
                                                <span className="text-xs font-bold text-gray-900 dark:text-gray-100">
                                                    {t('proxy.openclaw_sync.v2_label', { defaultValue: '≥ 2026.8.1 (v2.0 规范)' })}
                                                </span>
                                                <span className="text-[9px] px-1 rounded bg-red-500/10 text-red-500 font-bold uppercase">最新</span>
                                            </span>
                                            <span className="block text-[10px] text-gray-500 dark:text-gray-400 mt-1 leading-normal">
                                                {t('proxy.openclaw_sync.v2_desc', { defaultValue: '采用 modelPolicy.allow 白名单规范与 entries 架构，原生支持图像/工具多模态' })}
                                            </span>
                                        </span>
                                    </label>

                                    <label
                                        onClick={() => setTargetVersion('v1')}
                                        className={cn(
                                            "flex items-start gap-2.5 p-3 rounded-xl border cursor-pointer transition-all",
                                            targetVersion === 'v1'
                                                ? "border-red-500/70 bg-red-500/[0.04] dark:bg-red-500/[0.08] shadow-sm ring-1 ring-red-500/20"
                                                : "border-gray-200 dark:border-base-300 hover:border-gray-300"
                                        )}
                                    >
                                        <input
                                            type="radio"
                                            name="openclaw-version"
                                            checked={targetVersion === 'v1'}
                                            onChange={() => setTargetVersion('v1')}
                                            className="radio radio-xs radio-error mt-0.5"
                                        />
                                        <span className="min-w-0 flex-1">
                                            <span className="flex items-center gap-1.5">
                                                <span className="text-xs font-bold text-gray-900 dark:text-gray-100">
                                                    {t('proxy.openclaw_sync.v1_label', { defaultValue: '< 2026.8.1 (v1.0 规范)' })}
                                                </span>
                                                <span className="text-[9px] px-1 rounded bg-gray-100 dark:bg-base-300 text-gray-500 font-medium">经典</span>
                                            </span>
                                            <span className="block text-[10px] text-gray-500 dark:text-gray-400 mt-1 leading-normal">
                                                {t('proxy.openclaw_sync.v1_desc', { defaultValue: '采用 agents.defaults.models 显式模型字典注册，全面兼容 2026.7 及更早 SDK' })}
                                            </span>
                                        </span>
                                    </label>
                                </div>
                            </section>

                            {/* 2. 连接地址 */}
                            <section className="space-y-2">
                                <div className="flex items-center gap-2 text-[10px] font-bold text-gray-400 uppercase tracking-wider">
                                    <Globe2 size={13} />
                                    {t('proxy.openclaw_sync.endpoint_title', { defaultValue: '连接地址 (Base URL)' })}
                                </div>
                                <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                                    <label className={cn("flex items-start gap-2 p-3 rounded-xl border cursor-pointer transition-all", !useCustomUrl ? "border-red-500/70 bg-red-500/[0.04] dark:bg-red-500/[0.08]" : "border-gray-200 dark:border-base-300")}>
                                        <input type="radio" checked={!useCustomUrl} onChange={() => setUseCustomUrl(false)} className="radio radio-xs radio-error mt-0.5" />
                                        <span className="min-w-0">
                                            <span className="block text-xs font-bold">{t('proxy.openclaw_sync.local_endpoint', { defaultValue: '本地代理' })}</span>
                                            <span className="block text-[10px] font-mono text-gray-400 truncate mt-0.5">{localProxyUrl}</span>
                                        </span>
                                    </label>
                                    <label className={cn("flex items-start gap-2 p-3 rounded-xl border cursor-pointer transition-all", useCustomUrl ? "border-red-500/70 bg-red-500/[0.04] dark:bg-red-500/[0.08]" : "border-gray-200 dark:border-base-300")}>
                                        <input type="radio" checked={useCustomUrl} onChange={() => setUseCustomUrl(true)} className="radio radio-xs radio-error mt-0.5" />
                                        <span>
                                            <span className="block text-xs font-bold">{t('proxy.openclaw_sync.custom_endpoint', { defaultValue: '自定义地址' })}</span>
                                            <span className="block text-[10px] text-gray-400 mt-0.5">{t('proxy.openclaw_sync.custom_endpoint_desc', { defaultValue: 'Docker、局域网或反向代理' })}</span>
                                        </span>
                                    </label>
                                </div>
                                {useCustomUrl && (
                                    <input
                                        type="text"
                                        value={customBaseUrl}
                                        onChange={e => setCustomBaseUrl(e.target.value)}
                                        placeholder="http://host.docker.internal:8045/v1"
                                        className="w-full px-3 py-2 text-xs font-mono bg-white dark:bg-base-200 border border-gray-200 dark:border-base-300 rounded-lg focus:ring-1 focus:ring-red-500 focus:border-red-500 outline-none"
                                    />
                                )}
                            </section>

                            {/* 3. 模型选择 */}
                            <section className="space-y-2">
                                <div className="flex items-center gap-2 text-[10px] font-bold text-gray-400 uppercase tracking-wider">
                                    <ListChecks size={13} />
                                    {t('proxy.openclaw_sync.models_title', { defaultValue: '同步到 OpenClaw 的模型' })}
                                </div>
                                <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                                    <label className={cn("flex items-start gap-2 p-3 rounded-xl border cursor-pointer transition-all", !limitModels ? "border-red-500/70 bg-red-500/[0.04] dark:bg-red-500/[0.08]" : "border-gray-200 dark:border-base-300")}>
                                        <input type="radio" checked={!limitModels} onChange={() => setLimitModels(false)} className="radio radio-xs radio-error mt-0.5" />
                                        <span>
                                            <span className="block text-xs font-bold">{t('proxy.openclaw_sync.all_models', { defaultValue: '全部同步' })}</span>
                                            <span className="block text-[10px] text-gray-400 mt-0.5">{t('proxy.openclaw_sync.all_models_desc', { count: antigravityModels.length, defaultValue: `同步当前全部 ${antigravityModels.length} 个可用模型` })}</span>
                                        </span>
                                    </label>
                                    <label className={cn("flex items-start gap-2 p-3 rounded-xl border cursor-pointer transition-all", limitModels ? "border-red-500/70 bg-red-500/[0.04] dark:bg-red-500/[0.08]" : "border-gray-200 dark:border-base-300")}>
                                        <input type="radio" checked={limitModels} onChange={() => setLimitModels(true)} className="radio radio-xs radio-error mt-0.5" />
                                        <span>
                                            <span className="block text-xs font-bold">{t('proxy.openclaw_sync.selected_models', { defaultValue: '部分挑选' })}</span>
                                            <span className="block text-[10px] text-gray-400 mt-0.5">{selectedModels.size}/{antigravityModels.length}</span>
                                        </span>
                                    </label>
                                </div>
                                {limitModels && (
                                    <div className="p-3 rounded-xl border border-gray-200 dark:border-base-300 space-y-3 max-h-[26vh] overflow-auto">
                                        <div className="flex justify-end">
                                            <button type="button" onClick={toggleAll} className="text-[10px] text-red-500 hover:text-red-600 font-medium">
                                                {allSelected ? t('common.deselect_all', { defaultValue: '取消全选' }) : t('common.select_all', { defaultValue: '全选' })}
                                            </button>
                                        </div>
                                        {groups.map(group => (
                                            <div key={group}>
                                                <div className="text-[9px] font-bold text-gray-400 uppercase tracking-widest mb-1.5">{group}</div>
                                                <div className="flex flex-wrap gap-1.5">
                                                    {antigravityModels.filter(m => m.group === group).map(model => {
                                                        const selected = selectedModels.has(model.id);
                                                        return (
                                                            <button
                                                                type="button"
                                                                key={model.id}
                                                                onClick={() => toggleModel(model.id)}
                                                                className={cn(
                                                                    "px-2.5 py-1 rounded-md text-[11px] font-medium transition-all border",
                                                                    selected
                                                                        ? "bg-red-500 text-white border-red-500 shadow-2xs"
                                                                        : "bg-gray-50 dark:bg-base-200 text-gray-500 dark:text-gray-400 border-gray-200 dark:border-base-300 hover:border-red-300"
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

                            {/* 4. 默认主模型 */}
                            <section className="space-y-2">
                                <label className={cn("flex items-start gap-2 p-3 rounded-xl border cursor-pointer transition-all", activateProvider ? "border-red-500/70 bg-red-500/[0.04] dark:bg-red-500/[0.08]" : "border-gray-200 dark:border-base-300")}>
                                    <input type="checkbox" checked={activateProvider} onChange={e => setActivateProvider(e.target.checked)} className="checkbox checkbox-xs checkbox-error mt-0.5" />
                                    <span>
                                        <span className="block text-xs font-bold">{t('proxy.openclaw_sync.activate_provider', { defaultValue: '设为 OpenClaw 默认主要模型 (Primary)' })}</span>
                                        <span className="block text-[10px] text-gray-400 mt-0.5">{t('proxy.openclaw_sync.activate_provider_desc', { defaultValue: '勾选后将自动更新 agents.defaults.model.primary' })}</span>
                                    </span>
                                </label>
                                {activateProvider && (
                                    <div>
                                        <label className="block text-[9px] text-gray-400 uppercase font-bold tracking-wider mb-1">{t('proxy.openclaw_sync.default_model', { defaultValue: '默认模型' })}</label>
                                        <select
                                            value={defaultModel}
                                            onChange={e => setDefaultModel(e.target.value)}
                                            className="select select-bordered select-sm w-full text-xs"
                                        >
                                            <option value="" disabled>{t('proxy.openclaw_sync.select_default_model', { defaultValue: '选择模型' })}</option>
                                            {availableDefaultModels.map(m => <option key={m.id} value={m.id}>{m.id}</option>)}
                                        </select>
                                    </div>
                                )}
                            </section>

                            {/* 状态徽标栏 */}
                            <div className="flex items-start gap-2 p-3 bg-gray-50/80 dark:bg-gray-900/40 rounded-xl border border-dashed border-gray-200 dark:border-white/10 text-[11px] text-gray-500 dark:text-gray-400">
                                {status?.isSynced ? <CheckCircle2 size={14} className="text-green-500 shrink-0 mt-0.5" /> : <AlertTriangle size={14} className="text-amber-500 shrink-0 mt-0.5" />}
                                <div>
                                    <div className="flex items-center gap-1.5">
                                        <span>{status?.isSynced ? t('proxy.cli_sync.status.synced', { defaultValue: '已同步' }) : t('proxy.cli_sync.status.not_synced', { defaultValue: '未同步' })}</span>
                                        {status?.syncedVersion && (
                                            <span className="text-[9px] font-mono px-1 rounded bg-red-500/10 text-red-500 font-bold">
                                                {status.syncedVersion === 'v2' ? 'v2.0 规范' : 'v1.0 规范'}
                                            </span>
                                        )}
                                    </div>
                                    {status?.currentBaseUrl && <div className="font-mono text-[10px] mt-0.5 break-all">{status.currentBaseUrl}</div>}
                                    {status?.isActive && <div className="text-red-500 mt-0.5">{t('proxy.openclaw_sync.currently_active', { defaultValue: '当前已激活为主模型' })}{status.defaultModel ? ` — ${status.defaultModel}` : ''}</div>}
                                </div>
                            </div>
                        </>
                    )}
                </div>

                {/* 底部按钮栏：双版本专属同步按钮 */}
                <div className="px-5 py-3 border-t border-gray-100 dark:border-base-200 flex items-center justify-between gap-2 shrink-0 bg-gray-50/40 dark:bg-base-200/20">
                    <button type="button" className="px-3 py-1.5 text-xs text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 rounded-lg hover:bg-gray-100 dark:hover:bg-base-300 transition-colors" onClick={onClose}>
                        {t('common.cancel', { defaultValue: '取消' })}
                    </button>
                    
                    <div className="flex items-center gap-2">
                        {/* 按钮 1: < 2026.8.1 (v1.0) 同步 */}
                        <button
                            type="button"
                            className={cn(
                                "px-3.5 py-1.5 text-xs font-bold rounded-lg transition-all flex items-center gap-1.5 border shadow-xs disabled:opacity-50",
                                targetVersion === 'v1'
                                    ? "bg-red-500 hover:bg-red-600 text-white border-red-500 shadow-sm"
                                    : "bg-white dark:bg-base-200 hover:bg-gray-50 text-gray-700 dark:text-gray-200 border-gray-200 dark:border-base-300"
                            )}
                            disabled={loading || !!syncingVersion || !status}
                            onClick={() => executeSyncWithVersion('v1')}
                            title="适用于 2026.7 及更早历史版本的 OpenClaw SDK (采用 agents.defaults.models 字典注册)"
                        >
                            <RefreshCw size={11} className={syncingVersion === 'v1' ? 'animate-spin' : ''} />
                            {t('proxy.openclaw_sync.btn_sync_v1', { defaultValue: '同步 (< 2026.8.1 v1.0)' })}
                        </button>

                        {/* 按钮 2: ≥ 2026.8.1 (v2.0) 同步 */}
                        <button
                            type="button"
                            className={cn(
                                "px-4 py-1.5 text-xs font-bold rounded-lg transition-all flex items-center gap-1.5 border shadow-xs disabled:opacity-50",
                                targetVersion === 'v2'
                                    ? "bg-red-500 hover:bg-red-600 active:bg-red-700 text-white border-red-500 shadow-sm"
                                    : "bg-white dark:bg-base-200 hover:bg-gray-50 text-gray-700 dark:text-gray-200 border-gray-200 dark:border-base-300"
                            )}
                            disabled={loading || !!syncingVersion || !status}
                            onClick={() => executeSyncWithVersion('v2')}
                            title="适用于 2026.8.1 及以后的最新 OpenClaw 2.0 SDK (采用 modelPolicy.allow 白名单规范)"
                        >
                            <RefreshCw size={11} className={syncingVersion === 'v2' ? 'animate-spin' : ''} />
                            {t('proxy.openclaw_sync.btn_sync_v2', { defaultValue: '同步 (≥ 2026.8.1 v2.0)' })}
                        </button>
                    </div>
                </div>
            </div>
        </div>
    );
}
