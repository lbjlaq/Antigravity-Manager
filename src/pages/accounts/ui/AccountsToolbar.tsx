// File: src/pages/accounts/ui/AccountsToolbar.tsx
import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Search, RefreshCw, Download, Upload, Trash2, LayoutGrid, List, Sparkles } from 'lucide-react';
import { motion } from 'framer-motion';

import { cn } from '@/shared/lib';
import { ActionIcon } from './ActionIcon';
import type { FilterType, ViewMode } from '../model';

interface AccountsToolbarProps {
  searchQuery: string;
  onSearchChange: (value: string) => void;
  filter: FilterType;
  onFilterChange: (filter: FilterType) => void;
  filterCounts: Record<FilterType, number>;
  viewMode: ViewMode;
  onViewModeChange: (mode: ViewMode) => void;
  selectedCount: number;
  onExport: () => void;
  onBatchDelete: () => void;
  onRefreshClick: () => void;
  onWarmupClick: () => void;
  onImport: () => void;
}

export const AccountsToolbar = memo(function AccountsToolbar({
  searchQuery,
  onSearchChange,
  filter,
  onFilterChange,
  filterCounts,
  viewMode,
  onViewModeChange,
  selectedCount,
  onExport,
  onBatchDelete,
  onRefreshClick,
  onWarmupClick,
  onImport,
}: AccountsToolbarProps) {
  const { t } = useTranslation();

  return (
    <div className="flex-none flex items-center gap-2 lg:gap-4 p-3 border-b border-zinc-200 dark:border-zinc-800 bg-zinc-50 dark:bg-zinc-900/50">
      {/* Search Input */}
      <div className="relative group min-w-[180px] lg:min-w-[280px]">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-zinc-400 dark:text-zinc-500 group-focus-within:text-zinc-600 dark:group-focus-within:text-zinc-300 transition-colors" />
        <input
          type="text"
          placeholder={t('accounts.search_placeholder')}
          className="w-full h-9 pl-10 pr-4 bg-white dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg focus:outline-none focus:border-zinc-400 dark:focus:border-zinc-600 transition-all text-xs placeholder:text-zinc-400 dark:placeholder:text-zinc-600 text-zinc-900 dark:text-zinc-200"
          value={searchQuery}
          onChange={(e) => onSearchChange(e.target.value)}
        />
      </div>

      {/* Divider */}
      <div className="w-px h-6 bg-zinc-200 dark:bg-zinc-700 my-auto shrink-0" />

      {/* Filter Tabs */}
      <div className="flex items-center bg-zinc-100 dark:bg-zinc-800 p-0.5 rounded-lg border border-zinc-200 dark:border-zinc-700 shrink-0">
        {(['all', 'pro', 'ultra', 'free'] as const).map((type) => (
          <button
            key={type}
            onClick={() => onFilterChange(type)}
            className={cn(
              "relative px-3 py-1 rounded-md text-[10px] font-bold uppercase tracking-wider transition-all z-10",
              filter === type ? "text-zinc-900 dark:text-white" : "text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-300"
            )}
          >
            {filter === type && (
              <motion.div
                layoutId="activeFilter"
                className="absolute inset-0 bg-white dark:bg-zinc-700 rounded-md shadow-sm"
                transition={{ type: "spring", bounce: 0.2, duration: 0.6 }}
                style={{ zIndex: -1 }}
              />
            )}
            <span className="relative flex items-center gap-1.5">
              <span className="hidden sm:inline">{type}</span>
              <span className="sm:hidden">{type.charAt(0)}</span>
              <span className={cn(
                "px-1 py-0.5 rounded text-[8px]",
                filter === type ? "bg-zinc-100 dark:bg-zinc-600 text-zinc-700 dark:text-white" : "bg-zinc-200 dark:bg-zinc-700 text-zinc-500 dark:text-zinc-500"
              )}>
                {filterCounts[type] ?? 0}
              </span>
            </span>
          </button>
        ))}
      </div>

      {/* Spacer */}
      <div className="flex-1" />

      {/* Actions Group */}
      <div className="flex items-center gap-1">
        {/* View Toggle */}
        <div className="flex items-center bg-zinc-100 dark:bg-zinc-800 p-0.5 rounded-lg border border-zinc-200 dark:border-zinc-700 mr-2">
          <button
            onClick={() => onViewModeChange('list')}
            className={cn(
              "p-1.5 rounded-md transition-all",
              viewMode === 'list' ? "bg-white dark:bg-zinc-700 text-zinc-900 dark:text-white shadow-sm" : "text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-300"
            )}
            title={t('accounts.list_view')}
          >
            <List className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={() => onViewModeChange('grid')}
            className={cn(
              "p-1.5 rounded-md transition-all",
              viewMode === 'grid' ? "bg-white dark:bg-zinc-700 text-zinc-900 dark:text-white shadow-sm" : "text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-300"
            )}
            title={t('accounts.grid_view')}
          >
            <LayoutGrid className="w-3.5 h-3.5" />
          </button>
        </div>

        {selectedCount > 0 ? (
          <>
            <button
              onClick={onExport}
              className="h-8 px-3 rounded-lg bg-indigo-50 dark:bg-indigo-500/10 hover:bg-indigo-100 dark:hover:bg-indigo-500/20 text-indigo-600 dark:text-indigo-400 border border-indigo-200 dark:border-indigo-500/20 transition-all flex items-center gap-2"
            >
              <Download className="w-3.5 h-3.5" />
              <span className="text-[10px] font-bold">EXP ({selectedCount})</span>
            </button>
            <button
              onClick={onBatchDelete}
              className="h-8 px-3 rounded-lg bg-rose-50 dark:bg-rose-500/10 hover:bg-rose-100 dark:hover:bg-rose-500/20 text-rose-600 dark:text-rose-400 border border-rose-200 dark:border-rose-500/20 transition-all flex items-center gap-2"
            >
              <Trash2 className="w-3.5 h-3.5" />
              <span className="text-[10px] font-bold">DEL ({selectedCount})</span>
            </button>
          </>
        ) : (
          <>
            <ActionIcon
              icon={RefreshCw}
              onClick={onRefreshClick}
              label={t('common.refresh')}
              tooltip={t('accounts.refresh_all_tooltip')}
              className="h-8 px-2 text-xs"
              iconSize={14}
            />
            <ActionIcon
              icon={Sparkles}
              onClick={onWarmupClick}
              label={t('accounts.warmup_all')}
              tooltip="One-Click Warmup"
              className="text-amber-500 dark:text-amber-400 hover:bg-amber-50 dark:hover:bg-amber-500/10 h-8 px-2 text-xs"
              iconSize={14}
            />
            <div className="w-px h-5 bg-zinc-200 dark:bg-zinc-700 mx-1" />
            <ActionIcon
              icon={Upload}
              onClick={onImport}
              label={t('common.import')}
              tooltip={t('accounts.import_tooltip')}
              className="h-8 px-2 text-xs"
              iconSize={14}
            />
            <ActionIcon
              icon={Download}
              onClick={onExport}
              label={t('common.export')}
              tooltip={t('accounts.export_tooltip')}
              className="h-8 px-2 text-xs"
              iconSize={14}
            />
          </>
        )}
      </div>
    </div>
  );
});
