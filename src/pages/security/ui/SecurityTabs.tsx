// File: src/pages/security/ui/SecurityTabs.tsx
// Security page tabs

import { useTranslation } from 'react-i18next';
import { Ban, CheckCircle, FileText, Settings } from 'lucide-react';
import { cn } from '@/shared/lib';
import type { SecurityStats } from '@/types/security';
import type { SecurityTab } from '../lib/constants';

interface SecurityTabsProps {
    activeTab: SecurityTab;
    stats: SecurityStats | null;
    onTabChange: (tab: SecurityTab) => void;
}

export function SecurityTabs({ activeTab, stats, onTabChange }: SecurityTabsProps) {
    const { t } = useTranslation();

    const tabs: { id: SecurityTab; label: string; icon: React.ReactNode; count?: number }[] = [
        {
            id: 'blacklist',
            label: t('security.blacklist', 'Blacklist'),
            icon: <Ban className="w-4 h-4" />,
            count: stats?.blacklistCount,
        },
        {
            id: 'whitelist',
            label: t('security.whitelist', 'Whitelist'),
            icon: <CheckCircle className="w-4 h-4" />,
            count: stats?.whitelistCount,
        },
        {
            id: 'logs',
            label: t('security.access_logs', 'Logs'),
            icon: <FileText className="w-4 h-4" />,
        },
        {
            id: 'settings',
            label: t('security.settings', 'Settings'),
            icon: <Settings className="w-4 h-4" />,
        },
    ];

    return (
        <div className="flex items-center gap-1 mt-5 p-1 bg-gray-100 dark:bg-zinc-800 rounded-xl">
            {tabs.map((tab) => (
                <button
                    key={tab.id}
                    onClick={() => onTabChange(tab.id)}
                    className={cn(
                        'flex-1 flex items-center justify-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-all',
                        activeTab === tab.id
                            ? 'bg-white dark:bg-zinc-700 text-gray-900 dark:text-white shadow-sm'
                            : 'text-gray-500 dark:text-zinc-400 hover:text-gray-700 dark:hover:text-zinc-300'
                    )}
                >
                    {tab.icon}
                    {tab.label}
                    {tab.count !== undefined && tab.count > 0 && (
                        <span className={cn(
                            'px-1.5 py-0.5 text-[10px] font-bold rounded-full',
                            activeTab === tab.id
                                ? 'bg-indigo-100 dark:bg-indigo-500/20 text-indigo-600 dark:text-indigo-400'
                                : 'bg-gray-200 dark:bg-zinc-600 text-gray-600 dark:text-zinc-300'
                        )}>
                            {tab.count}
                        </span>
                    )}
                </button>
            ))}
        </div>
    );
}
