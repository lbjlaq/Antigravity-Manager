import React, { useEffect, useState } from 'react';
import { X, Sparkles } from 'lucide-react';
import { request as invoke } from '../utils/request';
import { useTranslation } from 'react-i18next';

interface UpdateInfo {
  has_update: boolean;
  latest_version: string;
  current_version: string;
  download_url: string;
  source?: string;
}

interface UpdateNotificationProps {
  onClose: () => void;
}

export const UpdateNotification: React.FC<UpdateNotificationProps> = ({ onClose }) => {
  const { t } = useTranslation();
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [isVisible, setIsVisible] = useState(false);
  const [isClosing, setIsClosing] = useState(false);

  useEffect(() => {
    checkUpdate();
  }, []);

  const checkUpdate = async () => {
    try {
      const info = await invoke<UpdateInfo>('check_for_updates');
      if (!info.has_update) {
        onClose();
        return;
      }
      setUpdateInfo(info);
      setTimeout(() => setIsVisible(true), 100);
    } catch {
      onClose();
    }
  };

  const handleClose = () => {
    setIsClosing(true);
    setTimeout(() => {
      setIsVisible(false);
      onClose();
    }, 300);
  };

  if (!updateInfo || !isVisible) return null;

  return (
    <div className={`fixed bottom-6 right-6 z-[9999] transition-all duration-300 ${isClosing ? 'opacity-0 translate-y-4' : 'opacity-100 translate-y-0'}`}>
      <div className="bg-white dark:bg-base-100 shadow-2xl border border-gray-100 dark:border-base-200 rounded-2xl p-5 max-w-sm flex items-start gap-3">
        <div className="p-2 rounded-xl bg-gradient-to-br from-blue-500 to-purple-600 text-white shadow-lg shadow-blue-500/25">
          <Sparkles size={18} />
        </div>
        <div className="flex-1">
          <h4 className="text-sm font-bold text-gray-900 dark:text-base-content">
            {t('update_notification.title', '发现新版本')}
            <span className="text-xs ml-1.5 px-2 py-0.5 rounded-full bg-blue-500/10 text-blue-600 dark:text-blue-400 font-medium">
              v{updateInfo.latest_version}
            </span>
          </h4>
          <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
            {updateInfo.current_version} → {updateInfo.latest_version}
          </p>
          <div className="flex gap-2 mt-3">
            <button
              onClick={() => window.open(updateInfo.download_url, '_blank')}
              className="px-3 py-1.5 bg-blue-600 hover:bg-blue-700 text-white text-xs font-medium rounded-lg transition-colors shadow-sm"
            >
              {t('common.download', '下载更新')}
            </button>
            <button
              onClick={handleClose}
              className="px-3 py-1.5 text-gray-500 hover:text-gray-700 dark:text-gray-400 text-xs font-medium"
            >
              {t('common.close', '关闭')}
            </button>
          </div>
        </div>
        <button onClick={handleClose} className="text-gray-400 hover:text-gray-600 p-1">
          <X size={16} />
        </button>
      </div>
    </div>
  );
};
