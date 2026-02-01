// File: src/widgets/dashboard/ui/StatsRow.tsx
// Premium Stats Row - Clean & Informative

import { memo } from 'react';
import { Users, Zap, Bot, AlertTriangle } from "lucide-react";
import { useTranslation } from 'react-i18next';
import { cn } from "@/shared/lib";

interface DashboardStats {
    total: number;
    avgGemini: number;
    avgGeminiImage?: number;
    avgClaude: number;
    lowQuota: number;
}

interface StatsRowProps {
    stats: DashboardStats;
}

export const StatsRow = memo(({ stats }: StatsRowProps) => {
    const { t } = useTranslation();

    return (
        <div className="grid gap-3 grid-cols-2 lg:grid-cols-4">
            {/* Total Accounts */}
            <StatCard
                title={t('dashboard.total_accounts')}
                value={stats.total}
                icon={Users}
                color="indigo"
            />

            {/* Gemini Pro Quota */}
            <StatCard
                title={t('dashboard.avg_gemini')}
                value={stats.avgGemini}
                suffix="%"
                icon={Zap}
                color="emerald"
                showBar
                barColor={stats.avgGemini >= 50 ? 'emerald' : 'amber'}
            />

            {/* Claude Quota */}
            <StatCard
                title={t('dashboard.avg_claude')}
                value={stats.avgClaude}
                suffix="%"
                icon={Bot}
                color="cyan"
                showBar
                barColor={stats.avgClaude >= 50 ? 'cyan' : 'amber'}
            />

            {/* Low Quota Alert */}
            <StatCard
                title={t('dashboard.quota_low')}
                value={stats.lowQuota}
                icon={AlertTriangle}
                color="red"
                alert={stats.lowQuota > 0}
                subtitle={stats.lowQuota > 0 ? t('dashboard.attention_needed') : t('dashboard.all_good', 'All good')}
            />
        </div>
    );
});

// Stat Card Component
interface StatCardProps {
    title: string;
    value: number;
    suffix?: string;
    icon: React.ElementType;
    color: 'indigo' | 'emerald' | 'cyan' | 'red' | 'amber';
    showBar?: boolean;
    barColor?: 'emerald' | 'cyan' | 'amber' | 'red';
    alert?: boolean;
    subtitle?: string;
}

const colorStyles = {
    indigo: {
        bg: 'bg-indigo-500/10',
        border: 'border-indigo-500/20',
        text: 'text-indigo-400',
        icon: 'text-indigo-500',
        bar: 'bg-indigo-500',
    },
    emerald: {
        bg: 'bg-emerald-500/10',
        border: 'border-emerald-500/20',
        text: 'text-emerald-400',
        icon: 'text-emerald-500',
        bar: 'bg-emerald-500',
    },
    cyan: {
        bg: 'bg-cyan-500/10',
        border: 'border-cyan-500/20',
        text: 'text-cyan-400',
        icon: 'text-cyan-500',
        bar: 'bg-cyan-500',
    },
    red: {
        bg: 'bg-red-500/10',
        border: 'border-red-500/20',
        text: 'text-red-400',
        icon: 'text-red-500',
        bar: 'bg-red-500',
    },
    amber: {
        bg: 'bg-amber-500/10',
        border: 'border-amber-500/20',
        text: 'text-amber-400',
        icon: 'text-amber-500',
        bar: 'bg-amber-500',
    },
};

function StatCard({ title, value, suffix = '', icon: Icon, color, showBar, barColor, alert, subtitle }: StatCardProps) {
    const styles = colorStyles[color];
    const barStyles = barColor ? colorStyles[barColor] : styles;

    return (
        <div className={cn(
            "relative p-4 rounded-xl border transition-all duration-200",
            "bg-white dark:bg-zinc-900/80",
            "border-zinc-200 dark:border-zinc-800",
            "hover:border-zinc-300 dark:hover:border-zinc-700",
            alert && "border-red-500/30 dark:border-red-500/30"
        )}>
            {/* Header */}
            <div className="flex items-center justify-between mb-3">
                <span className="text-xs font-medium text-zinc-500 dark:text-zinc-400 uppercase tracking-wide">
                    {title}
                </span>
                <div className={cn("p-1.5 rounded-lg", styles.bg)}>
                    <Icon className={cn("w-3.5 h-3.5", styles.icon, alert && "animate-pulse")} />
                </div>
            </div>

            {/* Value */}
            <div className="flex items-baseline gap-1">
                <span className={cn(
                    "text-2xl font-bold tracking-tight",
                    alert ? "text-red-500" : "text-zinc-900 dark:text-white"
                )}>
                    {value}
                </span>
                {suffix && (
                    <span className={cn("text-lg font-semibold", styles.text)}>
                        {suffix}
                    </span>
                )}
            </div>

            {/* Progress Bar */}
            {showBar && (
                <div className="mt-3 h-1.5 bg-zinc-200 dark:bg-zinc-800 rounded-full overflow-hidden">
                    <div
                        className={cn("h-full rounded-full transition-all duration-500", barStyles.bar)}
                        style={{ width: `${Math.min(value, 100)}%` }}
                    />
                </div>
            )}

            {/* Subtitle */}
            {subtitle && (
                <p className={cn(
                    "mt-2 text-[10px] font-medium uppercase tracking-wide",
                    alert ? "text-red-400" : "text-zinc-400"
                )}>
                    {subtitle}
                </p>
            )}
        </div>
    );
}
