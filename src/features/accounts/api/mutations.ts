// File: src/features/accounts/api/mutations.ts
// React Query mutations for accounts

import { useMutation, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@/shared/api';
import type { Account, QuotaData, DeviceProfile } from '@/entities/account';
import { accountKeys } from './keys';
import { showToast } from '@/shared/ui';
import { useTranslation } from 'react-i18next';

// Response types
export interface RefreshStats {
  total: number;
  success: number;
  failed: number;
  details: string[];
}

// Service functions
async function addAccount(email: string, refreshToken: string): Promise<Account> {
  return await invoke<Account>('add_account', { email, refreshToken });
}

async function deleteAccount(accountId: string): Promise<void> {
  return await invoke<void>('delete_account', { accountId });
}

async function deleteAccounts(accountIds: string[]): Promise<void> {
  return await invoke<void>('delete_accounts', { accountIds });
}

async function switchAccount(accountId: string): Promise<void> {
  return await invoke<void>('switch_account', { accountId });
}

async function fetchAccountQuota(accountId: string): Promise<QuotaData> {
  return await invoke<QuotaData>('fetch_account_quota', { accountId });
}

async function refreshAllQuotas(): Promise<RefreshStats> {
  return await invoke<RefreshStats>('refresh_all_quotas');
}

async function reorderAccounts(accountIds: string[]): Promise<void> {
  return await invoke<void>('reorder_accounts', { accountIds });
}

async function toggleProxyStatus(accountId: string, enable: boolean, reason?: string): Promise<void> {
  return await invoke<void>('toggle_proxy_status', { accountId, enable, reason });
}

async function warmUpAllAccounts(): Promise<string> {
  return await invoke<string>('warm_up_all_accounts');
}

async function warmUpAccount(accountId: string): Promise<string> {
  return await invoke<string>('warm_up_account', { accountId });
}

async function startOAuthLogin(): Promise<Account> {
  return await invoke<Account>('start_oauth_login');
}

async function completeOAuthLogin(): Promise<Account> {
  return await invoke<Account>('complete_oauth_login');
}

async function cancelOAuthLogin(): Promise<void> {
  return await invoke<void>('cancel_oauth_login');
}

async function importV1Accounts(): Promise<Account[]> {
  return await invoke<Account[]>('import_v1_accounts');
}

async function importFromDb(): Promise<Account> {
  return await invoke<Account>('import_from_db');
}

async function importFromCustomDb(path: string): Promise<Account> {
  return await invoke<Account>('import_custom_db', { path });
}

async function syncAccountFromDb(): Promise<Account | null> {
  return await invoke<Account | null>('sync_account_from_db');
}

async function bindDeviceProfile(accountId: string, mode: 'capture' | 'generate'): Promise<DeviceProfile> {
  return await invoke<DeviceProfile>('bind_device_profile', { accountId, mode });
}

async function bindDeviceProfileWithProfile(accountId: string, profile: DeviceProfile): Promise<DeviceProfile> {
  return await invoke<DeviceProfile>('bind_device_profile_with_profile', { accountId, profile });
}

async function previewGenerateProfile(): Promise<DeviceProfile> {
  return await invoke<DeviceProfile>('preview_generate_profile');
}

async function restoreOriginalDevice(): Promise<string> {
  return await invoke<string>('restore_original_device');
}

async function restoreDeviceVersion(accountId: string, versionId: string): Promise<DeviceProfile> {
  return await invoke<DeviceProfile>('restore_device_version', { accountId, versionId });
}

async function deleteDeviceVersion(accountId: string, versionId: string): Promise<void> {
  return await invoke<void>('delete_device_version', { accountId, versionId });
}

// Mutation hooks
export function useAddAccount() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: ({ email, refreshToken }: { email: string; refreshToken: string }) =>
      addAccount(email, refreshToken),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
      showToast(t('dashboard.toast.add_success', 'Account added successfully'), 'success');
    },
    onError: (error) => {
      showToast(`${t('dashboard.toast.add_error', 'Failed to add account')}: ${error}`, 'error');
    },
  });
}

export function useDeleteAccount() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: deleteAccount,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
      showToast(t('accounts.toast.delete_success', 'Account deleted'), 'success');
    },
    onError: (error) => {
      showToast(`${t('accounts.toast.delete_error', 'Failed to delete')}: ${error}`, 'error');
    },
  });
}

export function useDeleteAccounts() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: deleteAccounts,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
      showToast(t('accounts.toast.bulk_delete_success', 'Accounts deleted'), 'success');
    },
    onError: (error) => {
      showToast(`${t('accounts.toast.delete_error', 'Failed to delete')}: ${error}`, 'error');
    },
  });
}

export function useSwitchAccount() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: switchAccount,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: accountKeys.current() });
      queryClient.invalidateQueries({ queryKey: accountKeys.lists() });
    },
  });
}

export function useRefreshQuota() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: fetchAccountQuota,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
      showToast(t('dashboard.toast.refresh_success', 'Quota refreshed'), 'success');
    },
    onError: (error) => {
      showToast(`${t('dashboard.toast.refresh_error', 'Failed to refresh')}: ${error}`, 'error');
    },
  });
}

