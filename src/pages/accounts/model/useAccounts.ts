// File: src/pages/accounts/model/useAccounts.ts
// Accounts page business logic hook

import { useState, useEffect, useMemo, useRef, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { save, open } from '@tauri-apps/plugin-dialog';
import { join } from '@tauri-apps/api/path';

import { invoke } from '@/shared/api';
import { isTauri } from '@/shared/lib';
import { showToast } from '@/components/common/ToastContainer';
import {
  useAccounts as useAccountsQuery,
  useCurrentAccount,
  useAddAccount,
  useSwitchAccount,
  useDeleteAccount,
  useDeleteAccounts,
  useRefreshQuota,
  useRefreshAllQuotas,
  useToggleProxyStatus,
  useReorderAccounts,
  useWarmUpAccount,
  useWarmUpAccounts,
} from '@/features/accounts';
import type { Account } from '@/entities/account';
import { useConfigStore } from '@/stores/useConfigStore';

export type FilterType = 'all' | 'pro' | 'ultra' | 'free';
export type ViewMode = 'list' | 'grid';

export function useAccountsPage() {
  const { t } = useTranslation();

  // FSD Queries
  const { data: accounts = [], isLoading: loading, refetch: fetchAccounts } = useAccountsQuery();
  const { data: currentAccount } = useCurrentAccount();

  // FSD Mutations
  const addAccountMutation = useAddAccount();
  const deleteAccountMutation = useDeleteAccount();
  const deleteAccountsMutation = useDeleteAccounts();
  const switchAccountMutation = useSwitchAccount();
  const refreshQuotaMutation = useRefreshQuota();
  const refreshAllQuotasMutation = useRefreshAllQuotas();
  const toggleProxyMutation = useToggleProxyStatus();
  const reorderMutation = useReorderAccounts();
  const warmUpAccountMutation = useWarmUpAccount();
  const warmUpAllMutation = useWarmUpAccounts();

  // Config store
  const { config } = useConfigStore();

  // Extract selected accounts for proxy
  const proxySelectedAccountIds = useMemo(() => {
    if (config?.show_proxy_selected_badge === false) {
      return new Set<string>();
    }
    const scheduling = config?.proxy?.scheduling;
    if (scheduling?.mode === 'Selected' && scheduling?.selected_accounts) {
      return new Set(scheduling.selected_accounts);
    }
    return new Set<string>();
  }, [config?.proxy?.scheduling, config?.show_proxy_selected_badge]);

  // UI State
  const [searchQuery, setSearchQuery] = useState('');
  const [filter, setFilter] = useState<FilterType>('all');
  const [viewMode, setViewMode] = useState<ViewMode>(() => {
    const saved = localStorage.getItem('accounts_view_mode');
    return (saved === 'list' || saved === 'grid') ? saved : 'list';
  });
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [deviceAccount, setDeviceAccount] = useState<Account | null>(null);
  const [detailsAccount, setDetailsAccount] = useState<Account | null>(null);
  const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);
  const [isBatchDelete, setIsBatchDelete] = useState(false);
  const [toggleProxyConfirm, setToggleProxyConfirm] = useState<{ accountId: string; enable: boolean } | null>(null);
  const [isWarmupConfirmOpen, setIsWarmupConfirmOpen] = useState(false);
  const [refreshingIds, setRefreshingIds] = useState<Set<string>>(new Set());
  const [switchingAccountId, setSwitchingAccountId] = useState<string | null>(null);
  const [isRefreshConfirmOpen, setIsRefreshConfirmOpen] = useState(false);

  // Pagination
  const [currentPage, setCurrentPage] = useState(1);
  const [localPageSize, setLocalPageSize] = useState<number | null>(() => {
    const saved = localStorage.getItem('accounts_page_size');
    return saved ? parseInt(saved) : null;
  });
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });

  const fileInputRef = useRef<HTMLInputElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  // Save preferences
  useEffect(() => {
    localStorage.setItem('accounts_view_mode', viewMode);
  }, [viewMode]);

  useEffect(() => {
    if (localPageSize !== null) {
      localStorage.setItem('accounts_page_size', localPageSize.toString());
    }
  }, [localPageSize]);

  // Container resize observer
  useEffect(() => {
    if (!containerRef.current) return;
    const resizeObserver = new ResizeObserver((entries) => {
      for (let entry of entries) {
        setContainerSize({
          width: entry.contentRect.width,
          height: entry.contentRect.height,
        });
      }
    });
    resizeObserver.observe(containerRef.current);
    return () => resizeObserver.disconnect();
  }, []);

  // Dynamic items per page
  const ITEMS_PER_PAGE = useMemo(() => {
    if (localPageSize && localPageSize > 0) return localPageSize;
    if (config?.accounts_page_size && config.accounts_page_size > 0) return config.accounts_page_size;
    if (!containerSize.height) return viewMode === 'grid' ? 6 : 8;

    if (viewMode === 'list') {
      const headerHeight = 36;
      const rowHeight = 72;
      const autoFitCount = Math.floor((containerSize.height - headerHeight) / rowHeight);
      return Math.max(10, autoFitCount);
    } else {
      const cardHeight = 180;
      const gap = 16;
      let cols = 1;
      if (containerSize.width >= 1200) cols = 4;
      else if (containerSize.width >= 900) cols = 3;
      else if (containerSize.width >= 600) cols = 2;
      const rows = Math.max(1, Math.floor((containerSize.height + gap) / (cardHeight + gap)));
      return cols * rows;
    }
  }, [localPageSize, config?.accounts_page_size, containerSize, viewMode]);

  // Fetch on mount
  useEffect(() => {
    fetchAccounts();
  }, []);

  // Reset pagination on view mode change
  useEffect(() => {
    setCurrentPage(1);
  }, [viewMode]);

  // Search & Filter
  const searchedAccounts = useMemo(() => {
    if (!searchQuery) return accounts;
    const lowQuery = searchQuery.toLowerCase();
    return accounts.filter(a => a.email.toLowerCase().includes(lowQuery));
  }, [accounts, searchQuery]);

  const filterCounts = useMemo(() => {
    const pro = searchedAccounts.filter(a => a.quota?.subscription_tier?.toLowerCase().includes('pro')).length;
    const ultra = searchedAccounts.filter(a => a.quota?.subscription_tier?.toLowerCase().includes('ultra')).length;
    return {
      all: searchedAccounts.length,
      pro,
      ultra,
      free: searchedAccounts.length - pro - ultra,
    };
  }, [searchedAccounts]);

  const filteredAccounts = useMemo(() => {
    let result = searchedAccounts;
    if (filter === 'pro') {
      result = result.filter(a => a.quota?.subscription_tier?.toLowerCase().includes('pro'));
    } else if (filter === 'ultra') {
      result = result.filter(a => a.quota?.subscription_tier?.toLowerCase().includes('ultra'));
    } else if (filter === 'free') {
      result = result.filter(a => {
        const tier = a.quota?.subscription_tier?.toLowerCase();
        return !tier?.includes('pro') && !tier?.includes('ultra');
      });
    }
    return result;
  }, [searchedAccounts, filter]);

  const paginatedAccounts = useMemo(() => {
    const startIndex = (currentPage - 1) * ITEMS_PER_PAGE;
    return filteredAccounts.slice(startIndex, startIndex + ITEMS_PER_PAGE);
  }, [filteredAccounts, currentPage, ITEMS_PER_PAGE]);

  // Reset selection on filter/search change
  useEffect(() => {
    setSelectedIds(new Set());
    setCurrentPage(1);
  }, [filter, searchQuery]);

  // Handlers
  const handleToggleSelect = useCallback((id: string) => {
    setSelectedIds(prev => {
      const newSet = new Set(prev);
      if (newSet.has(id)) newSet.delete(id);
      else newSet.add(id);
      return newSet;
    });
  }, []);

  const handleToggleAll = useCallback(() => {
    const currentIds = paginatedAccounts.map(a => a.id);
    setSelectedIds(prev => {
      const allSelected = currentIds.every(id => prev.has(id));
      const newSet = new Set(prev);
      if (allSelected) currentIds.forEach(id => newSet.delete(id));
      else currentIds.forEach(id => newSet.add(id));
      return newSet;
    });
  }, [paginatedAccounts]);

  const handleAddAccount = useCallback(async (email: string, refreshToken: string) => {
    await addAccountMutation.mutateAsync({ email, refreshToken });
  }, [addAccountMutation]);

  const handleSwitch = useCallback(async (accountId: string) => {
    if (loading || switchingAccountId) return;
    setSwitchingAccountId(accountId);
    try {
      await switchAccountMutation.mutateAsync(accountId);
      showToast(t('common.success'), 'success');
    } catch (error) {
      showToast(`${t('common.error')}: ${error}`, 'error');
    } finally {
      setTimeout(() => setSwitchingAccountId(null), 500);
    }
  }, [loading, switchingAccountId, switchAccountMutation, t]);

  const handleRefresh = useCallback(async (accountId: string) => {
    setRefreshingIds(prev => new Set(prev).add(accountId));
    try {
      await refreshQuotaMutation.mutateAsync(accountId);
      showToast(t('common.success'), 'success');
    } catch (error) {
      showToast(`${t('common.error')}: ${error}`, 'error');
    } finally {
      setRefreshingIds(prev => {
        const next = new Set(prev);
        next.delete(accountId);
        return next;
      });
    }
  }, [refreshQuotaMutation, t]);

  const handleWarmup = useCallback(async (accountId: string) => {
    setRefreshingIds(prev => new Set(prev).add(accountId));
    try {
      const msg = await warmUpAccountMutation.mutateAsync(accountId);
      showToast(msg || t('accounts.warmup_triggered'), 'success');
    } catch (error) {
      showToast(`${t('common.error')}: ${error}`, 'error');
    } finally {
      setRefreshingIds(prev => {
        const next = new Set(prev);
        next.delete(accountId);
        return next;
      });
    }
  }, [warmUpAccountMutation, t]);

  const handleWarmupAll = useCallback(async () => {
    setIsWarmupConfirmOpen(false);
    try {
      const isBatch = selectedIds.size > 0;
      if (isBatch) {
        const ids = Array.from(selectedIds);
        setRefreshingIds(new Set(ids));
        const results = await Promise.allSettled(ids.map(id => warmUpAccountMutation.mutateAsync(id)));
        let successCount = 0;
        results.forEach(r => { if (r.status === 'fulfilled') successCount++; });
        showToast(t('accounts.warmup_batch_triggered', { count: successCount }), 'success');
      } else {
        const msg = await warmUpAllMutation.mutateAsync();
        showToast(msg || t('accounts.warmup_all_triggered', 'Warmup triggered for all accounts'), 'success');
      }
    } catch (error) {
      showToast(`${t('common.error')}: ${error}`, 'error');
    } finally {
      setRefreshingIds(new Set());
    }
  }, [selectedIds, warmUpAccountMutation, warmUpAllMutation, t]);

  const handleBatchDelete = useCallback(() => {
    if (selectedIds.size === 0) return;
    setIsBatchDelete(true);
  }, [selectedIds]);

  const executeBatchDelete = useCallback(async () => {
    setIsBatchDelete(false);
    try {
      const ids = Array.from(selectedIds);
      await deleteAccountsMutation.mutateAsync(ids);
      setSelectedIds(new Set());
      showToast(t('common.success'), 'success');
    } catch (error) {
      showToast(`${t('common.error')}: ${error}`, 'error');
    }
  }, [selectedIds, deleteAccountsMutation, t]);

  const handleDelete = useCallback((accountId: string) => {
    setDeleteConfirmId(accountId);
  }, []);

  const executeDelete = useCallback(async () => {
    if (!deleteConfirmId) return;
    try {
      await deleteAccountMutation.mutateAsync(deleteConfirmId);
      showToast(t('common.success'), 'success');
    } catch (error) {
      showToast(`${t('common.error')}: ${error}`, 'error');
    } finally {
      setDeleteConfirmId(null);
    }
  }, [deleteConfirmId, deleteAccountMutation, t]);

  const handleToggleProxy = useCallback((accountId: string, currentlyDisabled: boolean) => {
    setToggleProxyConfirm({ accountId, enable: currentlyDisabled });
  }, []);

  const executeToggleProxy = useCallback(async () => {
    if (!toggleProxyConfirm) return;
    try {
      await toggleProxyMutation.mutateAsync({
        accountId: toggleProxyConfirm.accountId,
        enable: toggleProxyConfirm.enable,
        reason: toggleProxyConfirm.enable ? undefined : t('accounts.proxy_disabled_reason_manual'),
      });
      showToast(t('common.success'), 'success');
    } catch (error) {
      showToast(`${t('common.error')}: ${error}`, 'error');
    } finally {
      setToggleProxyConfirm(null);
    }
  }, [toggleProxyConfirm, toggleProxyMutation, t]);

  const handleRefreshClick = useCallback(() => {
    setIsRefreshConfirmOpen(true);
  }, []);

  const executeRefresh = useCallback(async () => {
    setIsRefreshConfirmOpen(false);
    try {
      const isBatch = selectedIds.size > 0;
      let successCount = 0;
      let failedCount = 0;
      const details: string[] = [];

      if (isBatch) {
        const ids = Array.from(selectedIds);
        setRefreshingIds(new Set(ids));
        const results = await Promise.allSettled(ids.map(id => refreshQuotaMutation.mutateAsync(id)));
        results.forEach((result, index) => {
          const id = ids[index];
          const email = accounts.find(a => a.id === id)?.email || id;
          if (result.status === 'fulfilled') successCount++;
          else {
            failedCount++;
            details.push(`${email}: ${result.reason}`);
          }
        });
      } else {
        setRefreshingIds(new Set(accounts.map(a => a.id)));
        const stats = await refreshAllQuotasMutation.mutateAsync();
        if (stats) {
          successCount = stats.success;
          failedCount = stats.failed;
          details.push(...stats.details);
        }
      }

      if (failedCount === 0) {
        showToast(t('accounts.refresh_selected', { count: successCount }), 'success');
      } else {
        showToast(`${t('common.success')}: ${successCount}, ${t('common.error')}: ${failedCount}`, 'warning');
      }
    } catch (error) {
      showToast(`${t('common.error')}: ${error}`, 'error');
    } finally {
      setRefreshingIds(new Set());
    }
  }, [selectedIds, accounts, refreshQuotaMutation, refreshAllQuotasMutation, t]);

  // Export functions
  const exportAccountsToJson = useCallback(async (accountsToExport: Account[]) => {
    try {
      if (accountsToExport.length === 0) {
        showToast(t('dashboard.toast.export_no_accounts'), 'warning');
        return;
      }

      const exportData = accountsToExport.map(acc => ({
        email: acc.email,
        refresh_token: acc.token.refresh_token,
      }));
      const content = JSON.stringify(exportData, null, 2);
      const fileName = `antigravity_accounts_${new Date().toISOString().split('T')[0]}.json`;

      if (isTauri()) {
        let path: string | null = null;
        if (config?.default_export_path) {
          path = await join(config.default_export_path, fileName);
        } else {
          path = await save({
            filters: [{ name: 'JSON', extensions: ['json'] }],
            defaultPath: fileName,
          });
        }
        if (!path) return;
        await invoke('save_text_file', { path, content });
        showToast(`${t('common.success')} ${path}`, 'success');
      } else {
        const blob = new Blob([content], { type: 'application/json' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = fileName;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
        showToast(t('dashboard.toast.export_success', { path: fileName }), 'success');
      }
    } catch (error: any) {
      showToast(`${t('common.error')}: ${error}`, 'error');
    }
  }, [config?.default_export_path, t]);

  const handleExport = useCallback(() => {
    const idsToExport = selectedIds.size > 0 ? Array.from(selectedIds) : accounts.map(a => a.id);
    const accountsToExport = accounts.filter(a => idsToExport.includes(a.id));
    exportAccountsToJson(accountsToExport);
  }, [selectedIds, accounts, exportAccountsToJson]);

  const handleExportOne = useCallback(async (accountId: string) => {
    try {
      const account = accounts.find(a => a.id === accountId);
      if (!account) return;
      const path = await save({
        filters: [{ name: 'JSON', extensions: ['json'] }],
        defaultPath: `${account.email}_export.json`,
      });
      if (path) {
        await invoke('save_text_file', { path, content: JSON.stringify(account, null, 2) });
        showToast(t('common.success'), 'success');
      }
    } catch (error) {
      showToast(t('common.error'), 'error');
    }
  }, [accounts, t]);

  // Import functions
  const processImportData = useCallback(async (content: string) => {
    let importData: Array<{ email?: string; refresh_token?: string }>;
    try {
      importData = JSON.parse(content);
    } catch {
      showToast(t('accounts.import_invalid_format'), 'error');
      return;
    }

    if (!Array.isArray(importData) || importData.length === 0) {
      showToast(t('accounts.import_invalid_format'), 'error');
      return;
    }

    const validEntries = importData.filter(
      item => item.refresh_token && typeof item.refresh_token === 'string' && item.refresh_token.startsWith('1//')
    );

    if (validEntries.length === 0) {
      showToast(t('accounts.import_invalid_format'), 'error');
      return;
    }

    let successCount = 0;
    let failCount = 0;

    for (const entry of validEntries) {
      try {
        await addAccountMutation.mutateAsync({ email: entry.email || '', refreshToken: entry.refresh_token! });
        successCount++;
      } catch {
        failCount++;
      }
      await new Promise(r => setTimeout(r, 100));
    }

    if (failCount === 0) {
      showToast(t('accounts.import_success', { count: successCount }), 'success');
    } else if (successCount > 0) {
      showToast(t('accounts.import_partial', { success: successCount, fail: failCount }), 'warning');
    } else {
      showToast(t('accounts.import_fail', { error: 'All accounts failed to import' }), 'error');
    }
  }, [addAccountMutation, t]);

  const handleImportJson = useCallback(async () => {
    if (isTauri()) {
      try {
        const selected = await open({
          multiple: false,
          filters: [{ name: 'JSON', extensions: ['json'] }],
        });
        if (!selected || typeof selected !== 'string') return;
        const content: string = await invoke('read_text_file', { path: selected });
        await processImportData(content);
      } catch (error) {
        showToast(t('accounts.import_fail', { error: String(error) }), 'error');
      }
    } else {
      fileInputRef.current?.click();
    }
  }, [processImportData, t]);

  const handleFileChange = useCallback(async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    try {
      const content = await file.text();
      await processImportData(content);
    } catch (error) {
      showToast(t('accounts.import_fail', { error: String(error) }), 'error');
    } finally {
      event.target.value = '';
    }
  }, [processImportData, t]);

  const handleViewDetails = useCallback((accountId: string) => {
    const account = accounts.find(a => a.id === accountId);
    if (account) setDetailsAccount(account);
  }, [accounts]);

  const handleViewDevice = useCallback((accountId: string) => {
    const account = accounts.find(a => a.id === accountId);
    if (account) setDeviceAccount(account);
  }, [accounts]);

  const handlePageChange = useCallback((page: number) => {
    setCurrentPage(page);
  }, []);

  const handleReorder = useCallback((ids: string[]) => {
    reorderMutation.mutate(ids);
  }, [reorderMutation]);

  return {
    // Data
    accounts,
    currentAccount,
    paginatedAccounts,
    filteredAccounts,
    searchedAccounts,
    filterCounts,
    proxySelectedAccountIds,
    loading,
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
    localPageSize,
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
    handleDelete,
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
  };
}
