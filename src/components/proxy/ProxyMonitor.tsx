import React, { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { ask } from '@tauri-apps/plugin-dialog';
import { useTranslation } from 'react-i18next';
import { request as invoke } from '../../utils/request';
import { Trash2, Search, X, ArrowUp, ArrowDown } from 'lucide-react';
import { AppConfig } from '../../types/config';

interface ProxyRequestLog {
    id: string;
    timestamp: number;
    method: string;
    url: string;
    status: number;
    duration: number;
    model?: string;
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

    const loadData = async () => {
        try {
            const config = await invoke<AppConfig>('load_config');
            if (config && config.proxy) {
                setIsLoggingEnabled(config.proxy.enable_logging);
                // 确保后端状态同步
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
            // 1. 获取并更新配置
            const config = await invoke<AppConfig>('load_config');
            config.proxy.enable_logging = newState;
            await invoke('save_config', { config });
            
            // 2. 同步到后端
            await invoke('set_proxy_monitor_enabled', { enabled: newState });
            
            setIsLoggingEnabled(newState);
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
                console.log("[Monitor] Received new log via event:", newLog);
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

        return () => {
            if (unlistenFn) unlistenFn();
        };
    }, []);

    const filteredLogs = logs.filter(log => 
        log.url.toLowerCase().includes(filter.toLowerCase()) || 
        log.method.toLowerCase().includes(filter.toLowerCase()) ||
        (log.model && log.model.toLowerCase().includes(filter.toLowerCase())) ||
        log.status.toString().includes(filter)
    );

    const quickFilters = [
        { label: t('monitor.filters.all'), value: '' },
        { label: t('monitor.filters.error'), value: '40', color: 'text-error' },
        { label: t('monitor.filters.chat'), value: 'completions' },
        { label: t('monitor.filters.gemini'), value: 'gemini' },
        { label: t('monitor.filters.claude'), value: 'claude' },
        { label: t('monitor.filters.images'), value: 'images' },
    ];

    const clearLogs = async () => {
        const confirmed = await ask(t('monitor.dialog.clear_msg'), {
            title: t('monitor.dialog.clear_title'),
            kind: 'warning',
        });

        if (confirmed) {
            try {
                await invoke('clear_proxy_logs');
                setLogs([]);
                setStats({ total_requests: 0, success_count: 0, error_count: 0 });
            } catch (e) {
                console.error("Failed to clear logs", e);
            }
        }
    };

    // Helper to pretty print JSON
    const formatBody = (body?: string) => {
        if (!body) return <span className="text-gray-400 italic">Empty</span>;
        try {
            const obj = JSON.parse(body);
            return <pre className="text-[10px] font-mono whitespace-pre-wrap overflow-x-auto text-gray-700 dark:text-gray-300">{JSON.stringify(obj, null, 2)}</pre>;
        } catch (e) {
            return <pre className="text-[10px] font-mono whitespace-pre-wrap overflow-x-auto text-gray-700 dark:text-gray-300">{body}</pre>;
        }
    };

    return (
        <>
            <div className={`bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200 overflow-hidden flex flex-col ${className || 'h-[400px]'}`}>
                {/* Toolbar & Stats */}
                <div className="p-3 border-b border-gray-100 dark:border-base-200 space-y-3 bg-gray-50/30 dark:bg-base-200/30">
                    <div className="flex items-center gap-4">
                        {/* Recording Toggle */}
                        <button 
                            onClick={toggleLogging}
                            className={`btn btn-sm gap-2 px-4 border shadow-sm transition-all font-bold ${
                                isLoggingEnabled 
                                ? 'bg-red-500 hover:bg-red-600 border-red-600 text-white animate-pulse' 
                                : 'bg-white dark:bg-base-200 hover:bg-gray-100 dark:hover:bg-base-300 border-gray-300 dark:border-base-300 text-gray-600'
                            }`}
                            title={isLoggingEnabled ? "Stop Recording" : "Start Recording"}
                        >
                            <div className={`w-2.5 h-2.5 rounded-full ${isLoggingEnabled ? 'bg-white shadow-[0_0_8px_rgba(255,255,255,0.8)]' : 'bg-gray-400'}`} />
                            {isLoggingEnabled ? t('monitor.logging_status.active') : t('monitor.logging_status.paused')}
                        </button>

                        <div className="relative flex-1">
                            <Search className="absolute left-2.5 top-2 text-gray-400" size={14} />
                            <input 
                                type="text" 
                                placeholder={t('monitor.filters.placeholder')}
                                className="input input-sm input-bordered w-full pl-9 bg-white dark:bg-base-100 text-xs text-gray-900 dark:text-base-content focus:border-blue-500"
                                value={filter}
                                onChange={(e) => setFilter(e.target.value)}
                            />
                        </div>

                        {/* Compact Stats */}
                        <div className="hidden lg:flex gap-4 text-[10px] font-bold uppercase tracking-widest">
                            <div className="flex items-center gap-1.5">
                                <span className="w-2 h-2 rounded-full bg-blue-500"></span>
                                <span className="text-gray-500 dark:text-gray-400">{stats.total_requests} {t('monitor.stats.total')}</span>
                            </div>
                            <div className="flex items-center gap-1.5">
                                <span className="w-2 h-2 rounded-full bg-green-500"></span>
                                <span className="text-gray-500 dark:text-gray-400">{stats.success_count} {t('monitor.stats.ok')}</span>
                            </div>
                            <div className="flex items-center gap-1.5">
                                <span className="w-2 h-2 rounded-full bg-red-500"></span>
                                <span className="text-gray-500 dark:text-gray-400">{stats.error_count} {t('monitor.stats.err')}</span>
                            </div>
                        </div>

                        <div className="flex items-center gap-1 border-l border-gray-200 dark:border-base-300 pl-2">
                            <button onClick={clearLogs} className="btn btn-sm btn-ghost text-gray-400 hover:text-error transition-colors" title={t('common.clear')}>
                                <Trash2 size={16} />
                            </button>
                        </div>
                    </div>

                    {/* Quick Filters */}
                    <div className="flex flex-wrap items-center gap-2">
                        <span className="text-[10px] font-bold text-gray-400 uppercase mr-1">{t('monitor.filters.quick_filters')}</span>
                        {quickFilters.map(q => (
                            <button
                                key={q.label}
                                onClick={() => setFilter(q.value)}
                                className={`px-2 py-0.5 rounded-full text-[10px] font-medium border transition-all ${
                                    filter === q.value 
                                    ? 'bg-blue-500 text-white border-blue-500 shadow-sm' 
                                    : `bg-white dark:bg-base-200 text-gray-500 border-gray-200 dark:border-base-300 hover:border-blue-300 ${q.color || ''}`
                                }`}
                            >
                                {q.label}
                            </button>
                        ))}
                        {filter && (
                            <button 
                                onClick={() => setFilter('')}
                                className="text-[10px] text-blue-500 hover:underline flex items-center gap-0.5 ml-2"
                            >
                                <X size={10} /> {t('monitor.filters.reset')}
                            </button>
                        )}
                    </div>
                </div>

                {/* Table */}
                <div className="flex-1 overflow-auto bg-white dark:bg-base-100">
                    <table className="table table-xs w-full table-pin-rows">
                        <thead className="bg-gray-50 dark:bg-base-200 text-gray-500 dark:text-gray-400 sticky top-0 z-10">
                            <tr>
                                <th className="w-16 bg-gray-50 dark:bg-base-200">{t('monitor.table.status')}</th>
                                <th className="w-20 bg-gray-50 dark:bg-base-200">{t('monitor.table.method')}</th>
                                <th className="w-48 bg-gray-50 dark:bg-base-200">{t('monitor.table.model')}</th>
                                <th className="w-64 bg-gray-50 dark:bg-base-200">{t('monitor.table.path')}</th>
                                <th className="w-24 text-right bg-gray-50 dark:bg-base-200">{t('monitor.table.usage', 'Tokens')}</th>
                                <th className="w-24 text-right bg-gray-50 dark:bg-base-200">{t('monitor.table.duration')}</th>
                                <th className="w-32 text-right bg-gray-50 dark:bg-base-200">{t('monitor.table.time')}</th>
                            </tr>
                        </thead>
                        <tbody className="font-mono text-gray-700 dark:text-gray-300">
                            {filteredLogs.map(log => (
                                <tr 
                                    key={log.id} 
                                    className="hover:bg-blue-50 dark:hover:bg-blue-900/20 border-b border-gray-50 dark:border-base-200/50 cursor-pointer transition-colors"
                                    onClick={() => setSelectedLog(log)}
                                >
                                    <td>
                                        {log.status >= 200 && log.status < 400 ? (
                                            <span className="badge badge-xs badge-success text-white border-none">{log.status}</span>
                                        ) : (
                                            <span className="badge badge-xs badge-error text-white border-none">{log.status}</span>
                                        )}
                                    </td>
                                    <td className="font-bold">{log.method}</td>
                                    <td className="text-blue-600 dark:text-blue-400 truncate max-w-[180px]" title={log.model || '-'}>
                                        {log.model || '-'}
                                    </td>
                                    <td className="truncate max-w-[240px]" title={log.url}>{log.url}</td>
                                    <td className="text-right">
                                        {(log.input_tokens != null || log.output_tokens != null) ? (
                                            <div className="flex flex-col items-end gap-0.5 leading-none">
                                                <div className="text-[9px] flex items-center gap-1">
                                                    <span className="text-blue-500 font-bold opacity-70">I</span>
                                                    <span className="text-gray-500 dark:text-gray-400">{log.input_tokens ?? 0}</span>
                                                </div>
                                                <div className="text-[9px] flex items-center gap-1">
                                                    <span className="text-green-500 font-bold opacity-70">O</span>
                                                    <span className="text-gray-500 dark:text-gray-400">{log.output_tokens ?? 0}</span>
                                                </div>
                                            </div>
                                        ) : (
                                            <span className="text-gray-300 dark:text-gray-600">-</span>
                                        )}
                                    </td>
                                    <td className="text-right text-gray-500">{log.duration}ms</td>
                                    <td className="text-right text-gray-400 text-[10px]">
                                        {new Date(log.timestamp).toLocaleTimeString()}
                                    </td>
                                </tr>
                            ))}
                             {filteredLogs.length === 0 && (
                                <tr>
                                    <td colSpan={7} className="text-center py-8 text-gray-400">
                                        {t('monitor.table.empty')}
                                    </td>
                                </tr>
                            )}
                        </tbody>
                    </table>
                </div>
            </div>

            {/* Detail Modal */}
            {selectedLog && (
                <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm p-4 animate-in fade-in duration-200" onClick={() => setSelectedLog(null)}>
                    <div className="bg-white dark:bg-base-100 rounded-xl shadow-2xl w-full max-w-4xl max-h-[90vh] flex flex-col overflow-hidden" onClick={e => e.stopPropagation()}>
                        {/* Header */}
                        <div className="px-4 py-3 border-b border-gray-100 dark:border-base-200 flex items-center justify-between bg-gray-50 dark:bg-base-200">
                            <div className="flex items-center gap-3">
                                <span className={`badge badge-sm ${selectedLog.status >= 200 && selectedLog.status < 400 ? 'badge-success' : 'badge-error'} text-white border-none`}>
                                    {selectedLog.status}
                                </span>
                                <span className="font-mono font-bold text-sm">{selectedLog.method}</span>
                                <span className="text-xs text-gray-500 font-mono truncate max-w-md">{selectedLog.url}</span>
                            </div>
                            <button onClick={() => setSelectedLog(null)} className="btn btn-ghost btn-sm btn-circle">
                                <X size={18} />
                            </button>
                        </div>

                        {/* Content */}
                        <div className="flex-1 overflow-y-auto p-4 space-y-6">
                            {/* Metadata Section */}
                            <div className="bg-gray-50 dark:bg-base-200/50 p-4 rounded-xl border border-gray-100 dark:border-base-300/30 space-y-4">
                                <div className="flex flex-wrap gap-y-4 gap-x-10 text-xs">
                                    <div>
                                        <span className="block text-gray-400 mb-1.5 uppercase tracking-wider font-bold text-[10px]">{t('monitor.details.time')}</span>
                                        <span className="font-mono font-medium text-gray-700 dark:text-gray-200">{new Date(selectedLog.timestamp).toLocaleString()}</span>
                                    </div>
                                    <div>
                                        <span className="block text-gray-400 mb-1.5 uppercase tracking-wider font-bold text-[10px]">{t('monitor.details.duration')}</span>
                                        <span className="font-mono font-medium text-gray-700 dark:text-gray-200">{selectedLog.duration}ms</span>
                                    </div>
                                    <div>
                                        <span className="block text-gray-400 mb-1.5 uppercase tracking-wider font-bold text-[10px]">{t('monitor.details.tokens')}</span>
                                        <div className="font-mono text-[11px] flex gap-2">
                                            <span className="text-blue-600 bg-blue-100/50 dark:bg-blue-900/30 px-2 py-0.5 rounded border border-blue-200/50 dark:border-blue-800/50">In: {selectedLog.input_tokens ?? 0}</span>
                                            <span className="text-green-600 bg-green-100/50 dark:bg-green-900/30 px-2 py-0.5 rounded border border-green-200/50 dark:border-green-800/50">Out: {selectedLog.output_tokens ?? 0}</span>
                                        </div>
                                    </div>
                                </div>

                                <div className="pt-3 border-t border-gray-200/50 dark:border-base-300/50 grid grid-cols-1 md:grid-cols-2 gap-4">
                                    <div>
                                        <span className="block text-gray-400 mb-1.5 uppercase tracking-wider font-bold text-[10px]">{t('monitor.details.model')}</span>
                                        <span className="font-mono font-bold text-blue-600 dark:text-blue-400 break-all">{selectedLog.model || '-'}</span>
                                    </div>
                                    <div>
                                        <span className="block text-gray-400 mb-1.5 uppercase tracking-wider font-bold text-[10px]">{t('monitor.details.id')}</span>
                                        <span className="font-mono text-gray-500 dark:text-gray-400 break-all select-all bg-black/5 dark:bg-white/5 px-1.5 py-0.5 rounded">{selectedLog.id}</span>
                                    </div>
                                </div>
                            </div>

                            {/* Request Body */}
                            <div>
                                <h3 className="text-xs font-bold uppercase tracking-wider text-gray-400 mb-2 flex items-center gap-2">
                                    <ArrowUp size={14} className="text-blue-500" /> {t('monitor.details.request_payload')}
                                </h3>
                                <div className="bg-gray-50 dark:bg-base-300 rounded-lg p-3 border border-gray-100 dark:border-base-200 overflow-hidden">
                                    {formatBody(selectedLog.request_body)}
                                </div>
                            </div>

                            {/* Response Body */}
                            <div>
                                <h3 className="text-xs font-bold uppercase tracking-wider text-gray-400 mb-2 flex items-center gap-2">
                                    <ArrowDown size={14} className="text-green-500" /> {t('monitor.details.response_payload')}
                                </h3>
                                <div className="bg-gray-50 dark:bg-base-300 rounded-lg p-3 border border-gray-100 dark:border-base-200 overflow-hidden">
                                    {formatBody(selectedLog.response_body)}
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            )}
        </>
    );
};
