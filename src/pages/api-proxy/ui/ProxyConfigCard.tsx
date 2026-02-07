// File: src/pages/api-proxy/ui/ProxyConfigCard.tsx
// Main proxy configuration card

import { useTranslation } from 'react-i18next';
import {
    Power,
    Copy,
    RefreshCw,
    CheckCircle,
    Settings,
    X,
    Edit2
} from 'lucide-react';
import { HelpTooltip } from '@/shared/ui';
import type { AppConfig, ProxyConfig } from '@/entities/config';
import type { ProxyStatus } from '../lib/constants';

interface ProxyConfigCardProps {
    appConfig: AppConfig;
    status: ProxyStatus;
    loading: boolean;
    copied: string | null;
    isEditingApiKey: boolean;
    tempApiKey: string;
    isEditingAdminPassword: boolean;
    tempAdminPassword: string;
    onToggle: () => void;
    onUpdateProxyConfig: (updates: Partial<ProxyConfig>) => void;
    onEditApiKey: () => void;
    onSaveApiKey: () => void;
    onCancelEditApiKey: () => void;
    onGenerateApiKey: () => void;
    onEditAdminPassword: () => void;
    onSaveAdminPassword: () => void;
    onCancelEditAdminPassword: () => void;
    onCopy: (text: string, label: string) => void;
    setTempApiKey: (value: string) => void;
    setTempAdminPassword: (value: string) => void;
}

