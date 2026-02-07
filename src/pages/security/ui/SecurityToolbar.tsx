// File: src/pages/security/ui/SecurityToolbar.tsx
// Security page toolbar - styled like AccountsToolbar

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, RefreshCw, Trash2, Search, ChevronLeft, ChevronRight } from 'lucide-react';
import { cn } from '@/shared/lib';
import type { SecurityTab } from '../lib/constants';

interface SecurityToolbarProps {
    activeTab: SecurityTab;
    blacklistCount: number;
    whitelistCount: number;
    logsCount: number;
    searchQuery: string;
    onSearchChange: (query: string) => void;
    onAddClick: () => void;
    onRefreshLogs: () => void;
    onClearLogs: () => void;
    // Pagination
    logPage: number;
    onNextPage: () => void;
    onPrevPage: () => void;
    hasMoreLogs: boolean;
}

export const SecurityToolbar = memo(function SecurityToolbar({
    activeTab,
    blacklistCount,
    whitelistCount,
    logsCount,
    searchQuery,
    onSearchChange,
    onAddClick,
    onRefreshLogs,
    onClearLogs,
    logPage,
    onNextPage,
    onPrevPage,
    hasMoreLogs,
}: SecurityToolbarProps) {
    const { t } = useTranslation();

    // Don't show toolbar for settings tab
    if (activeTab === 'settings') {
        return null;
    }

    return (
        <div className="flex-none flex items-center justify-between px-5 py-2 border-b border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900">
            <div className="flex items-center gap-4">
                {/* Search Input */}
                <div className="relative">
                    <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-zinc-400" />
                    <input
                        type="text"
                        value={searchQuery}
                        onChange={(e) => onSearchChange(e.target.value)}
                        placeholder={t('common.search', 'Search...')}
                        className="h-8 pl-8 pr-3 text-xs bg-zinc-100 dark:bg-zinc-800 border-none rounded-lg focus:ring-1 focus:ring-indigo-500 w-48 transition-all"
                    />
                </div>

                <div className="h-4 w-px bg-zinc-200 dark:bg-zinc-800" />

                <div className="text-xs text-zinc-500 dark:text-zinc-500 font-medium">
                    {activeTab === 'blacklist' && `${blacklistCount} ${t('security.entries', 'entries')}`}
                    {activeTab === 'whitelist' && `${whitelistCount} ${t('security.entries', 'entries')}`}
                    {activeTab === 'logs' && `${logsCount} ${t('security.records', 'records')}`}
                </div>
            </div>

            <div className="flex items-center gap-2">
                {/* Pagination Controls (Logs Only) */}
                {activeTab === 'logs' && (
                    <div className="flex items-center gap-1 mr-2 px-2 py-1 bg-zinc-50 dark:bg-zinc-800/50 rounded-lg border border-zinc-100 dark:border-zinc-800">
                        <button
                            onClick={onPrevPage}
                            disabled={logPage <= 1}
                            className="p-1 rounded hover:bg-zinc-200 dark:hover:bg-zinc-700 disabled:opacity-30 disabled:hover:bg-transparent transition-colors"
                        >
                            <ChevronLeft className="w-3.5 h-3.5" />
                        </button>
                        <span className="text-[10px] font-mono w-12 text-center text-zinc-500">
                            Page {logPage}
                        </span>
                        <button
                            onClick={onNextPage}
                            disabled={!hasMoreLogs}
                            className="p-1 rounded hover:bg-zinc-200 dark:hover:bg-zinc-700 disabled:opacity-30 disabled:hover:bg-transparent transition-colors"
                        >
                            <ChevronRight className="w-3.5 h-3.5" />
                        </button>
                    </div>
                )}

                {(activeTab === 'blacklist' || activeTab === 'whitelist') && (
                    <button
                        onClick={onAddClick}
                        className={cn(
                            'flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-semibold transition-all shadow-sm',
                            activeTab === 'blacklist'
                                ? 'bg-gradient-to-r from-red-500 to-orange-500 text-white shadow-red-500/20 hover:shadow-red-500/30'
                                : 'bg-gradient-to-r from-emerald-500 to-teal-500 text-white shadow-emerald-500/20 hover:shadow-emerald-500/30'
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
                            className="flex items-center gap-1.5 p-1.5 rounded-lg bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 hover:bg-zinc-200 dark:hover:bg-zinc-700 transition-colors border border-transparent hover:border-zinc-300 dark:hover:border-zinc-600"
                            title={t('common.refresh', 'Refresh')}
                        >
                            <RefreshCw className="w-3.5 h-3.5" />
                        </button>
                        <button
                            onClick={onClearLogs}
                            className="flex items-center gap-1.5 p-1.5 rounded-lg bg-red-50 dark:bg-red-500/10 text-red-600 dark:text-red-400 hover:bg-red-100 dark:hover:bg-red-500/20 transition-colors border border-transparent hover:border-red-200 dark:hover:border-red-500/30"
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
