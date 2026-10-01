import React, { useEffect, useState, useRef, useCallback, useMemo } from 'react';
import { request as invoke } from '../utils/request';
import { useTranslation } from 'react-i18next';
import { BarChart, Bar, AreaChart, Area, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts';
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
    'default': { input: 1.00, output: 5.00, cached: 0.10 }
};

const PRICING_STORAGE_KEY = 'antigravity_model_pricing_v2026';

type TimeRange = 'hourly' | 'daily' | 'weekly' | 'all';
type ViewMode = 'model' | 'account';

const TOP_COLORS = ['#d97757', '#4f85e8', '#10a37f', '#a855f7', '#f59e0b', '#71717a'];

const formatNumber = (num: number): string => {
    if (num >= 1000000000) return `${(num / 1000000000).toFixed(1)}B`;
    if (num >= 1000000) return `${(num / 1000000).toFixed(1)}M`;
    if (num >= 1000) return `${(num / 1000).toFixed(1)}K`;
    return num.toString();
};

const TokenStats: React.FC = () => {
    const { t } = useTranslation();
    const [timeRange, setTimeRange] = useState<TimeRange>('daily');
    const [viewMode, setViewMode] = useState<ViewMode>('model');
    const [chartType, setChartType] = useState<'bar' | 'area'>('bar');
    const [accountData, setAccountData] = useState<AccountTokenStats[]>([]);
    const [modelData, setModelData] = useState<ModelTokenStats[]>([]);
    const [modelTrendData, setModelTrendData] = useState<any[]>([]);
    const [accountTrendData, setAccountTrendData] = useState<any[]>([]);
    const [allModels, setAllModels] = useState<string[]>([]);
    const [allAccounts, setAllAccounts] = useState<string[]>([]);
    const [summary, setSummary] = useState<TokenStatsSummary | null>(null);
    const [loading, setLoading] = useState(true);

    // 专属 13 周热力图数据与 24 小时紧凑柱状图数据
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

    // 缓存命中率与节约成本估算
    const cacheMetrics = useMemo(() => {
        if (!summary || summary.total_tokens === 0) return { hitRate: 0, savings: 0 };
        const totalInput = summary.total_input_tokens;
        const cached = summary.total_cached_tokens;
        const hitRate = totalInput > 0 ? (cached / totalInput) * 100 : 0;
        // 估算节约：假设平均每1M缓存比直接输入便宜约 $1.50
        const savings = (cached * 1.5) / 1_000_000;
        return { hitRate, savings };
    }, [summary]);

    const fetchData = async () => {
        setLoading(true);
        try {
            let hours = 24;
            let modelTrend: ModelTrendPoint[] = [];
            let accountTrend: AccountTrendPoint[] = [];

            switch (timeRange) {
                case 'hourly':
                    hours = 24;
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_hourly', { hours: 24 });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_hourly', { hours: 24 });
                    break;
                case 'daily':
                    hours = 168;
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_daily', { days: 7 });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_daily', { days: 7 });
                    break;
                case 'weekly':
                    hours = 720;
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_daily', { days: 30 });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_daily', { days: 30 });
                    break;
                case 'all':
                    hours = 0;
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_daily', { days: 0 });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_daily', { days: 0 });
                    break;
            }

            // 处理模型趋势数据：提取 Top 5 模型，长尾自动归并到 Other，避免图例与彩带过多导致画面杂乱
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
            const displayModelKeys = hasOther ? [...top5Models, '其他模型'] : top5Models;
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
                    row['其他模型'] = otherSum;
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

            // 并行获取账户统计、模型统计、全局总结，以及 13周热力图与 24小时柱状图专属数据
            const [accounts, models_stats, summaryData, raw91Days, raw24Hours] = await Promise.all([
                invoke<AccountTokenStats[]>('get_token_stats_by_account', { hours }),
                invoke<ModelTokenStats[]>('get_token_stats_by_model', { hours }),
                invoke<TokenStatsSummary>('get_token_stats_summary', { hours }),
                invoke<TokenStatsAggregated[]>('get_token_stats_daily', { days: 91 }),
                invoke<TokenStatsAggregated[]>('get_token_stats_hourly', { hours: 24 }),
            ]);

            setAccountData(accounts);
            setModelData(models_stats);
            setSummary(summaryData);

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

    // 趋势图高质感深色 Tooltip
    const CustomTrendTooltip = ({ active, payload, label }: any) => {
        if (!active || !payload || !payload.length) return null;
        const sortedPayload = [...payload].sort((a: any, b: any) => b.value - a.value);

        return (
            <div className="bg-[#181a20]/95 backdrop-blur-md p-3 rounded-xl shadow-2xl border border-white/[0.08] text-xs z-[100] min-w-[190px] pointer-events-none text-white">
                <p className="font-semibold text-white/90 mb-2 border-b border-white/[0.08] pb-1.5 font-mono text-[11px]">
                    {label}
                </p>
                <div className="max-h-[200px] overflow-y-auto space-y-1.5 pr-1 scrollbar-thin scrollbar-thumb-white/10">
                    {sortedPayload.map((entry: any, index: number) => {
                        const name = entry.name;
                        const displayName = viewMode === 'model' ? name : name.split('@')[0];
                        return (
                            <div key={index} className="flex items-center justify-between gap-4">
                                <div className="flex items-center gap-2 overflow-hidden">
                                    <div className="w-2 h-2 rounded-full flex-shrink-0" style={{ backgroundColor: entry.color }} />
                                    <span className="text-white/60 truncate max-w-[120px]" title={name}>
                                        {displayName}
                                    </span>
                                </div>
                                <span className="font-mono font-medium text-white/90">
                                    {formatNumber(entry.value)}
                                </span>
                            </div>
                        );
                    })}
                </div>
            </div>
        );
    };

    return (
        <div className="h-full w-full overflow-y-auto bg-[#0a0a0c] dark:bg-[#0a0a0c] text-white/90">
            <div className="p-4 sm:p-6 space-y-5 max-w-7xl mx-auto">
                {/* 顶部标题与控制器栏 */}
                <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4">
                    <div className="flex items-center gap-3">
                        <div className="p-2 rounded-xl bg-blue-500/10 border border-blue-500/20 text-blue-400">
                            <Zap className="w-5 h-5" />
                        </div>
                        <div>
                            <h1 className="text-xl font-bold tracking-tight text-white flex items-center gap-2">
                                {t('token_stats.title', 'Token 消费统计')}
                            </h1>
                            <p className="text-xs text-white/40 mt-0.5">
                                全局 Token 消耗流向、模型定价与活跃全景
                            </p>
                        </div>
                    </div>

                    <div className="flex items-center gap-2.5">
                        {/* 极简深色时间段选择药丸 */}
                        <div className="flex bg-[#16181d] border border-white/[0.08] rounded-xl p-1 shadow-sm">
                            <button
                                onClick={() => setTimeRange('hourly')}
                                className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1.5 ${
                                    timeRange === 'hourly'
                                        ? 'bg-white/[0.12] text-white shadow-sm'
                                        : 'text-white/50 hover:text-white/80'
                                }`}
                            >
                                <Clock className="w-3.5 h-3.5" />
                                {t('token_stats.hourly', '小时')}
                            </button>
                            <button
                                onClick={() => setTimeRange('daily')}
                                className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1.5 ${
                                    timeRange === 'daily'
                                        ? 'bg-white/[0.12] text-white shadow-sm'
                                        : 'text-white/50 hover:text-white/80'
                                }`}
                            >
                                <Calendar className="w-3.5 h-3.5" />
                                {t('token_stats.daily', '日')}
                            </button>
                            <button
                                onClick={() => setTimeRange('weekly')}
                                className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1.5 ${
                                    timeRange === 'weekly'
                                        ? 'bg-white/[0.12] text-white shadow-sm'
                                        : 'text-white/50 hover:text-white/80'
                                }`}
                            >
                                <CalendarDays className="w-3.5 h-3.5" />
                                {t('token_stats.weekly', '周')}
                            </button>
                            <button
                                onClick={() => setTimeRange('all')}
                                className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all flex items-center gap-1.5 ${
                                    timeRange === 'all'
                                        ? 'bg-white/[0.12] text-white shadow-sm'
                                        : 'text-white/50 hover:text-white/80'
                                }`}
                            >
                                <History className="w-3.5 h-3.5" />
                                {t('token_stats.all_time', '全部')}
                            </button>
                        </div>

                        {/* 单价配置按钮 */}
                        <button
                            onClick={() => setShowPricingModal(true)}
                            title={t('token_stats.custom_pricing', '自定义单价')}
                            className="p-2 rounded-xl bg-[#16181d] border border-white/[0.08] text-white/70 hover:text-white hover:bg-white/[0.06] transition-colors"
                        >
                            <Settings2 className="w-4 h-4" />
                        </button>

                        {/* 刷新按钮 */}
                        <button
                            onClick={fetchData}
                            disabled={loading}
                            className="p-2 rounded-xl bg-blue-600 hover:bg-blue-500 text-white transition-colors disabled:opacity-50 shadow-sm shadow-blue-900/30"
                        >
                            <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
                        </button>
                    </div>
                </div>

                {/* 核心指标微卡片（Apple Dark Surface 质感） */}
                {summary && (
                    <div className="grid grid-cols-2 md:grid-cols-4 gap-3.5">
                        {/* 总 Token */}
                        <div className="bg-[#121316] border border-white/[0.08] rounded-2xl p-4 shadow-sm hover:border-white/[0.15] transition-all flex flex-col justify-between">
                            <div className="flex items-center justify-between text-white/50 text-xs">
                                <span>{t('token_stats.total_tokens', '总 Token')}</span>
                                <div className="p-1.5 rounded-lg bg-white/[0.04]">
                                    <Zap className="w-3.5 h-3.5 text-white/70" />
                                </div>
                            </div>
                            <div className="mt-2 text-2xl font-bold font-mono tracking-tight text-white">
                                {formatNumber(summary.total_tokens)}
                            </div>
                            <div className="text-[11px] text-white/40 mt-1 font-mono">
                                入 {formatNumber(summary.total_input_tokens)} · 出 {formatNumber(summary.total_output_tokens)}
                            </div>
                        </div>

                        {/* 估算价值 */}
                        <div className="bg-[#121316] border border-white/[0.08] rounded-2xl p-4 shadow-sm hover:border-white/[0.15] transition-all flex flex-col justify-between">
                            <div className="flex items-center justify-between text-emerald-400/80 text-xs">
                                <span className="font-medium">{t('token_stats.estimated_value', '估算价值')}</span>
                                <div className="p-1.5 rounded-lg bg-emerald-500/10">
                                    <DollarSign className="w-3.5 h-3.5 text-emerald-400" />
                                </div>
                            </div>
                            <div className="mt-2 text-2xl font-bold font-mono tracking-tight text-emerald-400">
                                ${totalEstimatedCost.toFixed(2)}
                            </div>
                            <div className="text-[11px] text-white/40 mt-1 flex items-center justify-between">
                                <span>单价快照累计</span>
                                <button
                                    onClick={() => setShowPricingModal(true)}
                                    className="text-[10px] text-emerald-400 hover:underline"
                                >
                                    配置单价
                                </button>
                            </div>
                        </div>

                        {/* 缓存命中 */}
                        <div className="bg-[#121316] border border-white/[0.08] rounded-2xl p-4 shadow-sm hover:border-white/[0.15] transition-all flex flex-col justify-between">
                            <div className="flex items-center justify-between text-sky-400/80 text-xs">
                                <span className="font-medium">{t('token_stats.cached_token', '缓存命中')}</span>
                                <div className="p-1.5 rounded-lg bg-sky-500/10">
                                    <Database className="w-3.5 h-3.5 text-sky-400" />
                                </div>
                            </div>
                            <div className="mt-2 text-2xl font-bold font-mono tracking-tight text-sky-400 flex items-baseline gap-2">
                                <span>{formatNumber(summary.total_cached_tokens)}</span>
                                <span className="text-xs font-normal px-1.5 py-0.5 rounded-full bg-sky-500/10 border border-sky-500/20 text-sky-300">
                                    {cacheMetrics.hitRate.toFixed(1)}%
                                </span>
                            </div>
                            <div className="text-[11px] text-white/40 mt-1 font-mono">
                                节省约 ${cacheMetrics.savings.toFixed(2)}
                            </div>
                        </div>

                        {/* 活跃生态 */}
                        <div className="bg-[#121316] border border-white/[0.08] rounded-2xl p-4 shadow-sm hover:border-white/[0.15] transition-all flex flex-col justify-between">
                            <div className="flex items-center justify-between text-purple-400/80 text-xs">
                                <span className="font-medium">活跃生态</span>
                                <div className="p-1.5 rounded-lg bg-purple-500/10">
                                    <Users className="w-3.5 h-3.5 text-purple-400" />
                                </div>
                            </div>
                            <div className="mt-2 text-2xl font-bold font-mono tracking-tight text-white flex items-baseline gap-3">
                                <span>{summary.unique_accounts} <span className="text-xs text-white/50 font-normal">账号</span></span>
                                <span className="text-purple-400">{modelData.length} <span className="text-xs text-white/50 font-normal">模型</span></span>
                            </div>
                            <div className="text-[11px] text-white/40 mt-1 font-mono">
                                共 {summary.total_requests.toLocaleString()} 次请求
                            </div>
                        </div>
                    </div>
                )}

                {/* 核心亮点：图2同款两大卡片（活动热力图 + 24小时极简圆角柱条） */}
                <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                    {/* 活动热力图（13周方块日历、Streak火焰、活跃天数、峰值日） */}
                    <ActivityHeatmapCard
                        dailyData={heatmapData}
                        totalEstimatedCost={totalEstimatedCost}
                        formatNumber={formatNumber}
                    />

                    {/* 24小时紧凑趋势柱状图（0/12/23时、零消耗底线胶囊、悬浮动态副标题） */}
                    <HourlyTrendBarCard
                        hourlyData={hourlyTrendData}
                        formatNumber={formatNumber}
                    />
                </div>

                {/* 中层区域：主力模型占比条形卡片 + 趋势图 */}
                <div className="grid grid-cols-1 xl:grid-cols-12 gap-4">
                    {/* 主力模型横向胶囊条形清单 (NotchAgentShareCard 同款) */}
                    <div className="xl:col-span-5">
                        <TopModelsShareCard
                            models={modelShareItems}
                            formatNumber={formatNumber}
                        />
                    </div>

                    {/* 优化后的分模型/分账号趋势图（支持圆角堆叠柱状图与面积图） */}
                    <div className="xl:col-span-7 bg-[#121316] rounded-2xl p-4 sm:p-5 border border-white/[0.08] shadow-sm flex flex-col justify-between">
                        <div className="flex flex-wrap items-center justify-between gap-2 mb-3">
                            <div className="flex items-center gap-2">
                                {chartType === 'bar' ? (
                                    <BarChart3 className="w-4 h-4 text-blue-400 opacity-90" />
                                ) : (
                                    <TrendingUp className="w-4 h-4 text-blue-400 opacity-90" />
                                )}
                                <span className="text-[13px] font-semibold text-white/90 tracking-wide">
                                    {viewMode === 'model'
                                        ? t('token_stats.model_trend', '使用趋势 (按模型)')
                                        : t('token_stats.account_trend', '使用趋势 (按账号)')}
                                </span>
                            </div>

                            <div className="flex items-center gap-2">
                                {/* 图表形态切换器：柱状图 / 面积图 */}
                                <div className="flex bg-[#1a1d24] border border-white/[0.08] rounded-xl p-0.5">
                                    <button
                                        onClick={() => setChartType('bar')}
                                        title="柱状图"
                                        className={`px-2 py-1 text-xs font-medium rounded-lg transition-all flex items-center gap-1 ${
                                            chartType === 'bar'
                                                ? 'bg-white/[0.12] text-white shadow-sm'
                                                : 'text-white/50 hover:text-white/80'
                                        }`}
                                    >
                                        <BarChart3 className="w-3.5 h-3.5" />
                                        <span>柱状</span>
                                    </button>
                                    <button
                                        onClick={() => setChartType('area')}
                                        title="面积图"
                                        className={`px-2 py-1 text-xs font-medium rounded-lg transition-all flex items-center gap-1 ${
                                            chartType === 'area'
                                                ? 'bg-white/[0.12] text-white shadow-sm'
                                                : 'text-white/50 hover:text-white/80'
                                        }`}
                                    >
                                        <TrendingUp className="w-3.5 h-3.5" />
                                        <span>面积</span>
                                    </button>
                                </div>

                                {/* 模型 / 账号 维度切换器 */}
                                <div className="flex bg-[#1a1d24] border border-white/[0.08] rounded-xl p-0.5">
                                    <button
                                        onClick={() => setViewMode('model')}
                                        className={`px-2.5 py-1 text-xs font-medium rounded-lg transition-all ${
                                            viewMode === 'model'
                                                ? 'bg-white/[0.12] text-white shadow-sm'
                                                : 'text-white/50 hover:text-white/80'
                                        }`}
                                    >
                                        {t('token_stats.by_model', '按模型')}
                                    </button>
                                    <button
                                        onClick={() => setViewMode('account')}
                                        className={`px-2.5 py-1 text-xs font-medium rounded-lg transition-all ${
                                            viewMode === 'account'
                                                ? 'bg-white/[0.12] text-white shadow-sm'
                                                : 'text-white/50 hover:text-white/80'
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
                                            data={viewMode === 'model' ? modelTrendData : accountTrendData}
                                            onMouseMove={handleTrendChartMouseMove}
                                            onMouseLeave={() => setTooltipPosition(undefined)}
                                        >
                                            <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="rgba(255,255,255,0.06)" />
                                            <XAxis
                                                dataKey="period"
                                                tick={{ fontSize: 10, fill: 'rgba(255,255,255,0.4)', fontFamily: 'monospace' }}
                                                tickFormatter={(val) => {
                                                    if (timeRange === 'hourly') return val.split(' ')[1] || val;
                                                    if (timeRange === 'daily') return val.split('-').slice(1).join('/');
                                                    return val;
                                                }}
                                                axisLine={false}
                                                tickLine={false}
                                                dy={5}
                                            />
                                            <YAxis
                                                tick={{ fontSize: 10, fill: 'rgba(255,255,255,0.4)', fontFamily: 'monospace' }}
                                                tickFormatter={(val) => formatNumber(val)}
                                                axisLine={false}
                                                tickLine={false}
                                            />
                                            <Tooltip
                                                content={<CustomTrendTooltip />}
                                                cursor={{ fill: 'rgba(255,255,255,0.05)' }}
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
                                            data={viewMode === 'model' ? modelTrendData : accountTrendData}
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
                                            <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="rgba(255,255,255,0.06)" />
                                            <XAxis
                                                dataKey="period"
                                                tick={{ fontSize: 10, fill: 'rgba(255,255,255,0.4)', fontFamily: 'monospace' }}
                                                tickFormatter={(val) => {
                                                    if (timeRange === 'hourly') return val.split(' ')[1] || val;
                                                    if (timeRange === 'daily') return val.split('-').slice(1).join('/');
                                                    return val;
                                                }}
                                                axisLine={false}
                                                tickLine={false}
                                                dy={5}
                                            />
                                            <YAxis
                                                tick={{ fontSize: 10, fill: 'rgba(255,255,255,0.4)', fontFamily: 'monospace' }}
                                                tickFormatter={(val) => formatNumber(val)}
                                                axisLine={false}
                                                tickLine={false}
                                            />
                                            <Tooltip
                                                content={<CustomTrendTooltip />}
                                                cursor={{ stroke: 'rgba(255,255,255,0.2)', strokeWidth: 1, strokeDasharray: '4 4' }}
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
                                <div className="h-full flex items-center justify-center text-white/30 text-xs">
                                    {loading ? t('common.loading', '加载中...') : t('token_stats.no_data', '暂无数据')}
                                </div>
                            )}
                        </div>

                        {/* 底部紧凑图例 */}
                        <div className="flex items-center flex-wrap gap-3 pt-3 border-t border-white/[0.06] text-[11px] font-mono">
                            {displayModelKeysOrAccounts(viewMode, allModels, allAccounts).map((item, index) => (
                                <div key={item} className="flex items-center gap-1.5 text-white/60">
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

                {/* 底部详细统计表格 */}
                {viewMode === 'model' && modelData.length > 0 && (
                    <div className="bg-[#121316] rounded-2xl p-4 sm:p-5 border border-white/[0.08] shadow-sm">
                        <div className="flex items-center justify-between mb-4">
                            <h2 className="text-[13px] font-semibold text-white/90 flex items-center gap-2">
                                <Cpu className="w-4 h-4 text-blue-400" />
                                {t('token_stats.model_details', '分模型详细统计')}
                            </h2>
                            <span className="text-xs text-white/40 font-mono">
                                按照单价快照核算
                            </span>
                        </div>
                        <div className="overflow-x-auto">
                            <table className="w-full text-xs">
                                <thead>
                                    <tr className="border-b border-white/[0.08] text-white/40 uppercase tracking-wider text-[11px]">
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
                                <tbody className="divide-y divide-white/[0.04]">
                                    {modelData.map((model, index) => {
                                        const percentage = summary ? ((model.total_tokens / summary.total_tokens) * 100).toFixed(1) : '0';
                                        const cost = calculateModelCost(model);
                                        return (
                                            <tr key={model.model} className="hover:bg-white/[0.03] transition-colors">
                                                <td className="py-2.5 px-3">
                                                    <div className="flex items-center gap-2 font-mono">
                                                        <div
                                                            className="w-2 h-2 rounded-full"
                                                            style={{ backgroundColor: TOP_COLORS[index % TOP_COLORS.length] }}
                                                        />
                                                        <span className="text-white/90 font-medium">
                                                            {model.model}
                                                        </span>
                                                    </div>
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono text-white/60">
                                                    {model.request_count.toLocaleString()}
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono text-blue-400">
                                                    {formatNumber(model.total_input_tokens)}
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono text-purple-400">
                                                    {formatNumber(model.total_output_tokens)}
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono text-sky-400">
                                                    {formatNumber(model.total_cached_tokens)}
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono font-semibold text-white">
                                                    {formatNumber(model.total_tokens)}
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono text-white/60">
                                                    {percentage}%
                                                </td>
                                                <td className="py-2.5 px-3 text-right font-mono font-medium text-emerald-400">
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
                    <div className="bg-[#121316] rounded-2xl p-4 sm:p-5 border border-white/[0.08] shadow-sm">
                        <div className="flex items-center justify-between mb-4">
                            <h2 className="text-[13px] font-semibold text-white/90 flex items-center gap-2">
                                <Users className="w-4 h-4 text-green-400" />
                                {t('token_stats.account_details', '账号详细统计')}
                            </h2>
                        </div>
                        <div className="overflow-x-auto">
                            <table className="w-full text-xs">
                                <thead>
                                    <tr className="border-b border-white/[0.08] text-white/40 uppercase tracking-wider text-[11px]">
                                        <th className="text-left py-2.5 px-3 font-medium">{t('token_stats.account', '账号')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.requests', '请求数')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.input', '输入')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.output', '输出')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.cached_token', '缓存命中')}</th>
                                        <th className="text-right py-2.5 px-3 font-medium">{t('token_stats.total', '合计')}</th>
                                    </tr>
                                </thead>
                                <tbody className="divide-y divide-white/[0.04]">
                                    {accountData.map((account) => (
                                        <tr key={account.account_email} className="hover:bg-white/[0.03] transition-colors">
                                            <td className="py-2.5 px-3 font-mono text-white/90">
                                                {account.account_email}
                                            </td>
                                            <td className="py-2.5 px-3 text-right font-mono text-white/60">
                                                {account.request_count.toLocaleString()}
                                            </td>
                                            <td className="py-2.5 px-3 text-right font-mono text-blue-400">
                                                {formatNumber(account.total_input_tokens)}
                                            </td>
                                            <td className="py-2.5 px-3 text-right font-mono text-purple-400">
                                                {formatNumber(account.total_output_tokens)}
                                            </td>
                                            <td className="py-2.5 px-3 text-right font-mono text-sky-400">
                                                {formatNumber(account.total_cached_tokens)}
                                            </td>
                                            <td className="py-2.5 px-3 text-right font-mono font-semibold text-white">
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
