// File: src/pages/settings/ui/tabs/ProxyTab.tsx
// Proxy settings tab - unified style

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
    <SettingsCard title={t('proxy.config.upstream_proxy.title')} icon={Globe} description="Route traffic through another proxy">
      <div className="space-y-4">
        <div className="flex items-center justify-between py-2">
          <div className="space-y-0.5">
            <Label className="text-sm text-zinc-900 dark:text-zinc-100">{t('proxy.config.upstream_proxy.enable')}</Label>
            <p className="text-xs text-zinc-500">{t('proxy.config.upstream_proxy.desc')}</p>
          </div>
          <Switch
            checked={formData.proxy?.upstream_proxy?.enabled || false}
            onCheckedChange={(c) => updateUpstreamProxy({ enabled: c })}
          />
        </div>

        <div className="space-y-2">
          <Label className="text-xs text-zinc-500">{t('proxy.config.upstream_proxy.url')}</Label>
          <Input
            value={formData.proxy?.upstream_proxy?.url || ''}
            onChange={(e) => updateUpstreamProxy({ url: e.target.value })}
            className="h-9 bg-zinc-50 dark:bg-zinc-800 border-zinc-200 dark:border-zinc-700 text-zinc-900 dark:text-white"
            placeholder={t('proxy.config.upstream_proxy.url_placeholder')}
          />
        </div>
      </div>
    </SettingsCard>
  );
});
