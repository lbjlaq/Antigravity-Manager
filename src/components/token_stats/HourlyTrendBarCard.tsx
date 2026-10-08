import React, { useState, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Sparkles } from 'lucide-react';

export interface HourlyUsagePoint {
    hour: string; // "YYYY-MM-DD HH:00" or "HH:00"
    total_tokens: number;
    input_tokens: number;
    output_tokens: number;
    cached_tokens: number;
    request_count: number;
}

interface HourlyTrendBarCardProps {
    hourlyData: HourlyUsagePoint[];
    formatNumber: (n: number) => string;
}

export const HourlyTrendBarCard: React.FC<HourlyTrendBarCardProps> = ({
    hourlyData,
    formatNumber,
}) => {
    const { t } = useTranslation();
    const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);

    // 构建无碰撞的过去 24 个滚动小时桶 (now - 23h 到 now)
    const { buckets, peakTokens, past24hTotalTokens } = useMemo(() => {
        const hourMap = new Map<string, HourlyUsagePoint>();
        for (const item of hourlyData) {
            if (!item || !item.hour) continue;
            const cleanHour = item.hour.trim();
            hourMap.set(cleanHour, item);
            const prefix = cleanHour.slice(0, 13); // "YYYY-MM-DD HH"
            hourMap.set(prefix, item);
        }

        const now = new Date();
        const result: { id: string; label: string; fullTime: string; data: HourlyUsagePoint }[] = [];
        let total = 0;
        let peak = 1;

        for (let i = 23; i >= 0; i--) {
            const d = new Date(now.getTime() - i * 3600 * 1000);
            const yyyy = d.getFullYear();
            const mm = String(d.getMonth() + 1).padStart(2, '0');
            const dd = String(d.getDate()).padStart(2, '0');
            const hh = String(d.getHours()).padStart(2, '0');

            const keyWithMin = `${yyyy}-${mm}-${dd} ${hh}:00`;
            const keyPrefix = `${yyyy}-${mm}-${dd} ${hh}`;
            const existing = hourMap.get(keyWithMin) || hourMap.get(keyPrefix);

            const point: HourlyUsagePoint = existing || {
                hour: keyWithMin,
                total_tokens: 0,
                input_tokens: 0,
                output_tokens: 0,
                cached_tokens: 0,
                request_count: 0,
            };

            total += point.total_tokens;
            if (point.total_tokens > peak) {
                peak = point.total_tokens;
            }

            result.push({
                id: keyWithMin,
                label: `${hh}:00`,
                fullTime: keyWithMin,
                data: point,
            });
        }

        return {
            buckets: result,
            peakTokens: peak,
            past24hTotalTokens: total,
        };
    }, [hourlyData]);

    const hoveredBucket = hoveredIndex !== null ? buckets[hoveredIndex] : null;

    return (
        <div className="bg-white dark:bg-[#121316] text-gray-900 dark:text-white rounded-2xl p-4 sm:p-5 border border-gray-200/80 dark:border-white/[0.08] shadow-sm flex flex-col justify-between select-none">
            {/* 顶部标题栏 + 动态副标题 */}
            <div className="flex items-center justify-between mb-4">
                <div className="flex items-center gap-2">
                    <Sparkles className="w-4 h-4 text-amber-500 dark:text-amber-400 opacity-90" />
                    <span className="text-[13px] font-semibold text-gray-900 dark:text-white/90 tracking-wide">
                        {t('token_stats.hourly_activity', '24小时活跃分布')}
                    </span>
                </div>

                {/* 动态副标题 */}
                <div className="text-xs font-mono font-medium text-gray-500 dark:text-white/70">
                    {hoveredBucket ? (
                        <span className="text-amber-600 dark:text-amber-400">
                            {hoveredBucket.label} · {formatNumber(hoveredBucket.data.total_tokens)}
                        </span>
                    ) : (
                        <span>
                            24h · {formatNumber(past24hTotalTokens)}
                        </span>
                    )}
                </div>
            </div>

            {/* 24根圆角垂直柱条 */}
            <div className="h-28 w-full flex items-end gap-1.5 sm:gap-2 px-1 py-1">
                {buckets.map((b, idx) => {
                    const isHovered = hoveredIndex === idx;
                    const hasValue = b.data.total_tokens > 0;
                    const heightPercent = hasValue
                        ? Math.max(8, (b.data.total_tokens / peakTokens) * 100)
                        : 0;

                    const isClaudeHeavy = b.data.output_tokens > b.data.input_tokens * 0.1;
                    const barColor = isClaudeHeavy ? 'bg-[#d97757]' : 'bg-[#7a9aff]';

                    return (
                        <div
                            key={b.id}
                            onMouseEnter={() => setHoveredIndex(idx)}
                            onMouseLeave={() => setHoveredIndex(null)}
                            className="flex-1 h-full flex flex-col justify-end items-center cursor-pointer group"
                            title={`${b.fullTime}: ${formatNumber(b.data.total_tokens)} Tokens`}
                        >
                            {hasValue ? (
                                <div
                                    style={{ height: `${heightPercent}%` }}
                                    className={`w-full rounded-full transition-all duration-150 ${barColor} ${
                                        isHovered
                                            ? 'brightness-110 dark:brightness-125 scale-x-110 shadow-md'
                                            : hoveredIndex !== null
                                            ? 'opacity-30 dark:opacity-40'
                                            : 'opacity-90'
                                    }`}
                                />
                            ) : (
                                <div
                                    className={`w-full h-[2px] rounded-full transition-opacity ${
                                        hoveredIndex !== null && !isHovered
                                            ? 'bg-gray-100 dark:bg-white/[0.05]'
                                            : 'bg-gray-200 dark:bg-white/15'
                                    }`}
                                />
                            )}
                        </div>
                    );
                })}
            </div>

            {/* X 轴刻度：语言中立通用格式 */}
            <div className="flex items-center justify-between text-[11px] font-mono text-gray-400 dark:text-white/40 pt-2 px-1 border-t border-gray-100 dark:border-white/[0.06] mt-2">
                <span>{buckets[0]?.label || '00:00'}</span>
                <span>{buckets[12]?.label || '12:00'}</span>
                <span>{buckets[23]?.label || '23:00'}</span>
            </div>
        </div>
    );
};
