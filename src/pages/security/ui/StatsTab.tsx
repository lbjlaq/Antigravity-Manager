// File: src/pages/security/ui/StatsTab.tsx
// Security statistics dashboard with IP token usage analytics

import { memo, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { 
  Activity, 
  Shield, 
  ShieldOff, 
  Users, 
  Ban, 
  CheckCircle,
  TrendingUp,
  Zap,
  RefreshCw,
  AlertTriangle,
} from 'lucide-react';
import { cn } from '@/shared/lib';
import { useSecurityStats, useIpTokenStats } from '@/features/security/api';
import type { IpTokenStats } from '@/entities/security';
import { Skeleton } from '@/shared/ui';
import { formatCompactNumber } from '@/shared/lib/format';

type TimeRange = 1 | 24 | 168 | 720; // 1h, 24h, 7d, 30d

const TIME_RANGES: { value: TimeRange; label: string }[] = [
  { value: 1, label: '1h' },
  { value: 24, label: '24h' },
  { value: 168, label: '7d' },
  { value: 720, label: '30d' },
];

interface StatCardProps {
  title: string;
  value: number | string;
  icon: React.ReactNode;
  color: 'emerald' | 'rose' | 'amber' | 'sky' | 'violet';
  subtitle?: string;
  trend?: number;
}

const StatCard = memo(function StatCard({ 
  title, 
  value, 
  icon, 
  color,
  subtitle,
  trend,
}: StatCardProps) {
  const colorClasses = {
    emerald: 'from-emerald-500/20 to-emerald-600/5 border-emerald-500/30 text-emerald-400',
    rose: 'from-rose-500/20 to-rose-600/5 border-rose-500/30 text-rose-400',
    amber: 'from-amber-500/20 to-amber-600/5 border-amber-500/30 text-amber-400',
    sky: 'from-sky-500/20 to-sky-600/5 border-sky-500/30 text-sky-400',
    violet: 'from-violet-500/20 to-violet-600/5 border-violet-500/30 text-violet-400',
  };

  const iconBgClasses = {
    emerald: 'bg-emerald-500/20 text-emerald-400',
    rose: 'bg-rose-500/20 text-rose-400',
    amber: 'bg-amber-500/20 text-amber-400',
    sky: 'bg-sky-500/20 text-sky-400',
    violet: 'bg-violet-500/20 text-violet-400',
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      className={cn(
        "relative overflow-hidden rounded-xl border p-4",
        "bg-gradient-to-br backdrop-blur-sm",
        colorClasses[color]
      )}
    >
      {/* Background glow */}
      <div className={cn(
        "absolute -top-10 -right-10 w-32 h-32 rounded-full blur-3xl opacity-20",
        color === 'emerald' && 'bg-emerald-500',
        color === 'rose' && 'bg-rose-500',
        color === 'amber' && 'bg-amber-500',
        color === 'sky' && 'bg-sky-500',
        color === 'violet' && 'bg-violet-500',
      )} />
      
      <div className="relative flex items-start justify-between">
        <div className="space-y-1">
          <p className="text-xs font-medium text-zinc-400 uppercase tracking-wider">
            {title}
          </p>
          <p className="text-2xl font-bold text-white tabular-nums">
            {typeof value === 'number' ? formatCompactNumber(value) : value}
          </p>
          {subtitle && (
            <p className="text-xs text-zinc-500">{subtitle}</p>
          )}
          {trend !== undefined && trend !== 0 && (
            <div className={cn(
              "flex items-center gap-1 text-xs font-medium",
              trend > 0 ? "text-emerald-400" : "text-rose-400"
            )}>
              <TrendingUp className={cn("w-3 h-3", trend < 0 && "rotate-180")} />
              <span>{trend > 0 ? '+' : ''}{trend}%</span>
            </div>
          )}
        </div>
        <div className={cn(
          "p-2.5 rounded-lg",
          iconBgClasses[color]
        )}>
          {icon}
        </div>
      </div>
    </motion.div>
  );
});

const IpTokenTable = memo(function IpTokenTable({ 
  hours 
}: { 
  hours: number 
}) {
  const { t } = useTranslation();
  const { data: ipStats, isLoading, isError, refetch } = useIpTokenStats(hours);

  if (isLoading) {
    return (
      <div className="space-y-2">
        {[...Array(5)].map((_, i) => (
          <Skeleton key={i} className="h-12 w-full" />
        ))}
      </div>
    );
  }

  if (isError) {
    return (
      <div className="flex flex-col items-center justify-center py-12 text-zinc-500">
        <AlertTriangle className="w-8 h-8 mb-2 text-amber-500" />
        <p>{t('security.stats.loadError', 'Failed to load IP statistics')}</p>
        <button 
          onClick={() => refetch()}
          className="mt-2 text-sm text-sky-400 hover:text-sky-300 flex items-center gap-1"
        >
          <RefreshCw className="w-3 h-3" />
          {t('common.retry', 'Retry')}
        </button>
      </div>
    );
  }

  if (!ipStats || ipStats.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-12 text-zinc-500">
        <Activity className="w-8 h-8 mb-2 opacity-50" />
        <p>{t('security.stats.noData', 'No IP activity in this time range')}</p>
      </div>
    );
  }

  // Calculate max for progress bars
  const maxTokens = Math.max(...ipStats.map((s: IpTokenStats) => s.totalTokens));

  return (
    <div className="overflow-hidden rounded-lg border border-white/10">
      {/* Header */}
      <div className="grid grid-cols-12 gap-2 px-4 py-2 bg-zinc-800/50 text-xs font-medium text-zinc-400 uppercase tracking-wider">
        <div className="col-span-4">{t('security.stats.ip', 'IP Address')}</div>
        <div className="col-span-2 text-right">{t('security.stats.requests', 'Requests')}</div>
        <div className="col-span-2 text-right">{t('security.stats.input', 'Input')}</div>
        <div className="col-span-2 text-right">{t('security.stats.output', 'Output')}</div>
        <div className="col-span-2 text-right">{t('security.stats.total', 'Total')}</div>
      </div>
      
      {/* Rows */}
      <div className="divide-y divide-white/5">
        <AnimatePresence mode="popLayout">
          {ipStats.slice(0, 15).map((stat: IpTokenStats, index: number) => {
            const progressWidth = maxTokens > 0 ? (stat.totalTokens / maxTokens) * 100 : 0;
            
            return (
              <motion.div
                key={stat.clientIp}
                initial={{ opacity: 0, x: -10 }}
                animate={{ opacity: 1, x: 0 }}
                exit={{ opacity: 0, x: 10 }}
                transition={{ delay: index * 0.03 }}
                className="relative grid grid-cols-12 gap-2 px-4 py-3 text-sm hover:bg-white/5 transition-colors"
              >
                {/* Progress bar background */}
                <div 
                  className="absolute inset-0 bg-gradient-to-r from-sky-500/10 to-transparent"
                  style={{ width: `${progressWidth}%` }}
                />
                
                {/* Content */}
                <div className="relative col-span-4 font-mono text-zinc-300 truncate">
                  {stat.clientIp}
                </div>
                <div className="relative col-span-2 text-right font-mono text-zinc-400">
                  {formatCompactNumber(stat.requestCount)}
                </div>
                <div className="relative col-span-2 text-right font-mono text-emerald-400">
                  {formatCompactNumber(stat.inputTokens)}
                </div>
                <div className="relative col-span-2 text-right font-mono text-amber-400">
                  {formatCompactNumber(stat.outputTokens)}
                </div>
                <div className="relative col-span-2 text-right font-mono font-medium text-white">
                  {formatCompactNumber(stat.totalTokens)}
                </div>
              </motion.div>
            );
          })}
        </AnimatePresence>
      </div>
      
      {ipStats.length > 15 && (
        <div className="px-4 py-2 text-xs text-center text-zinc-500 bg-zinc-800/30">
          {t('security.stats.andMore', 'and {{count}} more IPs...', { count: ipStats.length - 15 })}
        </div>
      )}
    </div>
  );
});

