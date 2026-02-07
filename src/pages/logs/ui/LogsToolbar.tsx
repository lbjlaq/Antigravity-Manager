// File: src/pages/logs/ui/LogsToolbar.tsx
// Logs page toolbar component

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Search, User, RefreshCw, Trash2, X } from 'lucide-react';

import type { QuickFilterType } from '../model';

interface LogsToolbarProps {
  searchQuery: string;
  onSearchChange: (value: string) => void;
  quickFilter: QuickFilterType;
  quickFilters: Array<{ label: string; value: QuickFilterType }>;
  onQuickFilterChange: (value: QuickFilterType) => void;
  accountFilter: string;
  onAccountFilterChange: (value: string) => void;
  uniqueAccounts: string[];
  onRefresh: () => void;
  onClear: () => void;
  onResetFilters: () => void;
  loading: boolean;
  hasActiveFilters: boolean;
}

export const LogsToolbar = memo(function LogsToolbar({
  searchQuery,
  onSearchChange,
  quickFilter,
  quickFilters,
  onQuickFilterChange,
  accountFilter,
  onAccountFilterChange,
  uniqueAccounts,
  onRefresh,
  onClear,
  onResetFilters,
  loading,
  hasActiveFilters,
}: LogsToolbarProps) {
  const { t } = useTranslation();

  return (
    <div className="px-4 py-3 border-b border-zinc-200 dark:border-zinc-800 space-y-3">
      {/* Main toolbar row */}
      <div className="flex items-center gap-3">
        {/* Search input */}
        <div className="relative flex-1 max-w-md">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-zinc-400" />
          <input
            type="text"
            placeholder={t('logs.search.placeholder', 'Search logs...')}
            value={searchQuery}
            onChange={(e) => onSearchChange(e.target.value)}
            className="w-full pl-9 pr-4 py-2 text-sm bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-purple-500 dark:focus:ring-purple-400 focus:border-transparent"
          />
        </div>

        {/* Account filter */}
        <div className="relative">
          <User className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-zinc-400 pointer-events-none" />
          <select
            value={accountFilter}
            onChange={(e) => onAccountFilterChange(e.target.value)}
            className="pl-9 pr-8 py-2 text-sm bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-purple-500 appearance-none cursor-pointer min-w-[160px]"
          >
            <option value="">{t('logs.filter.all_accounts', 'All Accounts')}</option>
            {uniqueAccounts.map(email => (
              <option key={email} value={email}>
                {email.length > 25 ? `${email.slice(0, 22)}...` : email}
              </option>
            ))}
          </select>
        </div>

        {/* Action buttons */}
        <div className="flex items-center gap-1">
          <button
            onClick={onRefresh}
            disabled={loading}
            className="p-2 text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800 rounded-lg transition-colors disabled:opacity-50"
            title={t('common.refresh', 'Refresh')}
          >
            <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
          </button>
          <button
            onClick={onClear}
            className="p-2 text-zinc-500 hover:text-red-600 dark:hover:text-red-400 hover:bg-zinc-100 dark:hover:bg-zinc-800 rounded-lg transition-colors"
            title={t('logs.clear', 'Clear logs')}
          >
            <Trash2 className="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Quick filters row */}
      <div className="flex items-center gap-2 flex-wrap">
        <span className="text-xs font-medium text-zinc-500 dark:text-zinc-400 uppercase tracking-wide">
          {t('logs.quick_filters', 'Quick Filters')}
        </span>
        {quickFilters.map((filter) => (
          <button
            key={filter.value}
            onClick={() => onQuickFilterChange(filter.value)}
            className={`
              px-3 py-1 text-xs font-medium rounded-full border transition-colors
              ${quickFilter === filter.value
                ? 'bg-purple-500 text-white border-purple-500'
                : 'bg-white dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 border-zinc-200 dark:border-zinc-700 hover:border-purple-300 dark:hover:border-purple-600'
              }
            `}
          >
            {filter.label}
          </button>
        ))}
        {hasActiveFilters && (
          <button
            onClick={onResetFilters}
            className="flex items-center gap-1 px-2 py-1 text-xs text-purple-600 dark:text-purple-400 hover:underline"
          >
            <X className="w-3 h-3" />
            {t('logs.reset_filters', 'Reset')}
          </button>
        )}
      </div>
    </div>
  );
});
