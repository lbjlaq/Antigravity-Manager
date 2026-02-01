// File: src/pages/settings/ui/tabs/AccountTab.tsx
// Account settings tab - unified style

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { RefreshCw } from 'lucide-react';

import { Switch, Label, Input } from '@/shared/ui';
import { SettingsCard } from '../SettingsCard';
import type { AppConfig } from '@/entities/config';

interface AccountTabProps {
  formData: AppConfig;
  onUpdate: (updates: Partial<AppConfig>) => void;
}

export const AccountTab = memo(function AccountTab({ formData, onUpdate }: AccountTabProps) {
  const { t } = useTranslation();

  return (
    <SettingsCard title={t('settings.account.sync_settings')} icon={RefreshCw} description="Manage how accounts are synchronized">
      <div className="space-y-4">
        <div className="flex items-center justify-between py-2">
          <div className="space-y-0.5">
            <Label className="text-sm text-zinc-900 dark:text-zinc-100">{t('settings.account.auto_refresh')}</Label>
            <p className="text-xs text-zinc-500">{t('settings.account.auto_refresh_desc')}</p>
          </div>
          <span className="text-xs font-medium text-indigo-500 dark:text-indigo-400 uppercase tracking-wide">
            {t('settings.account.always_on')}
          </span>
        </div>

        <div className="flex items-center gap-3 pl-1">
          <Label className="text-xs text-zinc-500 w-32">{t('settings.account.refresh_interval')}</Label>
          <Input
            type="number"
            className="w-20 h-8 text-sm bg-zinc-50 dark:bg-zinc-800 border-zinc-200 dark:border-zinc-700 text-zinc-900 dark:text-white"
            value={formData.refresh_interval}
            onChange={(e) => onUpdate({ refresh_interval: Number(e.target.value) })}
          />
          <span className="text-xs text-zinc-500">min</span>
        </div>

        <div className="h-px bg-zinc-100 dark:bg-zinc-800" />

        <div className="flex items-center justify-between py-2">
          <div className="space-y-0.5">
            <Label className="text-sm text-zinc-900 dark:text-zinc-100">{t('settings.account.auto_sync')}</Label>
            <p className="text-xs text-zinc-500">{t('settings.account.auto_sync_desc')}</p>
          </div>
          <Switch
            checked={formData.auto_sync}
            onCheckedChange={(c) => onUpdate({ auto_sync: c })}
          />
        </div>

        {formData.auto_sync && (
          <div className="flex items-center gap-3 pl-1">
            <Label className="text-xs text-zinc-500 w-32">{t('settings.account.sync_interval')}</Label>
            <Input
              type="number"
              className="w-20 h-8 text-sm bg-zinc-50 dark:bg-zinc-800 border-zinc-200 dark:border-zinc-700 text-zinc-900 dark:text-white"
              value={formData.sync_interval}
              onChange={(e) => onUpdate({ sync_interval: Number(e.target.value) })}
            />
            <span className="text-xs text-zinc-500">min</span>
          </div>
        )}
      </div>
    </SettingsCard>
  );
});
