import React, { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { motion } from 'framer-motion';
import {
    TrendingUp,
    Cpu,
    Sparkles,
    ArrowUpRight,
    Users,
    Flame,
    Calendar,
} from 'lucide-react';
import {
    AreaChart,
    Area,
    BarChart,
    Bar,
    XAxis,
    YAxis,
    CartesianGrid,
    Tooltip,
    ResponsiveContainer,
} from 'recharts';
import { HourlyUsagePoint } from './HourlyTrendBarCard';
import { ModelShareItem } from './TopModelsShareCard';
import { DayUsagePoint } from './ActivityHeatmapCard';
import { AccountTokenStats, TokenStatsAggregated } from '../../pages/TokenStats';

interface TokenStatsSummary {
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_reasoning_tokens?: number;
    total_tokens: number;
    total_requests: number;
    unique_accounts: number;
    avg_ttft_ms?: number;
}

interface AppleBentoViewProps {
    summary: TokenStatsSummary | null;
    dailyData: DayUsagePoint[];
    hourlyData: HourlyUsagePoint[];
    timelineData?: TokenStatsAggregated[];
    modelShareItems: ModelShareItem[];
    accountData: AccountTokenStats[];
    displayModelTrendData: any[];
    displayAccountTrendData: any[];
    allModels: string[];
    allAccounts: string[];
    viewMode: 'model' | 'account';
    metricType: 'tokens' | 'cost';
    chartType: 'bar' | 'area';
    timeRange: 'minute' | 'hourly' | 'daily' | 'weekly' | 'all';
    totalEstimatedCost: number;
    cacheMetrics: { hitRate: number; savings: number };
    formatNumber: (n: number) => string;
    formatCurrency: (n: number) => string;
    switchChartType: (t: 'bar' | 'area') => void;
    switchMetricType: (m: 'tokens' | 'cost') => void;
    switchViewMode: (v: 'model' | 'account') => void;
    setShowPricingModal: (s: boolean) => void;
    customTooltip: React.FC<any>;
}

const CHART_PALETTE = ['#0071e3', '#af52de', '#30b0c7', '#34c759', '#ff9500', '#8e8e93'];

export const AppleBentoView: React.FC<AppleBentoViewProps> = ({
    summary,
    dailyData,
    hourlyData,
    timelineData,
    modelShareItems,
    accountData,
    displayModelTrendData,
    displayAccountTrendData,
    allModels,
    allAccounts,
    viewMode,
    metricType,
    chartType,
    timeRange,
    totalEstimatedCost,
    cacheMetrics,
    formatNumber,
    formatCurrency,
    switchChartType,
    switchMetricType,
    switchViewMode,
    setShowPricingModal,
    customTooltip: CustomTrendTooltip,
}) => {
    const { t } = useTranslation();
    const [hoveredHourIdx, setHoveredHourIdx] = useState<number | null>(null);
    const [hoveredDate, setHoveredDate] = useState<string | null>(null);

    // 构造过去 91 天（13周）的完整连续日期序列
    const { activityCols, totalRangeTokens, activeDaysCount, busiestDay, streak, dateMap } = useMemo(() => {
        const map = new Map<string, DayUsagePoint>();
        for (const d of dailyData) {
            map.set(d.date, d);
        }

        const now = new Date();
        const days: DayUsagePoint[] = [];
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

        const cols: DayUsagePoint[][] = [];
        for (let i = 0; i < days.length; i += 7) {
            cols.push(days.slice(i, i + 7));
        }

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

        let curStreak = 0;
        for (let i = days.length - 1; i >= 0; i--) {
            if (days[i].total_tokens > 0) {
                curStreak++;
            } else if (curStreak > 0 || i !== days.length - 1) {
                break;
            }
        }

        return {
            activityCols: cols,
            totalRangeTokens: total,
            activeDaysCount: active,
            busiestDay: maxDay,
            streak: curStreak,
            dateMap: map,
        };
    }, [dailyData]);

    const activityThresholds = useMemo(() => {
        const positiveValues: number[] = [];
        activityCols.forEach(col => {
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
    }, [activityCols]);

    const getActivityColor = (tokens: number): string => {
        if (tokens <= 0) return 'bg-gray-100 dark:bg-white/[0.06]';
        if (tokens < activityThresholds[0]) return 'bg-[#0071e3]/20 dark:bg-[#2997ff]/20';
        if (tokens < activityThresholds[1]) return 'bg-[#0071e3]/45 dark:bg-[#2997ff]/45';
        if (tokens < activityThresholds[2]) return 'bg-[#0071e3]/70 dark:bg-[#2997ff]/70';
        return 'bg-[#0071e3] dark:bg-[#2997ff]';
    };

    const hoveredDay = hoveredDate ? dateMap.get(hoveredDate) : null;

    // 计算 24 小时律动柱条
    const { hourBuckets, maxHourTokens, totalPast24h, peakHourLabel } = useMemo(() => {
        const map = new Map<string, HourlyUsagePoint>();
        for (const h of hourlyData) {
            map.set(h.hour, h);
        }

        const now = new Date();
        const buckets: { hourLabel: string; full: string; data: HourlyUsagePoint }[] = [];
        let peak = 1;
        let total = 0;
        let peakLabel = '';

        for (let i = 23; i >= 0; i--) {
            const target = new Date(now.getTime() - i * 3600 * 1000);
            const yyyy = target.getFullYear();
            const mm = String(target.getMonth() + 1).padStart(2, '0');
            const dd = String(target.getDate()).padStart(2, '0');
            const hh = String(target.getHours()).padStart(2, '0');
            const key = `${yyyy}-${mm}-${dd} ${hh}:00`;
            const point = map.get(key) || {
                hour: key,
                total_tokens: 0,
                input_tokens: 0,
                output_tokens: 0,
                cached_tokens: 0,
                request_count: 0,
            };
            total += point.total_tokens;
            if (point.total_tokens > peak) {
                peak = point.total_tokens;
                peakLabel = `${hh}:00`;
            }
            buckets.push({
                hourLabel: `${hh}:00`,
                full: key,
                data: point,
            });
        }

        return {
            hourBuckets: buckets,
            maxHourTokens: peak,
            totalPast24h: total,
            peakHourLabel: peakLabel || t('token_stats.no_peak', '暂无峰值'),
        };
    }, [hourlyData, t]);

    const activeHourData = hoveredHourIdx !== null ? hourBuckets[hoveredHourIdx] : null;

    // 计算核心模型统计
    const topModels = useMemo(() => {
        const sorted = [...modelShareItems].sort((a, b) => b.total_tokens - a.total_tokens);
        return sorted.slice(0, 5);
    }, [modelShareItems]);

    // 计算账号消耗排行 Top 5
    const topAccounts = useMemo(() => {
        const sorted = [...accountData].sort((a, b) => b.total_tokens - a.total_tokens);
        return sorted.slice(0, 5);
    }, [accountData]);

    // 计算输入输出比例 (Apple 环形/条形对比)
    const inOutRatio = useMemo(() => {
        if (!summary || summary.total_tokens === 0) return { inputPct: 50, outputPct: 50 };
        const total = summary.total_input_tokens + summary.total_output_tokens;
        if (total === 0) return { inputPct: 50, outputPct: 50 };
        const inputPct = Math.round((summary.total_input_tokens / total) * 100);
        return { inputPct, outputPct: 100 - inputPct };
    }, [summary]);

    // 计算顶部4个卡片的极简微缩趋势线（Sparkline SVG 曲线，基于当前选择周期的真实时间线数据）
    const cardSparklines = useMemo(() => {
        const createSmoothPath = (values: number[], width = 96, height = 32) => {
            if (!values || values.length === 0) {
                return {
                    path: `M 0,${height * 0.7} Q ${width / 2},${height * 0.7} ${width},${height * 0.7}`,
                    area: `M 0,${height * 0.7} Q ${width / 2},${height * 0.7} ${width},${height * 0.7} L ${width},${height} L 0,${height} Z`,
                    lastX: width,
                    lastY: height * 0.7,
                    hasData: false,
                };
            }
            const min = Math.min(...values);
            const max = Math.max(...values);
            const range = max - min || 1;
            const padding = 3;
            const usableH = height - padding * 2;

            const points = values.map((v, i) => {
                const x = (i / Math.max(1, values.length - 1)) * width;
                const y = height - padding - ((v - min) / range) * usableH;
                return { x, y };
            });

            if (points.length === 1) {
                return {
                    path: `M 0,${points[0].y} L ${width},${points[0].y}`,
                    area: `M 0,${points[0].y} L ${width},${points[0].y} L ${width},${height} L 0,${height} Z`,
                    lastX: width,
                    lastY: points[0].y,
                    hasData: true,
                };
            }

            // Catmull-Rom 或三次贝塞尔曲线控制点
            let path = `M ${points[0].x},${points[0].y}`;
            for (let i = 0; i < points.length - 1; i++) {
                const p0 = points[i === 0 ? 0 : i - 1];
                const p1 = points[i];
                const p2 = points[i + 1];
                const p3 = points[i + 2 < points.length ? i + 2 : i + 1];
                const cp1x = p1.x + (p2.x - p0.x) / 6;
                const cp1y = p1.y + (p2.y - p0.y) / 6;
                const cp2x = p2.x - (p3.x - p1.x) / 6;
                const cp2y = p2.y - (p3.y - p1.y) / 6;
                path += ` C ${cp1x},${cp1y} ${cp2x},${cp2y} ${p2.x},${p2.y}`;
            }

            const last = points[points.length - 1];
            const area = `${path} L ${last.x},${height} L 0,${height} Z`;

            return {
                path,
                area,
                lastX: last.x,
                lastY: last.y,
                hasData: max > 0,
            };
        };

        // 数据源：优先使用当前查询时间跨度的真实 timelineData，若不存在则回退至 dailyData 或 24小时数据
        const sourceData = timelineData && timelineData.length > 0
            ? timelineData
            : dailyData.length > 0
            ? dailyData
            : hourBuckets.map(b => b.data);

        const tokensSeries = sourceData.map((d: any) => d.total_tokens || 0);
        const inputSeries = sourceData.map((d: any) => d.total_input_tokens || d.input_tokens || 0);
        const cacheSeries = sourceData.map((d: any) => d.total_cached_tokens || d.cached_tokens || 0);
        const requestSeries = sourceData.map((d: any) => d.request_count || 0);

        return {
            tokens: createSmoothPath(tokensSeries, 96, 32),
            cost: createSmoothPath(inputSeries, 96, 32),
            cache: createSmoothPath(cacheSeries, 96, 32),
            network: createSmoothPath(requestSeries, 96, 32),
        };
    }, [timelineData, dailyData, hourBuckets]);

    return (
        <div className="space-y-4 font-sans text-gray-900 dark:text-[#f5f5f7]">
            {/* ================= 1. Apple 核心态势 Bento 顶部卡片组 ================= */}
            {summary && (
                <div className="grid grid-cols-2 lg:grid-cols-4 gap-3.5">
                    {/* Bento 1: 总消耗量与输入输出比 */}
                    <div className="bg-white/95 dark:bg-base-100 backdrop-blur-xl rounded-2xl p-4 sm:p-5 border border-black/[0.05] dark:border-base-200 shadow-sm flex flex-col justify-between transition-all hover:shadow-md relative overflow-hidden group">
                        <div className="flex items-start justify-between">
                            <span className="text-[11px] font-semibold text-gray-500 dark:text-gray-400 tracking-wider uppercase">
                                {t('token_stats.total_usage', '总消耗量')}
                            </span>
                            {/* 右上方：真实数据曲线 Sparkline */}
                            <div className="w-[100px] h-[34px] shrink-0 relative">
                                <svg width="100" height="34" className="overflow-visible">
                                    <defs>
                                        <linearGradient id="tokensSparklineGrad" x1="0" y1="0" x2="0" y2="1">
                                            <stop offset="0%" stopColor="#0071e3" stopOpacity="0.25" />
                                            <stop offset="100%" stopColor="#0071e3" stopOpacity="0.0" />
                                        </linearGradient>
                                    </defs>
                                    <path
                                        d={cardSparklines.tokens.area}
                                        fill="url(#tokensSparklineGrad)"
                                    />
                                    <path
                                        d={cardSparklines.tokens.path}
                                        fill="none"
                                        stroke="#0071e3"
                                        strokeWidth="2"
                                        strokeLinecap="round"
                                        strokeLinejoin="round"
                                    />
                                    {cardSparklines.tokens.hasData && (
                                        <>
                                            <circle
                                                cx={cardSparklines.tokens.lastX}
                                                cy={cardSparklines.tokens.lastY}
                                                r="5"
                                                fill="#0071e3"
                                                fillOpacity="0.25"
                                                className="animate-ping"
                                                style={{ animationDuration: '2.5s' }}
                                            />
                                            <circle
                                                cx={cardSparklines.tokens.lastX}
                                                cy={cardSparklines.tokens.lastY}
                                                r="2.5"
                                                fill="#0071e3"
                                            />
                                        </>
                                    )}
                                </svg>
                            </div>
                        </div>

                        {/* 中间主指标 */}
                        <div className="my-2">
                            <div className="text-3xl font-extrabold tracking-tight text-gray-900 dark:text-white font-mono leading-none">
                                {formatNumber(summary.total_tokens)}
                            </div>
                            <div className="text-[11px] text-gray-400 dark:text-gray-500 mt-2 flex items-center gap-1.5 font-mono">
                                <span>{t('token_stats.input_short', '入')} {formatNumber(summary.total_input_tokens)}</span>
                                <span className="text-gray-300 dark:text-gray-600">/</span>
                                <span>{t('token_stats.output_short', '出')} {formatNumber(summary.total_output_tokens)}</span>
                            </div>
                        </div>

                        {/* 胶囊比例条 */}
                        <div className="space-y-1.5 pt-2 border-t border-black/[0.04] dark:border-base-200">
                            <div className="w-full h-1.5 bg-gray-100 dark:bg-base-200 rounded-full overflow-hidden flex">
                                <div
                                    className="h-full bg-[#0071e3] rounded-full"
                                    style={{ width: `${inOutRatio.inputPct}%` }}
                                    title={t('token_stats.input_ratio_hint', '输入占比: {{pct}}%', { pct: inOutRatio.inputPct })}
                                />
                                <div
                                    className="h-full bg-[#af52de] rounded-full"
                                    style={{ width: `${inOutRatio.outputPct}%` }}
                                    title={t('token_stats.output_ratio_hint', '输出占比: {{pct}}%', { pct: inOutRatio.outputPct })}
                                />
                            </div>
                            <div className="flex items-center justify-between text-[11px] font-mono text-gray-400 dark:text-gray-500">
                                <span>{inOutRatio.inputPct}% 入</span>
                                <span>{inOutRatio.outputPct}% 出</span>
                            </div>
                        </div>
                    </div>

                    {/* Bento 2: 预估花费价值 */}
                    <div className="bg-white/95 dark:bg-base-100 backdrop-blur-xl rounded-2xl p-4 sm:p-5 border border-black/[0.05] dark:border-base-200 shadow-sm flex flex-col justify-between transition-all hover:shadow-md relative overflow-hidden group">
                        <div className="flex items-start justify-between">
                            <span className="text-[11px] font-semibold text-emerald-600 dark:text-emerald-400 tracking-wider uppercase">
                                {t('token_stats.estimated_spending', '预估算支出')}
                            </span>
                            {/* 右上方：真实支出走势 Sparkline */}
                            <div className="w-[100px] h-[34px] shrink-0 relative">
                                <svg width="100" height="34" className="overflow-visible">
                                    <defs>
                                        <linearGradient id="costSparklineGrad" x1="0" y1="0" x2="0" y2="1">
                                            <stop offset="0%" stopColor="#10b981" stopOpacity="0.25" />
                                            <stop offset="100%" stopColor="#10b981" stopOpacity="0.0" />
                                        </linearGradient>
                                    </defs>
                                    <path
                                        d={cardSparklines.cost.area}
                                        fill="url(#costSparklineGrad)"
                                    />
                                    <path
                                        d={cardSparklines.cost.path}
                                        fill="none"
                                        stroke="#10b981"
                                        strokeWidth="2"
                                        strokeLinecap="round"
                                        strokeLinejoin="round"
                                    />
                                    {cardSparklines.cost.hasData && (
                                        <>
                                            <circle
                                                cx={cardSparklines.cost.lastX}
                                                cy={cardSparklines.cost.lastY}
                                                r="5"
                                                fill="#10b981"
                                                fillOpacity="0.25"
                                                className="animate-ping"
                                                style={{ animationDuration: '2.5s' }}
                                            />
                                            <circle
                                                cx={cardSparklines.cost.lastX}
                                                cy={cardSparklines.cost.lastY}
                                                r="2.5"
                                                fill="#10b981"
                                            />
                                        </>
                                    )}
                                </svg>
                            </div>
                        </div>

                        {/* 中间主指标 */}
                        <div className="my-2">
                            <div className="text-3xl font-extrabold tracking-tight text-emerald-600 dark:text-emerald-400 font-mono leading-none">
                                ${totalEstimatedCost.toFixed(2)}
                            </div>
                            <div className="text-[11px] text-gray-400 dark:text-gray-500 mt-2 truncate">
                                {t('token_stats.pricing_snapshot_hint', '基于当前单价快照加权累计')}
                            </div>
                        </div>

                        <div className="flex items-center justify-between text-[11px] font-mono text-gray-400 dark:text-gray-500 pt-2 border-t border-black/[0.04] dark:border-base-200">
                            <span>{t('token_stats.billing_baseline', '实时计费基线')}</span>
                            <button
                                onClick={() => setShowPricingModal(true)}
                                className="text-emerald-600 dark:text-emerald-400 hover:underline flex items-center gap-0.5 font-sans font-medium"
                            >
                                {t('token_stats.pricing_matrix', '定价矩阵')} <ArrowUpRight className="w-3 h-3" />
                            </button>
                        </div>
                    </div>

                    {/* Bento 3: 缓存效能 */}
                    <div className="bg-white/95 dark:bg-base-100 backdrop-blur-xl rounded-2xl p-4 sm:p-5 border border-black/[0.05] dark:border-base-200 shadow-sm flex flex-col justify-between transition-all hover:shadow-md relative overflow-hidden group">
                        <div className="flex items-start justify-between">
                            <span className="text-[11px] font-semibold text-[#30b0c7] tracking-wider uppercase">
                                {t('token_stats.cache_efficiency', '缓存节省效能')}
                            </span>
                            {/* 右上方：真实缓存走势 Sparkline */}
                            <div className="w-[100px] h-[34px] shrink-0 relative">
                                <svg width="100" height="34" className="overflow-visible">
                                    <defs>
                                        <linearGradient id="cacheSparklineGrad" x1="0" y1="0" x2="0" y2="1">
                                            <stop offset="0%" stopColor="#30b0c7" stopOpacity="0.25" />
                                            <stop offset="100%" stopColor="#30b0c7" stopOpacity="0.0" />
                                        </linearGradient>
                                    </defs>
                                    <path
                                        d={cardSparklines.cache.area}
                                        fill="url(#cacheSparklineGrad)"
                                    />
                                    <path
                                        d={cardSparklines.cache.path}
                                        fill="none"
                                        stroke="#30b0c7"
                                        strokeWidth="2"
                                        strokeLinecap="round"
                                        strokeLinejoin="round"
                                    />
                                    {cardSparklines.cache.hasData && (
                                        <>
                                            <circle
                                                cx={cardSparklines.cache.lastX}
                                                cy={cardSparklines.cache.lastY}
                                                r="5"
                                                fill="#30b0c7"
                                                fillOpacity="0.25"
                                                className="animate-ping"
                                                style={{ animationDuration: '2.5s' }}
                                            />
                                            <circle
                                                cx={cardSparklines.cache.lastX}
                                                cy={cardSparklines.cache.lastY}
                                                r="2.5"
                                                fill="#30b0c7"
                                            />
                                        </>
                                    )}
                                </svg>
                            </div>
                        </div>

                        {/* 中间主指标 */}
                        <div className="my-2">
                            <div className="flex items-center gap-2">
                                <span className="text-3xl font-extrabold tracking-tight text-gray-900 dark:text-white font-mono leading-none">
                                    {formatNumber(summary.total_cached_tokens)}
                                </span>
                                <span className="text-[11px] font-bold px-1.5 py-0.5 rounded-md bg-[#30b0c7]/10 text-[#30b0c7] border border-[#30b0c7]/20 shrink-0">
                                    {cacheMetrics.hitRate.toFixed(1)}% 命中
                                </span>
                            </div>
                            <div className="text-[11px] text-gray-400 dark:text-gray-500 mt-2 font-mono truncate">
                                {t('token_stats.saved_funds', '节省资金约 ${{amount}}', { amount: cacheMetrics.savings.toFixed(2) })}
                            </div>
                        </div>

                        <div className="flex items-center justify-between text-[11px] font-mono text-gray-400 dark:text-gray-500 pt-2 border-t border-black/[0.04] dark:border-base-200">
                            <span>{t('token_stats.context_reuse', '智能上下文复用')}</span>
                            {summary?.avg_ttft_ms && summary.avg_ttft_ms > 0 ? (
                                <span className="text-[#30b0c7] font-medium" title={t('token_stats.avg_ttft_tip', '全局平均首字延迟')}>
                                    ⚡ {summary.avg_ttft_ms < 1000 ? `${summary.avg_ttft_ms.toFixed(0)}ms` : `${(summary.avg_ttft_ms / 1000).toFixed(2)}s`} TTFT
                                </span>
                            ) : (
                                <span className="text-[#30b0c7] font-medium">{t('token_stats.zero_latency', '0 延时吞吐')}</span>
                            )}
                        </div>
                    </div>

                    {/* Bento 4: 活跃节点网络 */}
                    <div className="bg-white/95 dark:bg-base-100 backdrop-blur-xl rounded-2xl p-4 sm:p-5 border border-black/[0.05] dark:border-base-200 shadow-sm flex flex-col justify-between transition-all hover:shadow-md relative overflow-hidden group">
                        <div className="flex items-start justify-between">
                            <span className="text-[11px] font-semibold text-[#af52de] tracking-wider uppercase">
                                {t('token_stats.active_network', '活跃调度网络')}
                            </span>
                            {/* 右上方：真实请求交互 Sparkline */}
                            <div className="w-[100px] h-[34px] shrink-0 relative">
                                <svg width="100" height="34" className="overflow-visible">
                                    <defs>
                                        <linearGradient id="networkSparklineGrad" x1="0" y1="0" x2="0" y2="1">
                                            <stop offset="0%" stopColor="#af52de" stopOpacity="0.25" />
                                            <stop offset="100%" stopColor="#af52de" stopOpacity="0.0" />
                                        </linearGradient>
                                    </defs>
                                    <path
                                        d={cardSparklines.network.area}
                                        fill="url(#networkSparklineGrad)"
                                    />
                                    <path
                                        d={cardSparklines.network.path}
                                        fill="none"
                                        stroke="#af52de"
                                        strokeWidth="2"
                                        strokeLinecap="round"
                                        strokeLinejoin="round"
                                    />
                                    {cardSparklines.network.hasData && (
                                        <>
                                            <circle
                                                cx={cardSparklines.network.lastX}
                                                cy={cardSparklines.network.lastY}
                                                r="5"
                                                fill="#af52de"
                                                fillOpacity="0.25"
                                                className="animate-ping"
                                                style={{ animationDuration: '2.5s' }}
                                            />
                                            <circle
                                                cx={cardSparklines.network.lastX}
                                                cy={cardSparklines.network.lastY}
                                                r="2.5"
                                                fill="#af52de"
                                            />
                                        </>
                                    )}
                                </svg>
                            </div>
                        </div>

                        {/* 中间主指标 */}
                        <div className="my-2">
                            <div className="flex items-baseline gap-3">
                                <div className="flex items-baseline gap-1">
                                    <span className="text-3xl font-extrabold tracking-tight text-gray-900 dark:text-white font-mono leading-none">
                                        {summary.unique_accounts}
                                    </span>
                                    <span className="text-[11px] font-medium text-gray-400">
                                        {t('token_stats.account', '账号')}
                                    </span>
                                </div>
                                <div className="flex items-baseline gap-1">
                                    <span className="text-3xl font-extrabold tracking-tight text-[#af52de] font-mono leading-none">
                                        {modelShareItems.length}
                                    </span>
                                    <span className="text-[11px] font-medium text-gray-400">
                                        {t('token_stats.model', '模型')}
                                    </span>
                                </div>
                            </div>
                            <div className="text-[11px] text-gray-400 dark:text-gray-500 mt-2 font-mono truncate">
                                {t('token_stats.total_interactions', '累计交互请求 {{count}} 轮次', { count: summary.total_requests })}
                            </div>
                        </div>

                        <div className="flex items-center justify-between text-[11px] font-mono text-gray-400 dark:text-gray-500 pt-2 border-t border-black/[0.04] dark:border-base-200">
                            <span>{t('token_stats.load_balance_ready', '负载均衡就绪')}</span>
                            <span className="text-emerald-500 font-medium">{t('token_stats.smooth_operation', '100% 顺畅')}</span>
                        </div>
                    </div>
                </div>
            )}

            {/* ================= 2. Apple 周期与时空双域全景 Bento (活跃度 91 天 & 律动 24 小时 + 效能诊断) ================= */}
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                {/* 左卡：91 天活动全景矩阵 (活跃度) */}
                <div className="bg-white/90 dark:bg-base-100 backdrop-blur-xl rounded-3xl p-5 border border-black/[0.04] dark:border-base-200 shadow-sm flex flex-col justify-between h-[285px]">
                    {/* 卡片头部 */}
                    <div className="flex items-center justify-between pb-2.5 border-b border-black/[0.03] dark:border-base-200">
                        <div className="flex items-center gap-2">
                            <div className="w-6 h-6 rounded-full bg-blue-500/10 dark:bg-blue-500/20 flex items-center justify-center text-[#0071e3] dark:text-[#2997ff]">
                                <Calendar className="w-3.5 h-3.5" />
                            </div>
                            <span className="text-sm font-semibold tracking-tight text-gray-900 dark:text-base-content">
                                {t('token_stats.activity', '活跃度')}
                            </span>
                            <span className="text-[11px] font-mono text-gray-400 dark:text-gray-500 hidden sm:inline">
                                {t('token_stats.panorama_91d', '91 天全景')}
                            </span>
                        </div>
                        {streak > 1 ? (
                            <div className="flex items-center gap-1 px-2.5 py-0.5 rounded-full bg-orange-500/10 border border-orange-500/20 text-orange-500 text-xs font-semibold">
                                <Flame className="w-3.5 h-3.5 fill-orange-500 text-orange-500" />
                                <span>{t('token_stats.streak_days', '连续 {{count}} 天', { count: streak })}</span>
                            </div>
                        ) : (
                            <span className="text-xs font-mono text-gray-400 dark:text-gray-500">
                                {t('token_stats.matrix_13w', '13 周点阵')}
                            </span>
                        )}
                    </div>

                    {/* 卡片主体：矩阵 + 详情 */}
                    <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 py-2">
                        {/* 13 列 x 7 行 圆角微晶块 */}
                        <div className="flex items-center gap-1 sm:gap-[4.5px] overflow-x-auto py-1 max-w-full">
                            {activityCols.map((col, cIdx) => (
                                <div key={cIdx} className="flex flex-col gap-1 sm:gap-[4.5px]">
                                    {col.map((day) => {
                                        const isHovered = hoveredDate === day.date;
                                        const now = new Date();
                                        const yyyy = now.getFullYear();
                                        const mm = String(now.getMonth() + 1).padStart(2, '0');
                                        const dd = String(now.getDate()).padStart(2, '0');
                                        const isCurToday = day.date === `${yyyy}-${mm}-${dd}`;
                                        return (
                                            <div
                                                key={day.date}
                                                onMouseEnter={() => setHoveredDate(day.date)}
                                                onMouseLeave={() => setHoveredDate(null)}
                                                className={`w-[13px] h-[13px] sm:w-[15px] sm:h-[15px] rounded-[3px] sm:rounded-[3.5px] transition-all cursor-pointer ${getActivityColor(day.total_tokens)} ${
                                                    isHovered
                                                        ? 'ring-2 ring-gray-900 dark:ring-white scale-125 z-10 shadow-sm'
                                                        : isCurToday
                                                        ? 'ring-1.5 ring-blue-500/80'
                                                        : ''
                                                }`}
                                                title={`${day.date}: ${formatNumber(day.total_tokens)} Tokens (${day.request_count} ${t('token_stats.times', '次')})`}
                                            />
                                        );
                                    })}
                                </div>
                            ))}
                        </div>

                        {/* 右侧核心态势数据与 Hover 联动 */}
                        <div className="flex flex-col justify-center sm:min-w-[150px] sm:pl-3 sm:border-l border-black/[0.03] dark:border-white/[0.04] flex-1">
                            <div>
                                <span className="text-[11px] text-gray-400 dark:text-gray-500">{t('token_stats.total_activity_13w', '13周累计活跃')}</span>
                                <div className="text-xl font-bold font-mono text-gray-900 dark:text-white mt-0.5">
                                    {formatNumber(totalRangeTokens)} <span className="text-xs font-normal text-gray-400">Tokens</span>
                                </div>
                            </div>
                            <div className="mt-2.5 space-y-1.5 text-xs">
                                <div className="flex items-center justify-between text-gray-500 dark:text-gray-400">
                                    <span>{t('token_stats.active_days', '活跃天数')}</span>
                                    <span className="font-mono font-medium text-gray-900 dark:text-white">
                                        {activeDaysCount} <span className="text-[10px] text-gray-400">({Math.round((activeDaysCount / 91) * 100)}%)</span>
                                    </span>
                                </div>
                                {hoveredDay ? (
                                    <div className="flex items-center justify-between text-blue-600 dark:text-blue-400 font-medium pt-1 border-t border-black/[0.03] dark:border-white/[0.04]">
                                        <span className="text-[11px] font-mono">{hoveredDay.date.slice(5)}</span>
                                        <span className="font-mono">{formatNumber(hoveredDay.total_tokens)}</span>
                                    </div>
                                ) : busiestDay ? (
                                    <div className="flex items-center justify-between text-gray-500 dark:text-gray-400 pt-1 border-t border-black/[0.03] dark:border-white/[0.04]">
                                        <span className="text-[11px]">{t('token_stats.peak_day', '峰值日 {{date}}', { date: busiestDay.date.slice(5) })}</span>
                                        <span className="font-mono text-gray-900 dark:text-white">{formatNumber(busiestDay.total_tokens)}</span>
                                    </div>
                                ) : null}
                            </div>
                        </div>
                    </div>

                    {/* 卡片底栏：分级色块 + 状态指示 */}
                    <div className="flex items-center justify-between text-[11px] font-mono text-gray-400 dark:text-gray-500 pt-2 border-t border-black/[0.03] dark:border-white/[0.04]">
                        <div className="flex items-center gap-1.5">
                            <span className="text-[10px]">{t('token_stats.less', '少')}</span>
                            <div className="flex items-center gap-1">
                                <span className="w-2.5 h-2.5 rounded-[2px] bg-gray-100 dark:bg-white/[0.06]" />
                                <span className="w-2.5 h-2.5 rounded-[2px] bg-[#0071e3]/20 dark:bg-[#2997ff]/20" />
                                <span className="w-2.5 h-2.5 rounded-[2px] bg-[#0071e3]/45 dark:bg-[#2997ff]/45" />
                                <span className="w-2.5 h-2.5 rounded-[2px] bg-[#0071e3]/70 dark:bg-[#2997ff]/70" />
                                <span className="w-2.5 h-2.5 rounded-[2px] bg-[#0071e3] dark:bg-[#2997ff]" />
                            </div>
                            <span className="text-[10px]">{t('token_stats.more', '多')}</span>
                        </div>
                        <span>{t('token_stats.activity_cadence', '连续周期沉淀')}</span>
                    </div>
                </div>

                {/* 右卡：24 小时极简律动音频柱条 + 效能诊断 (严格等高 285px) */}
                <div className="bg-white/90 dark:bg-base-100 backdrop-blur-xl rounded-3xl p-5 border border-black/[0.04] dark:border-base-200 shadow-sm flex flex-col justify-between h-[285px]">
                    <div className="flex items-center justify-between pb-2 border-b border-black/[0.03] dark:border-base-200">
                        <div className="flex items-center gap-2">
                            <div className="w-6 h-6 rounded-full bg-amber-500/10 flex items-center justify-center text-amber-500">
                                <Sparkles className="w-3.5 h-3.5" />
                            </div>
                            <span className="text-sm font-semibold tracking-tight text-gray-900 dark:text-base-content">
                                {t('token_stats.rhythm_24h', '24 小时节奏律动')}
                            </span>
                        </div>
                        <span className="text-xs font-mono text-gray-400 dark:text-gray-500">
                            {activeHourData ? (
                                <span className="text-[#0071e3] font-semibold">
                                    {activeHourData.hourLabel} · {formatNumber(activeHourData.data.total_tokens)}
                                </span>
                            ) : (
                                t('token_stats.peak_and_total', '峰值: {{peak}} · 全天: {{total}}', { peak: peakHourLabel, total: formatNumber(totalPast24h) })
                            )}
                        </span>
                    </div>

                    {/* 24 根纤细优雅律动柱条 */}
                    <div className="flex items-end gap-1 px-1 py-1.5 h-16">
                        {hourBuckets.map((b, idx) => {
                            const isHovered = hoveredHourIdx === idx;
                            const hasValue = b.data.total_tokens > 0;
                            const heightPct = hasValue ? Math.max(14, (b.data.total_tokens / maxHourTokens) * 100) : 0;
                            return (
                                <div
                                    key={b.full}
                                    onMouseEnter={() => setHoveredHourIdx(idx)}
                                    onMouseLeave={() => setHoveredHourIdx(null)}
                                    className="flex-1 h-full flex flex-col justify-end items-center cursor-pointer group"
                                    title={`${b.hourLabel}: ${formatNumber(b.data.total_tokens)} Tokens`}
                                >
                                    {hasValue ? (
                                        <motion.div
                                            style={{ height: `${heightPct}%` }}
                                            className={`w-full rounded-full transition-all ${
                                                isHovered
                                                    ? 'bg-[#0071e3] scale-110 shadow-[0_0_12px_rgba(0,113,227,0.5)]'
                                                    : 'bg-gradient-to-t from-[#0071e3]/70 to-[#2997ff]'
                                            }`}
                                        />
                                    ) : (
                                        <div className="w-full h-1 bg-gray-200/60 dark:bg-base-200 rounded-full" />
                                    )}
                                </div>
                            );
                        })}
                    </div>

                    {/* 宏观效能诊断胶囊双格 */}
                    <div className="grid grid-cols-2 gap-2.5 py-1">
                        <div className="bg-gray-50/80 dark:bg-base-200 rounded-2xl p-2.5 flex flex-col justify-between">
                            <span className="text-[11px] text-gray-400 dark:text-gray-400">{t('token_stats.avg_interaction', '平均单次交互量')}</span>
                            <span className="text-base font-bold font-mono text-gray-900 dark:text-base-content mt-0.5">
                                {summary && summary.total_requests > 0
                                    ? formatNumber(Math.round(summary.total_tokens / summary.total_requests))
                                    : '0'} <span className="text-[10px] font-normal text-gray-400">Tokens</span>
                            </span>
                        </div>

                        <div className="bg-gray-50/80 dark:bg-base-200 rounded-2xl p-2.5 flex flex-col justify-between">
                            <span className="text-[11px] text-gray-400 dark:text-gray-400">{t('token_stats.cost_per_million', '单位百万人次成本')}</span>
                            <span className="text-base font-bold font-mono text-emerald-600 dark:text-emerald-400 mt-0.5">
                                {summary && summary.total_tokens > 0
                                    ? `$${((totalEstimatedCost / summary.total_tokens) * 1_000_000).toFixed(2)}`
                                    : '$0.00'} <span className="text-[10px] font-normal text-gray-400">/ 1M</span>
                            </span>
                        </div>
                    </div>

                    <div className="flex items-center justify-between text-[11px] font-mono text-gray-400 dark:text-gray-500 pt-2 border-t border-black/[0.03] dark:border-base-200">
                        <span>{t('token_stats.pipeline_accel', 'Antigravity 智能流水线加速')}</span>
                        <span className="text-emerald-500">{t('token_stats.high_avail_ready', '高可用负载就绪')}</span>
                    </div>
                </div>
            </div>

            {/* ================= 3. Apple Bento 经典 50/50 严格对齐双栏 (模型负载 & 账号排位) ================= */}
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                {/* 左卡：核心模型负载分布 (Apple HIG 渐进条) */}
                <div className="bg-white/90 dark:bg-base-100 backdrop-blur-xl rounded-3xl p-5 border border-black/[0.04] dark:border-base-200 shadow-sm flex flex-col justify-between h-[310px]">
                    <div className="flex items-center justify-between pb-3 border-b border-black/[0.03] dark:border-base-200">
                        <div className="flex items-center gap-2">
                            <div className="w-6 h-6 rounded-full bg-[#0071e3]/10 dark:bg-[#0071e3]/20 flex items-center justify-center text-[#0071e3]">
                                <Cpu className="w-3.5 h-3.5" />
                            </div>
                            <span className="text-sm font-semibold tracking-tight text-gray-900 dark:text-base-content">
                                {t('token_stats.model_share_title', '核心模型生态占比')}
                            </span>
                        </div>
                        <span className="text-xs font-mono text-gray-400 dark:text-gray-500">
                            {t('token_stats.models_count', '共 {{count}} 个模型', { count: modelShareItems.length })}
                        </span>
                    </div>

                    {/* 列表区 */}
                    <div className="flex-1 flex flex-col justify-around py-2">
                        {topModels.length === 0 ? (
                            <div className="text-center text-xs text-gray-400 dark:text-gray-500 py-6">
                                {t('token_stats.no_model_activity', '暂无模型活动')}
                            </div>
                        ) : (
                            topModels.map((item, idx) => {
                                const total = modelShareItems.reduce((acc, cur) => acc + cur.total_tokens, 0);
                                const pct = total > 0 ? (item.total_tokens / total) * 100 : 0;
                                const color = CHART_PALETTE[idx % CHART_PALETTE.length];
                                return (
                                    <div key={item.model} className="flex items-center justify-between gap-3 text-xs">
                                        <div className="flex items-center gap-2 min-w-0 flex-1">
                                            <span className="w-2 h-2 rounded-full flex-shrink-0" style={{ backgroundColor: color }} />
                                            <span className="font-mono text-gray-800 dark:text-gray-200 truncate text-[11.5px] font-medium" title={item.model}>
                                                {item.model}
                                            </span>
                                            {item.avg_ttft_ms && item.avg_ttft_ms > 0 ? (
                                                <span className="text-[10px] font-mono text-teal-600 dark:text-teal-400 bg-teal-50 dark:bg-teal-950/40 px-1 py-0.2 rounded shrink-0" title={t('token_stats.avg_ttft_tip', '平均首字响应耗时')}>
                                                    {item.avg_ttft_ms < 1000 ? `${item.avg_ttft_ms.toFixed(0)}ms` : `${(item.avg_ttft_ms / 1000).toFixed(2)}s`}
                                                </span>
                                            ) : null}
                                        </div>
                                        <div className="w-28 sm:w-36 h-2 bg-gray-100 dark:bg-base-200 rounded-full overflow-hidden flex-shrink-0">
                                            <motion.div
                                                initial={{ width: 0 }}
                                                animate={{ width: `${pct}%` }}
                                                transition={{ duration: 0.4 }}
                                                className="h-full rounded-full"
                                                style={{ backgroundColor: color }}
                                            />
                                        </div>
                                        <div className="w-20 text-right font-mono flex items-baseline justify-end gap-1.5 flex-shrink-0">
                                            <span className="text-gray-900 dark:text-base-content font-semibold">
                                                {formatNumber(item.total_tokens)}
                                            </span>
                                            <span className="text-[10px] text-gray-400">
                                                {pct.toFixed(0)}%
                                            </span>
                                        </div>
                                    </div>
                                );
                            })
                        )}
                    </div>

                    <div className="flex items-center justify-between text-[11px] font-mono text-gray-400 dark:text-gray-500 pt-2.5 border-t border-black/[0.03] dark:border-base-200">
                        <span>{t('token_stats.top_models_load', 'Top 5 模型承载大部负荷')}</span>
                        <span>{t('token_stats.cost_accumulated', '费用累计: ${{amount}}', { amount: topModels.reduce((acc, cur) => acc + cur.cost, 0).toFixed(2) })}</span>
                    </div>
                </div>

                {/* 右卡：账号活跃排行 (等高 310px 1:1 对齐) */}
                <div className="bg-white/90 dark:bg-base-100 backdrop-blur-xl rounded-3xl p-5 border border-black/[0.04] dark:border-base-200 shadow-sm flex flex-col justify-between h-[310px]">
                    <div className="flex items-center justify-between pb-3 border-b border-black/[0.03] dark:border-base-200">
                        <div className="flex items-center gap-2">
                            <div className="w-6 h-6 rounded-full bg-[#af52de]/10 dark:bg-[#af52de]/20 flex items-center justify-center text-[#af52de]">
                                <Users className="w-3.5 h-3.5" />
                            </div>
                            <span className="text-sm font-semibold tracking-tight text-gray-900 dark:text-base-content">
                                {t('token_stats.top_accounts_title', '活跃账号消耗榜')}
                            </span>
                        </div>
                        <span className="text-xs font-mono text-gray-400 dark:text-gray-500">
                            {t('token_stats.accounts_count', '共 {{count}} 个账号', { count: accountData.length })}
                        </span>
                    </div>

                    {/* 列表区 */}
                    <div className="flex-1 flex flex-col justify-around py-2">
                        {topAccounts.length === 0 ? (
                            <div className="text-center text-xs text-gray-400 dark:text-gray-500 py-6">
                                {t('token_stats.no_account_activity', '暂无账号调用')}
                            </div>
                        ) : (
                            topAccounts.map((acc, idx) => {
                                const total = accountData.reduce((sum, a) => sum + a.total_tokens, 0);
                                const pct = total > 0 ? (acc.total_tokens / total) * 100 : 0;
                                const shortEmail = acc.account_email.split('@')[0];
                                return (
                                    <div key={acc.account_email} className="flex items-center justify-between gap-3 text-xs">
                                        <div className="flex items-center gap-2 min-w-0 flex-1">
                                            <span className="w-5 h-5 rounded-full bg-gray-100 dark:bg-base-200 text-gray-600 dark:text-gray-300 flex items-center justify-center font-mono text-[10px] font-semibold flex-shrink-0">
                                                {idx + 1}
                                            </span>
                                            <span className="font-mono text-gray-800 dark:text-gray-200 truncate text-[11.5px]" title={acc.account_email}>
                                                {shortEmail}
                                            </span>
                                        </div>
                                        <div className="w-28 sm:w-36 h-2 bg-gray-100 dark:bg-base-200 rounded-full overflow-hidden flex-shrink-0">
                                            <motion.div
                                                initial={{ width: 0 }}
                                                animate={{ width: `${pct}%` }}
                                                transition={{ duration: 0.4 }}
                                                className="h-full bg-gradient-to-r from-[#af52de] to-[#5856d6] rounded-full"
                                            />
                                        </div>
                                        <div className="w-20 text-right font-mono flex items-baseline justify-end gap-1.5 flex-shrink-0">
                                            <span className="text-gray-900 dark:text-base-content font-semibold">
                                                {formatNumber(acc.total_tokens)}
                                            </span>
                                            <span className="text-[10px] text-gray-400">
                                                {acc.request_count} {t('token_stats.times', '次')}
                                            </span>
                                        </div>
                                    </div>
                                );
                            })
                        )}
                    </div>

                    <div className="flex items-center justify-between text-[11px] font-mono text-gray-400 dark:text-gray-500 pt-2.5 border-t border-black/[0.03] dark:border-base-200">
                        <span>{t('token_stats.account_isolation', '账号层级隔离安全')}</span>
                        <span>{t('token_stats.hit_accounts_count', '总命中账号: {{count}}', { count: accountData.length })}</span>
                    </div>
                </div>
            </div>

            {/* ================= 3. Apple 宽幅时序演变全景趋势 Bento ================= */}
            <div className="bg-white/90 dark:bg-base-100 backdrop-blur-xl rounded-3xl p-5 border border-black/[0.04] dark:border-base-200 shadow-sm">
                <div className="flex flex-wrap items-center justify-between gap-3 mb-4 pb-3 border-b border-black/[0.03] dark:border-base-200">
                    <div className="flex items-center gap-2">
                        <div className="w-6 h-6 rounded-full bg-[#0071e3]/10 dark:bg-[#0071e3]/20 flex items-center justify-center text-[#0071e3]">
                            <TrendingUp className="w-3.5 h-3.5" />
                        </div>
                        <span className="text-sm font-semibold tracking-tight text-gray-900 dark:text-base-content">
                            {viewMode === 'model'
                                ? metricType === 'cost'
                                    ? t('token_stats.model_cost_trend_evolution', '分模型支出时序演进')
                                    : t('token_stats.model_token_trend_evolution', '分模型 Token 吞吐演进')
                                : metricType === 'cost'
                                    ? t('token_stats.account_cost_trend_evolution', '分账号支出时序演进')
                                    : t('token_stats.account_token_trend_evolution', '分账号 Token 吞吐演进')}
                        </span>
                    </div>

                    {/* Apple 经典分段胶囊控制器 (Segmented Controls) */}
                    <div className="flex flex-wrap items-center gap-2">
                        {/* 柱状 / 面积 */}
                        <div className="flex bg-gray-100/90 dark:bg-base-200 border border-gray-200/80 dark:border-base-300 rounded-full p-0.5 text-xs">
                            <button
                                onClick={() => switchChartType('area')}
                                className={`px-3 py-1 rounded-full transition-all font-medium ${chartType === 'area' ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content shadow-sm' : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'}`}
                            >
                                {t('token_stats.smooth_area', '平滑面积')}
                            </button>
                            <button
                                onClick={() => switchChartType('bar')}
                                className={`px-3 py-1 rounded-full transition-all font-medium ${chartType === 'bar' ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content shadow-sm' : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'}`}
                            >
                                {t('token_stats.stacked_bar', '堆叠柱状')}
                            </button>
                        </div>

                        {/* Token / 金额 */}
                        <div className="flex bg-gray-100/90 dark:bg-base-200 border border-gray-200/80 dark:border-base-300 rounded-full p-0.5 text-xs">
                            <button
                                onClick={() => switchMetricType('tokens')}
                                className={`px-3 py-1 rounded-full transition-all font-medium ${metricType === 'tokens' ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content shadow-sm' : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'}`}
                            >
                                {t('token_stats.metric_tokens', 'Token')}
                            </button>
                            <button
                                onClick={() => switchMetricType('cost')}
                                className={`px-3 py-1 rounded-full transition-all font-medium ${metricType === 'cost' ? 'bg-white dark:bg-base-100 text-emerald-600 dark:text-emerald-400 shadow-sm' : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'}`}
                            >
                                {t('token_stats.metric_cost_symbol', '金额 ($)')}
                            </button>
                        </div>

                        {/* 模型 / 账号 */}
                        <div className="flex bg-gray-100/90 dark:bg-base-200 border border-gray-200/80 dark:border-base-300 rounded-full p-0.5 text-xs">
                            <button
                                onClick={() => switchViewMode('model')}
                                className={`px-3 py-1 rounded-full transition-all font-medium ${viewMode === 'model' ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content shadow-sm' : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'}`}
                            >
                                {t('token_stats.by_model', '按模型')}
                            </button>
                            <button
                                onClick={() => switchViewMode('account')}
                                className={`px-3 py-1 rounded-full transition-all font-medium ${viewMode === 'account' ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content shadow-sm' : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'}`}
                            >
                                {t('token_stats.by_account_view', '按账号')}
                            </button>
                        </div>
                    </div>
                </div>

                <div className="h-64">
                    <ResponsiveContainer width="100%" height="100%">
                        {chartType === 'area' ? (
                            <AreaChart data={viewMode === 'model' ? displayModelTrendData : displayAccountTrendData}>
                                <defs>
                                    {(viewMode === 'model' ? allModels : allAccounts).map((key, i) => (
                                        <linearGradient key={`apple-grad-${key}`} id={`apple-grad-${i}`} x1="0" y1="0" x2="0" y2="1">
                                            <stop offset="5%" stopColor={CHART_PALETTE[i % CHART_PALETTE.length]} stopOpacity={0.35} />
                                            <stop offset="95%" stopColor={CHART_PALETTE[i % CHART_PALETTE.length]} stopOpacity={0.0} />
                                        </linearGradient>
                                    ))}
                                </defs>
                                <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="rgba(150, 150, 150, 0.1)" />
                                <XAxis
                                    dataKey="period"
                                    tick={{ fontSize: 10, fill: '#8e8e93', fontFamily: 'monospace' }}
                                    tickFormatter={(val) => {
                                        if (timeRange === 'minute') {
                                            const parts = val.split(' ');
                                            return parts.length > 1 ? parts[1] : val;
                                        }
                                        if (timeRange === 'hourly') return val.split(' ')[1] || val;
                                        if (timeRange === 'daily') return val.split('-').slice(1).join('/');
                                        if (timeRange === 'weekly') {
                                            const parts = val.split('-W');
                                            if (parts.length > 1) {
                                                const weekNum = parseInt(parts[1], 10);
                                                return t('token_stats.week_number', 'Week {{week}}', { week: weekNum });
                                            }
                                            return val;
                                        }
                                        if (timeRange === 'all') return val.split('-').slice(1).join('/');
                                        return val;
                                    }}
                                    axisLine={false}
                                    tickLine={false}
                                />
                                <YAxis
                                    tick={{ fontSize: 10, fill: '#8e8e93', fontFamily: 'monospace' }}
                                    tickFormatter={(val) => metricType === 'cost' ? formatCurrency(val) : formatNumber(val)}
                                    axisLine={false}
                                    tickLine={false}
                                />
                                <Tooltip content={<CustomTrendTooltip />} />
                                {(viewMode === 'model' ? allModels : allAccounts).map((key, i) => (
                                    <Area
                                        key={key}
                                        type="monotone"
                                        dataKey={key}
                                        stackId="1"
                                        stroke={CHART_PALETTE[i % CHART_PALETTE.length]}
                                        strokeWidth={2}
                                        fill={`url(#apple-grad-${i})`}
                                        isAnimationActive={false}
                                    />
                                ))}
                            </AreaChart>
                        ) : (
                            <BarChart data={viewMode === 'model' ? displayModelTrendData : displayAccountTrendData}>
                                <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="rgba(150, 150, 150, 0.1)" />
                                <XAxis
                                    dataKey="period"
                                    tick={{ fontSize: 10, fill: '#8e8e93', fontFamily: 'monospace' }}
                                    tickFormatter={(val) => {
                                        if (timeRange === 'minute') {
                                            const parts = val.split(' ');
                                            return parts.length > 1 ? parts[1] : val;
                                        }
                                        if (timeRange === 'hourly') return val.split(' ')[1] || val;
                                        if (timeRange === 'daily') return val.split('-').slice(1).join('/');
                                        if (timeRange === 'weekly') {
                                            const parts = val.split('-W');
                                            if (parts.length > 1) {
                                                const weekNum = parseInt(parts[1], 10);
                                                return t('token_stats.week_number', 'Week {{week}}', { week: weekNum });
                                            }
                                            return val;
                                        }
                                        if (timeRange === 'all') return val.split('-').slice(1).join('/');
                                        return val;
                                    }}
                                    axisLine={false}
                                    tickLine={false}
                                />
                                <YAxis
                                    tick={{ fontSize: 10, fill: '#8e8e93', fontFamily: 'monospace' }}
                                    tickFormatter={(val) => metricType === 'cost' ? formatCurrency(val) : formatNumber(val)}
                                    axisLine={false}
                                    tickLine={false}
                                />
                                <Tooltip content={<CustomTrendTooltip />} cursor={{ fill: 'rgba(150, 150, 150, 0.05)' }} />
                                {(viewMode === 'model' ? allModels : allAccounts).map((key, i) => (
                                    <Bar
                                        key={key}
                                        dataKey={key}
                                        stackId="stack"
                                        fill={CHART_PALETTE[i % CHART_PALETTE.length]}
                                        radius={[4, 4, 0, 0]}
                                        maxBarSize={28}
                                        isAnimationActive={false}
                                    />
                                ))}
                            </BarChart>
                        )}
                    </ResponsiveContainer>
                </div>
            </div>
        </div>
    );
};
