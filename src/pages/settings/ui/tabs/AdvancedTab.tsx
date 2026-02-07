// File: src/pages/settings/ui/tabs/AdvancedTab.tsx
// Advanced settings tab - unified style

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Bug, Shield, Terminal } from 'lucide-react';

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

  const validationBlockEnabled = (formData.validation_block_minutes ?? 10) > 0;

  return (
    <div className="space-y-4">
      <SettingsCard title={t('settings.advanced.debug_console', 'Debug Console')} icon={Bug} description="View internal logs and debug info">
        <DebugConsoleToggle />
      </SettingsCard>

      <SettingsCard title={t('settings.advanced.validation_block', 'Validation Block')} icon={Shield} description="Handle VALIDATION_REQUIRED errors">
        <div className="space-y-4">
          <div className="flex items-center justify-between py-2">
            <div className="space-y-0.5">
              <Label className="text-sm text-zinc-900 dark:text-zinc-100">
                {t('settings.advanced.validation_block_enable', 'Enable Validation Block')}
              </Label>
              <p className="text-xs text-zinc-500">
                {t('settings.advanced.validation_block_desc', 'Temporarily block accounts after VALIDATION_REQUIRED (403) error')}
              </p>
            </div>
            <Switch
              checked={validationBlockEnabled}
              onCheckedChange={(c) => onUpdate({ validation_block_minutes: c ? 10 : 0 })}
            />
          </div>

          {validationBlockEnabled && (
            <>
              <div className="h-px bg-zinc-100 dark:bg-zinc-800" />
              <div className="flex items-center justify-between py-2">
                <Label className="text-sm text-zinc-900 dark:text-zinc-100">
                  {t('settings.advanced.validation_block_duration', 'Block Duration')}
                </Label>
                <div className="flex items-center gap-2">
                  <Input
                    type="number"
                    min={1}
                    max={60}
                    className="w-16 h-8 text-sm text-center bg-zinc-50 dark:bg-zinc-800 border-zinc-200 dark:border-zinc-700 text-zinc-900 dark:text-white"
                    value={formData.validation_block_minutes ?? 10}
                    onChange={(e) => onUpdate({ validation_block_minutes: Number(e.target.value) })}
                  />
                  <span className="text-xs text-zinc-500">{t('common.minutes', 'min')}</span>
                </div>
              </div>
            </>
          )}
        </div>
      </SettingsCard>

      <SettingsCard title={t('settings.advanced.paths', 'Paths')} icon={Terminal} description="File system paths">
        <div className="space-y-2">
          <Label className="text-xs text-zinc-500">{t('settings.advanced.data_dir')}</Label>
          <div className="flex gap-2">
            <Input 
              readOnly 
              value={dataDirPath} 
              className="h-9 flex-1 bg-zinc-50 dark:bg-zinc-800 border-zinc-200 dark:border-zinc-700 text-zinc-500 dark:text-zinc-400 text-sm" 
            />
            {isTauri() && (
              <Button 
                variant="outline" 
                size="sm"
                className="h-9 border-zinc-200 dark:border-zinc-700 hover:bg-zinc-100 dark:hover:bg-zinc-800" 
                onClick={() => invoke('open_data_folder')}
              >
                {t('settings.advanced.open_btn')}
              </Button>
            )}
          </div>
        </div>
      </SettingsCard>
    </div>
  );
});
