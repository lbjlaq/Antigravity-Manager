// File: src/widgets/stats/ui/StatsSummary.tsx
// Stats summary cards component

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Zap, TrendingUp, Users, Cpu as CpuIcon } from 'lucide-react';
import { motion } from 'framer-motion';
import { cn } from '@/shared/lib';

interface TokenStatsSummary {
    total_input_tokens: number;
    total_output_tokens: number;
    total_tokens: number;
    total_requests: number;
    unique_accounts: number;
}

interface StatsSummaryProps {
    summary: TokenStatsSummary | null;
    modelCount: number;
    isLoading: boolean;
}

const formatNumber = (num: number): string => {
    if (num >= 1000000) return `${(num / 1000000).toFixed(1)}M`;
    if (num >= 1000) return `${(num / 1000).toFixed(1)}K`;
    return num.toString();
};

const StatCard = memo(({ 
    title, 
    value, 
    icon: Icon, 
    colorClass, 
    bgClass, 
    borderClass, 
    delay 
}: { 
    title: string; 
    value: string | number; 
    icon: any; 
    colorClass: string; 
    bgClass: string; 
    borderClass: string;
    delay: number;
}) => (
    <motion.div
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.3, delay }}
        className={cn(
            "relative overflow-hidden rounded-2xl p-5 shadow-sm border transition-shadow hover:shadow-md",
            bgClass,
            borderClass
        )}
    >
        <div className="flex items-center gap-3 mb-3">
             <div className={cn("p-2 rounded-lg bg-white/50 dark:bg-black/20 backdrop-blur-sm", colorClass)}>
                <Icon className="w-4 h-4" />
             </div>
             <span className={cn("text-xs font-semibold uppercase tracking-wider opacity-70", colorClass.replace('text-', 'text-'))}>
                {title}
             </span>
        </div>
        <div className={cn("text-2xl font-bold tracking-tight", colorClass)}>
            {value}
        </div>
    </motion.div>
));

export const StatsSummary = memo(({ summary, modelCount, isLoading }: StatsSummaryProps) => {
    const { t } = useTranslation();

    if (isLoading || !summary) {
        return (
            <div className="grid grid-cols-2 md:grid-cols-5 gap-4">
                {[...Array(5)].map((_, i) => (
                    <div key={i} className="h-28 rounded-2xl bg-gray-100 dark:bg-gray-800 animate-pulse" />
                ))}
            </div>
        );
    }

    return (
        <div className="grid grid-cols-2 md:grid-cols-5 gap-4">
            <StatCard
                title={t('token_stats.total_tokens', 'Total Tokens')}
                value={formatNumber(summary.total_tokens)}
                icon={Zap}
                colorClass="text-indigo-600 dark:text-indigo-400"
                bgClass="bg-gradient-to-br from-indigo-50/50 to-white dark:from-indigo-900/10 dark:to-gray-800"
                borderClass="border-indigo-100 dark:border-indigo-900/30"
                delay={0}
            />
            <StatCard
                title={t('token_stats.input_tokens', 'Input Tokens')}
                value={formatNumber(summary.total_input_tokens)}
                icon={TrendingUp}
                colorClass="text-blue-600 dark:text-blue-400"
                bgClass="bg-gradient-to-br from-blue-50/50 to-white dark:from-blue-900/10 dark:to-gray-800"
                borderClass="border-blue-100 dark:border-blue-900/30"
                delay={0.05}
            />
            <StatCard
                title={t('token_stats.output_tokens', 'Output Tokens')}
                value={formatNumber(summary.total_output_tokens)}
                icon={TrendingUp}
                colorClass="text-purple-600 dark:text-purple-400"
                bgClass="bg-gradient-to-br from-purple-50/50 to-white dark:from-purple-900/10 dark:to-gray-800"
                borderClass="border-purple-100 dark:border-purple-900/30"
                delay={0.1}
            />
            <StatCard
                title={t('token_stats.accounts_used', 'Active Accounts')}
                value={summary.unique_accounts}
                icon={Users}
                colorClass="text-emerald-600 dark:text-emerald-400"
                bgClass="bg-gradient-to-br from-emerald-50/50 to-white dark:from-emerald-900/10 dark:to-gray-800"
                borderClass="border-emerald-100 dark:border-emerald-900/30"
                delay={0.15}
            />
            <StatCard
                title={t('token_stats.models_used', 'Active Models')}
                value={modelCount}
                icon={CpuIcon}
                colorClass="text-orange-600 dark:text-orange-400"
                bgClass="bg-gradient-to-br from-orange-50/50 to-white dark:from-orange-900/10 dark:to-gray-800"
                borderClass="border-orange-100 dark:border-orange-900/30"
                delay={0.2}
            />
        </div>
    );
});
