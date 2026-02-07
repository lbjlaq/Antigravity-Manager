// File: src/widgets/update-notification/ui/UpdateNotification.tsx
// Update notification widget - opens GitHub releases link instead of auto-download

import React, { useEffect, useState } from 'react';
import { X, ExternalLink, ArrowRight, Rocket } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import { invoke } from '@/shared/api';
import { useTranslation } from 'react-i18next';
import { openUrl } from '@tauri-apps/plugin-opener';
import { isTauri } from '@/shared/lib';

interface UpdateInfo {
  has_update: boolean;
  latest_version: string;
  current_version: string;
  download_url: string;
}

interface UpdateNotificationProps {
  onClose: () => void;
}

const RELEASES_URL = 'https://github.com/GofMan5/Antigravity-Manager/releases/latest';

export const UpdateNotification: React.FC<UpdateNotificationProps> = ({ onClose }) => {
  const { t } = useTranslation();
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [isVisible, setIsVisible] = useState(false);

  useEffect(() => {
    checkForUpdates();
  }, []);

  const checkForUpdates = async () => {
    try {
      const info = await invoke<UpdateInfo>('check_for_updates');
      if (info.has_update) {
        setUpdateInfo(info);
        setTimeout(() => setIsVisible(true), 100);
      } else {
        onClose();
      }
    } catch (error) {
      console.error('[UpdateNotification] Failed to check:', error);
      onClose();
    }
  };

  const handleOpenReleases = async () => {
    try {
      if (isTauri()) {
        await openUrl(RELEASES_URL);
      } else {
        window.open(RELEASES_URL, '_blank');
      }
    } catch (error) {
      console.error('[UpdateNotification] Failed to open URL:', error);
      window.open(RELEASES_URL, '_blank');
    }
    handleClose();
  };

  const handleClose = () => {
    setIsVisible(false);
    setTimeout(onClose, 300);
  };

  if (!updateInfo) {
    return null;
  }

  return (
    <AnimatePresence>
      {isVisible && (
        <motion.div
          initial={{ opacity: 0, y: -20, scale: 0.95 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: -20, scale: 0.95 }}
          transition={{ type: 'spring', damping: 25, stiffness: 350 }}
          className="fixed top-20 right-6 z-[100]"
        >
          <div className="relative w-[340px] overflow-hidden rounded-2xl bg-zinc-950 border border-zinc-800 shadow-2xl shadow-black/50">
            {/* Gradient background */}
            <div className="absolute inset-0 rounded-2xl bg-gradient-to-br from-indigo-500/20 via-purple-500/10 to-pink-500/20 opacity-50" />
            
            <div className="relative">
              {/* Header */}
              <div className="flex items-center justify-between p-4 pb-3">
                <div className="flex items-center gap-3">
                  <motion.div 
                    animate={{ rotate: [0, 10, -10, 0] }}
                    transition={{ repeat: Infinity, duration: 2 }}
                    className="p-2 rounded-xl bg-gradient-to-br from-indigo-500 to-purple-600"
                  >
                    <Rocket className="w-5 h-5 text-white" />
                  </motion.div>
                  <div>
                    <h3 className="font-bold text-white text-sm">
                      {t('update_notification.title', 'Update Available')}
                    </h3>
                    <p className="text-xs text-zinc-500">
                      {t('update_notification.new_version_ready', 'New version is ready')}
                    </p>
                  </div>
                </div>

                <button
                  onClick={handleClose}
                  className="p-1.5 rounded-lg text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800 transition-all"
                >
                  <X className="w-4 h-4" />
                </button>
              </div>

              {/* Version comparison */}
              <div className="mx-4 mb-3 p-3 rounded-xl bg-zinc-900/80 border border-zinc-800">
                <div className="flex items-center justify-between">
                  <div className="text-center">
                    <div className="text-[10px] text-zinc-500 uppercase tracking-wider mb-1">
                      {t('update_notification.current', 'Current')}
                    </div>
                    <div className="font-mono text-sm text-zinc-400">v{updateInfo.current_version}</div>
                  </div>
                  <div className="flex items-center gap-2 px-3">
                    <ArrowRight className="w-4 h-4 text-zinc-600" />
                  </div>
                  <div className="text-center">
                    <div className="text-[10px] text-zinc-500 uppercase tracking-wider mb-1">
                      {t('update_notification.new', 'New')}
                    </div>
                    <div className="font-mono text-sm text-emerald-400 font-bold">v{updateInfo.latest_version}</div>
                  </div>
                </div>
              </div>

              {/* Action button */}
              <div className="p-4 pt-1">
                <motion.button
                  whileHover={{ scale: 1.02 }}
                  whileTap={{ scale: 0.98 }}
                  onClick={handleOpenReleases}
                  className="w-full py-2.5 bg-gradient-to-r from-indigo-500 to-purple-600 text-white text-sm font-bold rounded-xl hover:from-indigo-600 hover:to-purple-700 transition-all flex items-center justify-center gap-2"
                >
                  <ExternalLink className="w-4 h-4" />
                  {t('update_notification.download', 'Download from GitHub')}
                </motion.button>
                <p className="text-[10px] text-zinc-600 text-center mt-2">
                  {t('update_notification.hint', 'Opens releases page in browser')}
                </p>
              </div>
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
};

export default UpdateNotification;
