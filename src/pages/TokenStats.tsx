import React, { useEffect, useState, useRef, useCallback, useMemo } from 'react';
import { request as invoke } from '../utils/request';
import { useTranslation } from 'react-i18next';
import { AreaChart, Area, BarChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, PieChart, Pie, Cell, Legend } from 'recharts';
import { Clock, Calendar, CalendarDays, History, Users, Zap, TrendingUp, RefreshCw, Cpu, DollarSign, Settings2, X, RotateCcw, Plus, Trash2 } from 'lucide-react';

interface TokenStatsAggregated {
    period: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_tokens: number;
    request_count: number;
    uncached_input_tokens?: number;
}

interface AccountTokenStats {
    account_email: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_tokens: number;
    request_count: number;
}

interface ModelTokenStats {
    model: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    total_tokens: number;
    request_count: number;
}

interface ModelTrendPoint {
    period: string;
    model_data: Record<string, number>;
}

interface AccountTrendPoint {
    period: string;
    account_data: Record<string, number>;
}

interface TokenStatsSummary {
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

const MODEL_COLORS = [
    '#3b82f6', '#8b5cf6', '#ec4899', '#f59e0b', '#10b981',
    '#06b6d4', '#6366f1', '#f43f5e', '#84cc16', '#a855f7',
    '#14b8a6', '#f97316', '#64748b', '#0ea5e9', '#d946ef'
];

const COLORS = ['#3b82f6', '#8b5cf6', '#ec4899', '#f59e0b', '#10b981', '#06b6d4', '#6366f1', '#f43f5e'];

const formatNumber = (num: number): string => {
    if (num >= 1000000) return `${(num / 1000000).toFixed(1)}M`;
    if (num >= 1000) return `${(num / 1000).toFixed(1)}K`;
    return num.toString();
};

const TokenStats: React.FC = () => {
    const { t } = useTranslation();
    const [timeRange, setTimeRange] = useState<TimeRange>('daily');
    const [viewMode, setViewMode] = useState<ViewMode>('model');
    const [chartData, setChartData] = useState<TokenStatsAggregated[]>([]);
    const [accountData, setAccountData] = useState<AccountTokenStats[]>([]);
    const [modelData, setModelData] = useState<ModelTokenStats[]>([]);
    const [modelTrendData, setModelTrendData] = useState<any[]>([]);
    const [accountTrendData, setAccountTrendData] = useState<any[]>([]);
    const [allModels, setAllModels] = useState<string[]>([]);
    const [allAccounts, setAllAccounts] = useState<string[]>([]);
    const [summary, setSummary] = useState<TokenStatsSummary | null>(null);
    const [loading, setLoading] = useState(true);

    // 价格体系与自定义价格状态
    const [pricing, setPricing] = useState<Record<string, ModelPricingRule>>(() => {
        try {
            const saved = localStorage.getItem(PRICING_STORAGE_KEY);
            if (saved) return JSON.parse(saved);
        } catch (e) {}
        return DEFAULT_PRICING;
    });
    const [showPricingModal, setShowPricingModal] = useState(false);
    const [tempPricing, setTempPricing] = useState<Record<string, ModelPricingRule>>(pricing);
    const [newModelName, setNewModelName] = useState('');
    const [newInputPrice, setNewInputPrice] = useState('1.00');
    const [newOutputPrice, setNewOutputPrice] = useState('5.00');
    const [newCachePrice, setNewCachePrice] = useState('0.10');

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

    const fetchData = async () => {
        setLoading(true);
        try {
            let hours = 24;
            let data: TokenStatsAggregated[] = [];
            let modelTrend: ModelTrendPoint[] = [];
            let accountTrend: AccountTrendPoint[] = [];

            switch (timeRange) {
                case 'hourly':
                    hours = 24;
                    data = await invoke<TokenStatsAggregated[]>('get_token_stats_hourly', { hours: 24 });
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_hourly', { hours: 24 });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_hourly', { hours: 24 });
                    break;
                case 'daily':
                    hours = 168;
                    data = await invoke<TokenStatsAggregated[]>('get_token_stats_daily', { days: 7 });
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_daily', { days: 7 });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_daily', { days: 7 });
                    break;
                case 'weekly':
                    hours = 720;
                    data = await invoke<TokenStatsAggregated[]>('get_token_stats_weekly', { weeks: 4 });
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_daily', { days: 30 });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_daily', { days: 30 });
                    break;
                case 'all':
                    hours = 0;
                    data = await invoke<TokenStatsAggregated[]>('get_token_stats_daily', { days: 0 });
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_daily', { days: 0 });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_daily', { days: 0 });
                    break;
            }

            setChartData(data.map(point => ({
                ...point,
                total_cached_tokens: point.total_cached_tokens || 0,
                uncached_input_tokens: Math.max((point.total_input_tokens || 0) - (point.total_cached_tokens || 0), 0)
            })));

            const models = new Set<string>();
            modelTrend.forEach(point => {
                Object.keys(point.model_data).forEach(m => models.add(m));
            });
            const modelList = Array.from(models);
            setAllModels(modelList);

            const transformedTrend = modelTrend.map(point => {
                const row: Record<string, any> = { period: point.period };
                modelList.forEach(model => {
                    row[model] = point.model_data[model] || 0;
                });
                return row;
            });
            setModelTrendData(transformedTrend);

            // Process Account Trend Data
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

            const [accounts, models_stats, summaryData] = await Promise.all([
                invoke<AccountTokenStats[]>('get_token_stats_by_account', { hours }),
                invoke<ModelTokenStats[]>('get_token_stats_by_model', { hours }),
                invoke<TokenStatsSummary>('get_token_stats_summary', { hours })
            ]);

            setAccountData(accounts);
            setModelData(models_stats);
            setSummary(summaryData);
        } catch (error) {
            console.error('Failed to fetch token stats:', error);
        } finally {
            setLoading(false);
        }
    };

    useEffect(() => {
        fetchData();
    }, [timeRange]);

    const pieData = accountData.slice(0, 8).map((account, index) => ({
        name: account.account_email.split('@')[0] + '...',
        value: account.total_tokens,
        fullEmail: account.account_email,
        color: COLORS[index % COLORS.length]
    }));

    const trendChartContainerRef = useRef<HTMLDivElement>(null);
    const [tooltipPosition, setTooltipPosition] = useState<{ x: number; y: number } | undefined>(undefined);

    // Ref and state for pie chart tooltip position
    const pieChartContainerRef = useRef<HTMLDivElement>(null);
    const [pieTooltipPosition, setPieTooltipPosition] = useState<{ x: number; y: number } | undefined>(undefined);

    // Handle mouse move to calculate tooltip position
    const handleTrendChartMouseMove = useCallback((e: any) => {
        if (!trendChartContainerRef.current || !e?.activeCoordinate) return;

        const containerRect = trendChartContainerRef.current.getBoundingClientRect();
        const tooltipWidth = 200; // Approximate tooltip width
        const rightEdgeThreshold = containerRect.width - tooltipWidth - 20; // 20px buffer

        const mouseXInContainer = e.activeCoordinate.x;

        if (mouseXInContainer > rightEdgeThreshold) {
            setTooltipPosition({
                x: e.activeCoordinate.x - tooltipWidth - 15,
                y: e.activeCoordinate.y
            });
        } else {
            setTooltipPosition(undefined); // Use default positioning
        }
    }, []);

    // Handle mouse move for pie chart to calculate tooltip position
    const handlePieChartMouseMove = useCallback((e: any) => {
        if (!pieChartContainerRef.current) return;

        const containerRect = pieChartContainerRef.current.getBoundingClientRect();
        const tooltipWidth = 180; // Approximate tooltip width for pie chart

        // Get mouse position relative to container
        if (e?.activeCoordinate) {
            const mouseXInContainer = e.activeCoordinate.x;
            const rightEdgeThreshold = containerRect.width - tooltipWidth - 20;

            if (mouseXInContainer > rightEdgeThreshold) {
                setPieTooltipPosition({
                    x: e.activeCoordinate.x - tooltipWidth - 15,
                    y: e.activeCoordinate.y
                });
            } else {
                setPieTooltipPosition(undefined);
            }
        }
    }, []);

    // Custom Tooltip for Trend Chart
    const CustomTrendTooltip = ({ active, payload, label }: any) => {
        if (!active || !payload || !payload.length) return null;

        // Sort payload by value descending
        const sortedPayload = [...payload].sort((a: any, b: any) => b.value - a.value);

        return (
            <div className="bg-white/95 dark:bg-gray-800/95 backdrop-blur-sm p-2.5 rounded-xl shadow-xl border border-gray-100 dark:border-gray-700 text-xs z-[100] min-w-[180px] pointer-events-none">
                <p className="font-semibold text-gray-700 dark:text-gray-200 mb-1.5 border-b border-gray-100 dark:border-gray-700 pb-1.5">
                    {label}
                </p>
                <div className="max-h-[180px] overflow-y-auto space-y-1 pr-1.5 scrollbar-thin scrollbar-thumb-gray-200 dark:scrollbar-thumb-gray-700">
                    {sortedPayload.map((entry: any, index: number) => {
                        const name = entry.name;
                        const displayName = viewMode === 'model' ? name : name.split('@')[0];
                        return (
                            <div key={index} className="flex items-center justify-between gap-4">
                                <div className="flex items-center gap-2 overflow-hidden">
                                    <div className="w-2 h-2 rounded-full flex-shrink-0" style={{ backgroundColor: entry.color }} />
                                    <span className="text-gray-500 dark:text-gray-400 truncate max-w-[140px]" title={name}>
                                        {displayName}
                                    </span>
                                </div>
                                <span className="font-mono font-medium text-gray-700 dark:text-gray-200">
                                    {formatNumber(entry.value)}
                                </span>
                            </div>
                        );
                    })}
                </div>
            </div>
        );
    };

    const UsageTrendTooltip = ({ active, payload, label }: any) => {
        if (!active || !payload || !payload.length) return null;
        const row = payload[0]?.payload || {};
        const items = [
            { label: t('token_stats.total', '合计'), value: row.total_tokens || 0, color: '#111827' },
            { label: t('token_stats.input', '输入'), value: row.total_input_tokens || 0, color: '#3b82f6' },
            { label: t('token_stats.cached_token', '缓存命中'), value: row.total_cached_tokens || 0, color: '#93c5fd' },
            { label: t('token_stats.output', '输出'), value: row.total_output_tokens || 0, color: '#8b5cf6' },
        ];
        return (
            <div className="bg-white/95 dark:bg-gray-800/95 backdrop-blur-sm p-2.5 rounded-xl shadow-xl border border-gray-100 dark:border-gray-700 text-xs z-[100] pointer-events-none min-w-[170px]">
                {label && <p className="font-semibold text-gray-700 dark:text-gray-200 mb-2">{label}</p>}
                <div className="space-y-1">
                    {items.map((item) => (
                        <div key={item.label} className="flex items-center justify-between gap-4">
                            <div className="flex items-center gap-2">
                                <div className="w-2 h-2 rounded-full" style={{ backgroundColor: item.color }} />
                                <span className="text-gray-500 dark:text-gray-400">
                                    {item.label}:
                                </span>
                            </div>
                            <span className="font-mono font-medium text-gray-700 dark:text-gray-200">
                                {formatNumber(item.value)}
                            </span>
                        </div>
                    ))}
                    <div className="flex items-center justify-between gap-4 pt-1 border-t border-gray-100 dark:border-gray-700">
                        <span className="text-gray-500 dark:text-gray-400">
                            {t('token_stats.requests', '请求数')}:
                        </span>
                        <span className="font-mono font-medium text-gray-700 dark:text-gray-200">
                            {(row.request_count || 0).toLocaleString()}
                        </span>
                    </div>
                </div>
            </div>
        );
    };

    // Custom Tooltip for Pie Chart
    const CustomPieTooltip = ({ active, payload }: any) => {
        if (!active || !payload || !payload.length) return null;
        const entry = payload[0];
        return (
            <div className="bg-white/95 dark:bg-gray-800/95 backdrop-blur-sm p-2.5 rounded-xl shadow-xl border border-gray-100 dark:border-gray-700 text-xs z-[100] pointer-events-none">
                <div className="flex items-center gap-2">
                    <div className="w-2 h-2 rounded-full" style={{ backgroundColor: entry.payload.color || entry.color }} />
                    <span className="text-gray-500 dark:text-gray-400">
                        {entry.payload.fullEmail || entry.name}:
                    </span>
                    <span className="font-mono font-medium text-gray-700 dark:text-gray-200">
                        {formatNumber(entry.value)}
                    </span>
                </div>
            </div>
        );
    };

    return (
        <div className="h-full w-full overflow-y-auto">
            <div className="p-5 space-y-4 max-w-7xl mx-auto">
                <div className="flex items-center justify-between">
                    <h1 className="text-2xl font-bold text-gray-800 dark:text-white flex items-center gap-2">
                        <Zap className="w-6 h-6 text-blue-500" />
                        {t('token_stats.title', 'Token 消费统计')}
                    </h1>
                    <div className="flex items-center gap-2">
                        <div className="flex bg-gray-100 dark:bg-gray-800 rounded-lg p-1">
                            <button
                                onClick={() => setTimeRange('hourly')}
                                className={`px-3 py-1.5 rounded-md text-sm font-medium transition-colors flex items-center gap-1.5 ${timeRange === 'hourly'
                                    ? 'bg-white dark:bg-gray-700 text-blue-600 shadow-sm'
                                    : 'text-gray-600 dark:text-gray-400 hover:text-gray-800'
                                    }`}
                            >
                                <Clock className="w-4 h-4" />
                                {t('token_stats.hourly', '小时')}
                            </button>
                            <button
                                onClick={() => setTimeRange('daily')}
                                className={`px-3 py-1.5 rounded-md text-sm font-medium transition-colors flex items-center gap-1.5 ${timeRange === 'daily'
                                    ? 'bg-white dark:bg-gray-700 text-blue-600 shadow-sm'
                                    : 'text-gray-600 dark:text-gray-400 hover:text-gray-800'
                                    }`}
                            >
                                <Calendar className="w-4 h-4" />
                                {t('token_stats.daily', '日')}
                            </button>
                            <button
                                onClick={() => setTimeRange('weekly')}
                                className={`px-3 py-1.5 rounded-md text-sm font-medium transition-colors flex items-center gap-1.5 ${timeRange === 'weekly'
                                    ? 'bg-white dark:bg-gray-700 text-blue-600 shadow-sm'
                                    : 'text-gray-600 dark:text-gray-400 hover:text-gray-800'
                                    }`}
                            >
                                <CalendarDays className="w-4 h-4" />
                                {t('token_stats.weekly', '周')}
                            </button>
                            <button
                                onClick={() => setTimeRange('all')}
                                className={`px-3 py-1.5 rounded-md text-sm font-medium transition-colors flex items-center gap-1.5 ${timeRange === 'all'
                                    ? 'bg-white dark:bg-gray-700 text-blue-600 shadow-sm'
                                    : 'text-gray-600 dark:text-gray-400 hover:text-gray-800'
                                    }`}
                            >
                                <History className="w-4 h-4" />
                                {t('token_stats.all_time', '全部')}
                            </button>
                        </div>
                        <button
                            onClick={fetchData}
                            disabled={loading}
                            className="p-2 rounded-lg bg-blue-500 text-white hover:bg-blue-600 transition-colors disabled:opacity-50"
                        >
                            <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
                        </button>
                    </div>
                </div>

                {summary && (
                    <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-7 gap-3.5">
                        <div className="bg-gradient-to-br from-white to-gray-50 dark:from-gray-800 dark:to-gray-800/50 rounded-xl p-4 shadow-sm border border-gray-200 dark:border-gray-700 hover:shadow-md transition-shadow">
                            <div className="flex items-center gap-2 text-gray-500 dark:text-gray-400 text-sm mb-2">
                                <div className="p-1.5 rounded-lg bg-gray-100 dark:bg-gray-700">
                                    <Zap className="w-4 h-4 text-gray-600 dark:text-gray-300" />
                                </div>
                                {t('token_stats.total_tokens', '总 Token')}
                            </div>
                            <div className="text-2xl font-bold text-gray-800 dark:text-white">
                                {formatNumber(summary.total_tokens)}
                            </div>
                        </div>
                        <div className="bg-gradient-to-br from-blue-50/50 to-white dark:from-blue-900/10 dark:to-gray-800 rounded-xl p-4 shadow-sm border border-blue-100 dark:border-blue-900/30 hover:shadow-md transition-shadow">
                            <div className="flex items-center gap-2 text-blue-600/80 dark:text-blue-400/80 text-sm mb-2">
                                <div className="p-1.5 rounded-lg bg-blue-100/50 dark:bg-blue-900/30">
                                    <TrendingUp className="w-4 h-4 text-blue-600 dark:text-blue-400" />
                                </div>
                                {t('token_stats.input_tokens', '输入 Token')}
                            </div>
                            <div className="text-2xl font-bold text-blue-600 dark:text-blue-400">
                                {formatNumber(summary.total_input_tokens)}
                            </div>
                        </div>
                        <div className="bg-gradient-to-br from-purple-50/50 to-white dark:from-purple-900/10 dark:to-gray-800 rounded-xl p-4 shadow-sm border border-purple-100 dark:border-purple-900/30 hover:shadow-md transition-shadow">
                            <div className="flex items-center gap-2 text-purple-600/80 dark:text-purple-400/80 text-sm mb-2">
                                <div className="p-1.5 rounded-lg bg-purple-100/50 dark:bg-purple-900/30">
                                    <TrendingUp className="w-4 h-4 rotate-180 text-purple-600 dark:text-purple-400" />
                                </div>
                                {t('token_stats.output_tokens', '输出 Token')}
                            </div>
                            <div className="text-2xl font-bold text-purple-600 dark:text-purple-400">
                                {formatNumber(summary.total_output_tokens)}
                            </div>
                        </div>
                        <div className="bg-gradient-to-br from-sky-50/50 to-white dark:from-sky-900/10 dark:to-gray-800 rounded-xl p-4 shadow-sm border border-sky-100 dark:border-sky-900/30 hover:shadow-md transition-shadow">
                            <div className="flex items-center gap-2 text-sky-600/80 dark:text-sky-400/80 text-sm mb-2">
                                <div className="p-1.5 rounded-lg bg-sky-100/50 dark:bg-sky-900/30">
                                    <Zap className="w-4 h-4 text-sky-600 dark:text-sky-400" />
                                </div>
                                {t('token_stats.cached_token', '缓存命中')}
                            </div>
                            <div className="text-2xl font-bold text-sky-600 dark:text-sky-400">
                                {formatNumber(summary.total_cached_tokens)}
                            </div>
                        </div>

                        {/* 估算价值卡片 */}
                        <div className="bg-gradient-to-br from-emerald-50/60 to-white dark:from-emerald-950/20 dark:to-gray-800 rounded-xl p-4 shadow-sm border border-emerald-100 dark:border-emerald-900/40 hover:shadow-md transition-shadow">
                            <div className="flex items-center justify-between text-emerald-600/90 dark:text-emerald-400 text-sm mb-2">
                                <div className="flex items-center gap-1.5">
                                    <div className="p-1.5 rounded-lg bg-emerald-100/60 dark:bg-emerald-900/30">
                                        <DollarSign className="w-4 h-4 text-emerald-600 dark:text-emerald-400" />
                                    </div>
                                    <span className="font-medium">{t('token_stats.estimated_value', '估算价值')}</span>
                                </div>
                                <button
                                    onClick={() => {
                                        setTempPricing(pricing);
                                        setShowPricingModal(true);
                                    }}
                                    title={t('token_stats.custom_pricing', '自定义单价')}
                                    className="p-1 rounded-md text-emerald-600 dark:text-emerald-400 hover:bg-emerald-100/60 dark:hover:bg-emerald-900/40 transition-colors"
                                >
                                    <Settings2 className="w-3.5 h-3.5" />
                                </button>
                            </div>
                            <div className="text-2xl font-bold text-emerald-600 dark:text-emerald-400">
                                ${totalEstimatedCost.toFixed(2)}
                            </div>
                            <div className="text-[10px] text-gray-400 dark:text-gray-500 mt-1">
                                {t('token_stats.cost_desc', '按模型单价快照累计')}
                            </div>
                        </div>

                        <div className="bg-gradient-to-br from-green-50/50 to-white dark:from-green-900/10 dark:to-gray-800 rounded-xl p-4 shadow-sm border border-green-100 dark:border-green-900/30 hover:shadow-md transition-shadow">
                            <div className="flex items-center gap-2 text-green-600/80 dark:text-green-400/80 text-sm mb-2">
                                <div className="p-1.5 rounded-lg bg-green-100/50 dark:bg-green-900/30">
                                    <Users className="w-4 h-4 text-green-600 dark:text-green-400" />
                                </div>
                                {t('token_stats.accounts_used', '活跃账号')}
                            </div>
                            <div className="text-2xl font-bold text-green-600 dark:text-green-400">
                                {summary.unique_accounts}
                            </div>
                        </div>
                        <div className="bg-gradient-to-br from-orange-50/50 to-white dark:from-orange-900/10 dark:to-gray-800 rounded-xl p-4 shadow-sm border border-orange-100 dark:border-orange-900/30 hover:shadow-md transition-shadow">
                            <div className="flex items-center gap-2 text-orange-600/80 dark:text-orange-400/80 text-sm mb-2">
                                <div className="p-1.5 rounded-lg bg-orange-100/50 dark:bg-orange-900/30">
                                    <Cpu className="w-4 h-4 text-orange-600 dark:text-orange-400" />
                                </div>
                                {t('token_stats.models_used', '使用模型')}
                            </div>
                            <div className="text-2xl font-bold text-orange-600 dark:text-orange-400">
                                {modelData.length}
                            </div>
                        </div>
                    </div>
                )}

                <div className="bg-white dark:bg-gray-800 rounded-xl p-6 shadow-sm border border-gray-200 dark:border-gray-700">
                    <div className="flex items-center justify-between mb-4">
                        <h2 className="text-lg font-semibold text-gray-800 dark:text-white flex items-center gap-2">
                            {viewMode === 'model' ? (
                                <Cpu className="w-5 h-5 text-purple-500" />
                            ) : (
                                <Users className="w-5 h-5 text-green-500" />
                            )}
                            {viewMode === 'model'
                                ? t('token_stats.model_trend', '分模型使用趋势')
                                : t('token_stats.account_trend', '分账号使用趋势')
                            }
                        </h2>
                        <div className="flex bg-gray-100/80 dark:bg-gray-700/50 rounded-lg p-1">
                            <button
                                onClick={() => setViewMode('model')}
                                className={`px-3 py-1 text-xs font-medium rounded-md transition-all ${viewMode === 'model'
                                    ? 'bg-white dark:bg-gray-600 text-blue-600 dark:text-blue-400 shadow-sm'
                                    : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200'
                                    }`}
                            >
                                {t('token_stats.by_model', '按模型')}
                            </button>
                            <button
                                onClick={() => setViewMode('account')}
                                className={`px-3 py-1 text-xs font-medium rounded-md transition-all ${viewMode === 'account'
                                    ? 'bg-white dark:bg-gray-600 text-blue-600 dark:text-blue-400 shadow-sm'
                                    : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200'
                                    }`}
                            >
                                {t('token_stats.by_account_view', '按账号')}
                            </button>
                        </div>
                    </div>
                    <div className="h-72" ref={trendChartContainerRef}>
                        {modelTrendData.length > 0 && allModels.length > 0 ? (
                            <ResponsiveContainer width="100%" height="100%">
                                <AreaChart
                                    data={viewMode === 'model' ? modelTrendData : accountTrendData}
                                    onMouseMove={handleTrendChartMouseMove}
                                    onMouseLeave={() => setTooltipPosition(undefined)}
                                >
                                    <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="#374151" strokeOpacity={0.15} />
                                    <XAxis
                                        dataKey="period"
                                        tick={{ fontSize: 11, fill: '#6b7280' }}
                                        tickFormatter={(val) => {
                                            if (timeRange === 'hourly') return val.split(' ')[1] || val;
                                            if (timeRange === 'daily') return val.split('-').slice(1).join('/');
                                            return val;
                                        }}
                                        axisLine={false}
                                        tickLine={false}
                                        dy={10}
                                    />
                                    <YAxis
                                        tick={{ fontSize: 11, fill: '#6b7280' }}
                                        tickFormatter={(val) => formatNumber(val)}
                                        axisLine={false}
                                        tickLine={false}
                                    />
                                    <Tooltip
                                        content={<CustomTrendTooltip />}
                                        cursor={{ stroke: '#6b7280', strokeWidth: 1, strokeDasharray: '4 4', fill: 'transparent' }}
                                        allowEscapeViewBox={{ x: true, y: true }}
                                        position={tooltipPosition}
                                        wrapperStyle={{ zIndex: 100 }}
                                    />
                                    <Legend
                                        formatter={(value) => viewMode === 'model' ? value : value.split('@')[0]}
                                        wrapperStyle={{
                                            fontSize: '11px',
                                            paddingTop: '10px',
                                            maxHeight: '60px',
                                            overflowY: 'auto',
                                            zIndex: 0
                                        }}
                                    />
                                    {(viewMode === 'model' ? allModels : allAccounts).map((item, index) => (
                                        <Area
                                            key={item}
                                            type="monotone"
                                            dataKey={item}
                                            stackId="1"
                                            stroke={viewMode === 'model' ? MODEL_COLORS[index % MODEL_COLORS.length] : COLORS[index % COLORS.length]}
                                            fill={viewMode === 'model' ? MODEL_COLORS[index % MODEL_COLORS.length] : COLORS[index % COLORS.length]}
                                            fillOpacity={0.6}
                                        />
                                    ))}
                                </AreaChart>
                            </ResponsiveContainer>
                        ) : (
                            <div className="h-full flex items-center justify-center text-gray-400">
                                {loading ? t('common.loading', '加载中...') : t('token_stats.no_data', '暂无数据')}
                            </div>
                        )}
                    </div>
                </div>

                <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
                    <div className="lg:col-span-2 bg-white dark:bg-gray-800 rounded-xl p-6 shadow-sm border border-gray-200 dark:border-gray-700 flex flex-col">
                        <h2 className="text-lg font-semibold text-gray-800 dark:text-white mb-4">
                            {t('token_stats.usage_trend', 'Token 使用趋势')}
                        </h2>
                        <div className="flex-1 min-h-[16rem]">
                            {chartData.length > 0 ? (
                                <ResponsiveContainer width="100%" height="100%">
                                    <BarChart data={chartData}>
                                        <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="#374151" strokeOpacity={0.15} />
                                        <XAxis
                                            dataKey="period"
                                            tick={{ fontSize: 11, fill: '#6b7280' }}
                                            tickFormatter={(val) => {
                                                if (timeRange === 'hourly') return val.split(' ')[1] || val;
                                                if (timeRange === 'daily') return val.split('-').slice(1).join('/');
                                                return val;
                                            }}
                                            axisLine={false}
                                            tickLine={false}
                                            dy={10}
                                        />
                                        <YAxis
                                            tick={{ fontSize: 11, fill: '#6b7280' }}
                                            tickFormatter={(val) => formatNumber(val)}
                                            axisLine={false}
                                            tickLine={false}
                                        />
                                        <Tooltip
                                            content={<UsageTrendTooltip />}
                                            cursor={{ fill: 'transparent' }}
                                            allowEscapeViewBox={{ x: true, y: true }}
                                            wrapperStyle={{ zIndex: 100 }}
                                        />
                                        <Bar dataKey="total_cached_tokens" name={t('token_stats.cached_token', '缓存命中')} stackId="input" fill="#93c5fd" radius={[0, 0, 4, 4]} maxBarSize={50} />
                                        <Bar dataKey="uncached_input_tokens" name={t('token_stats.input', '输入')} stackId="input" fill="#3b82f6" radius={[4, 4, 0, 0]} maxBarSize={50} />
                                        <Bar dataKey="total_output_tokens" name={t('token_stats.output', '输出')} fill="#8b5cf6" radius={[4, 4, 0, 0]} maxBarSize={50} />
                                    </BarChart>
                                </ResponsiveContainer>
                            ) : (
                                <div className="h-full flex items-center justify-center text-gray-400">
                                    {loading ? t('common.loading', '加载中...') : t('token_stats.no_data', '暂无数据')}
                                </div>
                            )}
                        </div>
                    </div>

                    <div className="bg-white dark:bg-gray-800 rounded-xl p-6 shadow-sm border border-gray-200 dark:border-gray-700">
                        <h2 className="text-lg font-semibold text-gray-800 dark:text-white mb-4">
                            {t('token_stats.by_account', '分账号统计')}
                        </h2>
                        <div className="h-48" ref={pieChartContainerRef}>
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
                                            innerRadius={40}
                                            outerRadius={70}
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
                                            wrapperStyle={{ zIndex: 100 }}
                                        />
                                    </PieChart>
                                </ResponsiveContainer>
                            ) : (
                                <div className="h-full flex items-center justify-center text-gray-400">
                                    {loading ? t('common.loading', '加载中...') : t('token_stats.no_data', '暂无数据')}
                                </div>
                            )}
                        </div>
                        <div className="mt-4 space-y-2 max-h-32 overflow-y-auto">
                            {accountData.slice(0, 5).map((account, index) => (
                                <div key={account.account_email} className="flex items-center justify-between text-sm">
                                    <div className="flex items-center gap-2">
                                        <div
                                            className="w-3 h-3 rounded-full"
                                            style={{ backgroundColor: COLORS[index % COLORS.length] }}
                                        />
                                        <span className="text-gray-600 dark:text-gray-300 truncate max-w-[120px]">
                                            {account.account_email.split('@')[0]}
                                        </span>
                                    </div>
                                    <span className="font-medium text-gray-800 dark:text-white">
                                        {formatNumber(account.total_tokens)}
                                    </span>
                                </div>
                            ))}
                        </div>
                    </div>
                </div>


                {
                    modelData.length > 0 && viewMode === 'model' && (
                        <div className="bg-white dark:bg-gray-800 rounded-xl p-6 shadow-sm border border-gray-200 dark:border-gray-700">
                            <h2 className="text-lg font-semibold text-gray-800 dark:text-white mb-4 flex items-center gap-2">
                                <Cpu className="w-5 h-5 text-blue-500" />
                                {t('token_stats.model_details', '分模型详细统计')}
                            </h2>
                            <div className="overflow-x-auto">
                                <table className="w-full text-sm">
                                    <thead>
                                        <tr className="border-b border-gray-200 dark:border-gray-700">
                                            <th className="text-left py-3 px-4 font-medium text-gray-500 dark:text-gray-400">
                                                {t('token_stats.model', '模型')}
                                            </th>
                                            <th className="text-right py-3 px-4 font-medium text-gray-500 dark:text-gray-400">
                                                {t('token_stats.requests', '请求数')}
                                            </th>
                                            <th className="text-right py-3 px-4 font-medium text-gray-500 dark:text-gray-400">
                                                {t('token_stats.input', '输入')}
                                            </th>
                                            <th className="text-right py-3 px-4 font-medium text-gray-500 dark:text-gray-400">
                                                {t('token_stats.output', '输出')}
                                            </th>
                                            <th className="text-right py-3 px-4 font-medium text-gray-500 dark:text-gray-400">
                                                {t('token_stats.cached_token', '缓存命中')}
                                            </th>
                                            <th className="text-right py-3 px-4 font-medium text-gray-500 dark:text-gray-400">
                                                {t('token_stats.total', '合计')}
                                            </th>
                                            <th className="text-right py-3 px-4 font-medium text-gray-500 dark:text-gray-400">
                                                {t('token_stats.percentage', '占比')}
                                            </th>
                                            <th className="text-right py-3 px-4 font-medium text-gray-500 dark:text-gray-400">
                                                {t('token_stats.model_cost', '估算费用')}
                                            </th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {modelData.map((model, index) => {
                                            const percentage = summary ? ((model.total_tokens / summary.total_tokens) * 100).toFixed(1) : '0';
                                            const cost = calculateModelCost(model);
                                            return (
                                                <tr
                                                    key={model.model}
                                                    className="border-b border-gray-100 dark:border-gray-700/50 hover:bg-gray-50 dark:hover:bg-gray-700/30"
                                                >
                                                    <td className="py-3 px-4">
                                                        <div className="flex items-center gap-2">
                                                            <div
                                                                className="w-3 h-3 rounded-full"
                                                                style={{ backgroundColor: MODEL_COLORS[index % MODEL_COLORS.length] }}
                                                            />
                                                            <span className="text-gray-800 dark:text-white font-medium">
                                                                {model.model}
                                                            </span>
                                                        </div>
                                                    </td>
                                                    <td className="py-3 px-4 text-right text-gray-600 dark:text-gray-300">
                                                        {model.request_count.toLocaleString()}
                                                    </td>
                                                    <td className="py-3 px-4 text-right text-blue-600">
                                                        {formatNumber(model.total_input_tokens)}
                                                    </td>
                                                    <td className="py-3 px-4 text-right text-purple-600">
                                                        {formatNumber(model.total_output_tokens)}
                                                    </td>
                                                    <td className="py-3 px-4 text-right text-sky-600">
                                                        {formatNumber(model.total_cached_tokens)}
                                                    </td>
                                                    <td className="py-3 px-4 text-right font-semibold text-gray-800 dark:text-white">
                                                        {formatNumber(model.total_tokens)}
                                                    </td>
                                                    <td className="py-3 px-4 text-right">
                                                        <div className="flex items-center justify-end gap-2">
                                                            <div className="w-16 bg-gray-200 dark:bg-gray-700 rounded-full h-2">
                                                                <div
                                                                    className="h-2 rounded-full"
                                                                    style={{
                                                                        width: `${percentage}%`,
                                                                        backgroundColor: MODEL_COLORS[index % MODEL_COLORS.length]
                                                                    }}
                                                                />
                                                            </div>
                                                            <span className="text-gray-600 dark:text-gray-300 w-12 text-right">
                                                                {percentage}%
                                                            </span>
                                                        </div>
                                                    </td>
                                                    <td className="py-3 px-4 text-right font-medium text-emerald-600 dark:text-emerald-400">
                                                        ${cost.toFixed(2)}
                                                    </td>
                                                </tr>
                                            );
                                        })}
                                    </tbody>
                                </table>
                            </div>
                        </div>
                    )
                }

                {
                    accountData.length > 0 && viewMode === 'account' && (
                        <div className="bg-white dark:bg-gray-800 rounded-xl p-6 shadow-sm border border-gray-200 dark:border-gray-700">
                            <h2 className="text-lg font-semibold text-gray-800 dark:text-white mb-4">
                                {t('token_stats.account_details', '账号详细统计')}
                            </h2>
                            <div className="overflow-x-auto">
                                <table className="w-full text-sm">
                                    <thead>
                                        <tr className="border-b border-gray-200 dark:border-gray-700">
                                            <th className="text-left py-3 px-4 font-medium text-gray-500 dark:text-gray-400">
                                                {t('token_stats.account', '账号')}
                                            </th>
                                            <th className="text-right py-3 px-4 font-medium text-gray-500 dark:text-gray-400">
                                                {t('token_stats.requests', '请求数')}
                                            </th>
                                            <th className="text-right py-3 px-4 font-medium text-gray-500 dark:text-gray-400">
                                                {t('token_stats.input', '输入')}
                                            </th>
                                            <th className="text-right py-3 px-4 font-medium text-gray-500 dark:text-gray-400">
                                                {t('token_stats.output', '输出')}
                                            </th>
                                            <th className="text-right py-3 px-4 font-medium text-gray-500 dark:text-gray-400">
                                                {t('token_stats.cached_token', '缓存命中')}
                                            </th>
                                            <th className="text-right py-3 px-4 font-medium text-gray-500 dark:text-gray-400">
                                                {t('token_stats.total', '合计')}
                                            </th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {accountData.map((account) => (
                                            <tr
                                                key={account.account_email}
                                                className="border-b border-gray-100 dark:border-gray-700/50 hover:bg-gray-50 dark:hover:bg-gray-700/30"
                                            >
                                                <td className="py-3 px-4 text-gray-800 dark:text-white">
                                                    {account.account_email}
                                                </td>
                                                <td className="py-3 px-4 text-right text-gray-600 dark:text-gray-300">
                                                    {account.request_count.toLocaleString()}
                                                </td>
                                                <td className="py-3 px-4 text-right text-blue-600">
                                                    {formatNumber(account.total_input_tokens)}
                                                </td>
                                                <td className="py-3 px-4 text-right text-purple-600">
                                                    {formatNumber(account.total_output_tokens)}
                                                </td>
                                                <td className="py-3 px-4 text-right text-sky-600">
                                                    {formatNumber(account.total_cached_tokens)}
                                                </td>
                                                <td className="py-3 px-4 text-right font-semibold text-gray-800 dark:text-white">
                                                    {formatNumber(account.total_tokens)}
                                                </td>
                                            </tr>
                                        ))}
                                    </tbody>
                                </table>
                            </div>
                        </div>
                    )
                }

                {/* 自定义模型价格模态框 */}
                {showPricingModal && (
                    <div className="fixed inset-0 bg-black/50 backdrop-blur-sm z-50 flex items-center justify-center p-4">
                        <div className="bg-white dark:bg-gray-800 rounded-2xl max-w-2xl w-full p-6 shadow-2xl border border-gray-100 dark:border-gray-700 flex flex-col max-h-[85vh]">
                            <div className="flex items-center justify-between pb-4 border-b border-gray-100 dark:border-gray-700">
                                <div className="flex items-center gap-2">
                                    <DollarSign className="w-5 h-5 text-emerald-500" />
                                    <h3 className="text-lg font-bold text-gray-900 dark:text-white">
                                        {t('token_stats.pricing_settings', '模型单价配置 ($/1M Tokens)')}
                                    </h3>
                                </div>
                                <button
                                    onClick={() => setShowPricingModal(false)}
                                    className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-200"
                                >
                                    <X className="w-5 h-5" />
                                </button>
                            </div>

                            <div className="overflow-y-auto flex-1 my-4 space-y-3 pr-2 scrollbar-thin scrollbar-thumb-gray-200 dark:scrollbar-thumb-gray-700">
                                <div className="grid grid-cols-12 gap-2 text-xs font-semibold text-gray-500 dark:text-gray-400 px-2">
                                    <div className="col-span-4">模型匹配标识 (模糊匹配)</div>
                                    <div className="col-span-2 text-right">输入 ($/1M)</div>
                                    <div className="col-span-2 text-right">输出 ($/1M)</div>
                                    <div className="col-span-2 text-right">缓存 ($/1M)</div>
                                    <div className="col-span-2 text-center">操作</div>
                                </div>

                                {Object.entries(tempPricing).map(([mName, rule]: any) => (
                                    <div key={mName} className="grid grid-cols-12 gap-2 items-center p-2 rounded-lg bg-gray-50 dark:bg-gray-700/40 text-sm">
                                        <div className="col-span-4 font-mono text-xs font-medium text-gray-800 dark:text-gray-200 truncate" title={mName}>
                                            {mName}
                                        </div>
                                        <div className="col-span-2">
                                            <input
                                                type="number"
                                                step="0.01"
                                                value={rule.input}
                                                onChange={(e) => {
                                                    const val = parseFloat(e.target.value) || 0;
                                                    setTempPricing((prev: any) => ({
                                                        ...prev,
                                                        [mName]: { ...prev[mName], input: val }
                                                    }));
                                                }}
                                                className="w-full text-right px-2 py-1 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-600 rounded text-xs text-gray-800 dark:text-white"
                                            />
                                        </div>
                                        <div className="col-span-2">
                                            <input
                                                type="number"
                                                step="0.01"
                                                value={rule.output}
                                                onChange={(e) => {
                                                    const val = parseFloat(e.target.value) || 0;
                                                    setTempPricing((prev: any) => ({
                                                        ...prev,
                                                        [mName]: { ...prev[mName], output: val }
                                                    }));
                                                }}
                                                className="w-full text-right px-2 py-1 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-600 rounded text-xs text-gray-800 dark:text-white"
                                            />
                                        </div>
                                        <div className="col-span-2">
                                            <input
                                                type="number"
                                                step="0.001"
                                                value={rule.cached}
                                                onChange={(e) => {
                                                    const val = parseFloat(e.target.value) || 0;
                                                    setTempPricing((prev: any) => ({
                                                        ...prev,
                                                        [mName]: { ...prev[mName], cached: val }
                                                    }));
                                                }}
                                                className="w-full text-right px-2 py-1 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-600 rounded text-xs text-gray-800 dark:text-white"
                                            />
                                        </div>
                                        <div className="col-span-2 flex justify-center">
                                            {mName !== 'default' && (
                                                <button
                                                    onClick={() => {
                                                        const next = { ...tempPricing };
                                                        delete next[mName];
                                                        setTempPricing(next);
                                                    }}
                                                    className="text-red-400 hover:text-red-600 p-1"
                                                    title="删除"
                                                >
                                                    <Trash2 className="w-4 h-4" />
                                                </button>
                                            )}
                                        </div>
                                    </div>
                                ))}

                                {/* 添加新模型规则 */}
                                <div className="p-2.5 rounded-lg border border-dashed border-gray-200 dark:border-gray-600 mt-3 space-y-2">
                                    <div className="text-xs font-semibold text-gray-500 dark:text-gray-400">添加自定义模型规则</div>
                                    <div className="grid grid-cols-12 gap-2 items-center">
                                        <input
                                            type="text"
                                            placeholder="模型标识 (如 claude-5)"
                                            value={newModelName}
                                            onChange={(e) => setNewModelName(e.target.value)}
                                            className="col-span-4 px-2 py-1 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-600 rounded text-xs text-gray-800 dark:text-white"
                                        />
                                        <input
                                            type="number"
                                            step="0.01"
                                            placeholder="输入"
                                            value={newInputPrice}
                                            onChange={(e) => setNewInputPrice(e.target.value)}
                                            className="col-span-2 text-right px-2 py-1 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-600 rounded text-xs text-gray-800 dark:text-white"
                                        />
                                        <input
                                            type="number"
                                            step="0.01"
                                            placeholder="输出"
                                            value={newOutputPrice}
                                            onChange={(e) => setNewOutputPrice(e.target.value)}
                                            className="col-span-2 text-right px-2 py-1 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-600 rounded text-xs text-gray-800 dark:text-white"
                                        />
                                        <input
                                            type="number"
                                            step="0.001"
                                            placeholder="缓存"
                                            value={newCachePrice}
                                            onChange={(e) => setNewCachePrice(e.target.value)}
                                            className="col-span-2 text-right px-2 py-1 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-600 rounded text-xs text-gray-800 dark:text-white"
                                        />
                                        <button
                                            onClick={() => {
                                                if (!newModelName.trim()) return;
                                                const k = newModelName.trim().toLowerCase();
                                                setTempPricing((prev: any) => ({
                                                    ...prev,
                                                    [k]: {
                                                        input: parseFloat(newInputPrice) || 0,
                                                        output: parseFloat(newOutputPrice) || 0,
                                                        cached: parseFloat(newCachePrice) || 0
                                                    }
                                                }));
                                                setNewModelName('');
                                            }}
                                            className="col-span-2 px-2 py-1 bg-blue-500 hover:bg-blue-600 text-white rounded text-xs flex items-center justify-center gap-1"
                                        >
                                            <Plus className="w-3.5 h-3.5" /> 添加
                                        </button>
                                    </div>
                                </div>
                            </div>

                            <div className="flex items-center justify-between pt-4 border-t border-gray-100 dark:border-gray-700">
                                <button
                                    onClick={() => {
                                        setTempPricing(DEFAULT_PRICING);
                                    }}
                                    className="px-3 py-1.5 text-xs text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white flex items-center gap-1.5 border border-gray-200 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors"
                                >
                                    <RotateCcw className="w-3.5 h-3.5" />
                                    {t('token_stats.reset_pricing', '恢复官方默认')}
                                </button>
                                <div className="flex gap-2">
                                    <button
                                        onClick={() => setShowPricingModal(false)}
                                        className="px-4 py-1.5 text-xs font-medium text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition-colors"
                                    >
                                        取消
                                    </button>
                                    <button
                                        onClick={() => {
                                            setPricing(tempPricing);
                                            try {
                                                localStorage.setItem(PRICING_STORAGE_KEY, JSON.stringify(tempPricing));
                                            } catch (e) {}
                                            setShowPricingModal(false);
                                        }}
                                        className="px-4 py-1.5 text-xs font-medium text-white bg-emerald-500 hover:bg-emerald-600 rounded-lg shadow-sm transition-colors"
                                    >
                                        {t('token_stats.save_pricing', '保存单价配置')}
                                    </button>
                                </div>
                            </div>
                        </div>
                    </div>
                )}
            </div>
        </div>
    );
};

export default TokenStats;