export function useRefreshAllQuotas() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: refreshAllQuotas,
    onSuccess: (stats) => {
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
      showToast(
        t('accounts.toast.refresh_all_success', { success: stats.success, total: stats.total }),
        'success'
      );
    },
    onError: (error) => {
      showToast(`${t('accounts.toast.refresh_all_error', 'Refresh failed')}: ${error}`, 'error');
    },
  });
}

export function useReorderAccounts() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: reorderAccounts,
    onMutate: async (newOrder: string[]) => {
      // Cancel any outgoing refetches to prevent overwriting optimistic update
      await queryClient.cancelQueries({ queryKey: accountKeys.lists() });
      
      // Snapshot the previous value
      const previousAccounts = queryClient.getQueryData<Account[]>(accountKeys.lists());
      
      // Optimistically update to the new order
      if (previousAccounts) {
        const accountMap = new Map(previousAccounts.map(a => [a.id, a]));
        const reorderedAccounts = newOrder
          .map(id => accountMap.get(id))
          .filter((a): a is Account => a !== undefined);
        
        // Include any accounts not in newOrder at the end (shouldn't happen, but safety)
        const remainingAccounts = previousAccounts.filter(a => !newOrder.includes(a.id));
        
        queryClient.setQueryData(accountKeys.lists(), [...reorderedAccounts, ...remainingAccounts]);
      }
      
      return { previousAccounts };
    },
    onError: (_error, _variables, context) => {
      // Rollback to previous value on error
      if (context?.previousAccounts) {
        queryClient.setQueryData(accountKeys.lists(), context.previousAccounts);
      }
    },
    // Don't invalidate immediately - the optimistic update is already correct
    // Backend has already persisted the order, so no need to refetch
  });
}

export function useToggleProxyStatus() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ accountId, enable, reason }: { accountId: string; enable: boolean; reason?: string }) =>
      toggleProxyStatus(accountId, enable, reason),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
    },
  });
}

export function useWarmUpAccounts() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: warmUpAllAccounts,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
      showToast(t('accounts.toast.warmup_success', 'Warmup completed'), 'success');
    },
    onError: (error) => {
      showToast(`${t('accounts.toast.warmup_error', 'Warmup failed')}: ${error}`, 'error');
    },
  });
}

export function useWarmUpAccount() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: warmUpAccount,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
    },
  });
}

export function useStartOAuthLogin() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: startOAuthLogin,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
    },
  });
}

export function useCompleteOAuthLogin() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: completeOAuthLogin,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
    },
  });
}

export function useCancelOAuthLogin() {
  return useMutation({
    mutationFn: cancelOAuthLogin,
  });
}

export function useImportV1Accounts() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: importV1Accounts,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
      showToast(t('accounts.toast.import_success', 'Import completed'), 'success');
    },
    onError: (error) => {
      showToast(`${t('accounts.toast.import_error', 'Import failed')}: ${error}`, 'error');
    },
  });
}

export function useImportFromDb() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: importFromDb,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
      showToast(t('accounts.toast.import_success', 'Import completed'), 'success');
    },
    onError: (error) => {
      showToast(`${t('accounts.toast.import_error', 'Import failed')}: ${error}`, 'error');
    },
  });
}

export function useImportFromCustomDb() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: importFromCustomDb,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
      showToast(t('accounts.toast.import_success', 'Import completed'), 'success');
    },
    onError: (error) => {
      showToast(`${t('accounts.toast.import_error', 'Import failed')}: ${error}`, 'error');
    },
  });
}

export function useSyncAccountFromDb() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: syncAccountFromDb,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
    },
  });
}

export function useBindDeviceProfile() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ accountId, mode }: { accountId: string; mode: 'capture' | 'generate' }) =>
      bindDeviceProfile(accountId, mode),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: accountKeys.deviceProfiles(variables.accountId) });
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
    },
  });
}

export function useBindDeviceProfileWithProfile() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ accountId, profile }: { accountId: string; profile: DeviceProfile }) =>
      bindDeviceProfileWithProfile(accountId, profile),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: accountKeys.deviceProfiles(variables.accountId) });
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
    },
  });
}

export function usePreviewGenerateProfile() {
  return useMutation({
    mutationFn: previewGenerateProfile,
  });
}

export function useRestoreOriginalDevice() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: restoreOriginalDevice,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
    },
  });
}

export function useRestoreDeviceVersion() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ accountId, versionId }: { accountId: string; versionId: string }) =>
      restoreDeviceVersion(accountId, versionId),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: accountKeys.deviceProfiles(variables.accountId) });
      queryClient.invalidateQueries({ queryKey: accountKeys.all });
    },
  });
}

export function useDeleteDeviceVersion() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ accountId, versionId }: { accountId: string; versionId: string }) =>
      deleteDeviceVersion(accountId, versionId),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: accountKeys.deviceProfiles(variables.accountId) });
    },
  });
}
