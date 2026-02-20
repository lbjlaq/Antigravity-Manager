import { ArrowRightLeft, RefreshCw, Trash2, Download, Info, Lock, Ban, Diamond, Gem, Circle, Clock, ToggleLeft, ToggleRight, Fingerprint } from 'lucide-react';
import { Account } from '../../types/account';
import { getQuotaColor, formatTimeRemaining, getTimeRemainingColor } from '../../utils/format';
import { cn } from '../../utils/cn';
import { useTranslation } from 'react-i18next';

interface AccountRowProps {
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



function AccountRow({ account, selected, onSelect, isCurrent, isRefreshing, isSwitching = false, onSwitch, onRefresh, onViewDetails, onExport, onDelete, onToggleProxy, onViewDevice }: AccountRowProps) {
    const { t } = useTranslation();
    // [重构] 按组聚合查找逻辑，优先显示组内配额最低的型号以与锁定状态（🔒）对齐
    const geminiProModel = account.quota?.models
        .filter(m =>
            m.name.toLowerCase() === 'gemini-3-pro-high'
            || m.name.toLowerCase() === 'gemini-3-pro-low'
            || m.name.toLowerCase() === 'gemini-3.1-pro-high'
            || m.name.toLowerCase() === 'gemini-3.1-pro-low'
        )
        .sort((a, b) => (a.percentage || 0) - (b.percentage || 0))[0];

    const geminiFlashModel = account.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-flash');

    const geminiImageModel = account.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-pro-image');

    const claudeGroupNames = [
        'claude-opus-4-6-thinking',
        'claude'
    ];
    const claudeModel = account.quota?.models
        .filter(m => claudeGroupNames.includes(m.name.toLowerCase()))
        .sort((a, b) => (a.percentage || 0) - (b.percentage || 0))[0];
    const isDisabled = Boolean(account.disabled);

    // 颜色映射，避免动态类名被 Tailwind purge
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
            default: return 'text-blue-600 dark:text-blue-400';
        }
    };

    return (
        <tr className={cn(
            "group hover:bg-gray-50 dark:hover:bg-base-200 transition-colors border-b border-gray-100 dark:border-base-200",
            isCurrent && "bg-blue-50/50 dark:bg-blue-900/10",
            (isRefreshing || isDisabled) && "opacity-70"
        )}>
            {/* 序号 */}
            <td className="pl-6 py-1 w-12">
                <input
                    type="checkbox"
                    className="checkbox checkbox-xs rounded border-2 border-gray-400 dark:border-gray-500 checked:border-blue-600 checked:bg-blue-600 [--chkbg:theme(colors.blue.600)] [--chkfg:white]"
                    checked={selected}
                    onChange={() => onSelect()}
                    onClick={(e) => e.stopPropagation()}
                />
            </td>

            {/* 邮箱 */}
            <td className="px-4 py-1">
                <div className="flex items-center gap-3">
                    <span className={cn(
                        "font-medium text-sm truncate max-w-[180px] xl:max-w-none transition-colors",
                        isCurrent ? "text-blue-700 dark:text-blue-400" : "text-gray-900 dark:text-base-content"
                    )} title={account.email}>
                        {account.email}
                    </span>

                    <div className="flex items-center gap-1.5 shrink-0">
                        {isCurrent && (
                            <span className="px-2 py-0.5 rounded-md bg-blue-100 dark:bg-blue-900/50 text-blue-700 dark:text-blue-300 text-[10px] font-bold shadow-sm border border-blue-200/50 dark:border-blue-800/50">
                                {t('accounts.current').toUpperCase()}
                            </span>
                        )}

                        {isDisabled && (
                            <span
                                className="px-2 py-0.5 rounded-md bg-rose-100 dark:bg-rose-900/50 text-rose-700 dark:text-rose-300 text-[10px] font-bold flex items-center gap-1 shadow-sm border border-rose-200/50"
                                title={account.disabled_reason || t('accounts.disabled_tooltip')}
                            >
                                <Ban className="w-2.5 h-2.5" />
                                <span>{t('accounts.disabled')}</span>
                            </span>
                        )}

                        {account.proxy_disabled && (
                            <span
                                className="px-2 py-0.5 rounded-md bg-orange-100 dark:bg-orange-900/50 text-orange-700 dark:text-orange-300 text-[10px] font-bold flex items-center gap-1 shadow-sm border border-orange-200/50"
                                title={account.proxy_disabled_reason || t('accounts.proxy_disabled_tooltip')}
                            >
                                <Ban className="w-2.5 h-2.5" />
                                <span>{t('accounts.proxy_disabled')}</span>
                            </span>
                        )}

                        {account.quota?.is_forbidden && (
                            <span className="px-2 py-0.5 rounded-md bg-red-100 dark:bg-red-900/50 text-red-600 dark:text-red-400 text-[10px] font-bold flex items-center gap-1 shadow-sm border border-red-200/50" title={t('accounts.forbidden_tooltip')}>
                                <Lock className="w-2.5 h-2.5" />
                                <span>{t('accounts.forbidden')}</span>
                            </span>
                        )}

                        {/* 订阅类型徽章 */}
                        {account.quota?.subscription_tier && (() => {
                            const tier = account.quota.subscription_tier.toLowerCase();
                            if (tier.includes('ultra')) {
                                return (
                                    <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-gradient-to-r from-purple-600 to-pink-600 text-white text-[10px] font-bold shadow-sm hover:scale-105 transition-transform cursor-default">
                                        <Gem className="w-2.5 h-2.5 fill-current" />
                                        ULTRA
                                    </span>
                                );
                            } else if (tier.includes('pro')) {
                                return (
                                    <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-gradient-to-r from-blue-600 to-indigo-600 text-white text-[10px] font-bold shadow-sm hover:scale-105 transition-transform cursor-default">
                                        <Diamond className="w-2.5 h-2.5 fill-current" />
                                        PRO
                                    </span>
                                );
                            } else {
                                return (
                                    <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-gray-100 dark:bg-white/10 text-gray-600 dark:text-gray-400 text-[10px] font-bold shadow-sm border border-gray-200 dark:border-white/10 hover:bg-gray-200 transition-colors cursor-default">
                                        <Circle className="w-2.5 h-2.5" />
                                        FREE
                                    </span>
                                );
                            }
                        })()}
                    </div>
                </div>
            </td>

            {/* 模型配额 */}
            <td className="px-4 py-1">
                {account.quota?.is_forbidden ? (
                    <div className="flex items-center gap-2 text-xs text-red-500 dark:text-red-400 bg-red-50/50 dark:bg-red-900/10 p-1.5 rounded-lg border border-red-100 dark:border-red-900/30">
                        <Ban className="w-4 h-4 shrink-0" />
                        <span>{t('accounts.forbidden_msg')}</span>
                    </div>
                ) : (
                    <div className="grid grid-cols-2 gap-x-4 gap-y-1 py-0">
                        {/* Gemini Pro */}
                        <div className="relative h-[22px] flex items-center px-1.5 rounded-md overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5 group/quota">
                            {geminiProModel && (
                                <div
                                    className={`absolute inset-y-0 left-0 transition-all duration-700 ease-out opacity-15 dark:opacity-20 ${getColorClass(geminiProModel.percentage)}`}
                                    style={{ width: `${geminiProModel.percentage}%` }}
                                />
                            )}
                            <div className="relative z-10 w-full flex items-center text-[10px] font-mono leading-none">
                                <span className="w-[64px] text-gray-500 dark:text-gray-400 font-bold pr-1 flex items-center gap-1" title="Gemini 3.1 Pro">
                                    {(account.protected_models?.includes('gemini-3-pro-high') || account.protected_models?.includes('gemini-3.1-pro-high')) && <Lock className="w-2.5 h-2.5 text-rose-500 shrink-0 z-10" />}
                                    <span className="truncate">G3.1 Pro</span>
                                </span>
                                <div className="flex-1 flex justify-center">
                                    {geminiProModel?.reset_time ? (
                                        <span className={cn("flex items-center gap-0.5 font-medium transition-colors", getTimeColorClass(geminiProModel.reset_time))}>
                                            <Clock className="w-2.5 h-2.5" />
                                            {formatTimeRemaining(geminiProModel.reset_time)}
                                        </span>
                                    ) : (
                                        <span className="text-gray-300 dark:text-gray-600 italic scale-90">N/A</span>
                                    )}
                                </div>
                                <span className={cn("w-[36px] text-right font-bold transition-colors",
                                    getQuotaColor(geminiProModel?.percentage || 0) === 'success' ? 'text-emerald-600 dark:text-emerald-400' :
                                        getQuotaColor(geminiProModel?.percentage || 0) === 'warning' ? 'text-amber-600 dark:text-amber-400' : 'text-rose-600 dark:text-rose-400'
                                )}>
                                    {geminiProModel ? `${geminiProModel.percentage}%` : '-'}
                                </span>
                            </div>
                        </div>

                        {/* Gemini Flash */}
                        <div className="relative h-[22px] flex items-center px-1.5 rounded-md overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5 group/quota">
                            {geminiFlashModel && (
                                <div
                                    className={`absolute inset-y-0 left-0 transition-all duration-700 ease-out opacity-15 dark:opacity-20 ${getColorClass(geminiFlashModel.percentage)}`}
                                    style={{ width: `${geminiFlashModel.percentage}%` }}
                                />
                            )}
                            <div className="relative z-10 w-full flex items-center text-[10px] font-mono leading-none">
                                <span className="w-[64px] text-gray-500 dark:text-gray-400 font-bold pr-1 flex items-center gap-1" title="Gemini 3 Flash">
                                    {account.protected_models?.includes('gemini-3-flash') && <Lock className="w-2.5 h-2.5 text-rose-500 shrink-0 z-10" />}
                                    <span className="truncate">G3 Flash</span>
                                </span>
                                <div className="flex-1 flex justify-center">
                                    {geminiFlashModel?.reset_time ? (
                                        <span className={cn("flex items-center gap-0.5 font-medium transition-colors", getTimeColorClass(geminiFlashModel.reset_time))}>
                                            <Clock className="w-2.5 h-2.5" />
                                            {formatTimeRemaining(geminiFlashModel.reset_time)}
                                        </span>
                                    ) : (
                                        <span className="text-gray-300 dark:text-gray-600 italic scale-90">N/A</span>
                                    )}
                                </div>
                                <span className={cn("w-[36px] text-right font-bold transition-colors",
                                    getQuotaColor(geminiFlashModel?.percentage || 0) === 'success' ? 'text-emerald-600 dark:text-emerald-400' :
                                        getQuotaColor(geminiFlashModel?.percentage || 0) === 'warning' ? 'text-amber-600 dark:text-amber-400' : 'text-rose-600 dark:text-rose-400'
                                )}>
                                    {geminiFlashModel ? `${geminiFlashModel.percentage}%` : '-'}
                                </span>
                            </div>
                        </div>

                        {/* Gemini Image */}
                        <div className="relative h-[22px] flex items-center px-1.5 rounded-md overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5 group/quota">
                            {geminiImageModel && (
                                <div
                                    className={`absolute inset-y-0 left-0 transition-all duration-700 ease-out opacity-15 dark:opacity-20 ${getColorClass(geminiImageModel.percentage)}`}
                                    style={{ width: `${geminiImageModel.percentage}%` }}
                                />
                            )}
                            <div className="relative z-10 w-full flex items-center text-[10px] font-mono leading-none">
                                <span className="w-[64px] text-gray-500 dark:text-gray-400 font-bold pr-1 flex items-center gap-1" title="Gemini 3 Pro Image">
                                    {account.protected_models?.includes('gemini-3-pro-image') && <Lock className="w-2.5 h-2.5 text-rose-500 shrink-0 z-10" />}
                                    <span className="truncate">G3 Image</span>
                                </span>
                                <div className="flex-1 flex justify-center">
                                    {geminiImageModel?.reset_time ? (
                                        <span className={cn("flex items-center gap-0.5 font-medium transition-colors", getTimeColorClass(geminiImageModel.reset_time))}>
                                            <Clock className="w-2.5 h-2.5" />
                                            {formatTimeRemaining(geminiImageModel.reset_time)}
                                        </span>
                                    ) : (
                                        <span className="text-gray-300 dark:text-gray-600 italic scale-90">N/A</span>
                                    )}
                                </div>
                                <span className={cn("w-[36px] text-right font-bold transition-colors",
                                    getQuotaColor(geminiImageModel?.percentage || 0) === 'success' ? 'text-emerald-600 dark:text-emerald-400' :
                                        getQuotaColor(geminiImageModel?.percentage || 0) === 'warning' ? 'text-amber-600 dark:text-amber-400' : 'text-rose-600 dark:text-rose-400'
                                )}>
                                    {geminiImageModel ? `${geminiImageModel.percentage}%` : '-'}
                                </span>
                            </div>
                        </div>

                        {/* Claude */}
                        <div className="relative h-[22px] flex items-center px-1.5 rounded-md overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5 group/quota">
                            {claudeModel && (
                                <div
                                    className={`absolute inset-y-0 left-0 transition-all duration-700 ease-out opacity-15 dark:opacity-20 ${getColorClass(claudeModel.percentage)}`}
                                    style={{ width: `${claudeModel.percentage}%` }}
                                />
                            )}
                            <div className="relative z-10 w-full flex items-center text-[10px] font-mono leading-none">
                                <span className="w-[64px] text-gray-500 dark:text-gray-400 font-bold pr-1 flex items-center gap-1" title="Claude Series">
                                    {account.protected_models?.includes('claude') && <Lock className="w-2.5 h-2.5 text-rose-500 shrink-0 z-10" />}
                                    <span className="truncate">Claude</span>
                                </span>
                                <div className="flex-1 flex justify-center">
                                    {claudeModel?.reset_time ? (
                                        <span className={cn("flex items-center gap-0.5 font-medium transition-colors", getTimeColorClass(claudeModel.reset_time))}>
                                            <Clock className="w-2.5 h-2.5" />
                                            {formatTimeRemaining(claudeModel.reset_time)}
                                        </span>
                                    ) : (
                                        <span className="text-gray-300 dark:text-gray-600 italic scale-90">N/A</span>
                                    )}
                                </div>
                                <span className={cn("w-[36px] text-right font-bold transition-colors",
                                    getQuotaColor(claudeModel?.percentage || 0) === 'success' ? 'text-emerald-600 dark:text-emerald-400' :
                                        getQuotaColor(claudeModel?.percentage || 0) === 'warning' ? 'text-amber-600 dark:text-amber-400' : 'text-rose-600 dark:text-rose-400'
                                )}>
                                    {claudeModel ? `${claudeModel.percentage}%` : '-'}
                                </span>
                            </div>
                        </div>
                    </div>
                )}
            </td>

            {/* 最后使用 */}
            <td className="px-4 py-1">
                <div className="flex flex-col">
                    <span className="text-xs font-medium text-gray-600 dark:text-gray-400 font-mono whitespace-nowrap">
                        {new Date(account.last_used * 1000).toLocaleDateString()}
                    </span>
                    <span className="text-[10px] text-gray-400 dark:text-gray-500 font-mono whitespace-nowrap leading-tight">
                        {new Date(account.last_used * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                    </span>
                </div>
            </td>

            {/* 操作 */}
            <td className="px-4 py-1">
                <div className="flex items-center gap-0.5 opacity-60 group-hover:opacity-100 transition-opacity">
                    <button
                        className="p-1.5 text-gray-500 dark:text-gray-400 hover:text-sky-600 dark:hover:text-sky-400 hover:bg-sky-50 dark:hover:bg-sky-900/30 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); onViewDetails(); }}
                        title={t('common.details')}
                    >
                        <Info className="w-3.5 h-3.5" />
                        <button
                            className="p-1.5 text-gray-500 dark:text-gray-400 hover:text-indigo-600 dark:hover:text-indigo-400 hover:bg-indigo-50 dark:hover:bg-indigo-900/30 rounded-lg transition-all"
                            onClick={(e) => { e.stopPropagation(); onViewDevice(); }}
                            title={t('accounts.device_fingerprint')}
                        >
                            <Fingerprint className="w-3.5 h-3.5" />
                        </button>
                    </button>
                    <button
                        className={`p-1.5 text-gray-500 dark:text-gray-400 rounded-lg transition-all ${(isSwitching || isDisabled) ? 'bg-blue-50 dark:bg-blue-900/10 text-blue-600 dark:text-blue-400 cursor-not-allowed' : 'hover:text-blue-600 dark:hover:text-blue-400 hover:bg-blue-50 dark:hover:bg-blue-900/30'}`}
                        onClick={(e) => { e.stopPropagation(); onSwitch(); }}
                        title={isDisabled ? t('accounts.disabled_tooltip') : (isSwitching ? t('common.loading') : t('accounts.switch_to'))}
                        disabled={isSwitching || isDisabled}
                    >
                        <ArrowRightLeft className={`w-3.5 h-3.5 ${isSwitching ? 'animate-spin' : ''}`} />
                    </button>
                    <button
                        className={`p-1.5 text-gray-500 dark:text-gray-400 rounded-lg transition-all ${(isRefreshing || isDisabled) ? 'bg-green-50 dark:bg-green-900/10 text-green-600 dark:text-green-400 cursor-not-allowed' : 'hover:text-green-600 dark:hover:text-green-400 hover:bg-green-50 dark:hover:bg-green-900/30'}`}
                        onClick={(e) => { e.stopPropagation(); onRefresh(); }}
                        title={isDisabled ? t('accounts.disabled_tooltip') : (isRefreshing ? t('common.refreshing') : t('common.refresh'))}
                        disabled={isRefreshing || isDisabled}
                    >
                        <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin' : ''}`} />
                    </button>
                    <button
                        className="p-1.5 text-gray-500 dark:text-gray-400 hover:text-indigo-600 dark:hover:text-indigo-400 hover:bg-indigo-50 dark:hover:bg-indigo-900/30 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); onExport(); }}
                        title={t('common.export')}
                    >
                        <Download className="w-3.5 h-3.5" />
                    </button>
                    <button
                        className={cn(
                            "p-1.5 rounded-lg transition-all",
                            account.proxy_disabled
                                ? "text-gray-500 dark:text-gray-400 hover:text-green-600 dark:hover:text-green-400 hover:bg-green-50 dark:hover:bg-green-900/30"
                                : "text-gray-500 dark:text-gray-400 hover:text-orange-600 dark:hover:text-orange-400 hover:bg-orange-50 dark:hover:bg-orange-900/30"
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
                        className="p-1.5 text-gray-500 dark:text-gray-400 hover:text-red-600 dark:hover:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); onDelete(); }}
                        title={t('common.delete')}
                    >
                        <Trash2 className="w-3.5 h-3.5" />
                    </button>
                </div>
            </td>
        </tr>
    );
}

export default AccountRow;
