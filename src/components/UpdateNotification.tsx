import React, { useEffect, useState } from 'react';
import { X, Download, Loader2, CheckCircle, ExternalLink, ArrowRight, Rocket } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import { request as invoke } from '../utils/request';
import { useTranslation } from 'react-i18next';
import { check as tauriCheck } from '@tauri-apps/plugin-updater';
import { relaunch as tauriRelaunch } from '@tauri-apps/plugin-process';
import { isTauri } from '../utils/env';
import { showToast } from './common/ToastContainer';

interface UpdateInfo {
  has_update: boolean;
  latest_version: string;
  current_version: string;
  download_url: string;
}

type UpdateState = 'checking' | 'available' | 'downloading' | 'ready' | 'none';

interface UpdateNotificationProps {
  onClose: () => void;
}

export const UpdateNotification: React.FC<UpdateNotificationProps> = ({ onClose }) => {
  const { t } = useTranslation();
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [isVisible, setIsVisible] = useState(false);
  const [updateState, setUpdateState] = useState<UpdateState>('checking');
  const [downloadProgress, setDownloadProgress] = useState(0);

  useEffect(() => {
    checkForUpdates();
  }, []);

  const checkForUpdates = async () => {
    try {
      const info = await invoke<UpdateInfo>('check_for_updates');
      if (info.has_update) {
        setUpdateInfo(info);
        setUpdateState('available');
        setTimeout(() => setIsVisible(true), 100);
      } else {
        onClose();
      }
    } catch (error) {
      console.error('Failed to check for updates:', error);
      onClose();
    }
  };

  const handleAutoUpdate = async () => {
    if (!isTauri()) {
      handleManualDownload();
      return;
    }

    setUpdateState('downloading');
    try {
      const update = await tauriCheck();
      if (update) {
        let downloaded = 0;
        let contentLength = 0;

        await update.downloadAndInstall((event) => {
          switch (event.event) {
            case 'Started':
              contentLength = event.data.contentLength || 0;
              break;
            case 'Progress':
              downloaded += event.data.chunkLength;
              if (contentLength > 0) {
                setDownloadProgress(Math.round((downloaded / contentLength) * 100));
              }
              break;
            case 'Finished':
              setUpdateState('ready');
              break;
          }
        });

        setUpdateState('ready');
        setTimeout(async () => {
          await tauriRelaunch();
        }, 1500);
      } else {
        showToast(t('update_notification.fallback_manual', 'Opening download page...'), 'info');
        setUpdateState('available');
        handleManualDownload();
      }
    } catch (error) {
      console.error('Auto update failed:', error);
      showToast(t('update_notification.auto_failed', 'Auto-update failed'), 'error');
      setUpdateState('available');
      handleManualDownload();
    }
  };

  const handleManualDownload = () => {
    if (updateInfo?.download_url) {
      window.open(updateInfo.download_url, '_blank');
      handleClose();
    }
  };

  const handleClose = () => {
    setIsVisible(false);
    setTimeout(onClose, 300);
  };

  if (!updateInfo && updateState !== 'checking') {
    return null;
  }

  const isProcessing = updateState === 'downloading' || updateState === 'ready';

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
          {/* Main Card */}
          <div className="relative w-[340px] overflow-hidden rounded-2xl bg-zinc-950 border border-zinc-800 shadow-2xl shadow-black/50">
            {/* Gradient Border Effect */}
            <div className="absolute inset-0 rounded-2xl bg-gradient-to-br from-indigo-500/20 via-purple-500/10 to-pink-500/20 opacity-50" />
            
            {/* Content Container */}
            <div className="relative">
              {/* Header */}
              <div className="flex items-center justify-between p-4 pb-3">
                <div className="flex items-center gap-3">
                  <motion.div 
                    animate={{ rotate: updateState === 'ready' ? 0 : [0, 10, -10, 0] }}
                    transition={{ repeat: updateState === 'ready' ? 0 : Infinity, duration: 2 }}
                    className={`p-2 rounded-xl ${
                      updateState === 'ready' 
                        ? 'bg-emerald-500/20' 
                        : 'bg-gradient-to-br from-indigo-500 to-purple-600'
                    }`}
                  >
                    {updateState === 'ready' ? (
                      <CheckCircle className="w-5 h-5 text-emerald-400" />
                    ) : updateState === 'downloading' ? (
                      <Loader2 className="w-5 h-5 text-white animate-spin" />
                    ) : (
                      <Rocket className="w-5 h-5 text-white" />
                    )}
                  </motion.div>
                  <div>
                    <h3 className="font-bold text-white text-sm">
                      {updateState === 'ready'
                        ? t('update_notification.ready', 'Ready to restart')
                        : updateState === 'downloading'
                        ? t('update_notification.downloading', 'Downloading...')
                        : t('update_notification.title', 'Update Available')}
                    </h3>
                    <p className="text-xs text-zinc-500">
                      {updateState === 'downloading' 
                        ? `${downloadProgress}% complete`
                        : 'New version is ready'}
                    </p>
                  </div>
                </div>

                {!isProcessing && (
                  <button
                    onClick={handleClose}
                    className="p-1.5 rounded-lg text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800 transition-all"
                  >
                    <X className="w-4 h-4" />
                  </button>
                )}
              </div>

              {/* Version Info */}
              {updateInfo && (
                <div className="mx-4 mb-3 p-3 rounded-xl bg-zinc-900/80 border border-zinc-800">
                  <div className="flex items-center justify-between">
                    <div className="text-center">
                      <div className="text-[10px] text-zinc-500 uppercase tracking-wider mb-1">Current</div>
                      <div className="font-mono text-sm text-zinc-400">v{updateInfo.current_version}</div>
                    </div>
                    <div className="flex items-center gap-2 px-3">
                      <ArrowRight className="w-4 h-4 text-zinc-600" />
                    </div>
                    <div className="text-center">
                      <div className="text-[10px] text-zinc-500 uppercase tracking-wider mb-1">New</div>
                      <div className="font-mono text-sm text-emerald-400 font-bold">v{updateInfo.latest_version}</div>
                    </div>
                  </div>
                </div>
              )}

              {/* Progress Bar */}
              {updateState === 'downloading' && (
                <div className="mx-4 mb-3">
                  <div className="h-1.5 bg-zinc-800 rounded-full overflow-hidden">
                    <motion.div
                      initial={{ width: 0 }}
                      animate={{ width: `${downloadProgress}%` }}
                      className="h-full bg-gradient-to-r from-indigo-500 via-purple-500 to-pink-500 rounded-full"
                      transition={{ duration: 0.3 }}
                    />
                  </div>
                </div>
              )}

              {/* Actions */}
              {updateState === 'available' && (
                <div className="p-4 pt-1 flex gap-2">
                  <motion.button
                    whileHover={{ scale: 1.02 }}
                    whileTap={{ scale: 0.98 }}
                    onClick={handleAutoUpdate}
                    className="flex-1 py-2.5 bg-gradient-to-r from-indigo-500 to-purple-600 text-white text-sm font-bold rounded-xl hover:from-indigo-600 hover:to-purple-700 transition-all flex items-center justify-center gap-2"
                  >
                    <Download className="w-4 h-4" />
                    {t('update_notification.auto_update', 'Update Now')}
                  </motion.button>
                  <motion.button
                    whileHover={{ scale: 1.05 }}
                    whileTap={{ scale: 0.95 }}
                    onClick={handleManualDownload}
                    className="p-2.5 bg-zinc-800 text-zinc-400 rounded-xl hover:bg-zinc-700 hover:text-white transition-all"
                    title={t('update_notification.manual_download', 'Download manually')}
                  >
                    <ExternalLink className="w-4 h-4" />
                  </motion.button>
                </div>
              )}

              {/* Ready State */}
              {updateState === 'ready' && (
                <div className="p-4 pt-1">
                  <div className="flex items-center justify-center gap-2 py-2 text-emerald-400 text-sm">
                    <motion.div
                      initial={{ scale: 0 }}
                      animate={{ scale: 1 }}
                      transition={{ type: 'spring', stiffness: 300 }}
                    >
                      <CheckCircle className="w-5 h-5" />
                    </motion.div>
                    <span className="font-medium">{t('update_notification.restarting', 'Restarting...')}</span>
                  </div>
                </div>
              )}
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
};
