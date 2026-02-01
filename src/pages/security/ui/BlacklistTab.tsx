// File: src/pages/security/ui/BlacklistTab.tsx
// Blacklist table with headers

import { motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { Shield, Ban, Hash, Clock, Trash2 } from 'lucide-react';
import type { IpBlacklistEntry } from '@/entities/security';
import { cn } from '@/shared/lib';

interface BlacklistTabProps {
    blacklist: IpBlacklistEntry[];
    onRemove: (id: number) => void;
    formatExpiresAt: (expiresAt: number | null) => string;
}

export function BlacklistTab({ blacklist, onRemove, formatExpiresAt }: BlacklistTabProps) {
    const { t } = useTranslation();

    if (blacklist.length === 0) {
        return (
            <div className="flex flex-col items-center justify-center py-16 text-zinc-400 dark:text-zinc-500 bg-zinc-50/50 dark:bg-zinc-900/10 rounded-xl border border-dashed border-zinc-200 dark:border-zinc-800">
                <Shield className="w-12 h-12 mb-3 opacity-20" />
                <p className="text-sm font-medium">{t('security.no_blacklist', 'No blocked IPs')}</p>
                <p className="text-xs opacity-70 mt-1">Add an IP to block access to your proxy.</p>
            </div>
        );
    }

    return (
        <div className="w-full overflow-hidden">
             {/* Header */}
             <div className="grid grid-cols-[1fr_1.5fr_100px_150px_60px] gap-4 px-4 py-2 text-[10px] font-semibold text-zinc-500 dark:text-zinc-400 uppercase tracking-wide bg-zinc-50 dark:bg-zinc-800/50 border-b border-zinc-200 dark:border-zinc-800 rounded-t-lg">
                <div>IP Address</div>
                <div>Reason</div>
                <div className="text-center">Hits</div>
                <div>Expires</div>
                <div className="text-center">Action</div>
            </div>

            <motion.div
                key="blacklist"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="space-y-px bg-zinc-200 dark:bg-zinc-800" // Gap effect
            >
                {blacklist.map((entry) => (
                    <div
                        key={entry.id}
                        className="group grid grid-cols-[1fr_1.5fr_100px_150px_60px] gap-4 items-center px-4 py-3 bg-white dark:bg-zinc-900 hover:bg-zinc-50 dark:hover:bg-zinc-800/80 transition-colors"
                    >
                        {/* IP */}
                        <div className="flex items-center gap-3 min-w-0">
                            <div className="p-1.5 rounded-md bg-red-100 dark:bg-red-500/10 shrink-0">
                                <Ban className="w-3.5 h-3.5 text-red-500" />
                            </div>
                            <div className="flex items-center gap-2 min-w-0">
                                <span className="font-mono text-sm font-semibold text-zinc-700 dark:text-zinc-200 truncate">
                                    {entry.ipPattern}
                                </span>
                                {entry.ipPattern.includes('/') && (
                                    <span className="px-1.5 py-0.5 text-[9px] font-bold rounded bg-blue-100 dark:bg-blue-500/20 text-blue-600 dark:text-blue-400">
                                        CIDR
                                    </span>
                                )}
                            </div>
                        </div>

                        {/* Reason */}
                        <div className="text-xs text-zinc-500 dark:text-zinc-400 truncate pr-4">
                            {entry.reason || <span className="opacity-30 italic">No reason provided</span>}
                        </div>

                        {/* Hits */}
                        <div className="flex justify-center">
                            <div className="flex items-center gap-1.5 px-2 py-1 rounded bg-zinc-100 dark:bg-zinc-800 text-xs font-medium text-zinc-600 dark:text-zinc-400">
                                <Hash className="w-3 h-3 opacity-70" />
                                {entry.hitCount}
                            </div>
                        </div>

                        {/* Expires */}
                        <div className="flex items-center gap-2 text-xs text-zinc-500 dark:text-zinc-400">
                            <Clock className={cn("w-3.5 h-3.5", entry.expiresAt ? "text-orange-400" : "text-zinc-300")} />
                            <span>{formatExpiresAt(entry.expiresAt)}</span>
                        </div>

                        {/* Actions */}
                        <div className="flex justify-center opacity-0 group-hover:opacity-100 transition-opacity">
                            <button
                                onClick={() => onRemove(entry.id)}
                                className="p-1.5 rounded-md text-zinc-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-500/10 transition-colors"
                                title="Remove Block"
                            >
                                <Trash2 className="w-4 h-4" />
                            </button>
                        </div>
                    </div>
                ))}
            </motion.div>
        </div>
    );
}
