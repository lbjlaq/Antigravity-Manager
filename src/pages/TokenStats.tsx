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
    Terminal,
} from 'lucide-react';
import { motion } from 'framer-motion';
import type { DayUsagePoint } from '../components/token_stats/ActivityHeatmapCard';
import type { HourlyUsagePoint } from '../components/token_stats/HourlyTrendBarCard';
import type { ModelShareItem } from '../components/token_stats/TopModelsShareCard';
import { AppleBentoView } from '../components/token_stats/AppleBentoView';
import { CyberCockpitView } from '../components/token_stats/CyberCockpitView';
import { PricingModal } from '../components/token_stats/PricingModal';

export interface TokenStatsAggregated {
    period: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_reasoning_tokens?: number;
    total_tokens: number;
    request_count: number;
    uncached_input_tokens?: number;
}

export interface AccountTokenStats {
    account_email: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_reasoning_tokens?: number;
    total_tokens: number;
    request_count: number;
}

export interface ModelTokenStats {
    model: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_reasoning_tokens?: number;
    total_tokens: number;
    request_count: number;
    avg_ttft_ms?: number;
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
    total_reasoning_tokens?: number;
    total_tokens: number;
    total_requests: number;
    unique_accounts: number;
    avg_ttft_ms?: number;
}

export interface ModelPricingRule {
    input: number; // $ per 1M tokens
    output: number; // $ per 1M tokens
    cached: number; // $ per 1M tokens
}

export const DEFAULT_PRICING: Record<string, ModelPricingRule> = {
    // Google Gemini 3.x & 2.x
    'gemini-3.8-flash': { input: 0.75, output: 3.75, cached: 0.075 },
    'gemini-3.7-flash': { input: 0.75, output: 3.75, cached: 0.075 },
    'gemini-3.6-flash': { input: 0.75, output: 3.75, cached: 0.075 },
    'gemini-3.5-flash': { input: 0.75, output: 3.75, cached: 0.075 },
    'gemini-3-flash': { input: 0.75, output: 3.75, cached: 0.075 },
    'gemini-2.5-flash': { input: 0.75, output: 3.75, cached: 0.075 },
    'gemini-2.0-flash': { input: 0.10, output: 0.40, cached: 0.025 },
    'gemini-3.1-pro': { input: 2.00, output: 12.00, cached: 0.20 },
    'gemini-3-pro': { input: 2.00, output: 12.00, cached: 0.20 },
    'gemini-2.5-pro': { input: 2.00, output: 12.00, cached: 0.20 },
    // Anthropic Claude 4.x & 3.x
    'claude-sonnet-4-6': { input: 3.00, output: 15.00, cached: 0.30 },
    'claude-sonnet-4-5': { input: 3.00, output: 15.00, cached: 0.30 },
    'claude-3-7-sonnet': { input: 3.00, output: 15.00, cached: 0.30 },
    'claude-3-5-sonnet': { input: 3.00, output: 15.00, cached: 0.30 },
    'claude-opus-4-6': { input: 5.00, output: 25.00, cached: 0.50 },
    'claude-opus-4-5': { input: 5.00, output: 25.00, cached: 0.50 },
    'claude-3-opus': { input: 5.00, output: 25.00, cached: 0.50 },
    'claude-haiku-4': { input: 0.80, output: 4.00, cached: 0.08 },
    'claude-3-5-haiku': { input: 0.80, output: 4.00, cached: 0.08 },
    // OpenAI Models
    'gpt-4o': { input: 2.50, output: 10.00, cached: 1.25 },
    'gpt-4o-mini': { input: 0.15, output: 0.60, cached: 0.075 },
    'o3-mini': { input: 1.10, output: 4.40, cached: 0.55 },
    'o1': { input: 15.00, output: 60.00, cached: 7.50 },
    // Default Fallback
    'default': { input: 1.00, output: 5.00, cached: 0.10 },
};

const PRICING_STORAGE_KEY = 'antigravity_model_pricing_v2026';
const DASHBOARD_MODE_KEY = 'antigravity_token_stats_dashboard_mode';
const TIME_RANGE_KEY = 'antigravity_token_stats_time_range';
const MINUTE_SPAN_KEY = 'antigravity_token_stats_minute_span';
const VIEW_MODE_KEY = 'antigravity_token_stats_view_mode';
const MODEL_PERSPECTIVE_KEY = 'antigravity_token_stats_model_perspective';
const CHART_TYPE_KEY = 'antigravity_token_stats_chart_type';
const METRIC_TYPE_KEY = 'antigravity_token_stats_metric_type';

