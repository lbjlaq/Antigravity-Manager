// File: src/pages/security/ui/SecurityHeader.tsx
// Security page header with stats

import { useTranslation } from 'react-i18next';
import { Shield, Ban, Hash } from 'lucide-react';
import type { SecurityStats } from '@/types/security';

interface SecurityHeaderProps {
    stats: SecurityStats | null;
}

export function SecurityHeader({ stats }: SecurityHeaderProps) {
    const { t } = useTranslation();

    return (
        <div className="flex items-center justify-between">
            <div className="flex items-center gap-4">
                <div className="p-3 rounded-xl bg-gradient-to-br from-indigo-500 to-purple-600 shadow-lg shadow-indigo-500/25">
                    <Shield className="w-6 h-6 text-white" />
                </div>
                <div>
                    <h1 className="text-xl font-bold text-gray-900 dark:text-white">
                        {t('security.title', 'IP Security')}
                    </h1>
                    <p className="text-sm text-gray-500 dark:text-zinc-500">
                        {t('security.subtitle', 'Manage access control for your proxy')}
                    </p>
                </div>
            </div>

            {stats && (
                <div className="flex items-center gap-3">
                    <div className="flex items-center gap-2 px-3 py-1.5 rounded-full bg-red-50 dark:bg-red-500/10 border border-red-200 dark:border-red-500/20">
                        <Ban className="w-3.5 h-3.5 text-red-500" />
                        <span className="text-sm font-semibold text-red-600 dark:text-red-400">{stats.blockedRequests}</span>
                        <span className="text-xs text-red-500/70">{t('security.blocked', 'blocked')}</span>
                    </div>
                    <div className="flex items-center gap-2 px-3 py-1.5 rounded-full bg-blue-50 dark:bg-blue-500/10 border border-blue-200 dark:border-blue-500/20">
                        <Hash className="w-3.5 h-3.5 text-blue-500" />
                        <span className="text-sm font-semibold text-blue-600 dark:text-blue-400">{stats.uniqueIps}</span>
                        <span className="text-xs text-blue-500/70">{t('security.ips', 'IPs')}</span>
                    </div>
                </div>
            )}
        </div>
    );
}
