import { useState, useEffect } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { X, RefreshCw, Link2, Unlink } from 'lucide-react';
import { request } from '../../../utils/request';
import { showToast } from '../../common/ToastContainer';
import { useAccountStore } from '../../../stores/useAccountStore';
import { useConfigStore } from '../../../stores/useConfigStore';
import { ProxyEntry } from '../../../types/config';

interface ProxyBindingManagerProps {
    isOpen: boolean;
    onClose: () => void;
    proxies: ProxyEntry[];
}

export default function ProxyBindingManager({ isOpen, onClose, proxies }: ProxyBindingManagerProps) {
    const { t } = useTranslation();
    const { accounts, fetchAccounts } = useAccountStore();
    const [bindings, setBindings] = useState<Record<string, string>>({});
    const [isLoading, setIsLoading] = useState(false);
    const [isSaving, setIsSaving] = useState<string | null>(null);

    // Filter enabled proxies for selection
    const availableProxies = proxies.filter(p => p.enabled);

    useEffect(() => {
        if (isOpen) {
            refreshData();
        }
    }, [isOpen]);

    const refreshData = async () => {
        setIsLoading(true);
        try {
            await fetchAccounts();
            const currentBindings = await request<Record<string, string>>('get_all_account_bindings');
            setBindings(currentBindings || {});
        } catch (error) {
            console.error('Failed to load bindings:', error);
            showToast(t('settings.proxy_pool.binding.load_failed', 'Failed to load bindings'), 'error');
        } finally {
            setIsLoading(false);
        }
    };

    const handleBind = async (accountId: string, proxyId: string) => {
        setIsSaving(accountId);
        try {
            if (proxyId === '') {
                // Unbind
                await request('unbind_account_proxy', { accountId });
                const newBindings = { ...bindings };
                delete newBindings[accountId];
                setBindings(newBindings);
                useConfigStore.getState().syncConfigLocal(cfg =>
                    cfg.proxy.proxy_pool
                        ? {
                              ...cfg,
                              proxy: {
                                  ...cfg.proxy,
                                  proxy_pool: { ...cfg.proxy.proxy_pool, account_bindings: newBindings },
                              },
                          }
                        : cfg
                );
                showToast(t('settings.proxy_pool.binding.unbind_success', 'Unbound successfully'), 'success');
            } else {
                // Bind
                await request('bind_account_proxy', { accountId, proxyId });
                const newBindings = { ...bindings, [accountId]: proxyId };
                setBindings(newBindings);
                useConfigStore.getState().syncConfigLocal(cfg =>
                    cfg.proxy.proxy_pool
                        ? {
                              ...cfg,
                              proxy: {
                                  ...cfg.proxy,
                                  proxy_pool: { ...cfg.proxy.proxy_pool, account_bindings: newBindings },
                              },
                          }
                        : cfg
                );
                showToast(t('settings.proxy_pool.binding.bind_success', 'Bound successfully'), 'success');
            }
        } catch (error) {
            console.error('Failed to update binding:', error);
            showToast(t('settings.proxy_pool.binding.update_failed', 'Failed to update binding'), 'error');
        } finally {
            setIsSaving(null);
        }
    };

    if (!isOpen) return null;

    return createPortal(
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/50 backdrop-blur-sm animate-in fade-in duration-200">
            <div className="bg-white dark:bg-gray-800 rounded-xl shadow-xl w-full max-w-2xl max-h-[80vh] flex flex-col">

                {/* Header */}
                <div className="flex items-center justify-between p-4 border-b border-gray-200 dark:border-gray-700">
                    <h3 className="text-lg font-semibold text-gray-900 dark:text-white flex items-center gap-2">
                        <Link2 className="w-5 h-5" />
                        {t('settings.proxy_pool.binding.title', 'Account Proxy Bindings')}
                    </h3>
                    <div className="flex items-center gap-2">
                        <button
                            onClick={refreshData}
                            disabled={isLoading}
                            className="p-2 text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
                            title={t('common.refresh', 'Refresh')}
                        >
                            <RefreshCw size={18} className={isLoading ? 'animate-spin' : ''} />
                        </button>
                        <button
                            onClick={onClose}
                            className="p-2 text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
                        >
                            <X size={20} />
                        </button>
                    </div>
                </div>

                {/* Content */}
                <div className="flex-1 overflow-y-auto p-4 custom-scrollbar">
                    {isLoading && accounts.length === 0 ? (
                        <div className="flex justify-center py-8">
                            <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-600"></div>
                        </div>
                    ) : (
                        <div className="space-y-3">
                            <div className="grid grid-cols-12 gap-4 px-3 py-2 text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                                <div className="col-span-5">{t('settings.account.email', 'Email')}</div>
                                <div className="col-span-7">{t('settings.proxy_pool.binding.assigned_proxy', 'Assigned Proxy')}</div>
                            </div>

                            {accounts.map(account => {
                                const currentProxyId = bindings[account.id] || '';
                                return (
                                    <div key={account.id} className="grid grid-cols-12 gap-4 items-center p-3 bg-gray-50 dark:bg-gray-700/50 rounded-lg border border-gray-200 dark:border-gray-700 hover:border-gray-300 dark:hover:border-gray-600 transition-colors">
                                        <div className="col-span-5 truncate font-medium text-gray-900 dark:text-gray-200" title={account.email}>
                                            {account.email}
                                        </div>
                                        <div className="col-span-7 relative">
                                            <select
                                                value={currentProxyId}
                                                onChange={(e) => handleBind(account.id, e.target.value)}
                                                disabled={isSaving === account.id}
                                                className={`w-full appearance-none pl-3 pr-8 py-2 border rounded-md text-sm bg-white dark:bg-gray-800 text-gray-900 dark:text-white transition-colors
                                                    ${bindings[account.id]
                                                        ? 'border-blue-300 dark:border-blue-700 ring-1 ring-blue-100 dark:ring-blue-900/20'
                                                        : 'border-gray-300 dark:border-gray-600'
                                                    }`}
                                            >
                                                <option value="">{t('settings.proxy_pool.binding.default_strategy', 'Default (Follow Strategy)')}</option>
                                                <optgroup label={t('settings.proxy_pool.proxies', 'Proxies')}>
                                                    {availableProxies.map(proxy => (
                                                        <option key={proxy.id} value={proxy.id}>
                                                            {proxy.name || proxy.url}
                                                        </option>
                                                    ))}
                                                </optgroup>
                                            </select>
                                            <div className="absolute inset-y-0 right-0 flex items-center pr-2 pointer-events-none text-gray-500">
                                                {isSaving === account.id ? (
                                                    <div className="animate-spin h-4 w-4 border-2 border-gray-500 border-t-transparent rounded-full" />
                                                ) : (
                                                    bindings[account.id] ? <Link2 size={16} className="text-blue-500" /> : <Unlink size={16} className="opacity-50" />
                                                )}
                                            </div>
                                        </div>
                                    </div>
                                );
                            })}

                            {accounts.length === 0 && (
                                <div className="text-center py-8 text-gray-500 dark:text-gray-400">
                                    {t('settings.account.no_accounts', 'No accounts found')}
                                </div>
                            )}
                        </div>
                    )}
                </div>

                {/* Footer */}
                <div className="p-4 border-t border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-800/50 rounded-b-xl flex justify-end">
                    <button
                        onClick={onClose}
                        className="px-4 py-2 bg-white dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded-lg text-sm font-medium text-gray-700 dark:text-gray-200 hover:bg-gray-50 dark:hover:bg-gray-600 transition-colors"
                    >
                        {t('common.close', 'Close')}
                    </button>
                </div>
            </div>
        </div>,
        document.body
    );
}
