// File: src/features/settings/api/queries.ts
// React Query hooks for settings

import { useQuery } from '@tanstack/react-query';
import { invoke } from '@/shared/api';
import type { AppConfig } from '@/entities/config';
import { settingsKeys } from './keys';

// Types
export interface UpdateSettings {
  auto_check: boolean;
  check_interval_hours: number;
}

export interface HttpApiSettings {
  enabled: boolean;
  port: number;
  api_key?: string;
}

// Query hooks
export function useConfig() {
  return useQuery({
    queryKey: settingsKeys.config(),
    queryFn: () => invoke<AppConfig>('load_config'),
  });
}

export function useUpdateSettings() {
  return useQuery({
    queryKey: settingsKeys.updateSettings(),
    queryFn: () => invoke<UpdateSettings>('get_update_settings'),
  });
}

export function useHttpApiSettings() {
  return useQuery({
    queryKey: settingsKeys.httpApiSettings(),
    queryFn: () => invoke<HttpApiSettings>('get_http_api_settings'),
  });
}

export function useAutoLaunchEnabled() {
  return useQuery({
    queryKey: settingsKeys.autoLaunch(),
    queryFn: () => invoke<boolean>('is_auto_launch_enabled'),
  });
}

export function useDataDirPath() {
  return useQuery({
    queryKey: settingsKeys.dataDir(),
    queryFn: () => invoke<string>('get_data_dir_path'),
  });
}
