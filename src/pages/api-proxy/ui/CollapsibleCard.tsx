// File: src/pages/api-proxy/ui/CollapsibleCard.tsx
// Reusable collapsible card component

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { cn } from '@/shared/lib';

interface CollapsibleCardProps {
    title: string;
    icon: React.ReactNode;
    enabled?: boolean;
    onToggle?: (enabled: boolean) => void;
    children: React.ReactNode;
    defaultExpanded?: boolean;
    rightElement?: React.ReactNode;
    allowInteractionWhenDisabled?: boolean;
}

export function CollapsibleCard({
    title,
    icon,
    enabled,
    onToggle,
    children,
    defaultExpanded = false,
    rightElement,
    allowInteractionWhenDisabled = false,
}: CollapsibleCardProps) {
    const [isExpanded, setIsExpanded] = useState(defaultExpanded);
    const { t } = useTranslation();

    return (
        <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-gray-700/50 overflow-hidden transition-all duration-200 hover:shadow-md">
            <div
                className="px-5 py-4 flex items-center justify-between cursor-pointer bg-gray-50/50 dark:bg-gray-800/50 hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors"
                onClick={(e) => {
                    if ((e.target as HTMLElement).closest('.no-expand')) return;
                    setIsExpanded(!isExpanded);
                }}
            >
                <div className="flex items-center gap-3">
                    <div className="text-gray-500 dark:text-gray-400">
                        {icon}
                    </div>
                    <span className="font-medium text-sm text-gray-900 dark:text-gray-100">
                        {title}
                    </span>
                    {enabled !== undefined && (
                        <div className={cn('text-xs px-2 py-0.5 rounded-full', enabled ? 'bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-400' : 'bg-gray-100 text-gray-500 dark:bg-gray-600/50 dark:text-gray-300')}>
                            {enabled ? t('common.enabled') : t('common.disabled')}
                        </div>
                    )}
                </div>

                <div className="flex items-center gap-4 no-expand">
                    {rightElement}

                    {enabled !== undefined && onToggle && (
                        <div className="flex items-center" onClick={(e) => e.stopPropagation()}>
                            <input
                                type="checkbox"
                                className="toggle toggle-sm bg-gray-200 dark:bg-gray-700 border-gray-300 dark:border-gray-600 checked:bg-blue-500 checked:border-blue-500"
                                checked={enabled}
                                onChange={(e) => onToggle(e.target.checked)}
                            />
                        </div>
                    )}

                    <button
                        className={cn('p-1 rounded-lg hover:bg-gray-200 dark:hover:bg-gray-700 transition-all duration-200', isExpanded ? 'rotate-180' : '')}
                    >
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                            <path d="m6 9 6 6 6-6" />
                        </svg>
                    </button>
                </div>
            </div>

            <div
                className={`transition-all duration-300 ease-in-out border-t border-gray-100 dark:border-base-200 ${isExpanded ? 'max-h-[2000px] opacity-100' : 'max-h-0 opacity-0 overflow-hidden'}`}
            >
                <div className="p-5 relative">
                    {enabled === false && !allowInteractionWhenDisabled && (
                        <div className="absolute inset-0 bg-gray-100/40 dark:bg-black/30 z-10 cursor-not-allowed" />
                    )}
                    <div className={enabled === false && !allowInteractionWhenDisabled ? 'opacity-60 pointer-events-none select-none' : ''}>
                        {children}
                    </div>
                </div>
            </div>
        </div>
    );
}
