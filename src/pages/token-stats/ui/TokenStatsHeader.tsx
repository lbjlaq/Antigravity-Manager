// File: src/pages/token-stats/ui/TokenStatsHeader.tsx
// Token Stats page header with time range selector

import { useTranslation } from 'react-i18next';
import { Clock, Calendar, CalendarDays, RefreshCw } from 'lucide-react';
import { cn } from '@/shared/lib';
import { Button } from '@/shared/ui';
import type { TimeRange } from '../lib/constants';
import { TIME_RANGES } from '../lib/constants';

interface TokenStatsHeaderProps {
    timeRange: TimeRange;
    loading: boolean;
    onTimeRangeChange: (range: TimeRange) => void;
    onRefresh: () => void;
}

export function TokenStatsHeader({
    timeRange,
    loading,
    onTimeRangeChange,
    onRefresh,
}: TokenStatsHeaderProps) {
    const { t } = useTranslation();

    const getIcon = (range: TimeRange) => {
        switch (range) {
            case 'hourly': return <Clock className="w-4 h-4" />;
            case 'daily': return <Calendar className="w-4 h-4" />;
            case 'weekly': return <CalendarDays className="w-4 h-4" />;
        }
    };

    return (
        <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
            <div>
                <h1 className="text-3xl font-bold text-zinc-900 dark:text-white tracking-tight">
                    {t('token_stats.title', 'Statistics')}
                </h1>
                <p className="text-zinc-500 text-sm mt-1">
                    {t('token_stats.subtitle', 'Monitor your AI usage and costs in real-time')}
                </p>
            </div>

            <div className="flex items-center gap-3 bg-white dark:bg-zinc-900 p-1.5 rounded-xl shadow-sm border border-zinc-200 dark:border-zinc-800">
                {TIME_RANGES.map((range) => (
                    <button
                        key={range}
                        onClick={() => onTimeRangeChange(range)}
                        className={cn(
                            "px-4 py-2 rounded-lg text-sm font-medium transition-all flex items-center gap-2",
                            timeRange === range
                                ? "bg-zinc-100 dark:bg-zinc-800 text-zinc-900 dark:text-white shadow-sm"
                                : "text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-300"
                        )}
                    >
                        {getIcon(range)}
                        <span className="capitalize">{range}</span>
                    </button>
                ))}
                <div className="w-px h-6 bg-zinc-200 dark:bg-zinc-800 mx-1" />
                <Button
                    size="icon"
                    variant="ghost"
                    onClick={onRefresh}
                    className="h-9 w-9 text-zinc-500 hover:text-zinc-900 dark:hover:text-white"
                    disabled={loading}
                >
                    <RefreshCw className={cn("w-4 h-4", loading && "animate-spin")} />
                </Button>
            </div>
        </div>
    );
}
