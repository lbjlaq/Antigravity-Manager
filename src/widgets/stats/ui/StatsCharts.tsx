// File: src/widgets/stats/ui/StatsCharts.tsx
// Stats charts component with area, bar, and pie charts

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { 
    AreaChart, Area, BarChart, Bar, PieChart, Pie, 
    XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, Cell, Legend 
} from 'recharts';
import { motion } from 'framer-motion';
import { Cpu, Users, TrendingUp } from 'lucide-react';
import { cn } from '@/shared/lib';

// --- Types ---

export interface ChartDataPoints {
    period: string;
    [key: string]: any;
}

export interface PieDataPoint {
    name: string;
    value: number;
    color: string;
    fullEmail?: string;
    [key: string]: any; 
}

interface StatsChartsProps {
    trendData: any[];
    usageData: any[];
    pieData: PieDataPoint[];
    allKeys: string[];
    viewMode: 'model' | 'account';
    onViewModeChange: (mode: 'model' | 'account') => void;
    timeRange: 'hourly' | 'daily' | 'weekly';
    isLoading: boolean;
}

// --- Utils ---

const MODEL_COLORS = [
    '#3b82f6', '#8b5cf6', '#ec4899', '#f59e0b', '#10b981',
    '#06b6d4', '#6366f1', '#f43f5e', '#84cc16', '#a855f7',
    '#14b8a6', '#f97316', '#64748b', '#0ea5e9', '#d946ef'
];

const ACCOUNT_COLORS = ['#3b82f6', '#8b5cf6', '#ec4899', '#f59e0b', '#10b981', '#06b6d4', '#6366f1', '#f43f5e'];

const formatNumber = (num: number): string => {
    if (num >= 1000000) return `${(num / 1000000).toFixed(1)}M`;
    if (num >= 1000) return `${(num / 1000).toFixed(1)}K`;
    return num.toString();
};

const shortenName = (name: string, mode: 'model' | 'account') => {
    if (mode === 'account') return name.split('@')[0];
    return name.replace('gemini-', 'g-').replace('claude-', 'c-').replace('-preview', '').replace('-latest', '');
};

// --- Sub-components ---

const CustomTooltip = ({ active, payload, label }: any) => {
    if (!active || !payload || !payload.length) return null;
    const sorted = [...payload].sort((a: any, b: any) => b.value - a.value);

    return (
        <div className="bg-white/95 dark:bg-zinc-900/95 backdrop-blur-md p-3 rounded-xl shadow-xl border border-zinc-200 dark:border-zinc-800 text-xs">
            <p className="font-semibold text-zinc-900 dark:text-zinc-100 mb-2 border-b border-zinc-100 dark:border-zinc-800 pb-1">
                {label}
            </p>
            <div className="max-h-[200px] overflow-y-auto space-y-1 pr-1 custom-scrollbar">
                {sorted.map((entry: any, i: number) => (
                    <div key={i} className="flex items-center justify-between gap-4">
                        <div className="flex items-center gap-2">
                            <div className="w-2 h-2 rounded-full" style={{ backgroundColor: entry.color || entry.fill }} />
                            <span className="text-zinc-500 dark:text-zinc-400 max-w-[150px] truncate">
                                {entry.name}
                            </span>
                        </div>
                        <span className="font-mono font-medium text-zinc-700 dark:text-zinc-200">
                            {formatNumber(entry.value)}
                        </span>
                    </div>
                ))}
            </div>
        </div>
    );
};

