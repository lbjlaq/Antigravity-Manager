import React, { useEffect, useState, useRef } from 'react';
import { X, Sparkles, Loader2, CheckCircle, RotateCcw } from 'lucide-react';
import { request as invoke } from '../utils/request';
import { useTranslation } from 'react-i18next';
import { Update, check as tauriCheck } from '@tauri-apps/plugin-updater';
import { relaunch as tauriRelaunch } from '@tauri-apps/plugin-process';
import { isTauri } from '../utils/env';
import { showToast } from './common/ToastContainer';

interface UpdateInfo {
  has_update: boolean;
  latest_version: string;
  current_version: string;
  download_url: string;
  source?: string;
  proxy_url?: string;
  channel?: 'stable' | 'beta';
  updater_json_url?: string;
}

type UpdateState = 'checking' | 'downloading' | 'ready' | 'error' | 'none' | 'manual';

interface UpdateNotificationProps {
  onClose: () => void;
}

export const UpdateNotification: React.FC<UpdateNotificationProps> = ({ onClose }) => {
  const { t } = useTranslation();
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [isVisible, setIsVisible] = useState(false);
  const [isClosing, setIsClosing] = useState(false);
  const [updateState, setUpdateState] = useState<UpdateState>('checking');
  const [downloadProgress, setDownloadProgress] = useState(0);
  const downloadStarted = useRef(false);

  useEffect(() => {
    checkAndDownload();
  }, []);

  const checkAndDownload = async () => {
    try {
      // 1. Check for updates via backend
      const info = await invoke<UpdateInfo>('check_for_updates');
      if (!info.has_update) {
        onClose();
        return;
      }

      setUpdateInfo(info);

      // 2. If not in Tauri — no auto-update possible
      if (!isTauri()) {
        console.warn('Auto update is only available in Tauri environment');
        onClose();
        return;
      }

      // Check if Linux and not AppImage (e.g. RPM or DEB packages).
      // Tauri updater only supports AppImage on Linux.
      if (navigator.userAgent.toLowerCase().includes('linux')) {
        const isAppImage = await invoke<boolean>('check_appimage_installation');
        if (!isAppImage) {
          setUpdateState('manual');
          setTimeout(() => setIsVisible(true), 100);
          return;
        }
      }

      // 3. Start background download immediately
      if (downloadStarted.current) return;
      downloadStarted.current = true;

      setUpdateState('downloading');
      setTimeout(() => setIsVisible(true), 100);

      let update: Update | null = null;
      try {
        const metadata = await invoke<any>('check_native_update', {
          endpoint: info.updater_json_url,
          proxy: info.proxy_url,
        });
        if (metadata) {
          update = new Update(metadata);
        }
      } catch (err) {
        console.warn('Native update check via command failed, trying fallback plugin check:', err);
        update = await tauriCheck(
          info.proxy_url ? { proxy: info.proxy_url } : undefined
        );
      }

      if (!update) {
        // updater.json not ready yet or no update via native channel
        console.warn('Native updater returned null');
        showToast(t('update_notification.toast.not_ready'), 'info');
        if (info.download_url) {
          try {
            const { openUrl } = await import('@tauri-apps/plugin-opener');
            await openUrl(info.download_url);
          } catch (e) {
            console.error('Failed to open download url:', e);
            window.open(info.download_url, '_blank', 'noopener,noreferrer');
          }
        }
        handleClose();
        return;
      }

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
            break;
        }
      });

      // 4. Download complete — show restart prompt
      setUpdateState('ready');
      setDownloadProgress(100);
    } catch (error) {
      const errorMsg = error instanceof Error ? error.message : String(error);
      console.error('Auto update failed:', errorMsg);
      setUpdateState('error');
      showToast(`${t('update_notification.toast.failed')}: ${errorMsg}`, 'error');
    }
  };

  const handleRestart = async () => {
    try {
      await tauriRelaunch();
    } catch (error) {
      console.error('Relaunch failed:', error);
    }
  };

  const handleClose = () => {
    setIsClosing(true);
    setIsVisible(false);
    setTimeout(onClose, 400);
  };

  if (updateState === 'none') {
    return null;
  }

  return (
    <div
      className={`
        fixed top-6 right-6 z-[100]
        transition-all duration-500 ease-[cubic-bezier(0.34,1.56,0.64,1)]
        ${isVisible && !isClosing ? 'translate-y-0 opacity-100 scale-100' : '-translate-y-4 opacity-0 scale-95'}
      `}
    >
      <div className="
        relative overflow-hidden
        w-80 p-5
        rounded-2xl
        border border-white/20 dark:border-base-200
        shadow-[0_8px_32px_0_rgba(0,0,0,0.3)]
        backdrop-blur-xl
        bg-white/70 dark:bg-base-100/90
        group
      ">
        <div className="absolute -top-10 -right-10 w-32 h-32 bg-blue-500/20 rounded-full blur-3xl pointer-events-none group-hover:bg-blue-500/30 transition-colors duration-500" />
        <div className="absolute -bottom-10 -left-10 w-32 h-32 bg-purple-500/20 rounded-full blur-3xl pointer-events-none group-hover:bg-purple-500/30 transition-colors duration-500" />

        <div className="relative z-10">
          <div className="flex items-start justify-between mb-3">
            <div className="flex items-center gap-2">
              <div className="p-1.5 rounded-lg bg-gradient-to-br from-blue-500 to-purple-600 shadow-sm">
                {updateState === 'ready' ? (
                  <CheckCircle className="w-4 h-4 text-white" />
                ) : (
                  <Sparkles className="w-4 h-4 text-white" />
                )}
              </div>
              <div>
                <h3 className="font-bold text-gray-800 dark:text-white leading-tight">
                  {updateState === 'ready'
                    ? t('update_notification.ready')
                    : t('update_notification.title')}
                </h3>
                {updateInfo && (
                  <div className="flex items-center gap-1.5 mt-0.5">
                    <p className="text-xs font-medium text-blue-600 dark:text-blue-400">
                      v{updateInfo.latest_version}
                    </p>
                    {updateInfo.channel === 'beta' && (
                      <span className="px-1.5 py-0.2 text-[10px] font-semibold rounded bg-amber-100 dark:bg-amber-900/40 text-amber-700 dark:text-amber-300 border border-amber-200 dark:border-amber-800">
                        Beta
                      </span>
                    )}
                  </div>
                )}
              </div>
            </div>

            {(updateState === 'error' || updateState === 'ready' || updateState === 'manual') && (
              <button
                onClick={handleClose}
                className="
                  p-1 rounded-full 
                  text-gray-400 hover:text-gray-600 dark:text-gray-500 dark:hover:text-gray-300
                  hover:bg-black/5 dark:hover:bg-white/10
                  transition-all duration-200
                "
                aria-label={t('common.cancel')}
              >
                <X className="w-4 h-4" />
              </button>
            )}
          </div>

          {/* Status message */}
          <div className="mb-4">
            <p className="text-sm text-gray-600 dark:text-gray-300 leading-relaxed">
              {updateState === 'downloading' && t('update_notification.downloading')}
              {updateState === 'ready' && t('update_notification.restart_prompt')}
              {updateState === 'error' && `${t('update_notification.toast.failed')}`}
              {updateState === 'manual' && (
                navigator.language.startsWith('zh')
                  ? '检测到您当前运行的不是 AppImage 格式，自动更新仅支持 AppImage。请点击下方按钮手动下载更新。'
                  : 'We detected that you are not running the AppImage version. Auto-updates are only supported for AppImage. Please download the update manually.'
              )}
            </p>
          </div>

          {/* Progress bar during download */}
          {updateState === 'downloading' && (
            <div className="mb-4">
              <div className="w-full bg-gray-200 dark:bg-base-200 rounded-full h-2">
                <div
                  className="bg-gradient-to-r from-blue-500 to-purple-600 h-2 rounded-full transition-all duration-300"
                  style={{ width: `${downloadProgress}%` }}
                />
              </div>
              <div className="flex items-center justify-between mt-1">
                <p className="text-xs text-gray-500">{downloadProgress}%</p>
                <Loader2 className="w-3 h-3 animate-spin text-blue-500" />
              </div>
            </div>
          )}

          {/* Restart button when ready */}
          {updateState === 'ready' && (
            <div className="flex gap-2">
              <button
                onClick={handleRestart}
                className="
                  flex-1 group/btn
                  relative overflow-hidden
                  bg-gradient-to-r from-green-600 to-emerald-600 hover:from-green-500 hover:to-emerald-500
                  text-white font-medium
                  py-2.5 px-4 rounded-xl
                  shadow-lg shadow-green-500/25
                  transition-all duration-300
                  flex items-center justify-center gap-2
                  active:scale-[0.98]
                "
              >
                <RotateCcw className="w-4 h-4" />
                <span>{t('update_notification.btn_restart')}</span>
                <div className="absolute inset-0 -translate-x-full group-hover/btn:animate-[shimmer_1.5s_infinite] bg-gradient-to-r from-transparent via-white/20 to-transparent z-20 pointer-events-none" />
              </button>
              <button
                onClick={handleClose}
                className="
                  px-3 py-2.5 rounded-xl
                  text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200
                  hover:bg-black/5 dark:hover:bg-white/10
                  transition-all duration-200
                  text-sm font-medium
                "
              >
                {t('update_notification.btn_later')}
              </button>
            </div>
          )}

          {/* Manual download button */}
          {updateState === 'manual' && (
            <div className="flex gap-2">
              <button
                onClick={async () => {
                  if (updateInfo) {
                    try {
                      const { openUrl } = await import('@tauri-apps/plugin-opener');
                      await openUrl(updateInfo.download_url);
                    } catch (e) {
                      window.open(updateInfo.download_url, '_blank', 'noopener,noreferrer');
                    }
                  }
                }}
                className="
                  flex-1 group/btn
                  relative overflow-hidden
                  bg-gradient-to-r from-blue-600 to-purple-600 hover:from-blue-500 hover:to-purple-500
                  text-white font-medium
                  py-2.5 px-4 rounded-xl
                  shadow-lg shadow-blue-500/25
                  transition-all duration-300
                  flex items-center justify-center gap-2
                  active:scale-[0.98]
                "
              >
                <span>{navigator.language.startsWith('zh') ? '手动下载' : 'Download Manually'}</span>
              </button>
              <button
                onClick={handleClose}
                className="
                  px-3 py-2.5 rounded-xl
                  text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200
                  hover:bg-black/5 dark:hover:bg-white/10
                  transition-all duration-200
                  text-sm font-medium
                "
              >
                {t('update_notification.btn_later')}
              </button>
            </div>
          )}

          {/* Error state — retry button */}
          {updateState === 'error' && (
            <button
              onClick={() => {
                downloadStarted.current = false;
                setUpdateState('checking');
                setDownloadProgress(0);
                checkAndDownload();
              }}
              className="
                w-full
                bg-gradient-to-r from-blue-600 to-purple-600 hover:from-blue-500 hover:to-purple-500
                text-white font-medium
                py-2.5 px-4 rounded-xl
                shadow-lg shadow-blue-500/25
                transition-all duration-300
                flex items-center justify-center gap-2
                active:scale-[0.98]
              "
            >
              <RotateCcw className="w-4 h-4" />
              <span>{t('common.retry')}</span>
            </button>
          )}
        </div>
      </div>
    </div>
  );
};
