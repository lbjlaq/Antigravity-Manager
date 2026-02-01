// File: src/pages/security/ui/SecurityToolbar.tsx
// Security page toolbar - styled like AccountsToolbar

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
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

export const SecurityToolbar = memo(function SecurityToolbar({
    activeTab,
    blacklistCount,
    whitelistCount,
    logsCount,
    onAddClick,
    onRefreshLogs,
    onClearLogs,
}: SecurityToolbarProps) {
    const { t } = useTranslation();

    // Don't show toolbar for settings tab
    if (activeTab === 'settings') {
        return null;
    }

    return (
        <div className="flex-none flex items-center justify-between px-5 py-2 border-b border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900">
            <div className="text-xs text-zinc-500 dark:text-zinc-500">
                {activeTab === 'blacklist' && `${blacklistCount} ${t('security.entries', 'entries')}`}
                {activeTab === 'whitelist' && `${whitelistCount} ${t('security.entries', 'entries')}`}
                {activeTab === 'logs' && `${logsCount} ${t('security.records', 'records')}`}
            </div>

            <div className="flex items-center gap-2">
                {(activeTab === 'blacklist' || activeTab === 'whitelist') && (
                    <button
                        onClick={onAddClick}
                        className={cn(
                            'flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-semibold transition-all',
                            activeTab === 'blacklist'
                                ? 'bg-red-500 hover:bg-red-600 text-white'
                                : 'bg-emerald-500 hover:bg-emerald-600 text-white'
                        )}
                    >
                        <Plus className="w-3.5 h-3.5" />
                        {t('common.add', 'Add')}
                    </button>
                )}

                {activeTab === 'logs' && (
                    <>
                        <button
                            onClick={onRefreshLogs}
                            className="flex items-center gap-1.5 p-1.5 rounded-lg bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 hover:bg-zinc-200 dark:hover:bg-zinc-700 transition-colors"
                            title={t('common.refresh', 'Refresh')}
                        >
                            <RefreshCw className="w-3.5 h-3.5" />
                        </button>
                        <button
                            onClick={onClearLogs}
                            className="flex items-center gap-1.5 p-1.5 rounded-lg bg-red-50 dark:bg-red-500/10 text-red-600 dark:text-red-400 hover:bg-red-100 dark:hover:bg-red-500/20 transition-colors"
                            title={t('common.clear', 'Clear')}
                        >
                            <Trash2 className="w-3.5 h-3.5" />
                        </button>
                    </>
                )}
            </div>
        </div>
    );
});