export const StatsCharts = memo(({ 
    trendData, 
    usageData, 
    pieData, 
    allKeys, 
    viewMode, 
    onViewModeChange, 
    timeRange,
    isLoading 
}: StatsChartsProps) => {
    const { t } = useTranslation();

    if (isLoading) {
        return <div className="h-96 rounded-2xl bg-zinc-100 dark:bg-zinc-800 animate-pulse mt-6" />;
    }

    return (
        <div className="space-y-6 mt-6">
            {/* Trend Chart (Full Width) */}
            <motion.div 
                initial={{ opacity: 0, scale: 0.98 }}
                animate={{ opacity: 1, scale: 1 }}
                className="bg-white dark:bg-zinc-900/50 rounded-2xl p-6 shadow-sm border border-zinc-200 dark:border-zinc-800"
            >
                <div className="flex items-center justify-between mb-6">
                    <h2 className="text-lg font-bold text-zinc-900 dark:text-white flex items-center gap-2">
                        {viewMode === 'model' ? <Cpu className="w-5 h-5 text-indigo-500" /> : <Users className="w-5 h-5 text-emerald-500" />}
                        {viewMode === 'model' ? t('token_stats.model_trend', 'Model Consumption Trend') : t('token_stats.account_trend', 'Account Consumption Trend')}
                    </h2>
                    
                    <div className="flex p-1 bg-zinc-100 dark:bg-zinc-800 rounded-lg">
                        <button
                            onClick={() => onViewModeChange('model')}
                            className={cn(
                                "px-3 py-1.5 text-xs font-medium rounded-md transition-all",
                                viewMode === 'model' ? "bg-white dark:bg-zinc-700 shadow text-indigo-600 dark:text-indigo-400" : "text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-300"
                            )}
                        >
                            {t('token_stats.by_model', 'By Model')}
                        </button>
                        <button
                            onClick={() => onViewModeChange('account')}
                            className={cn(
                                "px-3 py-1.5 text-xs font-medium rounded-md transition-all",
                                viewMode === 'account' ? "bg-white dark:bg-zinc-700 shadow text-emerald-600 dark:text-emerald-400" : "text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-300"
                            )}
                        >
                            {t('token_stats.by_account_view', 'By Account')}
                        </button>
                    </div>
                </div>

                <div className="h-[350px] w-full">
                    {trendData.length > 0 ? (
                        <ResponsiveContainer width="100%" height="100%">
                            <AreaChart data={trendData}>
                                <defs>
                                    {allKeys.map((key, i) => (
                                        <linearGradient key={`grad-${key}`} id={`grad-${key}`} x1="0" y1="0" x2="0" y2="1">
                                            <stop offset="5%" stopColor={viewMode === 'model' ? MODEL_COLORS[i % MODEL_COLORS.length] : ACCOUNT_COLORS[i % ACCOUNT_COLORS.length]} stopOpacity={0.3}/>
                                            <stop offset="95%" stopColor={viewMode === 'model' ? MODEL_COLORS[i % MODEL_COLORS.length] : ACCOUNT_COLORS[i % ACCOUNT_COLORS.length]} stopOpacity={0}/>
                                        </linearGradient>
                                    ))}
                                </defs>
                                <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="#3f3f46" strokeOpacity={0.1} />
                                <XAxis 
                                    dataKey="period" 
                                    tick={{ fontSize: 10, fill: '#71717a' }}
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
                                    tick={{ fontSize: 10, fill: '#71717a' }}
                                    tickFormatter={formatNumber}
                                    axisLine={false}
                                    tickLine={false}
                                />
                                <Tooltip content={<CustomTooltip />} cursor={{ fill: 'transparent' }} />
                                <Legend 
                                    iconType="circle" 
                                    formatter={(val) => <span className="text-xs text-zinc-500 dark:text-zinc-400 ml-1">{shortenName(val, viewMode)}</span>}
                                />
                                {allKeys.map((key, i) => (
                                    <Area
                                        key={key}
                                        type="monotone"
                                        dataKey={key}
                                        stackId="1"
                                        stroke={viewMode === 'model' ? MODEL_COLORS[i % MODEL_COLORS.length] : ACCOUNT_COLORS[i % ACCOUNT_COLORS.length]}
                                        fill={`url(#grad-${key})`}
                                        strokeWidth={2}
                                    />
                                ))}
                            </AreaChart>
                        </ResponsiveContainer>
                    ) : (
                        <div className="h-full flex flex-col items-center justify-center text-zinc-400 dark:text-zinc-600">
                             <TrendingUp className="w-12 h-12 mb-2 opacity-20" />
                             <span>{t('token_stats.no_data', 'No Data Available')}</span>
                        </div>
                    )}
                </div>
            </motion.div>

            <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
                {/* Usage Bar Chart */}
                <motion.div 
                    initial={{ opacity: 0, y: 20 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ delay: 0.1 }}
                    className="lg:col-span-2 bg-white dark:bg-zinc-900/50 rounded-2xl p-6 shadow-sm border border-zinc-200 dark:border-zinc-800"
                >
                    <h2 className="text-lg font-bold text-zinc-900 dark:text-white mb-6">
                        {t('token_stats.usage_trend', 'Token Usage (In/Out)')}
                    </h2>
                    <div className="h-[250px] w-full">
                         <ResponsiveContainer width="100%" height="100%">
                            <BarChart data={usageData}>
                                <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="#3f3f46" strokeOpacity={0.1} />
                                <XAxis 
                                    dataKey="period" 
                                    tick={{ fontSize: 10, fill: '#71717a' }}
                                    tickFormatter={(val) => {
                                        if (timeRange === 'hourly') return val.split(' ')[1] || val;
                                        return val.split('-').slice(1).join('/');
                                    }}
                                    axisLine={false}
                                    tickLine={false}
                                    dy={10}
                                />
                                <YAxis 
                                    tick={{ fontSize: 10, fill: '#71717a' }}
                                    tickFormatter={formatNumber}
                                    axisLine={false}
                                    tickLine={false}
                                />
                                <Tooltip content={<CustomTooltip />} cursor={{ fill: 'transparent' }} />
                                <Bar dataKey="total_input_tokens" name="Input" fill="#3b82f6" radius={[4, 4, 0, 0]} barSize={20} stackId="a" />
                                <Bar dataKey="total_output_tokens" name="Output" fill="#a855f7" radius={[4, 4, 0, 0]} barSize={20} stackId="a" />
                            </BarChart>
                        </ResponsiveContainer>
                    </div>
                </motion.div>

                {/* Account Pie Chart */}
                <motion.div 
                     initial={{ opacity: 0, y: 20 }}
                     animate={{ opacity: 1, y: 0 }}
                     transition={{ delay: 0.2 }}
                     className="bg-white dark:bg-zinc-900/50 rounded-2xl p-6 shadow-sm border border-zinc-200 dark:border-zinc-800"
                >
                    <h2 className="text-lg font-bold text-zinc-900 dark:text-white mb-6">
                         {t('token_stats.by_account', 'Top Accounts')}
                    </h2>
                    <div className="h-[200px] w-full">
                         {pieData.length > 0 ? (
                            <ResponsiveContainer width="100%" height="100%">
                                <PieChart>
                                    <Pie
                                        data={pieData}
                                        cx="50%"
                                        cy="50%"
                                        innerRadius={60}
                                        outerRadius={80}
                                        paddingAngle={4}
                                        dataKey="value"
                                        stroke="none"
                                    >
                                        {pieData.map((entry, index) => (
                                            <Cell key={`cell-${index}`} fill={entry.color} />
                                        ))}
                                    </Pie>
                                    <Tooltip content={<CustomTooltip />} />
                                </PieChart>
                            </ResponsiveContainer>
                         ) : (
                            <div className="h-full flex items-center justify-center text-zinc-400">
                                No Data
                            </div>
                         )}
                    </div>
                    {/* Compact Legend */}
                    <div className="mt-4 space-y-2 max-h-[100px] overflow-y-auto custom-scrollbar">
                        {pieData.slice(0, 5).map((entry, i) => (
                            <div key={i} className="flex items-center justify-between text-xs">
                                <div className="flex items-center gap-2">
                                    <div className="w-2 h-2 rounded-full" style={{ backgroundColor: entry.color }} />
                                    <span className="text-zinc-600 dark:text-zinc-400 truncate max-w-[100px]">{entry.name}</span>
                                </div>
                                <span className="font-mono text-zinc-800 dark:text-zinc-200">{formatNumber(entry.value)}</span>
                            </div>
                        ))}
                    </div>
                </motion.div>
            </div>
        </div>
    );
});
