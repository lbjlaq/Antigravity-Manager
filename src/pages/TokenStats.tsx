import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import { BarChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, PieChart, Pie, Cell } from 'recharts';
import { Clock, Calendar, CalendarDays, Users, Zap, TrendingUp, RefreshCw } from 'lucide-react';

interface TokenStatsAggregated {
    period: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_tokens: number;
    request_count: number;
}

interface AccountTokenStats {
    account_email: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_tokens: number;
    request_count: number;
}

interface TokenStatsSummary {
    total_input_tokens: number;
    total_output_tokens: number;
    total_tokens: number;
    total_requests: number;
    unique_accounts: number;
}

type TimeRange = 'hourly' | 'daily' | 'weekly';

const COLORS = ['#3b82f6', '#8b5cf6', '#ec4899', '#f59e0b', '#10b981', '#06b6d4', '#6366f1', '#f43f5e'];

const formatNumber = (num: number): string => {
    if (num >= 1000000) return `${(num / 1000000).toFixed(1)}M`;
    if (num >= 1000) return `${(num / 1000).toFixed(1)}K`;
    return num.toString();
};

const CustomTooltip = ({ active, payload, label }: any) => {
    if (active && payload && payload.length) {
        return (
            <div className="bg-white/95 dark:bg-gray-800/95 backdrop-blur-sm p-3.5 rounded-xl shadow-xl border border-gray-100 dark:border-gray-700 text-xs text-left z-50">
                <p className="font-bold text-gray-900 dark:text-white mb-2">{label}</p>
                {payload.map((entry: any, index: number) => (
                    <div key={index} className="flex items-center gap-2.5 mb-1 last:mb-0">
                        <div
                            className="w-2 h-2 rounded-full ring-2 ring-white dark:ring-gray-800"
                            style={{ backgroundColor: entry.color }}
                        />
                        <span className="text-gray-500 dark:text-gray-300 font-medium min-w-[3rem]">
                            {entry.name}:
                        </span>
                        <span className="font-bold text-gray-900 dark:text-white font-mono">
                            {formatNumber(entry.value)}
                        </span>
                    </div>
                ))}
            </div>
        );
    }
    return null;
};

