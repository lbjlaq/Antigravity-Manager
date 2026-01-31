// File: src/pages/token-stats/model/useTokenStats.ts
// Business logic hook for Token Stats page

import { useState, useEffect, useMemo, useCallback } from 'react';
import { invoke } from '@/shared/api';
import type { ProxyStats } from '@/components/stats/RequestHealthCards';
import type { PieDataPoint } from '@/components/stats/StatsCharts';
import type {
    TokenStatsAggregated,
    AccountTokenStats,
    ModelTokenStats,
    ModelTrendPoint,
    AccountTrendPoint,
    TokenStatsSummary,
    TimeRange,
    ViewMode,
} from '../lib/constants';
import { CHART_COLORS } from '../lib/constants';

export function useTokenStats() {
    const [timeRange, setTimeRange] = useState<TimeRange>('daily');
    const [viewMode, setViewMode] = useState<ViewMode>('model');

    // Data States
    const [chartData, setChartData] = useState<TokenStatsAggregated[]>([]);
    const [accountData, setAccountData] = useState<AccountTokenStats[]>([]);
    const [modelData, setModelData] = useState<ModelTokenStats[]>([]);
    const [modelTrendData, setModelTrendData] = useState<any[]>([]);
    const [accountTrendData, setAccountTrendData] = useState<any[]>([]);
    const [allModels, setAllModels] = useState<string[]>([]);
    const [allAccounts, setAllAccounts] = useState<string[]>([]);
    const [summary, setSummary] = useState<TokenStatsSummary | null>(null);
    const [requestStats, setRequestStats] = useState<ProxyStats | null>(null);
    const [loading, setLoading] = useState(true);

    const fetchData = useCallback(async () => {
        setLoading(true);
        try {
            let hours = 24;
            let data: TokenStatsAggregated[] = [];
            let modelTrend: ModelTrendPoint[] = [];
            let accountTrend: AccountTrendPoint[] = [];

            // Fetch generic request stats
            const reqStats = await invoke<ProxyStats>('get_proxy_stats');
            setRequestStats(reqStats);

            switch (timeRange) {
                case 'hourly':
                    hours = 24;
                    data = await invoke<TokenStatsAggregated[]>('get_token_stats_hourly', { hours: 24 });
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_hourly', { hours: 24 });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_hourly', { hours: 24 });
                    break;
                case 'daily':
                    hours = 168; // 7 days
                    data = await invoke<TokenStatsAggregated[]>('get_token_stats_daily', { days: 7 });
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_daily', { days: 7 });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_daily', { days: 7 });
                    break;
                case 'weekly':
                    hours = 720; // 30 days
                    data = await invoke<TokenStatsAggregated[]>('get_token_stats_weekly', { weeks: 4 });
                    modelTrend = await invoke<ModelTrendPoint[]>('get_token_stats_model_trend_daily', { days: 30 });
                    accountTrend = await invoke<AccountTrendPoint[]>('get_token_stats_account_trend_daily', { days: 30 });
                    break;
            }

            setChartData(data);

            // Process Model Trend
            const models = new Set<string>();
            modelTrend.forEach(point => Object.keys(point.model_data).forEach(m => models.add(m)));
            const modelList = Array.from(models);
            setAllModels(modelList);
            const transformedTrend = modelTrend.map(point => {
                const row: Record<string, any> = { period: point.period };
                modelList.forEach(model => row[model] = point.model_data[model] || 0);
                return row;
            });
            setModelTrendData(transformedTrend);

            // Process Account Trend
            const accountsSet = new Set<string>();
            accountTrend.forEach(point => Object.keys(point.account_data).forEach(acc => accountsSet.add(acc)));
            const accountList = Array.from(accountsSet);
            setAllAccounts(accountList);
            const transformedAccountTrend = accountTrend.map(point => {
                const row: Record<string, any> = { period: point.period };
                accountList.forEach(acc => row[acc] = point.account_data[acc] || 0);
                return row;
            });
            setAccountTrendData(transformedAccountTrend);

            // Summary Stats
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
    }, [timeRange]);

    useEffect(() => {
        fetchData();
    }, [fetchData]);

    // Format Data for Charts
    const pieData: PieDataPoint[] = useMemo(() => accountData.slice(0, 8).map((account, index) => ({
        name: account.account_email.split('@')[0] + '...',
        value: account.total_tokens,
        fullEmail: account.account_email,
        color: CHART_COLORS[index % CHART_COLORS.length]
    })), [accountData]);

    const activeTrendData = viewMode === 'model' ? modelTrendData : accountTrendData;
    const activeKeys = viewMode === 'model' ? allModels : allAccounts;

    return {
        // State
        timeRange,
        viewMode,
        chartData,
        accountData,
        modelData,
        summary,
        requestStats,
        loading,
        pieData,
        activeTrendData,
        activeKeys,

        // Setters
        setTimeRange,
        setViewMode,

        // Actions
        fetchData,
    };
}
