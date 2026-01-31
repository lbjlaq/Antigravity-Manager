// File: src/pages/security/ui/LogsTab.tsx
// Access logs tab content

import { motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { FileText } from 'lucide-react';
import { cn } from '@/shared/lib';
import type { AccessLogEntry } from '@/types/security';

interface LogsTabProps {
    accessLogs: AccessLogEntry[];
    formatTimestamp: (timestamp: number) => string;
}

export function LogsTab({ accessLogs, formatTimestamp }: LogsTabProps) {
    const { t } = useTranslation();

    if (accessLogs.length === 0) {
        return (
            <div className="flex flex-col items-center justify-center py-12 text-gray-400 dark:text-zinc-500">
                <FileText className="w-12 h-12 mb-3 opacity-30" />
                <p className="text-sm">{t('security.no_logs', 'No access logs')}</p>
            </div>
        );
    }

    return (
        <motion.div
            key="logs"
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -10 }}
            className="space-y-2"
        >
            {accessLogs.map((log) => (
                <div
                    key={log.id}
                    className={cn(
                        'flex items-center justify-between p-3 rounded-xl border transition-all',
                        log.blocked
                            ? 'bg-red-50 dark:bg-red-500/5 border-red-200 dark:border-red-500/20'
                            : 'bg-gray-50 dark:bg-zinc-800/50 border-gray-100 dark:border-zinc-700/50'
                    )}
                >
                    <div className="flex items-center gap-3">
                        <span
                            className={cn(
                                'px-2 py-1 text-xs font-mono font-bold rounded',
                                log.blocked
                                    ? 'bg-red-100 dark:bg-red-500/20 text-red-600 dark:text-red-400'
                                    : 'bg-gray-100 dark:bg-zinc-700 text-gray-600 dark:text-zinc-300'
                            )}
                        >
                            {log.statusCode}
                        </span>
                        <span className="font-mono text-sm text-gray-700 dark:text-zinc-300">{log.ipAddress}</span>
                        <span className="text-xs text-gray-400 dark:text-zinc-500 truncate max-w-[200px]">
                            {log.method} {log.path}
                        </span>
                    </div>
                    <div className="flex items-center gap-3 text-xs text-gray-400 dark:text-zinc-500">
                        {log.blocked && log.blockReason && (
                            <span className="text-red-500 dark:text-red-400">{log.blockReason}</span>
                        )}
                        <span>{formatTimestamp(log.timestamp)}</span>
                    </div>
                </div>
            ))}
        </motion.div>
    );
}
