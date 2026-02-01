// File: src/pages/settings/ui/tabs/PerformanceTab.tsx
// Performance settings tab - unified style

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Zap } from 'lucide-react';

import SmartWarmup from '@/components/settings/SmartWarmup';
import { SettingsCard } from '../SettingsCard';
import type { AppConfig } from '@/entities/config';

interface PerformanceTabProps {
  formData: AppConfig;
  onUpdate: (updates: Partial<AppConfig>) => void;
}

export const PerformanceTab = memo(function PerformanceTab({ formData, onUpdate }: PerformanceTabProps) {
  const { t } = useTranslation();

  return (
    <SettingsCard title={t('settings.performance.smart_warmup')} icon={Zap} description="Pre-warm accounts to reduce latency">
      <SmartWarmup
        config={formData.scheduled_warmup}
        onChange={(newConfig) => onUpdate({ scheduled_warmup: newConfig })}
      />
    </SettingsCard>
  );
});
