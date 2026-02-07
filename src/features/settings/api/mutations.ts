// File: src/features/settings/api/mutations.ts
// React Query mutations for settings

import { useMutation, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@/shared/api';
import type { AppConfig } from '@/entities/config';
import { settingsKeys } from './keys';
import { showToast } from '@/shared/ui';
import { useTranslation } from 'react-i18next';

// Mutations
export function useSaveConfig() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (config: AppConfig) => invoke<void>('save_config', { config }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: settingsKeys.config() });
      showToast(t('common.saved', 'Settings saved'), 'success');
    },
    onError: (error) => {
      showToast(`${t('common.error', 'Error')}: ${error}`, 'error');
    },
  });
}

export function useSaveUpdateSettings() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (settings: { auto_check: boolean; check_interval_hours: number }) =>
      invoke<void>('save_update_settings', { settings }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: settingsKeys.updateSettings() });
    },
  });
}

export function useSaveHttpApiSettings() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (settings: { enabled: boolean; port: number; api_key?: string }) =>
      invoke<void>('save_http_api_settings', { settings }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: settingsKeys.httpApiSettings() });
      showToast(t('settings.http_api.saved', 'HTTP API settings saved'), 'success');
    },
  });
}

export function useToggleAutoLaunch() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (enable: boolean) => invoke<void>('toggle_auto_launch', { enable }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: settingsKeys.autoLaunch() });
    },
  });
}

export function useOpenDataFolder() {
  return useMutation({
    mutationFn: () => invoke<void>('open_data_folder'),
  });
}

export function useCheckForUpdates() {
  const { t } = useTranslation();

  return useMutation({
    mutationFn: () => invoke<{ has_update: boolean; latest_version: string }>('check_for_updates'),
    onSuccess: (data) => {
      if (data.has_update) {
        showToast(t('settings.update_available', `Update available: v${data.latest_version}`), 'info');
      } else {
        showToast(t('settings.up_to_date', 'You are up to date'), 'success');
      }
    },
  });
}
