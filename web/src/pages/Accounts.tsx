import { useState, useEffect, useMemo } from 'react';
import { save } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { join } from '@tauri-apps/api/path';
import { Search, RefreshCw, Download, Trash2, LayoutGrid, List } from 'lucide-react';
import { useAccountStore } from '../stores/useAccountStore';
import { useConfigStore } from '../stores/useConfigStore';
import AccountTable from '../components/accounts/AccountTable';
import AccountGrid from '../components/accounts/AccountGrid';
import AccountDetailsDialog from '../components/accounts/AccountDetailsDialog';
import AddAccountDialog from '../components/accounts/AddAccountDialog';
import ModalDialog from '../components/common/ModalDialog';
import Pagination from '../components/common/Pagination';
import { showToast } from '../components/common/ToastContainer';
import { Account } from '../types/account';

type FilterType = 'all' | 'available' | 'low';
type ViewMode = 'list' | 'grid';

import { useTranslation } from 'react-i18next';

function Accounts() {
    const { t } = useTranslation();
    const {
        accounts,
        currentAccount,
        fetchAccounts,
        addAccount,
        deleteAccount,
        switchAccount,
        loading,
        refreshQuota,
    } = useAccountStore();
    const { config } = useConfigStore();

    const [searchQuery, setSearchQuery] = useState('');
    const [filter, setFilter] = useState<FilterType>('all');
    const [viewMode, setViewMode] = useState<ViewMode>('list');
    const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
    const [detailsAccount, setDetailsAccount] = useState<Account | null>(null);
    const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);
    const [isBatchDelete, setIsBatchDelete] = useState(false);

    // Pagination State
    const [currentPage, setCurrentPage] = useState(1);
    const ITEMS_PER_PAGE = viewMode === 'grid' ? 6 : 8;

    useEffect(() => {
        fetchAccounts();
    }, []);

    // Reset pagination when view mode changes to avoid empty pages or confusion
    useEffect(() => {
        setCurrentPage(1);
    }, [viewMode]);

    // 过滤和搜索
    const filteredAccounts = useMemo(() => {
        let result = accounts;

        // 搜索过滤
        if (searchQuery) {
            result = result.filter(a =>
                a.email.toLowerCase().includes(searchQuery.toLowerCase())
            );
        }

        // 状态过滤
        if (filter === 'available') {
            result = result.filter(a => {
                const gemini = a.quota?.models.find(m => m.name.toLowerCase().includes('gemini'))?.percentage || 0;
                const claude = a.quota?.models.find(m => m.name.toLowerCase().includes('claude'))?.percentage || 0;
                return gemini >= 20 && claude >= 20;
            });
        } else if (filter === 'low') {
            result = result.filter(a => {
                const gemini = a.quota?.models.find(m => m.name.toLowerCase().includes('gemini'))?.percentage || 0;
                const claude = a.quota?.models.find(m => m.name.toLowerCase().includes('claude'))?.percentage || 0;
                return gemini < 20 || claude < 20;
            });
        }

        return result;
    }, [accounts, searchQuery, filter]);

    // Pagination Logic
    const paginatedAccounts = useMemo(() => {
        const startIndex = (currentPage - 1) * ITEMS_PER_PAGE;
        return filteredAccounts.slice(startIndex, startIndex + ITEMS_PER_PAGE);
    }, [filteredAccounts, currentPage, ITEMS_PER_PAGE]);

    const handlePageChange = (page: number) => {
        setCurrentPage(page);
    };

    // 清空选择当过滤改变 并重置分页
    useEffect(() => {
        setSelectedIds(new Set());
        setCurrentPage(1);
    }, [filter, searchQuery]);

    const handleToggleSelect = (id: string) => {
        const newSet = new Set(selectedIds);
        if (newSet.has(id)) {
            newSet.delete(id);
        } else {
            newSet.add(id);
        }
        setSelectedIds(newSet);
    };

    const handleToggleAll = () => {
        // 全选当前页的所有项
        const currentIds = paginatedAccounts.map(a => a.id);
        const allSelected = currentIds.every(id => selectedIds.has(id));

        const newSet = new Set(selectedIds);
        if (allSelected) {
            currentIds.forEach(id => newSet.delete(id));
        } else {
            currentIds.forEach(id => newSet.add(id));
        }
        setSelectedIds(newSet);
    };

    const handleAddAccount = async (email: string, refreshToken: string) => {
        await addAccount(email, refreshToken);
    };

    const [switchingAccountId, setSwitchingAccountId] = useState<string | null>(null);

    const handleSwitch = async (accountId: string) => {
        if (loading || switchingAccountId) return;

        setSwitchingAccountId(accountId);
        console.log('[Accounts] handleSwitch called for:', accountId);
        try {
            await switchAccount(accountId);
            showToast(t('common.success'), 'success');
        } catch (error) {
            console.error('[Accounts] Switch failed:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        } finally {
            // Add a small delay for smoother UX
            setTimeout(() => {
                setSwitchingAccountId(null);
            }, 500);
        }
    };

    const handleRefresh = async (accountId: string) => {
        setRefreshingIds(prev => {
            const next = new Set(prev);
            next.add(accountId);
            return next;
        });
        try {
            await refreshQuota(accountId);
            await refreshQuota(accountId);
            await refreshQuota(accountId);
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
    };

    const handleBatchDelete = () => {
        if (selectedIds.size === 0) return;
        setIsBatchDelete(true);
    };

    const executeBatchDelete = async () => {
        setIsBatchDelete(false);
        try {
            const ids = Array.from(selectedIds);
            console.log('[Accounts] Batch deleting:', ids);
            await Promise.all(ids.map(id => deleteAccount(id)));
            setSelectedIds(new Set());
            setSelectedIds(new Set());
            console.log('[Accounts] Batch delete success');
            showToast(t('common.success'), 'success');
        } catch (error) {
            console.error('[Accounts] Batch delete failed:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleDelete = (accountId: string) => {
        console.log('[Accounts] Request to delete:', accountId);
        setDeleteConfirmId(accountId);
    };

    const executeDelete = async () => {
        if (!deleteConfirmId) return;

        try {
            console.log('[Accounts] Executing delete for:', deleteConfirmId);
            await deleteAccount(deleteConfirmId);
            console.log('[Accounts] Delete success');
            showToast(t('common.success'), 'success');
        } catch (error) {
            console.error('[Accounts] Delete failed:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        } finally {
            setDeleteConfirmId(null);
        }
    };

    const [isRefreshing, setIsRefreshing] = useState(false);
    const [refreshingIds, setRefreshingIds] = useState<Set<string>>(new Set());
    const [isRefreshConfirmOpen, setIsRefreshConfirmOpen] = useState(false);

    const handleRefreshClick = () => {
        setIsRefreshConfirmOpen(true);
    };

    const executeRefresh = async () => {
        setIsRefreshConfirmOpen(false);
        setIsRefreshing(true);
        try {
            const isBatch = selectedIds.size > 0;
            let successCount = 0;
            let failedCount = 0;
            const details: string[] = [];

            if (isBatch) {
                // 批量刷新选中
                const ids = Array.from(selectedIds);
                setRefreshingIds(new Set(ids));

                const results = await Promise.allSettled(ids.map(id => refreshQuota(id)));

                results.forEach((result, index) => {
                    const id = ids[index];
                    const email = accounts.find(a => a.id === id)?.email || id;
                    if (result.status === 'fulfilled') {
                        successCount++;
                    } else {
                        failedCount++;
                        details.push(`${email}: ${result.reason}`);
                    }
                });
            } else {
                // 刷新所有
                setRefreshingIds(new Set(accounts.map(a => a.id)));
                const stats = await useAccountStore.getState().refreshAllQuotas();
                successCount = stats.success;
                failedCount = stats.failed;
                details.push(...stats.details);
            }

            if (failedCount === 0) {
                showToast(t('accounts.refresh_selected', { count: successCount }), 'success');
            } else {
                showToast(`${t('common.success')}: ${successCount}, ${t('common.error')}: ${failedCount}`, 'warning');
                // You might want to show details in a different way, but for toast, keep it simple or use a "view details" action if supported. 
                // For now, simpler toast is better than a huge alert.
                if (details.length > 0) {
                    console.warn('Refresh failures:', details);
                }
            }
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        } finally {
            setIsRefreshing(false);
            setRefreshingIds(new Set());
        }
    };

    const exportAccountsToJson = async (accountsToExport: Account[]) => {
        try {
            if (accountsToExport.length === 0) {
                showToast(t('dashboard.toast.export_no_accounts'), 'warning');
                return;
            }

            // 1. Prepare Content first
            const exportData = accountsToExport.map(acc => ({
                email: acc.email,
                refresh_token: acc.token.refresh_token
            }));
            const content = JSON.stringify(exportData, null, 2);

            let path: string | null = null;

            // 2. Determine Path
            if (config?.default_export_path) {
                // Use default path
                const fileName = `antigravity_accounts_${new Date().toISOString().split('T')[0]}.json`;
                path = await join(config.default_export_path, fileName);
            } else {
                // Use Native Dialog
                path = await save({
                    filters: [{
                        name: 'JSON',
                        extensions: ['json']
                    }],
                    defaultPath: `antigravity_accounts_${new Date().toISOString().split('T')[0]}.json`
                });
            }

            if (!path) return; // Cancelled

            // 3. Write File
            await invoke('save_text_file', { path, content });

            showToast(`${t('common.success')} ${path}`, 'success');
        } catch (error) {
            console.error('Export failed:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleExport = () => {
        const idsToExport = selectedIds.size > 0
            ? Array.from(selectedIds)
            : accounts.map(a => a.id);

        const accountsToExport = accounts.filter(a => idsToExport.includes(a.id));
        exportAccountsToJson(accountsToExport);
    };

    const handleExportOne = (accountId: string) => {
        const account = accounts.find(a => a.id === accountId);
        if (account) {
            exportAccountsToJson([account]);
        }
    };

    const handleViewDetails = (accountId: string) => {
        const account = accounts.find(a => a.id === accountId);
        if (account) {
            setDetailsAccount(account);
        }
    };

    return (
        <div className="h-full flex flex-col p-5 gap-4 max-w-7xl mx-auto w-full">
            {/* 测试按钮 - 在最顶部 */}

            {/* 顶部工具栏：搜索、过滤和操作按钮 */}
            <div className="flex-none flex items-center gap-4">
                {/* 搜索框 */}
                <div className="flex-1 max-w-md relative">
                    <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-gray-400" />
                    <input
                        type="text"
                        placeholder={t('accounts.search_placeholder')}
                        className="w-full pl-9 pr-4 py-2 bg-white dark:bg-base-100 text-sm text-gray-900 dark:text-base-content border border-gray-200 dark:border-base-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent placeholder:text-gray-400 dark:placeholder:text-gray-500"
                        value={searchQuery}
                        onChange={(e) => setSearchQuery(e.target.value)}
                    />
                </div>

                {/* 视图切换按钮组 */}
                <div className="flex gap-1 bg-gray-100 dark:bg-base-200 p-1 rounded-lg">
                    <button
                        className={`p-1.5 rounded-md transition-all ${viewMode === 'list'
                            ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-sm'
                            : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                            }`}
                        onClick={() => setViewMode('list')}
                        title={t('accounts.views.list')}
                    >
                        <List className="w-4 h-4" />
                    </button>
                    <button
                        className={`p-1.5 rounded-md transition-all ${viewMode === 'grid'
                            ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-sm'
                            : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                            }`}
                        onClick={() => setViewMode('grid')}
                        title={t('accounts.views.grid')}
                    >
                        <LayoutGrid className="w-4 h-4" />
                    </button>
                </div>

                {/* 过滤按钮组 */}
                <div className="flex gap-1 bg-gray-100 dark:bg-base-200 p-1 rounded-lg">
                    <button
                        className={`px-3 py-1.5 rounded-md text-xs font-medium transition-all ${filter === 'all'
                            ? 'bg-gray-200 dark:bg-gray-700 text-gray-900 dark:text-gray-100 shadow-sm'
                            : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                            }`}
                        onClick={() => setFilter('all')}
                    >
                        {t('accounts.all')} ({accounts.length})
                    </button>
                    <button
                        className={`px-3 py-1.5 rounded-md text-xs font-medium transition-all ${filter === 'available'
                            ? 'bg-gray-200 dark:bg-gray-700 text-gray-900 dark:text-gray-100 shadow-sm'
                            : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                            }`}
                        onClick={() => setFilter('available')}
                    >
                        {t('accounts.available')}
                    </button>
                    <button
                        className={`px-3 py-1.5 rounded-md text-xs font-medium transition-all ${filter === 'low'
                            ? 'bg-gray-200 dark:bg-gray-700 text-gray-900 dark:text-gray-100 shadow-sm'
                            : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                            }`}
                        onClick={() => setFilter('low')}
                    >
                        {t('accounts.low_quota')}
                    </button>
                </div>

                <div className="flex-1"></div>

                {/* 操作按钮组 */}
                <div className="flex items-center gap-2">
                    <AddAccountDialog onAdd={handleAddAccount} />

                    {selectedIds.size > 0 && (
                        <button
                            className="px-3 py-2 bg-red-500 text-white text-xs font-medium rounded-lg hover:bg-red-600 transition-colors flex items-center gap-1.5 shadow-sm"
                            onClick={handleBatchDelete}
                        >
                            <Trash2 className="w-3.5 h-3.5" />
                            {t('accounts.delete_selected', { count: selectedIds.size })}
                        </button>
                    )}

                    <button
                        className={`px-3 py-2 bg-blue-500 text-white text-xs font-medium rounded-lg hover:bg-blue-600 transition-colors flex items-center gap-1.5 shadow-sm ${isRefreshing ? 'opacity-70 cursor-not-allowed' : ''}`}
                        onClick={handleRefreshClick}
                        disabled={isRefreshing}
                    >
                        <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin' : ''}`} />
                        {isRefreshing ? t('common.loading') : (selectedIds.size > 0 ? t('accounts.refresh_selected', { count: selectedIds.size }) : t('accounts.refresh_all'))}
                    </button>

                    <button
                        className="px-3 py-2 border border-gray-200 dark:border-base-300 text-gray-700 dark:text-gray-300 text-xs font-medium rounded-lg hover:bg-gray-50 dark:hover:bg-base-200 transition-colors flex items-center gap-1.5"
                        onClick={handleExport}
                    >
                        <Download className="w-3.5 h-3.5" />
                        {selectedIds.size > 0 ? t('accounts.export_selected', { count: selectedIds.size }) : t('common.export')}
                    </button>
                </div>
            </div>

            {/* 账号列表内容区域 */}
            <div className="flex-1 min-h-0 relative">
                {viewMode === 'list' ? (
                    <div className="h-full bg-white dark:bg-base-100 rounded-2xl shadow-sm border border-gray-100 dark:border-base-200 flex flex-col overflow-hidden">
                        <div className="flex-1 overflow-y-auto">
                            <AccountTable
                                accounts={paginatedAccounts}
                                selectedIds={selectedIds}
                                refreshingIds={refreshingIds}
                                onToggleSelect={handleToggleSelect}
                                onToggleAll={handleToggleAll}
                                currentAccountId={currentAccount?.id || null}
                                switchingAccountId={switchingAccountId}
                                onSwitch={handleSwitch}
                                onRefresh={handleRefresh}
                                onViewDetails={handleViewDetails}
                                onExport={handleExportOne}
                                onDelete={handleDelete}
                            />
                        </div>
                    </div>
                ) : (
                    <div className="h-full overflow-y-auto">
                        <AccountGrid
                            accounts={paginatedAccounts}
                            selectedIds={selectedIds}
                            refreshingIds={refreshingIds}
                            onToggleSelect={handleToggleSelect}
                            currentAccountId={currentAccount?.id || null}
                            switchingAccountId={switchingAccountId}
                            onSwitch={handleSwitch}
                            onRefresh={handleRefresh}
                            onViewDetails={handleViewDetails}
                            onExport={handleExportOne}
                            onDelete={handleDelete}
                        />
                    </div>
                )}
            </div>

            {/* 极简分页 - 无边框浮动样式 */}
            {
                filteredAccounts.length > 0 && (
                    <div className="flex-none">
                        <Pagination
                            currentPage={currentPage}
                            totalPages={Math.ceil(filteredAccounts.length / ITEMS_PER_PAGE)}
                            onPageChange={handlePageChange}
                            totalItems={filteredAccounts.length}
                            itemsPerPage={ITEMS_PER_PAGE}
                        />
                    </div>
                )
            }

            <AccountDetailsDialog
                account={detailsAccount}
                onClose={() => setDetailsAccount(null)}
            />

            <ModalDialog
                isOpen={!!deleteConfirmId || isBatchDelete}
                title={isBatchDelete ? t('accounts.dialog.batch_delete_title') : t('accounts.dialog.delete_title')}
                message={isBatchDelete
                    ? t('accounts.dialog.batch_delete_msg', { count: selectedIds.size })
                    : t('accounts.dialog.delete_msg')
                }
                type="confirm"
                confirmText={t('common.delete')}
                isDestructive={true}
                onConfirm={isBatchDelete ? executeBatchDelete : executeDelete}
                onCancel={() => { setDeleteConfirmId(null); setIsBatchDelete(false); }}
            />

            <ModalDialog
                isOpen={isRefreshConfirmOpen}
                title={selectedIds.size > 0 ? t('accounts.dialog.batch_refresh_title') : t('accounts.dialog.refresh_title')}
                message={selectedIds.size > 0
                    ? t('accounts.dialog.batch_refresh_msg', { count: selectedIds.size })
                    : t('accounts.dialog.refresh_msg')
                }
                type="confirm"
                confirmText={t('common.refresh')}
                isDestructive={false}
                onConfirm={executeRefresh}
                onCancel={() => setIsRefreshConfirmOpen(false)}
            />
        </div >
    );
}

export default Accounts;
