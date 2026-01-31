// File: src/pages/settings/ui/tabs/AccountTab.tsx
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
      <div className="space-y-6">
        <div className="flex items-center justify-between p-2 rounded-lg hover:bg-white/5 transition-colors">
          <div className="space-y-1">
            <Label className="text-base text-zinc-200">{t('settings.account.auto_refresh')}</Label>
            <p className="text-sm text-zinc-500">{t('settings.account.auto_refresh_desc')}</p>
          </div>
          <div className="flex items-center gap-2">
            <span className="text-xs font-bold text-indigo-400 uppercase tracking-widest">{t('settings.account.always_on')}</span>
            <div className="w-2 h-2 rounded-full bg-indigo-500 animate-pulse shadow-[0_0_10px_rgba(99,102,241,0.5)]" />
          </div>
        </div>
        <div className="flex items-center gap-4 pt-2 pl-2">
          <Label className="w-40 text-zinc-400">{t('settings.account.refresh_interval')}</Label>
          <Input
            type="number"
            className="w-24 bg-zinc-50 dark:bg-zinc-900/50 border-zinc-200 dark:border-white/10 text-zinc-900 dark:text-white"
            value={formData.refresh_interval}
            onChange={(e) => onUpdate({ refresh_interval: Number(e.target.value) })}
          />
        </div>

        <div className="h-px bg-white/5 w-full" />

        <div className="flex items-center justify-between p-2 rounded-lg hover:bg-white/5 transition-colors">
          <div className="space-y-1">
            <Label className="text-base text-zinc-200">{t('settings.account.auto_sync')}</Label>
            <p className="text-sm text-zinc-500">{t('settings.account.auto_sync_desc')}</p>
          </div>
          <Switch
            checked={formData.auto_sync}
            onCheckedChange={(c) => onUpdate({ auto_sync: c })}
          />
        </div>
        {formData.auto_sync && (
          <div className="flex items-center gap-4 pt-2 pl-2">
            <Label className="w-40 text-zinc-400">{t('settings.account.sync_interval')}</Label>
            <Input
              type="number"
              className="w-24 bg-zinc-50 dark:bg-zinc-900/50 border-zinc-200 dark:border-white/10 text-zinc-900 dark:text-white"
              value={formData.sync_interval}
              onChange={(e) => onUpdate({ sync_interval: Number(e.target.value) })}
            />
          </div>
        )}
      </div>
    </SettingsCard>
  );
});
