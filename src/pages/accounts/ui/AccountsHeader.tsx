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
    <div className="flex-none flex items-center justify-between px-5 py-4 border-b border-white/5 bg-gradient-to-r from-zinc-900/80 to-zinc-900/40">
      <div className="flex items-center gap-4">
        <div className="p-2.5 rounded-xl bg-gradient-to-br from-indigo-500 to-purple-600 shadow-lg shadow-indigo-500/20">
          <Users className="w-5 h-5 text-white" />
        </div>
        <div>
          <h1 className="text-xl font-bold text-white tracking-tight">
            {t('nav.accounts')}
          </h1>
          <p className="text-xs text-zinc-500 mt-0.5">
            {accountCount} {t('common.accounts', 'accounts active')}
          </p>
        </div>
      </div>
      <AddAccountDialog onAdd={onAddAccount} />
    </div>
  );
});
