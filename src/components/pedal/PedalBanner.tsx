import React from 'react';
import { useTranslation } from 'react-i18next';
import { Zap, RotateCcw } from 'lucide-react';
import { usePedalModeStore } from '../../stores/usePedalModeStore';
import { useAccountStore } from '../../stores/useAccountStore';

export const PedalBanner: React.FC = () => {
    const { t } = useTranslation();
    const { active, targetModel, targetAccountIds, temporarilyDisabledIds, manualExit } = usePedalModeStore();
    const { toggleProxyStatus } = useAccountStore();

    if (!active || !targetModel) return null;

    const modelLabel = targetModel === 'gemini' ? 'Gemini' : 'Claude';

    const handleManualExit = async () => {
        await manualExit(toggleProxyStatus);
    };

    return (
        <div className="mx-4 mt-3 mb-1 p-3.5 rounded-xl bg-gradient-to-r from-amber-950/60 via-slate-900/90 to-amber-950/60 border border-amber-500/40 shadow-lg flex flex-col sm:flex-row sm:items-center justify-between gap-3 animate-in fade-in slide-in-from-top-2 duration-200">
            <div className="flex items-center gap-3">
                <div className="p-2 rounded-lg bg-amber-500/20 text-amber-400 flex-shrink-0">
                    <Zap className="w-5 h-5 animate-pulse" />
                </div>
                <div>
                    <div className="text-xs sm:text-sm font-bold text-amber-300 flex items-center gap-2 flex-wrap">
                        <span>{t('pedal.banner_running', '🚴 蹬额度模式运行中 ({{model}})', { model: modelLabel })}</span>
                        <span className="text-[10px] bg-amber-500/20 text-amber-300 px-2 py-0.5 rounded-full font-mono border border-amber-500/30">
                            {t('pedal.banner_suspended_count', '已挂起 {{count}} 个账号', { count: temporarilyDisabledIds.length })}
                        </span>
                        <span className="text-[10px] bg-emerald-500/10 text-emerald-400 px-2 py-0.5 rounded-full font-mono border border-emerald-500/20">
                            {t('pedal.banner_active_count', '{{count}} 个待消耗', { count: targetAccountIds.length })}
                        </span>
                    </div>
                    <div className="text-[11px] text-slate-400 mt-0.5">
                        {t(
                            'pedal.banner_subtext',
                            '正在全力消耗临期配额，当目标账号配额全部降至 0% 或到达重置时间后，系统将自动恢复其他账号。'
                        )}
                    </div>
                </div>
            </div>
            <div className="flex items-center gap-2 flex-shrink-0 self-end sm:self-auto">
                <button
                    onClick={handleManualExit}
                    type="button"
                    className="px-3 py-1.5 rounded-lg bg-slate-800 hover:bg-slate-700 text-xs font-semibold text-slate-200 border border-slate-700 transition flex items-center gap-1.5 shadow-sm"
                >
                    <RotateCcw className="w-3.5 h-3.5 text-slate-400" />
                    {t('pedal.exit_button', '手动退出并恢复')}
                </button>
            </div>
        </div>
    );
};
