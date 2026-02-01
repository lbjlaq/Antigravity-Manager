// File: src/pages/security/ui/SettingsTab.tsx
// Security settings tab content

import { motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { Ban, CheckCircle } from 'lucide-react';
import type { SecurityMonitorConfig } from '@/entities/security';

interface SettingsTabProps {
    config: SecurityMonitorConfig;
    onSaveConfig: (config: SecurityMonitorConfig) => void;
}

export function SettingsTab({ config, onSaveConfig }: SettingsTabProps) {
    const { t } = useTranslation();

    return (
        <motion.div
            key="settings"
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -10 }}
            className="space-y-4 max-w-xl"
        >
            {/* Blacklist Settings */}
            <div className="p-4 rounded-xl bg-gray-50 dark:bg-zinc-800/50 border border-gray-100 dark:border-zinc-700/50">
                <div className="flex items-center gap-3 mb-4">
                    <div className="p-2 rounded-lg bg-red-100 dark:bg-red-500/10">
                        <Ban className="w-4 h-4 text-red-500" />
                    </div>
                    <h3 className="font-semibold text-gray-900 dark:text-white">
                        {t('security.blacklist_settings', 'Blacklist')}
                    </h3>
                </div>
                <label className="flex items-center justify-between cursor-pointer">
                    <span className="text-sm text-gray-600 dark:text-zinc-400">
                        {t('security.enable_blacklist', 'Enable Blacklist')}
                    </span>
                    <input
                        type="checkbox"
                        checked={config.blacklist.enabled}
                        onChange={(e) =>
                            onSaveConfig({
                                ...config,
                                blacklist: { ...config.blacklist, enabled: e.target.checked },
                            })
                        }
                        className="w-5 h-5 rounded bg-gray-200 dark:bg-zinc-700 border-0 text-red-500 focus:ring-red-500"
                    />
                </label>
            </div>

            {/* Whitelist Settings */}
            <div className="p-4 rounded-xl bg-gray-50 dark:bg-zinc-800/50 border border-gray-100 dark:border-zinc-700/50">
                <div className="flex items-center gap-3 mb-4">
                    <div className="p-2 rounded-lg bg-emerald-100 dark:bg-emerald-500/10">
                        <CheckCircle className="w-4 h-4 text-emerald-500" />
                    </div>
                    <h3 className="font-semibold text-gray-900 dark:text-white">
                        {t('security.whitelist_settings', 'Whitelist')}
                    </h3>
                </div>
                <div className="space-y-3">
                    <label className="flex items-center justify-between cursor-pointer">
                        <span className="text-sm text-gray-600 dark:text-zinc-400">
                            {t('security.enable_whitelist', 'Enable Whitelist')}
                        </span>
                        <input
                            type="checkbox"
                            checked={config.whitelist.enabled}
                            onChange={(e) =>
                                onSaveConfig({
                                    ...config,
                                    whitelist: { ...config.whitelist, enabled: e.target.checked },
                                })
                            }
                            className="w-5 h-5 rounded bg-gray-200 dark:bg-zinc-700 border-0 text-emerald-500 focus:ring-emerald-500"
                        />
                    </label>
                    <label className="flex items-center justify-between cursor-pointer">
                        <div>
                            <span className="text-sm text-gray-600 dark:text-zinc-400">
                                {t('security.strict_mode', 'Strict Mode')}
                            </span>
                            <p className="text-xs text-gray-400 dark:text-zinc-500">
                                {t('security.strict_mode_desc', 'Only whitelisted IPs can access')}
                            </p>
                        </div>
                        <input
                            type="checkbox"
                            checked={config.whitelist.strictMode}
                            onChange={(e) =>
                                onSaveConfig({
                                    ...config,
                                    whitelist: { ...config.whitelist, strictMode: e.target.checked },
                                })
                            }
                            className="w-5 h-5 rounded bg-gray-200 dark:bg-zinc-700 border-0 text-emerald-500 focus:ring-emerald-500"
                        />
                    </label>
                </div>
            </div>
        </motion.div>
    );
}
