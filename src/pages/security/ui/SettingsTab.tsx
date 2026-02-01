// File: src/pages/security/ui/SettingsTab.tsx
// Security settings grid

import { motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { Ban, ShieldCheck, AlertTriangle, Shield } from 'lucide-react';
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
            className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6 p-1"
        >
            {/* Blacklist Card */}
            <div className="p-5 rounded-2xl bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 shadow-sm hover:shadow-md transition-shadow">
                <div className="flex items-start justify-between mb-4">
                    <div className="p-2.5 rounded-xl bg-red-100 dark:bg-red-500/10 text-red-600 dark:text-red-500">
                        <Ban className="w-5 h-5" />
                    </div>
                    <label className="relative inline-flex items-center cursor-pointer">
                        <input
                            type="checkbox"
                            className="sr-only peer"
                            checked={config.blacklist.enabled}
                            onChange={(e) =>
                                onSaveConfig({
                                    ...config,
                                    blacklist: { ...config.blacklist, enabled: e.target.checked },
                                })
                            }
                        />
                        <div className="w-9 h-5 bg-zinc-200 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-red-300 dark:peer-focus:ring-red-800 rounded-full peer dark:bg-zinc-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all dark:border-zinc-600 peer-checked:bg-red-500"></div>
                    </label>
                </div>
                <h3 className="text-sm font-bold text-zinc-900 dark:text-zinc-100 mb-1">
                    {t('security.blacklist_settings', 'Blacklist')}
                </h3>
                <p className="text-xs text-zinc-500 dark:text-zinc-400 leading-relaxed">
                    Automatically block IPs that exceed rate limits or trigger security rules.
                </p>
            </div>

            {/* Whitelist Card */}
            <div className="p-5 rounded-2xl bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 shadow-sm hover:shadow-md transition-shadow">
                <div className="flex items-start justify-between mb-4">
                    <div className="p-2.5 rounded-xl bg-emerald-100 dark:bg-emerald-500/10 text-emerald-600 dark:text-emerald-500">
                        <ShieldCheck className="w-5 h-5" />
                    </div>
                    <label className="relative inline-flex items-center cursor-pointer">
                        <input
                            type="checkbox"
                            className="sr-only peer"
                            checked={config.whitelist.enabled}
                            onChange={(e) =>
                                onSaveConfig({
                                    ...config,
                                    whitelist: { ...config.whitelist, enabled: e.target.checked },
                                })
                            }
                        />
                        <div className="w-9 h-5 bg-zinc-200 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-emerald-300 dark:peer-focus:ring-emerald-800 rounded-full peer dark:bg-zinc-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all dark:border-zinc-600 peer-checked:bg-emerald-500"></div>
                    </label>
                </div>
                <h3 className="text-sm font-bold text-zinc-900 dark:text-zinc-100 mb-1">
                    {t('security.whitelist_settings', 'Whitelist')}
                </h3>
                <p className="text-xs text-zinc-500 dark:text-zinc-400 leading-relaxed">
                    Allow specific IPs to bypass standard checks and limits.
                </p>
            </div>

            {/* Strict Mode Card */}
            <div className="p-5 rounded-2xl bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 shadow-sm hover:shadow-md transition-shadow">
                <div className="flex items-start justify-between mb-4">
                    <div className="p-2.5 rounded-xl bg-amber-100 dark:bg-amber-500/10 text-amber-600 dark:text-amber-500">
                        <Shield className="w-5 h-5" />
                    </div>
                    <label className="relative inline-flex items-center cursor-pointer">
                        <input
                            type="checkbox"
                            className="sr-only peer"
                            checked={config.whitelist.strictMode}
                            onChange={(e) =>
                                onSaveConfig({
                                    ...config,
                                    whitelist: { ...config.whitelist, strictMode: e.target.checked },
                                })
                            }
                        />
                        <div className="w-9 h-5 bg-zinc-200 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-amber-300 dark:peer-focus:ring-amber-800 rounded-full peer dark:bg-zinc-700 peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-white after:border after:rounded-full after:h-4 after:w-4 after:transition-all dark:border-zinc-600 peer-checked:bg-amber-500"></div>
                    </label>
                </div>
                <div className="flex items-center gap-2 mb-1">
                    <h3 className="text-sm font-bold text-zinc-900 dark:text-zinc-100">
                        {t('security.strict_mode', 'Strict Mode')}
                    </h3>
                    <AlertTriangle className="w-3.5 h-3.5 text-amber-500" />
                </div>
               
                <p className="text-xs text-zinc-500 dark:text-zinc-400 leading-relaxed">
                    {t('security.strict_mode_desc', 'Only whitelisted IPs can access. All others are blocked.')}
                </p>
            </div>
        </motion.div>
    );
}
