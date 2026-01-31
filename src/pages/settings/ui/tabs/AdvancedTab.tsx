// File: src/pages/settings/ui/tabs/AdvancedTab.tsx
import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Bug, Monitor, Terminal } from 'lucide-react';

import { invoke } from '@/shared/api';
import { isTauri } from '@/shared/lib';
import { Switch, Label, Input, Button } from '@/shared/ui';
import { SettingsCard } from '../SettingsCard';
import { DebugConsoleToggle } from '../DebugConsoleToggle';
import type { AppConfig } from '@/entities/config';

interface AdvancedTabProps {
  formData: AppConfig;
  dataDirPath: string;
  onUpdate: (updates: Partial<AppConfig>) => void;
}

export const AdvancedTab = memo(function AdvancedTab({ formData, dataDirPath, onUpdate }: AdvancedTabProps) {
  const { t } = useTranslation();

  return (
    <>
      <SettingsCard title={t('settings.advanced.debug_console', 'Debug Console')} icon={Bug}>
        <DebugConsoleToggle />
      </SettingsCard>

      <SettingsCard title={t('settings.advanced.display', 'Display Options')} icon={Monitor}>
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <div className="space-y-1">
              <Label className="text-base text-zinc-200">
                {t('settings.advanced.show_proxy_selected_badge', 'Show "SELECTED" badge')}
              </Label>
              <p className="text-sm text-zinc-500">
                {t('settings.advanced.show_proxy_selected_badge_desc', 'Display which accounts are selected for API Proxy scheduling on the Accounts page')}
              </p>
            </div>
            <Switch
              checked={formData.show_proxy_selected_badge ?? true}
              onCheckedChange={(c) => onUpdate({ show_proxy_selected_badge: c })}
            />
          </div>

          <div className="h-px bg-white/5 w-full" />

          <div className="space-y-3">
            <div className="space-y-1">
              <Label className="text-base text-zinc-200">
                {t('settings.advanced.validation_block_minutes', 'Validation Block Duration')}
              </Label>
              <p className="text-sm text-zinc-500">
                {t('settings.advanced.validation_block_minutes_desc', 'How long to temporarily block an account after VALIDATION_REQUIRED (403) error')}
              </p>
            </div>
            <div className="flex items-center gap-3">
              <Input
                type="number"
                min={1}
                max={60}
                className="w-24 bg-zinc-50 dark:bg-zinc-900/50 border-zinc-200 dark:border-white/10 text-zinc-900 dark:text-white"
                value={formData.validation_block_minutes ?? 10}
                onChange={(e) => onUpdate({ validation_block_minutes: Number(e.target.value) })}
              />
              <span className="text-sm text-zinc-500">{t('common.minutes', 'minutes')}</span>
            </div>
          </div>
        </div>
      </SettingsCard>

      <SettingsCard title={t('settings.advanced.paths')} icon={Terminal}>
        <div className="space-y-6">
          <div>
            <Label className="mb-2 block text-zinc-400">{t('settings.advanced.data_dir')}</Label>
            <div className="flex gap-2">
              <Input readOnly value={dataDirPath} className="bg-zinc-50 dark:bg-zinc-900/50 border-zinc-200 dark:border-white/10 text-zinc-500 dark:text-zinc-400" />
              {isTauri() && (
                <Button variant="outline" className="border-white/10 bg-white/5 hover:bg-white/10" onClick={() => invoke('open_data_folder')}>
                  {t('settings.advanced.open_btn')}
                </Button>
              )}
            </div>
          </div>
        </div>
      </SettingsCard>
    </>
  );
});
