// File: src/pages/security/ui/SecurityHeader.tsx
// Security page header - styled like AccountsHeader

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Shield, Ban, Hash } from 'lucide-react';
import type { SecurityStats } from '@/entities/security';

interface SecurityHeaderProps {
    stats: SecurityStats | null;
}

export const SecurityHeader = memo(function SecurityHeader({ stats }: SecurityHeaderProps) {
    const { t } = useTranslation();

    return (
        <div className="flex-none flex items-center justify-between px-5 py-4 border-b border-zinc-200 dark:border-zinc-800">
            <div className="flex items-center gap-3">
                <div className="p-2 rounded-lg bg-zinc-100 dark:bg-zinc-800">
                    <Shield className="w-5 h-5 text-zinc-600 dark:text-zinc-400" />
                </div>
                <div>
                    <h1 className="text-lg font-semibold text-zinc-900 dark:text-white">
                        {t('security.title', 'IP Security')}
                    </h1>
                    <p className="text-xs text-zinc-500 dark:text-zinc-500">
                        {t('security.subtitle', 'Manage access control for your proxy')}
                    </p>
                </div>
            </div>

            {stats && (
                <div className="flex items-center gap-2">
                    <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-red-50 dark:bg-red-500/10 border border-red-200 dark:border-red-500/20">
                        <Ban className="w-3.5 h-3.5 text-red-500" />
                        <span className="text-xs font-semibold text-red-600 dark:text-red-400">{stats.blockedRequests}</span>
                        <span className="text-[10px] text-red-500/70">{t('security.blocked', 'blocked')}</span>
                    </div>
                    <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-blue-50 dark:bg-blue-500/10 border border-blue-200 dark:border-blue-500/20">
                        <Hash className="w-3.5 h-3.5 text-blue-500" />
                        <span className="text-xs font-semibold text-blue-600 dark:text-blue-400">{stats.uniqueIps}</span>
                        <span className="text-[10px] text-blue-500/70">{t('security.ips', 'IPs')}</span>
                    </div>
                </div>
            )}
        </div>
    );
});
