import React, { useEffect, useState, useRef, useCallback, useMemo } from 'react';
import { request as invoke } from '../utils/request';
import { useTranslation } from 'react-i18next';
import {
    BarChart,
    Bar,
    AreaChart,
    Area,
    XAxis,
    YAxis,
    CartesianGrid,
    Tooltip,
    ResponsiveContainer,
    PieChart,
    Pie,
    Cell,
} from 'recharts';
import {
    Clock,
    Calendar,
    CalendarDays,
    History,
    Users,
    Zap,
    TrendingUp,
    RefreshCw,
    Cpu,
    DollarSign,
    Settings2,
    Database,
    BarChart3,
    Sparkles,
} from 'lucide-react';
import { ActivityHeatmapCard, DayUsagePoint } from '../components/token_stats/ActivityHeatmapCard';
import { HourlyTrendBarCard, HourlyUsagePoint } from '../components/token_stats/HourlyTrendBarCard';
import { TopModelsShareCard, ModelShareItem } from '../components/token_stats/TopModelsShareCard';
import { PricingModal } from '../components/token_stats/PricingModal';

export interface TokenStatsAggregated {
    period: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_tokens: number;
    request_count: number;
    uncached_input_tokens?: number;
}

export interface AccountTokenStats {
    account_email: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_tokens: number;
    request_count: number;
}

export interface ModelTokenStats {
    model: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_tokens: number;
    request_count: number;
}

export interface ModelTrendPoint {
    period: string;
    model_data: Record<string, number>;
}

export interface AccountTrendPoint {
    period: string;
    account_data: Record<string, number>;
}

export interface TokenStatsSummary {
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_tokens: number;
    total_requests: number;
    unique_accounts: number;
}

export interface ModelPricingRule {
    input: number; // $ per 1M tokens
    output: number; // $ per 1M tokens
    cached: number; // $ per 1M tokens
}

export const DEFAULT_PRICING: Record<string, ModelPricingRule> = {
    'gemini-3.7-flash': { input: 0.75, output: 3.75, cached: 0.075 },
    'gemini-3.6-flash': { input: 0.75, output: 3.75, cached: 0.075 },
    'gemini-3.5-flash': { input: 0.75, output: 3.75, cached: 0.075 },
    'gemini-2.5-flash': { input: 0.75, output: 3.75, cached: 0.075 },
    'gemini-3.1-pro': { input: 2.00, output: 12.00, cached: 0.20 },
    'gemini-3-pro': { input: 2.00, output: 12.00, cached: 0.20 },
    'gemini-2.5-pro': { input: 2.00, output: 12.00, cached: 0.20 },
    'claude-sonnet-4-6': { input: 3.00, output: 15.00, cached: 0.30 },
    'claude-sonnet-4.6': { input: 3.00, output: 15.00, cached: 0.30 },
    'claude-3-7-sonnet': { input: 3.00, output: 15.00, cached: 0.30 },
    'claude-3-5-sonnet': { input: 3.00, output: 15.00, cached: 0.30 },
    'claude-opus-4-6': { input: 5.00, output: 25.00, cached: 0.50 },
    'claude-opus-4.6': { input: 5.00, output: 25.00, cached: 0.50 },
    'claude-3-opus': { input: 5.00, output: 25.00, cached: 0.50 },
    'claude-haiku-4': { input: 0.80, output: 4.00, cached: 0.08 },
    'claude-3-5-haiku': { input: 0.80, output: 4.00, cached: 0.08 },
    'default': { input: 1.00, output: 5.00, cached: 0.10 },
};

const PRICING_STORAGE_KEY = 'antigravity_model_pricing_v2026';
const DASHBOARD_MODE_KEY = 'antigravity_token_stats_dashboard_mode';
const TIME_RANGE_KEY = 'antigravity_token_stats_time_range';
const VIEW_MODE_KEY = 'antigravity_token_stats_view_mode';
const CHART_TYPE_KEY = 'antigravity_token_stats_chart_type';
const METRIC_TYPE_KEY = 'antigravity_token_stats_metric_type';

type TimeRange = 'hourly' | 'daily' | 'weekly' | 'all';
type ViewMode = 'model' | 'account';
type DashboardMode = 'insights' | 'classic';
type MetricType = 'tokens' | 'cost';

const TOP_COLORS = ['#d97757', '#4f85e8', '#10a37f', '#a855f7', '#f59e0b', '#71717a'];
const PIE_COLORS = ['#3b82f6', '#8b5cf6', '#ec4899', '#f59e0b', '#10b981', '#06b6d4', '#6366f1', '#f43f5e'];

const formatNumber = (num: number): string => {
    if (num >= 1000000000) return `${(num / 1000000000).toFixed(1)}B`;
    if (num >= 1000000) return `${(num / 1000000).toFixed(1)}M`;
    if (num >= 1000) return `${(num / 1000).toFixed(1)}K`;
    return num.toString();
};

const formatCurrency = (val: number): string => {
    if (val >= 1000000) return `$${(val / 1000000).toFixed(2)}M`;
    if (val >= 1000) return `$${(val / 1000).toFixed(1)}K`;
    if (val >= 100) return `$${val.toFixed(1)}`;
    if (val >= 1) return `$${val.toFixed(2)}`;
    if (val > 0) return `$${val.toFixed(3)}`;
    return '$0';
};

const getSqliteWeekBucket = (dateStr: string): string => {
    const parts = dateStr.split('-');
    if (parts.length < 3) return dateStr;
    const year = parseInt(parts[0], 10);
    const month = parseInt(parts[1], 10) - 1;
    const day = parseInt(parts[2], 10);
    const d = new Date(year, month, day);

    const firstDayOfYear = new Date(year, 0, 1);
    const dayOfWeek = firstDayOfYear.getDay();
    const daysToFirstMonday = (8 - (dayOfWeek || 7)) % 7;
    const diffDays = Math.round((d.getTime() - firstDayOfYear.getTime()) / (24 * 3600 * 1000));
    let week = 0;
    if (diffDays >= daysToFirstMonday) {
        week = Math.floor((diffDays - daysToFirstMonday) / 7) + 1;
    }
    return `${year}-W${String(week).padStart(2, '0')}`;
};

