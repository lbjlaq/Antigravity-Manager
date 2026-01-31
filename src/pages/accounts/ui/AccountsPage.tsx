// File: src/pages/accounts/ui/AccountsPage.tsx
import { memo } from 'react';

import AccountTable from '@/components/accounts/AccountTable';
import AccountGrid from '@/components/accounts/AccountGrid';
import Pagination from '@/components/common/Pagination';

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
    <div className="h-full flex flex-col p-5 gap-4 max-w-7xl mx-auto w-full">
      {/* Hidden file input for import */}
      <input
        ref={fileInputRef}
        type="file"
        accept=".json,application/json"
        style={{ display: 'none' }}
        onChange={handleFileChange}
      />

      {/* Main Card */}
      <div className="flex-1 min-h-0 relative flex flex-col" ref={containerRef}>
        <div className="h-full bg-white dark:bg-zinc-900/40 backdrop-blur-xl rounded-2xl border border-white/5 flex flex-col overflow-hidden shadow-2xl">
          
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
          <div className="flex-1 min-h-0 overflow-y-auto p-0 scrollbar-thin scrollbar-thumb-white/10 scrollbar-track-transparent">
            {viewMode === 'list' ? (
              <div className="p-2 space-y-1">
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
        </div>
      </div>

      {/* Pagination */}
      {filteredAccounts.length > 0 && (
        <div className="flex-none">
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
