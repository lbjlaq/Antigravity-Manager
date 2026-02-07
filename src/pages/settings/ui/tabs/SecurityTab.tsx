// File: src/pages/settings/ui/tabs/SecurityTab.tsx
// Security settings tab - unified style

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Shield, Monitor } from 'lucide-react';

import { QuotaProtection, PinnedQuotaModels } from '@/features/settings';
import { SettingsCard } from '../SettingsCard';
import type { AppConfig } from '@/entities/config';

interface SecurityTabProps {
  formData: AppConfig;
  onUpdate: (updates: Partial<AppConfig>) => void;
}

export const SecurityTab = memo(function SecurityTab({ formData, onUpdate }: SecurityTabProps) {
  const { t } = useTranslation();

  return (
    <div className="space-y-4">
      <SettingsCard title={t('settings.security.quota_protection')} icon={Shield} description="Protect accounts from quota exhaustion">
        <QuotaProtection
          config={formData.quota_protection}
          onChange={(newConfig) => onUpdate({ quota_protection: newConfig })}
        />
      </SettingsCard>

      <SettingsCard title={t('settings.security.monitoring')} icon={Monitor} description="Pin models to monitor on accounts">
        <PinnedQuotaModels
          config={formData.pinned_quota_models}
          onChange={(newConfig) => onUpdate({ pinned_quota_models: newConfig })}
        />
      </SettingsCard>
    </div>
  );
});
