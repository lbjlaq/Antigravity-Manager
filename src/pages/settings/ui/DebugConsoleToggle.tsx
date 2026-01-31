// File: src/pages/settings/ui/DebugConsoleToggle.tsx
import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Terminal } from 'lucide-react';

import { Switch, Label } from '@/shared/ui';
import { useDebugConsole } from '@/stores/useDebugConsole';
import { showToast } from '@/components/common/ToastContainer';

export const DebugConsoleToggle = memo(function DebugConsoleToggle() {
  const { t } = useTranslation();
  const { isEnabled, enable, disable } = useDebugConsole();

  const handleToggle = async (checked: boolean) => {
    if (checked) {
      await enable();
      showToast(t('settings.advanced.debug_console_enabled', 'Debug console enabled'), 'success');
    } else {
      await disable();
      showToast(t('settings.advanced.debug_console_disabled', 'Debug console disabled'), 'info');
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div className="space-y-1">
          <Label className="text-base text-zinc-200">
            {t('settings.advanced.debug_console_enable', 'Enable Debug Console')}
          </Label>
          <p className="text-sm text-zinc-500">
            {t('settings.advanced.debug_console_desc', 'Show real-time application logs in the navbar. Useful for debugging.')}
          </p>
        </div>
        <Switch checked={isEnabled} onCheckedChange={handleToggle} />
      </div>
      {isEnabled && (
        <div className="p-3 rounded-lg bg-green-500/10 border border-green-500/20 text-green-400 text-sm">
          <div className="flex items-center gap-2">
            <Terminal size={16} />
            <span>{t('settings.advanced.debug_console_active', 'Console is active. Click the Console button in the navbar to view logs.')}</span>
          </div>
        </div>
      )}
    </div>
  );
});
