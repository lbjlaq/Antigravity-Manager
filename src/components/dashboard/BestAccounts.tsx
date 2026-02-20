import { TrendingUp } from 'lucide-react';
import { Account } from '../../types/account';

interface BestAccountsProps {
    accounts: Account[];
    currentAccountId?: string;
    onSwitch?: (accountId: string) => void;
}

import { useTranslation } from 'react-i18next';

function BestAccounts({ accounts, currentAccountId, onSwitch }: BestAccountsProps) {
    const { t } = useTranslation();
    // 1. 获取按配额排序的列表 (排除当前账号)
    const geminiSorted = accounts
        .filter(a => a.id !== currentAccountId)
        .map(a => {
            const proQuota = (a.quota?.models || [])
                .filter(m =>
                    m.name.toLowerCase() === 'gemini-3-pro-high'
                    || m.name.toLowerCase() === 'gemini-3-pro-low'
                    || m.name.toLowerCase() === 'gemini-3.1-pro-high'
                    || m.name.toLowerCase() === 'gemini-3.1-pro-low'
                )
                .reduce((best, model) => Math.max(best, model.percentage || 0), 0);
            const flashQuota = a.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-flash')?.percentage || 0;
            // 综合评分：Pro 权重更高 (70%)，Flash 权重 30%
            return {
                ...a,
                quotaVal: Math.round(proQuota * 0.7 + flashQuota * 0.3),
            };
        })
        .filter(a => a.quotaVal > 0)
        .sort((a, b) => b.quotaVal - a.quotaVal);

    const claudeSorted = accounts
        .filter(a => a.id !== currentAccountId)
        .map(a => ({
            ...a,
            quotaVal: a.quota?.models.find(m => m.name.toLowerCase().includes('claude'))?.percentage || 0,
        }))
        .filter(a => a.quotaVal > 0)
        .sort((a, b) => b.quotaVal - a.quotaVal);

    let bestGemini = geminiSorted[0];
    let bestClaude = claudeSorted[0];

    // 2. 如果推荐是同一个账号，且有其他选择，尝试寻找最优的"不同账号"组合
    if (bestGemini && bestClaude && bestGemini.id === bestClaude.id) {
        const nextGemini = geminiSorted[1];
        const nextClaude = claudeSorted[1];

        // 方案A: 保持 Gemini 最优，换 Claude 次优
        // 方案B: 换 Gemini 次优，保持 Claude 最优
        // 比较标准：两者配额之和最大化 (或者优先保住 100% 的那个)

        const scoreA = bestGemini.quotaVal + (nextClaude?.quotaVal || 0);
        const scoreB = (nextGemini?.quotaVal || 0) + bestClaude.quotaVal;

        if (nextClaude && (!nextGemini || scoreA >= scoreB)) {
            // 选方案A：换 Claude
            bestClaude = nextClaude;
        } else if (nextGemini) {
            // 选方案B：换 Gemini
            bestGemini = nextGemini;
        }
        // 如果都没有次优解（例如只有一个账号），则保持原样
    }

    // 构造最终用于显示的视图模型 (兼容原有渲染逻辑)
    const bestGeminiRender = bestGemini ? { ...bestGemini, geminiQuota: bestGemini.quotaVal } : undefined;
    const bestClaudeRender = bestClaude ? { ...bestClaude, claudeQuota: bestClaude.quotaVal } : undefined;

    return (
        <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200 h-full flex flex-col">
            <h2 className="text-base font-semibold text-gray-900 dark:text-base-content mb-3 flex items-center gap-2">
                <TrendingUp className="w-4 h-4 text-blue-500 dark:text-blue-400" />
                {t('dashboard.best_accounts')}
            </h2>

            <div className="space-y-2 flex-1">
                {/* Gemini 最佳 */}
                {bestGeminiRender && (
                    <div className="flex items-center justify-between p-2.5 bg-green-50 dark:bg-green-900/20 rounded-lg border border-green-100 dark:border-green-900/30">
                        <div className="flex-1 min-w-0">
                            <div className="text-[10px] text-green-600 dark:text-green-400 font-medium mb-0.5">{t('dashboard.for_gemini')}</div>
                            <div className="font-medium text-sm text-gray-900 dark:text-base-content truncate">
                                {bestGeminiRender.email}
                            </div>
                        </div>
                        <div className="ml-2 px-2 py-0.5 bg-green-500 text-white text-xs font-semibold rounded-full">
                            {bestGeminiRender.geminiQuota}%
                        </div>
                    </div>
                )}

                {/* Claude 最佳 */}
                {bestClaudeRender && (
                    <div className="flex items-center justify-between p-2.5 bg-cyan-50 dark:bg-cyan-900/20 rounded-lg border border-cyan-100 dark:border-cyan-900/30">
                        <div className="flex-1 min-w-0">
                            <div className="text-[10px] text-cyan-600 dark:text-cyan-400 font-medium mb-0.5">{t('dashboard.for_claude')}</div>
                            <div className="font-medium text-sm text-gray-900 dark:text-base-content truncate">
                                {bestClaudeRender.email}
                            </div>
                        </div>
                        <div className="ml-2 px-2 py-0.5 bg-cyan-500 text-white text-xs font-semibold rounded-full">
                            {bestClaudeRender.claudeQuota}%
                        </div>
                    </div>
                )}

                {(!bestGeminiRender && !bestClaudeRender) && (
                    <div className="text-center py-4 text-gray-400 text-sm">
                        {t('accounts.no_data')}
                    </div>
                )}
            </div>

            {(bestGeminiRender || bestClaudeRender) && onSwitch && (
                <div className="mt-auto pt-3">
                    <button
                        className="w-full px-3 py-1.5 bg-blue-500 text-white text-xs font-medium rounded-lg hover:bg-blue-600 transition-colors"
                        onClick={() => {
                            // 优先切换到配额更高的账号
                            let targetId = bestGeminiRender?.id;
                            if (bestClaudeRender && (!bestGeminiRender || bestClaudeRender.claudeQuota > bestGeminiRender.geminiQuota)) {
                                targetId = bestClaudeRender.id;
                            }

                            if (onSwitch && targetId) {
                                onSwitch(targetId);
                            }
                        }}
                    >
                        {t('dashboard.switch_best')}
                    </button>
                </div>
            )}
        </div>
    );

}

export default BestAccounts;
