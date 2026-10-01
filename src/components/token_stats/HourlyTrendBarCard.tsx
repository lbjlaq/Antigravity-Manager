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

    // 格式化确保刚好 24 个小时点
    const { buckets, peakTokens, todayTotalTokens } = useMemo(() => {
        // 构建当前日期 0-23 点或者最近 24 小时的稳定桶
        // 先建立已有小时的映射
        const hourMap = new Map<string, HourlyUsagePoint>();
        for (const item of hourlyData) {
            // 获取 HH:00 或提取纯小时数字
            const match = item.hour.match(/(\d{1,2}):00/);
            if (match) {
                const h = parseInt(match[1], 10);
                hourMap.set(String(h), item);
            }
        }

        // 默认显示今天的 24 个自然小时 00:00 -> 23:00
        const result: { hourNumber: number; label: string; data: HourlyUsagePoint }[] = [];
        let total = 0;
        let peak = 1;

        for (let h = 0; h < 24; h++) {
            const existing = hourMap.get(String(h));
            const point: HourlyUsagePoint = existing || {
                hour: `${String(h).padStart(2, '0')}:00`,
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
                hourNumber: h,
                label: `${String(h).padStart(2, '0')}:00`,
                data: point,
            });
        }

        return {
            buckets: result,
            peakTokens: peak,
            todayTotalTokens: total,
        };
    }, [hourlyData]);

    const hoveredBucket = hoveredIndex !== null ? buckets[hoveredIndex] : null;

    return (
        <div className="bg-[#121316] dark:bg-[#121316] text-white rounded-2xl p-4 sm:p-5 border border-white/[0.08] shadow-sm flex flex-col justify-between select-none">
            {/* 顶部标题栏 + 动态副标题 */}
            <div className="flex items-center justify-between mb-4">
                <div className="flex items-center gap-2">
                    <Sparkles className="w-4 h-4 text-amber-400 opacity-90" />
                    <span className="text-[13px] font-semibold text-white/90 tracking-wide">
                        {t('token_stats.hourly_activity', '24小时活动')}
                    </span>
                </div>

                {/* 动态副标题：平时显示今日汇总，悬浮时显示该小时的具体数值 */}
                <div className="text-xs font-mono font-medium text-white/70">
                    {hoveredBucket ? (
                        <span className="text-amber-400">
                            {hoveredBucket.label} · {formatNumber(hoveredBucket.data.total_tokens)}
                        </span>
                    ) : (
                        <span>
                            {t('token_stats.today', '今日')} · {formatNumber(todayTotalTokens)}
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

                    // 主题色彩：借鉴 Vorssaint，使用温润的琥珀橙红/灵动天蓝
                    const isClaudeHeavy = b.data.output_tokens > b.data.input_tokens * 0.1;
                    const barColor = isClaudeHeavy ? 'bg-[#d97757]' : 'bg-[#7a9aff]';

                    return (
                        <div
                            key={b.hourNumber}
                            onMouseEnter={() => setHoveredIndex(idx)}
                            onMouseLeave={() => setHoveredIndex(null)}
                            className="flex-1 h-full flex flex-col justify-end items-center cursor-pointer group"
                            title={`${b.label}: ${formatNumber(b.data.total_tokens)} Tokens`}
                        >
                            {hasValue ? (
                                <div
                                    style={{ height: `${heightPercent}%` }}
                                    className={`w-full rounded-full transition-all duration-150 ${barColor} ${
                                        isHovered
                                            ? 'brightness-125 scale-x-110 shadow-lg shadow-white/10'
                                            : hoveredIndex !== null
                                            ? 'opacity-40'
                                            : 'opacity-90'
                                    }`}
                                />
                            ) : (
                                <div
                                    className={`w-full h-[2px] rounded-full transition-opacity ${
                                        hoveredIndex !== null && !isHovered ? 'bg-white/[0.05]' : 'bg-white/15'
                                    }`}
                                />
                            )}
                        </div>
                    );
                })}
            </div>

            {/* X 轴刻度：极简 0时、12时、23时 */}
            <div className="flex items-center justify-between text-[11px] font-mono text-white/40 pt-2 px-1 border-t border-white/[0.06] mt-2">
                <span>0时</span>
                <span>12时</span>
                <span>23时</span>
            </div>
        </div>
    );
};
