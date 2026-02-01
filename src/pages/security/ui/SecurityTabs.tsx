// File: src/pages/security/ui/SecurityTabs.tsx
// Security page tabs - styled like AccountsToolbar filter tabs

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Ban, CheckCircle, FileText, Settings } from 'lucide-react';
import { motion } from 'framer-motion';
import { cn } from '@/shared/lib';
import type { SecurityStats } from '@/entities/security';
import type { SecurityTab } from '../lib/constants';

interface SecurityTabsProps {
    activeTab: SecurityTab;
    stats: SecurityStats | null;
    onTabChange: (tab: SecurityTab) => void;
}

export const SecurityTabs = memo(function SecurityTabs({ activeTab, stats, onTabChange }: SecurityTabsProps) {
    const { t } = useTranslation();

    const tabs: { id: SecurityTab; label: string; icon: React.ElementType; count?: number }[] = [
        {
            id: 'blacklist',
            label: t('security.blacklist', 'Blacklist'),
            icon: Ban,
            count: stats?.blacklistCount,
        },
        {
            id: 'whitelist',
            label: t('security.whitelist', 'Whitelist'),
            icon: CheckCircle,
            count: stats?.whitelistCount,
        },
        {
            id: 'logs',
            label: t('security.access_logs', 'Logs'),
            icon: FileText,
        },
        {
            id: 'settings',
            label: t('security.settings', 'Settings'),
            icon: Settings,
        },
    ];

    return (
        <div className="flex-none flex items-center gap-2 px-5 py-3 border-b border-zinc-200 dark:border-zinc-800 bg-zinc-50 dark:bg-zinc-900/50">
            {/* Tabs */}
            <div className="flex items-center bg-zinc-100 dark:bg-zinc-800 p-0.5 rounded-lg border border-zinc-200 dark:border-zinc-700">
                {tabs.map((tab) => {
                    const Icon = tab.icon;
                    return (
                        <button
                            key={tab.id}
                            onClick={() => onTabChange(tab.id)}
                            className={cn(
                                "relative px-3 py-1.5 rounded-md text-xs font-medium transition-all z-10 flex items-center gap-1.5",
                                activeTab === tab.id 
                                    ? "text-zinc-900 dark:text-white" 
                                    : "text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-300"
                            )}
                        >
                            {activeTab === tab.id && (
                                <motion.div
                                    layoutId="activeSecurityTab"
                                    className="absolute inset-0 bg-white dark:bg-zinc-700 rounded-md shadow-sm"
                                    transition={{ type: "spring", bounce: 0.2, duration: 0.6 }}
                                    style={{ zIndex: -1 }}
                                />
                            )}
                            <Icon className="w-3.5 h-3.5" />
                            <span className="hidden sm:inline">{tab.label}</span>
                            {tab.count !== undefined && (
                                <span className={cn(
                                    "px-1 py-0.5 rounded text-[8px] font-bold",
                                    activeTab === tab.id 
                                        ? "bg-zinc-100 dark:bg-zinc-600 text-zinc-700 dark:text-white" 
                                        : "bg-zinc-200 dark:bg-zinc-700 text-zinc-500 dark:text-zinc-500"
                                )}>
                                    {tab.count ?? 0}
                                </span>
                            )}
                        </button>
                    );
                })}
            </div>

            {/* Spacer */}
            <div className="flex-1" />
        </div>
    );
});
