// File: src/widgets/debug-console/ui/DebugConsole.tsx
// Debug console component - optimized with virtualization (react-window v2)

import React, { useEffect, useRef, useMemo, useCallback, useState, memo, CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { motion, AnimatePresence } from 'framer-motion';
import { List, useListRef } from 'react-window';
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
    ArrowDownToLine,
    Pause,
    Play
} from 'lucide-react';

import { cn, copyToClipboard } from '@/shared/lib';
import { useDebugConsole, LogEntry, LogLevel } from '../model/store';
import { showToast } from '@/shared/ui';

// Level configuration - static, no re-renders
const LEVEL_CONFIG: Record<LogLevel, { icon: React.ReactNode; color: string; bg: string }> = {
    ERROR: { 
        icon: <AlertCircle size={12} />, 
        color: 'text-rose-500', 
        bg: 'bg-rose-500/10' 
    },
    WARN: { 
        icon: <AlertTriangle size={12} />, 
        color: 'text-amber-500', 
        bg: 'bg-amber-500/10' 
    },
    INFO: { 
        icon: <Info size={12} />, 
        color: 'text-blue-500', 
        bg: 'bg-blue-500/10' 
    },
    DEBUG: { 
        icon: <Bug size={12} />, 
        color: 'text-purple-500', 
        bg: 'bg-purple-500/10' 
    },
    TRACE: { 
        icon: <Terminal size={12} />, 
        color: 'text-zinc-500', 
        bg: 'bg-zinc-500/10' 
    },
};

// ROW_HEIGHT for virtualization
const ROW_HEIGHT = 32;

// Row component props type
interface LogRowProps {
    logs: LogEntry[];
}

// Memoized log row component for react-window v2
const LogRowComponent = ({ index, style, logs }: {
    ariaAttributes: { 'aria-posinset': number; 'aria-setsize': number; role: 'listitem' };
    index: number;
    style: CSSProperties;
    logs: LogEntry[];
}) => {
    const log = logs[index];
    if (!log) return null;
    
    const config = LEVEL_CONFIG[log.level] || LEVEL_CONFIG.INFO;
    
    // Pre-compute time string
    const date = new Date(log.timestamp);
    const time = date.toLocaleTimeString('en-US', { 
        hour12: false, 
        hour: '2-digit', 
        minute: '2-digit', 
        second: '2-digit'
    }) + '.' + String(date.getMilliseconds()).padStart(3, '0');

    // Shortened target
    const shortTarget = log.target.split('::').slice(-2).join('::');

    return (
        <div 
            style={style}
            className="flex items-center gap-2 px-3 text-[11px] font-mono border-b border-zinc-100 dark:border-zinc-800/50 hover:bg-zinc-50 dark:hover:bg-zinc-800/30 transition-colors"
        >
            <span className="text-zinc-400 dark:text-zinc-500 shrink-0 w-20 select-none">{time}</span>
            <span className={cn("shrink-0 w-14 flex items-center gap-1 font-bold", config.color)}>
                {config.icon}
                <span className="text-[10px]">{log.level}</span>
            </span>
            <span 
                className="text-zinc-500 dark:text-zinc-500 shrink-0 max-w-32 truncate font-medium" 
                title={log.target}
            >
                {shortTarget}
            </span>
            <span className="text-zinc-700 dark:text-zinc-300 flex-1 truncate">
                {log.message}
            </span>
        </div>
    );
};

// Debounce hook
function useDebouncedValue<T>(value: T, delay: number): T {
    const [debouncedValue, setDebouncedValue] = useState(value);

    useEffect(() => {
        const timer = setTimeout(() => setDebouncedValue(value), delay);
        return () => clearTimeout(timer);
    }, [value, delay]);

    return debouncedValue;
}

// Memoized footer stats
const FooterStats = memo<{ logs: LogEntry[] }>(({ logs }) => {
    const counts = useMemo(() => {
        const result: Partial<Record<LogLevel, number>> = {};
        for (const log of logs) {
            result[log.level] = (result[log.level] || 0) + 1;
        }
        return result;
    }, [logs]);

    return (
        <div className="flex items-center gap-4">
            {(Object.keys(LEVEL_CONFIG) as LogLevel[]).map(level => {
                const count = counts[level];
                if (!count) return null;
                return (
                    <span 
                        key={level}
                        className="font-medium flex items-center gap-1.5 opacity-90"
                    >
                        {LEVEL_CONFIG[level].icon}
                        {count}
                    </span>
                );
            })}
        </div>
    );
});

FooterStats.displayName = 'FooterStats';

interface DebugConsoleProps {
    embedded?: boolean;
}

