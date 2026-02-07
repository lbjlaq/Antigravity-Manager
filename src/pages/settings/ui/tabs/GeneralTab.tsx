// File: src/pages/settings/ui/tabs/GeneralTab.tsx
// General settings tab - unified style

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Monitor, RefreshCcw } from 'lucide-react';

import { isTauri } from '@/shared/lib';
import { Switch, Label, Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/shared/ui';
import { SettingsCard } from '../SettingsCard';
import type { AppConfig } from '@/entities/config';

interface GeneralTabProps {
  formData: AppConfig;
  onLanguageChange: (val: string) => void;
  onThemeChange: (val: string) => void;
  onAutoLaunchChange: (checked: boolean) => void;
  onAutoCheckUpdateChange: (checked: boolean) => void;
}

export const GeneralTab = memo(function GeneralTab({
  formData,
  onLanguageChange,
  onThemeChange,
  onAutoLaunchChange,
  onAutoCheckUpdateChange,
}: GeneralTabProps) {
  const { t } = useTranslation();

  return (
    <div className="space-y-4">
      <SettingsCard title={t('settings.general.title')} icon={Monitor} description={t('settings.general.desc', 'Customize look and feel')}>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div className="space-y-2">
            <Label className="text-xs text-zinc-500">{t('settings.general.language')}</Label>
            <Select value={formData.language} onValueChange={onLanguageChange}>
              <SelectTrigger className="h-9 bg-zinc-50 dark:bg-zinc-800 border-zinc-200 dark:border-zinc-700 text-zinc-900 dark:text-white">
                <SelectValue />
              </SelectTrigger>
              <SelectContent className="bg-white dark:bg-zinc-800 border-zinc-200 dark:border-zinc-700">
                <SelectItem value="zh">简体中文</SelectItem>
                <SelectItem value="en">English</SelectItem>
                <SelectItem value="ru">Русский</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-2">
            <Label className="text-xs text-zinc-500">{t('settings.general.theme')}</Label>
            <Select value={formData.theme} onValueChange={onThemeChange}>
              <SelectTrigger className="h-9 bg-zinc-50 dark:bg-zinc-800 border-zinc-200 dark:border-zinc-700 text-zinc-900 dark:text-white">
                <SelectValue />
              </SelectTrigger>
              <SelectContent className="bg-white dark:bg-zinc-800 border-zinc-200 dark:border-zinc-700">
                <SelectItem value="light">{t('settings.general.theme_light')}</SelectItem>
                <SelectItem value="dark">{t('settings.general.theme_dark')}</SelectItem>
                <SelectItem value="system">{t('settings.general.theme_system')}</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>
      </SettingsCard>

      <SettingsCard title={t('settings.system')} icon={RefreshCcw} description="System-level behaviors">
        <div className="space-y-4">
          <div className="flex items-center justify-between py-2">
            <div className="space-y-0.5">
              <Label className="text-sm text-zinc-900 dark:text-zinc-100">{t('settings.general.auto_launch')}</Label>
              <p className="text-xs text-zinc-500">{t('settings.general.auto_launch_desc')}</p>
            </div>
            <Switch
              disabled={!isTauri()}
              checked={formData.auto_launch}
              onCheckedChange={onAutoLaunchChange}
            />
          </div>

          <div className="h-px bg-zinc-100 dark:bg-zinc-800" />

          <div className="flex items-center justify-between py-2">
            <div className="space-y-0.5">
              <Label className="text-sm text-zinc-900 dark:text-zinc-100">{t('settings.general.auto_check_update')}</Label>
              <p className="text-xs text-zinc-500">{t('settings.general.auto_check_update_desc')}</p>
            </div>
            <Switch
              checked={formData.auto_check_update ?? true}
              onCheckedChange={onAutoCheckUpdateChange}
            />
          </div>
        </div>
      </SettingsCard>
    </div>
  );
});