export function ProxyConfigCard({
    appConfig,
    status,
    loading,
    copied,
    isEditingApiKey,
    tempApiKey,
    isEditingAdminPassword,
    tempAdminPassword,
    onToggle,
    onUpdateProxyConfig,
    onEditApiKey,
    onSaveApiKey,
    onCancelEditApiKey,
    onGenerateApiKey,
    onEditAdminPassword,
    onSaveAdminPassword,
    onCancelEditAdminPassword,
    onCopy,
    setTempApiKey,
    setTempAdminPassword,
}: ProxyConfigCardProps) {
    const { t } = useTranslation();

    return (
        <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200">
            <div className="px-4 py-2.5 border-b border-gray-100 dark:border-base-200 flex items-center justify-between">
                <div className="flex items-center gap-4">
                    <h2 className="text-base font-semibold flex items-center gap-2 text-gray-900 dark:text-base-content">
                        <Settings size={18} />
                        {t('proxy.config.title')}
                    </h2>
                    <div className="flex items-center gap-2 pl-4 border-l border-gray-200 dark:border-base-300">
                        <div className={`w-2 h-2 rounded-full ${status.running ? 'bg-green-500 animate-pulse' : 'bg-gray-400'}`} />
                        <span className={`text-xs font-medium ${status.running ? 'text-green-600' : 'text-gray-500'}`}>
                            {status.running
                                ? `${t('proxy.status.running')} (${status.active_accounts} ${t('common.accounts') || 'Accounts'})`
                                : t('proxy.status.stopped')}
                        </span>
                    </div>
                </div>

                <div className="flex items-center gap-2">
                    <button
                        onClick={onToggle}
                        disabled={loading || !appConfig}
                        className={`px-3 py-1 rounded-lg text-xs font-medium transition-colors flex items-center gap-2 ${status.running
                            ? 'bg-red-50 to-red-600 text-red-600 hover:bg-red-100 border border-red-200'
                            : 'bg-blue-600 hover:bg-blue-700 text-white shadow-sm shadow-blue-500/30'
                            } ${(loading || !appConfig) ? 'opacity-50 cursor-not-allowed' : ''}`}
                    >
                        <Power size={14} />
                        {loading ? t('proxy.status.processing') : (status.running ? t('proxy.action.stop') : t('proxy.action.start'))}
                    </button>
                </div>
            </div>

            <div className="p-3 space-y-3">
                {/* Port, Timeout, Auto-start */}
                <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
                    <div>
                        <label className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1">
                            <span className="inline-flex items-center gap-1">
                                {t('proxy.config.port')}
                                <HelpTooltip text={t('proxy.config.port_tooltip')} ariaLabel={t('proxy.config.port')} placement="right" />
                            </span>
                        </label>
                        <input
                            type="number"
                            value={appConfig.proxy.port}
                            onChange={(e) => onUpdateProxyConfig({ port: parseInt(e.target.value) })}
                            min={8000}
                            max={65535}
                            disabled={status.running}
                            className="w-full px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 text-xs text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50 disabled:cursor-not-allowed"
                        />
                        <p className="mt-0.5 text-[10px] text-gray-500 dark:text-gray-400">
                            {t('proxy.config.port_hint')}
                        </p>
                    </div>
                    <div>
                        <label className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1">
                            <span className="inline-flex items-center gap-1">
                                {t('proxy.config.request_timeout')}
                                <HelpTooltip text={t('proxy.config.request_timeout_tooltip')} ariaLabel={t('proxy.config.request_timeout')} placement="top" />
                            </span>
                        </label>
                        <input
                            type="number"
                            value={appConfig.proxy.request_timeout || 120}
                            onChange={(e) => {
                                const value = parseInt(e.target.value);
                                const timeout = Math.max(30, Math.min(7200, value));
                                onUpdateProxyConfig({ request_timeout: timeout });
                            }}
                            min={30}
                            max={7200}
                            disabled={status.running}
                            className="w-full px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 text-xs text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50 disabled:cursor-not-allowed"
                        />
                        <p className="mt-0.5 text-[10px] text-gray-500 dark:text-gray-400">
                            {t('proxy.config.request_timeout_hint')}
                        </p>
                    </div>
                    <div className="flex items-center">
                        <label className="flex items-center cursor-pointer gap-3">
                            <input
                                type="checkbox"
                                className="toggle toggle-sm bg-gray-200 dark:bg-gray-700 border-gray-300 dark:border-gray-600 checked:bg-blue-500 checked:border-blue-500 disabled:opacity-50 disabled:bg-gray-100 dark:disabled:bg-gray-800"
                                checked={appConfig.proxy.auto_start}
                                onChange={(e) => onUpdateProxyConfig({ auto_start: e.target.checked })}
                            />
                            <span className="text-xs font-medium text-gray-900 dark:text-base-content inline-flex items-center gap-1">
                                {t('proxy.config.auto_start')}
                                <HelpTooltip text={t('proxy.config.auto_start_tooltip')} ariaLabel={t('proxy.config.auto_start')} placement="right" />
                            </span>
                        </label>
                    </div>
                </div>

                {/* LAN Access & Auth */}
                <div className="border-t border-gray-200 dark:border-base-300 pt-3 mt-3">
                    <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                        {/* LAN Access */}
                        <div className="space-y-2">
                            <div className="flex items-center justify-between">
                                <span className="text-xs font-medium text-gray-700 dark:text-gray-300 inline-flex items-center gap-1">
                                    {t('proxy.config.allow_lan_access')}
                                    <HelpTooltip text={t('proxy.config.allow_lan_access_tooltip')} ariaLabel={t('proxy.config.allow_lan_access')} placement="right" />
                                </span>
                                <input
                                    type="checkbox"
                                    className="toggle toggle-sm bg-gray-200 dark:bg-gray-700 border-gray-300 dark:border-gray-600 checked:bg-blue-500 checked:border-blue-500"
                                    checked={appConfig.proxy.allow_lan_access || false}
                                    onChange={(e) => onUpdateProxyConfig({ allow_lan_access: e.target.checked })}
                                />
                            </div>
                            <p className="text-[10px] text-gray-500 dark:text-gray-400">
                                {(appConfig.proxy.allow_lan_access || false)
                                    ? t('proxy.config.allow_lan_access_hint_enabled')
                                    : t('proxy.config.allow_lan_access_hint_disabled')}
                            </p>
                            {(appConfig.proxy.allow_lan_access || false) && (
                                <p className="text-[10px] text-amber-600 dark:text-amber-500">
                                    {t('proxy.config.allow_lan_access_warning')}
                                </p>
                            )}
                            {status.running && (
                                <p className="text-[10px] text-blue-600 dark:text-blue-400">
                                    {t('proxy.config.allow_lan_access_restart_hint')}
                                </p>
                            )}
                        </div>

                        {/* Auth */}
                        <div className="space-y-2">
                            <div className="flex items-center justify-between">
                                <label className="text-xs font-medium text-gray-700 dark:text-gray-300">
                                    <span className="inline-flex items-center gap-1">
                                        {t('proxy.config.auth.title')}
                                        <HelpTooltip text={t('proxy.config.auth.title_tooltip')} ariaLabel={t('proxy.config.auth.title')} placement="top" />
                                    </span>
                                </label>
                                <label className="flex items-center cursor-pointer gap-2">
                                    <span className="text-[11px] text-gray-600 dark:text-gray-400 inline-flex items-center gap-1">
                                        {(appConfig.proxy.auth_mode || 'off') !== 'off' ? t('proxy.config.auth.enabled') : t('common.disabled')}
                                        <HelpTooltip text={t('proxy.config.auth.enabled_tooltip')} ariaLabel={t('proxy.config.auth.enabled')} placement="left" />
                                    </span>
                                    <input
                                        type="checkbox"
                                        className="toggle toggle-sm bg-gray-200 dark:bg-gray-700 border-gray-300 dark:border-gray-600 checked:bg-blue-500 checked:border-blue-500 disabled:opacity-50 disabled:bg-gray-100 dark:disabled:bg-gray-800"
                                        checked={(appConfig.proxy.auth_mode || 'off') !== 'off'}
                                        onChange={(e) => {
                                            const nextMode = e.target.checked ? 'all_except_health' : 'off';
                                            onUpdateProxyConfig({ auth_mode: nextMode });
                                        }}
                                    />
                                </label>
                            </div>

                            <div>
                                <label className="block text-[11px] text-gray-600 dark:text-gray-400 mb-1">
                                    <span className="inline-flex items-center gap-1">
                                        {t('proxy.config.auth.mode')}
                                        <HelpTooltip text={t('proxy.config.auth.mode_tooltip')} ariaLabel={t('proxy.config.auth.mode')} placement="top" />
                                    </span>
                                </label>
                                <select
                                    value={appConfig.proxy.auth_mode || 'off'}
                                    onChange={(e) => onUpdateProxyConfig({ auth_mode: e.target.value as ProxyConfig['auth_mode'] })}
                                    className="w-full px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 text-xs text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                                >
                                    <option value="off">{t('proxy.config.auth.modes.off')}</option>
                                    <option value="strict">{t('proxy.config.auth.modes.strict')}</option>
                                    <option value="all_except_health">{t('proxy.config.auth.modes.all_except_health')}</option>
                                    <option value="auto">{t('proxy.config.auth.modes.auto')}</option>
                                </select>
                                <p className="mt-0.5 text-[10px] text-gray-500 dark:text-gray-400">
                                    {t('proxy.config.auth.hint')}
                                </p>
                            </div>
                        </div>
                    </div>
                </div>

                {/* API Key */}
                <div>
                    <label className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1">
                        <span className="inline-flex items-center gap-1">
                            {t('proxy.config.api_key')}
                            <HelpTooltip text={t('proxy.config.api_key_tooltip')} ariaLabel={t('proxy.config.api_key')} placement="right" />
                        </span>
                    </label>
                    <div className="flex gap-2">
                        <input
                            type="text"
                            value={isEditingApiKey ? tempApiKey : appConfig.proxy.api_key}
                            onChange={(e) => isEditingApiKey && setTempApiKey(e.target.value)}
                            readOnly={!isEditingApiKey}
                            className={`flex-1 px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg text-xs font-mono ${isEditingApiKey
                                ? 'bg-white dark:bg-base-200 text-gray-900 dark:text-base-content'
                                : 'bg-gray-50 dark:bg-base-300 text-gray-600 dark:text-gray-400'
                                }`}
                        />
                        {isEditingApiKey ? (
                            <>
                                <button onClick={onSaveApiKey} className="px-2.5 py-1.5 border border-green-300 dark:border-green-700 rounded-lg bg-green-50 dark:bg-green-900/20 hover:bg-green-100 dark:hover:bg-green-900/30 transition-colors text-green-600 dark:text-green-400" title={t('proxy.config.btn_save')}>
                                    <CheckCircle size={14} />
                                </button>
                                <button onClick={onCancelEditApiKey} className="px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors" title={t('common.cancel')}>
                                    <X size={14} />
                                </button>
                            </>
                        ) : (
                            <>
                                <button onClick={onEditApiKey} className="px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors" title={t('proxy.config.btn_edit')}>
                                    <Edit2 size={14} />
                                </button>
                                <button onClick={onGenerateApiKey} className="px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors" title={t('proxy.config.btn_regenerate')}>
                                    <RefreshCw size={14} />
                                </button>
                                <button onClick={() => onCopy(appConfig.proxy.api_key, 'api_key')} className="px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors" title={t('proxy.config.btn_copy')}>
                                    {copied === 'api_key' ? <CheckCircle size={14} className="text-green-500" /> : <Copy size={14} />}
                                </button>
                            </>
                        )}
                    </div>
                    <p className="mt-0.5 text-[10px] text-amber-600 dark:text-amber-500">
                        {t('proxy.config.warning_key')}
                    </p>
                </div>

                {/* Admin Password */}
                <div className="border-t border-gray-200 dark:border-base-300 pt-3 mt-3">
                    <label className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1">
                        <span className="inline-flex items-center gap-1">
                            {t('proxy.config.admin_password', { defaultValue: 'Web UI Login Password' })}
                            <HelpTooltip text={t('proxy.config.admin_password_tooltip', { defaultValue: 'Used for logging into the Web Management Console. If empty, it defaults to the API Key.' })} ariaLabel={t('proxy.config.admin_password')} placement="right" />
                        </span>
                    </label>
                    <div className="flex gap-2">
                        <input
                            type="text"
                            value={isEditingAdminPassword ? tempAdminPassword : (appConfig.proxy.admin_password || t('proxy.config.admin_password_default', { defaultValue: '(Same as API Key)' }))}
                            onChange={(e) => isEditingAdminPassword && setTempAdminPassword(e.target.value)}
                            readOnly={!isEditingAdminPassword}
                            placeholder={t('proxy.config.admin_password_placeholder', { defaultValue: 'Enter new password or leave empty to use API Key' })}
                            className={`flex-1 px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg text-xs font-mono ${isEditingAdminPassword
                                ? 'bg-white dark:bg-base-200 text-gray-900 dark:text-base-content'
                                : 'bg-gray-50 dark:bg-base-300 text-gray-600 dark:text-gray-400'
                                }`}
                        />
                        {isEditingAdminPassword ? (
                            <>
                                <button onClick={onSaveAdminPassword} className="px-2.5 py-1.5 border border-green-300 dark:border-green-700 rounded-lg bg-green-50 dark:bg-green-900/20 hover:bg-green-100 dark:hover:bg-green-900/30 transition-colors text-green-600 dark:text-green-400" title={t('proxy.config.btn_save')}>
                                    <CheckCircle size={14} />
                                </button>
                                <button onClick={onCancelEditAdminPassword} className="px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors" title={t('common.cancel')}>
                                    <X size={14} />
                                </button>
                            </>
                        ) : (
                            <>
                                <button onClick={onEditAdminPassword} className="px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors" title={t('proxy.config.btn_edit')}>
                                    <Edit2 size={14} />
                                </button>
                                <button onClick={() => onCopy(appConfig.proxy.admin_password || appConfig.proxy.api_key, 'admin_password')} className="px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors" title={t('proxy.config.btn_copy')}>
                                    {copied === 'admin_password' ? <CheckCircle size={14} className="text-green-500" /> : <Copy size={14} />}
                                </button>
                            </>
                        )}
                    </div>
                    <p className="mt-0.5 text-[10px] text-gray-500 dark:text-gray-400">
                        {t('proxy.config.admin_password_hint', { defaultValue: 'For safety in Docker/Browser environments, you can set a separate login password from your API Key.' })}
                    </p>
                </div>
            </div>
        </div>
    );
}
