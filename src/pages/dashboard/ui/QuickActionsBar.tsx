// File: src/pages/dashboard/ui/QuickActionsBar.tsx
import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Users, ArrowRight, Download } from 'lucide-react';

import { Button, Card } from '@/shared/ui';

interface QuickActionsBarProps {
  onExport: () => void;
  onViewAllAccounts: () => void;
}

export const QuickActionsBar = memo(function QuickActionsBar({
  onExport,
  onViewAllAccounts,
}: QuickActionsBarProps) {
  const { t } = useTranslation();

  return (
    <Card className="flex items-center justify-between p-3 bg-white/50 dark:bg-zinc-900/50 backdrop-blur-sm border-zinc-200 dark:border-white/5">
      <div className="flex items-center gap-3">
        <div className="p-2 bg-indigo-50 dark:bg-indigo-500/10 rounded-md text-indigo-600 dark:text-indigo-400">
          <Users size={16} />
        </div>
        <div className="hidden sm:block">
          <div className="text-xs font-bold text-zinc-900 dark:text-zinc-200 uppercase tracking-wider">
            Account Pool
          </div>
        </div>
      </div>
      <div className="flex gap-2">
        <Button variant="ghost" size="sm" onClick={onExport} className="gap-2 h-8">
          <Download size={14} />
          {t('dashboard.export_data')}
        </Button>
        <Button onClick={onViewAllAccounts} size="sm" className="gap-2 h-8">
          <span>{t('dashboard.view_all_accounts')}</span>
          <ArrowRight size={14} />
        </Button>
      </div>
    </Card>
  );
});