type MinuteSpan = '1h' | '24h';
type TimeRange = 'minute' | 'hourly' | 'daily' | 'weekly' | 'all';
type ViewMode = 'model' | 'account';
type ModelPerspective = 'routed' | 'requested';
type DashboardMode = 'cockpit' | 'insights' | 'classic';
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

    const formatPeriodLabel = useCallback(
        (val: string): string => {
            if (!val) return '';
            const parts = String(val).split('-W');
            if (parts.length > 1) {
                const weekNum = parseInt(parts[1], 10);
                return t('token_stats.year_week', 'Week {{week}}, {{year}}', { year: parts[0], week: weekNum });
            }
            return val;
        },
        [t]
    );

    const [timeRange, setTimeRange] = useState<TimeRange>(() => {
        try {
            const saved = localStorage.getItem(TIME_RANGE_KEY);
            if (saved === 'minute' || saved === 'hourly' || saved === 'daily' || saved === 'weekly' || saved === 'all') return saved;
        } catch (e) { }
        return 'daily';
    });

    const [minuteSpan, setMinuteSpan] = useState<MinuteSpan>(() => {
        try {
            const saved = localStorage.getItem(MINUTE_SPAN_KEY);
            if (saved === '1h' || saved === '24h') return saved;
        } catch (e) { }
        return '1h';
    });

    const switchTimeRange = useCallback((range: TimeRange) => {
        setTimeRange(range);
        try {
            localStorage.setItem(TIME_RANGE_KEY, range);
        } catch (e) { }
    }, []);

    const switchMinuteSpan = useCallback((span: MinuteSpan) => {
        setMinuteSpan(span);
        try {
            localStorage.setItem(MINUTE_SPAN_KEY, span);
        } catch (e) { }
    }, []);

    const formatPeriodAxisTick = useCallback(
        (val: string): string => {
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
        },
        [timeRange, t]
    );

    const [viewMode, setViewMode] = useState<ViewMode>(() => {
        try {
            const saved = localStorage.getItem(VIEW_MODE_KEY);
            if (saved === 'model' || saved === 'account') return saved;
        } catch (e) { }
        return 'model';
    });

    const switchViewMode = useCallback((mode: ViewMode) => {
        setViewMode(mode);
        try {
            localStorage.setItem(VIEW_MODE_KEY, mode);
        } catch (e) { }
    }, []);

    const [modelPerspective, setModelPerspective] = useState<ModelPerspective>(() => {
        try {
            const saved = localStorage.getItem(MODEL_PERSPECTIVE_KEY);
            if (saved === 'routed' || saved === 'requested') return saved;
        } catch (e) { }
        return 'routed';
    });

    const switchModelPerspective = useCallback((perspective: ModelPerspective) => {
        setModelPerspective(perspective);
        try {
            localStorage.setItem(MODEL_PERSPECTIVE_KEY, perspective);
        } catch (e) { }
    }, []);

    const [chartType, setChartType] = useState<'bar' | 'area'>(() => {
        try {
            const saved = localStorage.getItem(CHART_TYPE_KEY);
            if (saved === 'bar' || saved === 'area') return saved;
        } catch (e) { }
        return 'bar';
    });

    const switchChartType = useCallback((type: 'bar' | 'area') => {
        setChartType(type);
        try {
            localStorage.setItem(CHART_TYPE_KEY, type);
        } catch (e) { }
    }, []);

    const [metricType, setMetricType] = useState<MetricType>(() => {
        try {
            const saved = localStorage.getItem(METRIC_TYPE_KEY);
            if (saved === 'tokens' || saved === 'cost') return saved;
        } catch (e) { }
        return 'tokens';
    });

    const switchMetricType = useCallback((metric: MetricType) => {
        setMetricType(metric);
        try {
            localStorage.setItem(METRIC_TYPE_KEY, metric);
        } catch (e) { }
    }, []);

    const [dashboardMode, setDashboardMode] = useState<DashboardMode>(() => {
        try {
            const saved = localStorage.getItem(DASHBOARD_MODE_KEY);
            if (saved === 'cockpit' || saved === 'insights' || saved === 'classic') return saved;
            if (saved === 'apple') return 'insights';
        } catch (e) { }
        return 'cockpit';
    });

    const switchDashboardMode = useCallback((mode: DashboardMode) => {
        setDashboardMode(mode);
        try {
            localStorage.setItem(DASHBOARD_MODE_KEY, mode);
        } catch (e) { }
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
        } catch (e) { }
        return DEFAULT_PRICING;
    });
    const [showPricingModal, setShowPricingModal] = useState(false);

    const getPricing = useCallback((modelName: string): ModelPricingRule => {
        let key = modelName.toLowerCase().trim();
        // 1. 直接精确命中
        if (pricing[key]) return pricing[key];

        // 2. 剥除档位后缀进行规范化匹配 (如 -low, -high, -medium, -tiered, -extra-low, -xhigh, -lite, -default)
        const stripped = key.replace(/-(tiered|max|xhigh|high|medium|default|low|extra-low|lite)$/, '');
        if (pricing[stripped]) return pricing[stripped];

        // 3. 点分与破折号互通匹配 (如 claude-sonnet-4.6 与 claude-sonnet-4-6)
        const normalizedKey = key.replace(/\./g, '-');
        const normalizedStripped = stripped.replace(/\./g, '-');
        if (pricing[normalizedKey]) return pricing[normalizedKey];
        if (pricing[normalizedStripped]) return pricing[normalizedStripped];

        // 4. 子串模式匹配（排除 default）
        for (const [pattern, rule] of Object.entries(pricing)) {
            if (pattern === 'default') continue;
            const normPattern = pattern.toLowerCase().replace(/\./g, '-');
            if (key.includes(pattern) || normalizedKey.includes(normPattern) || normalizedStripped.includes(normPattern)) {
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
            avg_ttft_ms: m.avg_ttft_ms,
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
                case 'minute':
                    const minutes = minuteSpan === '1h' ? 60 : 1440;
                    const stepMinutes = minuteSpan === '1h' ? 1 : 5;
                    hours = minuteSpan === '1h' ? 1 : 24;
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_minute', { minutes, stepMinutes, viewMode: modelPerspective });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_minute', { minutes, stepMinutes });
                    classicDataPromise = invoke<TokenStatsAggregated[]>('get_token_stats_minute', { minutes, stepMinutes });
                    break;
                case 'hourly':
                    hours = 24;
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_hourly', { hours: 24, viewMode: modelPerspective });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_hourly', { hours: 24 });
                    classicDataPromise = invoke<TokenStatsAggregated[]>('get_token_stats_hourly', { hours: 24 });
                    break;
                case 'daily':
                    hours = 24; // 聚焦当天/近 1 日统计
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_daily', { days: 1, viewMode: modelPerspective });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_daily', { days: 1 });
                    classicDataPromise = invoke<TokenStatsAggregated[]>('get_token_stats_daily', { days: 1 });
                    break;
                case 'weekly':
                    hours = 2016; // 12 周 (约 3 个月 = 84 天)
                    const rawDailyModelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_daily', { days: 84, viewMode: modelPerspective });
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
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_daily', { days: 0, viewMode: modelPerspective });
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
                invoke<ModelTokenStats[]>('get_token_stats_by_model', { hours, viewMode: modelPerspective }),
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
    }, [timeRange, minuteSpan, modelPerspective]);

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
            <div className="bg-white/95 dark:bg-base-100/95 backdrop-blur-md p-3 rounded-xl shadow-xl border border-gray-200 dark:border-base-200 text-xs z-[100] min-w-[190px] pointer-events-none text-gray-900 dark:text-base-content">
                <p className="font-semibold text-gray-800 dark:text-base-content mb-2 border-b border-gray-200 dark:border-base-200 pb-1.5 font-mono text-[11px]">
                    {formatPeriodLabel(label)}
                </p>
                <div className="max-h-[200px] overflow-y-auto space-y-1.5 pr-1 scrollbar-thin scrollbar-thumb-gray-200 dark:scrollbar-thumb-base-300">
                    {sortedPayload.map((entry: any, index: number) => {
                        const name = entry.name;
                        const displayName = viewMode === 'model' ? name : name.split('@')[0];
                        return (
                            <div key={index} className="flex items-center justify-between gap-4">
                                <div className="flex items-center gap-2 overflow-hidden">
                                    <div className="w-2 h-2 rounded-full flex-shrink-0" style={{ backgroundColor: entry.color }} />
                                    <span className="text-gray-500 dark:text-gray-400 truncate max-w-[120px]" title={name}>
                                        {displayName}
                                    </span>
                                </div>
                                <span className="font-mono font-medium text-gray-800 dark:text-base-content">
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
            <div className="bg-white/95 dark:bg-base-100/95 backdrop-blur-md p-2.5 rounded-xl shadow-xl border border-gray-200 dark:border-base-200 text-xs z-[100] min-w-[180px] pointer-events-none text-gray-900 dark:text-base-content">
                <p className="font-semibold text-gray-800 dark:text-base-content mb-1.5 border-b border-gray-200 dark:border-base-200 pb-1.5 font-mono">
                    {formatPeriodLabel(label)}
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
            <div className="bg-white/95 dark:bg-base-100/95 backdrop-blur-md p-2.5 rounded-xl shadow-xl border border-gray-200 dark:border-base-200 text-xs z-[100] pointer-events-none text-gray-900 dark:text-base-content">
                <div className="flex items-center gap-2">
                    <div className="w-2 h-2 rounded-full" style={{ backgroundColor: entry.payload.color || entry.color }} />
                    <span className="text-gray-500 dark:text-gray-400 truncate max-w-[150px]">
                        {entry.payload.fullEmail || entry.name}:
                    </span>
                    <span className="font-mono font-medium text-gray-800 dark:text-base-content">
                        {formatNumber(entry.value)}
                    </span>
                </div>
            </div>
        );
    };

    return (
        <div className={`h-full w-full text-gray-800 dark:text-base-content [scrollbar-gutter:stable] ${dashboardMode === 'cockpit' ? 'overflow-hidden flex flex-col' : 'overflow-y-auto'
            }`}>
            <motion.div
                initial={{ opacity: 0, y: 4 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.15, ease: [0.16, 1, 0.3, 1] }}
                style={{ willChange: 'opacity, transform' }}
                className={`w-full max-w-7xl mx-auto p-4 sm:p-5 ${dashboardMode === 'cockpit'
                        ? 'h-full flex flex-col gap-3 overflow-hidden'
                        : 'space-y-4'
                    }`}
            >
                {/* 顶部多功能紧凑控制栏 (已精简冗余大标题，留出更多垂直空间) */}
                <div className="flex flex-wrap items-center justify-between gap-2.5 py-0.5">
                    {/* 左侧组：三看板模式切换器 + 模型视角切换器 */}
                    <div className="flex flex-wrap items-center gap-2">
                        {/* 三看板模式切换器 (Pill Toggle: 赛博大盘 / 深度洞察 / 经典看板) */}
                        <div className="flex bg-gray-100/90 dark:bg-base-200/90 border border-gray-200/80 dark:border-base-300/80 rounded-xl p-0.5 shadow-xs">
                            <button
                                onClick={() => switchDashboardMode('cockpit')}
                                className={`px-2.5 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1.5 ${
                                    dashboardMode === 'cockpit'
                                        ? 'bg-white dark:bg-base-100 text-emerald-600 dark:text-emerald-400 shadow-sm font-semibold'
                                        : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                                }`}
                            >
                                <Terminal className="w-3.5 h-3.5 text-emerald-500 dark:text-emerald-400" />
                                <span>{t('token_stats.mode_cockpit', '总览看板')}</span>
                            </button>
                            <button
                                onClick={() => switchDashboardMode('insights')}
                                className={`px-2.5 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1.5 ${
                                    dashboardMode === 'insights'
                                        ? 'bg-white dark:bg-base-100 text-amber-600 dark:text-amber-400 shadow-sm font-semibold'
                                        : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                                }`}
                            >
                                <Sparkles className="w-3.5 h-3.5 text-amber-500 dark:text-amber-400" />
                                <span>{t('token_stats.mode_insights', '深度洞察')}</span>
                            </button>
                            <button
                                onClick={() => switchDashboardMode('classic')}
                                className={`px-2.5 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1.5 ${
                                    dashboardMode === 'classic'
                                        ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-sm font-semibold'
                                        : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                                }`}
                            >
                                <BarChart3 className="w-3.5 h-3.5 text-blue-500 dark:text-blue-400" />
                                <span>{t('token_stats.mode_classic', '经典看板')}</span>
                            </button>
                        </div>

                        {/* 模型视角切换 (实际路由 / 请求模型) */}
                        <div className="flex bg-gray-100/90 dark:bg-base-200/90 border border-gray-200/80 dark:border-base-300/80 rounded-xl p-0.5 shadow-xs">
                            <button
                                onClick={() => switchModelPerspective('routed')}
                                className={`px-2.5 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1.5 ${
                                    modelPerspective === 'routed'
                                        ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-sm font-semibold'
                                        : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                                }`}
                                title={t('token_stats.perspective_routed_tip', '按实际路由与转发生效的模型统计消耗')}
                            >
                                <Cpu className="w-3.5 h-3.5" />
                                <span>{t('token_stats.perspective_routed', '实际路由')}</span>
                            </button>
                            <button
                                onClick={() => switchModelPerspective('requested')}
                                className={`px-2.5 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1.5 ${
                                    modelPerspective === 'requested'
                                        ? 'bg-white dark:bg-base-100 text-purple-600 dark:text-purple-400 shadow-sm font-semibold'
                                        : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                                }`}
                                title={t('token_stats.perspective_requested_tip', '按客户端发起请求时填写的原始模型统计')}
                            >
                                <Zap className="w-3.5 h-3.5" />
                                <span>{t('token_stats.perspective_requested', '请求模型')}</span>
                            </button>
                        </div>
                    </div>

                    {/* 右侧组：时间段选择药丸 + 单价配置与刷新操作组 */}
                    <div className="flex items-center gap-2">
                        {/* 时间段选择药丸（内联分钟细分） */}
                        <div className="flex items-center bg-gray-100/90 dark:bg-base-200/90 border border-gray-200/80 dark:border-base-300/80 rounded-xl p-0.5 shadow-xs">
                            <button
                                onClick={() => switchTimeRange('minute')}
                                className={`px-2.5 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1 ${
                                    timeRange === 'minute'
                                        ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content shadow-sm font-semibold'
                                        : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                                }`}
                            >
                                <Zap className="w-3.5 h-3.5 text-amber-500" />
                                <span>{t('token_stats.minute', '分钟')}</span>
                            </button>

                            {/* 分钟选中时直接内联无缝嵌入 1h/24h 细分子切项 */}
                            {timeRange === 'minute' && (
                                <div className="flex items-center mx-0.5 bg-gray-200/80 dark:bg-base-300 rounded-md p-0.5 animate-in fade-in duration-150">
                                    <button
                                        onClick={() => switchMinuteSpan('1h')}
                                        className={`px-1.5 py-0.5 rounded text-[11px] font-mono font-medium transition-all ${
                                            minuteSpan === '1h'
                                                ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-xs font-bold'
                                                : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                                        }`}
                                        title={t('token_stats.minute_1h', '1小时 (1m)')}
                                    >
                                        1h
                                    </button>
                                    <button
                                        onClick={() => switchMinuteSpan('24h')}
                                        className={`px-1.5 py-0.5 rounded text-[11px] font-mono font-medium transition-all ${
                                            minuteSpan === '24h'
                                                ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-xs font-bold'
                                                : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                                        }`}
                                        title={t('token_stats.minute_24h', '24小时 (5m)')}
                                    >
                                        24h
                                    </button>
                                </div>
                            )}

                            <button
                                onClick={() => switchTimeRange('hourly')}
                                className={`px-2.5 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1 ${
                                    timeRange === 'hourly'
                                        ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content shadow-sm font-semibold'
                                        : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                                }`}
                                title={t('token_stats.hourly_tip', '按小时汇总 (近 24 小时)')}
                            >
                                <Clock className="w-3.5 h-3.5 text-blue-500" />
                                <span>{t('token_stats.hourly', '小时')}</span>
                            </button>
                            <button
                                onClick={() => switchTimeRange('daily')}
                                className={`px-2.5 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1 ${
                                    timeRange === 'daily'
                                        ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content shadow-sm font-semibold'
                                        : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                                }`}
                                title={t('token_stats.daily_tip', '按日汇总 (近 24 小时 / 今日)')}
                            >
                                <Calendar className="w-3.5 h-3.5 text-emerald-500" />
                                <span>{t('token_stats.daily', '日')}</span>
                            </button>
                            <button
                                onClick={() => switchTimeRange('weekly')}
                                className={`px-2.5 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1 ${
                                    timeRange === 'weekly'
                                        ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content shadow-sm font-semibold'
                                        : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                                }`}
                                title={t('token_stats.weekly_tip', '按周汇总 (近 12 周)')}
                            >
                                <CalendarDays className="w-3.5 h-3.5 text-purple-500" />
                                <span>{t('token_stats.weekly', '周')}</span>
                            </button>
                            <button
                                onClick={() => switchTimeRange('all')}
                                className={`px-2.5 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1 ${
                                    timeRange === 'all'
                                        ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content shadow-sm font-semibold'
                                        : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                                }`}
                                title={t('token_stats.all_time_tip', '历史所有记录统计')}
                            >
                                <History className="w-3.5 h-3.5 text-cyan-500" />
                                <span>{t('token_stats.all_time', '全部历史')}</span>
                            </button>
                        </div>

                        {/* 单价配置与刷新操作组 */}
                        <div className="flex items-center gap-1 bg-gray-100/90 dark:bg-base-200/90 border border-gray-200/80 dark:border-base-300/80 rounded-xl p-0.5 shadow-xs">
                            <button
                                onClick={() => setShowPricingModal(true)}
                                title={t('token_stats.custom_pricing', '自定义单价')}
                                className="p-1.5 rounded-lg text-gray-600 dark:text-gray-300 hover:text-gray-900 dark:hover:text-white hover:bg-white dark:hover:bg-base-100 transition-all shadow-none hover:shadow-xs"
                            >
                                <Settings2 className="w-4 h-4" />
                            </button>
                            <button
                                onClick={fetchData}
                                disabled={loading}
                                title={t('common.refresh', '刷新数据')}
                                className="p-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 text-white transition-all disabled:opacity-50 shadow-xs"
                            >
                                <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
                            </button>
                        </div>
                    </div>
                </div>

                {/* ==================== 0. 赛博大盘模式 (Cyber Cockpit View) ==================== */}
                {dashboardMode === 'cockpit' && (
                    <div className="flex-1 min-h-0 overflow-hidden">
                        <CyberCockpitView
                            summary={summary}
                            hourlyData={hourlyTrendData}
                            timelineData={classicChartData}
                            modelShareItems={modelShareItems}
                            accountData={accountData}
                            timeRange={timeRange}
                            totalEstimatedCost={totalEstimatedCost}
                            cacheMetrics={cacheMetrics}
                            formatNumber={formatNumber}
                            formatCurrency={formatCurrency}
                            setShowPricingModal={setShowPricingModal}
                        />
                    </div>
                )}

                {/* ==================== 1. 深度洞察看板 (Apple Bento View) ==================== */}
                {dashboardMode === 'insights' && (
                    <AppleBentoView
                        summary={summary}
                        dailyData={heatmapData}
                        hourlyData={hourlyTrendData}
                        timelineData={classicChartData}
                        modelShareItems={modelShareItems}
                        accountData={accountData}
                        displayModelTrendData={displayModelTrendData}
                        displayAccountTrendData={displayAccountTrendData}
                        allModels={allModels}
                        allAccounts={allAccounts}
                        viewMode={viewMode}
                        metricType={metricType}
                        chartType={chartType}
                        timeRange={timeRange}
                        totalEstimatedCost={totalEstimatedCost}
                        cacheMetrics={cacheMetrics}
                        formatNumber={formatNumber}
                        formatCurrency={formatCurrency}
                        switchChartType={switchChartType}
                        switchMetricType={switchMetricType}
                        switchViewMode={switchViewMode}
                        setShowPricingModal={setShowPricingModal}
                        customTooltip={CustomTrendTooltip}
                    />
                )}

                {/* ==================== 3. 经典看板模式 (Classic View) ==================== */}
                {dashboardMode === 'classic' && (
                    <>
                        {/* 经典独立指标卡片 */}
                        {summary && (
                            <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-7 gap-3">
                                {/* 总 Token */}
                                <div className="bg-white dark:bg-base-100 rounded-2xl p-3.5 shadow-sm border border-gray-200/80 dark:border-base-200 flex flex-col justify-between">
                                    <div className="flex items-center gap-2 text-gray-500 dark:text-gray-400 text-xs mb-2">
                                        <div className="p-1.5 rounded-lg bg-gray-100 dark:bg-base-200">
                                            <Zap className="w-3.5 h-3.5 text-gray-600 dark:text-gray-300" />
                                        </div>
                                        <span>{t('token_stats.total_tokens', '总 Token')}</span>
                                    </div>
                                    <div className="text-xl font-bold font-mono tracking-tight text-gray-900 dark:text-base-content">
                                        {formatNumber(summary.total_tokens)}
                                    </div>
                                </div>

                                {/* 输入 Token */}
                                <div className="bg-white dark:bg-base-100 rounded-2xl p-3.5 shadow-sm border border-gray-200/80 dark:border-base-200 flex flex-col justify-between">
                                    <div className="flex items-center gap-2 text-blue-600 dark:text-blue-400 text-xs mb-2">
                                        <div className="p-1.5 rounded-lg bg-blue-50 dark:bg-blue-500/10">
                                            <TrendingUp className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" />
                                        </div>
                                        <span>{t('token_stats.input_tokens', '输入 Token')}</span>
                                    </div>
                                    <div className="text-xl font-bold font-mono tracking-tight text-blue-600 dark:text-blue-400">
                                        {formatNumber(summary.total_input_tokens)}
                                    </div>
                                </div>

                                {/* 输出 Token */}
                                <div className="bg-white dark:bg-base-100 rounded-2xl p-3.5 shadow-sm border border-gray-200/80 dark:border-base-200 flex flex-col justify-between">
                                    <div className="flex items-center gap-2 text-purple-600 dark:text-purple-400 text-xs mb-2">
                                        <div className="p-1.5 rounded-lg bg-purple-50 dark:bg-purple-500/10">
                                            <TrendingUp className="w-3.5 h-3.5 rotate-180 text-purple-600 dark:text-purple-400" />
                                        </div>
                                        <span>{t('token_stats.output_tokens', '输出 Token')}</span>
                                    </div>
                                    <div className="text-xl font-bold font-mono tracking-tight text-purple-600 dark:text-purple-400">
                                        {formatNumber(summary.total_output_tokens)}
                                    </div>
                                </div>

                                {/* 缓存命中 */}
                                <div className="bg-white dark:bg-base-100 rounded-2xl p-3.5 shadow-sm border border-gray-200/80 dark:border-base-200 flex flex-col justify-between">
                                    <div className="flex items-center gap-2 text-sky-600 dark:text-sky-400 text-xs mb-2">
                                        <div className="p-1.5 rounded-lg bg-sky-50 dark:bg-sky-500/10">
                                            <Database className="w-3.5 h-3.5 text-sky-600 dark:text-sky-400" />
                                        </div>
                                        <span>{t('token_stats.cached_token', '缓存命中')}</span>
                                    </div>
                                    <div className="text-xl font-bold font-mono tracking-tight text-sky-600 dark:text-sky-400">
                                        {formatNumber(summary.total_cached_tokens)}
                                    </div>
                                </div>

                                {/* 平均首字延迟 */}
                                <div className="bg-white dark:bg-base-100 rounded-2xl p-3.5 shadow-sm border border-gray-200/80 dark:border-base-200 flex flex-col justify-between">
                                    <div className="flex items-center gap-2 text-teal-600 dark:text-teal-400 text-xs mb-2">
                                        <div className="p-1.5 rounded-lg bg-teal-50 dark:bg-teal-500/10">
                                            <Clock className="w-3.5 h-3.5 text-teal-600 dark:text-teal-400" />
                                        </div>
                                        <span title={t('token_stats.avg_ttft_tip', '全局平均首字延迟')}>{t('token_stats.avg_ttft', '首字延迟')}</span>
                                    </div>
                                    <div className="text-xl font-bold font-mono tracking-tight text-teal-600 dark:text-teal-400">
                                        {summary.avg_ttft_ms && summary.avg_ttft_ms > 0
                                            ? summary.avg_ttft_ms < 1000
                                                ? `${summary.avg_ttft_ms.toFixed(0)} ms`
                                                : `${(summary.avg_ttft_ms / 1000).toFixed(2)} s`
                                            : '-'}
                                    </div>
                                </div>

                                {/* 活跃账号 */}
                                <div className="bg-white dark:bg-base-100 rounded-2xl p-3.5 shadow-sm border border-gray-200/80 dark:border-base-200 flex flex-col justify-between">
                                    <div className="flex items-center gap-2 text-emerald-600 dark:text-emerald-400 text-xs mb-2">
                                        <div className="p-1.5 rounded-lg bg-emerald-50 dark:bg-emerald-500/10">
                                            <Users className="w-3.5 h-3.5 text-emerald-600 dark:text-emerald-400" />
                                        </div>
                                        <span>{t('token_stats.accounts_used', '活跃账号')}</span>
                                    </div>
                                    <div className="text-xl font-bold font-mono tracking-tight text-emerald-600 dark:text-emerald-400">
                                        {summary.unique_accounts}
                                    </div>
                                </div>

                                {/* 使用模型 */}
                                <div className="bg-white dark:bg-base-100 rounded-2xl p-3.5 shadow-sm border border-gray-200/80 dark:border-base-200 flex flex-col justify-between">
                                    <div className="flex items-center gap-2 text-amber-600 dark:text-amber-400 text-xs mb-2">
                                        <div className="p-1.5 rounded-lg bg-amber-50 dark:bg-amber-500/10">
                                            <Cpu className="w-3.5 h-3.5 text-amber-600 dark:text-amber-400" />
                                        </div>
                                        <span>{t('token_stats.models_used', '使用模型')}</span>
                                    </div>
                                    <div className="text-xl font-bold font-mono tracking-tight text-amber-600 dark:text-amber-400">
                                        {modelData.length}
                                    </div>
                                </div>
                            </div>
                        )}

                        {/* 全宽趋势图 (Full-Width Trend Chart) */}
                        <div className="bg-white dark:bg-base-100 rounded-2xl p-4 sm:p-5 border border-gray-200/80 dark:border-base-200 shadow-sm flex flex-col justify-between">
                            <div className="flex flex-wrap items-center justify-between gap-2 mb-3">
                                <div className="flex items-center gap-2">
                                    {chartType === 'bar' ? (
                                        <BarChart3 className="w-4 h-4 text-blue-500 dark:text-blue-400 opacity-90" />
                                    ) : (
                                        <TrendingUp className="w-4 h-4 text-blue-500 dark:text-blue-400 opacity-90" />
                                    )}
                                    <span className="text-[13px] font-semibold text-gray-900 dark:text-base-content tracking-wide">
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
                                    <div className="flex bg-gray-100 dark:bg-base-200 border border-gray-200 dark:border-base-300 rounded-xl p-0.5">
                                        <button
                                            onClick={() => switchChartType('bar')}
                                            className={`px-2 py-1 text-xs font-medium rounded-lg transition-all flex items-center gap-1 ${chartType === 'bar'
                                                    ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content shadow-sm'
                                                    : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                                                }`}
                                        >
                                            <BarChart3 className="w-3.5 h-3.5" />
                                            <span>{t('token_stats.chart_type_bar', '柱状')}</span>
                                        </button>
                                        <button
                                            onClick={() => switchChartType('area')}
                                            className={`px-2 py-1 text-xs font-medium rounded-lg transition-all flex items-center gap-1 ${chartType === 'area'
                                                    ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content shadow-sm'
                                                    : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                                                }`}
                                        >
                                            <TrendingUp className="w-3.5 h-3.5" />
                                            <span>{t('token_stats.chart_type_area', '面积')}</span>
                                        </button>
                                    </div>

                                    {/* Token / 金额 维度切换 */}
                                    <div className="flex bg-gray-100 dark:bg-base-200 border border-gray-200 dark:border-base-300 rounded-xl p-0.5">
                                        <button
                                            onClick={() => switchMetricType('tokens')}
                                            className={`px-2 py-1 text-xs font-medium rounded-lg transition-all flex items-center gap-1 ${metricType === 'tokens'
                                                    ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content shadow-sm'
                                                    : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                                                }`}
                                        >
                                            <Zap className="w-3.5 h-3.5" />
                                            <span>{t('token_stats.metric_tokens', 'Token')}</span>
                                        </button>
                                        <button
                                            onClick={() => switchMetricType('cost')}
                                            className={`px-2 py-1 text-xs font-medium rounded-lg transition-all flex items-center gap-1 ${metricType === 'cost'
                                                    ? 'bg-white dark:bg-base-100 text-emerald-600 dark:text-emerald-400 shadow-sm font-semibold'
                                                    : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                                                }`}
                                        >
                                            <DollarSign className="w-3.5 h-3.5" />
                                            <span>{t('token_stats.metric_cost', '金额')}</span>
                                        </button>
                                    </div>

                                    {/* 模型 / 账号 维度切换 */}
                                    <div className="flex bg-gray-100 dark:bg-base-200 border border-gray-200 dark:border-base-300 rounded-xl p-0.5">
                                        <button
                                            onClick={() => switchViewMode('model')}
                                            className={`px-2.5 py-1 text-xs font-medium rounded-lg transition-all ${viewMode === 'model'
                                                    ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content shadow-sm'
                                                    : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
                                                }`}
                                        >
                                            {t('token_stats.by_model', '按模型')}
                                        </button>
                                        <button
                                            onClick={() => switchViewMode('account')}
                                            className={`px-2.5 py-1 text-xs font-medium rounded-lg transition-all ${viewMode === 'account'
                                                    ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content shadow-sm'
                                                    : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-base-content'
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
                                                    tickFormatter={formatPeriodAxisTick}
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
                                                            isAnimationActive={false}
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
                                                    tickFormatter={formatPeriodAxisTick}
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
                                                        isAnimationActive={false}
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
                            <div className="lg:col-span-2 bg-white dark:bg-base-100 rounded-2xl p-4 sm:p-5 border border-gray-200/80 dark:border-base-200 shadow-sm flex flex-col justify-between">
                                <h2 className="text-[13px] font-semibold text-gray-900 dark:text-base-content mb-3 flex items-center gap-2">
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
                                                    tickFormatter={formatPeriodAxisTick}
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
                                        <div className="h-full flex items-center justify-center text-gray-400 dark:text-gray-500 text-xs">
                                            {loading ? t('common.loading', '加载中...') : t('token_stats.no_data', '暂无数据')}
                                        </div>
                                    )}
                                </div>
                            </div>

                            {/* 分账号统计饼图 (Account Distribution Pie Chart) */}
                            <div className="bg-white dark:bg-base-100 rounded-2xl p-4 sm:p-5 border border-gray-200/80 dark:border-base-200 shadow-sm flex flex-col justify-between">
                                <h2 className="text-[13px] font-semibold text-gray-900 dark:text-base-content mb-2 flex items-center gap-2">
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
                                        <div className="h-full flex items-center justify-center text-gray-400 dark:text-gray-500 text-xs">
                                            {loading ? t('common.loading', '加载中...') : t('token_stats.no_data', '暂无数据')}
                                        </div>
                                    )}
                                </div>
                                <div className="space-y-1.5 max-h-28 overflow-y-auto pr-1 scrollbar-thin scrollbar-thumb-gray-200 dark:scrollbar-thumb-base-300 pt-2 border-t border-gray-100 dark:border-base-200">
                                    {accountData.slice(0, 5).map((account, index) => (
                                        <div key={account.account_email} className="flex items-center justify-between text-xs">
                                            <div className="flex items-center gap-2">
                                                <div
                                                    className="w-2 h-2 rounded-full flex-shrink-0"
                                                    style={{ backgroundColor: PIE_COLORS[index % PIE_COLORS.length] }}
                                                />
                                                <span className="text-gray-600 dark:text-gray-300 truncate max-w-[120px]">
                                                    {account.account_email.split('@')[0]}
                                                </span>
                                            </div>
                                            <span className="font-mono font-medium text-gray-800 dark:text-base-content">
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
                {dashboardMode === 'classic' && viewMode === 'model' && modelData.length > 0 && (
                    <div className="bg-white dark:bg-base-100 rounded-2xl p-4 sm:p-5 border border-gray-200/80 dark:border-base-200 shadow-sm">
                        <div className="flex items-center justify-between mb-4">
                            <h2 className="text-[13px] font-semibold text-gray-900 dark:text-base-content flex items-center gap-2">
                                <Cpu className="w-4 h-4 text-blue-500" />
                                {t('token_stats.model_details', '分模型详细统计')}
                                <span className="text-[11px] font-normal px-2 py-0.5 rounded-full bg-gray-100 dark:bg-base-200 text-gray-500 dark:text-gray-400">
                                    {modelPerspective === 'routed'
                                        ? t('token_stats.perspective_routed', '实际路由')
                                        : t('token_stats.perspective_requested', '请求模型')}
                                </span>
                            </h2>
                            <span className="text-xs text-gray-400 dark:text-gray-400 font-mono">
                                {t('token_stats.pricing_snapshot_note', '按照单价快照核算')}
                            </span>
                        </div>
                        <div className="overflow-x-auto">
                            <table className="w-full text-xs">
                                <thead>
                                    <tr className="border-b border-gray-200 dark:border-base-200 text-gray-400 dark:text-gray-400 uppercase tracking-wider text-[11px]">
                                        <th className="text-left py-2.5 px-3 font-medium">{t('token_stats.model', '模型')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.requests', '请求数')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.input', '输入')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.output', '输出')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.cached_token', '缓存命中')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.total', '合计')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium" title={t('token_stats.avg_ttft_tip', '平均首字响应耗时')}>{t('token_stats.ttft', '首字耗时')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.percentage', '占比')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.model_cost', '估算费用')}</th>
                                    </tr>
                                </thead>
                                <tbody className="divide-y divide-gray-100 dark:divide-base-200">
                                    {modelData.map((model, index) => {
                                        const percentage = summary && summary.total_tokens > 0 ? ((model.total_tokens / summary.total_tokens) * 100).toFixed(1) : '0';
                                        const cost = calculateModelCost(model);
                                        return (
                                            <tr key={model.model} className="hover:bg-gray-50/70 dark:hover:bg-base-200/50 transition-colors">
                                                <td className="py-2.5 px-3">
                                                    <div className="flex items-center gap-2 font-mono">
                                                        <div
                                                            className="w-2 h-2 rounded-full flex-shrink-0"
                                                            style={{ backgroundColor: TOP_COLORS[index % TOP_COLORS.length] }}
                                                        />
                                                        <span className="text-gray-900 dark:text-base-content font-medium truncate max-w-[180px]" title={model.model}>
                                                            {model.model}
                                                        </span>
                                                    </div>
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono text-gray-500 dark:text-gray-400">
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
                                                <td className="py-2.5 px-3 text-right font-mono font-semibold text-gray-900 dark:text-base-content">
                                                    {formatNumber(model.total_tokens)}
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono text-teal-600 dark:text-teal-400">
                                                    {model.avg_ttft_ms && model.avg_ttft_ms > 0
                                                        ? model.avg_ttft_ms < 1000
                                                            ? `${model.avg_ttft_ms.toFixed(0)} ms`
                                                            : `${(model.avg_ttft_ms / 1000).toFixed(2)} s`
                                                        : '-'}
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono text-gray-500 dark:text-gray-400">
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

                {dashboardMode === 'classic' && viewMode === 'account' && accountData.length > 0 && (
                    <div className="bg-white dark:bg-base-100 rounded-2xl p-4 sm:p-5 border border-gray-200/80 dark:border-base-200 shadow-sm">
                        <div className="flex items-center justify-between mb-4">
                            <h2 className="text-[13px] font-semibold text-gray-900 dark:text-base-content flex items-center gap-2">
                                <Users className="w-4 h-4 text-emerald-500" />
                                {t('token_stats.account_details', '账号详细统计')}
                            </h2>
                        </div>
                        <div className="overflow-x-auto">
                            <table className="w-full text-xs">
                                <thead>
                                    <tr className="border-b border-gray-200 dark:border-base-200 text-gray-400 dark:text-gray-400 uppercase tracking-wider text-[11px]">
                                        <th className="text-left py-2.5 px-3 font-medium">{t('token_stats.account', '账号')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.requests', '请求数')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.input', '输入')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.output', '输出')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.cached_token', '缓存命中')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.total', '合计')}</th>
                                    </tr>
                                </thead>
                                <tbody className="divide-y divide-gray-100 dark:divide-base-200">
                                    {accountData.map((account) => (
                                        <tr key={account.account_email} className="hover:bg-gray-50/70 dark:hover:bg-base-200/50 transition-colors">
                                            <td className="py-2.5 px-3 font-mono text-gray-900 dark:text-base-content truncate max-w-[200px]" title={account.account_email}>
                                                {account.account_email}
                                            </td>
                                            <td className="py-2.5 px-3 text-right font-mono text-gray-500 dark:text-gray-400">
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
                                            <td className="py-2.5 px-3 text-right font-mono font-semibold text-gray-900 dark:text-base-content">
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
                        } catch (e) { }
                        setShowPricingModal(false);
                    }}
                />
            </motion.div>
        </div>
    );
};

function displayModelKeysOrAccounts(viewMode: ViewMode, allModels: string[], allAccounts: string[]): string[] {
    return viewMode === 'model' ? allModels : allAccounts;
}

export default TokenStats;
