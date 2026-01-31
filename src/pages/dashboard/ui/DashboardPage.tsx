// File: src/pages/dashboard/ui/DashboardPage.tsx
import { memo } from 'react';
import { motion } from 'framer-motion';

import { DashboardSkeleton } from '@/components/dashboard/DashboardSkeleton';
import CurrentAccount from '@/components/dashboard/CurrentAccount';
import BestAccounts from '@/components/dashboard/BestAccounts';
import { StatsRow } from '@/components/dashboard/StatsRow';

import { useDashboard } from '../model';
import { containerVariants, itemVariants } from '../lib';
import { DashboardHeader } from './DashboardHeader';
import { QuickActionsBar } from './QuickActionsBar';

export const DashboardPage = memo(function DashboardPage() {
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

  return (
    <motion.div
      className="h-full w-full overflow-y-auto bg-white dark:bg-zinc-900/40 backdrop-blur-xl p-8 space-y-8 pb-32"
      variants={containerVariants}
      initial="hidden"
      animate="visible"
    >
      <div className="max-w-[1400px] mx-auto space-y-10">
        {/* Header Section */}
        <DashboardHeader
          currentAccount={currentAccount}
          isRefreshing={isRefreshing}
          onRefresh={handleRefreshCurrent}
          onAddAccount={handleAddAccount}
        />

        {/* Stats Row */}
        <motion.div variants={itemVariants}>
          <StatsRow stats={stats} />
        </motion.div>

        {/* Main Content Grid */}
        <motion.div variants={itemVariants} className="grid grid-cols-1 lg:grid-cols-12 gap-6">
          {/* Left Column: Current Account (7 cols) */}
          <div className="lg:col-span-7 flex flex-col gap-4">
            <CurrentAccount
              account={currentAccount}
              onSwitch={navigateToAccounts}
            />
            <QuickActionsBar
              onExport={handleExport}
              onViewAllAccounts={navigateToAccounts}
            />
          </div>

          {/* Right Column: Best Accounts (5 cols) */}
          <div className="lg:col-span-5 h-full min-h-[400px]">
            <BestAccounts
              accounts={accounts}
              currentAccountId={currentAccount?.id}
              onSwitch={handleSwitch}
            />
          </div>
        </motion.div>
      </div>
    </motion.div>
  );
});

export default DashboardPage;
