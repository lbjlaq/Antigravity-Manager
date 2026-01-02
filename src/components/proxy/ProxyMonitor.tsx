import React, { useEffect, useState, useMemo } from 'react';
import { listen } from '@tauri-apps/api/event';
import ModalDialog from '../common/ModalDialog';
import { useTranslation } from 'react-i18next';
import { request as invoke } from '../../utils/request';
import { Trash2, Search, X } from 'lucide-react';
import { AppConfig } from '../../types/config';
import { formatCompactNumber } from '../../utils/format';

interface ProxyRequestLog {
    id: string;
    timestamp: number;
    method: string;
    url: string;
    status: number;
    duration: number;
    model?: string;           // 客户端请求的模型
    mapped_model?: string;    // 实际路由后使用的模型
    account_email?: string;
    error?: string;
    request_body?: string;
    response_body?: string;
    input_tokens?: number;
    output_tokens?: number;
}

interface ProxyStats {
    total_requests: number;
    success_count: number;
    error_count: number;
}

interface ProxyMonitorProps {
    className?: string;
}

export const ProxyMonitor: React.FC<ProxyMonitorProps> = ({ className }) => {
    const { t } = useTranslation();
    const [logs, setLogs] = useState<ProxyRequestLog[]>([]);
    const [stats, setStats] = useState<ProxyStats>({ total_requests: 0, success_count: 0, error_count: 0 });
    const [filter, setFilter] = useState('');
    const [selectedLog, setSelectedLog] = useState<ProxyRequestLog | null>(null);
    const [isLoggingEnabled, setIsLoggingEnabled] = useState(false);
    const [isClearConfirmOpen, setIsClearConfirmOpen] = useState(false);
    const [showAccountStats, setShowAccountStats] = useState(false);
    const [showAccountFilter, setShowAccountFilter] = useState(false);
    const [selectedAccounts, setSelectedAccounts] = useState<Set<string>>(new Set()); // Empty = all selected

    // Calculate per-account statistics from logs
    const accountStats = useMemo(() => {
        const statsMap: Record<string, { success: number; error: number; total: number }> = {};

        logs.forEach(log => {
            const email = log.account_email || t('monitor.unknown_account');
            if (!statsMap[email]) {
                statsMap[email] = { success: 0, error: 0, total: 0 };
            }
            statsMap[email].total++;
            if (log.status >= 200 && log.status < 400) {
                statsMap[email].success++;
            } else {
                statsMap[email].error++;
            }
        });

        return Object.entries(statsMap)
            .map(([email, data]) => ({
                email,
                ...data,
                successRate: data.total > 0 ? Math.round((data.success / data.total) * 100) : 0
            }))
            .sort((a, b) => b.total - a.total);
    }, [logs, t]);

    const loadData = async () => {
        try {
            const config = await invoke<AppConfig>('load_config');
            if (config && config.proxy) {
                setIsLoggingEnabled(config.proxy.enable_logging);
                await invoke('set_proxy_monitor_enabled', { enabled: config.proxy.enable_logging });
            }

            const history = await invoke<ProxyRequestLog[]>('get_proxy_logs', { limit: 100 });
            if (Array.isArray(history)) setLogs(history);

            const currentStats = await invoke<ProxyStats>('get_proxy_stats');
            if (currentStats) setStats(currentStats);
        } catch (e) {
            console.error("Failed to load proxy data", e);
        }
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
        const setupListener = async () => {
            unlistenFn = await listen<ProxyRequestLog>('proxy://request', (event) => {
                const newLog = event.payload;
                setLogs(prev => [newLog, ...prev].slice(0, 1000));
                setStats((prev: ProxyStats) => {
                    const isSuccess = newLog.status >= 200 && newLog.status < 400;
                    return {
                        total_requests: prev.total_requests + 1,
                        success_count: prev.success_count + (isSuccess ? 1 : 0),
                        error_count: prev.error_count + (isSuccess ? 0 : 1),
                    };
                });
            });
        };
        setupListener();
        return () => { if (unlistenFn) unlistenFn(); };
    }, []);

    // Extract unique accounts from logs for filter dropdown
    const uniqueAccounts = useMemo(() => {
        const accounts = new Set<string>();
        logs.forEach(log => {
            if (log.account_email) {
                accounts.add(log.account_email);
            }
        });
        return Array.from(accounts).sort();
    }, [logs]);

    // 筛选函数 - 不使用 useMemo，直接计算
    const getFilteredLogs = (): ProxyRequestLog[] => {
        let result = [...logs];

        // Apply account filter (if any accounts are selected)
        if (selectedAccounts.size > 0) {
            result = result.filter(log =>
                log.account_email && selectedAccounts.has(log.account_email)
            );
        }

        // Apply text filter
        const filterTrimmed = filter.trim();
        if (filterTrimmed) {
            const lowerFilter = filterTrimmed.toLowerCase();

            if (lowerFilter === '__chat__') {
                result = result.filter(log => {
                    const url = log.url.toLowerCase();
                    return url.includes('/chat') || url.includes('/messages') ||
                        url.includes('/responses') || url.includes('/completions');
                });
            } else if (lowerFilter === '__gemini__') {
                result = result.filter(log => {
                    const model = (log.model || '').toLowerCase();
                    return (model.startsWith('gpt') || model.startsWith('gemini')) && !model.startsWith('claude');
                });
            } else if (lowerFilter === '__claude__') {
                result = result.filter(log => {
                    const model = (log.model || '').toLowerCase();
                    return model.startsWith('claude');
                });
            } else if (lowerFilter === '__images__') {
                result = result.filter(log => {
                    const url = log.url.toLowerCase();
                    return url.includes('/images');
                });
            } else if (lowerFilter === '__error__') {
                // 仅错误 - 匹配 4xx 状态码
                result = result.filter(log => log.status >= 400 && log.status < 500);
            } else {
                // Regular text search
                result = result.filter(log => {
                    const urlMatch = log.url.toLowerCase().includes(lowerFilter);
                    const methodMatch = log.method.toLowerCase().includes(lowerFilter);
                    const modelMatch = log.model?.toLowerCase().includes(lowerFilter) ?? false;
                    const accountMatch = log.account_email?.toLowerCase().includes(lowerFilter) ?? false;
                    const statusMatch = log.status.toString().includes(lowerFilter);
                    return urlMatch || methodMatch || modelMatch || accountMatch || statusMatch;
                });
            }
        }

        return result.sort((a, b) => b.timestamp - a.timestamp);
    };

    // 直接调用函数获取筛选结果
    const filteredLogs = getFilteredLogs();

    // Quick filter definitions with special matching logic
    // 'chat' matches any chat-related endpoints
    // 'gemini'/'claude' match model names (gpt-* routes through gemini)
    // 'images' matches image generation endpoints
    const quickFilters = [
        { label: t('monitor.filters.all'), value: '' },
        { label: t('monitor.filters.error'), value: '__error__' },
        { label: t('monitor.filters.chat'), value: '__chat__' }, // Special marker for chat endpoints
        { label: t('monitor.filters.gemini'), value: '__gemini__' }, // Match gpt-* models (routes through Gemini)
        { label: t('monitor.filters.claude'), value: '__claude__' }, // Match claude-* models
        { label: t('monitor.filters.images'), value: '__images__' }
    ];

    const clearLogs = () => {
        setIsClearConfirmOpen(true);
    };

    const executeClearLogs = async () => {
        setIsClearConfirmOpen(false);
        try {
            await invoke('clear_proxy_logs');
            // Immediately clear local state
            setLogs([]);
            setStats({ total_requests: 0, success_count: 0, error_count: 0 });
            // Also reload from backend to ensure sync
            await loadData();
        } catch (e) {
            console.error("Failed to clear logs", e);
        }
    };

    const formatBody = (body?: string) => {
        if (!body) return <span className="text-gray-400 italic">Empty</span>;
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

                    <div className="relative hidden lg:block">
                        <button
                            onClick={() => setShowAccountStats(!showAccountStats)}
                            className="flex gap-4 text-[10px] font-bold uppercase cursor-pointer hover:opacity-80 transition-opacity"
                        >
                            <span className="text-blue-500">{formatCompactNumber(stats.total_requests)} REQS</span>
                            <span className="text-green-500">{formatCompactNumber(stats.success_count)} OK</span>
                            <span className="text-red-500">{formatCompactNumber(stats.error_count)} ERR</span>
                        </button>

                        {/* Account Stats Dropdown */}
                        {showAccountStats && accountStats.length > 0 && (
                            <div className="absolute right-0 top-6 z-50 bg-white dark:bg-base-200 border border-gray-200 dark:border-base-300 rounded-lg shadow-lg p-3 min-w-[280px]">
                                <div className="text-xs font-bold text-gray-600 dark:text-gray-300 mb-2 border-b pb-2">
                                    {t('monitor.account_stats.title')}
                                </div>
                                <div className="space-y-2 max-h-[200px] overflow-y-auto">
                                    {accountStats.map(stat => (
                                        <div key={stat.email} className="flex items-center justify-between text-xs">
                                            <span className="truncate max-w-[120px] text-gray-700 dark:text-gray-300" title={stat.email}>
                                                {stat.email.split('@')[0]}
                                            </span>
                                            <div className="flex gap-2 text-[10px]">
                                                <span className="text-green-600">{stat.success}✓</span>
                                                <span className="text-red-500">{stat.error}✗</span>
                                                <span className={`font-bold ${stat.successRate >= 90 ? 'text-green-600' : stat.successRate >= 70 ? 'text-yellow-600' : 'text-red-500'}`}>
                                                    {stat.successRate}%
                                                </span>
                                            </div>
                                        </div>
                                    ))}
                                </div>
                            </div>
                        )}
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

                    {/* Account Filter Dropdown */}
                    {uniqueAccounts.length > 0 && (
                        <div className="relative">
                            <button
                                onClick={() => setShowAccountFilter(!showAccountFilter)}
                                className={`px-2 py-0.5 rounded-full text-[10px] border flex items-center gap-1 ${selectedAccounts.size > 0 ? 'bg-purple-500 text-white' : 'bg-white dark:bg-base-200 text-gray-500'}`}
                            >
                                {t('monitor.filters.account')} {selectedAccounts.size > 0 && `(${selectedAccounts.size})`}
                            </button>

                            {showAccountFilter && (
                                <div className="absolute left-0 top-6 z-50 bg-white dark:bg-base-200 border border-gray-200 dark:border-base-300 rounded-lg shadow-lg p-2 min-w-[200px]">
                                    <div className="text-xs font-bold text-gray-600 dark:text-gray-300 mb-2 pb-1 border-b flex justify-between">
                                        <span>{t('monitor.filters.select_accounts')}</span>
                                        <button
                                            onClick={() => setSelectedAccounts(new Set())}
                                            className="text-blue-500 text-[10px]"
                                        >
                                            {t('monitor.filters.select_all')}
                                        </button>
                                    </div>
                                    <div className="space-y-1 max-h-[150px] overflow-y-auto">
                                        {uniqueAccounts.map(account => (
                                            <label key={account} className="flex items-center gap-2 text-xs cursor-pointer hover:bg-gray-50 dark:hover:bg-base-300 p-1 rounded">
                                                <input
                                                    type="checkbox"
                                                    className="checkbox checkbox-xs checkbox-primary"
                                                    checked={selectedAccounts.size === 0 || selectedAccounts.has(account)}
                                                    onChange={(e) => {
                                                        const newSet = new Set(selectedAccounts);
                                                        if (e.target.checked) {
                                                            if (selectedAccounts.size === 0) {
                                                                // First selection from "all" state
                                                                newSet.add(account);
                                                            } else {
                                                                newSet.add(account);
                                                            }
                                                        } else {
                                                            if (selectedAccounts.size === 0) {
                                                                // Deselecting from "all" - select all except this one
                                                                uniqueAccounts.forEach(a => { if (a !== account) newSet.add(a); });
                                                            } else {
                                                                newSet.delete(account);
                                                            }
                                                        }
                                                        setSelectedAccounts(newSet);
                                                    }}
                                                />
                                                <span className="truncate">{account.split('@')[0]}</span>
                                            </label>
                                        ))}
                                    </div>
                                </div>
                            )}
                        </div>
                    )}

                    {(filter || selectedAccounts.size > 0) && <button onClick={() => { setFilter(''); setSelectedAccounts(new Set()); }} className="text-[10px] text-blue-500"> {t('monitor.filters.reset')} </button>}
                </div>
            </div>

            <div className="flex-1 overflow-auto bg-white dark:bg-base-100">
                <table className="table table-xs w-full">
                    <thead className="bg-gray-50 dark:bg-base-200 text-gray-500 sticky top-0">
                        <tr>
                            <th>{t('monitor.table.status')}</th>
                            <th>{t('monitor.table.method')}</th>
                            <th>{t('monitor.table.model')}</th>
                            <th>{t('monitor.table.account')}</th>
                            <th>{t('monitor.table.path')}</th>
                            <th className="text-right">{t('monitor.table.usage')}</th>
                            <th className="text-right">{t('monitor.table.duration')}</th>
                            <th className="text-right">{t('monitor.table.time')}</th>
                        </tr>
                    </thead>
                    <tbody className="font-mono text-gray-700 dark:text-gray-300">
                        {filteredLogs.map(log => (
                            <tr key={log.id} className="hover:bg-blue-50 dark:hover:bg-blue-900/20 cursor-pointer" onClick={() => setSelectedLog(log)}>
                                <td><span className={`badge badge-xs text-white border-none ${log.status >= 200 && log.status < 400 ? 'badge-success' : 'badge-error'}`}>{log.status}</span></td>
                                <td className="font-bold">{log.method}</td>
                                <td className="text-blue-600 truncate max-w-[150px]" title={log.mapped_model ? `实际: ${log.mapped_model}` : undefined}>
                                    {log.model || '-'}
                                    {log.mapped_model && log.mapped_model !== log.model && (
                                        <span className="text-[9px] text-gray-400 ml-1">→{log.mapped_model.replace('gemini-', 'g-').replace('-latest', '')}</span>
                                    )}
                                </td>
                                <td className="text-gray-500 truncate max-w-[160px] text-[10px]" title={log.account_email}>{log.account_email ? log.account_email.split('@')[0] : '-'}</td>
                                <td className="truncate max-w-[200px]">{log.url}</td>
                                <td className="text-right text-[9px]">
                                    {log.input_tokens != null && <div>I: {formatCompactNumber(log.input_tokens)}</div>}
                                    {log.output_tokens != null && <div>O: {formatCompactNumber(log.output_tokens)}</div>}
                                </td>
                                <td className="text-right">{log.duration}ms</td>
                                <td className="text-right text-[10px]">{new Date(log.timestamp).toLocaleTimeString()}</td>
                            </tr>
                        ))}
                    </tbody>
                </table>
            </div>

            {selectedLog && (
                <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4" onClick={() => setSelectedLog(null)}>
                    <div className="bg-white dark:bg-base-100 rounded-xl shadow-2xl w-full max-w-4xl max-h-[90vh] flex flex-col overflow-hidden border border-gray-200 dark:border-base-300" onClick={e => e.stopPropagation()}>
                        {/* Modal Header */}
                        <div className="px-4 py-3 border-b border-gray-100 dark:border-slate-700 flex items-center justify-between bg-gray-50 dark:bg-slate-900">
                            <div className="flex items-center gap-3">
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
                                    <span className="block text-gray-500 dark:text-slate-400 uppercase font-black text-[10px] tracking-widest mb-2">{t('monitor.details.model')}</span>
                                    <span className="font-mono font-black text-blue-600 dark:text-blue-400 break-all text-sm">{selectedLog.model || '-'}</span>
                                </div>
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
