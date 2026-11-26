import { useState, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { request } from '../../utils/request';
import { showToast } from '../common/ToastContainer';
import { Plus, Network, Upload, RefreshCw, Link2, SlidersHorizontal, Trash2, Power } from 'lucide-react';
import { ProxyPoolConfig, ProxyEntry } from '../../types/config';
import ProxyList from './proxy/ProxyList';
import ProxyEditModal from './proxy/ProxyEditModal';
import BatchImportModal from './proxy/BatchImportModal';
import ProxyBindingManager from './proxy/ProxyBindingManager';
import { useAccountStore } from '../../stores/useAccountStore';

interface ProxyPoolSettingsProps {
    config: ProxyPoolConfig;
    onChange: (config: ProxyPoolConfig, silent?: boolean) => void;
}

export default function ProxyPoolSettings({ config, onChange }: ProxyPoolSettingsProps) {
    const { t } = useTranslation();
    const { accounts, fetchAccounts } = useAccountStore();
    const [isAddModalOpen, setIsAddModalOpen] = useState(false);
    const [isBatchImportOpen, setIsBatchImportOpen] = useState(false);
    const [isBindingManagerOpen, setIsBindingManagerOpen] = useState(false);
    const [isTesting, setIsTesting] = useState(false);
    const [accountBindings, setAccountBindings] = useState<Record<string, string>>({});
    const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
    const configRef = useRef(config);
    configRef.current = config;
    const onChangeRef = useRef(onChange);
    onChangeRef.current = onChange;

    // Fetch bindings and accounts on mount
    useEffect(() => {
        fetchBindings();
        fetchAccounts();
    }, []);

    // Refresh bindings when manager closes
    useEffect(() => {
        if (!isBindingManagerOpen) {
            fetchBindings();
        }
    }, [isBindingManagerOpen]);

    // [FIX] Polling for proxy pool status
    // Only updates volatile status (is_healthy, latency) from latest configRef to avoid clobbering concurrent edits
    useEffect(() => {
        if (!config.enabled) return;

        const pollStatus = async () => {
            if (document.visibilityState === 'hidden') return;
            try {
                const liveConfig = await request<ProxyPoolConfig>('get_proxy_pool_config');
                if (liveConfig && liveConfig.proxies) {
                    const latestConfig = configRef.current;
                    const liveMap = new Map(liveConfig.proxies.map(p => [p.id, p]));

                    let hasChanges = false;
                    const updatedProxies = latestConfig.proxies.map(p => {
                        const live = liveMap.get(p.id);
                        if (live && (live.is_healthy !== p.is_healthy || live.latency !== p.latency || live.last_check_time !== p.last_check_time)) {
                            hasChanges = true;
                            return { ...p, is_healthy: live.is_healthy, latency: live.latency, last_check_time: live.last_check_time };
                        }
                        return p;
                    });

                    if (hasChanges) {
                        onChangeRef.current({ ...latestConfig, proxies: updatedProxies }, true);
                    }
                }
            } catch (e) {
                console.error('Failed to poll proxy pool config:', e);
            }
        };

        const interval = setInterval(pollStatus, 5000);
        return () => clearInterval(interval);
    }, [config.enabled]);

    const fetchBindings = async () => {
        try {
            const bindings = await request<Record<string, string>>('get_all_account_bindings');
            if (bindings) setAccountBindings(bindings);
        } catch (e) {
            console.error('Fetch bindings failed:', e);
        }
    };

    const safeConfig: ProxyPoolConfig = {
        enabled: config?.enabled ?? false,
        proxies: config?.proxies ?? [],
        health_check_interval: config?.health_check_interval ?? 300,
        auto_failover: config?.auto_failover ?? true,
        strategy: config?.strategy ?? 'priority',
        strict_mode: config?.strict_mode ?? false,
        account_bindings: config?.account_bindings ?? {},
    };

    const handleUpdateProxies = (proxies: ProxyEntry[]) => {
        onChange({ ...safeConfig, proxies });
    };

    const handleAddProxy = (entry: ProxyEntry) => {
        onChange({
            ...safeConfig,
            proxies: [...safeConfig.proxies, entry]
        });
    };

    const handleBatchImport = async (newProxies: ProxyEntry[]) => {
        const updatedProxies = [...safeConfig.proxies, ...newProxies];
        await onChange({
            ...safeConfig,
            proxies: updatedProxies
        });

        // Auto-trigger test after import is fully committed
        handleTestAll();
    };

    const handleBatchDelete = () => {
        if (selectedIds.size === 0) return;
        if (confirm(t('settings.proxy_pool.confirm_batch_delete', 'Are you sure you want to delete selected proxies?'))) {
            const newProxies = safeConfig.proxies.filter(p => !selectedIds.has(p.id));
            onChange({ ...safeConfig, proxies: newProxies });
            setSelectedIds(new Set());
            showToast(t('common.deleted', 'Deleted successfully'), 'success');
        }
    };

    const handleBatchToggleEnabled = (enabled: boolean) => {
        if (selectedIds.size === 0) return;
        const newProxies = safeConfig.proxies.map(p =>
            selectedIds.has(p.id) ? { ...p, enabled } : p
        );
        onChange({ ...safeConfig, proxies: newProxies });
        showToast(t(enabled ? 'common.enabled' : 'common.disabled', enabled ? 'Enabled' : 'Disabled'), 'success');
    };

    const handleTestAll = async () => {
        setIsTesting(true);
        try {
            const liveConfig = await request<ProxyPoolConfig>('check_proxy_health');
            if (liveConfig && liveConfig.proxies) {
                // [FIX] Use incremental merge to prevent race condition rollbacks
                const liveMap = new Map(liveConfig.proxies.map(p => [p.id, p]));

                const updatedProxies = config.proxies.map(p => {
                    const live = liveMap.get(p.id);
                    if (live) {
                        return {
                            ...p,
                            is_healthy: live.is_healthy,
                            latency: live.latency,
                            last_check_time: live.last_check_time
                        };
                    }
                    return p;
                });

                // Update local UI state silently (syncing health stats only)
                onChange({ ...config, proxies: updatedProxies }, true);
            }
            showToast(t('settings.proxy_pool.test_completed', 'Health check completed'), 'success');
        } catch (error) {
            console.error('Test all failed:', error);
            showToast(t('settings.proxy_pool.test_failed', 'Health check failed'), 'error');
        } finally {
            setIsTesting(false);
        }
    };

    return (
        <div className="space-y-3">
            {/* Consolidated Header & Toolbar */}
            <div className="flex flex-wrap items-center justify-between gap-4 p-1.5 bg-gray-100/30 dark:bg-gray-800/20 border border-gray-200 dark:border-gray-800/70 rounded-2xl">
                {/* Left: Component Identity & Feature Toggle */}
                <div className="flex items-center gap-2 p-1.5 bg-white dark:bg-gray-900 border border-gray-200/50 dark:border-gray-800 shadow-sm rounded-xl">
                    <div className="flex items-center gap-3 pr-3 border-r border-gray-100 dark:border-gray-800">
                        <div className="p-1.5 bg-blue-600 text-white rounded-lg shadow-blue-500/20 shadow-lg">
                            <Network className="w-3.5 h-3.5" />
                        </div>
                        <h3 className="text-xs font-black text-gray-900 dark:text-white whitespace-nowrap uppercase tracking-wider">
                            {t('settings.proxy_pool.title', 'Proxy Pool')}
                        </h3>
                        <label className="relative inline-flex items-center cursor-pointer ml-1">
                            <input
                                type="checkbox"
                                checked={safeConfig.enabled}
                                onChange={e => onChange({ ...safeConfig, enabled: e.target.checked })}
                                className="sr-only peer"
                            />
                            <div className="w-9 h-5 bg-gray-200 peer-focus:outline-none rounded-full peer dark:bg-gray-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all dark:border-gray-600 peer-checked:bg-blue-600"></div>
                        </label>
                    </div>

                    {/* Middle: Configuration Parameters (Strategy & Interval) */}
                    <div className="flex items-center gap-4 px-2">

                        <div className="flex items-center gap-2 group">
                            <SlidersHorizontal size={12} className="text-gray-400 group-hover:text-blue-500 transition-colors" />
                            <select
                                value={safeConfig.strategy}
                                onChange={e => onChange({ ...safeConfig, strategy: e.target.value as any })}
                                className="text-[10px] bg-transparent border-none p-0 pr-6 focus:ring-0 font-black uppercase tracking-tight text-gray-700 dark:text-gray-300 cursor-pointer hover:text-blue-600 dark:hover:text-blue-400 transition-colors"
                            >
                                <option value="priority">{t('settings.proxy_pool.strategy_priority', 'Priority')}</option>
                                <option value="round_robin">{t('settings.proxy_pool.strategy_round_robin', 'Round Robin')}</option>
                                <option value="random">{t('settings.proxy_pool.strategy_random', 'Random')}</option>
                                <option value="least_connections">{t('settings.proxy_pool.strategy_least_connections', 'Least Connections')}</option>
                            </select>
                        </div>

                        <div className="flex items-center gap-2 border-l border-gray-100 dark:border-gray-800 pl-4 group">
                            <RefreshCw size={12} className="text-gray-400 group-hover:text-emerald-500 transition-colors" />
                            <div className="flex items-center gap-1">
                                <input
                                    type="number"
                                    defaultValue={safeConfig.health_check_interval}
                                    onBlur={e => {
                                        const val = parseInt(e.target.value) || 60;
                                        if (val !== safeConfig.health_check_interval) {
                                            onChange({ ...safeConfig, health_check_interval: val });
                                        }
                                    }}
                                    className="w-10 text-[10px] bg-transparent border-none p-0 focus:ring-0 font-black text-gray-700 dark:text-gray-300 text-right group-hover:text-emerald-600 transition-colors"
                                />
                                <span className="text-[9px] text-gray-400 font-black uppercase tracking-tighter">{t('settings.proxy_pool.seconds', 'Sec')}</span>
                            </div>
                        </div>

                        <div className="flex items-center border-l border-gray-100 dark:border-gray-800 pl-4">
                            <button
                                type="button"
                                role="switch"
                                aria-checked={Boolean(safeConfig.strict_mode)}
                                onClick={() => onChange({ ...safeConfig, strict_mode: !safeConfig.strict_mode })}
                                title={t('settings.proxy_pool.strict_mode_desc', 'When enabled, unavailable or unbound proxies fail explicitly instead of falling back to direct connection (env: ABV_PROXY_POOL_STRICT_MODE).')}
                                className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-[10px] font-bold transition-all cursor-pointer border ${
                                    safeConfig.strict_mode
                                        ? 'bg-amber-500/15 text-amber-700 dark:text-amber-300 border-amber-500/30 hover:bg-amber-500/20'
                                        : 'bg-gray-100 dark:bg-gray-800 text-gray-500 dark:text-gray-400 border-gray-200 dark:border-gray-700 hover:bg-gray-200/70 dark:hover:bg-gray-700'
                                }`}
                            >
                                <span
                                    className={`w-1.5 h-1.5 rounded-full ${
                                        safeConfig.strict_mode ? 'bg-amber-500' : 'bg-gray-400 dark:bg-gray-500'
                                    }`}
                                />
                                {t('settings.proxy_pool.strict_mode', 'Strict Mode')}
                            </button>
                        </div>
                    </div>
                </div>

                {/* Right: Actions or Selection Toolbar */}
                <div className="flex items-center gap-2">
                    {selectedIds.size > 0 ? (
                        <div className="flex items-center gap-1.5 bg-blue-50/50 dark:bg-blue-900/20 px-2.5 py-1.5 rounded-xl border border-blue-100 dark:border-blue-800/50 animate-in zoom-in-95 duration-200">
                            <span className="text-[10px] font-black text-blue-700 dark:text-blue-300 mr-2 uppercase tracking-tight">
                                {selectedIds.size} {t('common.selected', 'Selected')}
                            </span>
                            <div className="flex items-center gap-1">
                                <button
                                    onClick={() => handleBatchToggleEnabled(true)}
                                    className="p-1.5 text-blue-600 hover:bg-white dark:hover:bg-blue-800 rounded-lg transition-all shadow-sm hover:shadow active:scale-90"
                                    title={t('common.enable', 'Enable')}
                                >
                                    <Power size={14} />
                                </button>
                                <button
                                    onClick={() => handleBatchToggleEnabled(false)}
                                    className="p-1.5 text-gray-400 hover:bg-white dark:hover:bg-gray-800 rounded-lg transition-all shadow-sm hover:shadow active:scale-90"
                                    title={t('common.disable', 'Disable')}
                                >
                                    <Power size={14} className="opacity-50" />
                                </button>
                                <button
                                    onClick={handleBatchDelete}
                                    className="p-1.5 text-rose-500 hover:bg-white dark:hover:bg-rose-900/40 rounded-lg transition-all shadow-sm hover:shadow active:scale-90"
                                    title={t('common.delete', 'Delete')}
                                >
                                    <Trash2 size={14} />
                                </button>
                            </div>
                            <div className="w-px h-4 bg-blue-200 dark:bg-blue-800 mx-1"></div>
                            <button
                                onClick={() => setSelectedIds(new Set())}
                                className="text-[10px] font-black text-blue-600 dark:text-blue-400 hover:text-blue-700 dark:hover:text-blue-300 uppercase px-2 py-1"
                            >
                                {t('common.cancel', 'Cancel')}
                            </button>
                        </div>
                    ) : (
                        <>
                            <div className="flex items-center gap-1 p-1 bg-gray-50 dark:bg-gray-800/50 rounded-xl border border-gray-100 dark:border-gray-800/50">
                                <button
                                    onClick={handleTestAll}
                                    disabled={isTesting}
                                    className={`p-2 text-gray-400 hover:text-emerald-500 hover:bg-white dark:hover:bg-gray-800 rounded-lg transition-all ${isTesting ? 'animate-spin opacity-50' : 'active:scale-90'}`}
                                    title={t('settings.proxy_pool.test_all', 'Test All')}
                                >
                                    <RefreshCw size={14} />
                                </button>
                                <button
                                    onClick={() => setIsBindingManagerOpen(true)}
                                    className="p-2 text-gray-400 hover:text-indigo-500 hover:bg-white dark:hover:bg-gray-800 rounded-lg transition-all active:scale-90"
                                    title={t('settings.proxy_pool.binding_manager', 'Manage Bindings')}
                                >
                                    <Link2 size={14} />
                                </button>
                            </div>

                            <div className="flex items-center gap-2 ml-1">
                                <button
                                    onClick={() => setIsBatchImportOpen(true)}
                                    className="flex items-center gap-1.5 px-3.5 py-2 text-[11px] font-black uppercase tracking-wider text-gray-600 hover:text-gray-900 dark:text-gray-400 dark:hover:text-white bg-gray-50 hover:bg-white dark:bg-gray-800/50 dark:hover:bg-gray-800 border border-gray-100 dark:border-gray-800/50 rounded-xl transition-all shadow-sm hover:shadow active:scale-95"
                                >
                                    <Upload size={12} />
                                    {t('settings.proxy_pool.batch_import', 'Import')}
                                </button>
                                <button
                                    onClick={() => setIsAddModalOpen(true)}
                                    className="flex items-center gap-1.5 px-4 py-2 text-[11px] font-black uppercase tracking-wider bg-gray-900 hover:bg-black dark:bg-white dark:hover:bg-gray-100 text-white dark:text-gray-900 rounded-xl transition-all shadow-lg hover:shadow-black/20 dark:hover:shadow-white/10 active:scale-95"
                                >
                                    <Plus size={12} />
                                    {t('settings.proxy_pool.add_proxy', 'Add')}
                                </button>
                            </div>
                        </>
                    )}
                </div>
            </div>

            {/* Proxy List - Always visible, with status context */}
            <div className="relative border border-gray-200 dark:border-gray-800 rounded-xl overflow-hidden bg-white dark:bg-gray-900 shadow-sm transition-all duration-300">
                {!safeConfig.enabled && (
                    <div className="absolute inset-x-0 top-0 z-10 bg-amber-50/80 dark:bg-amber-900/10 backdrop-blur-[2px] px-3 py-1 flex items-center justify-center border-b border-amber-100/50 dark:border-amber-900/20">
                        <span className="text-[10px] font-black text-amber-600/80 dark:text-amber-500/80 uppercase tracking-[0.2em] pointer-events-none">
                            {t('settings.proxy_pool.inactive_notice', 'Proxy Pool Inactive')}
                        </span>
                    </div>
                )}
                <div className={!safeConfig.enabled ? 'pt-6 opacity-60' : ''}>
                    <ProxyList
                        proxies={safeConfig.proxies}
                        onUpdate={handleUpdateProxies}
                        accountBindings={accountBindings}
                        accounts={accounts}
                        selectedIds={selectedIds}
                        onSelectionChange={setSelectedIds}
                        isTesting={isTesting}
                    />
                </div>
            </div>

            {isAddModalOpen && (
                <ProxyEditModal
                    isOpen={isAddModalOpen}
                    onClose={() => setIsAddModalOpen(false)}
                    onSave={handleAddProxy}
                    isEditing={false}
                />
            )}

            {isBatchImportOpen && (
                <BatchImportModal
                    isOpen={isBatchImportOpen}
                    onClose={() => setIsBatchImportOpen(false)}
                    onImport={handleBatchImport}
                />
            )}

            {isBindingManagerOpen && (
                <ProxyBindingManager
                    isOpen={isBindingManagerOpen}
                    onClose={() => setIsBindingManagerOpen(false)}
                    proxies={safeConfig.proxies}
                />
            )}
        </div>
    );
}
