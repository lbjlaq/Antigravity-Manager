// File: src/widgets/debug-console/ui/DebugConsole.tsx
// Debug console panel component

import React, { useEffect, useRef, useMemo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { motion, AnimatePresence } from 'framer-motion';
import { 
    X, 
    Copy, 
    Trash2, 
    Download, 
    Search, 
    Filter,
    ChevronDown,
    AlertCircle,
    AlertTriangle,
    Info,
    Bug,
    Terminal,
    ArrowDownToLine
} from 'lucide-react';

import { cn, copyToClipboard } from '@/shared/lib';
import { useDebugConsole, LogEntry, LogLevel } from '../model/store';

// Import toast from shared (will be migrated)
import { showToast } from '@/components/common/ToastContainer';

const LEVEL_CONFIG: Record<LogLevel, { icon: React.ReactNode; color: string; bg: string }> = {
    ERROR: { 
        icon: <AlertCircle size={12} />, 
        color: 'text-red-400', 
        bg: 'bg-red-500/10 border-red-500/20' 
    },
    WARN: { 
        icon: <AlertTriangle size={12} />, 
        color: 'text-amber-400', 
        bg: 'bg-amber-500/10 border-amber-500/20' 
    },
    INFO: { 
        icon: <Info size={12} />, 
        color: 'text-blue-400', 
        bg: 'bg-blue-500/10 border-blue-500/20' 
    },
    DEBUG: { 
        icon: <Bug size={12} />, 
        color: 'text-purple-400', 
        bg: 'bg-purple-500/10 border-purple-500/20' 
    },
    TRACE: { 
        icon: <Terminal size={12} />, 
        color: 'text-gray-400', 
        bg: 'bg-gray-500/10 border-gray-500/20' 
    },
};

const LogRow: React.FC<{ log: LogEntry }> = React.memo(({ log }) => {
    const config = LEVEL_CONFIG[log.level] || LEVEL_CONFIG.INFO;
    const date = new Date(log.timestamp);
    const time = date.toLocaleTimeString('en-US', { 
        hour12: false, 
        hour: '2-digit', 
        minute: '2-digit', 
        second: '2-digit'
    }) + '.' + String(date.getMilliseconds()).padStart(3, '0');

    return (
        <div className={cn(
            "flex items-start gap-2 px-3 py-1.5 border-b border-white/5 hover:bg-white/5 transition-colors text-[11px] font-mono",
            config.bg
        )}>
            <span className="text-zinc-500 shrink-0 w-20">{time}</span>
            <span className={cn("shrink-0 w-12 flex items-center gap-1", config.color)}>
                {config.icon}
                <span className="font-bold text-[10px]">{log.level}</span>
            </span>
            <span className="text-zinc-600 shrink-0 max-w-32 truncate" title={log.target}>
                [{log.target.split('::').pop()}]
            </span>
            <span className="text-zinc-200 flex-1 break-all whitespace-pre-wrap">
                {log.message}
                {Object.keys(log.fields).length > 0 && (
                    <span className="text-zinc-500 ml-2">
                        {Object.entries(log.fields).map(([k, v]) => `${k}=${v}`).join(' ')}
                    </span>
                )}
            </span>
        </div>
    );
});

LogRow.displayName = 'LogRow';

export const DebugConsole: React.FC = () => {
    const { t } = useTranslation();
    const {
        isOpen,
        close,
        logs,
        filter,
        searchTerm,
        autoScroll,
        setFilter,
        setSearchTerm,
        setAutoScroll,
        clearLogs,
    } = useDebugConsole();

    const scrollRef = useRef<HTMLDivElement>(null);
    const [filterOpen, setFilterOpen] = React.useState(false);

    const filteredLogs = useMemo(() => {
        return logs.filter(log => {
            if (!filter.includes(log.level)) return false;
            if (searchTerm) {
                const term = searchTerm.toLowerCase();
                return (
                    log.message.toLowerCase().includes(term) ||
                    log.target.toLowerCase().includes(term) ||
                    Object.values(log.fields).some(v => v.toLowerCase().includes(term))
                );
            }
            return true;
        });
    }, [logs, filter, searchTerm]);

    useEffect(() => {
        if (autoScroll && scrollRef.current) {
            scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
        }
    }, [filteredLogs, autoScroll]);

    const handleCopyAll = useCallback(async () => {
        const text = filteredLogs.map(log => {
            const time = new Date(log.timestamp).toISOString();
            const fields = Object.entries(log.fields).map(([k, v]) => `${k}=${v}`).join(' ');
            return `[${time}] [${log.level}] [${log.target}] ${log.message} ${fields}`.trim();
        }).join('\n');
        
        const success = await copyToClipboard(text);
        if (success) {
            showToast(t('debug_console.copied', { defaultValue: 'Logs copied to clipboard' }), 'success');
        }
    }, [filteredLogs, t]);

    const handleExport = useCallback(() => {
        const text = filteredLogs.map(log => {
            const time = new Date(log.timestamp).toISOString();
            return JSON.stringify({ ...log, time });
        }).join('\n');
        
        const blob = new Blob([text], { type: 'application/json' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = `antigravity-logs-${new Date().toISOString().split('T')[0]}.jsonl`;
        a.click();
        URL.revokeObjectURL(url);
        
        showToast(t('debug_console.exported', { defaultValue: 'Logs exported' }), 'success');
    }, [filteredLogs, t]);

    const toggleLevel = useCallback((level: LogLevel) => {
        if (filter.includes(level)) {
            setFilter(filter.filter(l => l !== level));
        } else {
            setFilter([...filter, level]);
        }
    }, [filter, setFilter]);

    const handleScroll = useCallback((e: React.UIEvent<HTMLDivElement>) => {
        const el = e.currentTarget;
        const isAtBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 50;
        if (isAtBottom !== autoScroll) {
            setAutoScroll(isAtBottom);
        }
    }, [autoScroll, setAutoScroll]);

    const scrollToBottom = useCallback(() => {
        if (scrollRef.current) {
            scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
            setAutoScroll(true);
        }
    }, [setAutoScroll]);

    return (
        <AnimatePresence>
            {isOpen && (
                <>
                    <motion.div
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 1 }}
                        exit={{ opacity: 0 }}
                        className="fixed inset-0 bg-black/50 z-40"
                        onClick={close}
                    />
                    
                    <motion.div
                        initial={{ x: '100%' }}
                        animate={{ x: 0 }}
                        exit={{ x: '100%' }}
                        transition={{ type: 'spring', damping: 25, stiffness: 300 }}
                        className="fixed right-0 top-0 bottom-0 w-full max-w-3xl bg-zinc-900 border-l border-white/10 z-50 flex flex-col shadow-2xl"
                    >
                        {/* Header */}
                        <div className="flex items-center justify-between px-4 py-3 border-b border-white/10 bg-zinc-800/50">
                            <div className="flex items-center gap-3">
                                <Terminal size={18} className="text-green-400" />
                                <h2 className="text-sm font-bold text-white">
                                    {t('debug_console.title', { defaultValue: 'Debug Console' })}
                                </h2>
                                <span className="text-[10px] font-mono text-zinc-500 bg-zinc-800 px-2 py-0.5 rounded">
                                    {filteredLogs.length} / {logs.length}
                                </span>
                            </div>
                            
                            <div className="flex items-center gap-2">
                                <div className="relative">
                                    <Search size={14} className="absolute left-2 top-1/2 -translate-y-1/2 text-zinc-500" />
                                    <input
                                        type="text"
                                        placeholder={t('debug_console.search', { defaultValue: 'Search...' })}
                                        value={searchTerm}
                                        onChange={(e) => setSearchTerm(e.target.value)}
                                        className="w-40 bg-zinc-800 border border-white/10 rounded pl-7 pr-2 py-1 text-xs text-white placeholder:text-zinc-600 focus:outline-none focus:border-blue-500/50"
                                    />
                                </div>
                                
                                <div className="relative">
                                    <button
                                        onClick={() => setFilterOpen(!filterOpen)}
                                        className={cn(
                                            "flex items-center gap-1 px-2 py-1 rounded text-xs font-medium transition-colors",
                                            filterOpen ? "bg-blue-500/20 text-blue-400" : "bg-zinc-800 text-zinc-400 hover:text-white"
                                        )}
                                    >
                                        <Filter size={14} />
                                        <ChevronDown size={12} className={cn("transition-transform", filterOpen && "rotate-180")} />
                                    </button>
                                    
                                    {filterOpen && (
                                        <div className="absolute right-0 top-full mt-1 bg-zinc-800 border border-white/10 rounded-lg shadow-xl p-2 z-10 min-w-32">
                                            {(Object.keys(LEVEL_CONFIG) as LogLevel[]).map(level => (
                                                <label
                                                    key={level}
                                                    className="flex items-center gap-2 px-2 py-1 hover:bg-white/5 rounded cursor-pointer"
                                                >
                                                    <input
                                                        type="checkbox"
                                                        checked={filter.includes(level)}
                                                        onChange={() => toggleLevel(level)}
                                                        className="w-3 h-3 rounded border-zinc-600"
                                                    />
                                                    <span className={cn("text-xs font-medium", LEVEL_CONFIG[level].color)}>
                                                        {level}
                                                    </span>
                                                </label>
                                            ))}
                                        </div>
                                    )}
                                </div>
                                
                                <button
                                    onClick={handleCopyAll}
                                    className="p-1.5 rounded bg-zinc-800 text-zinc-400 hover:text-white hover:bg-zinc-700 transition-colors"
                                    title={t('debug_console.copy_all', { defaultValue: 'Copy All' })}
                                >
                                    <Copy size={14} />
                                </button>
                                
                                <button
                                    onClick={handleExport}
                                    className="p-1.5 rounded bg-zinc-800 text-zinc-400 hover:text-white hover:bg-zinc-700 transition-colors"
                                    title={t('debug_console.export', { defaultValue: 'Export' })}
                                >
                                    <Download size={14} />
                                </button>
                                
                                <button
                                    onClick={clearLogs}
                                    className="p-1.5 rounded bg-zinc-800 text-zinc-400 hover:text-red-400 hover:bg-zinc-700 transition-colors"
                                    title={t('debug_console.clear', { defaultValue: 'Clear' })}
                                >
                                    <Trash2 size={14} />
                                </button>
                                
                                <button
                                    onClick={close}
                                    className="p-1.5 rounded bg-zinc-800 text-zinc-400 hover:text-white hover:bg-zinc-700 transition-colors"
                                >
                                    <X size={14} />
                                </button>
                            </div>
                        </div>
                        
                        {/* Log content */}
                        <div 
                            ref={scrollRef}
                            onScroll={handleScroll}
                            className="flex-1 overflow-y-auto overflow-x-hidden bg-zinc-950"
                        >
                            {filteredLogs.length === 0 ? (
                                <div className="flex flex-col items-center justify-center h-full text-zinc-600">
                                    <Terminal size={48} className="mb-4 opacity-50" />
                                    <p className="text-sm">{t('debug_console.no_logs', { defaultValue: 'No logs to display' })}</p>
                                    <p className="text-xs mt-1">{t('debug_console.no_logs_hint', { defaultValue: 'Logs will appear here in real-time' })}</p>
                                </div>
                            ) : (
                                filteredLogs.map(log => <LogRow key={log.id} log={log} />)
                            )}
                        </div>
                        
                        {/* Footer */}
                        <div className="flex items-center justify-between px-4 py-2 border-t border-white/10 bg-zinc-800/50">
                            <div className="flex items-center gap-3">
                                {(Object.keys(LEVEL_CONFIG) as LogLevel[]).map(level => {
                                    const count = logs.filter(l => l.level === level).length;
                                    if (count === 0) return null;
                                    return (
                                        <span 
                                            key={level}
                                            className={cn("text-[10px] font-mono flex items-center gap-1", LEVEL_CONFIG[level].color)}
                                        >
                                            {LEVEL_CONFIG[level].icon}
                                            {count}
                                        </span>
                                    );
                                })}
                            </div>
                            
                            {!autoScroll && (
                                <button
                                    onClick={scrollToBottom}
                                    className="flex items-center gap-1 px-2 py-1 rounded bg-blue-500/20 text-blue-400 text-xs font-medium hover:bg-blue-500/30 transition-colors"
                                >
                                    <ArrowDownToLine size={12} />
                                    {t('debug_console.scroll_to_bottom', { defaultValue: 'Scroll to bottom' })}
                                </button>
                            )}
                        </div>
                    </motion.div>
                </>
            )}
        </AnimatePresence>
    );
};

export default DebugConsole;