const TokenStats: React.FC = () => {
    const { t } = useTranslation();
    const [timeRange, setTimeRange] = useState<TimeRange>('daily');
    const [chartData, setChartData] = useState<TokenStatsAggregated[]>([]);
    const [accountData, setAccountData] = useState<AccountTokenStats[]>([]);
    const [summary, setSummary] = useState<TokenStatsSummary | null>(null);
    const [loading, setLoading] = useState(true);

    const fetchData = async () => {
        setLoading(true);
        try {
            let hours = 24;
            let data: TokenStatsAggregated[] = [];

            switch (timeRange) {
                case 'hourly':
                    hours = 24;
                    data = await invoke<TokenStatsAggregated[]>('get_token_stats_hourly', { hours: 24 });
                    break;
                case 'daily':
                    hours = 168;
                    data = await invoke<TokenStatsAggregated[]>('get_token_stats_daily', { days: 7 });
                    break;
                case 'weekly':
                    hours = 720;
                    data = await invoke<TokenStatsAggregated[]>('get_token_stats_weekly', { weeks: 4 });
                    break;
            }

            setChartData(data);

            const [accounts, summaryData] = await Promise.all([
                invoke<AccountTokenStats[]>('get_token_stats_by_account', { hours }),
                invoke<TokenStatsSummary>('get_token_stats_summary', { hours })
            ]);

            setAccountData(accounts);
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

    return (
        <div className="h-full w-full overflow-y-auto">
            <div className="p-5 space-y-4 max-w-7xl mx-auto">
                <div className="flex items-center justify-between">
                    <h1 className="text-2xl font-bold text-gray-900 dark:text-base-content flex items-center gap-2">
                        {t('token_stats.title', 'Token 消费统计')}
                    </h1>
                    <div className="flex items-center gap-2">
                        <div className="flex bg-gray-100 dark:bg-base-200 rounded-lg p-1">
                            <button
                                onClick={() => setTimeRange('hourly')}
                                className={`px-3 py-1.5 rounded-md text-xs font-medium transition-colors flex items-center gap-1.5 ${timeRange === 'hourly'
                                    ? 'bg-white dark:bg-base-100 text-blue-600 shadow-sm'
                                    : 'text-gray-500 dark:text-gray-400 hover:text-gray-800'
                                    }`}
                            >
                                <Clock className="w-3.5 h-3.5" />
                                {t('token_stats.hourly', '小时')}
                            </button>
                            <button
                                onClick={() => setTimeRange('daily')}
                                className={`px-3 py-1.5 rounded-md text-xs font-medium transition-colors flex items-center gap-1.5 ${timeRange === 'daily'
                                    ? 'bg-white dark:bg-base-100 text-blue-600 shadow-sm'
                                    : 'text-gray-500 dark:text-gray-400 hover:text-gray-800'
                                    }`}
                            >
                                <Calendar className="w-3.5 h-3.5" />
                                {t('token_stats.daily', '日')}
                            </button>
                            <button
                                onClick={() => setTimeRange('weekly')}
                                className={`px-3 py-1.5 rounded-md text-xs font-medium transition-colors flex items-center gap-1.5 ${timeRange === 'weekly'
                                    ? 'bg-white dark:bg-base-100 text-blue-600 shadow-sm'
                                    : 'text-gray-500 dark:text-gray-400 hover:text-gray-800'
                                    }`}
                            >
                                <CalendarDays className="w-3.5 h-3.5" />
                                {t('token_stats.weekly', '周')}
                            </button>
                        </div>
                        <button
                            onClick={fetchData}
                            disabled={loading}
                            className={`p-2 rounded-lg bg-blue-500 text-white hover:bg-blue-600 transition-colors shadow-sm ${loading ? 'opacity-70 cursor-not-allowed' : ''}`}
                        >
                            <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
                        </button>
                    </div>
                </div>

                {summary && (
                    <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
                        <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
                            <div className="flex items-center justify-between mb-2">
                                <div className="p-1.5 bg-blue-50 dark:bg-blue-900/20 rounded-md">
                                    <Zap className="w-4 h-4 text-blue-500 dark:text-blue-400" />
                                </div>
                            </div>
                            <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">
                                {formatNumber(summary.total_tokens)}
                            </div>
                            <div className="text-xs text-gray-500 dark:text-gray-400">{t('token_stats.total_tokens', '总 Token')}</div>
                        </div>
                        <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
                            <div className="flex items-center justify-between mb-2">
                                <div className="p-1.5 bg-green-50 dark:bg-green-900/20 rounded-md">
                                    <TrendingUp className="w-4 h-4 text-green-500 dark:text-green-400" />
                                </div>
                            </div>
                            <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">
                                {formatNumber(summary.total_input_tokens)}
                            </div>
                            <div className="text-xs text-gray-500 dark:text-gray-400">{t('token_stats.input_tokens', '输入 Token')}</div>
                        </div>
                        <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
                            <div className="flex items-center justify-between mb-2">
                                <div className="p-1.5 bg-purple-50 dark:bg-purple-900/20 rounded-md">
                                    <TrendingUp className="w-4 h-4 rotate-180 text-purple-500 dark:text-purple-400" />
                                </div>
                            </div>
                            <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">
                                {formatNumber(summary.total_output_tokens)}
                            </div>
                            <div className="text-xs text-gray-500 dark:text-gray-400">{t('token_stats.output_tokens', '输出 Token')}</div>
                        </div>
                        <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
                            <div className="flex items-center justify-between mb-2">
                                <div className="p-1.5 bg-cyan-50 dark:bg-cyan-900/20 rounded-md">
                                    <Users className="w-4 h-4 text-cyan-500 dark:text-cyan-400" />
                                </div>
                            </div>
                            <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">
                                {summary.unique_accounts}
                            </div>
                            <div className="text-xs text-gray-500 dark:text-gray-400">{t('token_stats.accounts_used', '活跃账号')}</div>
                        </div>
                    </div>
                )}

                <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
                    <div className="lg:col-span-2 bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
                        <h2 className="text-base font-semibold text-gray-900 dark:text-base-content mb-4 flex items-center gap-2">
                            <TrendingUp className="w-4 h-4 text-blue-500" />
                            {t('token_stats.usage_trend', 'Token 使用趋势')}
                        </h2>
                        <div className="h-64">
                            {chartData.length > 0 ? (
                                <ResponsiveContainer width="100%" height="100%">
                                    <BarChart data={chartData}>
                                        <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="#e5e7eb" opacity={0.5} />
                                        <XAxis
                                            dataKey="period"
                                            tick={{ fontSize: 10, fill: '#9ca3af' }}
                                            axisLine={false}
                                            tickLine={false}
                                            tickFormatter={(val) => {
                                                if (timeRange === 'hourly') return val.split(' ')[1] || val;
                                                if (timeRange === 'daily') return val.split('-').slice(1).join('/');
                                                return val;
                                            }}
                                        />
                                        <YAxis
                                            tick={{ fontSize: 10, fill: '#9ca3af' }}
                                            axisLine={false}
                                            tickLine={false}
                                            tickFormatter={(val) => formatNumber(val)}
                                        />
                                        <Tooltip
                                            content={<CustomTooltip />}
                                            cursor={{ fill: 'transparent', opacity: 0 }}
                                        />
                                        <Bar dataKey="total_input_tokens" name={t('token_stats.input', 'Input')} fill="#3b82f6" radius={[3, 3, 0, 0]} barSize={20} />
                                        <Bar dataKey="total_output_tokens" name={t('token_stats.output', 'Output')} fill="#8b5cf6" radius={[3, 3, 0, 0]} barSize={20} />
                                    </BarChart>
                                </ResponsiveContainer>
                            ) : (
                                <div className="h-full flex items-center justify-center text-gray-400 text-sm">
                                    {loading ? t('common.loading', '加载中...') : t('token_stats.no_data', '暂无数据')}
                                </div>
                            )}
                        </div>
                    </div>

                    <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
                        <h2 className="text-base font-semibold text-gray-900 dark:text-base-content mb-4 flex items-center gap-2">
                            <Users className="w-4 h-4 text-purple-500" />
                            {t('token_stats.by_account', '分账号统计')}
                        </h2>
                        <div className="h-44">
                            {pieData.length > 0 ? (
                                <ResponsiveContainer width="100%" height="100%">
                                    <PieChart>
                                        <Pie
                                            data={pieData}
                                            cx="50%"
                                            cy="50%"
                                            innerRadius={45}
                                            outerRadius={65}
                                            paddingAngle={4}
                                            dataKey="value"
                                            stroke="none"
                                        >
                                            {pieData.map((entry, index) => (
                                                <Cell key={`cell-${index}`} fill={entry.color} />
                                            ))}
                                        </Pie>
                                        <Tooltip
                                            content={<CustomTooltip />}
                                        />
                                    </PieChart>
                                </ResponsiveContainer>
                            ) : (
                                <div className="h-full flex items-center justify-center text-gray-400 text-sm">
                                    {loading ? t('common.loading', '加载中...') : t('token_stats.no_data', '暂无数据')}
                                </div>
                            )}
                        </div>
                        <div className="mt-4 space-y-2.5 max-h-32 overflow-y-auto pr-1 custom-scrollbar">
                            {accountData.slice(0, 5).map((account, index) => (
                                <div key={account.account_email} className="flex items-center justify-between text-xs">
                                    <div className="flex items-center gap-2 min-w-0">
                                        <div
                                            className="w-2.5 h-2.5 rounded-full shrink-0"
                                            style={{ backgroundColor: COLORS[index % COLORS.length] }}
                                        />
                                        <span className="text-gray-600 dark:text-gray-400 truncate">
                                            {account.account_email.split('@')[0]}
                                        </span>
                                    </div>
                                    <span className="font-semibold text-gray-900 dark:text-base-content ml-2">
                                        {formatNumber(account.total_tokens)}
                                    </span>
                                </div>
                            ))}
                        </div>
                    </div>
                </div>

                {accountData.length > 0 && (
                    <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
                        <h2 className="text-base font-semibold text-gray-900 dark:text-base-content mb-4 flex items-center gap-2">
                            <Clock className="w-4 h-4 text-cyan-500" />
                            {t('token_stats.account_details', '账号详细统计')}
                        </h2>
                        <div className="overflow-x-auto">
                            <table className="w-full text-xs text-left">
                                <thead>
                                    <tr className="text-gray-400 dark:text-gray-500 font-medium border-b border-gray-100 dark:border-base-200">
                                        <th className="py-2.5 px-2">{t('token_stats.account', '账号')}</th>
                                        <th className="py-2.5 px-2 text-right">{t('token_stats.requests', '请求数')}</th>
                                        <th className="py-2.5 px-2 text-right">{t('token_stats.input', '输入')}</th>
                                        <th className="py-2.5 px-2 text-right">{t('token_stats.output', '输出')}</th>
                                        <th className="py-2.5 px-2 text-right">{t('token_stats.total', '合计')}</th>
                                    </tr>
                                </thead>
                                <tbody className="divide-y divide-gray-50 dark:divide-base-200/50 text-gray-600 dark:text-gray-300">
                                    {accountData.map((account) => (
                                        <tr
                                            key={account.account_email}
                                            className="hover:bg-gray-50/50 dark:hover:bg-base-200/30 transition-colors"
                                        >
                                            <td className="py-3 px-2 font-medium text-gray-700 dark:text-gray-300">
                                                {account.account_email}
                                            </td>
                                            <td className="py-3 px-2 text-right">
                                                {account.request_count.toLocaleString()}
                                            </td>
                                            <td className="py-3 px-2 text-right text-emerald-600 dark:text-emerald-400">
                                                {formatNumber(account.total_input_tokens)}
                                            </td>
                                            <td className="py-3 px-2 text-right text-indigo-600 dark:text-indigo-400">
                                                {formatNumber(account.total_output_tokens)}
                                            </td>
                                            <td className="py-3 px-2 text-right font-bold text-gray-900 dark:text-base-content">
                                                {formatNumber(account.total_tokens)}
                                            </td>
                                        </tr>
                                    ))}
                                </tbody>
                            </table>
                        </div>
                    </div>
                )}
            </div>
        </div>
    );
};

export default TokenStats;
