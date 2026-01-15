import React, { useEffect, useState, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import ModalDialog from '../common/ModalDialog';
import { useTranslation } from 'react-i18next';
import { request as invoke } from '../../utils/request';
import { Trash2, Search, X } from 'lucide-react';
import { AppConfig } from '../../types/config';
import { formatCompactNumber } from '../../utils/format';
import { useVirtualizer } from '@tanstack/react-virtual';

interface ProxyRequestLog {
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
}

interface ProxyStats {
    total_requests: number;
    success_count: number;
    error_count: number;
}

interface ProxyMonitorProps {
    className?: string;
}

// Virtualized Log Table Component
interface VirtualizedLogTableProps {
    logs: ProxyRequestLog[];
    loading: boolean;
    hasMore: boolean;
    loadMore: () => void;
    onLogClick: (log: ProxyRequestLog) => void;
    t: any;
}

const VirtualizedLogTable: React.FC<VirtualizedLogTableProps> = ({
    logs,
    loading,
    hasMore,
    loadMore,
    onLogClick,
    t
}) => {
    const parentRef = useRef<HTMLDivElement>(null);

    const rowVirtualizer = useVirtualizer({
        count: logs.length,
        getScrollElement: () => parentRef.current,
        estimateSize: () => 36, // Row height
        overscan: 5,
    });

    const virtualItems = rowVirtualizer.getVirtualItems();

    return (
        <>
            <div ref={parentRef} className="flex-1 overflow-auto bg-white dark:bg-base-100">
                <table className="table table-xs w-full">
                    <thead className="bg-gray-50 dark:bg-base-200 text-gray-500 sticky top-0 z-10">
                        <tr>
                            <th style={{ width: '80px' }}>{t('monitor.table.status')}</th>
                            <th style={{ width: '80px' }}>{t('monitor.table.method')}</th>
                            <th style={{ width: '200px' }}>{t('monitor.table.model')}</th>
                            <th style={{ width: '150px' }}>{t('monitor.table.account')}</th>
                            <th>{t('monitor.table.path')}</th>
                            <th className="text-right" style={{ width: '100px' }}>{t('monitor.table.usage')}</th>
                            <th className="text-right" style={{ width: '100px' }}>{t('monitor.table.duration')}</th>
                            <th className="text-right" style={{ width: '100px' }}>{t('monitor.table.time')}</th>
                        </tr>
                    </thead>
                    <tbody className="font-mono text-gray-700 dark:text-gray-300" style={{ height: `${rowVirtualizer.getTotalSize()}px` }}>
                        {virtualItems.map((virtualRow) => {
                            const log = logs[virtualRow.index];
                            return (
                                <tr
                                    key={log.id}
                                    className="hover:bg-blue-50 dark:hover:bg-blue-900/20 cursor-pointer"
                                    style={{
                                        position: 'absolute',
                                        top: 0,
                                        left: 0,
                                        width: '100%',
                                        height: `${virtualRow.size}px`,
                                        transform: `translateY(${virtualRow.start}px)`,
                                    }}
                                    onClick={() => onLogClick(log)}
                                >
                                    <td style={{ width: '80px' }}>
                                        <span className={`badge badge-xs text-white border-none ${log.status >= 200 && log.status < 400 ? 'badge-success' : 'badge-error'}`}>
                                            {log.status}
                                        </span>
                                    </td>
                                    <td className="font-bold" style={{ width: '80px' }}>{log.method}</td>
                                    <td className="text-blue-600 truncate" style={{ width: '200px', maxWidth: '200px' }}>
                                        {log.mapped_model && log.model !== log.mapped_model
                                            ? `${log.model} => ${log.mapped_model}`
                                            : (log.model || '-')}
                                    </td>
                                    <td className="text-gray-600 dark:text-gray-400 truncate text-[10px]" style={{ width: '150px', maxWidth: '150px' }}>
                                        {log.account_email ? log.account_email.replace(/(.{3}).*(@.*)/, '$1***$2') : '-'}
                                    </td>
                                    <td className="truncate" style={{ maxWidth: '300px' }}>{log.url}</td>
                                    <td className="text-right text-[9px]" style={{ width: '100px' }}>
                                        {log.input_tokens != null && <div>I: {formatCompactNumber(log.input_tokens)}</div>}
                                        {log.output_tokens != null && <div>O: {formatCompactNumber(log.output_tokens)}</div>}
                                    </td>
                                    <td className="text-right" style={{ width: '100px' }}>{log.duration}ms</td>
                                    <td className="text-right text-[10px]" style={{ width: '100px' }}>
                                        {new Date(log.timestamp).toLocaleTimeString()}
                                    </td>
                                </tr>
                            );
                        })}
                    </tbody>
                </table>
            </div>

            {/* Loading indicator - outside scroll container */}
            {loading && (
                <div className="flex items-center justify-center p-4 bg-white dark:bg-base-100 border-t border-gray-200 dark:border-base-300">
                    <div className="loading loading-spinner loading-md"></div>
                    <span className="ml-3 text-sm text-gray-500">{t('common.loading') || 'Loading...'}</span>
                </div>
            )}

            {/* Load more button - outside scroll container */}
            {!loading && hasMore && logs.length > 0 && (
                <div className="flex justify-center p-4 bg-white dark:bg-base-100 border-t border-gray-200 dark:border-base-300">
                    <button onClick={loadMore} className="btn btn-sm btn-outline">
                        {t('common.load_more') || 'Load More'}
                    </button>
                </div>
            )}
        </>
    );
};


export const ProxyMonitor: React.FC<ProxyMonitorProps> = ({ className }) => {
    const { t } = useTranslation();
    const [logs, setLogs] = useState<ProxyRequestLog[]>([]);
    const [stats, setStats] = useState<ProxyStats>({ total_requests: 0, success_count: 0, error_count: 0 });
    const [filter, setFilter] = useState('');
    const [selectedLog, setSelectedLog] = useState<ProxyRequestLog | null>(null);
    const [isLoggingEnabled, setIsLoggingEnabled] = useState(false);
    const [isClearConfirmOpen, setIsClearConfirmOpen] = useState(false);

    // Pagination state
    const [pageSize] = useState(20);
    const [hasMore, setHasMore] = useState(true);
    const [loading, setLoading] = useState(false);
    const [loadingDetail, setLoadingDetail] = useState(false);

    const loadData = async (append = false) => {
        if (loading) return;
        setLoading(true);

        try {
            // Add timeout control (10 seconds)
            const timeoutPromise = new Promise((_, reject) =>
                setTimeout(() => reject(new Error('Request timeout')), 10000)
            );

            const config = await Promise.race([
                invoke<AppConfig>('load_config'),
                timeoutPromise
            ]) as AppConfig;

            if (config && config.proxy) {
                setIsLoggingEnabled(config.proxy.enable_logging);
                await invoke('set_proxy_monitor_enabled', { enabled: config.proxy.enable_logging });
            }

            // Use paginated query
            const offset = append ? logs.length : 0;
            const history = await Promise.race([
                invoke<ProxyRequestLog[]>('get_proxy_logs_paginated', {
                    limit: pageSize,
                    offset: offset
                }),
                timeoutPromise
            ]) as ProxyRequestLog[];

            if (Array.isArray(history)) {
                if (append) {
                    setLogs(prev => [...prev, ...history]);
                } else {
                    setLogs(history);
                }
                setHasMore(history.length === pageSize);
            }

            const currentStats = await Promise.race([
                invoke<ProxyStats>('get_proxy_stats'),
                timeoutPromise
            ]) as ProxyStats;

            if (currentStats) setStats(currentStats);
        } catch (e: any) {
            console.error("Failed to load proxy data", e);
            if (e.message === 'Request timeout') {
                // Show timeout error to user
                console.error('Loading monitor data timeout, please try again later');
            }
        } finally {
            setLoading(false);
        }
    };

    const loadMore = () => {
        loadData(true);
    };

    const toggleLogging = async () => {
        const newState = !isLoggingEnabled;
        try {
            const config = await invoke<AppConfig>('load_config');
            if (config && config.proxy) {
                config.proxy.enable_logging = newState;
                await invoke('save_config', { config });
                await invoke('set_proxy_monitor_enabled', { enabled: newState });
                setIsLoggingEnabled(newState);
            }
        } catch (e) {
            console.error("Failed to toggle logging", e);
        }
    };

    useEffect(() => {
        loadData();
        let unlistenFn: (() => void) | null = null;
        let updateTimeout: number | null = null;
        let pendingLogs: ProxyRequestLog[] = [];

        const setupListener = async () => {
            unlistenFn = await listen<ProxyRequestLog>('proxy://request', (event) => {
                const newLog = event.payload;

                // 移除 body 以减少内存占用
                const logSummary = {
                    ...newLog,
                    request_body: undefined,
                    response_body: undefined
                };

                // 添加到待处理队列
                pendingLogs.push(logSummary);

                // 防抖:每 500ms 批量更新一次
                if (updateTimeout) clearTimeout(updateTimeout);
                updateTimeout = setTimeout(() => {
                    if (pendingLogs.length > 0) {
                        setLogs(prev => [...pendingLogs, ...prev].slice(0, 100)); // 1000 → 100

                        // 批量更新统计
                        setStats((prev: ProxyStats) => {
                            const successCount = pendingLogs.filter(log => log.status >= 200 && log.status < 400).length;
                            return {
                                total_requests: prev.total_requests + pendingLogs.length,
                                success_count: prev.success_count + successCount,
                                error_count: prev.error_count + (pendingLogs.length - successCount),
                            };
                        });

                        pendingLogs = [];
                    }
                }, 500);
            });
        };
        setupListener();
        return () => {
            if (unlistenFn) unlistenFn();
            if (updateTimeout) clearTimeout(updateTimeout);
        };
    }, []);

    const filteredLogs = logs
        .filter(log =>
            log.url.toLowerCase().includes(filter.toLowerCase()) ||
            log.method.toLowerCase().includes(filter.toLowerCase()) ||
            (log.model && log.model.toLowerCase().includes(filter.toLowerCase())) ||
            log.status.toString().includes(filter)
        )
        .sort((a, b) => b.timestamp - a.timestamp);

    const quickFilters = [
        { label: t('monitor.filters.all'), value: '' },
        { label: t('monitor.filters.error'), value: '40' },
        { label: t('monitor.filters.chat'), value: 'completions' },
        { label: t('monitor.filters.gemini'), value: 'gemini' },
        { label: t('monitor.filters.claude'), value: 'claude' },
        { label: t('monitor.filters.images'), value: 'images' }
    ];

    const clearLogs = () => {
        setIsClearConfirmOpen(true);
    };

    const executeClearLogs = async () => {
        setIsClearConfirmOpen(false);
        try {
            await invoke('clear_proxy_logs');
            setLogs([]);
            setStats({ total_requests: 0, success_count: 0, error_count: 0 });
        } catch (e) {
            console.error("Failed to clear logs", e);
        }
    };

    const formatBody = (body?: string) => {
        if (!body) return <span className="text-gray-400 italic">{t('monitor.details.payload_empty')}</span>;
        try {
            const obj = JSON.parse(body);
            return <pre className="text-[10px] font-mono whitespace-pre-wrap text-gray-700 dark:text-gray-300">{JSON.stringify(obj, null, 2)}</pre>;
        } catch (e) {
            return <pre className="text-[10px] font-mono whitespace-pre-wrap text-gray-700 dark:text-gray-300">{body}</pre>;
        }
    };

    return (
        <div className={`flex flex-col bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200 overflow-hidden ${className || 'h-[400px]'}`}>
            <div className="p-3 border-b border-gray-100 dark:border-base-200 space-y-3 bg-gray-50/30 dark:bg-base-200/30">
                <div className="flex items-center gap-4">
                    <button
                        onClick={toggleLogging}
                        className={`btn btn-sm gap-2 px-4 border font-bold ${isLoggingEnabled
                            ? 'bg-red-500 border-red-600 text-white animate-pulse'
                            : 'bg-white dark:bg-base-200 border-gray-300 text-gray-600'
                            }`}
                    >
                        <div className={`w-2.5 h-2.5 rounded-full ${isLoggingEnabled ? 'bg-white' : 'bg-gray-400'}`} />
                        {isLoggingEnabled ? t('monitor.logging_status.active') : t('monitor.logging_status.paused')}
                    </button>

                    <div className="relative flex-1">
                        <Search className="absolute left-2.5 top-2 text-gray-400" size={14} />
                        <input
                            type="text"
                            placeholder={t('monitor.filters.placeholder')}
                            className="input input-sm input-bordered w-full pl-9 text-xs"
                            value={filter}
                            onChange={(e) => setFilter(e.target.value)}
                        />
                    </div>

                    <div className="hidden lg:flex gap-4 text-[10px] font-bold uppercase">
                        <span className="text-blue-500">{formatCompactNumber(stats.total_requests)} REQS</span>
                        <span className="text-green-500">{formatCompactNumber(stats.success_count)} OK</span>
                        <span className="text-red-500">{formatCompactNumber(stats.error_count)} ERR</span>
                    </div>

                    <button onClick={clearLogs} className="btn btn-sm btn-ghost text-gray-400">
                        <Trash2 size={16} />
                    </button>
                </div>

                <div className="flex flex-wrap items-center gap-2">
                    <span className="text-[10px] font-bold text-gray-400 uppercase">{t('monitor.filters.quick_filters')}</span>
                    {quickFilters.map(q => (
                        <button key={q.label} onClick={() => setFilter(q.value)} className={`px-2 py-0.5 rounded-full text-[10px] border ${filter === q.value ? 'bg-blue-500 text-white' : 'bg-white dark:bg-base-200 text-gray-500'}`}>
                            {q.label}
                        </button>
                    ))}
                    {filter && <button onClick={() => setFilter('')} className="text-[10px] text-blue-500"> {t('monitor.filters.reset')} </button>}
                </div>
            </div>

            <VirtualizedLogTable
                logs={filteredLogs}
                loading={loading}
                hasMore={hasMore}
                loadMore={loadMore}
                onLogClick={async (log) => {
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
                }}
                t={t}
            />

            {selectedLog && (
                <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4" onClick={() => setSelectedLog(null)}>
                    <div className="bg-white dark:bg-base-100 rounded-xl shadow-2xl w-full max-w-4xl max-h-[90vh] flex flex-col overflow-hidden border border-gray-200 dark:border-base-300" onClick={e => e.stopPropagation()}>
                        {/* Modal Header */}
                        <div className="px-4 py-3 border-b border-gray-100 dark:border-slate-700 flex items-center justify-between bg-gray-50 dark:bg-slate-900">
                            <div className="flex items-center gap-3">
                                {loadingDetail && <div className="loading loading-spinner loading-sm"></div>}
                                <span className={`badge badge-sm text-white border-none ${selectedLog.status >= 200 && selectedLog.status < 400 ? 'badge-success' : 'badge-error'}`}>{selectedLog.status}</span>
                                <span className="font-mono font-bold text-gray-900 dark:text-white text-sm">{selectedLog.method}</span>
                                <span className="text-xs text-gray-500 dark:text-slate-400 font-mono truncate max-w-md hidden sm:inline">{selectedLog.url}</span>
                            </div>
                            <button onClick={() => setSelectedLog(null)} className="btn btn-ghost btn-sm btn-circle text-gray-500 dark:text-slate-400 hover:dark:bg-slate-800"><X size={18} /></button>
                        </div>

                        {/* Modal Content */}
                        <div className="flex-1 overflow-y-auto p-4 space-y-6 bg-white dark:bg-slate-900">
                            {/* Metadata Section */}
                            <div className="bg-gray-50 dark:bg-slate-800 p-5 rounded-xl border border-gray-200 dark:border-slate-700 shadow-inner">
                                <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-y-5 gap-x-10">
                                    <div className="space-y-1.5">
                                        <span className="block text-gray-500 dark:text-slate-400 uppercase font-black text-[10px] tracking-widest">{t('monitor.details.time')}</span>
                                        <span className="font-mono font-semibold text-gray-900 dark:text-white text-xs">{new Date(selectedLog.timestamp).toLocaleString()}</span>
                                    </div>
                                    <div className="space-y-1.5">
                                        <span className="block text-gray-500 dark:text-slate-400 uppercase font-black text-[10px] tracking-widest">{t('monitor.details.duration')}</span>
                                        <span className="font-mono font-semibold text-gray-900 dark:text-white text-xs">{selectedLog.duration}ms</span>
                                    </div>
                                    <div className="space-y-1.5">
                                        <span className="block text-gray-500 dark:text-slate-400 uppercase font-black text-[10px] tracking-widest">{t('monitor.details.tokens')}</span>
                                        <div className="font-mono text-[11px] flex gap-2">
                                            <span className="text-blue-700 dark:text-blue-300 bg-blue-100 dark:bg-blue-900/40 px-2.5 py-1 rounded-md border border-blue-200 dark:border-blue-800/50 font-bold">In: {formatCompactNumber(selectedLog.input_tokens ?? 0)}</span>
                                            <span className="text-green-700 dark:text-green-300 bg-green-100 dark:bg-green-900/40 px-2.5 py-1 rounded-md border border-green-200 dark:border-green-800/50 font-bold">Out: {formatCompactNumber(selectedLog.output_tokens ?? 0)}</span>
                                        </div>
                                    </div>
                                </div>
                                <div className="mt-5 pt-5 border-t border-gray-200 dark:border-slate-700">
                                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-5">
                                        <div className="space-y-1.5">
                                            <span className="block text-gray-500 dark:text-slate-400 uppercase font-black text-[10px] tracking-widest">{t('monitor.details.model')}</span>
                                            <span className="font-mono font-black text-blue-600 dark:text-blue-400 break-all text-sm">{selectedLog.model || '-'}</span>
                                        </div>
                                        {selectedLog.mapped_model && selectedLog.model !== selectedLog.mapped_model && (
                                            <div className="space-y-1.5">
                                                <span className="block text-gray-500 dark:text-slate-400 uppercase font-black text-[10px] tracking-widest">{t('monitor.details.mapped_model')}</span>
                                                <span className="font-mono font-black text-green-600 dark:text-green-400 break-all text-sm">{selectedLog.mapped_model}</span>
                                            </div>
                                        )}
                                    </div>
                                </div>
                                {selectedLog.account_email && (
                                    <div className="mt-5 pt-5 border-t border-gray-200 dark:border-slate-700">
                                        <span className="block text-gray-500 dark:text-slate-400 uppercase font-black text-[10px] tracking-widest mb-2">{t('monitor.details.account_used')}</span>
                                        <span className="font-mono font-semibold text-gray-900 dark:text-white text-xs">{selectedLog.account_email}</span>
                                    </div>
                                )}
                            </div>

                            {/* Payloads */}
                            <div className="space-y-4">
                                <div>
                                    <h3 className="text-xs font-bold uppercase text-gray-400 mb-2 flex items-center gap-2">{t('monitor.details.request_payload')}</h3>
                                    <div className="bg-gray-50 dark:bg-base-300 rounded-lg p-3 border border-gray-100 dark:border-base-300 overflow-hidden">{formatBody(selectedLog.request_body)}</div>
                                </div>
                                <div>
                                    <h3 className="text-xs font-bold uppercase text-gray-400 mb-2 flex items-center gap-2">{t('monitor.details.response_payload')}</h3>
                                    <div className="bg-gray-50 dark:bg-base-300 rounded-lg p-3 border border-gray-100 dark:border-base-300 overflow-hidden">{formatBody(selectedLog.response_body)}</div>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            )}

            <ModalDialog
                isOpen={isClearConfirmOpen}
                title={t('monitor.dialog.clear_title')}
                message={t('monitor.dialog.clear_msg')}
                type="confirm"
                confirmText={t('common.delete')}
                isDestructive={true}
                onConfirm={executeClearLogs}
                onCancel={() => setIsClearConfirmOpen(false)}
            />
        </div>
    );
};
