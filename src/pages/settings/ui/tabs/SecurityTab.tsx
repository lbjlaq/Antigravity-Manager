// File: src/pages/settings/ui/tabs/SecurityTab.tsx
import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Shield, Monitor } from 'lucide-react';

import QuotaProtection from '@/components/settings/QuotaProtection';
import PinnedQuotaModels from '@/components/settings/PinnedQuotaModels';
import { SettingsCard } from '../SettingsCard';
import type { AppConfig } from '@/entities/config';

interface SecurityTabProps {
  formData: AppConfig;
  onUpdate: (updates: Partial<AppConfig>) => void;
}

export const SecurityTab = memo(function SecurityTab({ formData, onUpdate }: SecurityTabProps) {
  const { t } = useTranslation();

  return (
    <>
      <SettingsCard title={t('settings.security.quota_protection')} icon={Shield}>
        <QuotaProtection
          config={formData.quota_protection}
          onChange={(newConfig) => onUpdate({ quota_protection: newConfig })}
        />
      </SettingsCard>
      <SettingsCard title={t('settings.security.monitoring')} icon={Monitor}>
        <PinnedQuotaModels
          config={formData.pinned_quota_models}
          onChange={(newConfig) => onUpdate({ pinned_quota_models: newConfig })}
        />
      </SettingsCard>
    </>
  );
});
