import { ArrowRightLeft, RefreshCw, Trash2, Download, Info, Lock, Ban } from 'lucide-react';
import { Account } from '../../types/account';
import { getQuotaColor, formatTimeRemaining } from '../../utils/format';

interface AccountRowProps {
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

function AccountRow({ account, selected, onSelect, isCurrent, isRefreshing, isSwitching = false, onSwitch, onRefresh, onViewDetails, onExport, onDelete }: AccountRowProps) {
    const { t } = useTranslation();
    const geminiModel = account.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-pro-high');
    const geminiImageModel = account.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-pro-image');
    const claudeModel = account.quota?.models.find(m => m.name.toLowerCase() === 'claude-sonnet-4-5');

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

    return (
        <tr className={`h-[48px] group hover:bg-gray-50 dark:hover:bg-base-200 transition-colors border-b border-gray-100 dark:border-base-200 ${isCurrent ? 'bg-blue-50/50 dark:bg-blue-900/10' : ''} ${isRefreshing ? 'opacity-70' : ''}`}>
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
            <td className="py-1">
                <div className="flex items-center gap-2">
                    <span className={`font-medium text-sm truncate max-w-[200px] xl:max-w-none ${isCurrent ? 'text-blue-700 dark:text-blue-400' : 'text-gray-900 dark:text-base-content'}`} title={account.email}>
                        {account.email}
                    </span>
                    {isCurrent && (
                        <span className="px-1.5 py-0.5 rounded-full bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 text-[10px] font-semibold whitespace-nowrap">
                            {t('accounts.current')}
                        </span>
                    )}
                    {account.quota?.is_forbidden && (
                        <span className="px-1.5 py-0.5 rounded-full bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400 text-[10px] font-semibold flex items-center gap-1 whitespace-nowrap" title={t('accounts.forbidden_tooltip')}>
                            <Lock className="w-3 h-3" />
                            <span>{t('accounts.forbidden')}</span>
                        </span>
                    )}
                </div>
            </td>

            {/* 模型配额 */}
            <td className="py-1">
                {account.quota?.is_forbidden ? (
                    <div className="flex items-center gap-2 text-xs text-red-500 dark:text-red-400 bg-red-50/50 dark:bg-red-900/10 p-1.5 rounded-lg border border-red-100 dark:border-red-900/30">
                        <Ban className="w-4 h-4 shrink-0" />
                        <span>{t('accounts.forbidden_msg')}</span>
                    </div>
                ) : (
                    <div className="flex flex-col gap-0.5">
                        {/* Gemini */}
                        <div className="flex items-center gap-2">
                            <div className="w-32 text-xs font-medium text-gray-500 dark:text-gray-400">Gemini 3 Pro (High)</div>
                            {geminiModel ? (
                                <>
                                    <div className="w-24 h-1 bg-gray-100 dark:bg-base-300 rounded-full overflow-hidden">
                                        <div
                                            className={`h-full ${getColorClass(geminiModel.percentage)} rounded-full`}
                                            style={{ width: `${geminiModel.percentage}%` }}
                                        />
                                    </div>
                                    <div className="w-8 text-xs text-right text-gray-700 dark:text-gray-300 font-bold font-mono">
                                        {geminiModel.percentage}%
                                    </div>
                                    {geminiModel.reset_time && (
                                        <div className="text-[10px] text-gray-400 dark:text-gray-500 font-mono" title={`${t('accounts.reset_time')}: ${new Date(geminiModel.reset_time).toLocaleString()}`}>
                                            R: {formatTimeRemaining(geminiModel.reset_time)}
                                        </div>
                                    )}
                                </>
                            ) : (
                                <span className="text-xs text-gray-400 dark:text-gray-500 flex-1">无数据</span>
                            )}
                        </div>

                        {/* Gemini Image */}
                        <div className="flex items-center gap-2">
                            <div className="w-32 text-xs font-medium text-gray-500 dark:text-gray-400">Gemini 3 Pro Image</div>
                            {geminiImageModel ? (
                                <>
                                    <div className="w-24 h-1 bg-gray-100 dark:bg-base-300 rounded-full overflow-hidden">
                                        <div
                                            className={`h-full ${getColorClass(geminiImageModel.percentage)} rounded-full`}
                                            style={{ width: `${geminiImageModel.percentage}%` }}
                                        />
                                    </div>
                                    <div className="w-8 text-xs text-right text-gray-700 dark:text-gray-300 font-bold font-mono">
                                        {geminiImageModel.percentage}%
                                    </div>
                                    {geminiImageModel.reset_time && (
                                        <div className="text-[10px] text-gray-400 dark:text-gray-500 font-mono" title={`${t('accounts.reset_time')}: ${new Date(geminiImageModel.reset_time).toLocaleString()}`}>
                                            R: {formatTimeRemaining(geminiImageModel.reset_time)}
                                        </div>
                                    )}
                                </>
                            ) : (
                                <span className="text-xs text-gray-400 dark:text-gray-500 flex-1">无数据</span>
                            )}
                        </div>

                        {/* Claude */}
                        <div className="flex items-center gap-2">
                            <div className="w-32 text-xs font-medium text-gray-500 dark:text-gray-400">Claude-sonnet-4.5</div>
                            {claudeModel ? (
                                <>
                                    <div className="w-24 h-1 bg-gray-100 dark:bg-base-300 rounded-full overflow-hidden">
                                        <div
                                            className={`h-full ${getColorClass(claudeModel.percentage)} rounded-full`}
                                            style={{ width: `${claudeModel.percentage}%` }}
                                        />
                                    </div>
                                    <div className="w-8 text-xs text-right text-gray-700 dark:text-gray-300 font-bold font-mono">
                                        {claudeModel.percentage}%
                                    </div>
                                    {claudeModel.reset_time && (
                                        <div className="text-[10px] text-gray-400 dark:text-gray-500 font-mono" title={`重置时间: ${new Date(claudeModel.reset_time).toLocaleString()}`}>
                                            R: {formatTimeRemaining(claudeModel.reset_time)}
                                        </div>
                                    )}
                                </>
                            ) : (
                                <span className="text-xs text-gray-400 dark:text-gray-500 flex-1">无数据</span>
                            )}
                        </div>
                    </div>
                )}
            </td>

            {/* 最后使用 */}
            <td className="py-1">
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
            <td className="py-1">
                <div className="flex items-center gap-0.5 opacity-60 group-hover:opacity-100 transition-opacity">
                    <button
                        className="p-1.5 text-gray-500 dark:text-gray-400 hover:text-sky-600 dark:hover:text-sky-400 hover:bg-sky-50 dark:hover:bg-sky-900/30 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); onViewDetails(); }}
                        title={t('common.details')}
                    >
                        <Info className="w-3.5 h-3.5" />
                    </button>
                    {!isCurrent && (
                        <button
                            className={`p-1.5 text-gray-500 dark:text-gray-400 rounded-lg transition-all ${isSwitching ? 'bg-blue-50 dark:bg-blue-900/10 text-blue-600 dark:text-blue-400 cursor-not-allowed' : 'hover:text-blue-600 dark:hover:text-blue-400 hover:bg-blue-50 dark:hover:bg-blue-900/30'}`}
                            onClick={(e) => { e.stopPropagation(); onSwitch(); }}
                            title={isSwitching ? t('common.loading') : t('accounts.switch_to')}
                            disabled={isSwitching}
                        >
                            <ArrowRightLeft className={`w-3.5 h-3.5 ${isSwitching ? 'animate-spin' : ''}`} />
                        </button>
                    )}
                    <button
                        className={`p-1.5 text-gray-500 dark:text-gray-400 rounded-lg transition-all ${isRefreshing ? 'bg-green-50 dark:bg-green-900/10 text-green-600 dark:text-green-400 cursor-not-allowed' : 'hover:text-green-600 dark:hover:text-green-400 hover:bg-green-50 dark:hover:bg-green-900/30'}`}
                        onClick={(e) => { e.stopPropagation(); onRefresh(); }}
                        title={isRefreshing ? t('common.refreshing') : t('common.refresh')}
                        disabled={isRefreshing}
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
