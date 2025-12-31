import React, { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { ask } from '@tauri-apps/plugin-dialog';
import { useTranslation } from 'react-i18next';
import { request as invoke } from '../../utils/request';
import { Trash2, Search, X } from 'lucide-react';
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

    const filteredLogs = logs.filter(log => 
        log.url.toLowerCase().includes(filter.toLowerCase()) || 
        log.method.toLowerCase().includes(filter.toLowerCase()) ||
        (log.model && log.model.toLowerCase().includes(filter.toLowerCase())) ||
        log.status.toString().includes(filter)
    );

    const quickFilters = [
        { label: t('monitor.filters.all'), value: '' },
        { label: t('monitor.filters.error'), value: '40' },
        { label: t('monitor.filters.chat'), value: 'completions' },
        { label: t('monitor.filters.gemini'), value: 'gemini' },
        { label: t('monitor.filters.claude'), value: 'claude' },
        { label: t('monitor.filters.images'), value: 'images' }
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
                        className={`btn btn-sm gap-2 px-4 border font-bold ${
                            isLoggingEnabled 
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
                        <span className="text-blue-500">{stats.total_requests} REQS</span>
                        <span className="text-green-500">{stats.success_count} OK</span>
                        <span className="text-red-500">{stats.error_count} ERR</span>
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

            <div className="flex-1 overflow-auto bg-white dark:bg-base-100">
                <table className="table table-xs w-full">
                    <thead className="bg-gray-50 dark:bg-base-200 text-gray-500 sticky top-0">
                        <tr>
                            <th>{t('monitor.table.status')}</th>
                            <th>{t('monitor.table.method')}</th>
                            <th>{t('monitor.table.model')}</th>
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
                                <td className="text-blue-600 truncate max-w-[180px]">{log.model || '-'}</td>
                                <td className="truncate max-w-[240px]">{log.url}</td>
                                <td className="text-right text-[9px]">
                                    {log.input_tokens != null && <div>I: {log.input_tokens}</div>}
                                    {log.output_tokens != null && <div>O: {log.output_tokens}</div>}
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
                                            <span className="text-blue-700 dark:text-blue-300 bg-blue-100 dark:bg-blue-900/40 px-2.5 py-1 rounded-md border border-blue-200 dark:border-blue-800/50 font-bold">In: {selectedLog.input_tokens ?? 0}</span>
                                            <span className="text-green-700 dark:text-green-300 bg-green-100 dark:bg-green-900/40 px-2.5 py-1 rounded-md border border-green-200 dark:border-green-800/50 font-bold">Out: {selectedLog.output_tokens ?? 0}</span>
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
        </div>
    );
};
