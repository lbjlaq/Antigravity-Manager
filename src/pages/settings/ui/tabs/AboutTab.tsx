// File: src/pages/settings/ui/tabs/AboutTab.tsx
// About tab - unified style without decorative blur elements

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { User, MessageCircle, Github, RefreshCw } from 'lucide-react';

import { showToast } from '@/shared/ui';

interface AboutTabProps {
  appVersion: string;
}

export const AboutTab = memo(function AboutTab({ appVersion }: AboutTabProps) {
  const { t } = useTranslation();

  return (
    <div className="space-y-4">
      {/* App Info Card */}
      <div className="rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 p-5">
        <div className="flex items-center gap-4">
          <img
            src="/icon.png"
            alt="Logo"
            className="w-16 h-16 rounded-xl shadow-lg"
          />
          <div className="flex-1">
            <h2 className="text-lg font-bold text-zinc-900 dark:text-white">Antigravity Manager</h2>
            <p className="text-xs text-zinc-500 mb-2">Advanced API Proxy & Account Management</p>
            <div className="flex items-center gap-2 flex-wrap">
              <span className="px-2 py-0.5 rounded-md bg-indigo-100 dark:bg-indigo-500/20 text-indigo-600 dark:text-indigo-400 text-xs font-medium">
                v{appVersion || '5.0.5'}
              </span>
              <span className="px-2 py-0.5 rounded-md bg-emerald-100 dark:bg-emerald-500/20 text-emerald-600 dark:text-emerald-400 text-xs font-medium">
                Stable
              </span>
              <span className="px-2 py-0.5 rounded-md bg-zinc-100 dark:bg-zinc-800 text-zinc-500 text-xs font-mono">
                Tauri v2 + React 18
              </span>
            </div>
          </div>
          <button
            onClick={() => showToast(t('settings.about.latest_version', "You're up to date!"), 'success')}
            className="flex items-center gap-2 px-3 py-2 rounded-lg bg-zinc-100 dark:bg-zinc-800 hover:bg-zinc-200 dark:hover:bg-zinc-700 text-zinc-700 dark:text-zinc-300 text-sm font-medium transition-colors"
          >
            <RefreshCw className="w-4 h-4" />
            {t('settings.about.check_update', 'Check Update')}
          </button>
        </div>
      </div>

      {/* Info Grid */}
      <div className="grid grid-cols-3 gap-3">
        <div className="rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 p-4">
          <div className="flex items-center gap-2 mb-2">
            <div className="p-1.5 rounded-lg bg-blue-100 dark:bg-blue-500/20">
              <User className="w-4 h-4 text-blue-500" />
            </div>
            <div>
              <div className="text-[10px] text-zinc-500 uppercase tracking-wide">{t('settings.about.author', 'Author')}</div>
              <div className="text-sm font-semibold text-zinc-900 dark:text-white">GofMan5</div>
            </div>
          </div>
          <p className="text-xs text-zinc-500">Creator & Maintainer</p>
        </div>

        <a
          href="https://t.me/GofMan5"
          target="_blank"
          rel="noreferrer"
          className="rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 p-4 hover:border-blue-300 dark:hover:border-blue-500/50 transition-colors"
        >
          <div className="flex items-center gap-2 mb-2">
            <div className="p-1.5 rounded-lg bg-blue-100 dark:bg-blue-500/20">
              <MessageCircle className="w-4 h-4 text-blue-500" />
            </div>
            <div>
              <div className="text-[10px] text-zinc-500 uppercase tracking-wide">{t('settings.about.telegram', 'Telegram')}</div>
              <div className="text-sm font-semibold text-zinc-900 dark:text-white">@GofMan5</div>
            </div>
          </div>
          <p className="text-xs text-zinc-500">Support & Updates</p>
        </a>

        <a
          href="https://github.com/GofMan5/Antigravity-Manager"
          target="_blank"
          rel="noreferrer"
          className="rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 p-4 hover:border-zinc-400 dark:hover:border-zinc-600 transition-colors"
        >
          <div className="flex items-center gap-2 mb-2">
            <div className="p-1.5 rounded-lg bg-zinc-100 dark:bg-zinc-800">
              <Github className="w-4 h-4 text-zinc-600 dark:text-zinc-400" />
            </div>
            <div>
              <div className="text-[10px] text-zinc-500 uppercase tracking-wide">{t('settings.about.github', 'GitHub')}</div>
              <div className="text-sm font-semibold text-zinc-900 dark:text-white">Source Code</div>
            </div>
          </div>
          <p className="text-xs text-zinc-500">Star & Contribute</p>
        </a>
      </div>

      {/* Footer */}
      <div className="text-center pt-3">
        <p className="text-xs text-zinc-400">
          {t('settings.about.copyright', '© 2025-2026 Antigravity. All rights reserved.')}
        </p>
      </div>
    </div>
  );
});
