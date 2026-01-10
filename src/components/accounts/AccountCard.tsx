import { ArrowRightLeft, RefreshCw, Trash2, Download, Info, Lock, Ban, Diamond, Gem, Circle, Clock, ToggleLeft, ToggleRight, Fingerprint } from 'lucide-react';
import { Account } from '../../types/account';
import { getQuotaColor, formatTimeRemaining, getTimeRemainingColor } from '../../utils/format';
import { cn } from '../../utils/cn';
import { useTranslation } from 'react-i18next';

interface AccountCardProps {
    account: Account;
    selected: boolean;
    onSelect: () => void;
    isCurrent: boolean;
    isRefreshing: boolean;
    isSwitching?: boolean;
    onSwitch: () => void;
    onRefresh: () => void;
    onViewDevice: () => void;
    onViewDetails: () => void;
    onExport: () => void;
    onDelete: () => void;
    onToggleProxy: () => void;
}


function AccountCard({ account, selected, onSelect, isCurrent, isRefreshing, isSwitching = false, onSwitch, onRefresh, onViewDetails, onExport, onDelete, onToggleProxy, onViewDevice }: AccountCardProps) {
    const { t } = useTranslation();
    const geminiProModel = account.quota?.models.find(m => m.name === 'gemini-3-pro-high');
    const geminiFlashModel = account.quota?.models.find(m => m.name === 'gemini-3-flash');
    const geminiImageModel = account.quota?.models.find(m => m.name === 'gemini-3-pro-image');
    const claudeModel = account.quota?.models.find(m => m.name === 'claude-sonnet-4-5-thinking');
    const isDisabled = Boolean(account.disabled);

    const getColorClass = (percentage: number) => {
        const color = getQuotaColor(percentage);
        switch (color) {
            case 'success': return 'bg-emerald-500';
            case 'warning': return 'bg-amber-500';
            case 'error': return 'bg-rose-500';
            default: return 'bg-gray-500';
        }
    };

    const getTimeColorClass = (resetTime: string | undefined) => {
        const color = getTimeRemainingColor(resetTime);
        switch (color) {
            case 'success': return 'text-emerald-500 dark:text-emerald-400';
            case 'warning': return 'text-amber-500 dark:text-amber-400';
            default: return 'text-gray-400 dark:text-gray-500 opacity-60';
        }
    };

    return (
        <div className={cn(
            "flex flex-col p-3 rounded-xl border transition-all hover:shadow-md",
            isCurrent
                ? "bg-blue-50/30 border-blue-200 dark:bg-blue-900/10 dark:border-blue-900/30"
                : "bg-white dark:bg-base-100 border-gray-200 dark:border-base-300",
            (isRefreshing || isDisabled) && "opacity-70"
        )}>

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
                        <h3 className={cn(
                            "font-semibold text-sm truncate",
                            isCurrent ? "text-blue-700 dark:text-blue-400" : "text-gray-900 dark:text-base-content"
                        )} title={account.email}>
                            {account.email}
                        </h3>
                        <div className="flex items-center gap-1.5 shrink-0">
                            {isCurrent && (
                                <span className="px-1.5 py-0.5 rounded-md bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300 text-[9px] font-bold shadow-sm border border-blue-200/50">
                                    {t('accounts.current').toUpperCase()}
                                </span>
                            )}
                            {isDisabled && (
                                <span
                                    className="px-1.5 py-0.5 rounded-md bg-rose-100 dark:bg-rose-900/40 text-rose-700 dark:text-rose-300 text-[9px] font-bold flex items-center gap-1 shadow-sm border border-rose-200/50"
                                    title={account.disabled_reason || t('accounts.disabled_tooltip')}
                                >
                                    <Ban className="w-2.5 h-2.5" />
                                    {t('accounts.disabled').toUpperCase()}
                                </span>
                            )}
                            {account.quota?.is_forbidden && (
                                <span className="px-1.5 py-0.5 rounded-md bg-red-100 dark:bg-red-900/40 text-red-600 dark:text-red-400 text-[9px] font-bold flex items-center gap-1 shadow-sm border border-red-200/50" title={t('accounts.forbidden_tooltip')}>
                                    <Lock className="w-2.5 h-2.5" />
                                    {t('accounts.forbidden').toUpperCase()}
                                </span>
                            )}
                            {/* 订阅类型徽章 */}
                            {account.quota?.subscription_tier && (() => {
                                const tier = account.quota.subscription_tier.toLowerCase();
                                if (tier.includes('ultra')) {
                                    return (
                                        <span className="flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-gradient-to-r from-purple-600 to-pink-600 text-white text-[9px] font-bold shadow-sm">
                                            <Gem className="w-2.5 h-2.5 fill-current" />
                                            ULTRA
                                        </span>
                                    );
                                } else if (tier.includes('pro')) {
                                    return (
                                        <span className="flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-gradient-to-r from-blue-600 to-indigo-600 text-white text-[9px] font-bold shadow-sm">
                                            <Diamond className="w-2.5 h-2.5 fill-current" />
                                            PRO
                                        </span>
                                    );
                                } else {
                                    return (
                                        <span className="flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-gray-100 dark:bg-white/10 text-gray-500 dark:text-gray-400 text-[9px] font-bold shadow-sm border border-gray-200 dark:border-white/10">
                                            <Circle className="w-2.5 h-2.5" />
                                            FREE
                                        </span>
                                    );
                                }
                            })()}
                        </div>
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
                        <div className="grid grid-cols-2 gap-1.5">
                            {/* Gemini Pro */}
                            <div className="relative h-[26px] flex items-center px-1 rounded-lg overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5 group/quota">
                                {geminiProModel && (
                                    <div
                                        className={`absolute inset-y-0 left-0 transition-all duration-700 ease-out opacity-15 dark:opacity-20 ${getColorClass(geminiProModel.percentage)}`}
                                        style={{ width: `${geminiProModel.percentage}%` }}
                                    />
                                )}
                                <div className="relative z-10 w-full flex items-center text-[9px] font-mono leading-none whitespace-nowrap">
                                    <span className="w-[46px] text-gray-500 dark:text-gray-400 font-bold truncate pr-0.5" title="Gemini 3 Pro">G3 Pro</span>
                                    <div className="flex-1 flex justify-center overflow-hidden">
                                        {geminiProModel?.reset_time ? (
                                            <span className={cn("flex items-center gap-0.5 font-medium transition-colors whitespace-nowrap", getTimeColorClass(geminiProModel.reset_time))}>
                                                <Clock className="w-2.5 h-2.5 shrink-0" />
                                                {formatTimeRemaining(geminiProModel.reset_time)}
                                            </span>
                                        ) : (
                                            <span className="text-gray-300 dark:text-gray-600 italic scale-90">N/A</span>
                                        )}
                                    </div>
                                    <span className={cn("w-[30px] text-right font-bold transition-colors shrink-0",
                                        getQuotaColor(geminiProModel?.percentage || 0) === 'success' ? 'text-emerald-600 dark:text-emerald-400' :
                                            getQuotaColor(geminiProModel?.percentage || 0) === 'warning' ? 'text-amber-600 dark:text-amber-400' : 'text-rose-600 dark:text-rose-400'
                                    )}>
                                        {(geminiProModel?.percentage || 0)}%
                                    </span>
                                </div>
                            </div>

                            {/* Gemini Flash */}
                            <div className="relative h-[26px] flex items-center px-1 rounded-lg overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5 group/quota">
                                {geminiFlashModel && (
                                    <div
                                        className={`absolute inset-y-0 left-0 transition-all duration-700 ease-out opacity-15 dark:opacity-20 ${getColorClass(geminiFlashModel.percentage)}`}
                                        style={{ width: `${geminiFlashModel.percentage}%` }}
                                    />
                                )}
                                <div className="relative z-10 w-full flex items-center text-[9px] font-mono leading-none whitespace-nowrap">
                                    <span className="w-[46px] text-gray-500 dark:text-gray-400 font-bold truncate pr-0.5" title="Gemini 3 Flash">G3 Flash</span>
                                    <div className="flex-1 flex justify-center overflow-hidden">
                                        {geminiFlashModel?.reset_time ? (
                                            <span className={cn("flex items-center gap-0.5 font-medium transition-colors whitespace-nowrap", getTimeColorClass(geminiFlashModel.reset_time))}>
                                                <Clock className="w-2.5 h-2.5 shrink-0" />
                                                {formatTimeRemaining(geminiFlashModel.reset_time)}
                                            </span>
                                        ) : (
                                            <span className="text-gray-300 dark:text-gray-600 italic scale-90">N/A</span>
                                        )}
                                    </div>
                                    <span className={cn("w-[30px] text-right font-bold transition-colors shrink-0",
                                        getQuotaColor(geminiFlashModel?.percentage || 0) === 'success' ? 'text-emerald-600 dark:text-emerald-400' :
                                            getQuotaColor(geminiFlashModel?.percentage || 0) === 'warning' ? 'text-amber-600 dark:text-amber-400' : 'text-rose-600 dark:text-rose-400'
                                    )}>
                                        {(geminiFlashModel?.percentage || 0)}%
                                    </span>
                                </div>
                            </div>

                            {/* Gemini Image */}
                            <div className="relative h-[26px] flex items-center px-1 rounded-lg overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5 group/quota">
                                {geminiImageModel && (
                                    <div
                                        className={`absolute inset-y-0 left-0 transition-all duration-700 ease-out opacity-15 dark:opacity-20 ${getColorClass(geminiImageModel.percentage)}`}
                                        style={{ width: `${geminiImageModel.percentage}%` }}
                                    />
                                )}
                                <div className="relative z-10 w-full flex items-center text-[9px] font-mono leading-none whitespace-nowrap">
                                    <span className="w-[46px] text-gray-500 dark:text-gray-400 font-bold truncate pr-0.5" title="Gemini 3 Pro Image">G3 Image</span>
                                    <div className="flex-1 flex justify-center overflow-hidden">
                                        {geminiImageModel?.reset_time ? (
                                            <span className={cn("flex items-center gap-0.5 font-medium transition-colors whitespace-nowrap", getTimeColorClass(geminiImageModel.reset_time))}>
                                                <Clock className="w-2.5 h-2.5 shrink-0" />
                                                {formatTimeRemaining(geminiImageModel.reset_time)}
                                            </span>
                                        ) : (
                                            <span className="text-gray-300 dark:text-gray-600 italic scale-90">N/A</span>
                                        )}
                                    </div>
                                    <span className={cn("w-[30px] text-right font-bold transition-colors shrink-0",
                                        getQuotaColor(geminiImageModel?.percentage || 0) === 'success' ? 'text-emerald-600 dark:text-emerald-400' :
                                            getQuotaColor(geminiImageModel?.percentage || 0) === 'warning' ? 'text-amber-600 dark:text-amber-400' : 'text-rose-600 dark:text-rose-400'
                                    )}>
                                        {(geminiImageModel?.percentage || 0)}%
                                    </span>
                                </div>
                            </div>

                            {/* Claude */}
                            <div className="relative h-[26px] flex items-center px-1 rounded-lg overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5 group/quota">
                                {claudeModel && (
                                    <div
                                        className={`absolute inset-y-0 left-0 transition-all duration-700 ease-out opacity-15 dark:opacity-20 ${getColorClass(claudeModel.percentage)}`}
                                        style={{ width: `${claudeModel.percentage}%` }}
                                    />
                                )}
                                <div className="relative z-10 w-full flex items-center text-[9px] font-mono leading-none whitespace-nowrap">
                                    <span className="w-[46px] text-gray-500 dark:text-gray-400 font-bold truncate pr-0.5" title="Claude-sonnet-4.5">Claude 4.5</span>
                                    <div className="flex-1 flex justify-center overflow-hidden">
                                        {claudeModel?.reset_time ? (
                                            <span className={cn("flex items-center gap-0.5 font-medium transition-colors whitespace-nowrap", getTimeColorClass(claudeModel.reset_time))}>
                                                <Clock className="w-2.5 h-2.5 shrink-0" />
                                                {formatTimeRemaining(claudeModel.reset_time)}
                                            </span>
                                        ) : (
                                            <span className="text-gray-300 dark:text-gray-600 italic scale-90">N/A</span>
                                        )}
                                    </div>
                                    <span className={cn("w-[30px] text-right font-bold transition-colors shrink-0",
                                        getQuotaColor(claudeModel?.percentage || 0) === 'success' ? 'text-emerald-600 dark:text-emerald-400' :
                                            getQuotaColor(claudeModel?.percentage || 0) === 'warning' ? 'text-amber-600 dark:text-amber-400' : 'text-rose-600 dark:text-rose-400'
                                    )}>
                                        {(claudeModel?.percentage || 0)}%
                                    </span>
                                </div>
                            </div>
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
                    <button
                        className="p-1.5 text-gray-400 hover:text-indigo-600 dark:hover:text-indigo-400 hover:bg-indigo-50 dark:hover:bg-indigo-900/30 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); onViewDevice(); }}
                        title="设备指纹"
                    >
                        <Fingerprint className="w-3.5 h-3.5" />
                    </button>
                    <button
                        className={`p-1.5 rounded-lg transition-all ${(isSwitching || isDisabled) ? 'text-blue-600 bg-blue-50 dark:text-blue-400 dark:bg-blue-900/10 cursor-not-allowed' : 'text-gray-400 hover:text-blue-600 dark:hover:text-blue-400 hover:bg-blue-50 dark:hover:bg-blue-900/30'}`}
                        onClick={(e) => { e.stopPropagation(); onSwitch(); }}
                        title={isDisabled ? t('accounts.disabled_tooltip') : (isSwitching ? t('common.loading') : t('common.switch'))}
                        disabled={isSwitching || isDisabled}
                    >
                        <ArrowRightLeft className={`w-3.5 h-3.5 ${isSwitching ? 'animate-spin' : ''}`} />
                    </button>
                    <button
                        className={`p-1.5 rounded-lg transition-all ${isRefreshing
                            ? 'text-green-600 bg-green-50'
                            : 'text-gray-400 hover:text-green-600 hover:bg-green-50'}`}
                        onClick={(e) => { e.stopPropagation(); onRefresh(); }}
                        disabled={isRefreshing || isDisabled}
                        title={isDisabled ? t('accounts.disabled_tooltip') : t('common.refresh')}
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
                        className={cn(
                            "p-1.5 rounded-lg transition-all",
                            account.proxy_disabled
                                ? "text-gray-400 hover:text-green-600 hover:bg-green-50"
                                : "text-gray-400 hover:text-orange-600 hover:bg-orange-50"
                        )}
                        onClick={(e) => { e.stopPropagation(); onToggleProxy(); }}
                        title={account.proxy_disabled ? t('accounts.enable_proxy') : t('accounts.disable_proxy')}
                    >
                        {account.proxy_disabled ? (
                            <ToggleRight className="w-3.5 h-3.5" />
                        ) : (
                            <ToggleLeft className="w-3.5 h-3.5" />
                        )}
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
