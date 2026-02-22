

import {
  Download,
  LayoutGrid,
  List,
  RefreshCw,
  Search,
  Sparkles,
  ToggleLeft,
  ToggleRight,
  Trash2,
  Upload,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import AccountDetailsDialog from "../components/accounts/AccountDetailsDialog";
import AccountGrid from "../components/accounts/AccountGrid";
import AccountTable from "../components/accounts/AccountTable";
import AddAccountDialog from "../components/accounts/AddAccountDialog";
import DeviceFingerprintDialog from "../components/accounts/DeviceFingerprintDialog";
import ModalDialog from "../components/common/ModalDialog";
import Pagination from "../components/common/Pagination";
import AccountErrorDialog from "../components/accounts/AccountErrorDialog";
import { showToast } from "../components/common/ToastContainer";
import { exportAccounts } from "../services/accountService";
import { useAccountStore } from "../stores/useAccountStore";
import { useConfigStore } from "../stores/useConfigStore";
import { Account } from "../types/account";
import { cn } from "../utils/cn";
import { isTauri } from "../utils/env";
import { request as invoke } from "../utils/request";
import { useTranslation } from "react-i18next";

type FilterType = "all" | "pro" | "ultra" | "free" | string;
type ViewMode = "list" | "grid";


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
    updateAccountLabel,
  } = useAccountStore();
  const { config, showAllQuotas, toggleShowAllQuotas } = useConfigStore();

  const [searchQuery, setSearchQuery] = useState('');
  const [filter, setFilter] = useState<FilterType>('all');
  const [isSearchExpanded, setIsSearchExpanded] = useState(false);
  const searchInputRef = useRef<HTMLInputElement>(null);
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
  const [toggleProxyConfirm, setToggleProxyConfirm] = useState<{
    accountId: string;
    enable: boolean;
  } | null>(null);
  const [isWarmupConfirmOpen, setIsWarmupConfirmOpen] = useState(false);
  const [isWarmuping, setIsWarmuping] = useState(false);
  const [refreshingIds, setRefreshingIds] = useState<Set<string>>(new Set());
  const [errorAccountId, setErrorAccountId] = useState<string | null>(null);

  const handleWarmup = async (accountId: string) => {
    setRefreshingIds((prev) => {
      const next = new Set(prev);
      next.add(accountId);
      return next;
    });
    try {
      const msg = await warmUpAccount(accountId);
      showToast(msg, "success");
    } catch (error) {
      showToast(`${t("common.error")}: ${error}`, "error");
    } finally {
      setRefreshingIds((prev) => {
        const next = new Set(prev);
        next.delete(accountId);
        return next;
      });
    }
  };

  const handleUpdateLabel = async (accountId: string, label: string) => {
    try {
      await updateAccountLabel(accountId, label);
      showToast(t('accounts.label_updated', 'Label updated'), 'success');
    } catch (error) {
      showToast(`${t('common.error')}: ${error}`, 'error');
    }
  };

  const handleWarmupAll = async () => {
    setIsWarmupConfirmOpen(false);
    setIsWarmuping(true);
    try {
      const isBatch = selectedIds.size > 0;
      if (isBatch) {
        const ids = Array.from(selectedIds);
        setRefreshingIds(new Set(ids));
        const results = await Promise.allSettled(
          ids.map((id) => warmUpAccount(id)),
        );
        let successCount = 0;
        results.forEach((r) => {
          if (r.status === "fulfilled") successCount++;
        });
        showToast(
          t("accounts.warmup_batch_triggered", { count: successCount }),
          "success",
        );
      } else {
        const msg = await warmUpAccounts();
        if (msg) {
          showToast(msg, "success");
        } else {
          showToast(
            t("accounts.warmup_all_triggered", "全量预热任务已触发"),
            "success",
          );
        }
      }
    } catch (error) {
      showToast(`${t("common.error")}: ${error}`, "error");
    } finally {
      setIsWarmuping(false);
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
          height: entry.contentRect.height,
        });
      }
    });
    resizeObserver.observe(containerRef.current);
    return () => resizeObserver.disconnect();
  }, []);

  // Pagination State
  const [currentPage, setCurrentPage] = useState(1);
  const [localPageSize, setLocalPageSize] = useState<number | null>(() => {
    const saved = localStorage.getItem("accounts_page_size");
    return saved ? parseInt(saved) : null;
  }); // 本地分页大小状态

  // Save page size preference
  useEffect(() => {
    if (localPageSize !== null) {
      localStorage.setItem("accounts_page_size", localPageSize.toString());
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
    if (!containerSize.height) return viewMode === "grid" ? 6 : 8;

    if (viewMode === "list") {
      const headerHeight = 36; // 缩深后的表头高度
      const rowHeight = 72; // 包含多行模型信息后的实际行高
      // 计算能容纳多少行, 默认最低 10 行
      const autoFitCount = Math.floor(
        (containerSize.height - headerHeight) / rowHeight,
      );
      return Math.max(10, autoFitCount);
    } else {
      const cardHeight = 180; // AccountCard 实际高度 (含间距)
      const gap = 16; // gap-4

      // 匹配 Tailwind 断点逻辑
      let cols = 1;
      if (containerSize.width >= 1200)
        cols = 4; // xl (约为 1280 左右)
      else if (containerSize.width >= 900)
        cols = 3; // lg (约为 1024 左右)
      else if (containerSize.width >= 600) cols = 2; // md (约为 768 左右)

      const rows = Math.max(
        1,
        Math.floor((containerSize.height + gap) / (cardHeight + gap)),
      );
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
    return accounts.filter((a) => a.email.toLowerCase().includes(lowQuery));
  }, [accounts, searchQuery]);

  // 计算各筛选状态下的数量 (基于搜索结果)
  const filterCounts = useMemo(() => {
    return {
      all: searchedAccounts.length,
      pro: searchedAccounts.filter((a) =>
        a.quota?.subscription_tier?.toLowerCase().includes("pro"),
      ).length,
      ultra: searchedAccounts.filter((a) =>
        a.quota?.subscription_tier?.toLowerCase().includes("ultra"),
      ).length,
      free: searchedAccounts.filter((a) => {
        const tier = a.quota?.subscription_tier?.toLowerCase();
        return tier && !tier.includes("pro") && !tier.includes("ultra");
      }).length,
    };
  }, [searchedAccounts]);

  // 收集所有唯一的自定义标签
  const uniqueLabels = useMemo(() => {
    const labels = new Set<string>();
    searchedAccounts.forEach((a) => {
      if (a.custom_label?.trim()) labels.add(a.custom_label.trim());
    });
    return Array.from(labels).sort();
  }, [searchedAccounts]);

  // 计算各标签的数量
  const labelCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    uniqueLabels.forEach((label) => {
      counts[label] = searchedAccounts.filter(
        (a) => a.custom_label?.trim() === label
      ).length;
    });
    return counts;
  }, [searchedAccounts, uniqueLabels]);

  // 过滤和搜索最终结果
  const filteredAccounts = useMemo(() => {
    let result = searchedAccounts;

    if (filter === "pro") {
      result = result.filter((a) =>
        a.quota?.subscription_tier?.toLowerCase().includes("pro"),
      );
    } else if (filter === "ultra") {
      result = result.filter((a) =>
        a.quota?.subscription_tier?.toLowerCase().includes("ultra"),
      );
    } else if (filter === "free") {
      result = result.filter((a) => {
        const tier = a.quota?.subscription_tier?.toLowerCase();
        return tier && !tier.includes("pro") && !tier.includes("ultra");
      });
    } else if (filter !== "all") {
      // 自定义标签筛选
      result = result.filter((a) => a.custom_label?.trim() === filter);
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
    const currentIds = paginatedAccounts.map((a) => a.id);
    const allSelected = currentIds.every((id) => selectedIds.has(id));

    const newSet = new Set(selectedIds);
    if (allSelected) {
      currentIds.forEach((id) => newSet.delete(id));
    } else {
      currentIds.forEach((id) => newSet.add(id));
    }
    setSelectedIds(newSet);
  };

  const handleAddAccount = async (email: string, refreshToken: string) => {
    await addAccount(email, refreshToken);
  };

  const [switchingAccountId, setSwitchingAccountId] = useState<string | null>(
    null,
  );

  const handleSwitch = async (accountId: string) => {
    if (loading || switchingAccountId) return;

    setSwitchingAccountId(accountId);
    console.log("[Accounts] handleSwitch called for:", accountId);
    try {
      await switchAccount(accountId);
      showToast(t("common.success"), "success");
    } catch (error) {
      console.error("[Accounts] Switch failed:", error);
      showToast(`${t("common.error")}: ${error}`, "error");
    } finally {
      // Add a small delay for smoother UX
      setTimeout(() => {
        setSwitchingAccountId(null);
      }, 500);
    }
  };

  const handleRefresh = async (accountId: string) => {
    setRefreshingIds((prev) => {
      const next = new Set(prev);
      next.add(accountId);
      return next;
    });
    try {
      await refreshQuota(accountId);
      await refreshQuota(accountId);
      await refreshQuota(accountId);
      showToast(t("common.success"), "success");
    } catch (error) {
      showToast(`${t("common.error")}: ${error}`, "error");
    } finally {
      setRefreshingIds((prev) => {
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
      console.log("[Accounts] Batch deleting:", ids);
      await deleteAccounts(ids);
      setSelectedIds(new Set());
      console.log("[Accounts] Batch delete success");
      showToast(t("common.success"), "success");
    } catch (error) {
      console.error("[Accounts] Batch delete failed:", error);
      showToast(`${t("common.error")}: ${error}`, "error");
    }
  };

  const handleDelete = (accountId: string) => {
    console.log("[Accounts] Request to delete:", accountId);
    setDeleteConfirmId(accountId);
  };

  const executeDelete = async () => {
    if (!deleteConfirmId) return;

    try {
      console.log("[Accounts] Executing delete for:", deleteConfirmId);
      await deleteAccount(deleteConfirmId);
      console.log("[Accounts] Delete success");
      showToast(t("common.success"), "success");
    } catch (error) {
      console.error("[Accounts] Delete failed:", error);
      showToast(`${t("common.error")}: ${error}`, "error");
    } finally {
      setDeleteConfirmId(null);
    }
  };

  const handleToggleProxy = (accountId: string, currentlyDisabled: boolean) => {
    setToggleProxyConfirm({ accountId, enable: currentlyDisabled });
  };

  const executeToggleProxy = async () => {
    if (!toggleProxyConfirm) return;

    try {
      await toggleProxyStatus(
        toggleProxyConfirm.accountId,
        toggleProxyConfirm.enable,
        toggleProxyConfirm.enable
          ? undefined
          : t("accounts.proxy_disabled_reason_manual"),
      );
      showToast(t("common.success"), "success");
    } catch (error) {
      console.error("[Accounts] Toggle proxy status failed:", error);
      showToast(`${t("common.error")}: ${error}`, "error");
    } finally {
      setToggleProxyConfirm(null);
    }
  };

  const handleBatchToggleProxy = async (enable: boolean) => {
    if (selectedIds.size === 0) return;

    try {
      const promises = Array.from(selectedIds).map((id) =>
        toggleProxyStatus(
          id,
          enable,
          enable ? undefined : t("accounts.proxy_disabled_reason_batch"),
        ),
      );
      await Promise.all(promises);
      showToast(
        enable
          ? t("accounts.toast.proxy_enabled", { count: selectedIds.size })
          : t("accounts.toast.proxy_disabled", { count: selectedIds.size }),
        "success",
      );
      setSelectedIds(new Set());
    } catch (error) {
      console.error("[Accounts] Batch toggle proxy status failed:", error);
      showToast(`${t("common.error")}: ${error}`, "error");
    }
  };

  const [isRefreshing, setIsRefreshing] = useState(false);
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

        const results = await Promise.allSettled(
          ids.map((id) => refreshQuota(id)),
        );

        results.forEach((result, index) => {
          const id = ids[index];
          const email = accounts.find((a) => a.id === id)?.email || id;
          if (result.status === "fulfilled") {
            successCount++;
          } else {
            failedCount++;
            details.push(`${email}: ${result.reason}`);
          }
        });
      } else {
        // 刷新所有
        setRefreshingIds(new Set(accounts.map((a) => a.id)));
        const stats = await useAccountStore.getState().refreshAllQuotas();
        if (stats) {
          successCount = stats.success;
          failedCount = stats.failed;
          details.push(...stats.details);
        }
      }

      if (failedCount === 0) {
        showToast(
          t("accounts.refresh_selected", { count: successCount }),
          "success",
        );
      } else {
        showToast(
          `${t("common.success")}: ${successCount}, ${t("common.error")}: ${failedCount}`,
          "warning",
        );
        // You might want to show details in a different way, but for toast, keep it simple or use a "view details" action if supported.
        // For now, simpler toast is better than a huge alert.
        if (details.length > 0) {
          console.warn("Refresh failures:", details);
        }
      }
    } catch (error) {
      showToast(`${t("common.error")}: ${error}`, "error");
    } finally {
      setIsRefreshing(false);
      setRefreshingIds(new Set());
    }
  };

  const exportAccountsToJson = async (accountsToExport: Account[]) => {
    try {
      if (accountsToExport.length === 0) {
        showToast(t("dashboard.toast.export_no_accounts"), "warning");
        return;
      }

      // 1. Get export data from API (contains refresh_token)
      const accountIds = accountsToExport.map((acc) => acc.id);
      const response = await exportAccounts(accountIds);

      if (!response.accounts || response.accounts.length === 0) {
        showToast(t("dashboard.toast.export_no_accounts"), "warning");
        return;
      }

      const exportData = response.accounts;
      const content = JSON.stringify(exportData, null, 2);
      const fileName = `antigravity_accounts_${new Date().toISOString().split("T")[0]}.json`;

      // 2. Determine Path & Export
      if (isTauri()) {
        let path: string | null = null;
        const { join } = await import("@tauri-apps/api/path");

        if (config?.default_export_path) {
          // Use default path
          path = await join(config.default_export_path, fileName);
        } else {
          // Use Native Dialog
          const { save } = await import("@tauri-apps/plugin-dialog");
          path = await save({
            filters: [
              {
                name: "JSON",
                extensions: ["json"],
              },
            ],
            defaultPath: fileName,
          });
        }

        if (!path) return; // Cancelled

        // 3. Write File
        await invoke("save_text_file", { path, content });
        showToast(`${t("common.success")} ${path}`, "success");
      } else {
        // Web 模式：使用浏览器下载
        const blob = new Blob([content], { type: "application/json" });
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = fileName;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
        showToast(
          t("dashboard.toast.export_success", { path: fileName }),
          "success",
        );
      }
    } catch (error: any) {
      console.error("Export failed:", error);
      showToast(`${t("common.error")}: ${error}`, "error");
    }
  };

  const handleExport = () => {
    const idsToExport =
      selectedIds.size > 0
        ? Array.from(selectedIds)
        : accounts.map((a) => a.id);

    const accountsToExport = accounts.filter((a) => idsToExport.includes(a.id));
    exportAccountsToJson(accountsToExport);
  };

  const handleCopyOne = async (accountId: string) => {
    try {
      const response = await exportAccounts([accountId]);
      if (!response.accounts || response.accounts.length === 0) {
        showToast(t("dashboard.toast.export_no_accounts"), "warning");
        return;
      }
      const content = JSON.stringify(response.accounts[0]);
      await navigator.clipboard.writeText(content);
      showToast(t("common.copy") + " " + t("common.success"), "success");
    } catch (error: any) {
      console.error("Copy failed:", error);
      showToast(`${t("common.error")}: ${error}`, "error");
    }
  };

  const processImportData = async (content: string) => {
    let importData: Array<{ email?: string; refresh_token?: string }>;
    try {
      importData = JSON.parse(content);
    } catch {
      showToast(t("accounts.import_invalid_format"), "error");
      return;
    }

    if (!Array.isArray(importData) || importData.length === 0) {
      showToast(t("accounts.import_invalid_format"), "error");
      return;
    }

    const validEntries = importData.filter(
      (item) =>
        item.refresh_token &&
        typeof item.refresh_token === "string" &&
        item.refresh_token.startsWith("1//"),
    );

    if (validEntries.length === 0) {
      showToast(t("accounts.import_invalid_format"), "error");
      return;
    }

    let successCount = 0;
    let failCount = 0;

    for (const entry of validEntries) {
      try {
        await addAccount(entry.email || "", entry.refresh_token!);
        successCount++;
      } catch (error) {
        console.error("Import account failed:", error);
        failCount++;
      }
      await new Promise((r) => setTimeout(r, 100));
    }

    if (failCount === 0) {
      showToast(
        t("accounts.import_success", { count: successCount }),
        "success",
      );
    } else if (successCount > 0) {
      showToast(
        t("accounts.import_partial", {
          success: successCount,
          fail: failCount,
        }),
        "warning",
      );
    } else {
      showToast(
        t("accounts.import_fail", { error: "All accounts failed to import" }),
        "error",
      );
    }
  };

  const handleImportJson = async () => {
    if (isTauri()) {
      try {
        const { open } = await import("@tauri-apps/plugin-dialog");
        const selected = await open({
          multiple: false,
          filters: [
            {
              name: "JSON",
              extensions: ["json"],
            },
          ],
        });
        if (!selected || typeof selected !== "string") return;

        const content: string = await invoke("read_text_file", {
          path: selected,
        });
        await processImportData(content);
      } catch (error) {
        console.error("Import failed:", error);
        showToast(t("accounts.import_fail", { error: String(error) }), "error");
      }
    } else {
      // Web 模式: 触发隐藏的 file input
      fileInputRef.current?.click();
    }
  };

  const handleFileChange = async (
    event: React.ChangeEvent<HTMLInputElement>,
  ) => {
    const file = event.target.files?.[0];
    if (!file) return;

    try {
      const content = await file.text();
      await processImportData(content);
    } catch (error) {
      console.error("Import failed:", error);
      showToast(t("accounts.import_fail", { error: String(error) }), "error");
    } finally {
      // 重置 input,允许重复选择同一文件
      event.target.value = "";
    }
  };

  const handleViewDetails = (accountId: string) => {
    const account = accounts.find((a) => a.id === accountId);
    if (account) {
      setDetailsAccount(account);
    }
  };
  const handleViewDevice = (accountId: string) => {
    const account = accounts.find((a) => a.id === accountId);
    if (account) {
      setDeviceAccount(account);
    }
  };

  return (
    <div className="h-full flex flex-col p-5 gap-4 max-w-7xl mx-auto w-full">
      {/* 测试按钮 - 在最顶部 */}
      <input
        ref={fileInputRef}
        type="file"
        accept=".json,application/json"
        style={{ display: "none" }}
        onChange={handleFileChange}
      />

      {/* 顶部工具栏:搜索、过滤和操作按钮 */}
      <div className="flex-none flex items-center gap-2">
        {/* 搜索框 - 响应式:大屏显示输入框,小屏显示图标 */}
        <div className="hidden lg:block flex-none w-40 relative transition-all focus-within:w-48">
          <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-gray-400" />
          <input
            type="text"
            placeholder={t('accounts.search_placeholder')}
            className="w-full pl-9 pr-4 py-2 bg-white dark:bg-base-100 text-sm text-gray-900 dark:text-base-content border border-gray-200 dark:border-base-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent placeholder:text-gray-400 dark:placeholder:text-gray-500"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
        </div>

        {/* 搜索按钮 - 小屏显示 */}
        <div className="lg:hidden relative">
          {!isSearchExpanded ? (
            <button
              onClick={() => {
                setIsSearchExpanded(true);
                setTimeout(() => searchInputRef.current?.focus(), 100);
              }}
              className="p-2 bg-gray-100 dark:bg-base-200 hover:bg-gray-200 dark:hover:bg-base-100 rounded-lg transition-colors"
              title={t('accounts.search_placeholder')}
            >
              <Search className="w-4 h-4 text-gray-600 dark:text-gray-300" />
            </button>
          ) : (
            <div className="absolute left-0 top-0 z-10 w-64 flex items-center gap-1">
              <div className="flex-1 relative">
                <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-gray-400" />
                <input
                  ref={searchInputRef}
                  type="text"
                  placeholder={t('accounts.search_placeholder')}
                  className="w-full pl-9 pr-4 py-2 bg-white dark:bg-base-100 text-sm text-gray-900 dark:text-base-content border border-gray-200 dark:border-base-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent placeholder:text-gray-400 dark:placeholder:text-gray-500 shadow-lg"
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  onBlur={() => setIsSearchExpanded(false)}
                />
              </div>
            </div>
          )}
        </div>

        {/* 视图切换按钮组 */}
        <div className="flex gap-1 bg-gray-100 dark:bg-base-200 p-1 rounded-lg shrink-0">
          <button
            className={cn(
              "p-1.5 rounded-md transition-all",
              viewMode === "list"
                ? "bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-sm"
                : "text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content",
            )}
            onClick={() => setViewMode("list")}
            title={t("accounts.views.list")}
          >
            <List className="w-4 h-4" />
          </button>
          <button
            className={cn(
              "p-1.5 rounded-md transition-all",
              viewMode === "grid"
                ? "bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-sm"
                : "text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content",
            )}
            onClick={() => setViewMode("grid")}
            title={t("accounts.views.grid")}
          >
            <LayoutGrid className="w-4 h-4" />
          </button>
        </div>

        {/* 过滤按钮组 - 图标化响应式 */}
        <div className="flex gap-0.5 bg-gray-100/80 dark:bg-base-200 p-1 rounded-xl border border-gray-200/50 dark:border-white/5 shrink-0">
          {/* 全部 */}
          <button
            className={cn(
              "px-2 md:px-3 py-1.5 rounded-lg text-[11px] font-semibold transition-all flex items-center gap-1 md:gap-1.5 whitespace-nowrap shrink-0",
              filter === 'all'
                ? "bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-sm ring-1 ring-black/5"
                : "text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content hover:bg-white/40"
            )}
            onClick={() => setFilter('all')}
            title={`${t('accounts.all')} (${filterCounts.all})`}
          >
            <span className="hidden md:inline">{t('accounts.all')}</span>
            <span className={cn(
              "px-1.5 py-0.5 rounded-md text-[10px] font-bold transition-colors",
              filter === 'all'
                ? "bg-blue-100 dark:bg-blue-500/20 text-blue-600 dark:text-blue-400"
                : "bg-gray-200 dark:bg-gray-700 text-gray-500 dark:text-gray-400"
            )}>
              {filterCounts.all}
            </span>
          </button>

          {/* PRO */}
          <button
            className={cn(
              "px-2 md:px-3 py-1.5 rounded-lg text-[11px] font-semibold transition-all flex items-center gap-1 md:gap-1.5 whitespace-nowrap shrink-0",
              filter === 'pro'
                ? "bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-sm ring-1 ring-black/5"
                : "text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content hover:bg-white/40"
            )}
            onClick={() => setFilter('pro')}
            title={`${t('accounts.pro')} (${filterCounts.pro})`}
          >
            <span className="hidden md:inline">{t('accounts.pro')}</span>
            <span className={cn(
              "px-1.5 py-0.5 rounded-md text-[10px] font-bold transition-colors",
              filter === 'pro'
                ? "bg-blue-100 dark:bg-blue-500/20 text-blue-600 dark:text-blue-400"
                : "bg-gray-200 dark:bg-gray-700 text-gray-500 dark:text-gray-400"
            )}>
              {filterCounts.pro}
            </span>
          </button>

          {/* ULTRA */}
          <button
            className={cn(
              "flex px-2 lg:px-3 py-1.5 rounded-lg text-[11px] font-semibold transition-all items-center gap-1 lg:gap-1.5 whitespace-nowrap shrink-0",
              filter === 'ultra'
                ? "bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-sm ring-1 ring-black/5"
                : "text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content hover:bg-white/40"
            )}
            onClick={() => setFilter('ultra')}
            title={`${t('accounts.ultra')} (${filterCounts.ultra})`}
          >
            <span className="hidden md:inline">{t('accounts.ultra')}</span>
            <span className={cn(
              "px-1.5 py-0.5 rounded-md text-[10px] font-bold transition-colors",
              filter === 'ultra'
                ? "bg-blue-100 dark:bg-blue-500/20 text-blue-600 dark:text-blue-400"
                : "bg-gray-200 dark:bg-gray-700 text-gray-500 dark:text-gray-400"
            )}>
              {filterCounts.ultra}
            </span>
          </button>

          {/* FREE */}
          <button
            className={cn(
              "flex px-2 lg:px-3 py-1.5 rounded-lg text-[11px] font-semibold transition-all items-center gap-1 lg:gap-1.5 whitespace-nowrap shrink-0",
              filter === 'free'
                ? "bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-sm ring-1 ring-black/5"
                : "text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content hover:bg-white/40"
            )}
            onClick={() => setFilter('free')}
            title={`${t('accounts.free')} (${filterCounts.free})`}
          >
            <span className="hidden md:inline">{t('accounts.free')}</span>
            <span className={cn(
              "px-1.5 py-0.5 rounded-md text-[10px] font-bold transition-colors",
              filter === 'free'
                ? "bg-blue-100 dark:bg-blue-500/20 text-blue-600 dark:text-blue-400"
                : "bg-gray-200 dark:bg-gray-700 text-gray-500 dark:text-gray-400"
            )}>
              {filterCounts.free}
            </span>
          </button>

          {/* 动态标签筛选 */}
          {uniqueLabels.length > 0 && (
            <div className="w-px h-4 bg-gray-300 dark:bg-gray-600 mx-0.5 self-center" />
          )}
          {uniqueLabels.map((label) => (
            <button
              key={`label-${label}`}
              className={cn(
                "flex px-2 lg:px-3 py-1.5 rounded-lg text-[11px] font-semibold transition-all items-center gap-1 lg:gap-1.5 whitespace-nowrap shrink-0",
                filter === label
                  ? "bg-white dark:bg-base-100 text-purple-600 dark:text-purple-400 shadow-sm ring-1 ring-black/5"
                  : "text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content hover:bg-white/40"
              )}
              onClick={() => setFilter(label)}
              title={`${label} (${labelCounts[label] || 0})`}
            >
              <span className="hidden md:inline">{label}</span>
              <span className={cn(
                "px-1.5 py-0.5 rounded-md text-[10px] font-bold transition-colors",
                filter === label
                  ? "bg-purple-100 dark:bg-purple-500/20 text-purple-600 dark:text-purple-400"
                  : "bg-gray-200 dark:bg-gray-700 text-gray-500 dark:text-gray-400"
              )}>
                {labelCounts[label] || 0}
              </span>
            </button>
          ))}
        </div>

        <div className="flex-1 min-w-[8px]"></div>

        {/* 操作按钮组 */}
        <div className="flex items-center gap-1.5 shrink-0">
          <AddAccountDialog onAdd={handleAddAccount} showText={false} />

          {selectedIds.size > 0 && (
            <>
              <button
                className="px-2.5 py-2 bg-red-500 text-white text-xs font-medium rounded-lg hover:bg-red-600 transition-colors flex items-center gap-1.5 shadow-sm"
                onClick={handleBatchDelete}
                title={t("accounts.delete_selected", {
                  count: selectedIds.size,
                })}
              >
                <Trash2 className="w-3.5 h-3.5" />
                <span className="hidden xl:inline">
                  {t("accounts.delete_selected", { count: selectedIds.size })}
                </span>
              </button>
              <button
                className="px-2.5 py-2 bg-orange-500 text-white text-xs font-medium rounded-lg hover:bg-orange-600 transition-colors flex items-center gap-1.5 shadow-sm"
                onClick={() => handleBatchToggleProxy(false)}
                title={t("accounts.disable_proxy_selected", {
                  count: selectedIds.size,
                })}
              >
                <ToggleLeft className="w-3.5 h-3.5" />
                <span className="hidden xl:inline">
                  {t("accounts.disable_proxy_selected", {
                    count: selectedIds.size,
                  })}
                </span>
              </button>
              <button
                className="px-2.5 py-2 bg-green-500 text-white text-xs font-medium rounded-lg hover:bg-green-600 transition-colors flex items-center gap-1.5 shadow-sm"
                onClick={() => handleBatchToggleProxy(true)}
                title={t("accounts.enable_proxy_selected", {
                  count: selectedIds.size,
                })}
              >
                <ToggleRight className="w-3.5 h-3.5" />
                <span className="hidden xl:inline">
                  {t("accounts.enable_proxy_selected", {
                    count: selectedIds.size,
                  })}
                </span>
              </button>
            </>
          )}

          <button
            className={`px-2.5 py-2 bg-blue-500 text-white text-xs font-medium rounded-lg hover:bg-blue-600 transition-colors flex items-center gap-1.5 shadow-sm ${isRefreshing ? "opacity-70 cursor-not-allowed" : ""}`}
            onClick={handleRefreshClick}
            disabled={isRefreshing}
            title={
              selectedIds.size > 0
                ? t("accounts.refresh_selected", { count: selectedIds.size })
                : t("accounts.refresh_all")
            }
          >
            <RefreshCw
              className={`w-3.5 h-3.5 ${isRefreshing ? "animate-spin" : ""}`}
            />
            <span className="hidden xl:inline">
              {isRefreshing
                ? t("common.loading")
                : selectedIds.size > 0
                  ? t("accounts.refresh_selected", { count: selectedIds.size })
                  : t("accounts.refresh_all")}
            </span>
          </button>

          <button
            className={`px-2.5 py-2 bg-orange-500 text-white text-xs font-medium rounded-lg hover:bg-orange-600 transition-colors flex items-center gap-1.5 shadow-sm ${isWarmuping ? "opacity-70 cursor-not-allowed" : ""}`}
            onClick={() => setIsWarmupConfirmOpen(true)}
            disabled={isWarmuping}
            title={
              selectedIds.size > 0
                ? t("accounts.warmup_selected", { count: selectedIds.size })
                : t("accounts.warmup_all", "一键预热所有账号")
            }
          >
            <Sparkles
              className={`w-3.5 h-3.5 ${isWarmuping ? "animate-pulse" : ""}`}
            />
            <span className="hidden xl:inline">
              {isWarmuping
                ? t("common.loading")
                : selectedIds.size > 0
                  ? t("accounts.warmup_selected", { count: selectedIds.size })
                  : t("accounts.warmup_all", "一键预热")}
            </span>
          </button>

          <label className="flex items-center gap-2 cursor-pointer select-none px-2 py-2 border border-transparent hover:bg-gray-100 dark:hover:bg-base-200 rounded-lg transition-colors" title={t('accounts.show_all_quotas')}>
            <span className="text-xs font-medium text-gray-600 dark:text-gray-300 hidden xl:inline">
              {t('accounts.show_all_quotas')}
            </span>
            <input
              type="checkbox"
              className="toggle toggle-xs toggle-primary"
              checked={showAllQuotas}
              onChange={toggleShowAllQuotas}
            />
          </label>
          <div className="w-px h-4 bg-gray-200 dark:bg-gray-700 self-center mx-1 shrink-0"></div>

          <button
            className="px-2.5 py-2 border border-gray-200 dark:border-base-300 text-gray-700 dark:text-gray-300 text-xs font-medium rounded-lg hover:bg-gray-50 dark:hover:bg-base-200 transition-colors flex items-center gap-1.5"
            onClick={handleImportJson}
            title={t("accounts.import_json")}
          >
            <Upload className="w-3.5 h-3.5" />
            <span className="hidden lg:inline">
              {t("accounts.import_json")}
            </span>
          </button>

          <button
            className="px-2.5 py-2 border border-gray-200 dark:border-base-300 text-gray-700 dark:text-gray-300 text-xs font-medium rounded-lg hover:bg-gray-50 dark:hover:bg-base-200 transition-colors flex items-center gap-1.5"
            onClick={handleExport}
            title={
              selectedIds.size > 0
                ? t("accounts.export_selected", { count: selectedIds.size })
                : t("common.export")
            }
          >
            <Download className="w-3.5 h-3.5" />
            <span className="hidden lg:inline">
              {selectedIds.size > 0
                ? t("accounts.export_selected", { count: selectedIds.size })
                : t("common.export")}
            </span>
          </button>
        </div>
      </div>

      {/* 账号列表内容区域 */}
      <div className="flex-1 min-h-0 relative" ref={containerRef}>
        {viewMode === "list" ? (
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
                onViewDevice={handleViewDevice}
                onViewDetails={handleViewDetails}
                onCopy={handleCopyOne}
                onDelete={handleDelete}
                onToggleProxy={(id) =>
                  handleToggleProxy(
                    id,
                    !!accounts.find((a) => a.id === id)?.proxy_disabled,
                  )
                }
                onReorder={reorderAccounts}
                onWarmup={handleWarmup}
                onUpdateLabel={handleUpdateLabel}
                onViewError={(id: string) => setErrorAccountId(id)}
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
              onViewDevice={handleViewDevice}
              onViewDetails={handleViewDetails}
              onCopy={handleCopyOne}
              onDelete={handleDelete}
              onToggleProxy={(id) =>
                handleToggleProxy(
                  id,
                  !!accounts.find((a) => a.id === id)?.proxy_disabled,
                )
              }
              onWarmup={handleWarmup}
              onUpdateLabel={handleUpdateLabel}
              onViewError={(id: string) => setErrorAccountId(id)}
            />
          </div>
        )}
      </div>

      {/* 极简分页 - 无边框浮动样式 */}
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
              setCurrentPage(1); // 重置到第一页
            }}
            pageSizeOptions={[10, 20, 50, 100]}
          />
        </div>
      )}

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
        title={
          isBatchDelete
            ? t("accounts.dialog.batch_delete_title")
            : t("accounts.dialog.delete_title")
        }
        message={
          isBatchDelete
            ? t("accounts.dialog.batch_delete_msg", { count: selectedIds.size })
            : t("accounts.dialog.delete_msg")
        }
        type="confirm"
        confirmText={t("common.delete")}
        isDestructive={true}
        onConfirm={isBatchDelete ? executeBatchDelete : executeDelete}
        onCancel={() => {
          setDeleteConfirmId(null);
          setIsBatchDelete(false);
        }}
      />

      <ModalDialog
        isOpen={isRefreshConfirmOpen}
        title={
          selectedIds.size > 0
            ? t("accounts.dialog.batch_refresh_title")
            : t("accounts.dialog.refresh_title")
        }
        message={
          selectedIds.size > 0
            ? t("accounts.dialog.batch_refresh_msg", {
              count: selectedIds.size,
            })
            : t("accounts.dialog.refresh_msg")
        }
        type="confirm"
        confirmText={t("common.refresh")}
        isDestructive={false}
        onConfirm={executeRefresh}
        onCancel={() => setIsRefreshConfirmOpen(false)}
      />

      {toggleProxyConfirm && (
        <ModalDialog
          isOpen={!!toggleProxyConfirm}
          onCancel={() => setToggleProxyConfirm(null)}
          onConfirm={executeToggleProxy}
          title={
            toggleProxyConfirm.enable
              ? t("accounts.dialog.enable_proxy_title")
              : t("accounts.dialog.disable_proxy_title")
          }
          message={
            toggleProxyConfirm.enable
              ? t("accounts.dialog.enable_proxy_msg")
              : t("accounts.dialog.disable_proxy_msg")
          }
        />
      )}

      <ModalDialog
        isOpen={isWarmupConfirmOpen}
        title={
          selectedIds.size > 0
            ? t("accounts.dialog.batch_warmup_title", "批量手动预热")
            : t("accounts.dialog.warmup_all_title", "全量手动预热")
        }
        message={
          selectedIds.size > 0
            ? t(
              "accounts.dialog.batch_warmup_msg",
              "确定要为选中的 {{count}} 个账号立即触发预热吗？",
              { count: selectedIds.size },
            )
            : t(
              "accounts.dialog.warmup_all_msg",
              "确定要立即为所有符合条件的账号触发预热任务吗？这将向 Google 服务发送极小流量。",
            )
        }
        type="confirm"
        confirmText={t("accounts.warmup_now", "立即预热")}
        isDestructive={false}
        onConfirm={handleWarmupAll}
        onCancel={() => setIsWarmupConfirmOpen(false)}
      />

      {/* 账号详情弹窗 */}
      <AccountDetailsDialog
        account={detailsAccount}
        onClose={() => setDetailsAccount(null)}
      />

      {/* 账号错误详情弹窗 */}
      <AccountErrorDialog
        account={accounts.find(a => a.id === errorAccountId) || null}
        onClose={() => setErrorAccountId(null)}
      />
    </div>
  );
}

export default Accounts;