const TokenStats: React.FC = () => {
    const { t } = useTranslation();
    const [timeRange, setTimeRange] = useState<TimeRange>(() => {
        try {
            const saved = localStorage.getItem(TIME_RANGE_KEY);
            if (saved === 'hourly' || saved === 'daily' || saved === 'weekly' || saved === 'all') return saved;
        } catch (e) {}
        return 'daily';
    });

    const switchTimeRange = useCallback((range: TimeRange) => {
        setTimeRange(range);
        try {
            localStorage.setItem(TIME_RANGE_KEY, range);
        } catch (e) {}
    }, []);

    const [viewMode, setViewMode] = useState<ViewMode>(() => {
        try {
            const saved = localStorage.getItem(VIEW_MODE_KEY);
            if (saved === 'model' || saved === 'account') return saved;
        } catch (e) {}
        return 'model';
    });

    const switchViewMode = useCallback((mode: ViewMode) => {
        setViewMode(mode);
        try {
            localStorage.setItem(VIEW_MODE_KEY, mode);
        } catch (e) {}
    }, []);

    const [chartType, setChartType] = useState<'bar' | 'area'>(() => {
        try {
            const saved = localStorage.getItem(CHART_TYPE_KEY);
            if (saved === 'bar' || saved === 'area') return saved;
        } catch (e) {}
        return 'bar';
    });

    const switchChartType = useCallback((type: 'bar' | 'area') => {
        setChartType(type);
        try {
            localStorage.setItem(CHART_TYPE_KEY, type);
        } catch (e) {}
    }, []);

    const [metricType, setMetricType] = useState<MetricType>(() => {
        try {
            const saved = localStorage.getItem(METRIC_TYPE_KEY);
            if (saved === 'tokens' || saved === 'cost') return saved;
        } catch (e) {}
        return 'tokens';
    });

    const switchMetricType = useCallback((metric: MetricType) => {
        setMetricType(metric);
        try {
            localStorage.setItem(METRIC_TYPE_KEY, metric);
        } catch (e) {}
    }, []);

    const [dashboardMode, setDashboardMode] = useState<DashboardMode>(() => {
        try {
            const saved = localStorage.getItem(DASHBOARD_MODE_KEY);
            if (saved === 'classic' || saved === 'insights') return saved;
        } catch (e) {}
        return 'insights';
    });

    const switchDashboardMode = useCallback((mode: DashboardMode) => {
        setDashboardMode(mode);
        try {
            localStorage.setItem(DASHBOARD_MODE_KEY, mode);
        } catch (e) {}
    }, []);

    const [accountData, setAccountData] = useState<AccountTokenStats[]>([]);
    const [modelData, setModelData] = useState<ModelTokenStats[]>([]);
    const [modelTrendData, setModelTrendData] = useState<any[]>([]);
    const [accountTrendData, setAccountTrendData] = useState<any[]>([]);
    const [allModels, setAllModels] = useState<string[]>([]);
    const [allAccounts, setAllAccounts] = useState<string[]>([]);
    const [summary, setSummary] = useState<TokenStatsSummary | null>(null);
    const [loading, setLoading] = useState(true);

    // 经典模式专属堆叠图数据
    const [classicChartData, setClassicChartData] = useState<TokenStatsAggregated[]>([]);

    // Insights 模式专属热力图与 24 小时紧凑柱状图数据
    const [heatmapData, setHeatmapData] = useState<DayUsagePoint[]>([]);
    const [hourlyTrendData, setHourlyTrendData] = useState<HourlyUsagePoint[]>([]);

    // 价格体系与自定义价格状态
    const [pricing, setPricing] = useState<Record<string, ModelPricingRule>>(() => {
        try {
            const saved = localStorage.getItem(PRICING_STORAGE_KEY);
            if (saved) return JSON.parse(saved);
        } catch (e) {}
        return DEFAULT_PRICING;
    });
    const [showPricingModal, setShowPricingModal] = useState(false);

    const getPricing = useCallback((modelName: string): ModelPricingRule => {
        const key = modelName.toLowerCase();
        if (pricing[key]) return pricing[key];
        for (const [pattern, rule] of Object.entries(pricing)) {
            if (pattern !== 'default' && key.includes(pattern)) {
                return rule;
            }
        }
        return pricing['default'] || DEFAULT_PRICING['default'];
    }, [pricing]);

    const calculateModelCost = useCallback((m: ModelTokenStats): number => {
        const p = getPricing(m.model);
        const uncachedInput = Math.max(0, m.total_input_tokens - (m.total_cached_tokens || 0));
        const inputCost = (uncachedInput * p.input) / 1_000_000;
        const cacheCost = ((m.total_cached_tokens || 0) * p.cached) / 1_000_000;
        const outputCost = (m.total_output_tokens * p.output) / 1_000_000;
        return inputCost + cacheCost + outputCost;
    }, [getPricing]);

    const totalEstimatedCost = useMemo(() => {
        return modelData.reduce((sum, m) => sum + calculateModelCost(m), 0);
    }, [modelData, calculateModelCost]);

    // 主力模型列表数据（带单项费用）
    const modelShareItems: ModelShareItem[] = useMemo(() => {
        return modelData.map(m => ({
            model: m.model,
            total_tokens: m.total_tokens,
            request_count: m.request_count,
            total_input_tokens: m.total_input_tokens,
            total_output_tokens: m.total_output_tokens,
            total_cached_tokens: m.total_cached_tokens,
            cost: calculateModelCost(m),
        }));
    }, [modelData, calculateModelCost]);

    // 经典模式分账号饼图数据
    const pieData = useMemo(() => {
        return accountData.slice(0, 8).map((account, index) => ({
            name: account.account_email.split('@')[0] + '...',
            value: account.total_tokens,
            fullEmail: account.account_email,
            color: PIE_COLORS[index % PIE_COLORS.length],
        }));
    }, [accountData]);

    // 缓存命中率与节约成本估算
    const cacheMetrics = useMemo(() => {
        if (!summary || summary.total_tokens === 0) return { hitRate: 0, savings: 0 };
        const totalInput = summary.total_input_tokens;
        const cached = summary.total_cached_tokens;
        const hitRate = totalInput > 0 ? (cached / totalInput) * 100 : 0;
        // 估算节约：假设平均每 1M 缓存相比直接输入节省约 $1.50
        const savings = (cached * 1.5) / 1_000_000;
        return { hitRate, savings };
    }, [summary]);

    const fetchData = async () => {
        setLoading(true);
        try {
            let hours = 24;
            let modelTrend: ModelTrendPoint[] = [];
            let accountTrend: AccountTrendPoint[] = [];
            let classicDataPromise: Promise<TokenStatsAggregated[]>;

            switch (timeRange) {
                case 'hourly':
                    hours = 24;
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_hourly', { hours: 24 });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_hourly', { hours: 24 });
                    classicDataPromise = invoke<TokenStatsAggregated[]>('get_token_stats_hourly', { hours: 24 });
                    break;
                case 'daily':
                    hours = 336; // 14 天 (2 周)
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_daily', { days: 14 });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_daily', { days: 14 });
                    classicDataPromise = invoke<TokenStatsAggregated[]>('get_token_stats_daily', { days: 14 });
                    break;
                case 'weekly':
                    hours = 2016; // 12 周 (约 3 个月 = 84 天)
                    const rawDailyModelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_daily', { days: 84 });
                    const rawDailyAccountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_daily', { days: 84 });
                    classicDataPromise = invoke<TokenStatsAggregated[]>('get_token_stats_weekly', { weeks: 12 });

                    // 按周合并聚合 modelTrend
                    const weeklyModelMap = new Map<string, Record<string, number>>();
                    for (const pt of rawDailyModelTrend) {
                        const wb = getSqliteWeekBucket(pt.period);
                        if (!weeklyModelMap.has(wb)) weeklyModelMap.set(wb, {});
                        const acc = weeklyModelMap.get(wb)!;
                        for (const [m, count] of Object.entries(pt.model_data)) {
                            acc[m] = (acc[m] || 0) + count;
                        }
                    }
                    modelTrend = Array.from(weeklyModelMap.entries())
                        .sort(([a], [b]) => a.localeCompare(b))
                        .map(([period, model_data]) => ({ period, model_data }));

                    // 按周合并聚合 accountTrend
                    const weeklyAccountMap = new Map<string, Record<string, number>>();
                    for (const pt of rawDailyAccountTrend) {
                        const wb = getSqliteWeekBucket(pt.period);
                        if (!weeklyAccountMap.has(wb)) weeklyAccountMap.set(wb, {});
                        const acc = weeklyAccountMap.get(wb)!;
                        for (const [a, count] of Object.entries(pt.account_data)) {
                            acc[a] = (acc[a] || 0) + count;
                        }
                    }
                    accountTrend = Array.from(weeklyAccountMap.entries())
                        .sort(([a], [b]) => a.localeCompare(b))
                        .map(([period, account_data]) => ({ period, account_data }));
                    break;
                case 'all':
                    hours = 0; // 0 表示查询全部历史
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_daily', { days: 0 });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_daily', { days: 0 });
                    classicDataPromise = invoke<TokenStatsAggregated[]>('get_token_stats_daily', { days: 0 });
                    break;
            }

            // 处理模型趋势数据：提取 Top 5 模型，长尾自动归并到 Other，避免图例过多导致排版杂乱
            const modelsSet = new Set<string>();
            modelTrend.forEach(point => {
                Object.keys(point.model_data).forEach(m => modelsSet.add(m));
            });
            const modelTotals: Record<string, number> = {};
            modelTrend.forEach(p => {
                Object.entries(p.model_data).forEach(([m, val]) => {
                    modelTotals[m] = (modelTotals[m] || 0) + val;
                });
            });
            const sortedModels = Array.from(modelsSet).sort((a, b) => (modelTotals[b] || 0) - (modelTotals[a] || 0));
            const top5Models = sortedModels.slice(0, 5);
            const hasOther = sortedModels.length > 5;
            const otherModelKey = t('token_stats.other_models', 'Other Models');
            const displayModelKeys = hasOther ? [...top5Models, otherModelKey] : top5Models;
            setAllModels(displayModelKeys);

            const transformedTrend = modelTrend.map(point => {
                const row: Record<string, any> = { period: point.period };
                let otherSum = 0;
                Object.entries(point.model_data).forEach(([model, val]) => {
                    if (top5Models.includes(model)) {
                        row[model] = val;
                    } else {
                        otherSum += val;
                    }
                });
                if (hasOther) {
                    row[otherModelKey] = otherSum;
                }
                return row;
            });
            setModelTrendData(transformedTrend);

            // 处理账号趋势数据
            const accountsSet = new Set<string>();
            accountTrend.forEach(point => {
                Object.keys(point.account_data).forEach(acc => accountsSet.add(acc));
            });
            const accountList = Array.from(accountsSet);
            setAllAccounts(accountList);

            const transformedAccountTrend = accountTrend.map(point => {
                const row: Record<string, any> = { period: point.period };
                accountList.forEach(acc => {
                    row[acc] = point.account_data[acc] || 0;
                });
                return row;
            });
            setAccountTrendData(transformedAccountTrend);

            // 并行拉取账户统计、模型统计、全局概览、经典看板堆叠数据，以及洞察看板的 91 天热力图与 24 小时柱状图
            const [accounts, models_stats, summaryData, classicData, raw91Days, raw24Hours] = await Promise.all([
                invoke<AccountTokenStats[]>('get_token_stats_by_account', { hours }),
                invoke<ModelTokenStats[]>('get_token_stats_by_model', { hours }),
                invoke<TokenStatsSummary>('get_token_stats_summary', { hours }),
                classicDataPromise,
                invoke<TokenStatsAggregated[]>('get_token_stats_daily', { days: 91 }),
                invoke<TokenStatsAggregated[]>('get_token_stats_hourly', { hours: 24 }),
            ]);

            setAccountData(accounts);
            setModelData(models_stats);
            setSummary(summaryData);

            // 转换经典看板堆叠柱状图数据
            setClassicChartData(
                classicData.map(point => ({
                    ...point,
                    total_cached_tokens: point.total_cached_tokens || 0,
                    uncached_input_tokens: Math.max((point.total_input_tokens || 0) - (point.total_cached_tokens || 0), 0),
                }))
            );

            // 转换 91 天热力图点
            setHeatmapData(
                raw91Days.map(d => ({
                    date: d.period,
                    total_tokens: d.total_tokens,
                    request_count: d.request_count,
                    total_input_tokens: d.total_input_tokens,
                    total_output_tokens: d.total_output_tokens,
                    total_cached_tokens: d.total_cached_tokens,
                }))
            );

            // 转换 24 小时柱状图点
            setHourlyTrendData(
                raw24Hours.map(h => ({
                    hour: h.period,
                    total_tokens: h.total_tokens,
                    input_tokens: h.total_input_tokens,
                    output_tokens: h.total_output_tokens,
                    cached_tokens: h.total_cached_tokens,
                    request_count: h.request_count,
                }))
            );
        } catch (error) {
            console.error('Failed to fetch token stats:', error);
        } finally {
            setLoading(false);
        }
    };

    useEffect(() => {
        fetchData();
    }, [timeRange]);

    // 计算 24 小时预估金额
    const hourly24hEstimatedCost = useMemo(() => {
        if (!summary || summary.total_tokens === 0) return 0;
        const avgTokenRate = totalEstimatedCost / summary.total_tokens;
        const past24hTokens = hourlyTrendData.reduce((sum, h) => sum + (h.total_tokens || 0), 0);
        return past24hTokens * avgTokenRate;
    }, [summary, totalEstimatedCost, hourlyTrendData]);

    // 计算单个模型的有效单价 ($ / Token)
    const getModelEffectiveRate = useCallback((modelName: string): number => {
        const stats = modelData.find(m => m.model === modelName);
        if (stats && stats.total_tokens > 0) {
            return calculateModelCost(stats) / stats.total_tokens;
        }
        const p = getPricing(modelName);
        return ((p.input + p.output) / 2) / 1_000_000;
    }, [modelData, calculateModelCost, getPricing]);

    // 计算“其他模型”的加权平均单价
    const getOtherModelsEffectiveRate = useCallback((): number => {
        const otherStats = modelData.filter(m => !allModels.includes(m.model) || m.model === t('token_stats.other_models', 'Other Models'));
        const totalOtherTokens = otherStats.reduce((sum, m) => sum + m.total_tokens, 0);
        const totalOtherCost = otherStats.reduce((sum, m) => sum + calculateModelCost(m), 0);
        if (totalOtherTokens > 0) {
            return totalOtherCost / totalOtherTokens;
        }
        return (DEFAULT_PRICING['default'].input + DEFAULT_PRICING['default'].output) / 2 / 1_000_000;
    }, [modelData, allModels, calculateModelCost, t]);

    // 动态根据 metricType ('tokens' | 'cost') 转换模型趋势数据
    const displayModelTrendData = useMemo(() => {
        if (metricType === 'tokens') return modelTrendData;
        const otherModelKey = t('token_stats.other_models', 'Other Models');
        const otherRate = getOtherModelsEffectiveRate();
        return modelTrendData.map(row => {
            const costRow: Record<string, any> = { period: row.period };
            allModels.forEach(modelKey => {
                const tokenVal = row[modelKey] || 0;
                const rate = modelKey === otherModelKey ? otherRate : getModelEffectiveRate(modelKey);
                costRow[modelKey] = Number((tokenVal * rate).toFixed(4));
            });
            return costRow;
        });
    }, [modelTrendData, metricType, allModels, getModelEffectiveRate, getOtherModelsEffectiveRate, t]);

    // 动态根据 metricType ('tokens' | 'cost') 转换账号趋势数据
    const displayAccountTrendData = useMemo(() => {
        if (metricType === 'tokens') return accountTrendData;
        const avgTokenRate = summary && summary.total_tokens > 0
            ? totalEstimatedCost / summary.total_tokens
            : 1.5 / 1_000_000;
        return accountTrendData.map(row => {
            const costRow: Record<string, any> = { period: row.period };
            allAccounts.forEach(acc => {
                const tokenVal = row[acc] || 0;
                costRow[acc] = Number((tokenVal * avgTokenRate).toFixed(4));
            });
            return costRow;
        });
    }, [accountTrendData, metricType, allAccounts, summary, totalEstimatedCost]);

    const trendChartContainerRef = useRef<HTMLDivElement>(null);
    const [tooltipPosition, setTooltipPosition] = useState<{ x: number; y: number } | undefined>(undefined);

    const handleTrendChartMouseMove = useCallback((e: any) => {
        if (!trendChartContainerRef.current || !e?.activeCoordinate) return;
        const containerRect = trendChartContainerRef.current.getBoundingClientRect();
        const tooltipWidth = 200;
        const rightEdgeThreshold = containerRect.width - tooltipWidth - 20;
        const mouseXInContainer = e.activeCoordinate.x;

        if (mouseXInContainer > rightEdgeThreshold) {
            setTooltipPosition({
                x: e.activeCoordinate.x - tooltipWidth - 15,
                y: e.activeCoordinate.y,
            });
        } else {
            setTooltipPosition(undefined);
        }
    }, []);

    // 趋势图 Tooltip（双主题适配）
    const CustomTrendTooltip = ({ active, payload, label }: any) => {
        if (!active || !payload || !payload.length) return null;
        const sortedPayload = [...payload].sort((a: any, b: any) => b.value - a.value);

        return (
            <div className="bg-white/95 dark:bg-[#181a20]/95 backdrop-blur-md p-3 rounded-xl shadow-xl border border-gray-200 dark:border-white/[0.08] text-xs z-[100] min-w-[190px] pointer-events-none text-gray-900 dark:text-white">
                <p className="font-semibold text-gray-800 dark:text-white/90 mb-2 border-b border-gray-200 dark:border-white/[0.08] pb-1.5 font-mono text-[11px]">
                    {label}
                </p>
                <div className="max-h-[200px] overflow-y-auto space-y-1.5 pr-1 scrollbar-thin scrollbar-thumb-gray-200 dark:scrollbar-thumb-white/10">
                    {sortedPayload.map((entry: any, index: number) => {
                        const name = entry.name;
                        const displayName = viewMode === 'model' ? name : name.split('@')[0];
                        return (
                            <div key={index} className="flex items-center justify-between gap-4">
                                <div className="flex items-center gap-2 overflow-hidden">
                                    <div className="w-2 h-2 rounded-full flex-shrink-0" style={{ backgroundColor: entry.color }} />
                                    <span className="text-gray-500 dark:text-white/60 truncate max-w-[120px]" title={name}>
                                        {displayName}
                                    </span>
                                </div>
                                <span className="font-mono font-medium text-gray-800 dark:text-white/90">
                                    {metricType === 'cost' ? formatCurrency(entry.value) : formatNumber(entry.value)}
                                </span>
                            </div>
                        );
                    })}
                </div>
            </div>
        );
    };

    // 经典模式堆叠趋势图 Tooltip
    const UsageTrendTooltip = ({ active, payload, label }: any) => {
        if (!active || !payload || !payload.length) return null;
        const row = payload[0]?.payload;
        if (!row) return null;

        return (
            <div className="bg-white/95 dark:bg-[#181a20]/95 backdrop-blur-md p-2.5 rounded-xl shadow-xl border border-gray-200 dark:border-white/[0.08] text-xs z-[100] min-w-[180px] pointer-events-none text-gray-900 dark:text-white">
                <p className="font-semibold text-gray-800 dark:text-gray-200 mb-1.5 border-b border-gray-200 dark:border-white/[0.08] pb-1.5 font-mono">
                    {label}
                </p>
                <div className="space-y-1">
                    {payload.map((item: any, idx: number) => (
                        <div key={idx} className="flex items-center justify-between gap-4">
                            <div className="flex items-center gap-2">
                                <div className="w-2 h-2 rounded-full" style={{ backgroundColor: item.fill || item.color }} />
                                <span className="text-gray-500 dark:text-white/60">
                                    {item.name}:
                                </span>
                            </div>
                            <span className="font-mono font-medium text-gray-800 dark:text-white">
                                {formatNumber(item.value)}
                            </span>
                        </div>
                    ))}
                    <div className="flex items-center justify-between gap-4 pt-1 border-t border-gray-200 dark:border-white/[0.08]">
                        <span className="text-gray-500 dark:text-white/60">
                            {t('token_stats.requests', '请求数')}:
                        </span>
                        <span className="font-mono font-medium text-gray-800 dark:text-white">
                            {(row.request_count || 0).toLocaleString()}
                        </span>
                    </div>
                </div>
            </div>
        );
    };

    // 经典模式饼图容器与 Tooltip
    const pieChartContainerRef = useRef<HTMLDivElement>(null);
    const [pieTooltipPosition, setPieTooltipPosition] = useState<{ x: number; y: number } | undefined>(undefined);

    const handlePieChartMouseMove = useCallback((e: any) => {
        if (!pieChartContainerRef.current) return;
        const containerRect = pieChartContainerRef.current.getBoundingClientRect();
        const tooltipWidth = 180;

        if (e?.activeCoordinate) {
            const mouseXInContainer = e.activeCoordinate.x;
            const rightEdgeThreshold = containerRect.width - tooltipWidth - 20;

            if (mouseXInContainer > rightEdgeThreshold) {
                setPieTooltipPosition({
                    x: e.activeCoordinate.x - tooltipWidth - 15,
                    y: e.activeCoordinate.y,
                });
            } else {
                setPieTooltipPosition(undefined);
            }
        }
    }, []);

    const CustomPieTooltip = ({ active, payload }: any) => {
        if (!active || !payload || !payload.length) return null;
        const entry = payload[0];
        return (
            <div className="bg-white/95 dark:bg-[#181a20]/95 backdrop-blur-md p-2.5 rounded-xl shadow-xl border border-gray-200 dark:border-white/[0.08] text-xs z-[100] pointer-events-none text-gray-900 dark:text-white">
                <div className="flex items-center gap-2">
                    <div className="w-2 h-2 rounded-full" style={{ backgroundColor: entry.payload.color || entry.color }} />
                    <span className="text-gray-500 dark:text-white/60 truncate max-w-[150px]">
                        {entry.payload.fullEmail || entry.name}:
                    </span>
                    <span className="font-mono font-medium text-gray-800 dark:text-white">
                        {formatNumber(entry.value)}
                    </span>
                </div>
            </div>
        );
    };

    return (
        <div className="h-full w-full overflow-y-auto bg-gray-50/50 dark:bg-[#0a0a0c] text-gray-800 dark:text-white/90">
            <div className="p-4 sm:p-6 space-y-5 max-w-7xl mx-auto">
                {/* 顶部标题与多功能工具栏 */}
                <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-4">
                    <div className="flex items-center gap-3">
                        <div className="p-2 rounded-xl bg-blue-500/10 border border-blue-500/20 text-blue-500 dark:text-blue-400">
                            <Zap className="w-5 h-5" />
                        </div>
                        <div>
                            <h1 className="text-xl font-bold tracking-tight text-gray-900 dark:text-white flex items-center gap-2">
                                {t('token_stats.title', 'Token 消费统计')}
                            </h1>
                            <p className="text-xs text-gray-500 dark:text-white/40 mt-0.5">
                                {t('token_stats.subtitle', '全局 Token 消耗流向、模型定价与活跃全景')}
                            </p>
                        </div>
                    </div>

                    <div className="flex flex-wrap items-center gap-2.5">
                        {/* 双看板模式切换器 (Pill Toggle: 深度洞察 / 经典看板) */}
                        <div className="flex bg-gray-100 dark:bg-[#16181d] border border-gray-200 dark:border-white/[0.08] rounded-xl p-1 shadow-sm">
                            <button
                                onClick={() => switchDashboardMode('insights')}
                                className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1.5 ${
                                    dashboardMode === 'insights'
                                        ? 'bg-white dark:bg-white/[0.12] text-blue-600 dark:text-white shadow-sm'
                                        : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                }`}
                            >
                                <Sparkles className="w-3.5 h-3.5 text-amber-500 dark:text-amber-400" />
                                {t('token_stats.mode_insights', '深度洞察')}
                            </button>
                            <button
                                onClick={() => switchDashboardMode('classic')}
                                className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1.5 ${
                                    dashboardMode === 'classic'
                                        ? 'bg-white dark:bg-white/[0.12] text-blue-600 dark:text-white shadow-sm'
                                        : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                }`}
                            >
                                <BarChart3 className="w-3.5 h-3.5 text-blue-500 dark:text-blue-400" />
                                {t('token_stats.mode_classic', '经典看板')}
                            </button>
                        </div>

                        {/* 时间段选择药丸 */}
                        <div className="flex bg-gray-100 dark:bg-[#16181d] border border-gray-200 dark:border-white/[0.08] rounded-xl p-1 shadow-sm">
                            <button
                                onClick={() => switchTimeRange('hourly')}
                                className={`px-2.5 sm:px-3 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1.5 ${
                                    timeRange === 'hourly'
                                        ? 'bg-white dark:bg-white/[0.12] text-gray-900 dark:text-white shadow-sm'
                                        : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                }`}
                            >
                                <Clock className="w-3.5 h-3.5" />
                                {t('token_stats.hourly', '小时')}
                            </button>
                            <button
                                onClick={() => switchTimeRange('daily')}
                                className={`px-2.5 sm:px-3 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1.5 ${
                                    timeRange === 'daily'
                                        ? 'bg-white dark:bg-white/[0.12] text-gray-900 dark:text-white shadow-sm'
                                        : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                }`}
                            >
                                <Calendar className="w-3.5 h-3.5" />
                                {t('token_stats.daily', '日')}
                            </button>
                            <button
                                onClick={() => switchTimeRange('weekly')}
                                className={`px-2.5 sm:px-3 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1.5 ${
                                    timeRange === 'weekly'
                                        ? 'bg-white dark:bg-white/[0.12] text-gray-900 dark:text-white shadow-sm'
                                        : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                }`}
                            >
                                <CalendarDays className="w-3.5 h-3.5" />
                                {t('token_stats.weekly', '周')}
                            </button>
                            <button
                                onClick={() => switchTimeRange('all')}
                                className={`px-2.5 sm:px-3 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1.5 ${
                                    timeRange === 'all'
                                        ? 'bg-white dark:bg-white/[0.12] text-gray-900 dark:text-white shadow-sm'
                                        : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                }`}
                            >
                                <History className="w-3.5 h-3.5" />
                                {t('token_stats.all_time', '全部历史')}
                            </button>
                        </div>

                        {/* 单价配置按钮 */}
                        <button
                            onClick={() => setShowPricingModal(true)}
                            title={t('token_stats.custom_pricing', '自定义单价')}
                            className="p-2 rounded-xl bg-white dark:bg-[#16181d] border border-gray-200 dark:border-white/[0.08] text-gray-600 dark:text-white/70 hover:text-gray-900 dark:hover:text-white hover:bg-gray-100 dark:hover:bg-white/[0.06] transition-colors shadow-sm"
                        >
                            <Settings2 className="w-4 h-4" />
                        </button>

                        {/* 刷新按钮 */}
                        <button
                            onClick={fetchData}
                            disabled={loading}
                            className="p-2 rounded-xl bg-blue-600 hover:bg-blue-500 text-white transition-colors disabled:opacity-50 shadow-sm shadow-blue-500/20"
                        >
                            <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
                        </button>
                    </div>
                </div>

                {/* ==================== 1. 深度洞察看板 (Insights View) ==================== */}
                {dashboardMode === 'insights' && (
                    <>
                        {/* 核心指标微卡片（4卡片设计，双主题适配） */}
                        {summary && (
                            <div className="grid grid-cols-2 md:grid-cols-4 gap-3.5">
                                {/* 总 Token */}
                                <div className="bg-white dark:bg-[#121316] border border-gray-200/80 dark:border-white/[0.08] rounded-2xl p-4 shadow-sm hover:border-gray-300 dark:hover:border-white/[0.15] transition-all flex flex-col justify-between">
                                    <div className="flex items-center justify-between text-gray-500 dark:text-white/50 text-xs">
                                        <span>{t('token_stats.total_tokens', '总 Token')}</span>
                                        <div className="p-1.5 rounded-lg bg-gray-100 dark:bg-white/[0.04]">
                                            <Zap className="w-3.5 h-3.5 text-gray-600 dark:text-white/70" />
                                        </div>
                                    </div>
                                    <div className="mt-2 text-2xl font-bold font-mono tracking-tight text-gray-900 dark:text-white">
                                        {formatNumber(summary.total_tokens)}
                                    </div>
                                    <div className="text-[11px] text-gray-400 dark:text-white/40 mt-1 font-mono">
                                        {t('token_stats.input_short', '入')} {formatNumber(summary.total_input_tokens)} · {t('token_stats.output_short', '出')} {formatNumber(summary.total_output_tokens)}
                                    </div>
                                </div>

                                {/* 估算价值 */}
                                <div className="bg-white dark:bg-[#121316] border border-gray-200/80 dark:border-white/[0.08] rounded-2xl p-4 shadow-sm hover:border-gray-300 dark:hover:border-white/[0.15] transition-all flex flex-col justify-between">
                                    <div className="flex items-center justify-between text-emerald-600 dark:text-emerald-400/80 text-xs">
                                        <span className="font-medium">{t('token_stats.estimated_value', '估算价值')}</span>
                                        <div className="p-1.5 rounded-lg bg-emerald-50 dark:bg-emerald-500/10">
                                            <DollarSign className="w-3.5 h-3.5 text-emerald-600 dark:text-emerald-400" />
                                        </div>
                                    </div>
                                    <div className="mt-2 text-2xl font-bold font-mono tracking-tight text-emerald-600 dark:text-emerald-400">
                                        ${totalEstimatedCost.toFixed(2)}
                                    </div>
                                    <div className="text-[11px] text-gray-400 dark:text-white/40 mt-1 flex items-center justify-between">
                                        <span>{t('token_stats.pricing_accumulated', '单价快照累计')}</span>
                                        <button
                                            onClick={() => setShowPricingModal(true)}
                                            className="text-[10px] text-emerald-600 dark:text-emerald-400 hover:underline"
                                        >
                                            {t('token_stats.configure_pricing', '配置单价')}
                                        </button>
                                    </div>
                                </div>

                                {/* 缓存命中 */}
                                <div className="bg-white dark:bg-[#121316] border border-gray-200/80 dark:border-white/[0.08] rounded-2xl p-4 shadow-sm hover:border-gray-300 dark:hover:border-white/[0.15] transition-all flex flex-col justify-between">
                                    <div className="flex items-center justify-between text-sky-600 dark:text-sky-400/80 text-xs">
                                        <span className="font-medium">{t('token_stats.cached_token', '缓存命中')}</span>
                                        <div className="p-1.5 rounded-lg bg-sky-50 dark:bg-sky-500/10">
                                            <Database className="w-3.5 h-3.5 text-sky-600 dark:text-sky-400" />
                                        </div>
                                    </div>
                                    <div className="mt-2 text-2xl font-bold font-mono tracking-tight text-sky-600 dark:text-sky-400 flex items-baseline gap-2">
                                        <span>{formatNumber(summary.total_cached_tokens)}</span>
                                        <span className="text-xs font-normal px-1.5 py-0.5 rounded-full bg-sky-50 dark:bg-sky-500/10 border border-sky-200 dark:border-sky-500/20 text-sky-600 dark:text-sky-300">
                                            {cacheMetrics.hitRate.toFixed(1)}%
                                        </span>
                                    </div>
                                    <div className="text-[11px] text-gray-400 dark:text-white/40 mt-1 font-mono">
                                        {t('token_stats.estimated_savings', { amount: cacheMetrics.savings.toFixed(2) })}
                                    </div>
                                </div>

                                {/* 活跃生态 */}
                                <div className="bg-white dark:bg-[#121316] border border-gray-200/80 dark:border-white/[0.08] rounded-2xl p-4 shadow-sm hover:border-gray-300 dark:hover:border-white/[0.15] transition-all flex flex-col justify-between">
                                    <div className="flex items-center justify-between text-purple-600 dark:text-purple-400/80 text-xs">
                                        <span className="font-medium">{t('token_stats.active_ecosystem', '活跃生态')}</span>
                                        <div className="p-1.5 rounded-lg bg-purple-50 dark:bg-purple-500/10">
                                            <Users className="w-3.5 h-3.5 text-purple-600 dark:text-purple-400" />
                                        </div>
                                    </div>
                                    <div className="mt-2 text-2xl font-bold font-mono tracking-tight text-gray-900 dark:text-white flex items-baseline gap-3">
                                        <span>{summary.unique_accounts} <span className="text-xs text-gray-400 dark:text-white/50 font-normal">{t('token_stats.accounts_unit', '账号')}</span></span>
                                        <span className="text-purple-600 dark:text-purple-400">{modelData.length} <span className="text-xs text-gray-400 dark:text-white/50 font-normal">{t('token_stats.models_unit', '模型')}</span></span>
                                    </div>
                                    <div className="text-[11px] text-gray-400 dark:text-white/40 mt-1 font-mono">
                                        {t('token_stats.total_requests_count', { count: summary.total_requests })}
                                    </div>
                                </div>
                            </div>
                        )}

                        {/* 核心亮点：活动热力图 + 24小时极简圆角柱条 */}
                        <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                            <ActivityHeatmapCard
                                dailyData={heatmapData}
                                totalEstimatedCost={totalEstimatedCost}
                                formatNumber={formatNumber}
                            />
                            <HourlyTrendBarCard
                                hourlyData={hourlyTrendData}
                                totalEstimatedCost={hourly24hEstimatedCost}
                                formatNumber={formatNumber}
                            />
                        </div>

                        {/* 主力模型占比条形卡片 + 趋势图 */}
                        <div className="grid grid-cols-1 xl:grid-cols-12 gap-4">
                            <div className="xl:col-span-5">
                                <TopModelsShareCard
                                    models={modelShareItems}
                                    formatNumber={formatNumber}
                                />
                            </div>

                            <div className="xl:col-span-7 bg-white dark:bg-[#121316] rounded-2xl p-4 sm:p-5 border border-gray-200/80 dark:border-white/[0.08] shadow-sm flex flex-col justify-between">
                                <div className="flex flex-wrap items-center justify-between gap-2 mb-3">
                                    <div className="flex items-center gap-2">
                                        {chartType === 'bar' ? (
                                            <BarChart3 className="w-4 h-4 text-blue-500 dark:text-blue-400 opacity-90" />
                                        ) : (
                                            <TrendingUp className="w-4 h-4 text-blue-500 dark:text-blue-400 opacity-90" />
                                        )}
                                        <span className="text-[13px] font-semibold text-gray-900 dark:text-white/90 tracking-wide">
                                            {viewMode === 'model'
                                                ? metricType === 'cost'
                                                    ? t('token_stats.model_cost_trend', '分模型金额趋势')
                                                    : t('token_stats.model_trend', '分模型使用趋势')
                                                : metricType === 'cost'
                                                    ? t('token_stats.account_cost_trend', '分账号金额趋势')
                                                    : t('token_stats.account_trend', '分账号使用趋势')}
                                        </span>
                                    </div>

                                    <div className="flex flex-wrap items-center gap-2">
                                        {/* 图表形态切换器：柱状图 / 面积图 */}
                                        <div className="flex bg-gray-100 dark:bg-[#1a1d24] border border-gray-200 dark:border-white/[0.08] rounded-xl p-0.5">
                                            <button
                                                onClick={() => switchChartType('bar')}
                                                title={t('token_stats.chart_type_bar', '柱状')}
                                                className={`px-2 py-1 text-xs font-medium rounded-lg transition-all flex items-center gap-1 ${
                                                    chartType === 'bar'
                                                        ? 'bg-white dark:bg-white/[0.12] text-gray-900 dark:text-white shadow-sm'
                                                        : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                                }`}
                                            >
                                                <BarChart3 className="w-3.5 h-3.5" />
                                                <span>{t('token_stats.chart_type_bar', '柱状')}</span>
                                            </button>
                                            <button
                                                onClick={() => switchChartType('area')}
                                                title={t('token_stats.chart_type_area', '面积')}
                                                className={`px-2 py-1 text-xs font-medium rounded-lg transition-all flex items-center gap-1 ${
                                                    chartType === 'area'
                                                        ? 'bg-white dark:bg-white/[0.12] text-gray-900 dark:text-white shadow-sm'
                                                        : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                                }`}
                                            >
                                                <TrendingUp className="w-3.5 h-3.5" />
                                                <span>{t('token_stats.chart_type_area', '面积')}</span>
                                            </button>
                                        </div>

                                        {/* Token / 金额 维度切换器 */}
                                        <div className="flex bg-gray-100 dark:bg-[#1a1d24] border border-gray-200 dark:border-white/[0.08] rounded-xl p-0.5">
                                            <button
                                                onClick={() => switchMetricType('tokens')}
                                                className={`px-2 py-1 text-xs font-medium rounded-lg transition-all flex items-center gap-1 ${
                                                    metricType === 'tokens'
                                                        ? 'bg-white dark:bg-white/[0.12] text-gray-900 dark:text-white shadow-sm'
                                                        : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                                }`}
                                            >
                                                <Zap className="w-3.5 h-3.5" />
                                                <span>{t('token_stats.metric_tokens', 'Token')}</span>
                                            </button>
                                            <button
                                                onClick={() => switchMetricType('cost')}
                                                className={`px-2 py-1 text-xs font-medium rounded-lg transition-all flex items-center gap-1 ${
                                                    metricType === 'cost'
                                                        ? 'bg-white dark:bg-white/[0.12] text-emerald-600 dark:text-emerald-400 shadow-sm font-semibold'
                                                        : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                                }`}
                                            >
                                                <DollarSign className="w-3.5 h-3.5" />
                                                <span>{t('token_stats.metric_cost', '金额')}</span>
                                            </button>
                                        </div>

                                        {/* 模型 / 账号 维度切换器 */}
                                        <div className="flex bg-gray-100 dark:bg-[#1a1d24] border border-gray-200 dark:border-white/[0.08] rounded-xl p-0.5">
                                            <button
                                                onClick={() => switchViewMode('model')}
                                                className={`px-2.5 py-1 text-xs font-medium rounded-lg transition-all ${
                                                    viewMode === 'model'
                                                        ? 'bg-white dark:bg-white/[0.12] text-gray-900 dark:text-white shadow-sm'
                                                        : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                                }`}
                                            >
                                                {t('token_stats.by_model', '按模型')}
                                            </button>
                                            <button
                                                onClick={() => switchViewMode('account')}
                                                className={`px-2.5 py-1 text-xs font-medium rounded-lg transition-all ${
                                                    viewMode === 'account'
                                                        ? 'bg-white dark:bg-white/[0.12] text-gray-900 dark:text-white shadow-sm'
                                                        : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                                }`}
                                            >
                                                {t('token_stats.by_account_view', '按账号')}
                                            </button>
                                        </div>
                                    </div>
                                </div>

                                <div className="h-60" ref={trendChartContainerRef}>
                                    {modelTrendData.length > 0 && allModels.length > 0 ? (
                                        <ResponsiveContainer width="100%" height="100%">
                                            {chartType === 'bar' ? (
                                                <BarChart
                                                    data={viewMode === 'model' ? displayModelTrendData : displayAccountTrendData}
                                                    onMouseMove={handleTrendChartMouseMove}
                                                    onMouseLeave={() => setTooltipPosition(undefined)}
                                                >
                                                    <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="rgba(150, 150, 150, 0.15)" />
                                                    <XAxis
                                                        dataKey="period"
                                                        tick={{ fontSize: 10, fill: '#888888', fontFamily: 'monospace' }}
                                                        tickFormatter={(val) => {
                                                            if (timeRange === 'hourly') return val.split(' ')[1] || val;
                                                            if (timeRange === 'daily') return val.split('-').slice(1).join('/');
                                                            if (timeRange === 'weekly') {
                                                                const parts = val.split('-W');
                                                                return parts.length > 1 ? `W${parts[1]}` : val;
                                                            }
                                                            if (timeRange === 'all') return val.split('-').slice(1).join('/');
                                                            return val;
                                                        }}
                                                        axisLine={false}
                                                        tickLine={false}
                                                        dy={5}
                                                    />
                                                    <YAxis
                                                        tick={{ fontSize: 10, fill: '#888888', fontFamily: 'monospace' }}
                                                        tickFormatter={(val) => metricType === 'cost' ? formatCurrency(val) : formatNumber(val)}
                                                        axisLine={false}
                                                        tickLine={false}
                                                    />
                                                    <Tooltip
                                                        content={<CustomTrendTooltip />}
                                                        cursor={{ fill: 'rgba(150, 150, 150, 0.08)' }}
                                                        allowEscapeViewBox={{ x: true, y: true }}
                                                        position={tooltipPosition}
                                                    />
                                                    {displayModelKeysOrAccounts(viewMode, allModels, allAccounts).map((item, index) => {
                                                        const items = displayModelKeysOrAccounts(viewMode, allModels, allAccounts);
                                                        const isTop = index === items.length - 1;
                                                        return (
                                                            <Bar
                                                                key={item}
                                                                dataKey={item}
                                                                stackId="stack"
                                                                fill={TOP_COLORS[index % TOP_COLORS.length]}
                                                                radius={isTop ? [3, 3, 0, 0] : [0, 0, 0, 0]}
                                                                maxBarSize={30}
                                                            />
                                                        );
                                                    })}
                                                </BarChart>
                                            ) : (
                                                <AreaChart
                                                    data={viewMode === 'model' ? displayModelTrendData : displayAccountTrendData}
                                                    onMouseMove={handleTrendChartMouseMove}
                                                    onMouseLeave={() => setTooltipPosition(undefined)}
                                                >
                                                    <defs>
                                                        {displayModelKeysOrAccounts(viewMode, allModels, allAccounts).map((item, idx) => (
                                                            <linearGradient key={`grad-${item}`} id={`color-${idx}`} x1="0" y1="0" x2="0" y2="1">
                                                                <stop offset="5%" stopColor={TOP_COLORS[idx % TOP_COLORS.length]} stopOpacity={0.4} />
                                                                <stop offset="95%" stopColor={TOP_COLORS[idx % TOP_COLORS.length]} stopOpacity={0.0} />
                                                            </linearGradient>
                                                        ))}
                                                    </defs>
                                                    <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="rgba(150, 150, 150, 0.15)" />
                                                    <XAxis
                                                        dataKey="period"
                                                        tick={{ fontSize: 10, fill: '#888888', fontFamily: 'monospace' }}
                                                        tickFormatter={(val) => {
                                                            if (timeRange === 'hourly') return val.split(' ')[1] || val;
                                                            if (timeRange === 'daily') return val.split('-').slice(1).join('/');
                                                            if (timeRange === 'weekly') {
                                                                const parts = val.split('-W');
                                                                return parts.length > 1 ? `W${parts[1]}` : val;
                                                            }
                                                            if (timeRange === 'all') return val.split('-').slice(1).join('/');
                                                            return val;
                                                        }}
                                                        axisLine={false}
                                                        tickLine={false}
                                                        dy={5}
                                                    />
                                                    <YAxis
                                                        tick={{ fontSize: 10, fill: '#888888', fontFamily: 'monospace' }}
                                                        tickFormatter={(val) => metricType === 'cost' ? formatCurrency(val) : formatNumber(val)}
                                                        axisLine={false}
                                                        tickLine={false}
                                                    />
                                                    <Tooltip
                                                        content={<CustomTrendTooltip />}
                                                        cursor={{ stroke: 'rgba(150, 150, 150, 0.3)', strokeWidth: 1, strokeDasharray: '4 4' }}
                                                        allowEscapeViewBox={{ x: true, y: true }}
                                                        position={tooltipPosition}
                                                    />
                                                    {displayModelKeysOrAccounts(viewMode, allModels, allAccounts).map((item, index) => (
                                                        <Area
                                                            key={item}
                                                            type="monotone"
                                                            dataKey={item}
                                                            stackId="1"
                                                            stroke={TOP_COLORS[index % TOP_COLORS.length]}
                                                            strokeWidth={1.5}
                                                            fill={`url(#color-${index})`}
                                                        />
                                                    ))}
                                                </AreaChart>
                                            )}
                                        </ResponsiveContainer>
                                    ) : (
                                        <div className="h-full flex items-center justify-center text-gray-400 dark:text-white/30 text-xs">
                                            {loading ? t('common.loading', '加载中...') : t('token_stats.no_data', '暂无数据')}
                                        </div>
                                    )}
                                </div>

                                {/* 底部紧凑图例 */}
                                <div className="flex items-center flex-wrap gap-3 pt-3 border-t border-gray-100 dark:border-white/[0.06] text-[11px] font-mono">
                                    {displayModelKeysOrAccounts(viewMode, allModels, allAccounts).map((item, index) => (
                                        <div key={item} className="flex items-center gap-1.5 text-gray-600 dark:text-white/60">
                                            <div
                                                className="w-2 h-2 rounded-full"
                                                style={{ backgroundColor: TOP_COLORS[index % TOP_COLORS.length] }}
                                            />
                                            <span className="truncate max-w-[120px]">
                                                {viewMode === 'model' ? item : item.split('@')[0]}
                                            </span>
                                        </div>
                                    ))}
                                </div>
                            </div>
                        </div>
                    </>
                )}

                {/* ==================== 2. 经典看板模式 (Classic View) ==================== */}
                {dashboardMode === 'classic' && (
                    <>
                        {/* 6 个经典独立指标卡片 */}
                        {summary && (
                            <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-6 gap-3.5">
                                {/* 总 Token */}
                                <div className="bg-white dark:bg-[#121316] rounded-2xl p-4 shadow-sm border border-gray-200/80 dark:border-white/[0.08] flex flex-col justify-between">
                                    <div className="flex items-center gap-2 text-gray-500 dark:text-white/50 text-xs mb-2">
                                        <div className="p-1.5 rounded-lg bg-gray-100 dark:bg-white/[0.05]">
                                            <Zap className="w-3.5 h-3.5 text-gray-600 dark:text-white/70" />
                                        </div>
                                        <span>{t('token_stats.total_tokens', '总 Token')}</span>
                                    </div>
                                    <div className="text-2xl font-bold font-mono tracking-tight text-gray-900 dark:text-white">
                                        {formatNumber(summary.total_tokens)}
                                    </div>
                                </div>

                                {/* 输入 Token */}
                                <div className="bg-white dark:bg-[#121316] rounded-2xl p-4 shadow-sm border border-gray-200/80 dark:border-white/[0.08] flex flex-col justify-between">
                                    <div className="flex items-center gap-2 text-blue-600 dark:text-blue-400 text-xs mb-2">
                                        <div className="p-1.5 rounded-lg bg-blue-50 dark:bg-blue-500/10">
                                            <TrendingUp className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" />
                                        </div>
                                        <span>{t('token_stats.input_tokens', '输入 Token')}</span>
                                    </div>
                                    <div className="text-2xl font-bold font-mono tracking-tight text-blue-600 dark:text-blue-400">
                                        {formatNumber(summary.total_input_tokens)}
                                    </div>
                                </div>

                                {/* 输出 Token */}
                                <div className="bg-white dark:bg-[#121316] rounded-2xl p-4 shadow-sm border border-gray-200/80 dark:border-white/[0.08] flex flex-col justify-between">
                                    <div className="flex items-center gap-2 text-purple-600 dark:text-purple-400 text-xs mb-2">
                                        <div className="p-1.5 rounded-lg bg-purple-50 dark:bg-purple-500/10">
                                            <TrendingUp className="w-3.5 h-3.5 rotate-180 text-purple-600 dark:text-purple-400" />
                                        </div>
                                        <span>{t('token_stats.output_tokens', '输出 Token')}</span>
                                    </div>
                                    <div className="text-2xl font-bold font-mono tracking-tight text-purple-600 dark:text-purple-400">
                                        {formatNumber(summary.total_output_tokens)}
                                    </div>
                                </div>

                                {/* 缓存命中 */}
                                <div className="bg-white dark:bg-[#121316] rounded-2xl p-4 shadow-sm border border-gray-200/80 dark:border-white/[0.08] flex flex-col justify-between">
                                    <div className="flex items-center gap-2 text-sky-600 dark:text-sky-400 text-xs mb-2">
                                        <div className="p-1.5 rounded-lg bg-sky-50 dark:bg-sky-500/10">
                                            <Database className="w-3.5 h-3.5 text-sky-600 dark:text-sky-400" />
                                        </div>
                                        <span>{t('token_stats.cached_token', '缓存命中')}</span>
                                    </div>
                                    <div className="text-2xl font-bold font-mono tracking-tight text-sky-600 dark:text-sky-400">
                                        {formatNumber(summary.total_cached_tokens)}
                                    </div>
                                </div>

                                {/* 活跃账号 */}
                                <div className="bg-white dark:bg-[#121316] rounded-2xl p-4 shadow-sm border border-gray-200/80 dark:border-white/[0.08] flex flex-col justify-between">
                                    <div className="flex items-center gap-2 text-emerald-600 dark:text-emerald-400 text-xs mb-2">
                                        <div className="p-1.5 rounded-lg bg-emerald-50 dark:bg-emerald-500/10">
                                            <Users className="w-3.5 h-3.5 text-emerald-600 dark:text-emerald-400" />
                                        </div>
                                        <span>{t('token_stats.accounts_used', '活跃账号')}</span>
                                    </div>
                                    <div className="text-2xl font-bold font-mono tracking-tight text-emerald-600 dark:text-emerald-400">
                                        {summary.unique_accounts}
                                    </div>
                                </div>

                                {/* 使用模型 */}
                                <div className="bg-white dark:bg-[#121316] rounded-2xl p-4 shadow-sm border border-gray-200/80 dark:border-white/[0.08] flex flex-col justify-between">
                                    <div className="flex items-center gap-2 text-amber-600 dark:text-amber-400 text-xs mb-2">
                                        <div className="p-1.5 rounded-lg bg-amber-50 dark:bg-amber-500/10">
                                            <Cpu className="w-3.5 h-3.5 text-amber-600 dark:text-amber-400" />
                                        </div>
                                        <span>{t('token_stats.models_used', '使用模型')}</span>
                                    </div>
                                    <div className="text-2xl font-bold font-mono tracking-tight text-amber-600 dark:text-amber-400">
                                        {modelData.length}
                                    </div>
                                </div>
                            </div>
                        )}

                        {/* 全宽趋势图 (Full-Width Trend Chart) */}
                        <div className="bg-white dark:bg-[#121316] rounded-2xl p-4 sm:p-5 border border-gray-200/80 dark:border-white/[0.08] shadow-sm flex flex-col justify-between">
                            <div className="flex flex-wrap items-center justify-between gap-2 mb-3">
                                <div className="flex items-center gap-2">
                                    {chartType === 'bar' ? (
                                        <BarChart3 className="w-4 h-4 text-blue-500 dark:text-blue-400 opacity-90" />
                                    ) : (
                                        <TrendingUp className="w-4 h-4 text-blue-500 dark:text-blue-400 opacity-90" />
                                    )}
                                    <span className="text-[13px] font-semibold text-gray-900 dark:text-white/90 tracking-wide">
                                        {viewMode === 'model'
                                            ? metricType === 'cost'
                                                ? t('token_stats.model_cost_trend', '分模型金额趋势')
                                                : t('token_stats.model_trend', '使用趋势 (按模型)')
                                            : metricType === 'cost'
                                                ? t('token_stats.account_cost_trend', '分账号金额趋势')
                                                : t('token_stats.account_trend', '使用趋势 (按账号)')}
                                    </span>
                                </div>

                                <div className="flex flex-wrap items-center gap-2">
                                    {/* 柱状 / 面积 切换 */}
                                    <div className="flex bg-gray-100 dark:bg-[#1a1d24] border border-gray-200 dark:border-white/[0.08] rounded-xl p-0.5">
                                        <button
                                            onClick={() => switchChartType('bar')}
                                            className={`px-2 py-1 text-xs font-medium rounded-lg transition-all flex items-center gap-1 ${
                                                chartType === 'bar'
                                                    ? 'bg-white dark:bg-white/[0.12] text-gray-900 dark:text-white shadow-sm'
                                                    : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                            }`}
                                        >
                                            <BarChart3 className="w-3.5 h-3.5" />
                                            <span>{t('token_stats.chart_type_bar', '柱状')}</span>
                                        </button>
                                        <button
                                            onClick={() => switchChartType('area')}
                                            className={`px-2 py-1 text-xs font-medium rounded-lg transition-all flex items-center gap-1 ${
                                                chartType === 'area'
                                                    ? 'bg-white dark:bg-white/[0.12] text-gray-900 dark:text-white shadow-sm'
                                                    : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                            }`}
                                        >
                                            <TrendingUp className="w-3.5 h-3.5" />
                                            <span>{t('token_stats.chart_type_area', '面积')}</span>
                                        </button>
                                    </div>

                                    {/* Token / 金额 维度切换 */}
                                    <div className="flex bg-gray-100 dark:bg-[#1a1d24] border border-gray-200 dark:border-white/[0.08] rounded-xl p-0.5">
                                        <button
                                            onClick={() => switchMetricType('tokens')}
                                            className={`px-2 py-1 text-xs font-medium rounded-lg transition-all flex items-center gap-1 ${
                                                metricType === 'tokens'
                                                    ? 'bg-white dark:bg-white/[0.12] text-gray-900 dark:text-white shadow-sm'
                                                    : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                            }`}
                                        >
                                            <Zap className="w-3.5 h-3.5" />
                                            <span>{t('token_stats.metric_tokens', 'Token')}</span>
                                        </button>
                                        <button
                                            onClick={() => switchMetricType('cost')}
                                            className={`px-2 py-1 text-xs font-medium rounded-lg transition-all flex items-center gap-1 ${
                                                metricType === 'cost'
                                                    ? 'bg-white dark:bg-white/[0.12] text-emerald-600 dark:text-emerald-400 shadow-sm font-semibold'
                                                    : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                            }`}
                                        >
                                            <DollarSign className="w-3.5 h-3.5" />
                                            <span>{t('token_stats.metric_cost', '金额')}</span>
                                        </button>
                                    </div>

                                    {/* 模型 / 账号 维度切换 */}
                                    <div className="flex bg-gray-100 dark:bg-[#1a1d24] border border-gray-200 dark:border-white/[0.08] rounded-xl p-0.5">
                                        <button
                                            onClick={() => switchViewMode('model')}
                                            className={`px-2.5 py-1 text-xs font-medium rounded-lg transition-all ${
                                                viewMode === 'model'
                                                    ? 'bg-white dark:bg-white/[0.12] text-gray-900 dark:text-white shadow-sm'
                                                    : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                            }`}
                                        >
                                            {t('token_stats.by_model', '按模型')}
                                        </button>
                                        <button
                                            onClick={() => switchViewMode('account')}
                                            className={`px-2.5 py-1 text-xs font-medium rounded-lg transition-all ${
                                                viewMode === 'account'
                                                    ? 'bg-white dark:bg-white/[0.12] text-gray-900 dark:text-white shadow-sm'
                                                    : 'text-gray-500 dark:text-white/50 hover:text-gray-900 dark:hover:text-white/80'
                                            }`}
                                        >
                                            {t('token_stats.by_account_view', '按账号')}
                                        </button>
                                    </div>
                                </div>
                            </div>

                            <div className="h-64" ref={trendChartContainerRef}>
                                {modelTrendData.length > 0 && allModels.length > 0 ? (
                                    <ResponsiveContainer width="100%" height="100%">
                                        {chartType === 'bar' ? (
                                            <BarChart
                                                data={viewMode === 'model' ? displayModelTrendData : displayAccountTrendData}
                                                onMouseMove={handleTrendChartMouseMove}
                                                onMouseLeave={() => setTooltipPosition(undefined)}
                                            >
                                                <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="rgba(150, 150, 150, 0.15)" />
                                                <XAxis
                                                    dataKey="period"
                                                    tick={{ fontSize: 10, fill: '#888888', fontFamily: 'monospace' }}
                                                    tickFormatter={(val) => {
                                                        if (timeRange === 'hourly') return val.split(' ')[1] || val;
                                                        if (timeRange === 'daily') return val.split('-').slice(1).join('/');
                                                        if (timeRange === 'weekly') {
                                                            const parts = val.split('-W');
                                                            return parts.length > 1 ? `W${parts[1]}` : val;
                                                        }
                                                        return val;
                                                    }}
                                                    axisLine={false}
                                                    tickLine={false}
                                                    dy={5}
                                                />
                                                <YAxis
                                                    tick={{ fontSize: 10, fill: '#888888', fontFamily: 'monospace' }}
                                                    tickFormatter={(val) => (metricType === 'cost' ? formatCurrency(val) : formatNumber(val))}
                                                    axisLine={false}
                                                    tickLine={false}
                                                />
                                                <Tooltip
                                                    content={<CustomTrendTooltip />}
                                                    cursor={{ fill: 'rgba(150, 150, 150, 0.08)' }}
                                                    allowEscapeViewBox={{ x: true, y: true }}
                                                    position={tooltipPosition}
                                                />
                                                {displayModelKeysOrAccounts(viewMode, allModels, allAccounts).map((item, index) => {
                                                    const items = displayModelKeysOrAccounts(viewMode, allModels, allAccounts);
                                                    const isTop = index === items.length - 1;
                                                    return (
                                                        <Bar
                                                            key={item}
                                                            dataKey={item}
                                                            stackId="stack"
                                                            fill={TOP_COLORS[index % TOP_COLORS.length]}
                                                            radius={isTop ? [3, 3, 0, 0] : [0, 0, 0, 0]}
                                                            maxBarSize={30}
                                                        />
                                                    );
                                                })}
                                            </BarChart>
                                        ) : (
                                            <AreaChart
                                                data={viewMode === 'model' ? displayModelTrendData : displayAccountTrendData}
                                                onMouseMove={handleTrendChartMouseMove}
                                                onMouseLeave={() => setTooltipPosition(undefined)}
                                            >
                                                <defs>
                                                    {displayModelKeysOrAccounts(viewMode, allModels, allAccounts).map((item, idx) => (
                                                        <linearGradient key={`grad-classic-${item}`} id={`color-classic-${idx}`} x1="0" y1="0" x2="0" y2="1">
                                                            <stop offset="5%" stopColor={TOP_COLORS[idx % TOP_COLORS.length]} stopOpacity={0.4} />
                                                            <stop offset="95%" stopColor={TOP_COLORS[idx % TOP_COLORS.length]} stopOpacity={0.0} />
                                                        </linearGradient>
                                                    ))}
                                                </defs>
                                                <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="rgba(150, 150, 150, 0.15)" />
                                                <XAxis
                                                    dataKey="period"
                                                    tick={{ fontSize: 10, fill: '#888888', fontFamily: 'monospace' }}
                                                    tickFormatter={(val) => {
                                                        if (timeRange === 'hourly') return val.split(' ')[1] || val;
                                                        if (timeRange === 'daily') return val.split('-').slice(1).join('/');
                                                        if (timeRange === 'weekly') {
                                                            const parts = val.split('-W');
                                                            return parts.length > 1 ? `W${parts[1]}` : val;
                                                        }
                                                        return val;
                                                    }}
                                                    axisLine={false}
                                                    tickLine={false}
                                                    dy={5}
                                                />
                                                <YAxis
                                                    tick={{ fontSize: 10, fill: '#888888', fontFamily: 'monospace' }}
                                                    tickFormatter={(val) => (metricType === 'cost' ? formatCurrency(val) : formatNumber(val))}
                                                    axisLine={false}
                                                    tickLine={false}
                                                />
                                                <Tooltip
                                                    content={<CustomTrendTooltip />}
                                                    cursor={{ stroke: 'rgba(150, 150, 150, 0.3)', strokeWidth: 1, strokeDasharray: '4 4' }}
                                                    allowEscapeViewBox={{ x: true, y: true }}
                                                    position={tooltipPosition}
                                                />
                                                {displayModelKeysOrAccounts(viewMode, allModels, allAccounts).map((item, index) => (
                                                    <Area
                                                        key={item}
                                                        type="monotone"
                                                        dataKey={item}
                                                        stackId="1"
                                                        stroke={TOP_COLORS[index % TOP_COLORS.length]}
                                                        strokeWidth={1.5}
                                                        fill={`url(#color-classic-${index})`}
                                                    />
                                                ))}
                                            </AreaChart>
                                        )}
                                    </ResponsiveContainer>
                                ) : (
                                    <div className="h-full flex items-center justify-center text-gray-400 dark:text-white/30 text-xs">
                                        {loading ? t('common.loading', '加载中...') : t('token_stats.no_data', '暂无数据')}
                                    </div>
                                )}
                            </div>

                            <div className="flex items-center flex-wrap gap-3 pt-3 border-t border-gray-100 dark:border-white/[0.06] text-[11px] font-mono">
                                {displayModelKeysOrAccounts(viewMode, allModels, allAccounts).map((item, index) => (
                                    <div key={item} className="flex items-center gap-1.5 text-gray-600 dark:text-white/60">
                                        <div
                                            className="w-2 h-2 rounded-full"
                                            style={{ backgroundColor: TOP_COLORS[index % TOP_COLORS.length] }}
                                        />
                                        <span className="truncate max-w-[120px]">
                                            {viewMode === 'model' ? item : item.split('@')[0]}
                                        </span>
                                    </div>
                                ))}
                            </div>
                        </div>

                        {/* 经典双栏：左侧 2/3 为 Token 使用趋势堆叠柱状图，右侧 1/3 为分账号统计环形饼图 */}
                        <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
                            {/* 堆叠柱状图 (Stacked Token Usage Trend Chart) */}
                            <div className="lg:col-span-2 bg-white dark:bg-[#121316] rounded-2xl p-4 sm:p-5 border border-gray-200/80 dark:border-white/[0.08] shadow-sm flex flex-col justify-between">
                                <h2 className="text-[13px] font-semibold text-gray-900 dark:text-white/90 mb-3 flex items-center gap-2">
                                    <BarChart3 className="w-4 h-4 text-blue-500" />
                                    {t('token_stats.usage_trend', 'Token 使用趋势')}
                                </h2>
                                <div className="h-56">
                                    {classicChartData.length > 0 ? (
                                        <ResponsiveContainer width="100%" height="100%">
                                            <BarChart data={classicChartData}>
                                                <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="rgba(150, 150, 150, 0.15)" />
                                                <XAxis
                                                    dataKey="period"
                                                    tick={{ fontSize: 10, fill: '#888888', fontFamily: 'monospace' }}
                                                    tickFormatter={(val) => {
                                                        if (timeRange === 'hourly') return val.split(' ')[1] || val;
                                                        if (timeRange === 'daily') return val.split('-').slice(1).join('/');
                                                        if (timeRange === 'weekly') {
                                                            const parts = val.split('-W');
                                                            return parts.length > 1 ? `W${parts[1]}` : val;
                                                        }
                                                        return val;
                                                    }}
                                                    axisLine={false}
                                                    tickLine={false}
                                                    dy={5}
                                                />
                                                <YAxis
                                                    tick={{ fontSize: 10, fill: '#888888', fontFamily: 'monospace' }}
                                                    tickFormatter={(val) => formatNumber(val)}
                                                    axisLine={false}
                                                    tickLine={false}
                                                />
                                                <Tooltip
                                                    content={<UsageTrendTooltip />}
                                                    cursor={{ fill: 'rgba(150, 150, 150, 0.08)' }}
                                                    allowEscapeViewBox={{ x: true, y: true }}
                                                />
                                                <Bar
                                                    dataKey="total_cached_tokens"
                                                    name={t('token_stats.cached_token', '缓存命中')}
                                                    stackId="input"
                                                    fill="#7dd3fc"
                                                    radius={[0, 0, 3, 3]}
                                                    maxBarSize={36}
                                                />
                                                <Bar
                                                    dataKey="uncached_input_tokens"
                                                    name={t('token_stats.input', '输入')}
                                                    stackId="input"
                                                    fill="#3b82f6"
                                                    radius={[3, 3, 0, 0]}
                                                    maxBarSize={36}
                                                />
                                                <Bar
                                                    dataKey="total_output_tokens"
                                                    name={t('token_stats.output', '输出')}
                                                    fill="#a855f7"
                                                    radius={[3, 3, 0, 0]}
                                                    maxBarSize={36}
                                                />
                                            </BarChart>
                                        </ResponsiveContainer>
                                    ) : (
                                        <div className="h-full flex items-center justify-center text-gray-400 dark:text-white/30 text-xs">
                                            {loading ? t('common.loading', '加载中...') : t('token_stats.no_data', '暂无数据')}
                                        </div>
                                    )}
                                </div>
                            </div>

                            {/* 分账号统计饼图 (Account Distribution Pie Chart) */}
                            <div className="bg-white dark:bg-[#121316] rounded-2xl p-4 sm:p-5 border border-gray-200/80 dark:border-white/[0.08] shadow-sm flex flex-col justify-between">
                                <h2 className="text-[13px] font-semibold text-gray-900 dark:text-white/90 mb-2 flex items-center gap-2">
                                    <Users className="w-4 h-4 text-emerald-500" />
                                    {t('token_stats.by_account', '分账号统计')}
                                </h2>
                                <div className="h-40" ref={pieChartContainerRef}>
                                    {pieData.length > 0 ? (
                                        <ResponsiveContainer width="100%" height="100%">
                                            <PieChart
                                                onMouseMove={handlePieChartMouseMove}
                                                onMouseLeave={() => setPieTooltipPosition(undefined)}
                                            >
                                                <Pie
                                                    data={pieData}
                                                    cx="50%"
                                                    cy="50%"
                                                    innerRadius={36}
                                                    outerRadius={62}
                                                    paddingAngle={2}
                                                    dataKey="value"
                                                >
                                                    {pieData.map((entry, index) => (
                                                        <Cell key={`cell-${index}`} fill={entry.color} />
                                                    ))}
                                                </Pie>
                                                <Tooltip
                                                    content={<CustomPieTooltip />}
                                                    allowEscapeViewBox={{ x: true, y: true }}
                                                    position={pieTooltipPosition}
                                                />
                                            </PieChart>
                                        </ResponsiveContainer>
                                    ) : (
                                        <div className="h-full flex items-center justify-center text-gray-400 dark:text-white/30 text-xs">
                                            {loading ? t('common.loading', '加载中...') : t('token_stats.no_data', '暂无数据')}
                                        </div>
                                    )}
                                </div>
                                <div className="space-y-1.5 max-h-28 overflow-y-auto pr-1 scrollbar-thin scrollbar-thumb-gray-200 dark:scrollbar-thumb-white/10 pt-2 border-t border-gray-100 dark:border-white/[0.06]">
                                    {accountData.slice(0, 5).map((account, index) => (
                                        <div key={account.account_email} className="flex items-center justify-between text-xs">
                                            <div className="flex items-center gap-2">
                                                <div
                                                    className="w-2 h-2 rounded-full flex-shrink-0"
                                                    style={{ backgroundColor: PIE_COLORS[index % PIE_COLORS.length] }}
                                                />
                                                <span className="text-gray-600 dark:text-white/70 truncate max-w-[120px]">
                                                    {account.account_email.split('@')[0]}
                                                </span>
                                            </div>
                                            <span className="font-mono font-medium text-gray-800 dark:text-white">
                                                {formatNumber(account.total_tokens)}
                                            </span>
                                        </div>
                                    ))}
                                </div>
                            </div>
                        </div>
                    </>
                )}

                {/* ==================== 底部通用详细统计表格 ==================== */}
                {viewMode === 'model' && modelData.length > 0 && (
                    <div className="bg-white dark:bg-[#121316] rounded-2xl p-4 sm:p-5 border border-gray-200/80 dark:border-white/[0.08] shadow-sm">
                        <div className="flex items-center justify-between mb-4">
                            <h2 className="text-[13px] font-semibold text-gray-900 dark:text-white/90 flex items-center gap-2">
                                <Cpu className="w-4 h-4 text-blue-500" />
                                {t('token_stats.model_details', '分模型详细统计')}
                            </h2>
                            <span className="text-xs text-gray-400 dark:text-white/40 font-mono">
                                {t('token_stats.pricing_snapshot_note', '按照单价快照核算')}
                            </span>
                        </div>
                        <div className="overflow-x-auto">
                            <table className="w-full text-xs">
                                <thead>
                                    <tr className="border-b border-gray-200 dark:border-white/[0.08] text-gray-400 dark:text-white/40 uppercase tracking-wider text-[11px]">
                                        <th className="text-left py-2.5 px-3 font-medium">{t('token_stats.model', '模型')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.requests', '请求数')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.input', '输入')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.output', '输出')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.cached_token', '缓存命中')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.total', '合计')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.percentage', '占比')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.model_cost', '估算费用')}</th>
                                    </tr>
                                </thead>
                                <tbody className="divide-y divide-gray-100 dark:divide-white/[0.04]">
                                    {modelData.map((model, index) => {
                                        const percentage = summary && summary.total_tokens > 0 ? ((model.total_tokens / summary.total_tokens) * 100).toFixed(1) : '0';
                                        const cost = calculateModelCost(model);
                                        return (
                                            <tr key={model.model} className="hover:bg-gray-50/70 dark:hover:bg-white/[0.03] transition-colors">
                                                <td className="py-2.5 px-3">
                                                    <div className="flex items-center gap-2 font-mono">
                                                        <div
                                                            className="w-2 h-2 rounded-full flex-shrink-0"
                                                            style={{ backgroundColor: TOP_COLORS[index % TOP_COLORS.length] }}
                                                        />
                                                        <span className="text-gray-900 dark:text-white/90 font-medium truncate max-w-[180px]" title={model.model}>
                                                            {model.model}
                                                        </span>
                                                    </div>
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono text-gray-500 dark:text-white/60">
                                                    {model.request_count.toLocaleString()}
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono text-blue-600 dark:text-blue-400">
                                                    {formatNumber(model.total_input_tokens)}
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono text-purple-600 dark:text-purple-400">
                                                    {formatNumber(model.total_output_tokens)}
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono text-sky-600 dark:text-sky-400">
                                                    {formatNumber(model.total_cached_tokens)}
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono font-semibold text-gray-900 dark:text-white">
                                                    {formatNumber(model.total_tokens)}
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono text-gray-500 dark:text-white/60">
                                                    {percentage}%
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono font-medium text-emerald-600 dark:text-emerald-400">
                                                    ${cost.toFixed(2)}
                                                </td>
                                            </tr>
                                        );
                                    })}
                                </tbody>
                            </table>
                        </div>
                    </div>
                )}

                {viewMode === 'account' && accountData.length > 0 && (
                    <div className="bg-white dark:bg-[#121316] rounded-2xl p-4 sm:p-5 border border-gray-200/80 dark:border-white/[0.08] shadow-sm">
                        <div className="flex items-center justify-between mb-4">
                            <h2 className="text-[13px] font-semibold text-gray-900 dark:text-white/90 flex items-center gap-2">
                                <Users className="w-4 h-4 text-emerald-500" />
                                {t('token_stats.account_details', '账号详细统计')}
                            </h2>
                        </div>
                        <div className="overflow-x-auto">
                            <table className="w-full text-xs">
                                <thead>
                                    <tr className="border-b border-gray-200 dark:border-white/[0.08] text-gray-400 dark:text-white/40 uppercase tracking-wider text-[11px]">
                                        <th className="text-left py-2.5 px-3 font-medium">{t('token_stats.account', '账号')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.requests', '请求数')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.input', '输入')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.output', '输出')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.cached_token', '缓存命中')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.total', '合计')}</th>
                                    </tr>
                                </thead>
                                <tbody className="divide-y divide-gray-100 dark:divide-white/[0.04]">
                                    {accountData.map((account) => (
                                        <tr key={account.account_email} className="hover:bg-gray-50/70 dark:hover:bg-white/[0.03] transition-colors">
                                            <td className="py-2.5 px-3 font-mono text-gray-900 dark:text-white/90 truncate max-w-[200px]" title={account.account_email}>
                                                {account.account_email}
                                            </td>
                                            <td className="py-2.5 px-3 text-right font-mono text-gray-500 dark:text-white/60">
                                                {account.request_count.toLocaleString()}
                                            </td>
                                            <td className="py-2.5 px-3 text-right font-mono text-blue-600 dark:text-blue-400">
                                                {formatNumber(account.total_input_tokens)}
                                            </td>
                                            <td className="py-2.5 px-3 text-right font-mono text-purple-600 dark:text-purple-400">
                                                {formatNumber(account.total_output_tokens)}
                                            </td>
                                            <td className="py-2.5 px-3 text-right font-mono text-sky-600 dark:text-sky-400">
                                                {formatNumber(account.total_cached_tokens)}
                                            </td>
                                            <td className="py-2.5 px-3 text-right font-mono font-semibold text-gray-900 dark:text-white">
                                                {formatNumber(account.total_tokens)}
                                            </td>
                                        </tr>
                                    ))}
                                </tbody>
                            </table>
                        </div>
                    </div>
                )}

                {/* 自定义模型价格模态框 */}
                <PricingModal
                    isOpen={showPricingModal}
                    onClose={() => setShowPricingModal(false)}
                    currentPricing={pricing}
                    onSave={(newRules) => {
                        setPricing(newRules);
                        try {
                            localStorage.setItem(PRICING_STORAGE_KEY, JSON.stringify(newRules));
                        } catch (e) {}
                        setShowPricingModal(false);
                    }}
                />
            </div>
        </div>
    );
};

function displayModelKeysOrAccounts(viewMode: ViewMode, allModels: string[], allAccounts: string[]): string[] {
    return viewMode === 'model' ? allModels : allAccounts;
}

export default TokenStats;
