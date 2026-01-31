// File: src/pages/settings/ui/tabs/GeneralTab.tsx
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
    <>
      <SettingsCard title={t('settings.general.title')} icon={Monitor} description={t('settings.general.desc', 'Customize look and feel')} className="z-20">
        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
          <div className="space-y-3">
            <Label className="text-zinc-400">{t('settings.general.language')}</Label>
            <Select value={formData.language} onValueChange={onLanguageChange}>
              <SelectTrigger className="bg-zinc-50 dark:bg-zinc-900/50 border-zinc-200 dark:border-white/10 text-zinc-900 dark:text-white">
                <SelectValue />
              </SelectTrigger>
              <SelectContent className="bg-white dark:bg-zinc-900 border-zinc-200 dark:border-white/10 text-zinc-900 dark:text-white">
                <SelectItem value="zh">简体中文</SelectItem>
                <SelectItem value="en">English</SelectItem>
                <SelectItem value="ru">Русский</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-3">
            <Label className="text-zinc-400">{t('settings.general.theme')}</Label>
            <Select value={formData.theme} onValueChange={onThemeChange}>
              <SelectTrigger className="bg-zinc-50 dark:bg-zinc-900/50 border-zinc-200 dark:border-white/10 text-zinc-900 dark:text-white">
                <SelectValue />
              </SelectTrigger>
              <SelectContent className="bg-white dark:bg-zinc-900 border-zinc-200 dark:border-white/10 text-zinc-900 dark:text-white">
                <SelectItem value="light">{t('settings.general.theme_light')}</SelectItem>
                <SelectItem value="dark">{t('settings.general.theme_dark')}</SelectItem>
                <SelectItem value="system">{t('settings.general.theme_system')}</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>
      </SettingsCard>

      <SettingsCard title={t('settings.system')} icon={RefreshCcw} description="System-level behaviors" className="z-10">
        <div className="space-y-6">
          <div className="flex items-center justify-between p-2 rounded-lg hover:bg-white/5 transition-colors">
            <div className="space-y-1">
              <Label className="text-base text-zinc-200">{t('settings.general.auto_launch')}</Label>
              <p className="text-sm text-zinc-500">{t('settings.general.auto_launch_desc')}</p>
            </div>
            <Switch
              disabled={!isTauri()}
              checked={formData.auto_launch}
              onCheckedChange={onAutoLaunchChange}
            />
          </div>

          <div className="h-px bg-white/5 w-full" />

          <div className="flex items-center justify-between p-2 rounded-lg hover:bg-white/5 transition-colors">
            <div className="space-y-1">
              <Label className="text-base text-zinc-200">{t('settings.general.auto_check_update')}</Label>
              <p className="text-sm text-zinc-500">{t('settings.general.auto_check_update_desc')}</p>
            </div>
            <Switch
              checked={formData.auto_check_update ?? true}
              onCheckedChange={onAutoCheckUpdateChange}
            />
          </div>
        </div>
      </SettingsCard>
    </>
  );
});
