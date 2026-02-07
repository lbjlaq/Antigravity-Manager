// File: src/features/accounts/api/queries.ts
// React Query hooks for accounts

import { useQuery } from '@tanstack/react-query';
import { invoke } from '@/shared/api';
import type { Account, QuotaData, DeviceProfile, DeviceProfileVersion } from '@/entities/account';
import { accountKeys } from './keys';

// Response types
interface ListAccountsResponse {
  accounts?: Account[];
}

export interface DeviceProfilesResponse {
  current_storage?: DeviceProfile;
  history?: DeviceProfileVersion[];
  baseline?: DeviceProfile;
}

// Service functions
async function listAccounts(): Promise<Account[]> {
  const response = await invoke<ListAccountsResponse | Account[]>('list_accounts');
  if (response && typeof response === 'object' && 'accounts' in response && Array.isArray(response.accounts)) {
    return response.accounts;
  }
  return (response as Account[]) || [];
}

async function getCurrentAccount(): Promise<Account | null> {
  return await invoke<Account | null>('get_current_account');
}

async function getDeviceProfiles(accountId: string): Promise<DeviceProfilesResponse> {
  return await invoke<DeviceProfilesResponse>('get_device_profiles', { accountId });
}

// Query hooks
export function useAccounts() {
  return useQuery({
    queryKey: accountKeys.lists(),
    queryFn: listAccounts,
    refetchInterval: 60_000, // Auto-refresh every 60 seconds
    refetchIntervalInBackground: false, // Only when tab is focused
    // Note: No select/sort here - order is preserved from backend (supports drag-and-drop reorder)
  });
}

export function useCurrentAccount() {
  return useQuery({
    queryKey: accountKeys.current(),
    queryFn: getCurrentAccount,
  });
}

export function useDeviceProfiles(accountId: string) {
  return useQuery({
    queryKey: accountKeys.deviceProfiles(accountId),
    queryFn: () => getDeviceProfiles(accountId),
    enabled: !!accountId,
  });
}

// Re-export types
export type { Account, QuotaData, DeviceProfile, DeviceProfileVersion };
