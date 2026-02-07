// File: src/pages/security/ui/WhitelistTab.tsx
// Whitelist table with headers

import { motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { ShieldCheck, CheckCircle, Clock, Trash2 } from 'lucide-react';
import type { IpWhitelistEntry } from '@/entities/security';

interface WhitelistTabProps {
    whitelist: IpWhitelistEntry[];
    onRemove: (id: number) => void;
    formatTimestamp: (timestamp: number) => string;
}

export function WhitelistTab({ whitelist, onRemove, formatTimestamp }: WhitelistTabProps) {
    const { t } = useTranslation();

    if (whitelist.length === 0) {
        return (
            <div className="flex flex-col items-center justify-center py-16 text-zinc-400 dark:text-zinc-500 bg-zinc-50/50 dark:bg-zinc-900/10 rounded-xl border border-dashed border-zinc-200 dark:border-zinc-800">
                <ShieldCheck className="w-12 h-12 mb-3 opacity-20" />
                <p className="text-sm font-medium">{t('security.no_whitelist', 'No whitelisted IPs')}</p>
                <p className="text-xs opacity-70 mt-1">Add an IP to bypass checks or allow access in Strict Mode.</p>
            </div>
        );
    }

    return (
        <div className="w-full overflow-hidden">
             {/* Header */}
             <div className="grid grid-cols-[1fr_2fr_150px_60px] gap-4 px-4 py-2 text-[10px] font-semibold text-zinc-500 dark:text-zinc-400 uppercase tracking-wide bg-zinc-50 dark:bg-zinc-800/50 border-b border-zinc-200 dark:border-zinc-800 rounded-t-lg">
                <div>IP Address</div>
                <div>Description</div>
                <div>Added</div>
                <div className="text-center">Action</div>
            </div>

            <motion.div
                key="whitelist"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="space-y-px bg-zinc-200 dark:bg-zinc-800"
            >
                {whitelist.map((entry) => (
                    <div
                        key={entry.id}
                        className="group grid grid-cols-[1fr_2fr_150px_60px] gap-4 items-center px-4 py-3 bg-white dark:bg-zinc-900 hover:bg-zinc-50 dark:hover:bg-zinc-800/80 transition-colors"
                    >
                         {/* IP */}
                         <div className="flex items-center gap-3 min-w-0">
                            <div className="p-1.5 rounded-md bg-emerald-100 dark:bg-emerald-500/10 shrink-0">
                                <CheckCircle className="w-3.5 h-3.5 text-emerald-500" />
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

                        {/* Description */}
                        <div className="text-xs text-zinc-500 dark:text-zinc-400 truncate pr-4">
                            {entry.description || <span className="opacity-30 italic">No description</span>}
                        </div>

                        {/* Added Time */}
                        <div className="flex items-center gap-2 text-xs text-zinc-500 dark:text-zinc-400">
                            <Clock className="w-3.5 h-3.5 text-zinc-300" />
                            <span>{formatTimestamp(entry.createdAt)}</span>
                        </div>

                        {/* Actions */}
                        <div className="flex justify-center opacity-0 group-hover:opacity-100 transition-opacity">
                            <button
                                onClick={() => onRemove(entry.id)}
                                className="p-1.5 rounded-md text-zinc-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-500/10 transition-colors"
                                title="Remove IP"
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
