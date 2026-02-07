// File: src/pages/settings/ui/DebugConsoleToggle.tsx
// Debug console toggle - unified style

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Terminal } from 'lucide-react';

import { Switch, Label } from '@/shared/ui';
import { useDebugConsole } from '@/widgets/debug-console';
import { showToast } from '@/shared/ui';

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
      <div className="flex items-center justify-between py-2">
        <div className="space-y-0.5">
          <Label className="text-sm text-zinc-900 dark:text-zinc-100">
            {t('settings.advanced.debug_console_enable', 'Enable Debug Console')}
          </Label>
          <p className="text-xs text-zinc-500">
            {t('settings.advanced.debug_console_desc', 'Show real-time application logs in the navbar')}
          </p>
        </div>
        <Switch checked={isEnabled} onCheckedChange={handleToggle} />
      </div>
      {isEnabled && (
        <div className="p-3 rounded-lg bg-emerald-50 dark:bg-emerald-500/10 border border-emerald-200 dark:border-emerald-500/20 text-emerald-600 dark:text-emerald-400 text-xs">
          <div className="flex items-center gap-2">
            <Terminal size={14} />
            <span>{t('settings.advanced.debug_console_active', 'Console is active. Click the Console button in the navbar to view logs.')}</span>
          </div>
        </div>
      )}
    </div>
  );
});
