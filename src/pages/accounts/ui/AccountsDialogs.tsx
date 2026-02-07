// File: src/pages/accounts/ui/AccountsDialogs.tsx
import { memo } from 'react';
import { useTranslation } from 'react-i18next';

import { ModalDialog } from '@/shared/ui';
import { AccountDetailsDialog, DeviceFingerprintDialog } from '@/features/accounts/ui';
import type { Account } from '@/entities/account';

interface AccountsDialogsProps {
  // Details & Device dialogs
  detailsAccount: Account | null;
  onCloseDetails: () => void;
  deviceAccount: Account | null;
  onCloseDevice: () => void;

  // Delete dialog
  deleteConfirmId: string | null;
  isBatchDelete: boolean;
  selectedCount: number;
  onConfirmDelete: () => void;
  onCancelDelete: () => void;

  // Refresh dialog
  isRefreshConfirmOpen: boolean;
  onConfirmRefresh: () => void;
  onCancelRefresh: () => void;

  // Toggle proxy dialog
  toggleProxyConfirm: { accountId: string; enable: boolean } | null;
  onConfirmToggleProxy: () => void;
  onCancelToggleProxy: () => void;

  // Warmup dialog
  isWarmupConfirmOpen: boolean;
  onConfirmWarmup: () => void;
  onCancelWarmup: () => void;
}

export const AccountsDialogs = memo(function AccountsDialogs({
  detailsAccount,
  onCloseDetails,
  deviceAccount,
  onCloseDevice,
  deleteConfirmId,
  isBatchDelete,
  selectedCount,
  onConfirmDelete,
  onCancelDelete,
  isRefreshConfirmOpen,
  onConfirmRefresh,
  onCancelRefresh,
  toggleProxyConfirm,
  onConfirmToggleProxy,
  onCancelToggleProxy,
  isWarmupConfirmOpen,
  onConfirmWarmup,
  onCancelWarmup,
}: AccountsDialogsProps) {
  const { t } = useTranslation();

  return (
    <>
      <AccountDetailsDialog
        account={detailsAccount}
        onClose={onCloseDetails}
      />
      {deviceAccount && (
        <DeviceFingerprintDialog
          account={deviceAccount}
          onClose={onCloseDevice}
        />
      )}

      <ModalDialog
        isOpen={!!deleteConfirmId || isBatchDelete}
        title={isBatchDelete ? t('accounts.dialog.batch_delete_title') : t('accounts.dialog.delete_title')}
        message={isBatchDelete
          ? t('accounts.dialog.batch_delete_msg', { count: selectedCount })
          : t('accounts.dialog.delete_msg')
        }
        type="confirm"
        confirmText={t('common.delete')}
        isDestructive={true}
        onConfirm={onConfirmDelete}
        onCancel={onCancelDelete}
      />

      <ModalDialog
        isOpen={isRefreshConfirmOpen}
        title={selectedCount > 0 ? t('accounts.dialog.batch_refresh_title') : t('accounts.dialog.refresh_title')}
        message={selectedCount > 0
          ? t('accounts.dialog.batch_refresh_msg', { count: selectedCount })
          : t('accounts.dialog.refresh_msg')
        }
        type="confirm"
        confirmText={t('common.refresh')}
        isDestructive={false}
        onConfirm={onConfirmRefresh}
        onCancel={onCancelRefresh}
      />

      {toggleProxyConfirm && (
        <ModalDialog
          isOpen={!!toggleProxyConfirm}
          onCancel={onCancelToggleProxy}
          onConfirm={onConfirmToggleProxy}
          title={toggleProxyConfirm.enable ? t('accounts.dialog.enable_proxy_title') : t('accounts.dialog.disable_proxy_title')}
          message={toggleProxyConfirm.enable ? t('accounts.dialog.enable_proxy_msg') : t('accounts.dialog.disable_proxy_msg')}
        />
      )}

      <ModalDialog
        isOpen={isWarmupConfirmOpen}
        title={selectedCount > 0 ? t('accounts.dialog.batch_warmup_title', '批量手动预热') : t('accounts.dialog.warmup_all_title', '全量手动预热')}
        message={selectedCount > 0
          ? t('accounts.dialog.batch_warmup_msg', '确定要为选中的 {{count}} 个账号立即触发预热吗？', { count: selectedCount })
          : t('accounts.dialog.warmup_all_msg', '确定要立即为所有符合条件的账号触发预热任务吗？这将向 Google 服务发送极小流量。')
        }
        type="confirm"
        confirmText={t('accounts.warmup_now', '立即预热')}
        isDestructive={false}
        onConfirm={onConfirmWarmup}
        onCancel={onCancelWarmup}
      />
    </>
  );
});
