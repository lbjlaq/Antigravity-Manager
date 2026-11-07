import React from 'react';
import { useTranslation } from 'react-i18next';
import { Zap, RotateCcw } from 'lucide-react';
import { usePedalModeStore } from '../../stores/usePedalModeStore';
import { useAccountStore } from '../../stores/useAccountStore';

export const PedalBanner: React.FC = () => {
    const { t } = useTranslation();
    const { active, targetModel, targetAccountIds, manualExit } = usePedalModeStore();
    const { accounts, updateAccountPriority, toggleProxyStatus } = useAccountStore();

    if (!active || !targetModel) return null;

    const modelLabel = targetModel === 'gemini' ? 'Gemini' : 'Claude';
    const targetAccounts = accounts.filter(a => targetAccountIds.includes(a.id));

    const handleManualExit = async () => {
        await manualExit(updateAccountPriority, toggleProxyStatus);
    };

    return (
        <div className="mx-4 mt-3 mb-1 p-3.5 rounded-xl bg-gradient-to-r from-amber-50 via-orange-50/60 to-amber-50 dark:from-amber-950/40 dark:via-slate-900/80 dark:to-amber-950/40 border border-amber-200/90 dark:border-amber-500/30 shadow-xs dark:shadow-md flex flex-col sm:flex-row sm:items-center justify-between gap-3 animate-in fade-in slide-in-from-top-2 duration-200">
            <div className="flex items-start sm:items-center gap-3 min-w-0">
                <div className="p-2 rounded-lg bg-amber-100 text-amber-600 dark:bg-amber-500/20 dark:text-amber-400 border border-amber-200/60 dark:border-amber-500/30 flex-shrink-0">
                    <Zap className="w-5 h-5 animate-pulse" />
                </div>
                <div className="min-w-0 flex-1">
                    <div className="text-xs sm:text-sm font-bold text-amber-950 dark:text-amber-300 flex items-center gap-2 flex-wrap">
                        <span>{t('pedal.banner_running', '🚴 蹬额度模式运行中 ({{model}})', { model: modelLabel })}</span>
                        <span className="text-[10px] bg-amber-100/90 text-amber-800 dark:bg-amber-500/20 dark:text-amber-300 px-2 py-0.5 rounded-full font-mono border border-amber-300/80 dark:border-amber-500/30 font-semibold">
                            {t('pedal.banner_priority_badge', '最高优先级调度')}
                        </span>
                        <span className="text-[10px] bg-emerald-100/90 text-emerald-800 dark:bg-emerald-500/10 dark:text-emerald-400 px-2 py-0.5 rounded-full font-mono border border-emerald-300/80 dark:border-emerald-500/20 font-semibold">
                            {t('pedal.banner_active_count', '{{count}} 个待消耗', { count: targetAccountIds.length })}
                        </span>
                    </div>
                    <div className="text-[11px] text-slate-600 dark:text-slate-400 mt-0.5 line-clamp-2 sm:line-clamp-none">
                        {t(
                            'pedal.banner_subtext',
                            '正在全力优先消耗临期配额，其他正常账号正常待命作为会话连续性与并发弹性保底。当目标账号配额全部降至 0% 或到达重置时间后，系统将自动恢复默认调度。'
                        )}
                    </div>
                    {/* 直观展示正在蹬额度的具体账号列表 */}
                    {targetAccounts.length > 0 && (
                        <div className="flex items-center gap-1.5 mt-2 flex-wrap">
                            <span className="text-[10px] font-semibold text-amber-900 dark:text-amber-300 flex items-center gap-1">
                                <span className="w-1.5 h-1.5 rounded-full bg-amber-500 animate-ping inline-block" />
                                {t('pedal.target_accounts', '待蹬账号')}:
                            </span>
                            {targetAccounts.map(acc => (
                                <span
                                    key={acc.id}
                                    className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium bg-white/90 dark:bg-slate-800/90 border border-amber-200 dark:border-amber-500/30 text-amber-900 dark:text-amber-200 shadow-2xs"
                                    title={acc.email}
                                >
                                    <span className="max-w-[150px] truncate">{acc.custom_label || acc.name || acc.email}</span>
                                </span>
                            ))}
                        </div>
                    )}
                </div>
            </div>
            <div className="flex items-center gap-2 flex-shrink-0 self-end sm:self-auto">
                <button
                    onClick={handleManualExit}
                    type="button"
                    className="px-3 py-1.5 rounded-lg bg-white hover:bg-slate-50 text-slate-700 hover:text-slate-900 border border-slate-200 shadow-xs dark:bg-slate-800 dark:hover:bg-slate-700 dark:text-slate-200 dark:border-slate-700 transition flex items-center gap-1.5 text-xs font-semibold"
                >
                    <RotateCcw className="w-3.5 h-3.5 text-slate-500 dark:text-slate-400" />
                    {t('pedal.exit_button', '手动退出并恢复')}
                </button>
            </div>
        </div>
    );
};
