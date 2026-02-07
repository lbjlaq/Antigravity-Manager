// File: src/widgets/stats/ui/RequestHealthCards.tsx
// Request health cards component

import React, { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Activity, CheckCircle, AlertCircle, Timer, ArrowDownToLine, ArrowUpFromLine } from 'lucide-react';
import { motion } from 'framer-motion';
import { cn } from '@/shared/lib';

export interface ProxyStats {
    total_requests: number;
    success_count: number;
    error_count: number;
    avg_latency: number;
}

const HealthCard = memo(({ 
    title, 
    value, 
    subValue,
    icon: Icon, 
    colorClass, 
    bgClass, 
    delay 
}: { 
    title: string; 
    value: string | number;
    subValue?: React.ReactNode; 
    icon: any; 
    colorClass: string; 
    bgClass: string; 
    delay: number;
}) => (
    <motion.div
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.3, delay }}
        className={cn(
            "relative overflow-hidden rounded-2xl p-5 shadow-sm border transition-shadow hover:shadow-md",
            bgClass,
            "border-white/10 dark:border-white/5"
        )}
    >
        <div className="flex items-center gap-3 mb-3">
             <div className={cn("p-2 rounded-lg bg-white/50 dark:bg-black/20 backdrop-blur-sm", colorClass)}>
                <Icon className="w-4 h-4" />
             </div>
             <span className={cn("text-xs font-semibold uppercase tracking-wider opacity-70", colorClass)}>
                {title}
             </span>
        </div>
        <div className={cn("text-2xl font-bold tracking-tight", colorClass)}>
            {value}
        </div>
        {subValue && <div className="mt-2">{subValue}</div>}
    </motion.div>
));

export const RequestHealthCards = memo(({ 
    stats, 
    isLoading, 
    fallbackTotalRequests = 0, 
    fallbackTotalTokens = 0,
    fallbackInputTokens = 0,
    fallbackOutputTokens = 0
}: { 
    stats: ProxyStats | null, 
    isLoading: boolean, 
    fallbackTotalRequests?: number, 
    fallbackTotalTokens?: number,
    fallbackInputTokens?: number,
    fallbackOutputTokens?: number
}) => {
    const { t } = useTranslation();

    if (isLoading || !stats) {
        return (
            <div className="grid grid-cols-1 md:grid-cols-4 gap-4 mb-6">
                {[...Array(4)].map((_, i) => (
                    <div key={i} className="h-32 rounded-2xl bg-gray-100 dark:bg-gray-800 animate-pulse" />
                ))}
            </div>
        );
    }

    const displayTotal = Math.max(stats.total_requests, fallbackTotalRequests);
    const hasLogData = stats.total_requests > 0;
    const hasFallbackData = !hasLogData && fallbackTotalRequests > 0;

    let card2 = {
        title: t('stats.success_rate', 'Success Rate'),
        value: hasLogData ? `${((stats.success_count / stats.total_requests) * 100).toFixed(1)}%` : '-',
        subValue: hasLogData ? (
            <div className="w-full bg-emerald-100/30 dark:bg-emerald-900/20 rounded-full h-1.5 overflow-hidden">
                <div className="h-full bg-emerald-500 rounded-full" style={{ width: `${((stats.success_count / stats.total_requests) * 100).toFixed(1)}%` }} />
            </div>
        ) : undefined,
        icon: CheckCircle,
        color: "text-emerald-600 dark:text-emerald-400",
        bg: "bg-emerald-50/50 dark:bg-emerald-900/10"
    };

    let card3 = {
        title: t('stats.error_rate', 'Error Rate'),
        value: hasLogData ? `${((stats.error_count / stats.total_requests) * 100).toFixed(1)}%` : '-',
        subValue: hasLogData ? (
            <div className="w-full bg-red-100/30 dark:bg-red-900/20 rounded-full h-1.5 overflow-hidden">
                <div className="h-full bg-red-500 rounded-full" style={{ width: `${((stats.error_count / stats.total_requests) * 100).toFixed(1)}%` }} />
            </div>
        ) : undefined,
        icon: AlertCircle,
        color: "text-red-600 dark:text-red-400",
        bg: "bg-red-50/50 dark:bg-red-900/10"
    };

    let card4 = {
        title: t('stats.avg_latency', 'Avg Latency'),
        value: hasLogData ? `${stats.avg_latency.toFixed(0)} ms` : '-',
        icon: Timer,
        color: "text-amber-600 dark:text-amber-400",
        bg: "bg-amber-50/50 dark:bg-amber-900/10"
    };

    if (hasFallbackData) {
        card2 = {
            title: t('stats.avg_input', 'Avg Input'),
            value: (fallbackInputTokens / fallbackTotalRequests).toFixed(0),
            subValue: <span className="text-xs opacity-50">tokens/req</span>,
            icon: ArrowDownToLine,
            color: "text-sky-600 dark:text-sky-400",
            bg: "bg-sky-50/50 dark:bg-sky-900/10"
        };

        card3 = {
            title: t('stats.avg_output', 'Avg Output'),
            value: (fallbackOutputTokens / fallbackTotalRequests).toFixed(0),
            subValue: <span className="text-xs opacity-50">tokens/req</span>,
            icon: ArrowUpFromLine,
            color: "text-violet-600 dark:text-violet-400",
            bg: "bg-violet-50/50 dark:bg-violet-900/10"
        };
        
        card4 = {
            title: t('stats.avg_total', 'Avg Total'),
            value: (fallbackTotalTokens / fallbackTotalRequests).toFixed(0),
            icon: Activity,
            color: "text-blue-600 dark:text-blue-400",
            bg: "bg-blue-50/50 dark:bg-blue-900/10"
        };
    }

    return (
        <div className="grid grid-cols-1 md:grid-cols-4 gap-4 mb-8">
            <HealthCard
                title={t('stats.total_requests', 'Total Requests')}
                value={displayTotal.toLocaleString()}
                subValue={hasFallbackData ? <span className="text-xs opacity-50">Estimated from usage</span> : undefined}
                icon={Activity}
                colorClass="text-zinc-600 dark:text-zinc-300"
                bgClass="bg-white dark:bg-zinc-900"
                delay={0}
            />
            
            <HealthCard
                title={card2.title}
                value={card2.value}
                subValue={card2.subValue}
                icon={card2.icon}
                colorClass={card2.color}
                bgClass={card2.bg}
                delay={0.1}
            />

            <HealthCard
                title={card3.title}
                value={card3.value}
                subValue={card3.subValue}
                icon={card3.icon}
                colorClass={card3.color}
                bgClass={card3.bg}
                delay={0.2}
            />

            <HealthCard
                title={card4.title}
                value={card4.value}
                icon={card4.icon}
                colorClass={card4.color}
                bgClass={card4.bg}
                delay={0.3}
            />
        </div>
    );
});
