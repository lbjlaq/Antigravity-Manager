// File: src/pages/settings/ui/SettingsTabs.tsx
// Settings page tabs - styled like SecurityTabs/AccountsToolbar

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { motion } from 'framer-motion';
import { cn } from '@/shared/lib';
import { SECTIONS, type SectionId } from '../lib';

interface SettingsTabsProps {
    activeTab: SectionId;
    onTabChange: (tab: SectionId) => void;
}

export const SettingsTabs = memo(function SettingsTabs({ activeTab, onTabChange }: SettingsTabsProps) {
    const { t } = useTranslation();

    return (
        <div className="flex-none flex items-center gap-2 px-5 py-3 border-b border-zinc-200 dark:border-zinc-800 bg-zinc-50 dark:bg-zinc-900/50 overflow-x-auto">
            {/* Tabs */}
            <div className="flex items-center bg-zinc-100 dark:bg-zinc-800 p-0.5 rounded-lg border border-zinc-200 dark:border-zinc-700">
                {SECTIONS.map((section) => {
                    const Icon = section.icon;
                    return (
                        <button
                            key={section.id}
                            onClick={() => onTabChange(section.id)}
                            className={cn(
                                "relative px-3 py-1.5 rounded-md text-xs font-medium transition-all z-10 flex items-center gap-1.5 whitespace-nowrap",
                                activeTab === section.id 
                                    ? "text-zinc-900 dark:text-white" 
                                    : "text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-300"
                            )}
                        >
                            {activeTab === section.id && (
                                <motion.div
                                    layoutId="activeSettingsTab"
                                    className="absolute inset-0 bg-white dark:bg-zinc-700 rounded-md shadow-sm"
                                    transition={{ type: "spring", bounce: 0.2, duration: 0.6 }}
                                    style={{ zIndex: -1 }}
                                />
                            )}
                            <Icon className="w-3.5 h-3.5" />
                            <span className="hidden sm:inline">{t(section.label)}</span>
                        </button>
                    );
                })}
            </div>

            {/* Spacer */}
            <div className="flex-1" />
        </div>
    );
});
