// File: src/pages/token-stats/ui/TokenStatsPage.tsx
// Main Token Stats page component

import { useTokenStats } from '../model';
import { TokenStatsHeader } from './TokenStatsHeader';
import { StatsSummary, StatsCharts, RequestHealthCards } from '@/widgets/stats';

export function TokenStatsPage() {
    const stats = useTokenStats();

    return (
        <div className="h-full w-full overflow-y-auto bg-gray-50/30 dark:bg-black/20">
            <div className="p-6 md:p-8 max-w-7xl mx-auto space-y-8">
                {/* Header Actions */}
                <TokenStatsHeader
                    timeRange={stats.timeRange}
                    loading={stats.loading}
                    onTimeRangeChange={stats.setTimeRange}
                    onRefresh={stats.fetchData}
                />

                {/* Request Health */}
                <RequestHealthCards
                    stats={stats.requestStats}
                    isLoading={stats.loading}
                    fallbackTotalRequests={stats.summary?.total_requests}
                    fallbackTotalTokens={stats.summary?.total_tokens}
                    fallbackInputTokens={stats.summary?.total_input_tokens}
                    fallbackOutputTokens={stats.summary?.total_output_tokens}
                />

                {/* Summary Cards */}
                <StatsSummary
                    summary={stats.summary}
                    modelCount={stats.modelData.length}
                    isLoading={stats.loading}
                />

                {/* Charts */}
                <StatsCharts
                    trendData={stats.activeTrendData}
                    usageData={stats.chartData}
                    pieData={stats.pieData}
                    allKeys={stats.activeKeys}
                    viewMode={stats.viewMode}
                    onViewModeChange={stats.setViewMode}
                    timeRange={stats.timeRange}
                    isLoading={stats.loading}
                />
            </div>
        </div>
    );
}