export const StatsTab = memo(function StatsTab() {
  const { t } = useTranslation();
  const [timeRange, setTimeRange] = useState<TimeRange>(24);
  const { data: stats, isLoading: statsLoading, refetch } = useSecurityStats();

  return (
    <div className="space-y-6">
      {/* Time Range Selector */}
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium text-zinc-400">
          {t('security.stats.overview', 'Security Overview')}
        </h3>
        
        <div className="flex items-center gap-2">
          <button
            onClick={() => refetch()}
            className="p-1.5 rounded-lg text-zinc-500 hover:text-zinc-300 hover:bg-white/5 transition-colors"
            title={t('common.refresh', 'Refresh')}
          >
            <RefreshCw className="w-4 h-4" />
          </button>
          
          <div className="flex rounded-lg border border-white/10 overflow-hidden">
            {TIME_RANGES.map((range) => (
              <button
                key={range.value}
                onClick={() => setTimeRange(range.value)}
                className={cn(
                  "px-3 py-1.5 text-xs font-medium transition-all",
                  timeRange === range.value
                    ? "bg-sky-500/20 text-sky-400"
                    : "text-zinc-500 hover:text-zinc-300 hover:bg-white/5"
                )}
              >
                {range.label}
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* Stats Cards Grid */}
      <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-4">
        {statsLoading ? (
          [...Array(5)].map((_, i) => (
            <Skeleton key={i} className="h-28" />
          ))
        ) : (
          <>
            <StatCard
              title={t('security.stats.totalRequests', 'Total Requests')}
              value={stats?.totalRequests ?? 0}
              icon={<Activity className="w-5 h-5" />}
              color="sky"
            />
            <StatCard
              title={t('security.stats.uniqueIps', 'Unique IPs')}
              value={stats?.uniqueIps ?? 0}
              icon={<Users className="w-5 h-5" />}
              color="violet"
            />
            <StatCard
              title={t('security.stats.blocked', 'Blocked')}
              value={stats?.blockedRequests ?? 0}
              icon={<ShieldOff className="w-5 h-5" />}
              color="rose"
            />
            <StatCard
              title={t('security.stats.blacklisted', 'Blacklisted')}
              value={stats?.blacklistCount ?? 0}
              icon={<Ban className="w-5 h-5" />}
              color="amber"
            />
            <StatCard
              title={t('security.stats.whitelisted', 'Whitelisted')}
              value={stats?.whitelistCount ?? 0}
              icon={<CheckCircle className="w-5 h-5" />}
              color="emerald"
            />
          </>
        )}
      </div>

      {/* IP Token Usage Table */}
      <div className="space-y-3">
        <div className="flex items-center gap-2">
          <Zap className="w-4 h-4 text-amber-400" />
          <h3 className="text-sm font-medium text-zinc-300">
            {t('security.stats.topIpsByTokens', 'Top IPs by Token Usage')}
          </h3>
          <span className="text-xs text-zinc-500">
            ({t('security.stats.last', 'Last')} {timeRange}h)
          </span>
        </div>
        
        <IpTokenTable hours={timeRange} />
      </div>

      {/* Top Blocked IPs */}
      {stats?.topBlockedIps && stats.topBlockedIps.length > 0 && (
        <div className="space-y-3">
          <div className="flex items-center gap-2">
            <Shield className="w-4 h-4 text-rose-400" />
            <h3 className="text-sm font-medium text-zinc-300">
              {t('security.stats.topBlockedIps', 'Most Blocked IPs')}
            </h3>
          </div>
          
          <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-2">
            {stats.topBlockedIps.slice(0, 10).map(([ip, count]: [string, number], index: number) => (
              <motion.div
                key={ip}
                initial={{ opacity: 0, scale: 0.95 }}
                animate={{ opacity: 1, scale: 1 }}
                transition={{ delay: index * 0.05 }}
                className="flex items-center justify-between px-3 py-2 rounded-lg bg-rose-500/10 border border-rose-500/20"
              >
                <span className="text-xs font-mono text-zinc-300 truncate">
                  {ip}
                </span>
                <span className="text-xs font-bold text-rose-400 ml-2">
                  {count}
                </span>
              </motion.div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
});
