import { Clock, Lock, Sparkles } from 'lucide-react';
import { Gemini } from '@lobehub/icons';
import { useTranslation } from 'react-i18next';
import { cn } from '../../utils/cn';
import { getQuotaColor, formatTimeRemaining, getTimeRemainingColor } from '../../utils/format';
import type { GroupQuotaSummary } from '../../utils/quotaDisplay';
import type { LiveLimitStatus } from '../../types/account';
import { getLiveLimitState } from '../../utils/liveLimit';

interface GroupQuotaCardProps {
    summary: GroupQuotaSummary;
    liveLimit?: LiveLimitStatus;
    isProtected?: boolean;
    compact?: boolean;
    fullWidth?: boolean;
}

export function GroupQuotaCard({ summary, liveLimit, isProtected, fullWidth }: GroupQuotaCardProps) {
    const { t } = useTranslation();
    const liveState = getLiveLimitState(liveLimit);
    const isUnavailable = liveState.isActive || summary.isWeeklyExhausted;

    const fiveHour = summary.fiveHour;
    const weekly = summary.weekly;
    const fiveHourPercentage = fiveHour?.percentage ?? 0;
    const weeklyPercentage = weekly?.percentage ?? 0;

    const label = summary.dynamicLabel || (
        summary.key === 'gemini'
            ? 'Gemini'
            : summary.key === 'argon'
                ? (fullWidth ? 'Gemini 4 Neon' : 'G4 Neon')
                : 'Claude / GPT'
    );
    const isMultiBucket = fiveHour !== undefined && weekly !== undefined;
    // 当上方已有 5H 展示完整 label 时，下方周配额条采用紧凑前缀，彻底避免横向截断
    const weeklyLabel = isMultiBucket
        ? (summary.key === 'gemini' ? 'Gemini' : summary.key === 'claude' ? 'Claude' : label)
        : label;

    const formatTooltip = (windowType: '5h' | 'weekly') => {
        const windowLabel = windowType === '5h' ? '5H' : t('accounts.quota_window_weekly_short', '周配额');
        const titleLine = `${label} (${windowLabel})`;
        const currentPercentage = windowType === '5h' ? fiveHourPercentage : weeklyPercentage;
        const pctLine = t('accounts.remaining_quota', '剩余配额: {{percentage}}%', { percentage: currentPercentage });
        const resetTime = windowType === '5h' ? fiveHour?.resetTime : weekly?.resetTime;
        const timeLine = resetTime
            ? t('accounts.reset_in', '重置: {{time}}', { time: formatTimeRemaining(resetTime) })
            : '';
        const models = summary.supportedModels && summary.supportedModels.length > 0
            ? `\n${t('accounts.supported_official_models', '支持官方模型 ({{count}}个):', { count: summary.supportedModels.length })}\n• ${summary.supportedModels.join('\n• ')}`
            : '';
        return [titleLine, pctLine, timeLine].filter(Boolean).join(' | ') + models;
    };
    const Icon = summary.key === 'gemini' || summary.key === 'argon' ? Gemini.Color : Sparkles;

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
        <div className="flex flex-col gap-1 min-w-0">
            {/* 5H 滑动配额胶囊条 (仅当 5H 存在时渲染) */}
            {fiveHour !== undefined && (
                <div
                    className={cn(
                        "relative h-[22px] flex items-center px-1.5 rounded-md overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5 group/quota transition-all",
                        isUnavailable && "border-rose-400/70 dark:border-rose-500/70 bg-rose-50/80 dark:bg-rose-950/30 ring-rose-400/30"
                    )}
                    title={summary.isWeeklyExhausted ? t('accounts.weekly_exhausted_tooltip') : formatTooltip('5h')}
                >
                    {/* Bottom Accent Progress Line */}
                    <div className="absolute bottom-0 inset-x-0 h-[2.5px] bg-black/[0.05] dark:bg-white/[0.08] overflow-hidden">
                        <div
                            className={cn(
                                "h-full transition-all duration-700 ease-out rounded-r-full",
                                isUnavailable ? "bg-rose-500" : getBgColorClass(fiveHourPercentage)
                            )}
                            style={{ width: `${fiveHourPercentage}%` }}
                        />
                    </div>

                    {/* Content */}
                    <div className="relative z-10 w-full flex items-center text-[10px] font-mono leading-none gap-1.5">
                        {/* Title + Icon */}
                        <span className={cn(
                            "flex-1 min-w-0 text-gray-700 dark:text-gray-200 font-bold truncate text-left flex items-center gap-1",
                            isUnavailable && "text-rose-700 dark:text-rose-300"
                        )}>
                            <Icon size={12} className="shrink-0" />
                            <span className="truncate">{label}</span>
                            <span className="text-[9px] font-semibold text-gray-700 dark:text-gray-200 shrink-0">5H</span>
                        </span>

                        {/* Reset Time */}
                        <div className="w-[58px] flex justify-start shrink-0">
                            {fiveHour.resetTime ? (
                                <span className={cn("flex items-center gap-0.5 font-medium transition-colors truncate", getTimeColorClass(fiveHour.resetTime))}>
                                    <Clock className="w-2.5 h-2.5 shrink-0" />
                                    {formatTimeRemaining(fiveHour.resetTime)}
                                </span>
                            ) : (
                                <span className="text-gray-400 dark:text-gray-500 italic scale-90">N/A</span>
                            )}
                        </div>

                        {/* Percentage */}
                        <span className={cn(
                            "text-right font-bold transition-colors flex items-center justify-end gap-0.5 shrink-0 w-[28px]",
                            isUnavailable ? "text-rose-700 dark:text-rose-300" : getTextColorClass(fiveHourPercentage)
                        )}>
                            {isProtected && (
                                <span title={t('accounts.quota_protected')}><Lock className="w-2.5 h-2.5 text-amber-500" /></span>
                            )}
                            {`${fiveHourPercentage}%`}
                        </span>
                    </div>
                </div>
            )}

            {/* 周配额胶囊条 */}
            {weekly && (
                <div
                    className={cn(
                        "relative h-[22px] flex items-center px-1.5 rounded-md overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5 group/quota transition-all",
                        summary.key === 'argon' && !isUnavailable && "border-indigo-300/50 dark:border-indigo-700/40 bg-indigo-50/25 dark:bg-indigo-950/25",
                        summary.isWeeklyExhausted && "border-rose-400/70 dark:border-rose-500/70 bg-rose-50/80 dark:bg-rose-950/30 ring-rose-400/30",
                        fiveHour === undefined && isUnavailable && "border-rose-400/70 dark:border-rose-500/70 bg-rose-50/80 dark:bg-rose-950/30 ring-rose-400/30"
                    )}
                    title={formatTooltip('weekly')}
                >
                    {/* Bottom Accent Progress Line */}
                    <div className="absolute bottom-0 inset-x-0 h-[2.5px] bg-black/[0.05] dark:bg-white/[0.08] overflow-hidden">
                        <div
                            className={cn(
                                "h-full transition-all duration-700 ease-out rounded-r-full",
                                summary.isWeeklyExhausted ? "bg-rose-500" : getBgColorClass(weeklyPercentage)
                            )}
                            style={{ width: `${weeklyPercentage}%` }}
                        />
                    </div>

                    {/* Content */}
                    <div className="relative z-10 w-full flex items-center text-[10px] font-mono leading-none gap-1.5">
                        <span className={cn(
                            "flex-1 min-w-0 text-gray-700 dark:text-gray-200 font-bold truncate text-left flex items-center gap-1",
                            (summary.isWeeklyExhausted || (fiveHour === undefined && isUnavailable)) && "text-rose-700 dark:text-rose-300"
                        )}>
                            {/* 统一展示图标、分组名称与周配额标识 */}
                            <Icon size={12} className="shrink-0" />
                            <span className="truncate">{weeklyLabel}</span>
                            <span className="text-[9px] font-semibold text-gray-700 dark:text-gray-200 shrink-0">
                                {t('accounts.quota_window_weekly_short', '周配额')}
                            </span>
                            {weekly.cycleTokens !== undefined && weekly.cycleTokens !== null && weekly.cycleTokens > 0 && (
                                <span className="text-[9px] font-medium text-gray-600 dark:text-gray-300 shrink-0"
                                      title={t('accounts.weekly_tokens_tooltip')}>
                                    ({weekly.cycleTokens.toLocaleString('en-US', { notation: 'compact', maximumFractionDigits: 1 })})
                                </span>
                            )}
                            {summary.key === 'argon' && (
                                <span className="hidden sm:inline-flex items-center gap-1 shrink-0 ml-0.5">
                                    <span className="text-[8px] px-1 py-0.2 rounded bg-purple-500/15 text-purple-700 dark:text-purple-300 font-bold">High</span>
                                    <span className="text-[8px] px-1 py-0.2 rounded bg-indigo-500/15 text-indigo-700 dark:text-indigo-300 font-bold">Med</span>
                                    <span className="text-[8px] px-1 py-0.2 rounded bg-blue-500/15 text-blue-700 dark:text-blue-300 font-bold">Low</span>
                                </span>
                            )}
                        </span>

                        {/* Weekly Reset Time */}
                        <div className="w-[58px] flex justify-start shrink-0">
                            {weekly.resetTime ? (
                                <span className={cn("flex items-center gap-0.5 font-medium transition-colors truncate", getTimeColorClass(weekly.resetTime))}>
                                    <Clock className="w-2.5 h-2.5 shrink-0" />
                                    {formatTimeRemaining(weekly.resetTime)}
                                </span>
                            ) : (
                                <span className="text-gray-400 dark:text-gray-500 italic scale-90">N/A</span>
                            )}
                        </div>

                        {/* Percentage */}
                        <span className={cn(
                            "text-right font-bold transition-colors flex items-center justify-end gap-0.5 shrink-0 w-[28px]",
                            summary.isWeeklyExhausted ? "text-rose-700 dark:text-rose-300" : getTextColorClass(weeklyPercentage)
                        )}>
                            {fiveHour === undefined && isProtected && (
                                <span title={t('accounts.quota_protected')}><Lock className="w-2.5 h-2.5 text-amber-500" /></span>
                            )}
                            {weeklyPercentage}%
                        </span>
                    </div>
                </div>
            )}

            {/* 如果两者都不存在（数据缺失），展示占位条 */}
            {fiveHour === undefined && !weekly && (
                <div className="relative h-[22px] flex items-center px-1.5 rounded-md overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5">
                    <div className="relative z-10 w-full flex items-center text-[10px] font-mono leading-none gap-1.5">
                        <span className="flex-1 min-w-0 text-gray-700 dark:text-gray-200 font-bold truncate text-left flex items-center gap-1">
                            <Icon size={12} className="shrink-0 opacity-60" />
                            <span className="truncate">{label}</span>
                        </span>
                        <span className="text-right font-bold text-gray-400 dark:text-gray-500 shrink-0 w-[28px]">
                            -
                        </span>
                    </div>
                </div>
            )}
        </div>
    );
}
