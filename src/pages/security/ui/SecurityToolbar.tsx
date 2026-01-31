// File: src/pages/security/ui/SecurityToolbar.tsx
// Security page toolbar

import { useTranslation } from 'react-i18next';
import { motion } from 'framer-motion';
import { Plus, RefreshCw, Trash2 } from 'lucide-react';
import { cn } from '@/shared/lib';
import type { SecurityTab } from '../lib/constants';

interface SecurityToolbarProps {
    activeTab: SecurityTab;
    blacklistCount: number;
    whitelistCount: number;
    logsCount: number;
    onAddClick: () => void;
    onRefreshLogs: () => void;
    onClearLogs: () => void;
}

export function SecurityToolbar({
    activeTab,
    blacklistCount,
    whitelistCount,
    logsCount,
    onAddClick,
    onRefreshLogs,
    onClearLogs,
}: SecurityToolbarProps) {
    const { t } = useTranslation();

    return (
        <div className="flex items-center justify-between px-5 py-3 border-b border-gray-100 dark:border-zinc-800">
            <div className="text-sm text-gray-500 dark:text-zinc-500">
                {activeTab === 'blacklist' && `${blacklistCount} ${t('security.entries', 'entries')}`}
                {activeTab === 'whitelist' && `${whitelistCount} ${t('security.entries', 'entries')}`}
                {activeTab === 'logs' && `${logsCount} ${t('security.records', 'records')}`}
                {activeTab === 'settings' && t('security.configure', 'Configure security settings')}
            </div>

            <div className="flex items-center gap-2">
                {(activeTab === 'blacklist' || activeTab === 'whitelist') && (
                    <motion.button
                        whileHover={{ scale: 1.02 }}
                        whileTap={{ scale: 0.98 }}
                        onClick={onAddClick}
                        className={cn(
                            'flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-semibold transition-all',
                            activeTab === 'blacklist'
                                ? 'bg-red-500 hover:bg-red-600 text-white'
                                : 'bg-emerald-500 hover:bg-emerald-600 text-white'
                        )}
                    >
                        <Plus className="w-4 h-4" />
                        {t('common.add', 'Add')}
                    </motion.button>
                )}

                {activeTab === 'logs' && (
                    <>
                        <button
                            onClick={onRefreshLogs}
                            className="flex items-center gap-2 px-3 py-2 rounded-lg bg-gray-100 dark:bg-zinc-800 text-gray-600 dark:text-zinc-300 hover:bg-gray-200 dark:hover:bg-zinc-700 transition-colors text-sm"
                        >
                            <RefreshCw className="w-4 h-4" />
                        </button>
                        <button
                            onClick={onClearLogs}
                            className="flex items-center gap-2 px-3 py-2 rounded-lg bg-red-50 dark:bg-red-500/10 text-red-600 dark:text-red-400 hover:bg-red-100 dark:hover:bg-red-500/20 transition-colors text-sm"
                        >
                            <Trash2 className="w-4 h-4" />
                        </button>
                    </>
                )}
            </div>
        </div>
    );
}
