// File: src/pages/accounts/ui/AccountsHeader.tsx
import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Users } from 'lucide-react';

import AddAccountDialog from '@/components/accounts/AddAccountDialog';

interface AccountsHeaderProps {
  accountCount: number;
  onAddAccount: (email: string, refreshToken: string) => Promise<void>;
}

export const AccountsHeader = memo(function AccountsHeader({
  accountCount,
  onAddAccount,
}: AccountsHeaderProps) {
  const { t } = useTranslation();

  return (
    <div className="flex-none flex items-center justify-between px-5 py-4 border-b border-zinc-200 dark:border-zinc-800">
      <div className="flex items-center gap-3">
        <div className="p-2 rounded-lg bg-zinc-100 dark:bg-zinc-800">
          <Users className="w-5 h-5 text-zinc-600 dark:text-zinc-400" />
        </div>
        <div>
          <h1 className="text-lg font-semibold text-zinc-900 dark:text-white">
            {t('nav.accounts')}
          </h1>
          <p className="text-xs text-zinc-500 dark:text-zinc-500">
            {accountCount} {t('common.accounts', 'accounts active')}
          </p>
        </div>
      </div>
      <AddAccountDialog onAdd={onAddAccount} />
    </div>
  );
});