export const DebugConsole: React.FC<DebugConsoleProps> = ({ embedded = false }) => {
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

    const listRef = useListRef(null);
    const containerRef = useRef<HTMLDivElement>(null);
    const [filterOpen, setFilterOpen] = useState(false);
    const [inputValue, setInputValue] = useState(searchTerm);
    const prevLogsLengthRef = useRef(0);

    // Debounce search term (300ms)
    const debouncedSearch = useDebouncedValue(inputValue, 300);

    // Sync debounced search to store
    useEffect(() => {
        setSearchTerm(debouncedSearch);
    }, [debouncedSearch, setSearchTerm]);

    // Filter logs - memoized with debounced search
    const filteredLogs = useMemo(() => {
        const term = debouncedSearch.toLowerCase();
        return logs.filter(log => {
            if (!filter.includes(log.level)) return false;
            if (term) {
                return (
                    log.message.toLowerCase().includes(term) ||
                    log.target.toLowerCase().includes(term)
                );
            }
            return true;
        });
    }, [logs, filter, debouncedSearch]);

    // Auto-scroll to bottom when new logs arrive
    useEffect(() => {
        if (autoScroll && listRef.current && filteredLogs.length > 0 && filteredLogs.length !== prevLogsLengthRef.current) {
            listRef.current.scrollToRow({ index: filteredLogs.length - 1, align: 'end' });
        }
        prevLogsLengthRef.current = filteredLogs.length;
    }, [filteredLogs.length, autoScroll, listRef]);

    const handleCopyAll = useCallback(async () => {
        const text = filteredLogs.map(log => {
            const time = new Date(log.timestamp).toISOString();
            return `[${time}] [${log.level}] [${log.target}] ${log.message}`;
        }).join('\n');
        
        const success = await copyToClipboard(text);
        if (success) {
            showToast(t('debug_console.copied', { defaultValue: 'Logs copied to clipboard' }), 'success');
        }
    }, [filteredLogs, t]);

    const handleExport = useCallback(() => {
        const text = filteredLogs.map(log => JSON.stringify(log)).join('\n');
        
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

    const scrollToBottom = useCallback(() => {
        if (listRef.current && filteredLogs.length > 0) {
            listRef.current.scrollToRow({ index: filteredLogs.length - 1, align: 'end' });
            setAutoScroll(true);
        }
    }, [filteredLogs.length, setAutoScroll, listRef]);

    // Content component (shared between embedded and panel modes)
    const content = (
        <div className={cn(
            "flex flex-col font-sans transition-colors duration-200",
            "bg-white dark:bg-zinc-900",
            "text-zinc-700 dark:text-zinc-300",
            embedded
                ? "h-full w-full rounded-xl border border-zinc-200 dark:border-zinc-800 overflow-hidden"
                : "fixed right-0 top-0 bottom-0 w-full max-w-3xl border-l border-zinc-200 dark:border-zinc-800 z-50 shadow-2xl"
        )}>
            {/* Header */}
            <div className={cn(
                "flex items-center justify-between px-4 py-3 border-b shrink-0",
                "bg-zinc-50 dark:bg-zinc-900/80",
                "border-zinc-200 dark:border-zinc-800",
                embedded && "rounded-t-xl"
            )}>
                <div className="flex items-center gap-3">
                    <div className="p-1.5 rounded-lg bg-emerald-500/10">
                        <Terminal size={16} className="text-emerald-500" />
                    </div>
                    <h2 className="text-sm font-bold text-zinc-900 dark:text-white">
                        {t('debug_console.title', { defaultValue: 'Console' })}
                    </h2>
                    <span className="text-[10px] font-mono text-zinc-500 bg-zinc-100 dark:bg-zinc-800 px-2 py-0.5 rounded">
                        {filteredLogs.length} / {logs.length}
                    </span>
                </div>
                
                <div className="flex items-center gap-2">
                    {/* Search */}
                    <div className="relative">
                        <Search size={14} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-zinc-400" />
                        <input
                            type="text"
                            placeholder={t('debug_console.search', { defaultValue: 'Search...' })}
                            value={inputValue}
                            onChange={(e) => setInputValue(e.target.value)}
                            className={cn(
                                "w-40 bg-zinc-100 dark:bg-zinc-800 border border-transparent rounded-lg pl-8 pr-3 py-1.5 text-xs transition-all",
                                "text-zinc-700 dark:text-zinc-300 placeholder:text-zinc-400",
                                "focus:outline-none focus:w-56 focus:border-zinc-300 dark:focus:border-zinc-600"
                            )}
                        />
                    </div>
                    
                    {/* Filter dropdown */}
                    <div className="relative">
                        <button
                            onClick={() => setFilterOpen(!filterOpen)}
                            className={cn(
                                "flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs font-medium transition-colors",
                                filterOpen 
                                    ? "bg-indigo-100 dark:bg-indigo-500/20 text-indigo-600 dark:text-indigo-400" 
                                    : "bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-white"
                            )}
                        >
                            <Filter size={14} />
                            <ChevronDown size={12} className={cn("transition-transform", filterOpen && "rotate-180")} />
                        </button>
                        
                        {filterOpen && (
                            <div className="absolute right-0 top-full mt-1 bg-white dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg shadow-xl p-2 z-10 min-w-36">
                                {(Object.keys(LEVEL_CONFIG) as LogLevel[]).map(level => (
                                    <label
                                        key={level}
                                        className="flex items-center gap-2 px-2 py-1.5 hover:bg-zinc-50 dark:hover:bg-zinc-700/50 rounded-md cursor-pointer"
                                    >
                                        <input
                                            type="checkbox"
                                            checked={filter.includes(level)}
                                            onChange={() => toggleLevel(level)}
                                            className="w-3.5 h-3.5 rounded border-zinc-300 dark:border-zinc-600"
                                        />
                                        <span className={cn("text-xs font-medium", LEVEL_CONFIG[level].color)}>
                                            {level}
                                        </span>
                                    </label>
                                ))}
                            </div>
                        )}
                    </div>

                    {/* Auto-scroll toggle */}
                    <button
                        onClick={() => setAutoScroll(!autoScroll)}
                        className={cn(
                            "p-1.5 rounded-lg transition-colors",
                            autoScroll
                                ? "bg-emerald-100 dark:bg-emerald-500/20 text-emerald-600 dark:text-emerald-400"
                                : "bg-zinc-100 dark:bg-zinc-800 text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300"
                        )}
                        title={autoScroll ? t('debug_console.pause_scroll', { defaultValue: 'Pause scroll' }) : t('debug_console.resume_scroll', { defaultValue: 'Resume scroll' })}
                    >
                        {autoScroll ? <Pause size={14} /> : <Play size={14} />}
                    </button>
                    
                    <button
                        onClick={handleCopyAll}
                        className="p-1.5 rounded-lg bg-zinc-100 dark:bg-zinc-800 text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300 transition-colors"
                        title={t('debug_console.copy_all', { defaultValue: 'Copy All' })}
                    >
                        <Copy size={14} />
                    </button>
                    
                    <button
                        onClick={handleExport}
                        className="p-1.5 rounded-lg bg-zinc-100 dark:bg-zinc-800 text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300 transition-colors"
                        title={t('debug_console.export', { defaultValue: 'Export' })}
                    >
                        <Download size={14} />
                    </button>
                    
                    <button
                        onClick={clearLogs}
                        className="p-1.5 rounded-lg bg-zinc-100 dark:bg-zinc-800 text-zinc-400 hover:text-rose-500 transition-colors"
                        title={t('debug_console.clear', { defaultValue: 'Clear' })}
                    >
                        <Trash2 size={14} />
                    </button>

                    {!embedded && (
                        <button
                            onClick={close}
                            className="p-1.5 rounded-lg bg-zinc-100 dark:bg-zinc-800 text-zinc-400 hover:text-zinc-600 dark:hover:text-white transition-colors"
                        >
                            <X size={14} />
                        </button>
                    )}
                </div>
            </div>
            
            {/* Log content - Virtualized */}
            <div 
                ref={containerRef}
                className={cn(
                    "flex-1 overflow-hidden",
                    "bg-white dark:bg-zinc-950"
                )}
                style={{ minHeight: 0 }}
            >
                {filteredLogs.length === 0 ? (
                    <div className="flex flex-col items-center justify-center h-full text-zinc-400 dark:text-zinc-600">
                        <Terminal size={48} className="mb-4 opacity-30" />
                        <p className="text-sm font-medium">{t('debug_console.no_logs', { defaultValue: 'No logs to display' })}</p>
                        <p className="text-xs mt-1 opacity-70">{t('debug_console.no_logs_hint', { defaultValue: 'Logs will appear here in real-time' })}</p>
                    </div>
                ) : (
                    <List<LogRowProps>
                        listRef={listRef}
                        rowCount={filteredLogs.length}
                        rowHeight={ROW_HEIGHT}
                        rowComponent={LogRowComponent}
                        rowProps={{ logs: filteredLogs }}
                        overscanCount={20}
                        className="scrollbar-thin scrollbar-thumb-zinc-300 dark:scrollbar-thumb-zinc-700 scrollbar-track-transparent"
                        style={{ height: '100%', width: '100%' }}
                    />
                )}
            </div>
            
            {/* Footer */}
            <div className={cn(
                "flex items-center justify-between px-4 py-2 border-t text-white text-[10px] shrink-0",
                "bg-indigo-600",
                embedded && "rounded-b-xl"
            )}>
                <FooterStats logs={logs} />
                
                <div className="flex items-center gap-3">
                    {!autoScroll && (
                        <button
                            onClick={scrollToBottom}
                            className="flex items-center gap-1.5 px-2 py-0.5 rounded bg-black/20 hover:bg-black/30 font-medium transition-colors"
                        >
                            <ArrowDownToLine size={10} />
                            {t('debug_console.scroll_to_bottom', { defaultValue: 'Scroll' })}
                        </button>
                    )}
                    <span className="opacity-80 flex items-center gap-1">
                        <div className="w-1.5 h-1.5 rounded-full bg-white animate-pulse"></div>
                        Live
                    </span>
                </div>
            </div>
        </div>
    );

    // If embedded, just return the content directly
    if (embedded) {
        return content;
    }

    // Panel mode with animations
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
                    >
                        {content}
                    </motion.div>
                </>
            )}
        </AnimatePresence>
    );
};

export default DebugConsole;
