// File: src/pages/logs/ui/LogsPage.tsx
// Traffic logs page - FSD architecture

import { memo } from 'react';

import { ModalDialog, Pagination } from '@/shared/ui';
import { useTranslation } from 'react-i18next';

import { useLogsPage } from '../model';
import { LogsHeader } from './LogsHeader';
import { LogsToolbar } from './LogsToolbar';
import { LogsTable } from './LogsTable';
import { LogDetailModal } from './LogDetailModal';

export const LogsPage = memo(function LogsPage() {
  const { t } = useTranslation();
  const {
    // Data
    logs,
    stats,
    totalCount,
    uniqueAccounts,

    // Filter state
    searchQuery,
    setSearchQuery,
    quickFilter,
    quickFilters,
    handleQuickFilterChange,
    accountFilter,
    setAccountFilter,
    resetFilters,

    // UI state
    isLoggingEnabled,
    isAutoRefresh,
    loading,
    loadingDetail,
    selectedLog,
    setSelectedLog,
    isClearConfirmOpen,
    setIsClearConfirmOpen,

    // Pagination
    pageSize,
    setPageSize,
    currentPage,
    totalPages,
    goToPage,
    PAGE_SIZE_OPTIONS,

    // Actions
    loadData,
    toggleLogging,
    toggleAutoRefresh,
    clearLogs,
    loadLogDetail,
  } = useLogsPage();

  const hasActiveFilters = !!(searchQuery || accountFilter);

  return (
    <div className="h-full overflow-y-auto p-5 max-w-7xl mx-auto w-full">
      {/* Main Card */}
      <div className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 flex flex-col overflow-hidden h-full">
        
        {/* Header */}
        <LogsHeader
          stats={stats}
          isLoggingEnabled={isLoggingEnabled}
          isAutoRefresh={isAutoRefresh}
          onToggleLogging={toggleLogging}
          onToggleAutoRefresh={toggleAutoRefresh}
        />

        {/* Toolbar */}
        <LogsToolbar
          searchQuery={searchQuery}
          onSearchChange={setSearchQuery}
          quickFilter={quickFilter}
          quickFilters={quickFilters}
          onQuickFilterChange={handleQuickFilterChange}
          accountFilter={accountFilter}
          onAccountFilterChange={setAccountFilter}
          uniqueAccounts={uniqueAccounts}
          onRefresh={() => loadData()}
          onClear={() => setIsClearConfirmOpen(true)}
          onResetFilters={resetFilters}
          loading={loading}
          hasActiveFilters={hasActiveFilters}
        />

        {/* Content Area */}
        <div className="flex-1 min-h-0 overflow-y-auto">
          <LogsTable
            logs={logs}
            loading={loading}
            onLogClick={loadLogDetail}
          />
        </div>

        {/* Pagination */}
        {totalCount > 0 && (
          <div className="border-t border-zinc-200 dark:border-zinc-800 px-3 py-2">
            <Pagination
              currentPage={currentPage}
              totalPages={totalPages}
              onPageChange={goToPage}
              totalItems={totalCount}
              itemsPerPage={pageSize}
              onPageSizeChange={(newSize) => {
                setPageSize(newSize);
                goToPage(1);
              }}
              pageSizeOptions={PAGE_SIZE_OPTIONS}
            />
          </div>
        )}
      </div>

      {/* Log Detail Modal */}
      <LogDetailModal
        log={selectedLog}
        loading={loadingDetail}
        onClose={() => setSelectedLog(null)}
      />

      {/* Clear Confirm Dialog */}
      <ModalDialog
        isOpen={isClearConfirmOpen}
        title={t('logs.dialog.clear_title', 'Clear Logs')}
        message={t('logs.dialog.clear_message', 'Are you sure you want to clear all traffic logs? This action cannot be undone.')}
        type="confirm"
        confirmText={t('common.delete', 'Delete')}
        isDestructive={true}
        onConfirm={clearLogs}
        onCancel={() => setIsClearConfirmOpen(false)}
      />
    </div>
  );
});

export default LogsPage;
