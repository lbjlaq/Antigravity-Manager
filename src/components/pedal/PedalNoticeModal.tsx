import React from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { Zap, Clock, ShieldAlert, Sparkles } from 'lucide-react';
import { usePedalModeStore } from '../../stores/usePedalModeStore';
import { useAccountStore } from '../../stores/useAccountStore';
import { formatTimeRemaining } from '../../utils/format';

export const PedalNoticeModal: React.FC = () => {
    const { t } = useTranslation();
    const { pendingModal, dismissModal, suppressModel, activatePedalMode } = usePedalModeStore();
    const { accounts, updateAccountPriority } = useAccountStore();

    if (!pendingModal) return null;

    const { model, modelLabel, candidates } = pendingModal;

    const handleActivate = async () => {
        const targetIds = candidates.map(c => c.accountId);
        await activatePedalMode(model, targetIds, accounts, updateAccountPriority);
    };

    const handleSuppress = () => {
        suppressModel(model);
    };

    return createPortal(
        <div className="fixed inset-0 z-50 bg-black/60 dark:bg-black/80 backdrop-blur-sm flex items-center justify-center p-4">
            <div className="bg-white dark:bg-slate-900 border border-amber-200 dark:border-amber-500/30 rounded-2xl max-w-lg w-full p-6 shadow-2xl space-y-4 animate-in fade-in zoom-in-95 duration-150">
                {/* 头部 */}
                <div className="flex items-start gap-3">
                    <div className="p-2.5 rounded-xl bg-amber-100 text-amber-600 dark:bg-amber-500/10 dark:text-amber-400 border border-amber-200/80 dark:border-amber-500/30 flex-shrink-0">
                        <Zap className="w-6 h-6 animate-pulse" />
                    </div>
                    <div>
                        <h3 className="text-base font-bold text-slate-900 dark:text-slate-100 flex items-center gap-2">
                            <span>{t('pedal.notice_title', '发现即将重置的 {{model}} 周配额！', { model: modelLabel })}</span>
                        </h3>
                        <p className="text-xs text-slate-600 dark:text-slate-400 mt-1">
                            {t('pedal.notice_desc', '以下账号的周配额在 24 小时内即将重置清零，可启动“蹬额度模式”全力消耗，避免额度作废：')}
                        </p>
                    </div>
                </div>

                {/* 候选账号列表 */}
                <div className="space-y-2 bg-slate-50 dark:bg-slate-950/60 p-3 rounded-xl border border-slate-200/80 dark:border-slate-800/80 max-h-56 overflow-y-auto">
                    {candidates.map(candidate => (
                        <div
                            key={candidate.accountId}
                            className="flex items-center justify-between text-xs py-2 px-2.5 bg-white dark:bg-slate-900/90 border border-slate-200 dark:border-slate-800 rounded-lg shadow-2xs"
                        >
                            <div className="flex items-center gap-2 min-w-0 pr-2">
                                <span className="w-2 h-2 rounded-full bg-amber-500 flex-shrink-0" />
                                <span className="font-semibold text-slate-800 dark:text-slate-200 truncate">{candidate.accountName}</span>
                                <span className="text-[11px] text-slate-500 font-mono truncate">({candidate.email})</span>
                            </div>
                            <div className="flex items-center gap-3 font-mono flex-shrink-0">
                                <span className="text-amber-600 dark:text-amber-400 font-bold">{candidate.percentage}%</span>
                                <span className="text-slate-500 dark:text-slate-400 flex items-center gap-1 text-[11px]">
                                    <Clock className="w-3 h-3 text-slate-400 dark:text-slate-500" />
                                    {formatTimeRemaining(candidate.resetTime)}
                                </span>
                            </div>
                        </div>
                    ))}
                </div>

                {/* 说明框 */}
                <div className="text-xs text-amber-900 dark:text-amber-300/90 bg-amber-50 dark:bg-amber-500/5 border border-amber-200/80 dark:border-amber-500/10 p-2.5 rounded-lg flex items-start gap-2">
                    <ShieldAlert className="w-4 h-4 text-amber-600 dark:text-amber-400 flex-shrink-0 mt-0.5" />
                    <span>
                        {t(
                            'pedal.mode_explanation',
                            '启动后将设置以上账号为最高调度优先级（priority = 1），优先全力消耗临期配额，同时保留其他账号用于会话连续性与并发弹性保底。待额度全部用完 (0%) 或官方重置后，系统将自动恢复默认优先级。'
                        )}
                    </span>
                </div>

                {/* 底部按钮栏 */}
                <div className="flex flex-col sm:flex-row items-center justify-between gap-3 pt-3 border-t border-slate-200 dark:border-slate-800">
                    <button
                        onClick={handleSuppress}
                        type="button"
                        className="text-xs text-slate-500 hover:text-rose-600 dark:text-slate-400 dark:hover:text-rose-400 transition underline underline-offset-2"
                    >
                        {t('pedal.suppress_button', '不再提示 {{model}} 模型', { model: modelLabel })}
                    </button>
                    <div className="flex items-center gap-2 w-full sm:w-auto">
                        <button
                            onClick={dismissModal}
                            type="button"
                            className="flex-1 sm:flex-initial px-4 py-2 rounded-lg bg-slate-100 hover:bg-slate-200 text-slate-700 dark:bg-slate-800 dark:hover:bg-slate-700 text-xs dark:text-slate-300 font-medium transition"
                        >
                            {t('common.cancel', '暂不启动')}
                        </button>
                        <button
                            onClick={handleActivate}
                            type="button"
                            className="flex-1 sm:flex-initial px-4 py-2 rounded-lg bg-amber-500 hover:bg-amber-400 text-xs text-slate-950 font-bold transition shadow-lg shadow-amber-500/20 flex items-center justify-center gap-1.5"
                        >
                            <Sparkles className="w-3.5 h-3.5 text-slate-950" />
                            {t('pedal.activate_button', '启动蹬额度模式')}
                        </button>
                    </div>
                </div>
            </div>
        </div>,
        document.body
    );
};
