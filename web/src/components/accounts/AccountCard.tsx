import { ArrowRightLeft, RefreshCw, Trash2, Download, Info, Lock, Ban } from 'lucide-react';
import { Account } from '../../types/account';
import { getQuotaColor, formatTimeRemaining } from '../../utils/format';

interface AccountCardProps {
    account: Account;
    selected: boolean;
    onSelect: () => void;
    isCurrent: boolean;
    isRefreshing: boolean;
    isSwitching?: boolean;
    onSwitch: () => void;
    onRefresh: () => void;
    onViewDetails: () => void;
    onExport: () => void;
    onDelete: () => void;
}

import { useTranslation } from 'react-i18next';

function AccountCard({ account, selected, onSelect, isCurrent, isRefreshing, isSwitching = false, onSwitch, onRefresh, onViewDetails, onExport, onDelete }: AccountCardProps) {
    const { t } = useTranslation();
    const geminiModel = account.quota?.models.find(m => m.name.toLowerCase().includes('gemini'));
    const claudeModel = account.quota?.models.find(m => m.name.toLowerCase().includes('claude'));

    const getColorClass = (percentage: number) => {
        const color = getQuotaColor(percentage);
        switch (color) {
            case 'success': return 'bg-emerald-500';
            case 'warning': return 'bg-amber-500';
            case 'error': return 'bg-rose-500';
            default: return 'bg-gray-500';
        }
    };

    return (
        <div className={`h-[208px] flex flex-col p-3 rounded-xl border transition-all hover:shadow-md ${isCurrent
            ? 'bg-blue-50/30 border-blue-200 dark:bg-blue-900/10 dark:border-blue-900/30'
            : 'bg-white dark:bg-base-100 border-gray-200 dark:border-base-300'
            } ${isRefreshing ? 'opacity-70' : ''}`}>

            {/* Header: Checkbox + Email + Badges */}
            <div className="flex-none flex items-start gap-3 mb-2">
                <input
                    type="checkbox"
                    className="mt-1 checkbox checkbox-xs rounded border-2 border-gray-400 dark:border-gray-500 checked:border-blue-600 checked:bg-blue-600 [--chkbg:theme(colors.blue.600)] [--chkfg:white]"
                    checked={selected}
                    onChange={() => onSelect()}
                    onClick={(e) => e.stopPropagation()}
                />
                <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 flex-wrap">
                        <h3 className={`font-semibold text-sm truncate ${isCurrent ? 'text-blue-700 dark:text-blue-400' : 'text-gray-900 dark:text-base-content'}`} title={account.email}>
                            {account.email}
                        </h3>
                        {isCurrent && (
                            <span className="px-1.5 py-0.5 rounded-full bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 text-[10px] font-semibold whitespace-nowrap">
                                {t('accounts.current')}
                            </span>
                        )}
                        {account.quota?.is_forbidden && (
                            <span className="px-1.5 py-0.5 rounded-full bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400 text-[10px] font-semibold flex items-center gap-1 whitespace-nowrap" title={t('accounts.forbidden_tooltip')}>
                                <Lock className="w-3 h-3" />
                                {t('accounts.forbidden')}
                            </span>
                        )}
                    </div>
                </div>
            </div>

            {/* Quota Section */}
            <div className="flex-1 mb-2 space-y-2 overflow-y-auto scrollbar-none">
                {account.quota?.is_forbidden ? (
                    <div className="flex items-center gap-2 text-xs text-red-500 dark:text-red-400 bg-red-50/50 dark:bg-red-900/10 p-2 rounded-lg border border-red-100 dark:border-red-900/30">
                        <Ban className="w-4 h-4 shrink-0" />
                        <span>{t('accounts.forbidden_msg')}</span>
                    </div>
                ) : (
                    <>
                        {/* Gemini */}
                        <div>
                            <div className="flex justify-between items-center mb-1">
                                <span className="text-xs font-medium text-gray-500 dark:text-gray-400">Gemini 3 Pro</span>
                                <span className="text-xs font-bold font-mono dark:text-gray-300">{geminiModel?.percentage ?? 0}%</span>
                            </div>
                            <div className="h-1 bg-gray-100 dark:bg-base-300 rounded-full overflow-hidden">
                                {geminiModel && (
                                    <div
                                        className={`h-full ${getColorClass(geminiModel.percentage)} rounded-full`}
                                        style={{ width: `${geminiModel.percentage}%` }}
                                    />
                                )}
                            </div>
                            {geminiModel?.reset_time && (
                                <div className="text-[10px] text-gray-400 text-right mt-0.5 font-mono">
                                    R: {formatTimeRemaining(geminiModel.reset_time)}
                                </div>
                            )}
                        </div>

                        {/* Claude */}
                        <div>
                            <div className="flex justify-between items-center mb-1">
                                <span className="text-xs font-medium text-gray-500 dark:text-gray-400">Claude 4.5</span>
                                <span className="text-xs font-bold font-mono dark:text-gray-300">{claudeModel?.percentage ?? 0}%</span>
                            </div>
                            <div className="h-1 bg-gray-100 dark:bg-base-300 rounded-full overflow-hidden">
                                {claudeModel && (
                                    <div
                                        className={`h-full ${getColorClass(claudeModel.percentage)} rounded-full`}
                                        style={{ width: `${claudeModel.percentage}%` }}
                                    />
                                )}
                            </div>
                            {claudeModel?.reset_time && (
                                <div className="text-[10px] text-gray-400 text-right mt-0.5 font-mono">
                                    R: {formatTimeRemaining(claudeModel.reset_time)}
                                </div>
                            )}
                        </div>
                    </>
                )}
            </div>

            {/* Footer: Actions & Date */}
            <div className="flex-none flex items-center justify-between pt-2 border-t border-gray-100 dark:border-base-200">
                <span className="text-[10px] text-gray-400 dark:text-gray-500 font-mono">
                    {new Date(account.last_used * 1000).toLocaleString([], { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' })}
                </span>

                <div className="flex items-center gap-1">
                    <button
                        className="p-1.5 text-gray-400 hover:text-sky-600 dark:hover:text-sky-400 hover:bg-sky-50 dark:hover:bg-sky-900/30 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); onViewDetails(); }}
                        title={t('common.details')}
                    >
                        <Info className="w-3.5 h-3.5" />
                    </button>
                    {!isCurrent && (
                        <button
                            className={`p-1.5 rounded-lg transition-all ${isSwitching ? 'text-blue-600 bg-blue-50 dark:text-blue-400 dark:bg-blue-900/10 cursor-not-allowed' : 'text-gray-400 hover:text-blue-600 dark:hover:text-blue-400 hover:bg-blue-50 dark:hover:bg-blue-900/30'}`}
                            onClick={(e) => { e.stopPropagation(); onSwitch(); }}
                            title={isSwitching ? t('common.loading') : t('common.switch')}
                            disabled={isSwitching}
                        >
                            <ArrowRightLeft className={`w-3.5 h-3.5 ${isSwitching ? 'animate-spin' : ''}`} />
                        </button>
                    )}
                    <button
                        className={`p-1.5 rounded-lg transition-all ${isRefreshing
                            ? 'text-green-600 bg-green-50'
                            : 'text-gray-400 hover:text-green-600 hover:bg-green-50'}`}
                        onClick={(e) => { e.stopPropagation(); onRefresh(); }}
                        disabled={isRefreshing}
                        title={t('common.refresh')}
                    >
                        <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin' : ''}`} />
                    </button>
                    <button
                        className="p-1.5 text-gray-400 hover:text-indigo-600 hover:bg-indigo-50 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); onExport(); }}
                        title={t('common.export')}
                    >
                        <Download className="w-3.5 h-3.5" />
                    </button>
                    <button
                        className="p-1.5 text-gray-400 hover:text-red-600 hover:bg-red-50 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); onDelete(); }}
                        title={t('common.delete')}
                    >
                        <Trash2 className="w-3.5 h-3.5" />
                    </button>
                </div>
            </div>
        </div>
    );
}

export default AccountCard;
