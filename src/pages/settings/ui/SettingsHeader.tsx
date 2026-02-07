// File: src/pages/settings/ui/SettingsHeader.tsx
// Settings page header - styled like AccountsHeader

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Settings, Save } from 'lucide-react';

interface SettingsHeaderProps {
    onSave: () => void;
}

export const SettingsHeader = memo(function SettingsHeader({ onSave }: SettingsHeaderProps) {
    const { t } = useTranslation();

    return (
        <div className="flex-none flex items-center justify-between px-5 py-4 border-b border-zinc-200 dark:border-zinc-800">
            <div className="flex items-center gap-3">
                <div className="p-2 rounded-lg bg-zinc-100 dark:bg-zinc-800">
                    <Settings className="w-5 h-5 text-zinc-600 dark:text-zinc-400" />
                </div>
                <div>
                    <h1 className="text-lg font-semibold text-zinc-900 dark:text-white">
                        {t('nav.settings', 'Settings')}
                    </h1>
                    <p className="text-xs text-zinc-500 dark:text-zinc-500">
                        {t('settings.subtitle', 'Configure your application')}
                    </p>
                </div>
            </div>
            <button
                type="button"
                onClick={onSave}
                className="px-4 py-2 flex items-center gap-2 rounded-lg border border-zinc-300 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 text-sm font-medium hover:bg-zinc-100 dark:hover:bg-zinc-700 hover:text-zinc-900 dark:hover:text-white transition-colors"
            >
                <Save className="w-4 h-4" />
                {t('settings.save', 'Save Settings')}
            </button>
        </div>
    );
});
