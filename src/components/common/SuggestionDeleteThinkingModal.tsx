import React, { useEffect, useState, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import ModalDialog from './ModalDialog';
import { useConfigStore } from '../../stores/useConfigStore';
import { request } from '../../utils/request';
import { showToast } from './ToastContainer';

// ============================================================================
// 开发者发版控制项 (Release Control Configuration)
// ============================================================================
// 当你需要发布新版本并控制是否提醒用户删除思考块缓存时，修改以下两个配置项：
//
// 1. SUGGESTION_DELETE_THINKING_STORE:
//    - true:  开启本版本建议（若检测到老用户升级且有旧缓存，会弹窗提醒一次）
//    - false: 关闭建议（任何用户升级都不会弹窗打扰）
export const SUGGESTION_DELETE_THINKING_STORE = false;

// 2. SUGGESTION_TARGET_VERSION:
//    - 建议清理的目标版本号 / 批次号（例如 "4.8.2"）。
//    - 只要你将此版本号更新为当前发布的新版本，老用户升级后就会获得一次弹窗建议。
//    - 用户无论点击【立即删除】还是【暂不删除】，本地均会记录已确认此版本，绝不重复弹窗。
export const SUGGESTION_TARGET_VERSION = '4.8.1';

export const SuggestionDeleteThinkingModal: React.FC = () => {
  const { t } = useTranslation();
  const { config, saveConfig } = useConfigStore();
  const [isOpen, setIsOpen] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const checkedRef = useRef(false);

  useEffect(() => {
    if (!config || checkedRef.current) return;

    const checkSuggestion = async () => {
      checkedRef.current = true;

      // 1. 若开发者在本版本未开启建议删除，直接退出
      if (!SUGGESTION_DELETE_THINKING_STORE) {
        return;
      }

      // 2. 若用户在此版本已经做出过选择（已清理或已取消），永久不再弹窗骚扰
      if (config.dismissed_thinking_cleanup_version === SUGGESTION_TARGET_VERSION) {
        return;
      }

      // 3. 检查当前 SQLite 数据库中是否存在历史思考块记录
      try {
        const res = await request<number | { count?: number }>('get_thinking_store_count');
        const count = typeof res === 'number' ? res : res?.count ?? 0;

        if (count === 0) {
          // 纯新安装用户或当前无旧思考块记录，静默标记为当前版本已对齐，避免弹窗打扰新用户
          await saveConfig(
            {
              ...config,
              suggestion_delete_thinking_store: true,
              thinking_cleanup_dismissed: true,
              dismissed_thinking_cleanup_version: SUGGESTION_TARGET_VERSION,
            },
            true
          );
          return;
        }

        // 老用户且本地存在历史旧数据，延迟展示弹窗
        setTimeout(() => {
          setIsOpen(true);
        }, 1200);
      } catch (err) {
        console.warn('[SuggestionDeleteThinking] Failed to query thinking count:', err);
      }
    };

    checkSuggestion();
  }, [config, saveConfig]);

  const handleConfirm = async () => {
    if (!config) return;
    setIsDeleting(true);
    try {
      await request('clear_thinking_store');
      await saveConfig(
        {
          ...config,
          suggestion_delete_thinking_store: true,
          thinking_cleanup_dismissed: true,
          dismissed_thinking_cleanup_version: SUGGESTION_TARGET_VERSION,
        },
        true
      );
      showToast(t('suggestion_delete_thinking.success'), 'success');
      setIsOpen(false);
    } catch (err: any) {
      console.error('[SuggestionDeleteThinking] Clear error:', err);
      showToast(String(err), 'error');
    } finally {
      setIsDeleting(false);
    }
  };

  const handleCancel = async () => {
    if (!config) return;
    try {
      // 写入建议为 false，并记录用户已确认该版本，下次启动绝不再弹窗
      await saveConfig(
        {
          ...config,
          suggestion_delete_thinking_store: false,
          thinking_cleanup_dismissed: true,
          dismissed_thinking_cleanup_version: SUGGESTION_TARGET_VERSION,
        },
        true
      );
    } catch (err) {
      console.warn('[SuggestionDeleteThinking] Save dismissed state error:', err);
    }
    setIsOpen(false);
  };

  if (!isOpen) return null;

  return (
    <ModalDialog
      isOpen={isOpen}
      title={t('suggestion_delete_thinking.title')}
      message={t('suggestion_delete_thinking.message')}
      type="confirm"
      isDestructive={true}
      isLoading={isDeleting}
      confirmText={t('suggestion_delete_thinking.confirm')}
      cancelText={t('suggestion_delete_thinking.cancel')}
      onConfirm={handleConfirm}
      onCancel={handleCancel}
    />
  );
};

export default SuggestionDeleteThinkingModal;
