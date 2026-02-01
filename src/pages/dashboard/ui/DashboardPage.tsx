// File: src/pages/dashboard/ui/DashboardPage.tsx
// Premium Dashboard - Clean & Functional Design

import { memo } from 'react';
import { motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { Users, ArrowRight, Download, RefreshCw, LayoutDashboard } from 'lucide-react';

import { DashboardSkeleton, CurrentAccount, BestAccounts, StatsRow } from '@/widgets/dashboard';
import { AddAccountDialog } from '@/features/accounts';
import { Button } from '@/shared/ui';

import { useDashboard } from '../model';
import { containerVariants, itemVariants } from '../lib';

export const DashboardPage = memo(function DashboardPage() {
    const { t } = useTranslation();
    const {
        accounts,
        currentAccount,
        stats,
        isLoadingAccounts,
        isRefreshing,
        handleSwitch,
        handleAddAccount,
        handleRefreshCurrent,
        handleExport,
        navigateToAccounts,
    } = useDashboard();

    if (isLoadingAccounts && !accounts?.length) {
        return <DashboardSkeleton />;
    }

    if (!accounts) return null;

    const userName = currentAccount
        ? (currentAccount.name || currentAccount.email.split('@')[0])
        : 'User';

    return (
        <div className="h-full w-full overflow-y-auto bg-white dark:bg-zinc-900/40">
            <motion.div
                className="max-w-[1400px] mx-auto p-6 lg:p-8 space-y-6"
                variants={containerVariants}
                initial="hidden"
                animate="visible"
            >
                {/* Header */}
                <motion.div variants={itemVariants} className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
                    <div className="flex items-center gap-3">
                        <div className="p-2.5 rounded-xl bg-indigo-600 shadow-lg shadow-indigo-500/20">
                            <LayoutDashboard className="w-5 h-5 text-white" />
                        </div>
                        <div>
                            <h1 className="text-xl font-bold text-zinc-900 dark:text-white">
                                {t('dashboard.hello', { user: userName })}
                            </h1>
                            <p className="text-sm text-zinc-500">
                                {t('dashboard.subtitle', 'Manage your AI accounts')}
                            </p>
                        </div>
                    </div>

                    <div className="flex items-center gap-2">
                        <Button
                            variant="outline"
                            size="sm"
                            className="h-9 gap-2 border-zinc-300 dark:border-zinc-700"
                            onClick={handleRefreshCurrent}
                            disabled={isRefreshing || !currentAccount}
                        >
                            <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin' : ''}`} />
                            {isRefreshing ? t('dashboard.refreshing') : t('dashboard.refresh_quota')}
                        </Button>
                        <AddAccountDialog onAdd={handleAddAccount} />
                    </div>
                </motion.div>

                {/* Stats Row */}
                <motion.div variants={itemVariants}>
                    <StatsRow stats={stats} />
                </motion.div>

                {/* Main Content Grid */}
                <motion.div variants={itemVariants} className="grid grid-cols-1 lg:grid-cols-12 gap-6">
                    {/* Left Column (7 cols) */}
                    <div className="lg:col-span-7 space-y-4">
                        <CurrentAccount
                            account={currentAccount}
                            onSwitch={navigateToAccounts}
                        />

                        {/* Quick Actions */}
                        <div className="flex items-center justify-between p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900">
                            <div className="flex items-center gap-3">
                                <div className="p-2 rounded-lg bg-indigo-500/10">
                                    <Users className="w-4 h-4 text-indigo-500" />
                                </div>
                                <div className="hidden sm:block">
                                    <p className="text-xs font-bold text-zinc-700 dark:text-zinc-200 uppercase tracking-wide">
                                        {t('dashboard.account_pool', 'Account Pool')}
                                    </p>
                                    <p className="text-[10px] text-zinc-400">
                                        {accounts.length} {t('common.accounts', 'accounts')}
                                    </p>
                                </div>
                            </div>
                            <div className="flex gap-2">
                                <Button
                                    variant="ghost"
                                    size="sm"
                                    onClick={handleExport}
                                    className="h-8 gap-2 text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-200"
                                >
                                    <Download className="w-3.5 h-3.5" />
                                    <span className="hidden sm:inline">{t('dashboard.export_data')}</span>
                                </Button>
                                <Button
                                    size="sm"
                                    onClick={navigateToAccounts}
                                    className="h-8 gap-2 bg-indigo-600 hover:bg-indigo-500"
                                >
                                    {t('dashboard.view_all_accounts')}
                                    <ArrowRight className="w-3.5 h-3.5" />
                                </Button>
                            </div>
                        </div>
                    </div>

                    {/* Right Column (5 cols) */}
                    <div className="lg:col-span-5 min-h-[400px]">
                        <BestAccounts
                            accounts={accounts}
                            currentAccountId={currentAccount?.id}
                            onSwitch={handleSwitch}
                        />
                    </div>
                </motion.div>
            </motion.div>
        </div>
    );
});

export default DashboardPage;
