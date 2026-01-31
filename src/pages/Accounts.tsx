import { useState, useEffect, useMemo, useRef, useCallback } from 'react';
import { save, open } from '@tauri-apps/plugin-dialog';
import { request as invoke } from '../utils/request';
import { join } from '@tauri-apps/api/path';
import { Search, RefreshCw, Download, Upload, Trash2, LayoutGrid, List, Sparkles, Users } from 'lucide-react';
import { motion } from 'framer-motion';
import { useAccountStore } from '../stores/useAccountStore';
import { useConfigStore } from '../stores/useConfigStore';
import AccountTable from '../components/accounts/AccountTable';
import AccountGrid from '../components/accounts/AccountGrid';
import DeviceFingerprintDialog from '../components/accounts/DeviceFingerprintDialog';
import AccountDetailsDialog from '../components/accounts/AccountDetailsDialog';
import AddAccountDialog from '../components/accounts/AddAccountDialog';
import ModalDialog from '../components/common/ModalDialog';
import Pagination from '../components/common/Pagination';
import { showToast } from '../components/common/ToastContainer';
import { Account } from '../types/account';
import { cn } from '../utils/cn';
import { isTauri } from '../utils/env';

// ... (省略中间代码)


type FilterType = 'all' | 'pro' | 'ultra' | 'free';
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
        deleteAccounts,
        switchAccount,
        loading,
        refreshQuota,
        toggleProxyStatus,
        reorderAccounts,
        warmUpAccounts,
        warmUpAccount,
    } = useAccountStore();
    const { config } = useConfigStore();

    // Extract selected accounts for proxy (scheduling mode) - only if badge is enabled
    const proxySelectedAccountIds = useMemo(() => {
        // Check if badge display is enabled in settings (default true)
        if (config?.show_proxy_selected_badge === false) {
            return new Set<string>();
        }
        const scheduling = config?.proxy?.scheduling;
        if (scheduling?.mode === 'Selected' && scheduling?.selected_accounts) {
            return new Set(scheduling.selected_accounts);
        }
        return new Set<string>();
    }, [config?.proxy?.scheduling, config?.show_proxy_selected_badge]);

    const [searchQuery, setSearchQuery] = useState('');
    const [filter, setFilter] = useState<FilterType>('all');
    const [viewMode, setViewMode] = useState<ViewMode>(() => {
        const saved = localStorage.getItem('accounts_view_mode');
        return (saved === 'list' || saved === 'grid') ? saved : 'list';
    });

    // Save view mode preference
    useEffect(() => {
        localStorage.setItem('accounts_view_mode', viewMode);
    }, [viewMode]);
    const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
    const [deviceAccount, setDeviceAccount] = useState<Account | null>(null);
    const [detailsAccount, setDetailsAccount] = useState<Account | null>(null);
    const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);
    const [isBatchDelete, setIsBatchDelete] = useState(false);
    const [toggleProxyConfirm, setToggleProxyConfirm] = useState<{ accountId: string; enable: boolean } | null>(null);
    const [isWarmupConfirmOpen, setIsWarmupConfirmOpen] = useState(false);

    const [refreshingIds, setRefreshingIds] = useState<Set<string>>(new Set());


    const handleWarmup = useCallback(async (accountId: string) => {
        setRefreshingIds(prev => {
            const next = new Set(prev);
            next.add(accountId);
            return next;
        });
        try {
            const msg = await warmUpAccount(accountId);
            showToast(msg, 'success');
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        } finally {
            setRefreshingIds(prev => {
                const next = new Set(prev);
                next.delete(accountId);
                return next;
            });
        }
    }, [warmUpAccount, t]);

    const handleWarmupAll = async () => {
        setIsWarmupConfirmOpen(false);

        try {
            const isBatch = selectedIds.size > 0;
            if (isBatch) {
                const ids = Array.from(selectedIds);
                setRefreshingIds(new Set(ids));
                const results = await Promise.allSettled(ids.map(id => warmUpAccount(id)));
                let successCount = 0;
                results.forEach(r => { if (r.status === 'fulfilled') successCount++; });
                showToast(t('accounts.warmup_batch_triggered', { count: successCount }), 'success');
            } else {
                const msg = await warmUpAccounts();
                if (msg) {
                    showToast(msg, 'success');
                } else {
                    showToast(t('accounts.warmup_all_triggered', '全量预热任务已触发'), 'success');
                }
            }
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        } finally {

            setRefreshingIds(new Set());
        }
    };



    const fileInputRef = useRef<HTMLInputElement>(null);
    const containerRef = useRef<HTMLDivElement>(null);
    const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });

    useEffect(() => {
        if (!containerRef.current) return;
        const resizeObserver = new ResizeObserver((entries) => {
            for (let entry of entries) {
                setContainerSize({
                    width: entry.contentRect.width,
                    height: entry.contentRect.height
                });
            }
        });
        resizeObserver.observe(containerRef.current);
        return () => resizeObserver.disconnect();
    }, []);

    // Pagination State
    const [currentPage, setCurrentPage] = useState(1);
    const [localPageSize, setLocalPageSize] = useState<number | null>(() => {
        const saved = localStorage.getItem('accounts_page_size');
        return saved ? parseInt(saved) : null;
    }); // 本地分页大小状态

    // Save page size preference
    useEffect(() => {
        if (localPageSize !== null) {
            localStorage.setItem('accounts_page_size', localPageSize.toString());
        }
    }, [localPageSize]);

    // 动态计算分页条数
    const ITEMS_PER_PAGE = useMemo(() => {
        // 优先使用本地设置的分页大小
        if (localPageSize && localPageSize > 0) {
            return localPageSize;
        }

        // 其次使用用户配置的固定值
        if (config?.accounts_page_size && config.accounts_page_size > 0) {
            return config.accounts_page_size;
        }

        // 回退到原有的动态计算逻辑
        if (!containerSize.height) return viewMode === 'grid' ? 6 : 8;

        if (viewMode === 'list') {
            const headerHeight = 36; // 缩深后的表头高度
            const rowHeight = 72;    // 包含多行模型信息后的实际行高
            // 计算能容纳多少行, 默认最低 10 行
            const autoFitCount = Math.floor((containerSize.height - headerHeight) / rowHeight);
            return Math.max(10, autoFitCount);
        } else {
            const cardHeight = 180; // AccountCard 实际高度 (含间距)
            const gap = 16;         // gap-4

            // 匹配 Tailwind 断点逻辑
            let cols = 1;
            if (containerSize.width >= 1200) cols = 4;      // xl (约为 1280 左右)
            else if (containerSize.width >= 900) cols = 3;   // lg (约为 1024 左右)
            else if (containerSize.width >= 600) cols = 2;   // md (约为 768 左右)

            const rows = Math.max(1, Math.floor((containerSize.height + gap) / (cardHeight + gap)));
            return cols * rows;
        }
    }, [localPageSize, config?.accounts_page_size, containerSize, viewMode]);

    useEffect(() => {
        fetchAccounts();
    }, []);

    // Reset pagination when view mode changes to avoid empty pages or confusion
    useEffect(() => {
        setCurrentPage(1);
    }, [viewMode]);

    // 搜索过滤逻辑
    const searchedAccounts = useMemo(() => {
        if (!searchQuery) return accounts;
        const lowQuery = searchQuery.toLowerCase();
        return accounts.filter(a => a.email.toLowerCase().includes(lowQuery));
    }, [accounts, searchQuery]);

    // 计算各筛选状态下的数量 (基于搜索结果)
    const filterCounts = useMemo(() => {
        return {
            all: searchedAccounts.length,
            pro: searchedAccounts.filter(a => a.quota?.subscription_tier?.toLowerCase().includes('pro')).length,
            ultra: searchedAccounts.filter(a => a.quota?.subscription_tier?.toLowerCase().includes('ultra')).length,
            free: searchedAccounts.filter(a => {
                const tier = a.quota?.subscription_tier?.toLowerCase();
                return tier && !tier.includes('pro') && !tier.includes('ultra');
            }).length,
        };
    }, [searchedAccounts]);

    // 过滤和搜索最终结果
    const filteredAccounts = useMemo(() => {
        let result = searchedAccounts;

        if (filter === 'pro') {
            result = result.filter(a => a.quota?.subscription_tier?.toLowerCase().includes('pro'));
        } else if (filter === 'ultra') {
            result = result.filter(a => a.quota?.subscription_tier?.toLowerCase().includes('ultra'));
        } else if (filter === 'free') {
            result = result.filter(a => {
                const tier = a.quota?.subscription_tier?.toLowerCase();
                return tier && !tier.includes('pro') && !tier.includes('ultra');
            });
        }

        return result;
    }, [searchedAccounts, filter]);

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

    const handleToggleSelect = useCallback((id: string) => {
        setSelectedIds(prev => {
            const newSet = new Set(prev);
            if (newSet.has(id)) {
                newSet.delete(id);
            } else {
                newSet.add(id);
            }
            return newSet;
        });
    }, []);

    const handleToggleAll = useCallback(() => {
        // 全选当前页的所有项
        const currentIds = paginatedAccounts.map(a => a.id);
        
        setSelectedIds(prev => {
            const allSelected = currentIds.every(id => prev.has(id));
            const newSet = new Set(prev);
            if (allSelected) {
                currentIds.forEach(id => newSet.delete(id));
            } else {
                currentIds.forEach(id => newSet.add(id));
            }
            return newSet;
        });
    }, [paginatedAccounts]);

    const handleAddAccount = async (email: string, refreshToken: string) => {
        await addAccount(email, refreshToken);
    };

    const [switchingAccountId, setSwitchingAccountId] = useState<string | null>(null);

    const handleSwitch = useCallback(async (accountId: string) => {
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
    }, [loading, switchingAccountId, switchAccount, t]);

    const handleRefresh = useCallback(async (accountId: string) => {
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
    }, [refreshQuota, t]);

    const handleBatchDelete = () => {
        if (selectedIds.size === 0) return;
        setIsBatchDelete(true);
    };

    const executeBatchDelete = async () => {
        setIsBatchDelete(false);
        try {
            const ids = Array.from(selectedIds);
            console.log('[Accounts] Batch deleting:', ids);
            await deleteAccounts(ids);
            setSelectedIds(new Set());
            console.log('[Accounts] Batch delete success');
            showToast(t('common.success'), 'success');
        } catch (error) {
            console.error('[Accounts] Batch delete failed:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleDelete = useCallback((accountId: string) => {
        console.log('[Accounts] Request to delete:', accountId);
        setDeleteConfirmId(accountId);
    }, []);

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

    const handleToggleProxy = useCallback((accountId: string, currentlyDisabled: boolean) => {
        setToggleProxyConfirm({ accountId, enable: currentlyDisabled });
    }, []);

    const executeToggleProxy = async () => {
        if (!toggleProxyConfirm) return;

        try {
            await toggleProxyStatus(
                toggleProxyConfirm.accountId,
                toggleProxyConfirm.enable,
                toggleProxyConfirm.enable ? undefined : t('accounts.proxy_disabled_reason_manual')
            );
            showToast(t('common.success'), 'success');
        } catch (error) {
            console.error('[Accounts] Toggle proxy status failed:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        } finally {
            setToggleProxyConfirm(null);
        }
    };




    const [isRefreshConfirmOpen, setIsRefreshConfirmOpen] = useState(false);

    const handleRefreshClick = () => {
        setIsRefreshConfirmOpen(true);
    };

    const executeRefresh = async () => {
        setIsRefreshConfirmOpen(false);

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
                // You might want to show details in a different way, but for toast, keep it simple or use a "view details" action if supported. 
                // For now, simpler toast is better than a huge alert.
                if (details.length > 0) {
                    console.warn('Refresh failures:', details);
                }
            }
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        } finally {

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
            const fileName = `antigravity_accounts_${new Date().toISOString().split('T')[0]}.json`;

            // 2. Determine Path & Export
            if (isTauri()) {
                let path: string | null = null;
                if (config?.default_export_path) {
                    // Use default path
                    path = await join(config.default_export_path, fileName);
                } else {
                    // Use Native Dialog
                    path = await save({
                        filters: [{
                            name: 'JSON',
                            extensions: ['json']
                        }],
                        defaultPath: fileName
                    });
                }

                if (!path) return; // Cancelled

                // 3. Write File
                await invoke('save_text_file', { path, content });
                showToast(`${t('common.success')} ${path}`, 'success');
            } else {
                // Web 模式：使用浏览器下载
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

    const handleExportOne = useCallback(async (accountId: string) => {
        try {
            const account = accounts.find(a => a.id === accountId);
            if (!account) return;

            // Assuming 'save' and 'invoke' are available in this scope (Tauri context)
            // This new implementation exports the full account object, not just email/refresh_token
            const path = await save({
                filters: [{ name: 'JSON', extensions: ['json'] }],
                defaultPath: `${account.email}_export.json`,
            });
            if (path) {
                await invoke('save_text_file', { path, content: JSON.stringify(account, null, 2) });
                showToast(t('common.success'), 'success');
            }
        } catch (error) {
            console.error(error);
            showToast(t('common.error'), 'error');
        }
    }, [accounts, t]);

    const processImportData = async (content: string) => {
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
                await addAccount(entry.email || '', entry.refresh_token!);
                successCount++;
            } catch (error) {
                console.error('Import account failed:', error);
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
    };

    const handleImportJson = async () => {
        if (isTauri()) {
            try {
                const selected = await open({
                    multiple: false,
                    filters: [{
                        name: 'JSON',
                        extensions: ['json']
                    }]
                });
                if (!selected || typeof selected !== 'string') return;

                const content: string = await invoke('read_text_file', { path: selected });
                await processImportData(content);
            } catch (error) {
                console.error('Import failed:', error);
                showToast(t('accounts.import_fail', { error: String(error) }), 'error');
            }
        } else {
            // Web 模式: 触发隐藏的 file input
            fileInputRef.current?.click();
        }
    };

    const handleFileChange = async (event: React.ChangeEvent<HTMLInputElement>) => {
        const file = event.target.files?.[0];
        if (!file) return;

        try {
            const content = await file.text();
            await processImportData(content);
        } catch (error) {
            console.error('Import failed:', error);
            showToast(t('accounts.import_fail', { error: String(error) }), 'error');
        } finally {
            // 重置 input,允许重复选择同一文件
            event.target.value = '';
        }
    };

    const handleViewDetails = useCallback((accountId: string) => {
        const account = accounts.find(a => a.id === accountId);
        if (account) setDetailsAccount(account);
    }, [accounts]);
    const handleViewDevice = useCallback((accountId: string) => {
        const account = accounts.find(a => a.id === accountId);
        if (account) setDeviceAccount(account);
    }, [accounts]);


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

            {/* Combined Card: Header + Toolbar + Accounts List */}
            <div className="flex-1 min-h-0 relative flex flex-col" ref={containerRef}>
                <div className="h-full bg-white dark:bg-zinc-900/40 backdrop-blur-xl rounded-2xl border border-white/5 flex flex-col overflow-hidden shadow-2xl">
                    
                    {/* Compact Header with Title + Stats + Add Button */}
                    <div className="flex-none flex items-center justify-between px-5 py-4 border-b border-white/5 bg-gradient-to-r from-zinc-900/80 to-zinc-900/40">
                        <div className="flex items-center gap-4">
                            {/* Icon */}
                            <div className="p-2.5 rounded-xl bg-gradient-to-br from-indigo-500 to-purple-600 shadow-lg shadow-indigo-500/20">
                                <Users className="w-5 h-5 text-white" />
                            </div>
                            {/* Title & Stats */}
                            <div>
                                <h1 className="text-xl font-bold text-white tracking-tight">
                                    {t('nav.accounts')}
                                </h1>
                                <p className="text-xs text-zinc-500 mt-0.5">
                                    {searchedAccounts.length} {t('common.accounts', 'accounts active')}
                                </p>
                            </div>
                        </div>

                        {/* Add Account Button - Dashboard Style */}
                        <AddAccountDialog onAdd={handleAddAccount} />
                    </div>
                    
                    {/* Toolbar */}
                    <div className="flex-none flex items-center gap-2 lg:gap-4 p-3 border-b border-white/5 bg-white/5">
                        {/* Search Input */}
                        <div className="relative group min-w-[180px] lg:min-w-[280px]">
                            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-zinc-500 group-focus-within:text-indigo-400 transition-colors" />
                            <input
                                type="text"
                                placeholder={t('accounts.search_placeholder')}
                                className="w-full h-9 pl-10 pr-4 bg-zinc-900/50 border border-white/5 rounded-lg focus:outline-none focus:border-indigo-500/50 focus:bg-zinc-800/50 transition-all text-xs placeholder:text-zinc-600 text-zinc-200"
                                value={searchQuery}
                                onChange={(e) => setSearchQuery(e.target.value)}
                            />
                        </div>

                        {/* Divider */}
                        <div className="w-px h-6 bg-white/5 my-auto shrink-0" />

                        {/* Filter Tabs */}
                        <div className="flex items-center bg-zinc-900/50 p-0.5 rounded-lg border border-white/5 shrink-0">
                            {['all', 'pro', 'ultra', 'free'].map((type) => (
                                <button
                                    key={type}
                                    onClick={() => setFilter(type as any)}
                                    className={cn(
                                        "relative px-3 py-1 rounded-md text-[10px] font-bold uppercase tracking-wider transition-all z-10",
                                        filter === type ? "text-white" : "text-zinc-500 hover:text-zinc-300"
                                    )}
                                >
                                    {filter === type && (
                                        <motion.div
                                            layoutId="activeFilter"
                                            className="absolute inset-0 bg-zinc-700/80 rounded-md shadow-sm"
                                            transition={{ type: "spring", bounce: 0.2, duration: 0.6 }}
                                            style={{ zIndex: -1 }}
                                        />
                                    )}
                                    <span className="relative flex items-center gap-1.5">
                                        <span className="hidden sm:inline">{type}</span>
                                        <span className="sm:hidden">{type.charAt(0)}</span>
                                        <span className={cn(
                                            "px-1 py-0.5 rounded text-[8px]",
                                            filter === type ? "bg-white/10 text-white" : "bg-zinc-800 text-zinc-600"
                                        )}>
                                            {filterCounts[type as keyof typeof filterCounts]}
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
                            <div className="flex items-center bg-zinc-900/50 p-0.5 rounded-lg border border-white/5 mr-2">
                                <button
                                    onClick={() => setViewMode('list')}
                                    className={cn(
                                        "p-1.5 rounded-md transition-all",
                                        viewMode === 'list' ? "bg-zinc-700 text-white shadow-sm" : "text-zinc-500 hover:text-zinc-300"
                                    )}
                                    title={t('accounts.list_view')}
                                >
                                    <List className="w-3.5 h-3.5" />
                                </button>
                                <button
                                    onClick={() => setViewMode('grid')}
                                    className={cn(
                                        "p-1.5 rounded-md transition-all",
                                        viewMode === 'grid' ? "bg-zinc-700 text-white shadow-sm" : "text-zinc-500 hover:text-zinc-300"
                                    )}
                                    title={t('accounts.grid_view')}
                                >
                                    <LayoutGrid className="w-3.5 h-3.5" />
                                </button>
                            </div>

                            {selectedIds.size > 0 ? (
                                <>
                                    <button 
                                        onClick={handleExport}
                                        className="h-8 px-3 rounded-lg bg-indigo-500/10 hover:bg-indigo-500/20 text-indigo-400 border border-indigo-500/20 hover:border-indigo-500/40 transition-all flex items-center gap-2"
                                    >
                                        <Download className="w-3.5 h-3.5" />
                                        <span className="text-[10px] font-bold">EXP ({selectedIds.size})</span>
                                    </button>
                                    <button 
                                        onClick={handleBatchDelete}
                                        className="h-8 px-3 rounded-lg bg-rose-500/10 hover:bg-rose-500/20 text-rose-400 border border-rose-500/20 hover:border-rose-500/40 transition-all flex items-center gap-2"
                                    >
                                        <Trash2 className="w-3.5 h-3.5" />
                                        <span className="text-[10px] font-bold">DEL ({selectedIds.size})</span>
                                    </button>
                                </>
                            ) : (
                                <>
                                    <ActionIcon 
                                        icon={RefreshCw} 
                                        onClick={handleRefreshClick} 
                                        label={t('common.refresh')} 
                                        tooltip={t('accounts.refresh_all_tooltip')}
                                        className="h-8 px-2 text-xs"
                                        iconSize={14}
                                    />
                                    <ActionIcon 
                                        icon={Sparkles} 
                                        onClick={() => setIsWarmupConfirmOpen(true)} 
                                        label={t('accounts.warmup_all')} 
                                        tooltip="One-Click Warmup"
                                        className="text-amber-400 hover:bg-amber-500/10 hover:text-amber-300 h-8 px-2 text-xs"
                                        iconSize={14}
                                    />
                                    <div className="w-px h-5 bg-white/5 mx-1" />
                                    <ActionIcon 
                                        icon={Upload} 
                                        onClick={handleImportJson} 
                                        label={t('common.import')} 
                                        tooltip={t('accounts.import_tooltip')}
                                        className="h-8 px-2 text-xs"
                                        iconSize={14}
                                    />
                                    <ActionIcon 
                                        icon={Download} 
                                        onClick={handleExport} 
                                        label={t('common.export')} 
                                        tooltip={t('accounts.export_tooltip')}
                                        className="h-8 px-2 text-xs"
                                        iconSize={14}
                                    />
                                </>
                            )}
                        </div>
                    </div>

                    {/* Content Area */}
                    <div className="flex-1 min-h-0 overflow-y-auto p-0 scrollbar-thin scrollbar-thumb-white/10 scrollbar-track-transparent">
                        {viewMode === 'list' ? (
                            <div className="p-2 space-y-1">
                                <AccountTable
                                    accounts={paginatedAccounts}
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
                                    onDelete={handleDelete}
                                    onToggleProxy={(id: string) => handleToggleProxy(id, !!accounts.find(a => a.id === id)?.proxy_disabled)}
                                    onReorder={reorderAccounts}
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
                                    onDelete={handleDelete}
                                    onToggleProxy={(id) => handleToggleProxy(id, !!accounts.find(a => a.id === id)?.proxy_disabled)}
                                    onWarmup={handleWarmup}
                                />
                            </div>
                        )}
                    </div>
                </div>
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
                            onPageSizeChange={(newSize) => {
                                setLocalPageSize(newSize);
                                setCurrentPage(1); // 重置到第一页
                            }}
                            pageSizeOptions={[10, 20, 50, 100]}
                        />
                    </div>
                )
            }

            <AccountDetailsDialog
                account={detailsAccount}
                onClose={() => setDetailsAccount(null)}
            />
            <DeviceFingerprintDialog
                account={deviceAccount}
                onClose={() => setDeviceAccount(null)}
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

            {toggleProxyConfirm && (
                <ModalDialog
                    isOpen={!!toggleProxyConfirm}
                    onCancel={() => setToggleProxyConfirm(null)}
                    onConfirm={executeToggleProxy}
                    title={toggleProxyConfirm.enable ? t('accounts.dialog.enable_proxy_title') : t('accounts.dialog.disable_proxy_title')}
                    message={toggleProxyConfirm.enable ? t('accounts.dialog.enable_proxy_msg') : t('accounts.dialog.disable_proxy_msg')}
                />
            )}

            <ModalDialog
                isOpen={isWarmupConfirmOpen}
                title={selectedIds.size > 0 ? t('accounts.dialog.batch_warmup_title', '批量手动预热') : t('accounts.dialog.warmup_all_title', '全量手动预热')}
                message={selectedIds.size > 0
                    ? t('accounts.dialog.batch_warmup_msg', '确定要为选中的 {{count}} 个账号立即触发预热吗？', { count: selectedIds.size })
                    : t('accounts.dialog.warmup_all_msg', '确定要立即为所有符合条件的账号触发预热任务吗？这将向 Google 服务发送极小流量。')
                }
                type="confirm"
                confirmText={t('accounts.warmup_now', '立即预热')}
                isDestructive={false}
                onConfirm={handleWarmupAll}
                onCancel={() => setIsWarmupConfirmOpen(false)}
            />
        </div >
    );
}

// Helper Component for Header Actions
function ActionIcon({ icon: Icon, onClick, label, tooltip, className }: any) {
    return (
        <button
            onClick={onClick}
            title={tooltip}
            className={cn(
                "h-10 px-3 rounded-xl flex items-center gap-2 transition-all border border-transparent",
                "text-zinc-400 hover:text-white hover:bg-zinc-800 hover:border-white/10",
                className
            )}
        >
            <Icon className="w-4 h-4" />
            <span className="text-xs font-bold tracking-wide">{label}</span>
        </button>
    );
}

export default Accounts;
