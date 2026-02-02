// File: src/features/security/api/mutations.ts
// React Query mutations for security

import { useMutation, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@/shared/api';
import { securityKeys } from './keys';
import { showToast } from '@/shared/ui';
import { useTranslation } from 'react-i18next';
import type { 
  SecurityMonitorConfig, 
  AddToBlacklistRequest, 
  AddToWhitelistRequest,
  OperationResult,
} from '@/entities/security';

// Mutations
export function useAddToBlacklist() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    // [FIX] Pass request object directly without extra wrapper
    mutationFn: (data: AddToBlacklistRequest) =>
      invoke<OperationResult>('security_add_to_blacklist', data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: securityKeys.blacklist() });
      showToast(t('security.toast.added_blacklist', 'Added to blacklist'), 'success');
    },
    onError: (error) => {
      showToast(`Failed: ${error}`, 'error');
    },
  });
}

export function useAddToWhitelist() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    // [FIX] Pass request object directly without extra wrapper
    mutationFn: (data: AddToWhitelistRequest) =>
      invoke<OperationResult>('security_add_to_whitelist', data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: securityKeys.whitelist() });
      showToast(t('security.toast.added_whitelist', 'Added to whitelist'), 'success');
    },
  });
}

export function useRemoveFromBlacklist() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (ipPattern: string) => invoke<OperationResult>('security_remove_from_blacklist', { ipPattern }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: securityKeys.blacklist() });
      showToast(t('security.toast.removed', 'Removed'), 'success');
    },
  });
}

export function useRemoveFromBlacklistById() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (id: number) => invoke<OperationResult>('security_remove_from_blacklist_by_id', { id }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: securityKeys.blacklist() });
      showToast(t('security.toast.removed', 'Removed'), 'success');
    },
  });
}

export function useRemoveFromWhitelist() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (ipPattern: string) => invoke<OperationResult>('security_remove_from_whitelist', { ipPattern }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: securityKeys.whitelist() });
      showToast(t('security.toast.removed', 'Removed'), 'success');
    },
  });
}

export function useRemoveFromWhitelistById() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (id: number) => invoke<OperationResult>('security_remove_from_whitelist_by_id', { id }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: securityKeys.whitelist() });
      showToast(t('security.toast.removed', 'Removed'), 'success');
    },
  });
}

export function useClearAccessLogs() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: () => invoke<OperationResult>('security_clear_all_logs'),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: securityKeys.accessLogs() });
      showToast(t('security.toast.logs_cleared', 'Logs cleared'), 'success');
    },
  });
}

export function useCleanupAccessLogs() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (days: number) => invoke<OperationResult>('security_cleanup_logs', { days }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: securityKeys.accessLogs() });
      showToast(t('security.toast.logs_cleaned', 'Old logs cleaned'), 'success');
    },
  });
}

export function useUpdateSecurityConfig() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (config: SecurityMonitorConfig) =>
      invoke<void>('update_security_config', { config }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: securityKeys.settings() });
      showToast(t('common.saved', 'Settings saved'), 'success');
    },
  });
}

// Legacy alias for backward compatibility
export const useUpdateSecuritySettings = useUpdateSecurityConfig;
