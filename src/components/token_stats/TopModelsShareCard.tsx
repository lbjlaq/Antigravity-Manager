import React, { useMemo } from 'react';
import { Cpu } from 'lucide-react';
import { useTranslation } from 'react-i18next';

export interface ModelShareItem {
    model: string;
    total_tokens: number;
    request_count: number;
    total_input_tokens: number;
    total_output_tokens: number;
    total_cached_tokens: number;
    cost: number;
}

interface TopModelsShareCardProps {
    models: ModelShareItem[];
    formatNumber: (n: number) => string;
}

// 模型品牌色彩映射
const getModelTint = (modelName: string): string => {
    const lower = modelName.toLowerCase();
    if (lower.includes('claude')) return '#d97757'; // 珊瑚橙
    if (lower.includes('gemini')) return '#4f85e8'; // 谷歌蓝
    if (lower.includes('gpt') || lower.includes('codex') || lower.includes('openai')) return '#10a37f'; // 翡翠绿
    if (lower.includes('deepseek')) return '#0ea5e9'; // 深海蓝
    return '#8b5cf6'; // 默认紫
};

export const TopModelsShareCard: React.FC<TopModelsShareCardProps> = ({
    models,
    formatNumber,
}) => {
    const { t } = useTranslation();

    const { topModels, peakTokens, totalTokens } = useMemo(() => {
        const sorted = [...models].sort((a, b) => b.total_tokens - a.total_tokens);
        const top = sorted.slice(0, 5);
        const peak = top.length > 0 ? top[0].total_tokens : 1;
        const sum = sorted.reduce((acc, cur) => acc + cur.total_tokens, 0);
        return {
            topModels: top,
            peakTokens: Math.max(peak, 1),
            totalTokens: sum,
        };
    }, [models]);

    return (
        <div className="bg-[#121316] dark:bg-[#121316] text-white rounded-2xl p-4 sm:p-5 border border-white/[0.08] shadow-sm flex flex-col justify-between select-none">
            {/* 顶部标题栏 */}
            <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                    <Cpu className="w-4 h-4 text-purple-400 opacity-90" />
                    <span className="text-[13px] font-semibold text-white/90 tracking-wide">
                        {t('token_stats.top_models', '主力模型')}
                    </span>
                </div>
                <span className="text-xs font-mono text-white/50">
                    {models.length} {t('token_stats.models_count', '个模型')}
                </span>
            </div>

            {/* 模型列表 */}
            {topModels.length === 0 ? (
                <div className="py-6 text-center text-xs text-white/40">
                    {t('token_stats.no_activity', '暂无模型活动')}
                </div>
            ) : (
                <div className="space-y-3 py-1">
                    {topModels.map((item) => {
                        const tint = getModelTint(item.model);
                        const percent = totalTokens > 0 ? (item.total_tokens / totalTokens) * 100 : 0;
                        const widthPct = Math.max(6, (item.total_tokens / peakTokens) * 100);

                        return (
                            <div
                                key={item.model}
                                className="flex items-center justify-between gap-3 text-xs group"
                                title={`${item.model}: ${formatNumber(item.total_tokens)} Tokens ($${item.cost.toFixed(2)})`}
                            >
                                {/* 模型名 */}
                                <div className="flex items-center gap-2 min-w-0 flex-1">
                                    <span
                                        className="w-1.5 h-1.5 rounded-full flex-shrink-0"
                                        style={{ backgroundColor: tint }}
                                    />
                                    <span className="font-mono text-[11.5px] text-white/85 truncate group-hover:text-white transition-colors">
                                        {item.model}
                                    </span>
                                </div>

                                {/* 水平胶囊指示条 */}
                                <div className="w-14 sm:w-16 h-1.5 bg-white/[0.07] rounded-full overflow-hidden flex-shrink-0">
                                    <div
                                        className="h-full rounded-full transition-all duration-300"
                                        style={{
                                            width: `${widthPct}%`,
                                            backgroundColor: tint,
                                            opacity: 0.85,
                                        }}
                                    />
                                </div>

                                {/* 数量与占比 */}
                                <div className="flex items-center gap-1.5 flex-shrink-0 w-20 justify-end font-mono">
                                    <span className="text-white/80 font-medium">
                                        {formatNumber(item.total_tokens)}
                                    </span>
                                    <span className="text-[10px] text-white/40 min-w-[28px] text-right">
                                        {percent.toFixed(0)}%
                                    </span>
                                </div>
                            </div>
                        );
                    })}
                </div>
            )}
        </div>
    );
};
