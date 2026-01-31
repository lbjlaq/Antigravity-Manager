// File: src/pages/settings/ui/tabs/AboutTab.tsx
import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { motion } from 'framer-motion';
import { User, MessageCircle, Github, RefreshCw } from 'lucide-react';

import { showToast } from '@/components/common/ToastContainer';

interface AboutTabProps {
  appVersion: string;
}

export const AboutTab = memo(function AboutTab({ appVersion }: AboutTabProps) {
  const { t } = useTranslation();

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      className="space-y-6"
    >
      {/* Hero Card */}
      <div className="relative overflow-hidden rounded-3xl bg-gradient-to-br from-indigo-600 via-purple-600 to-pink-500 p-[1px]">
        <div className="relative rounded-3xl bg-zinc-950 p-8">
          <div className="absolute top-0 right-0 w-64 h-64 bg-indigo-500/20 rounded-full blur-3xl" />
          <div className="absolute bottom-0 left-0 w-48 h-48 bg-purple-500/20 rounded-full blur-3xl" />

          <div className="relative flex items-center gap-6">
            <div className="relative group">
              <div className="absolute inset-0 bg-gradient-to-br from-indigo-500 to-purple-500 rounded-2xl blur-xl opacity-50 group-hover:opacity-75 transition-opacity" />
              <img
                src="/icon.png"
                alt="Logo"
                className="relative w-20 h-20 rounded-2xl shadow-2xl ring-2 ring-white/10 group-hover:ring-white/20 transition-all"
              />
            </div>

            <div className="flex-1">
              <h2 className="text-2xl font-bold text-white mb-1">Antigravity Manager</h2>
              <p className="text-zinc-400 text-sm mb-3">Advanced API Proxy & Account Management</p>
              <div className="flex items-center gap-2">
                <span className="px-2.5 py-1 rounded-lg bg-indigo-500/20 text-indigo-300 text-xs font-semibold border border-indigo-500/30">
                  v{appVersion || '5.0.4'}
                </span>
                <span className="px-2.5 py-1 rounded-lg bg-emerald-500/20 text-emerald-300 text-xs font-semibold border border-emerald-500/30">
                  Stable
                </span>
                <span className="px-2.5 py-1 rounded-lg bg-zinc-800 text-zinc-400 text-xs font-mono border border-zinc-700">
                  Tauri v2 + React 18
                </span>
              </div>
            </div>

            <button
              onClick={() => showToast(t('settings.about.latest_version', "You're up to date!"), 'success')}
              className="px-4 py-2.5 rounded-xl bg-white/10 hover:bg-white/20 text-white text-sm font-medium border border-white/10 hover:border-white/20 transition-all flex items-center gap-2"
            >
              <RefreshCw className="w-4 h-4" />
              {t('settings.about.check_update', 'Check Update')}
            </button>
          </div>
        </div>
      </div>

      {/* Info Grid */}
      <div className="grid grid-cols-3 gap-4">
        <div className="group p-5 rounded-2xl bg-zinc-900/50 border border-zinc-800 hover:border-indigo-500/30 transition-all">
          <div className="flex items-center gap-3 mb-3">
            <div className="p-2.5 rounded-xl bg-blue-500/10">
              <User className="w-5 h-5 text-blue-400" />
            </div>
            <div>
              <div className="text-[10px] text-zinc-500 uppercase tracking-wider font-semibold">{t('settings.about.author', 'Author')}</div>
              <div className="text-white font-bold">GofMan5</div>
            </div>
          </div>
          <p className="text-xs text-zinc-500">Creator & Maintainer</p>
        </div>

        <a
          href="https://t.me/GofMan5"
          target="_blank"
          rel="noreferrer"
          className="group p-5 rounded-2xl bg-zinc-900/50 border border-zinc-800 hover:border-blue-500/30 hover:bg-blue-500/5 transition-all cursor-pointer"
        >
          <div className="flex items-center gap-3 mb-3">
            <div className="p-2.5 rounded-xl bg-blue-500/10 group-hover:bg-blue-500/20 transition-colors">
              <MessageCircle className="w-5 h-5 text-blue-400" />
            </div>
            <div>
              <div className="text-[10px] text-zinc-500 uppercase tracking-wider font-semibold">{t('settings.about.telegram', 'Telegram')}</div>
              <div className="text-white font-bold group-hover:text-blue-400 transition-colors">@GofMan5</div>
            </div>
          </div>
          <p className="text-xs text-zinc-500">Support & Updates</p>
        </a>

        <a
          href="https://github.com/GofMan5/Antigravity-Manager"
          target="_blank"
          rel="noreferrer"
          className="group p-5 rounded-2xl bg-zinc-900/50 border border-zinc-800 hover:border-zinc-600 hover:bg-zinc-800/50 transition-all cursor-pointer"
        >
          <div className="flex items-center gap-3 mb-3">
            <div className="p-2.5 rounded-xl bg-zinc-800 group-hover:bg-zinc-700 transition-colors">
              <Github className="w-5 h-5 text-white" />
            </div>
            <div>
              <div className="text-[10px] text-zinc-500 uppercase tracking-wider font-semibold">{t('settings.about.github', 'GitHub')}</div>
              <div className="text-white font-bold group-hover:text-zinc-300 transition-colors">Source Code</div>
            </div>
          </div>
          <p className="text-xs text-zinc-500">Star & Contribute</p>
        </a>
      </div>

      {/* Footer */}
      <div className="text-center pt-4 border-t border-zinc-800">
        <p className="text-xs text-zinc-600">
          {t('settings.about.copyright', '© 2025-2026 Antigravity. All rights reserved.')}
        </p>
        <p className="text-[10px] text-zinc-700 mt-1">
          Made with ❤️ for developers
        </p>
      </div>
    </motion.div>
  );
});
