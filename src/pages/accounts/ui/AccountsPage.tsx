// File: src/pages/accounts/ui/AccountsPage.tsx
import { memo } from 'react';

import { AccountTable, AccountGrid } from '@/widgets/accounts';
import { Pagination } from '@/shared/ui';

import { useAccountsPage } from '../model';
import { AccountsHeader } from './AccountsHeader';
import { AccountsToolbar } from './AccountsToolbar';
import { AccountsDialogs } from './AccountsDialogs';

export const AccountsPage = memo(function AccountsPage() {
  const {
    // Data
    accounts,
    currentAccount,
    paginatedAccounts,
    filteredAccounts,
    searchedAccounts,
    filterCounts,
    proxySelectedAccountIds,
    ITEMS_PER_PAGE,

    // UI State
    searchQuery,
    setSearchQuery,
    filter,
    setFilter,
    viewMode,
    setViewMode,
    selectedIds,
    refreshingIds,
    switchingAccountId,
    currentPage,
    setLocalPageSize,

    // Dialogs
    deviceAccount,
    setDeviceAccount,
    detailsAccount,
    setDetailsAccount,
    deleteConfirmId,
    setDeleteConfirmId,
    isBatchDelete,
    setIsBatchDelete,
    toggleProxyConfirm,
    setToggleProxyConfirm,
    isWarmupConfirmOpen,
    setIsWarmupConfirmOpen,
    isRefreshConfirmOpen,
    setIsRefreshConfirmOpen,

    // Refs
    fileInputRef,
    containerRef,

    // Handlers
    handleToggleSelect,
    handleToggleAll,
    handleAddAccount,
    handleSwitch,
    handleRefresh,
    handleWarmup,
    handleWarmupAll,
    handleBatchDelete,
    executeBatchDelete,
    executeDelete,
    handleToggleProxy,
    executeToggleProxy,
    handleRefreshClick,
    executeRefresh,
    handleExport,
    handleExportOne,
    handleImportJson,
    handleFileChange,
    handleViewDetails,
    handleViewDevice,
    handlePageChange,
    handleReorder,
  } = useAccountsPage();

  return (
    <div className="h-full overflow-y-auto p-5 max-w-7xl mx-auto w-full">
      {/* Hidden file input for import */}
      <input
        ref={fileInputRef}
        type="file"
        accept=".json,application/json"
        style={{ display: 'none' }}
        onChange={handleFileChange}
      />

      {/* Main Card */}
      <div ref={containerRef}>
        <div className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 overflow-hidden">
          
          {/* Header */}
          <AccountsHeader
            accountCount={searchedAccounts.length}
            onAddAccount={handleAddAccount}
          />

          {/* Toolbar */}
          <AccountsToolbar
            searchQuery={searchQuery}
            onSearchChange={setSearchQuery}
            filter={filter}
            onFilterChange={setFilter}
            filterCounts={filterCounts}
            viewMode={viewMode}
            onViewModeChange={setViewMode}
            selectedCount={selectedIds.size}
            onExport={handleExport}
            onBatchDelete={handleBatchDelete}
            onRefreshClick={handleRefreshClick}
            onWarmupClick={() => setIsWarmupConfirmOpen(true)}
            onImport={handleImportJson}
          />

          {/* Content Area */}
          <div>
            {viewMode === 'list' ? (
              <div className="px-2 py-1">
                <AccountTable
                  accounts={paginatedAccounts}
                  allAccounts={accounts}
                  selectedIds={selectedIds}
                  refreshingIds={refreshingIds}
                  proxySelectedAccountIds={proxySelectedAccountIds}
                  onToggleSelect={handleToggleSelect}
                  onToggleAll={handleToggleAll}
                  currentAccountId={currentAccount?.id || null}
                  switchingAccountId={switchingAccountId}
                  onSwitch={handleSwitch}
                  onRefresh={handleRefresh}
                  onViewDevice={handleViewDevice}
                  onViewDetails={handleViewDetails}
                  onExport={handleExportOne}
                  onDelete={(id) => setDeleteConfirmId(id)}
                  onToggleProxy={(id: string) => handleToggleProxy(id, !!accounts.find(a => a.id === id)?.proxy_disabled)}
                  onReorder={handleReorder}
                  onWarmup={handleWarmup}
                />
              </div>
            ) : (
              <div className="p-3">
                <AccountGrid
                  accounts={paginatedAccounts}
                  selectedIds={selectedIds}
                  refreshingIds={refreshingIds}
                  proxySelectedAccountIds={proxySelectedAccountIds}
                  onToggleSelect={handleToggleSelect}
                  currentAccountId={currentAccount?.id || null}
                  switchingAccountId={switchingAccountId}
                  onSwitch={handleSwitch}
                  onRefresh={handleRefresh}
                  onViewDevice={handleViewDevice}
                  onViewDetails={handleViewDetails}
                  onExport={handleExportOne}
                  onDelete={(id) => setDeleteConfirmId(id)}
                  onToggleProxy={(id) => handleToggleProxy(id, !!accounts.find(a => a.id === id)?.proxy_disabled)}
                  onWarmup={handleWarmup}
                />
              </div>
            )}
          </div>

          {/* Pagination - inside card */}
          {filteredAccounts.length > 0 && (
            <div className="border-t border-zinc-200 dark:border-zinc-800 px-3 py-2">
              <Pagination
                currentPage={currentPage}
                totalPages={Math.ceil(filteredAccounts.length / ITEMS_PER_PAGE)}
                onPageChange={handlePageChange}
                totalItems={filteredAccounts.length}
                itemsPerPage={ITEMS_PER_PAGE}
                onPageSizeChange={(newSize) => {
                  setLocalPageSize(newSize);
                  handlePageChange(1);
                }}
                pageSizeOptions={[10, 20, 50, 100]}
              />
            </div>
          )}
        </div>
      </div>

      {/* Dialogs */}
      <AccountsDialogs
        detailsAccount={detailsAccount}
        onCloseDetails={() => setDetailsAccount(null)}
        deviceAccount={deviceAccount}
        onCloseDevice={() => setDeviceAccount(null)}
        deleteConfirmId={deleteConfirmId}
        isBatchDelete={isBatchDelete}
        selectedCount={selectedIds.size}
        onConfirmDelete={isBatchDelete ? executeBatchDelete : executeDelete}
        onCancelDelete={() => { setDeleteConfirmId(null); setIsBatchDelete(false); }}
        isRefreshConfirmOpen={isRefreshConfirmOpen}
        onConfirmRefresh={executeRefresh}
        onCancelRefresh={() => setIsRefreshConfirmOpen(false)}
        toggleProxyConfirm={toggleProxyConfirm}
        onConfirmToggleProxy={executeToggleProxy}
        onCancelToggleProxy={() => setToggleProxyConfirm(null)}
        isWarmupConfirmOpen={isWarmupConfirmOpen}
        onConfirmWarmup={handleWarmupAll}
        onCancelWarmup={() => setIsWarmupConfirmOpen(false)}
      />
    </div>
  );
});

export default AccountsPage;
