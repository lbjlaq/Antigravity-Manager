import { CheckCircle, Mail } from 'lucide-react';
import { Account } from '../../types/account';
import { formatTimeRemaining } from '../../utils/format';

interface CurrentAccountProps {
    account: Account | null;
    onSwitch?: () => void;
}

import { useTranslation } from 'react-i18next';

function CurrentAccount({ account, onSwitch }: CurrentAccountProps) {
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

    const geminiModel = account.quota?.models.find(m => m.name.toLowerCase().includes('gemini'));
    const claudeModel = account.quota?.models.find(m => m.name.toLowerCase().includes('claude'));

    return (
        <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200 h-full flex flex-col">
            <h2 className="text-base font-semibold text-gray-900 dark:text-base-content mb-3 flex items-center gap-2">
                <CheckCircle className="w-4 h-4 text-green-500" />
                {t('dashboard.current_account')}
            </h2>

            <div className="space-y-3 flex-1">
                <div className="flex items-center gap-2 mb-2">
                    <Mail className="w-3.5 h-3.5 text-gray-400" />
                    <span className="text-sm font-medium text-gray-700 dark:text-gray-300 truncate">{account.email}</span>
                </div>

                {/* Gemini 配额 */}
                {geminiModel && (
                    <div className="space-y-1">
                        <div className="flex justify-between items-baseline">
                            <span className="text-xs text-gray-600 dark:text-gray-400">Gemini 3 Pro</span>
                            <div className="flex items-center gap-2">
                                <span className="text-[10px] text-gray-400 dark:text-gray-500" title={`${t('accounts.reset_time')}: ${new Date(geminiModel.reset_time).toLocaleString()}`}>
                                    {geminiModel.reset_time ? `R: ${formatTimeRemaining(geminiModel.reset_time)}` : t('common.unknown')}
                                </span>
                                <span className={`text-xs font-semibold ${geminiModel.percentage >= 50 ? 'text-green-600 dark:text-green-400' :
                                    geminiModel.percentage >= 20 ? 'text-orange-600 dark:text-orange-400' : 'text-red-600 dark:text-red-400'
                                    }`}>
                                    {geminiModel.percentage}%
                                </span>
                            </div>
                        </div>
                        <div className="w-full bg-gray-100 dark:bg-base-300 rounded-full h-1.5 overflow-hidden">
                            <div
                                className={`h-full rounded-full transition-all ${geminiModel.percentage >= 50 ? 'bg-gradient-to-r from-green-400 to-green-500' :
                                    geminiModel.percentage >= 20 ? 'bg-gradient-to-r from-orange-400 to-orange-500' :
                                        'bg-gradient-to-r from-red-400 to-red-500'
                                    }`}
                                style={{ width: `${geminiModel.percentage}%` }}
                            ></div>
                        </div>
                    </div>
                )}

                {/* Claude 配额 */}
                {claudeModel && (
                    <div className="space-y-1">
                        <div className="flex justify-between items-baseline">
                            <span className="text-xs text-gray-600 dark:text-gray-400">Claude 4.5</span>
                            <div className="flex items-center gap-2">
                                <span className="text-[10px] text-gray-400 dark:text-gray-500" title={`${t('accounts.reset_time')}: ${new Date(claudeModel.reset_time).toLocaleString()}`}>
                                    {claudeModel.reset_time ? `R: ${formatTimeRemaining(claudeModel.reset_time)}` : t('common.unknown')}
                                </span>
                                <span className={`text-xs font-semibold ${claudeModel.percentage >= 50 ? 'text-cyan-600 dark:text-cyan-400' :
                                    claudeModel.percentage >= 20 ? 'text-orange-600 dark:text-orange-400' : 'text-red-600 dark:text-red-400'
                                    }`}>
                                    {claudeModel.percentage}%
                                </span>
                            </div>
                        </div>
                        <div className="w-full bg-gray-100 dark:bg-base-300 rounded-full h-1.5 overflow-hidden">
                            <div
                                className={`h-full rounded-full transition-all ${claudeModel.percentage >= 50 ? 'bg-gradient-to-r from-cyan-400 to-cyan-500' :
                                    claudeModel.percentage >= 20 ? 'bg-gradient-to-r from-orange-400 to-orange-500' :
                                        'bg-gradient-to-r from-red-400 to-red-500'
                                    }`}
                                style={{ width: `${claudeModel.percentage}%` }}
                            ></div>
                        </div>
                    </div>
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
