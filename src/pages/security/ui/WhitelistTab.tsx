// File: src/pages/security/ui/WhitelistTab.tsx
// Whitelist tab content

import { motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { ShieldCheck, CheckCircle, Trash2 } from 'lucide-react';
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
            <div className="flex flex-col items-center justify-center py-12 text-gray-400 dark:text-zinc-500">
                <ShieldCheck className="w-12 h-12 mb-3 opacity-30" />
                <p className="text-sm">{t('security.no_whitelist', 'No whitelisted IPs')}</p>
            </div>
        );
    }

    return (
        <motion.div
            key="whitelist"
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -10 }}
            className="space-y-2"
        >
            {whitelist.map((entry) => (
                <div
                    key={entry.id}
                    className="group flex items-center justify-between p-4 rounded-xl bg-gray-50 dark:bg-zinc-800/50 border border-gray-100 dark:border-zinc-700/50 hover:border-emerald-200 dark:hover:border-emerald-500/30 transition-all"
                >
                    <div className="flex items-center gap-4">
                        <div className="p-2 rounded-lg bg-emerald-100 dark:bg-emerald-500/10">
                            <CheckCircle className="w-4 h-4 text-emerald-500" />
                        </div>
                        <div>
                            <div className="flex items-center gap-2">
                                <span className="font-mono text-sm font-semibold text-gray-900 dark:text-white">
                                    {entry.ipPattern}
                                </span>
                                {entry.ipPattern.includes('/') && (
                                    <span className="px-1.5 py-0.5 text-[10px] font-bold rounded bg-blue-100 dark:bg-blue-500/20 text-blue-600 dark:text-blue-400">
                                        CIDR
                                    </span>
                                )}
                            </div>
                            {entry.description && (
                                <p className="text-xs text-gray-500 dark:text-zinc-500 mt-0.5">{entry.description}</p>
                            )}
                        </div>
                    </div>
                    <div className="flex items-center gap-4">
                        <span className="text-xs text-gray-400 dark:text-zinc-500">
                            {formatTimestamp(entry.createdAt)}
                        </span>
                        <button
                            onClick={() => onRemove(entry.id)}
                            className="p-2 rounded-lg text-gray-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-500/10 opacity-0 group-hover:opacity-100 transition-all"
                        >
                            <Trash2 className="w-4 h-4" />
                        </button>
                    </div>
                </div>
            ))}
        </motion.div>
    );
}
