import React, { useEffect, useState } from 'react';
import { X, Sparkles, GitCompare, Copy, ExternalLink } from 'lucide-react';
import { request as invoke } from '../utils/request';
import { useTranslation } from 'react-i18next';
import { showToast } from './common/ToastContainer';
import { copyToClipboard } from '../utils/clipboard';

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

type UpdateState = 'checking' | 'custom_guide' | 'none';

interface UpdateNotificationProps {
  onClose: () => void;
}

export const UpdateNotification: React.FC<UpdateNotificationProps> = ({ onClose }) => {
  const { t } = useTranslation();
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [isVisible, setIsVisible] = useState(false);
  const [isClosing, setIsClosing] = useState(false);
  const [updateState, setUpdateState] = useState<UpdateState>('checking');

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

      // 独立定制版逻辑：直接进入定制版更新指引态，绝不下载官方二进制破坏本地自研资产
      setUpdateState('custom_guide');
      setTimeout(() => setIsVisible(true), 100);
    } catch (error) {
      const errorMsg = error instanceof Error ? error.message : String(error);
      console.error('Update check failed:', errorMsg);
      onClose();
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
                <Sparkles className="w-4 h-4 text-white" />
              </div>
              <div>
                <h3 className="font-bold text-gray-800 dark:text-white leading-tight">
                  {t('update_notification.title')}
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

            {updateState === 'custom_guide' && (
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
              {navigator.language.startsWith('zh')
                ? `上游官方已发布新版本 v${updateInfo?.latest_version}（当前定制版: v${updateInfo?.current_version}），建议拉取上游代码合并更新。`
                : `Upstream v${updateInfo?.latest_version} released (Current: v${updateInfo?.current_version}). Review diff to update.`}
            </p>
          </div>

          {/* Custom guide action buttons */}
          {updateState === 'custom_guide' && (
            <div className="flex flex-col gap-2">
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => {
                    if (updateInfo) {
                      const diffUrl = `https://github.com/lbjlaq/Antigravity-Manager/compare/v${updateInfo.current_version}...v${updateInfo.latest_version}`;
                      window.open(diffUrl, '_blank', 'noopener,noreferrer');
                    }
                  }}
                  className="
                    flex-1
                    bg-blue-600 hover:bg-blue-500
                    text-white font-medium text-xs
                    py-2 px-3 rounded-xl
                    transition-all duration-200
                    flex items-center justify-center gap-1.5
                    shadow-sm active:scale-95 cursor-pointer
                  "
                >
                  <GitCompare className="w-3.5 h-3.5" />
                  <span>查看代码差异</span>
                </button>
                <button
                  type="button"
                  onClick={async () => {
                    await copyToClipboard('git fetch upstream && git merge upstream/main');
                    showToast('已复制 Git 合并命令到剪贴板', 'success');
                  }}
                  className="
                    flex-1
                    bg-purple-600 hover:bg-purple-500
                    text-white font-medium text-xs
                    py-2 px-3 rounded-xl
                    transition-all duration-200
                    flex items-center justify-center gap-1.5
                    shadow-sm active:scale-95 cursor-pointer
                  "
                >
                  <Copy className="w-3.5 h-3.5" />
                  <span>复制合并命令</span>
                </button>
              </div>
              <div className="flex gap-2">
                <a
                  href={updateInfo?.download_url || `https://github.com/lbjlaq/Antigravity-Manager/releases/tag/v${updateInfo?.latest_version}`}
                  target="_blank"
                  rel="noreferrer"
                  className="
                    flex-1
                    bg-gray-100 hover:bg-gray-200 dark:bg-base-200 dark:hover:bg-base-300
                    text-gray-700 dark:text-gray-200 font-medium text-xs
                    py-1.5 px-3 rounded-xl
                    transition-all duration-200
                    flex items-center justify-center gap-1.5
                    border border-gray-200 dark:border-base-300
                  "
                >
                  <span>查看Releases</span>
                  <ExternalLink className="w-3 h-3 text-gray-400" />
                </a>
                <button
                  type="button"
                  onClick={handleClose}
                  className="
                    px-3 py-1.5 rounded-xl
                    text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200
                    hover:bg-black/5 dark:hover:bg-white/10
                    transition-all duration-200
                    text-xs font-medium cursor-pointer
                  "
                >
                  {t('update_notification.btn_later', { defaultValue: '稍后再说' })}
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
