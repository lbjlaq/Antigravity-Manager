// File: src/pages/dashboard/ui/DashboardHeader.tsx
import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { LayoutDashboard, RefreshCw } from 'lucide-react';
import { motion } from 'framer-motion';

import { Button } from '@/shared/ui';
import AddAccountDialog from '@/components/accounts/AddAccountDialog';
import type { Account } from '@/entities/account';
import { itemVariants } from '../lib';

interface DashboardHeaderProps {
  currentAccount: Account | null | undefined;
  isRefreshing: boolean;
  onRefresh: () => void;
  onAddAccount: (email: string, refreshToken: string) => Promise<void>;
}

export const DashboardHeader = memo(function DashboardHeader({
  currentAccount,
  isRefreshing,
  onRefresh,
  onAddAccount,
}: DashboardHeaderProps) {
  const { t } = useTranslation();

  const userName = currentAccount
    ? currentAccount.name || currentAccount.email.split('@')[0]
    : 'User';

  return (
    <motion.div
      variants={itemVariants}
      className="flex flex-col md:flex-row justify-between items-start md:items-end gap-4"
    >
      <div>
        <h1 className="text-2xl font-bold tracking-tight text-gray-900 dark:text-zinc-100 flex items-center gap-2">
          <div className="p-1.5 bg-indigo-600 rounded-lg shadow-lg shadow-indigo-500/30 text-white">
            <LayoutDashboard className="w-5 h-5" />
          </div>
          {t('dashboard.hello', { user: userName })}
        </h1>
      </div>

      <div className="flex items-center gap-2 w-full md:w-auto">
        <Button
          variant="outline"
          size="sm"
          className="gap-2 shadow-sm h-9"
          onClick={onRefresh}
          disabled={isRefreshing || !currentAccount}
        >
          <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin' : ''}`} />
          {isRefreshing ? t('dashboard.refreshing') : t('dashboard.refresh_quota')}
        </Button>
        <AddAccountDialog onAdd={onAddAccount} />
      </div>
    </motion.div>
  );
});
