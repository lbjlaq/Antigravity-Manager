// File: src/pages/settings/ui/tabs/ProxyTab.tsx
import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Globe } from 'lucide-react';

import { Switch, Label, Input } from '@/shared/ui';
import { SettingsCard } from '../SettingsCard';
import type { AppConfig } from '@/entities/config';

interface ProxyTabProps {
  formData: AppConfig;
  onUpdate: (updates: Partial<AppConfig>) => void;
}

export const ProxyTab = memo(function ProxyTab({ formData, onUpdate }: ProxyTabProps) {
  const { t } = useTranslation();

  const updateUpstreamProxy = (updates: Partial<NonNullable<AppConfig['proxy']['upstream_proxy']>>) => {
    onUpdate({
      proxy: {
        ...formData.proxy,
        upstream_proxy: { ...formData.proxy.upstream_proxy, ...updates },
      },
    });
  };

  return (
    <SettingsCard title={t('proxy.config.upstream_proxy.title')} icon={Globe}>
      <div className="space-y-6">
        <div className="flex items-center justify-between">
          <div className="space-y-1">
            <Label className="text-base text-zinc-200">{t('proxy.config.upstream_proxy.enable')}</Label>
            <p className="text-sm text-zinc-500">{t('proxy.config.upstream_proxy.desc')}</p>
          </div>
          <Switch
            checked={formData.proxy?.upstream_proxy?.enabled || false}
            onCheckedChange={(c) => updateUpstreamProxy({ enabled: c })}
          />
        </div>
        <div className="pt-2 space-y-3">
          <Label className="text-zinc-400">{t('proxy.config.upstream_proxy.url')}</Label>
          <Input
            value={formData.proxy?.upstream_proxy?.url || ''}
            onChange={(e) => updateUpstreamProxy({ url: e.target.value })}
            className="bg-zinc-50 dark:bg-zinc-900/50 border-zinc-200 dark:border-white/10 text-zinc-900 dark:text-white"
            placeholder={t('proxy.config.upstream_proxy.url_placeholder')}
          />
        </div>
      </div>
    </SettingsCard>
  );
});
