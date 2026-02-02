// File: src/pages/logs/model/useLogsPage.ts
// Traffic logs page business logic hook

import { useState, useEffect, useMemo, useRef, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';

import { invoke } from '@/shared/api';
import { isTauri } from '@/shared/lib';
import { showToast } from '@/shared/ui';
import { useAccounts } from '@/features/accounts';
import type { AppConfig } from '@/entities/config';

export interface ProxyRequestLog {
  id: string;
  timestamp: number;
  method: string;
  url: string;
  status: number;
  duration: number;
  model?: string;
  mapped_model?: string;
  error?: string;
  request_body?: string;
  response_body?: string;
  input_tokens?: number;
  output_tokens?: number;
  account_email?: string;
  protocol?: string;
}

export interface ProxyStats {
  total_requests: number;
  success_count: number;
  error_count: number;
}

export type QuickFilterType = '' | '__ERROR__' | 'completions' | 'gemini' | 'claude' | 'images';

// Auto-refresh interval in ms
const AUTO_REFRESH_INTERVAL = 3000;

export function useLogsPage() {
  const { t } = useTranslation();

  // Data state
  const [logs, setLogs] = useState<ProxyRequestLog[]>([]);
  const [stats, setStats] = useState<ProxyStats>({ total_requests: 0, success_count: 0, error_count: 0 });
  const [totalCount, setTotalCount] = useState(0);

  // Filter state
  const [searchQuery, setSearchQuery] = useState('');
  const [quickFilter, setQuickFilter] = useState<QuickFilterType>('');
  const [accountFilter, setAccountFilter] = useState('');

  // UI state
  const [isLoggingEnabled, setIsLoggingEnabled] = useState(false);
  const [isAutoRefresh, setIsAutoRefresh] = useState(true);
  const [loading, setLoading] = useState(false);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [selectedLog, setSelectedLog] = useState<ProxyRequestLog | null>(null);
  const [isClearConfirmOpen, setIsClearConfirmOpen] = useState(false);

  // Pagination
  const PAGE_SIZE_OPTIONS = [50, 100, 200, 500];
  const [pageSize, setPageSize] = useState(100);
  const [currentPage, setCurrentPage] = useState(1);

  // Accounts for filter
  const { data: accounts = [] } = useAccounts();

  // Refs
  const pendingLogsRef = useRef<ProxyRequestLog[]>([]);
  const listenerSetupRef = useRef(false);
  const isMountedRef = useRef(true);
  const loadingRef = useRef(false);
  const lastLoadTimeRef = useRef(0);

  // Unique accounts from logs + loaded accounts
  const uniqueAccounts = useMemo(() => {
    const emailSet = new Set<string>();
    logs.forEach(log => {
      if (log.account_email) {
        emailSet.add(log.account_email);
      }
    });
    accounts.forEach(acc => {
      emailSet.add(acc.email);
    });
    return Array.from(emailSet).sort();
  }, [logs, accounts]);

  // Load data function (non-blocking for auto-refresh)
  const loadData = useCallback(async (
    page = currentPage,
    filter = searchQuery,
    account = accountFilter,
    isAutoRefreshCall = false
  ) => {
    // Prevent concurrent loads
    if (loadingRef.current) return;
    
    // Throttle auto-refresh calls
    if (isAutoRefreshCall) {
      const now = Date.now();
      if (now - lastLoadTimeRef.current < 2000) return;
      lastLoadTimeRef.current = now;
    }

    loadingRef.current = true;
    if (!isAutoRefreshCall) setLoading(true);

    try {
      const timeoutPromise = new Promise((_, reject) =>
        setTimeout(() => reject(new Error('Request timeout')), 10000)
      );

      // Load config only on manual refresh
      if (!isAutoRefreshCall) {
        const config = await Promise.race([
          invoke<AppConfig>('load_config'),
          timeoutPromise
        ]) as AppConfig;

        if (config?.proxy) {
          setIsLoggingEnabled(config.proxy.enable_logging);
          await invoke('set_proxy_monitor_enabled', { enabled: config.proxy.enable_logging });
        }
      }

      const errorsOnly = filter === '__ERROR__';
      const baseFilter = errorsOnly ? '' : filter;
      const actualFilter = account
        ? (baseFilter ? `${baseFilter} ${account}` : account)
        : baseFilter;

      // Parallel fetch: count + logs + stats
      const offset = (page - 1) * pageSize;
      const [count, history, currentStats] = await Promise.all([
        invoke<number>('get_proxy_logs_count_filtered', {
          filter: actualFilter,
          errorsOnly
        }),
        invoke<ProxyRequestLog[]>('get_proxy_logs_filtered', {
          filter: actualFilter,
          errorsOnly,
          limit: pageSize,
          offset
        }),
        invoke<ProxyStats>('get_proxy_stats'),
      ]);

      if (isMountedRef.current) {
        setTotalCount(count);
        if (Array.isArray(history)) {
          setLogs(history);
          pendingLogsRef.current = [];
        }
        if (currentStats) setStats(currentStats);
      }
    } catch (e: unknown) {
      console.error('Failed to load proxy data', e);
      if (!isAutoRefreshCall && e instanceof Error && e.message === 'Request timeout') {
        showToast(t('monitor.error.timeout', 'Loading timeout'), 'error');
      }
    } finally {
      loadingRef.current = false;
      if (!isAutoRefreshCall) setLoading(false);
    }
  }, [currentPage, searchQuery, accountFilter, pageSize, t]);

  // Toggle logging
  const toggleLogging = useCallback(async () => {
    const newState = !isLoggingEnabled;
    try {
      const config = await invoke<AppConfig>('load_config');
      if (config?.proxy) {
        config.proxy.enable_logging = newState;
        await invoke('save_config', { config });
        await invoke('set_proxy_monitor_enabled', { enabled: newState });
        setIsLoggingEnabled(newState);
        showToast(
          newState ? t('monitor.logging_enabled', 'Logging enabled') : t('monitor.logging_disabled', 'Logging paused'),
          'success'
        );
      }
    } catch (e) {
      console.error('Failed to toggle logging', e);
      showToast(t('common.error'), 'error');
    }
  }, [isLoggingEnabled, t]);

  // Toggle auto-refresh
  const toggleAutoRefresh = useCallback(() => {
    setIsAutoRefresh(prev => !prev);
  }, []);

  // Clear logs
  const clearLogs = useCallback(async () => {
    setIsClearConfirmOpen(false);
    try {
      await invoke('clear_proxy_logs');
      setLogs([]);
      setStats({ total_requests: 0, success_count: 0, error_count: 0 });
      setTotalCount(0);
      showToast(t('monitor.logs_cleared', 'Logs cleared'), 'success');
    } catch (e) {
      console.error('Failed to clear logs', e);
      showToast(t('common.error'), 'error');
    }
  }, [t]);

  // Load log detail
  const loadLogDetail = useCallback(async (log: ProxyRequestLog) => {
    setLoadingDetail(true);
    try {
      const detail = await invoke<ProxyRequestLog>('get_proxy_log_detail', { logId: log.id });
      setSelectedLog(detail);
    } catch (e) {
      console.error('Failed to load log detail', e);
      setSelectedLog(log);
    } finally {
      setLoadingDetail(false);
    }
  }, []);

  // Pagination
  const totalPages = Math.ceil(totalCount / pageSize);

  const goToPage = useCallback((page: number) => {
    if (page >= 1 && page <= totalPages && page !== currentPage) {
      setCurrentPage(page);
      loadData(page, searchQuery, accountFilter);
    }
  }, [totalPages, currentPage, loadData, searchQuery, accountFilter]);

  // Filter logs by account on frontend
  const filteredLogs = useMemo(() => {
    if (!accountFilter) return logs;
    return logs.filter(log => log.account_email === accountFilter);
  }, [logs, accountFilter]);

  // Quick filters
  const quickFilters = useMemo(() => [
    { label: t('monitor.filters.all', 'All'), value: '' as QuickFilterType },
    { label: t('monitor.filters.error', 'Errors'), value: '__ERROR__' as QuickFilterType },
    { label: t('monitor.filters.chat', 'Chat'), value: 'completions' as QuickFilterType },
    { label: t('monitor.filters.gemini', 'Gemini'), value: 'gemini' as QuickFilterType },
    { label: t('monitor.filters.claude', 'Claude'), value: 'claude' as QuickFilterType },
    { label: t('monitor.filters.images', 'Images'), value: 'images' as QuickFilterType },
  ], [t]);

  // Initial load + real-time event listener
  useEffect(() => {
    isMountedRef.current = true;
    loadData(1, searchQuery, accountFilter, false);

    let unlistenFn: (() => void) | null = null;
    let updateTimeout: ReturnType<typeof setTimeout> | null = null;

    const setupListener = async () => {
      if (!isTauri()) return;
      if (listenerSetupRef.current) return;
      listenerSetupRef.current = true;

      unlistenFn = await listen<ProxyRequestLog>('proxy://request', (event) => {
        if (!isMountedRef.current) return;

        const newLog = {
          ...event.payload,
          request_body: undefined,
          response_body: undefined
        };

        const alreadyExists = pendingLogsRef.current.some(log => log.id === newLog.id);
        if (alreadyExists) return;

        pendingLogsRef.current.push(newLog);

        // Debounce: batch updates every 300ms
        if (updateTimeout) clearTimeout(updateTimeout);
        updateTimeout = setTimeout(async () => {
          if (!isMountedRef.current) return;

          const currentPending = pendingLogsRef.current;
          if (currentPending.length > 0) {
            setLogs(prev => {
              const existingIds = new Set(prev.map(log => log.id));
              const uniqueNewLogs = currentPending.filter(log => !existingIds.has(log.id));
              const merged = [...uniqueNewLogs, ...prev];
              merged.sort((a, b) => b.timestamp - a.timestamp);
              return merged.slice(0, pageSize);
            });

            // Update stats in background
            try {
              const [currentStats, count] = await Promise.all([
                invoke<ProxyStats>('get_proxy_stats'),
                invoke<number>('get_proxy_logs_count_filtered', { filter: '', errorsOnly: false })
              ]);
              if (isMountedRef.current) {
                if (currentStats) setStats(currentStats);
                setTotalCount(count);
              }
            } catch (e) {
              console.error('Failed to fetch stats:', e);
            }

            pendingLogsRef.current = [];
          }
        }, 300);
      });
    };

    setupListener();

    return () => {
      isMountedRef.current = false;
      listenerSetupRef.current = false;
      if (unlistenFn) unlistenFn();
      if (updateTimeout) clearTimeout(updateTimeout);
    };
  }, []);

  // Auto-refresh polling (works for both Tauri and Web)
  useEffect(() => {
    if (!isAutoRefresh) return;

    const pollInterval = setInterval(() => {
      if (isMountedRef.current && !loadingRef.current) {
        loadData(currentPage, searchQuery, accountFilter, true);
      }
    }, AUTO_REFRESH_INTERVAL);

    return () => clearInterval(pollInterval);
  }, [isAutoRefresh, currentPage, searchQuery, accountFilter, loadData]);

  // Reload on filter change
  useEffect(() => {
    setCurrentPage(1);
    loadData(1, searchQuery, accountFilter, false);
  }, [searchQuery, accountFilter, pageSize]);

  // Handle quick filter change
  const handleQuickFilterChange = useCallback((value: QuickFilterType) => {
    setQuickFilter(value);
    setSearchQuery(value);
  }, []);

  // Reset all filters
  const resetFilters = useCallback(() => {
    setSearchQuery('');
    setQuickFilter('');
    setAccountFilter('');
  }, []);

  return {
    // Data
    logs: filteredLogs,
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
    loadData: () => loadData(currentPage, searchQuery, accountFilter, false),
    toggleLogging,
    toggleAutoRefresh,
    clearLogs,
    loadLogDetail,
  };
}
