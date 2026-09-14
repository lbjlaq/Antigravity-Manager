
import { AlertTriangle, Clock, Lock } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { cn } from '../../utils/cn';
import { getQuotaColor, formatTimeRemaining, getTimeRemainingColor } from '../../utils/format';
import type { LiveLimitStatus } from '../../types/account';
import { formatCompactDuration, getLiveLimitState } from '../../utils/liveLimit';

interface QuotaItemProps {
    label: string;
    percentage: number;
    resetTime?: string;
    isProtected?: boolean;
    liveLimit?: LiveLimitStatus;
    className?: string;
    Icon?: React.ComponentType<{ size?: number; className?: string }>;
}

export function QuotaItem({ label, percentage, resetTime, isProtected, liveLimit, className, Icon }: QuotaItemProps) {
    const { t } = useTranslation();
    const liveState = getLiveLimitState(liveLimit);
    const showLiveIssue = liveState.shouldShow;
    const liveStatus = liveLimit?.status || 'ERR';
    const liveLimitTitle = liveLimit
        ? [
            liveState.isActive
                ? `Live image endpoint is temporarily unavailable for ${formatCompactDuration(liveState.secondsRemaining)}.`
                : `Image endpoint returned ${liveStatus} ${formatCompactDuration(liveState.secondsAgo)} ago.`,
            `Reason: ${liveLimit.reason}.`,
            `Quota snapshot can still show ${percentage}%.`,
            liveLimit.message ? `Message: ${liveLimit.message}` : null,
        ].filter(Boolean).join(' ')
        : label;
    const getBgColorClass = (p: number) => {
        const color = getQuotaColor(p);
        switch (color) {
            case 'success': return 'bg-emerald-500';
            case 'warning': return 'bg-amber-500';
            case 'error': return 'bg-rose-500';
            default: return 'bg-gray-500';
        }
    };

    const getTextColorClass = (p: number) => {
        const color = getQuotaColor(p);
        switch (color) {
            case 'success': return 'text-emerald-600 dark:text-emerald-400';
            case 'warning': return 'text-amber-600 dark:text-amber-400';
            case 'error': return 'text-rose-600 dark:text-rose-400';
            default: return 'text-gray-500';
        }
    };

    const getTimeColorClass = (time?: string) => {
        if (!time) return 'text-gray-300 dark:text-gray-600';
        const color = getTimeRemainingColor(time);
        switch (color) {
            case 'success': return 'text-emerald-600 dark:text-emerald-400';
            case 'warning': return 'text-amber-600 dark:text-amber-400';
            default: return 'text-blue-600 dark:text-blue-400';
        }
    };

    return (
        <div className={cn(
            "relative h-[22px] flex items-center px-1.5 rounded-md overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5 group/quota",
            showLiveIssue && "border-amber-400/70 dark:border-amber-500/70 bg-amber-50/80 dark:bg-amber-950/30 ring-1 ring-amber-400/30",
            liveState.isActive && "border-rose-400/70 dark:border-rose-500/70 bg-rose-50/80 dark:bg-rose-950/30 ring-rose-400/30",
            className
        )}
            title={showLiveIssue ? liveLimitTitle : label}
        >
            {/* Background Progress Bar */}
            <div
                className={cn(
                    "absolute inset-y-0 left-0 transition-all duration-700 ease-out opacity-15 dark:opacity-20",
                    showLiveIssue ? (liveState.isActive ? "bg-rose-500" : "bg-amber-500") : getBgColorClass(percentage)
                )}
                style={{ width: `${percentage}%` }}
            />

            {/* Content */}
            <div className="relative z-10 w-full flex items-center text-[10px] font-mono leading-none gap-1.5">
                {/* Model Name */}
                <span className={cn(
                    "flex-1 min-w-0 text-gray-500 dark:text-gray-400 font-bold truncate text-left flex items-center gap-1",
                    showLiveIssue && "text-amber-700 dark:text-amber-300",
                    liveState.isActive && "text-rose-700 dark:text-rose-300"
                )} title={showLiveIssue ? liveLimitTitle : label}>
                    {showLiveIssue && (
                        <AlertTriangle
                            size={12}
                            className={cn(
                                "shrink-0",
                                liveState.isActive ? "text-rose-500" : "text-amber-500"
                            )}
                        />
                    )}
                    {Icon && <Icon size={12} className="shrink-0" />}
                    {label}
                </span>

                {/* Reset Time */}
                <div className="w-[58px] flex justify-start shrink-0">
                    {resetTime ? (
                        <span className={cn("flex items-center gap-0.5 font-medium transition-colors truncate", getTimeColorClass(resetTime))}>
                            <Clock className="w-2.5 h-2.5 shrink-0" />
                            {formatTimeRemaining(resetTime)}
                        </span>
                    ) : (
                        <span className="text-gray-300 dark:text-gray-600 italic scale-90">N/A</span>
                    )}
                </div>

                {/* Percentage */}
                <span className={cn(
                    "text-right font-bold transition-colors flex items-center justify-end gap-0.5 shrink-0",
                    showLiveIssue ? "w-[58px]" : "w-[28px]",
                    showLiveIssue ? (liveState.isActive ? "text-rose-700 dark:text-rose-300" : "text-amber-700 dark:text-amber-300") : getTextColorClass(percentage)
                )}>
                    {isProtected && (
                        <span title={t('accounts.quota_protected')}><Lock className="w-2.5 h-2.5 text-amber-500" /></span>
                    )}
                    {showLiveIssue && (
                        <span
                            className={cn(
                                "rounded px-1 py-[1px] text-[9px] leading-none",
                                liveState.isActive
                                    ? "bg-rose-500/15 text-rose-700 dark:text-rose-300"
                                    : "bg-amber-500/15 text-amber-700 dark:text-amber-300"
                            )}
                        >
                            {liveStatus}
                        </span>
                    )}
                    {percentage}%
                </span>
            </div>
        </div>
    );
}
