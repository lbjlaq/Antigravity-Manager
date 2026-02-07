// File: src/features/proxy/api/mutations.ts
// React Query mutations for proxy

import { useMutation, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@/shared/api';
import { proxyKeys } from './keys';
import { showToast } from '@/shared/ui';
import { useTranslation } from 'react-i18next';

// Mutations
export function useStartProxy() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: () => invoke<void>('start_proxy_service'),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: proxyKeys.status() });
      showToast(t('proxy.toast.started', 'Proxy started'), 'success');
    },
    onError: (error) => {
      showToast(`${t('proxy.toast.start_error', 'Failed to start')}: ${error}`, 'error');
    },
  });
}

export function useStopProxy() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: () => invoke<void>('stop_proxy_service'),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: proxyKeys.status() });
      showToast(t('proxy.toast.stopped', 'Proxy stopped'), 'info');
    },
    onError: (error) => {
      showToast(`${t('proxy.toast.stop_error', 'Failed to stop')}: ${error}`, 'error');
    },
  });
}

export function useGenerateApiKey() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: () => invoke<string>('generate_api_key'),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: proxyKeys.status() });
      showToast(t('proxy.toast.key_generated', 'API key generated'), 'success');
    },
  });
}

export function useUpdateModelMapping() {
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (mapping: Record<string, string>) => 
      invoke<void>('update_model_mapping', { mapping }),
    onSuccess: () => {
      showToast(t('proxy.toast.mapping_updated', 'Model mapping updated'), 'success');
    },
  });
}

export function useClearProxyLogs() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: () => invoke<void>('clear_proxy_logs'),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: proxyKeys.logs() });
      showToast(t('proxy.toast.logs_cleared', 'Logs cleared'), 'success');
    },
  });
}

export function useClearSessionBindings() {
  const { t } = useTranslation();

  return useMutation({
    mutationFn: () => invoke<void>('clear_proxy_session_bindings'),
    onSuccess: () => {
      showToast(t('proxy.toast.bindings_cleared', 'Session bindings cleared'), 'success');
    },
  });
}

export function useClearRateLimits() {
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (accountId?: string) => 
      accountId 
        ? invoke<void>('clear_proxy_rate_limit', { accountId })
        : invoke<void>('clear_all_proxy_rate_limits'),
    onSuccess: () => {
      showToast(t('proxy.toast.rate_limits_cleared', 'Rate limits cleared'), 'success');
    },
  });
}

export function useSetProxyMonitorEnabled() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (enabled: boolean) => 
      invoke<void>('set_proxy_monitor_enabled', { enabled }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: proxyKeys.status() });
    },
  });
}

// Cloudflared mutations
export function useInstallCloudflared() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: () => invoke<void>('cloudflared_install'),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: proxyKeys.cloudflared() });
      showToast(t('proxy.cloudflared.installed', 'Cloudflared installed'), 'success');
    },
    onError: (error) => {
      showToast(`Install failed: ${error}`, 'error');
    },
  });
}

export function useStartCloudflared() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: () => invoke<void>('cloudflared_start'),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: proxyKeys.cloudflared() });
      showToast(t('proxy.cloudflared.started', 'Tunnel started'), 'success');
    },
  });
}

export function useStopCloudflared() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: () => invoke<void>('cloudflared_stop'),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: proxyKeys.cloudflared() });
      showToast(t('proxy.cloudflared.stopped', 'Tunnel stopped'), 'info');
    },
  });
}

// CLI Sync mutations
export function useExecuteCliSync() {
  const { t } = useTranslation();

  return useMutation({
    mutationFn: () => invoke<void>('execute_cli_sync'),
    onSuccess: () => {
      showToast(t('proxy.cli.sync_success', 'CLI sync completed'), 'success');
    },
    onError: (error) => {
      showToast(`Sync failed: ${error}`, 'error');
    },
  });
}

export function useExecuteCliRestore() {
  const { t } = useTranslation();

  return useMutation({
    mutationFn: () => invoke<void>('execute_cli_restore'),
    onSuccess: () => {
      showToast(t('proxy.cli.restore_success', 'CLI restore completed'), 'success');
    },
  });
}
