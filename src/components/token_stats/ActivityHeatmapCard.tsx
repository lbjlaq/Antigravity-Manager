import React, { useState, useMemo } from 'react';
import { Flame } from 'lucide-react';
import { useTranslation } from 'react-i18next';

export interface DayUsagePoint {
    date: string; // YYYY-MM-DD
    total_tokens: number;
    request_count: number;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
}

interface ActivityHeatmapCardProps {
    dailyData: DayUsagePoint[];
    totalEstimatedCost?: number;
    formatNumber: (n: number) => string;
}

export const ActivityHeatmapCard: React.FC<ActivityHeatmapCardProps> = ({
    dailyData,
    totalEstimatedCost,
    formatNumber,
}) => {
    const { t } = useTranslation();
    const [hoveredDate, setHoveredDate] = useState<string | null>(null);

    // 构造过去 91 天（13周）的完整连续日期序列
    const { columns, totalRangeTokens, activeDaysCount, busiestDay, streak, dateMap } = useMemo(() => {
        const map = new Map<string, DayUsagePoint>();
        for (const d of dailyData) {
            map.set(d.date, d);
        }

        const now = new Date();
        const days: DayUsagePoint[] = [];
        // 取 91 天（13 * 7）
        for (let i = 90; i >= 0; i--) {
            const target = new Date(now);
            target.setDate(now.getDate() - i);
            const yyyy = target.getFullYear();
            const mm = String(target.getMonth() + 1).padStart(2, '0');
            const dd = String(target.getDate()).padStart(2, '0');
            const dateStr = `${yyyy}-${mm}-${dd}`;
            const existing = map.get(dateStr);
            days.push(
                existing || {
                    date: dateStr,
                    total_tokens: 0,
                    request_count: 0,
                    total_input_tokens: 0,
                    total_output_tokens: 0,
                    total_cached_tokens: 0,
                }
            );
        }

        // 计算分列：每列 7 天
        const cols: DayUsagePoint[][] = [];
        for (let i = 0; i < days.length; i += 7) {
            cols.push(days.slice(i, i + 7));
        }

        // 计算指标
        let total = 0;
        let active = 0;
        let maxDay: DayUsagePoint | null = null;
        for (const d of days) {
            total += d.total_tokens;
            if (d.total_tokens > 0) {
                active++;
                if (!maxDay || d.total_tokens > maxDay.total_tokens) {
                    maxDay = d;
                }
            }
        }

        // 计算连续活跃天数 Streak (从今天往前倒推)
        let curStreak = 0;
        for (let i = days.length - 1; i >= 0; i--) {
            if (days[i].total_tokens > 0) {
                curStreak++;
            } else if (curStreak > 0 || i !== days.length - 1) {
                break;
            }
        }

        return {
            columns: cols,
            totalRangeTokens: total,
            activeDaysCount: active,
            busiestDay: maxDay,
            streak: curStreak,
            dateMap: map,
        };
    }, [dailyData]);

    // 计算分位数四分阶梯
    const thresholds = useMemo(() => {
        const positiveValues: number[] = [];
        columns.forEach(col => {
            col.forEach(d => {
                if (d.total_tokens > 0) positiveValues.push(d.total_tokens);
            });
        });
        positiveValues.sort((a, b) => a - b);
        if (positiveValues.length === 0) return [0, 0, 0];
        return [
            positiveValues[Math.floor(positiveValues.length * 0.25)] || 0,
            positiveValues[Math.floor(positiveValues.length * 0.50)] || 0,
            positiveValues[Math.floor(positiveValues.length * 0.75)] || 0,
        ];
    }, [columns]);

    const getCellColorClass = (tokens: number): string => {
        if (tokens <= 0) return 'bg-gray-200/60 dark:bg-white/[0.07]';
        if (tokens < thresholds[0]) return 'bg-gray-900/25 dark:bg-white/25';
        if (tokens < thresholds[1]) return 'bg-gray-900/45 dark:bg-white/45';
        if (tokens < thresholds[2]) return 'bg-gray-900/70 dark:bg-white/70';
        return 'bg-gray-900 dark:bg-white';
    };

    // 格式化日期为语言中立的友好显示 (MM-DD)
    const formatFriendlyDate = (dateStr: string) => {
        const parts = dateStr.split('-');
        if (parts.length < 3) return dateStr;
        return `${parts[1]}-${parts[2]}`;
    };

    const hoveredDayData = hoveredDate ? dateMap.get(hoveredDate) || null : null;
    const isToday = (dateStr: string) => {
        const today = new Date();
        const yyyy = today.getFullYear();
        const mm = String(today.getMonth() + 1).padStart(2, '0');
        const dd = String(today.getDate()).padStart(2, '0');
        return `${yyyy}-${mm}-${dd}` === dateStr;
    };

    return (
        <div className="bg-white dark:bg-[#121316] text-gray-900 dark:text-white rounded-2xl p-4 sm:p-5 border border-gray-200/80 dark:border-white/[0.08] shadow-sm flex flex-col justify-between select-none">
            {/* 顶部标题栏 */}
            <div className="flex items-center justify-between mb-4">
                <div className="flex items-center gap-2">
                    {/* 网格四方点图标 */}
                    <div className="grid grid-cols-2 gap-0.5 w-3.5 h-3.5 opacity-80">
                        <div className="bg-gray-800 dark:bg-white rounded-[1px]" />
                        <div className="bg-gray-800 dark:bg-white rounded-[1px]" />
                        <div className="bg-gray-800 dark:bg-white rounded-[1px]" />
                        <div className="bg-gray-800 dark:bg-white rounded-[1px]" />
                    </div>
                    <span className="text-[13px] font-semibold text-gray-900 dark:text-white/90 tracking-wide">
                        {t('token_stats.activity', '活跃度')}
                    </span>
                </div>

                {/* 连续打卡火焰标签 */}
                {streak > 1 && (
                    <div className="flex items-center gap-1 px-2 py-0.5 rounded-full bg-orange-500/10 border border-orange-500/20 text-orange-500 dark:text-orange-400 text-xs font-semibold">
                        <Flame className="w-3.5 h-3.5 fill-orange-500 dark:fill-orange-400 text-orange-500 dark:text-orange-400" />
                        <span>{streak}</span>
                    </div>
                )}
            </div>

            {/* 内容区：左侧 13 周热力图网格 + 右侧数据概览 */}
            <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-5">
                {/* 13周 7行 方块矩阵 (加大尺寸与呼吸感) */}
                <div className="flex items-center gap-1 sm:gap-[5px] overflow-x-auto py-1 max-w-full">
                    {columns.map((col, colIdx) => (
                        <div key={colIdx} className="flex flex-col gap-1 sm:gap-[5px]">
                            {col.map((day) => {
                                const currentToday = isToday(day.date);
                                const isHovered = hoveredDate === day.date;
                                return (
                                    <div
                                        key={day.date}
                                        onMouseEnter={() => setHoveredDate(day.date)}
                                        onMouseLeave={() => setHoveredDate(null)}
                                        className={`w-[14px] h-[14px] sm:w-[16px] sm:h-[16px] rounded-[3px] sm:rounded-[3.5px] transition-all cursor-pointer ${getCellColorClass(day.total_tokens)} ${
                                            isHovered
                                                ? 'ring-2 ring-gray-900 dark:ring-white scale-125 z-10'
                                                : currentToday
                                                ? 'ring-1.5 ring-blue-500/80 dark:ring-blue-400/80'
                                                : ''
                                        }`}
                                        title={`${day.date}: ${formatNumber(day.total_tokens)} Tokens (${t('token_stats.requests_count', { count: day.request_count, defaultValue: `${day.request_count} requests` })})`}
                                    />
                                );
                            })}
                        </div>
                    ))}
                </div>

                {/* 右侧核心成就数据 */}
                <div className="flex flex-col justify-center min-w-[145px] pl-1 md:pl-4 border-l border-gray-200/80 dark:border-white/[0.06]">
                    {/* 总量与周期 */}
                    <div>
                        <div className="flex items-baseline gap-2">
                            <span className="text-2xl font-bold font-mono tracking-tight text-gray-900 dark:text-white">
                                {formatNumber(totalRangeTokens)}
                            </span>
                            <span className="text-[11px] text-gray-500 dark:text-white/50 font-medium">
                                {t('token_stats.weeks_count', { count: 13, defaultValue: '13 周' })}
                            </span>
                        </div>
                        {totalEstimatedCost !== undefined && totalEstimatedCost > 0 && (
                            <div className="text-xs font-mono font-medium text-emerald-600 dark:text-emerald-400 mt-0.5">
                                ~${totalEstimatedCost.toFixed(2)}
                            </div>
                        )}
                    </div>

                    <div className="mt-3 space-y-1.5 text-xs">
                        {/* 活跃天数 */}
                        <div className="flex items-center justify-between gap-3 text-gray-500 dark:text-white/60">
                            <span>{t('token_stats.active_days', '活跃天数')}</span>
                            <span className="font-mono font-medium text-gray-900 dark:text-white/90">{activeDaysCount}</span>
                        </div>

                        {/* 峰值消耗日 / 悬浮联动天 */}
                        {hoveredDayData ? (
                            <div className="flex items-center justify-between gap-2 text-gray-500 dark:text-white/60 pt-0.5">
                                <span className="text-blue-600 dark:text-blue-400 font-medium text-[11px] whitespace-nowrap">
                                    {formatFriendlyDate(hoveredDayData.date)}
                                </span>
                                <span className="font-mono font-medium text-blue-600 dark:text-blue-400">
                                    {formatNumber(hoveredDayData.total_tokens)}
                                </span>
                            </div>
                        ) : busiestDay ? (
                            <div className="flex items-center justify-between gap-2 text-gray-500 dark:text-white/60 pt-0.5">
                                <span className="text-gray-500 dark:text-white/60 text-[11px] whitespace-nowrap">
                                    {t('token_stats.busiest_day', '峰值消耗日')}
                                </span>
                                <span className="font-mono font-medium text-gray-900 dark:text-white/90 whitespace-nowrap">
                                    {formatFriendlyDate(busiestDay.date)} · {formatNumber(busiestDay.total_tokens)}
                                </span>
                            </div>
                        ) : null}
                    </div>
                </div>
            </div>
        </div>
    );
};
