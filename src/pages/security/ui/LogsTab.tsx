// File: src/pages/security/ui/LogsTab.tsx
// Access logs table

import { motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { FileText, ShieldAlert, CheckCircle2, Clock } from 'lucide-react';
import { cn } from '@/shared/lib';
import type { AccessLogEntry } from '@/entities/security';

interface LogsTabProps {
    accessLogs: AccessLogEntry[];
    formatTimestamp: (timestamp: number) => string;
}

export function LogsTab({ accessLogs, formatTimestamp }: LogsTabProps) {
    const { t } = useTranslation();

    if (accessLogs.length === 0) {
        return (
            <div className="flex flex-col items-center justify-center py-16 text-zinc-400 dark:text-zinc-500 bg-zinc-50/50 dark:bg-zinc-900/10 rounded-xl border border-dashed border-zinc-200 dark:border-zinc-800">
                <FileText className="w-12 h-12 mb-3 opacity-20" />
                <p className="text-sm font-medium">{t('security.no_logs', 'No access logs')}</p>
                <p className="text-xs opacity-70 mt-1">Requests to the proxy will appear here.</p>
            </div>
        );
    }

    return (
         <div className="w-full overflow-hidden">
             {/* Header */}
             <div className="grid grid-cols-[80px_100px_1fr_1.5fr_1.5fr_150px] gap-4 px-4 py-2 text-[10px] font-semibold text-zinc-500 dark:text-zinc-400 uppercase tracking-wide bg-zinc-50 dark:bg-zinc-800/50 border-b border-zinc-200 dark:border-zinc-800 rounded-t-lg">
                <div>Status</div>
                <div>Method</div>
                <div>Path</div>
                <div>IP Address</div>
                <div>Action / Reason</div>
                <div>Time</div>
            </div>

            <motion.div
                key="logs"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="space-y-px bg-zinc-200 dark:bg-zinc-800"
            >
                {accessLogs.map((log) => {
                    const isBlocked = log.blocked || log.statusCode === 403 || log.statusCode === 429;
                    const isSuccess = log.statusCode >= 200 && log.statusCode < 300;
                    
                    return (
                        <div
                            key={log.id}
                            className={cn(
                                "grid grid-cols-[80px_100px_1fr_1.5fr_1.5fr_150px] gap-4 items-center px-4 py-2.5 bg-white dark:bg-zinc-900 transition-colors",
                                isBlocked 
                                    ? "hover:bg-red-50/50 dark:hover:bg-red-900/10" 
                                    : "hover:bg-zinc-50 dark:hover:bg-zinc-800/80"
                            )}
                        >
                            {/* Status */}
                            <div>
                                <span className={cn(
                                    "px-2 py-0.5 rounded text-[10px] font-bold font-mono",
                                    isBlocked ? "bg-red-100 dark:bg-red-500/20 text-red-600 dark:text-red-400" :
                                    isSuccess ? "bg-emerald-100 dark:bg-emerald-500/20 text-emerald-600 dark:text-emerald-400" :
                                    "bg-zinc-100 dark:bg-zinc-700 text-zinc-600 dark:text-zinc-400"
                                )}>
                                    {log.statusCode}
                                </span>
                            </div>

                            {/* Method */}
                            <div className="text-xs font-semibold text-zinc-500 dark:text-zinc-400">
                                {log.method}
                            </div>

                            {/* Path */}
                            <div className="text-xs font-mono text-zinc-600 dark:text-zinc-300 truncate" title={log.path}>
                                {log.path}
                            </div>

                            {/* IP */}
                            <div className="text-xs font-mono font-medium text-zinc-700 dark:text-zinc-200 truncate">
                                {log.ipAddress}
                            </div>

                            {/* Reason */}
                            <div className="text-xs truncate">
                                {isBlocked ? (
                                    <div className="flex items-center gap-1.5 text-red-600 dark:text-red-400">
                                        <ShieldAlert className="w-3.5 h-3.5" />
                                        <span>{log.blockReason || 'Blocked'}</span>
                                    </div>
                                ) : (
                                    <div className="flex items-center gap-1.5 text-emerald-600 dark:text-emerald-400 opacity-60">
                                        <CheckCircle2 className="w-3.5 h-3.5" />
                                        <span>Allowed</span>
                                    </div>
                                )}
                            </div>

                            {/* Time */}
                            <div className="flex items-center gap-1.5 text-xs text-zinc-400 dark:text-zinc-500">
                                <Clock className="w-3 h-3 opacity-70" />
                                {formatTimestamp(log.timestamp)}
                            </div>
                        </div>
                    );
                })}
            </motion.div>
        </div>
    );
}
