// File: src/pages/security/ui/BlacklistTab.tsx
// Blacklist tab content

import { motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { Shield, Ban, Hash, Clock, Trash2 } from 'lucide-react';
import type { IpBlacklistEntry } from '@/entities/security';

interface BlacklistTabProps {
    blacklist: IpBlacklistEntry[];
    onRemove: (id: number) => void;
    formatExpiresAt: (expiresAt: number | null) => string;
}

export function BlacklistTab({ blacklist, onRemove, formatExpiresAt }: BlacklistTabProps) {
    const { t } = useTranslation();

    if (blacklist.length === 0) {
        return (
            <div className="flex flex-col items-center justify-center py-12 text-gray-400 dark:text-zinc-500">
                <Shield className="w-12 h-12 mb-3 opacity-30" />
                <p className="text-sm">{t('security.no_blacklist', 'No blocked IPs')}</p>
            </div>
        );
    }

    return (
        <motion.div
            key="blacklist"
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -10 }}
            className="space-y-2"
        >
            {blacklist.map((entry) => (
                <div
                    key={entry.id}
                    className="group flex items-center justify-between p-4 rounded-xl bg-gray-50 dark:bg-zinc-800/50 border border-gray-100 dark:border-zinc-700/50 hover:border-red-200 dark:hover:border-red-500/30 transition-all"
                >
                    <div className="flex items-center gap-4">
                        <div className="p-2 rounded-lg bg-red-100 dark:bg-red-500/10">
                            <Ban className="w-4 h-4 text-red-500" />
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
                            {entry.reason && (
                                <p className="text-xs text-gray-500 dark:text-zinc-500 mt-0.5">{entry.reason}</p>
                            )}
                        </div>
                    </div>
                    <div className="flex items-center gap-4">
                        <div className="flex items-center gap-3 text-xs text-gray-400 dark:text-zinc-500">
                            <span className="flex items-center gap-1">
                                <Hash className="w-3 h-3" />
                                {entry.hitCount}
                            </span>
                            <span className="flex items-center gap-1">
                                <Clock className="w-3 h-3" />
                                {formatExpiresAt(entry.expiresAt)}
                            </span>
                        </div>
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
