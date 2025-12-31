import { useEffect, useMemo, useState, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { Users, Sparkles, Bot, AlertTriangle, ArrowRight, Download, RefreshCw } from 'lucide-react';
import { useAccountStore } from '../stores/useAccountStore';
import CurrentAccount from '../components/dashboard/CurrentAccount';
import BestAccounts from '../components/dashboard/BestAccounts';
import AddAccountDialog from '../components/accounts/AddAccountDialog';
import { save } from '@tauri-apps/plugin-dialog';
import { request as invoke } from '../utils/request';
import { showToast } from '../components/common/ToastContainer';
import { Account } from '../types/account';

function Dashboard() {
    const { t } = useTranslation();
    const navigate = useNavigate();
    const {
        accounts,
        currentAccount,
        fetchAccounts,
        fetchCurrentAccount,
        switchAccount,
        addAccount,
        refreshQuota,
        loading
    } = useAccountStore();

    useEffect(() => {
        fetchAccounts();
        fetchCurrentAccount();
    }, []);

    // 计算统计数据
    const stats = useMemo(() => {
        const geminiQuotas = accounts
            .map(a => a.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-pro-high')?.percentage || 0)
            .filter(q => q > 0);

        const geminiImageQuotas = accounts
            .map(a => a.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-pro-image')?.percentage || 0)
            .filter(q => q > 0);

        const claudeQuotas = accounts
            .map(a => a.quota?.models.find(m => m.name.toLowerCase() === 'claude-sonnet-4-5')?.percentage || 0)
            .filter(q => q > 0);

        const lowQuotaCount = accounts.filter(a => {
            const gemini = a.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-pro-high')?.percentage || 0;
            const claude = a.quota?.models.find(m => m.name.toLowerCase() === 'claude-sonnet-4-5')?.percentage || 0;
            return gemini < 20 || claude < 20;
        }).length;

        return {
            total: accounts.length,
            avgGemini: geminiQuotas.length > 0
                ? Math.round(geminiQuotas.reduce((a, b) => a + b, 0) / geminiQuotas.length)
                : 0,
            avgGeminiImage: geminiImageQuotas.length > 0
                ? Math.round(geminiImageQuotas.reduce((a, b) => a + b, 0) / geminiImageQuotas.length)
                : 0,
            avgClaude: claudeQuotas.length > 0
                ? Math.round(claudeQuotas.reduce((a, b) => a + b, 0) / claudeQuotas.length)
                : 0,
            lowQuota: lowQuotaCount,
        };
    }, [accounts]);

    const isSwitchingRef = useRef(false);

    const handleSwitch = async (accountId: string) => {
        if (loading || isSwitchingRef.current) return;

        isSwitchingRef.current = true;
        console.log('[Dashboard] handleSwitch called for', accountId);
        try {
            await switchAccount(accountId);
            showToast(t('dashboard.toast.switch_success'), 'success');
        } catch (error) {
            console.error('切换账号失败:', error);
            showToast(`${t('dashboard.toast.switch_error')}: ${error}`, 'error');
        } finally {
            setTimeout(() => {
                isSwitchingRef.current = false;
            }, 1000);
        }
    };

    const handleAddAccount = async (email: string, refreshToken: string) => {
        await addAccount(email, refreshToken);
        await fetchAccounts(); // 刷新列表
    };

    const [isRefreshing, setIsRefreshing] = useState(false);

    const handleRefreshCurrent = async () => {
        if (!currentAccount) return;

        setIsRefreshing(true);
        try {
            await refreshQuota(currentAccount.id);
            // 刷新成功后重新获取最新数据
            await fetchCurrentAccount();
            showToast(t('dashboard.toast.refresh_success'), 'success');
        } catch (error) {
            console.error('[Dashboard] Refresh failed:', error);
            showToast(`${t('dashboard.toast.refresh_error')}: ${error}`, 'error');
        } finally {
            setIsRefreshing(false);
        }
    };

    const exportAccountsToJson = async (accountsToExport: Account[]) => {
        try {
            if (accountsToExport.length === 0) {
                showToast(t('dashboard.toast.export_no_accounts'), 'warning');
                return;
            }

            const path = await save({
                filters: [{
                    name: 'JSON',
                    extensions: ['json']
                }],
                defaultPath: `antigravity_accounts_${new Date().toISOString().split('T')[0]}.json`
            });

            if (!path) return;

            const exportData = accountsToExport.map(acc => ({
                email: acc.email,
                refresh_token: acc.token.refresh_token
            }));

            const content = JSON.stringify(exportData, null, 2);

            await invoke('save_text_file', { path, content });

            showToast(t('dashboard.toast.export_success', { path }), 'success');
        } catch (error) {
            console.error('Export failed:', error);
            showToast(`${t('dashboard.toast.export_error')}: ${error}`, 'error');
        }
    };

    const handleExport = () => {
        exportAccountsToJson(accounts);
    };

    return (
        <div className="h-full w-full overflow-y-auto">
            <div
                className="p-5 space-y-4 max-w-7xl mx-auto"
                onMouseMove={() => console.log('Mouse moving over Dashboard')}
                style={{ position: 'relative', zIndex: 1 }}
            >
                {/* 问候语和操作按钮 */}
                <div
                    className="flex justify-between items-center"
                >
                    <div>
                        <h1 className="text-2xl font-bold text-gray-900 dark:text-base-content">
                            {currentAccount
                                ? t('dashboard.hello').replace('用户', currentAccount.name || currentAccount.email.split('@')[0])
                                : t('dashboard.hello')
                            }
                        </h1>
                    </div>
                    <div className="flex gap-2">
                        <AddAccountDialog onAdd={handleAddAccount} />
                        <button
                            className={`px-3 py-1.5 bg-blue-500 text-white text-xs font-medium rounded-lg hover:bg-blue-600 transition-colors flex items-center gap-1.5 shadow-sm ${isRefreshing || !currentAccount ? 'opacity-70 cursor-not-allowed' : ''}`}
                            onClick={handleRefreshCurrent}
                            disabled={isRefreshing || !currentAccount}
                        >
                            <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin' : ''}`} />
                            {isRefreshing ? t('dashboard.refreshing') : t('dashboard.refresh_quota')}
                        </button>
                    </div>
                </div>

                {/* 统计卡片 - 5 columns on medium screens and up */}
                <div className="grid grid-cols-2 md:grid-cols-5 gap-3">
                    <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
                        <div className="flex items-center justify-between mb-2">
                            <div className="p-1.5 bg-blue-50 dark:bg-blue-900/20 rounded-md">
                                <Users className="w-4 h-4 text-blue-500 dark:text-blue-400" />
                            </div>
                        </div>
                        <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{stats.total}</div>
                        <div className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.total_accounts')}</div>
                    </div>

                    <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
                        <div className="flex items-center justify-between mb-2">
                            <div className="p-1.5 bg-green-50 dark:bg-green-900/20 rounded-md">
                                <Sparkles className="w-4 h-4 text-green-500 dark:text-green-400" />
                            </div>
                        </div>
                        <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{stats.avgGemini}%</div>
                        <div className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.avg_gemini')}</div>
                        {stats.avgGemini > 0 && (
                            <div className={`text-[10px] mt-1 ${stats.avgGemini >= 50 ? 'text-green-600 dark:text-green-400' : 'text-orange-600 dark:text-orange-400'}`}>
                                {stats.avgGemini >= 50 ? t('dashboard.quota_sufficient') : t('dashboard.quota_low')}
                            </div>
                        )}
                    </div>

                    <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
                        <div className="flex items-center justify-between mb-2">
                            <div className="p-1.5 bg-purple-50 dark:bg-purple-900/20 rounded-md">
                                <Sparkles className="w-4 h-4 text-purple-500 dark:text-purple-400" />
                            </div>
                        </div>
                        <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{stats.avgGeminiImage}%</div>
                        <div className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.avg_gemini_image')}</div>
                        {stats.avgGeminiImage > 0 && (
                            <div className={`text-[10px] mt-1 ${stats.avgGeminiImage >= 50 ? 'text-green-600 dark:text-green-400' : 'text-orange-600 dark:text-orange-400'}`}>
                                {stats.avgGeminiImage >= 50 ? t('dashboard.quota_sufficient') : t('dashboard.quota_low')}
                            </div>
                        )}
                    </div>

                    <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
                        <div className="flex items-center justify-between mb-2">
                            <div className="p-1.5 bg-cyan-50 dark:bg-cyan-900/20 rounded-md">
                                <Bot className="w-4 h-4 text-cyan-500 dark:text-cyan-400" />
                            </div>
                        </div>
                        <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{stats.avgClaude}%</div>
                        <div className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.avg_claude')}</div>
                        {stats.avgClaude > 0 && (
                            <div className={`text-[10px] mt-1 ${stats.avgClaude >= 50 ? 'text-green-600 dark:text-green-400' : 'text-orange-600 dark:text-orange-400'}`}>
                                {stats.avgClaude >= 50 ? t('dashboard.quota_sufficient') : t('dashboard.quota_low')}
                            </div>
                        )}
                    </div>

                    <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
                        <div className="flex items-center justify-between mb-2">
                            <div className="p-1.5 bg-orange-50 dark:bg-orange-900/20 rounded-md">
                                <AlertTriangle className="w-4 h-4 text-orange-500 dark:text-orange-400" />
                            </div>
                        </div>
                        <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{stats.lowQuota}</div>
                        <div className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.low_quota_accounts')}</div>
                        <div className="text-[10px] text-gray-400 dark:text-gray-500 mt-1">{t('dashboard.quota_desc')}</div>
                    </div>
                </div>

                {/* 双栏布局 */}
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <CurrentAccount
                        account={currentAccount}
                        onSwitch={() => navigate('/accounts')}
                    />
                    <BestAccounts
                        accounts={accounts}
                        currentAccountId={currentAccount?.id}
                        onSwitch={handleSwitch}
                    />
                </div>

                {/* 快速链接 */}
                <div className="grid grid-cols-2 gap-3">
                    <button
                        className="bg-indigo-50 dark:bg-indigo-900/20 rounded-lg p-3 shadow-sm border border-indigo-100 dark:border-indigo-900/30 hover:border-indigo-300 dark:hover:border-indigo-700 hover:shadow-md transition-all flex items-center justify-between group"
                        onClick={() => navigate('/accounts')}
                    >
                        <span className="text-indigo-700 dark:text-indigo-300 font-medium text-sm">{t('dashboard.view_all_accounts')}</span>
                        <ArrowRight className="w-4 h-4 text-indigo-400 dark:text-indigo-500 group-hover:text-indigo-600 dark:group-hover:text-indigo-300 group-hover:translate-x-1 transition-all" />
                    </button>
                    <button
                        className="bg-purple-50 dark:bg-purple-900/20 rounded-lg p-3 shadow-sm border border-purple-100 dark:border-purple-900/30 hover:border-purple-300 dark:hover:border-purple-700 hover:shadow-md transition-all flex items-center justify-between group"
                        onClick={handleExport}
                    >
                        <span className="text-purple-700 dark:text-purple-300 font-medium text-sm">{t('dashboard.export_data')}</span>
                        <Download className="w-4 h-4 text-purple-400 dark:text-purple-500 group-hover:text-purple-600 dark:group-hover:text-purple-300 transition-all" />
                    </button>
                </div>
            </div>
        </div>
    );
}

export default Dashboard;
