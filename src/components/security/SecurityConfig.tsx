import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { request as invoke } from '../../utils/request';
import { Save, AlertTriangle, Shield, ShieldCheck } from 'lucide-react';
import { showToast } from '../common/ToastContainer';
import { useConfigStore } from '../../stores/useConfigStore';

interface IpBlacklistConfig {
    enabled: boolean;
    block_message: string;
}

interface IpWhitelistConfig {
    enabled: boolean;
    whitelist_priority: boolean;
}

interface SecurityMonitorConfig {
    blacklist: IpBlacklistConfig;
    whitelist: IpWhitelistConfig;
}

export const SecurityConfig: React.FC = () => {
    const { t } = useTranslation();
    const [config, setConfig] = useState<SecurityMonitorConfig | null>(null);
    const [loading, setLoading] = useState(false);
    const [saving, setSaving] = useState(false);

    useEffect(() => {
        loadConfig();
    }, []);

    const loadConfig = async () => {
        setLoading(true);
        try {
            const data = await invoke<SecurityMonitorConfig>('get_security_config');
            setConfig(data);
        } catch (e) {
            console.error('Failed to load security config', e);
            showToast(t('security.config.load_error'), 'error');
        } finally {
            setLoading(false);
        }
    };

    const handleSave = async () => {
        if (!config) return;
        setSaving(true);
        try {
            await invoke('update_security_config', { config });
            await useConfigStore.getState().loadConfig();
            showToast(t('security.config.save_success'), 'success');
        } catch (e) {
            console.error('Failed to save security config', e);
            showToast(t('security.config.save_error'), 'error');
        } finally {
            setSaving(false);
        }
    };

    if (loading) {
        return <div className="p-10 text-center"><span className="loading loading-spinner"></span></div>;
    }

    if (!config) {
        return <div className="p-10 text-center text-error">{t('security.config.load_error')}</div>;
    }

    return (
        <div className="p-6 max-w-4xl mx-auto space-y-8 h-full overflow-y-auto">
            <div className="flex items-center justify-between">
                <h2 className="text-xl font-bold">{t('security.config.title')}</h2>
                <button
                    onClick={handleSave}
                    className="btn btn-primary gap-2"
                    disabled={saving}
                >
                    {saving ? <span className="loading loading-spinner loading-xs"></span> : <Save size={18} />}
                    {saving ? t('security.config.saving') : t('security.config.save')}
                </button>
            </div>

            {/* Blacklist Settings */}
            <div className="card bg-base-100 border border-gray-200 dark:border-base-300 shadow-sm">
                <div className="card-body">
                    <h3 className="card-title flex items-center gap-2 text-red-500">
                        <Shield size={24} />
                        {t('security.config.blacklist_title')}
                    </h3>
                    <p className="text-sm text-gray-500 mb-4">{t('security.config.blacklist_desc')}</p>

                    <div className="form-control">
                        <label className="label cursor-pointer justify-start gap-4">
                            <input
                                type="checkbox"
                                className="toggle toggle-error"
                                checked={config.blacklist.enabled}
                                onChange={(e) => setConfig({
                                    ...config,
                                    blacklist: { ...config.blacklist, enabled: e.target.checked }
                                })}
                            />
                            <span className="label-text font-medium">{t('security.config.enable_blacklist')}</span>
                        </label>
                    </div>

                    <div className="form-control w-full mt-4">
                        <label className="label">
                            <span className="label-text">{t('security.config.block_msg_label')}</span>
                        </label>
                        <input
                            type="text"
                            className="input input-bordered w-full"
                            value={config.blacklist.block_message}
                            onChange={(e) => setConfig({
                                ...config,
                                blacklist: { ...config.blacklist, block_message: e.target.value }
                            })}
                        />
                        <label className="label">
                            <span className="label-text-alt text-gray-400">{t('security.config.block_msg_desc')}</span>
                        </label>
                    </div>
                </div>
            </div>

            {/* Whitelist Settings */}
            <div className="card bg-base-100 border border-gray-200 dark:border-base-300 shadow-sm">
                <div className="card-body">
                    <h3 className="card-title flex items-center gap-2 text-green-500">
                        <ShieldCheck size={24} />
                        {t('security.config.whitelist_title')}
                    </h3>
                    <p className="text-sm text-gray-500 mb-4">{t('security.config.whitelist_desc')}</p>

                    <div className="form-control">
                        <label className="label cursor-pointer justify-start gap-4">
                            <input
                                type="checkbox"
                                className="toggle toggle-success"
                                checked={config.whitelist.enabled}
                                onChange={(e) => setConfig({
                                    ...config,
                                    whitelist: { ...config.whitelist, enabled: e.target.checked }
                                })}
                            />
                            <span className="label-text font-medium">{t('security.config.enable_whitelist')}</span>
                        </label>
                        <div className="text-xs text-gray-500 ml-14 mt-1 bg-yellow-50 dark:bg-yellow-900/20 p-2 rounded flex items-start gap-2">
                            <AlertTriangle size={14} className="mt-0.5 text-yellow-600 dark:text-yellow-400 shrink-0" />
                            {t('security.config.whitelist_warning')}
                        </div>
                    </div>

                    <div className="form-control mt-4">
                        <label className="label cursor-pointer justify-start gap-4">
                            <input
                                type="checkbox"
                                className="checkbox checkbox-success"
                                checked={config.whitelist.whitelist_priority}
                                disabled={!config.whitelist.enabled}
                                onChange={(e) => setConfig({
                                    ...config,
                                    whitelist: { ...config.whitelist, whitelist_priority: e.target.checked }
                                })}
                            />
                            <span className="label-text font-medium">{t('security.config.whitelist_priority')}</span>
                        </label>
                        <label className="label ml-8 pt-0">
                            <span className="label-text-alt text-gray-400">{t('security.config.whitelist_priority_desc')}</span>
                        </label>
                    </div>
                </div>
            </div>
        </div>
    );
};
