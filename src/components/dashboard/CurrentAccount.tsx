import { CheckCircle, Mail, Diamond, Gem, Circle, Tag, Lock, Clock, AlertTriangle } from 'lucide-react';
import { Account, getAccountTier, ModelQuota } from '../../types/account';
import { formatTimeRemaining } from '../../utils/format';
import { findQuotaModel, getModelProtectionKey, getModelDisplayName, findImageQuotaModel } from '../../config/modelConfig';
import { getModelConstrainedQuota, DashboardQuotaView } from '../../utils/quotaDisplay';
import { useTranslation } from 'react-i18next';

interface CurrentAccountProps {
    account: Account | null;
    quotaView?: DashboardQuotaView;
    onSwitch?: () => void;
}

function CurrentAccount({ account, quotaView = 'weighted', onSwitch }: CurrentAccountProps) {
    const { t } = useTranslation();
    if (!account) {
        return (
            <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
                <h2 className="text-base font-semibold text-gray-900 dark:text-base-content mb-2 flex items-center gap-2">
                    <CheckCircle className="w-4 h-4 text-green-500" />
                    {t('dashboard.current_account')}
                </h2>
                <div className="text-center py-4 text-gray-400 dark:text-gray-500 text-sm">
                    {t('dashboard.no_active_account')}
                </div>
            </div>
        );
    }

    const geminiProModel = findQuotaModel(account.quota?.models, 'gemini-pro');
    const geminiFlashModel = findQuotaModel(account.quota?.models, 'gemini-flash');

    const geminiImageModel = findImageQuotaModel(account.quota?.models);
    const nowSeconds = Math.floor(Date.now() / 1000);
    const imageProtectionKey = getModelProtectionKey(geminiImageModel?.name || '');
    const liveImageLimit = imageProtectionKey
        ? account.live_limited_models?.[imageProtectionKey]
        : undefined;
    const isImageLiveLimited = Boolean(liveImageLimit && liveImageLimit.until > nowSeconds);

    const claudeModel = findQuotaModel(account.quota?.models, 'claude');

    // 辅助渲染单条模型配额（支持综合加权、5H、周配额及双向木桶约束标记）
    const renderModelItem = (
        model: ModelQuota,
        displayName: string,
        colorTheme: 'emerald' | 'cyan',
        isLocked: boolean,
        extraIcon?: React.ReactNode
    ) => {
        const q = getModelConstrainedQuota(model.name, model, account.quota?.quota_groups, quotaView);

        // 重置时间详情 tooltip
        const resetTooltip = [
            q.fiveHourResetTime ? `5H: ${new Date(q.fiveHourResetTime).toLocaleTimeString()}` : null,
            q.weeklyResetTime ? `周: ${new Date(q.weeklyResetTime).toLocaleDateString()}` : null,
        ].filter(Boolean).join(' | ');

        // 状态徽标
        let statusBadge: React.ReactNode = null;
        if (q.isWeeklyExhausted) {
            statusBadge = (
                <span
                    className="px-1 py-0.2 rounded text-[9px] font-bold bg-rose-100 text-rose-700 dark:bg-rose-900/40 dark:text-rose-300"
                    title={t('dashboard.zero_weekly_warning', { count: 1, defaultValue: '周配额已耗尽触发熔断 (0%)' })}
                >
                    {t('dashboard.mini_tag_exhausted', '熔断')}
                </span>
            );
        } else if (quotaView === '5h' && q.isWeeklyConstrained && q.raw5h !== null && q.rawWeekly !== null) {
            statusBadge = (
                <span
                    className="px-1 py-0.2 rounded text-[9px] font-bold bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-300 flex items-center gap-0.5"
                    title={t('dashboard.constrained_by_weekly_desc', {
                        raw5h: q.raw5h,
                        rawWeekly: q.rawWeekly,
                        effective: q.effectivePercentage,
                        defaultValue: `5H 滚动剩余 ${q.raw5h}%，但受周总配额 ${q.rawWeekly}% 约束`,
                    })}
                >
                    <AlertTriangle className="w-2.5 h-2.5" />
                    {t('dashboard.mini_tag_constrained', '周限')}: {q.rawWeekly}%
                </span>
            );
        } else if (quotaView === 'weekly' && q.is5hCooling) {
            statusBadge = (
                <span
                    className="px-1 py-0.2 rounded text-[9px] font-bold bg-blue-100 text-blue-800 dark:bg-blue-900/40 dark:text-blue-300 flex items-center gap-0.5"
                    title={t('dashboard.cooling_5h_desc', {
                        rawWeekly: q.rawWeekly,
                        defaultValue: `本周总配额为 ${q.rawWeekly}%，但当前 5H 窗口打满已耗尽，处于即时冷却重置期`,
                    })}
                >
                    <Clock className="w-2.5 h-2.5" />
                    {t('dashboard.mini_tag_cooling', '冷却')}
                </span>
            );
        }

        // 颜色与样式
        const pct = q.effectivePercentage;
        let textColor = '';
        let barGradient = '';

        if (colorTheme === 'cyan') {
            textColor = pct >= 50 ? 'text-cyan-600 dark:text-cyan-400' : pct >= 20 ? 'text-orange-600 dark:text-orange-400' : 'text-rose-600 dark:text-rose-400';
            barGradient = pct >= 50 ? 'bg-gradient-to-r from-cyan-400 to-cyan-500' : pct >= 20 ? 'bg-gradient-to-r from-orange-400 to-orange-500' : 'bg-gradient-to-r from-rose-400 to-rose-500';
        } else {
            textColor = pct >= 50 ? 'text-emerald-600 dark:text-emerald-400' : pct >= 20 ? 'text-amber-600 dark:text-amber-400' : 'text-rose-600 dark:text-rose-400';
            barGradient = pct >= 50 ? 'bg-gradient-to-r from-emerald-400 to-emerald-500' : pct >= 20 ? 'bg-gradient-to-r from-amber-400 to-amber-500' : 'bg-gradient-to-r from-rose-400 to-rose-500';
        }

        return (
            <div className="space-y-1.5" key={model.name}>
                <div className="flex justify-between items-baseline">
                    <span className="text-xs font-medium text-gray-600 dark:text-gray-400 flex items-center gap-1">
                        {extraIcon}
                        {isLocked && <Lock className="w-2.5 h-2.5 text-rose-500" />}
                        {displayName}
                    </span>
                    <div className="flex items-center gap-1.5">
                        {statusBadge}
                        <span className="text-[10px] text-gray-400 dark:text-gray-500" title={resetTooltip || `${t('accounts.reset_time')}: ${model.reset_time}`}>
                            {q.resetTime ? `R: ${formatTimeRemaining(q.resetTime)}` : t('common.unknown')}
                        </span>
                        <span className={`text-xs font-bold ${textColor}`}>
                            {pct}%
                        </span>
                    </div>
                </div>
                <div className="w-full bg-gray-100 dark:bg-base-300 rounded-full h-1.5 overflow-hidden">
                    <div
                        className={`h-full rounded-full transition-all duration-700 ${barGradient}`}
                        style={{ width: `${pct}%` }}
                    />
                </div>
            </div>
        );
    };

    return (
        <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200 h-full flex flex-col">
            <h2 className="text-base font-semibold text-gray-900 dark:text-base-content mb-3 flex items-center gap-2">
                <CheckCircle className="w-4 h-4 text-green-500" />
                {t('dashboard.current_account')}
            </h2>

            <div className="space-y-4 flex-1">
                <div className="flex items-center gap-3 mb-1">
                    <div className="flex items-center gap-2 flex-1 min-w-0">
                        <Mail className="w-3.5 h-3.5 text-gray-400" />
                        <span className="text-sm font-medium text-gray-700 dark:text-gray-300 truncate">{account.email}</span>
                    </div>
                    {/* 订阅类型 */}
                    {(() => {
                        const tier = getAccountTier(account);
                        if (tier === 'ultra') {
                            return (
                                <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-gradient-to-r from-purple-600 to-pink-600 text-white text-[10px] font-bold shadow-sm shrink-0">
                                    <Gem className="w-2.5 h-2.5 fill-current" />
                                    ULTRA
                                </span>
                            );
                        } else if (tier === 'pro') {
                            return (
                                <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-gradient-to-r from-blue-600 to-indigo-600 text-white text-[10px] font-bold shadow-sm shrink-0">
                                    <Diamond className="w-2.5 h-2.5 fill-current" />
                                    PRO
                                </span>
                            );
                        } else {
                            return (
                                <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-gray-100 dark:bg-white/10 text-gray-500 dark:text-gray-400 text-[10px] font-bold shadow-sm border border-gray-200 dark:border-white/10 shrink-0">
                                    <Circle className="w-2.5 h-2.5" />
                                    FREE
                                </span>
                            );
                        }
                    })()}
                    {/* 自定义标签 */}
                    {account.custom_label && (
                        <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-orange-100 dark:bg-orange-900/30 text-orange-600 dark:text-orange-400 text-[10px] font-bold shadow-sm shrink-0">
                            <Tag className="w-2.5 h-2.5" />
                            {account.custom_label}
                        </span>
                    )}
                </div>

                {/* Gemini Pro 配额 */}
                {geminiProModel && renderModelItem(
                    geminiProModel,
                    getModelDisplayName(geminiProModel),
                    'emerald',
                    Boolean(account.protected_models?.includes('gemini-3-pro-high') || account.protected_models?.includes('gemini-3.1-pro-high'))
                )}

                {/* Gemini 3 Pro Image 配额 */}
                {geminiImageModel && renderModelItem(
                    geminiImageModel,
                    getModelDisplayName(geminiImageModel),
                    'emerald',
                    Boolean(imageProtectionKey && account.protected_models?.includes(imageProtectionKey)),
                    isImageLiveLimited ? <Clock className="w-2.5 h-2.5 text-amber-500" /> : undefined
                )}

                {/* Gemini Flash 配额 */}
                {geminiFlashModel && renderModelItem(
                    geminiFlashModel,
                    getModelDisplayName(geminiFlashModel),
                    'emerald',
                    Boolean(account.protected_models?.includes('gemini-3-flash'))
                )}

                {/* Claude 配额 */}
                {claudeModel && renderModelItem(
                    claudeModel,
                    getModelDisplayName(claudeModel, t('common.claude_series', 'Claude 系列')),
                    'cyan',
                    Boolean(account.protected_models?.includes('claude'))
                )}
            </div>

            {onSwitch && (
                <div className="mt-auto pt-3">
                    <button
                        className="w-full px-3 py-1.5 text-xs text-gray-700 dark:text-gray-300 border border-gray-200 dark:border-base-300 rounded-lg hover:bg-gray-50 dark:hover:bg-base-200 transition-colors"
                        onClick={onSwitch}
                    >
                        {t('dashboard.switch_account')}
                    </button>
                </div>
            )}
        </div>
    );
}

export default CurrentAccount;
