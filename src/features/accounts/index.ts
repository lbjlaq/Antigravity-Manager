// File: src/features/accounts/index.ts
// Public API for accounts feature

// API layer (React Query hooks)
export {
  accountKeys,
  useAccounts,
  useCurrentAccount,
  useDeviceProfiles,
  useAddAccount,
  useDeleteAccount,
  useDeleteAccounts,
  useSwitchAccount,
  useRefreshQuota,
  useRefreshAllQuotas,
  useReorderAccounts,
  useToggleProxyStatus,
  useWarmUpAccounts,
  useWarmUpAccount,
  useStartOAuthLogin,
  useCompleteOAuthLogin,
  useCancelOAuthLogin,
  useImportV1Accounts,
  useImportFromDb,
  useImportFromCustomDb,
  useSyncAccountFromDb,
  useBindDeviceProfile,
  useBindDeviceProfileWithProfile,
  usePreviewGenerateProfile,
  useRestoreOriginalDevice,
  useRestoreDeviceVersion,
  useDeleteDeviceVersion,
  type RefreshStats,
  type DeviceProfilesResponse,
} from './api';

// Model layer (UI store)
export { useAccountsUI } from './model';

// Re-export entity types for convenience
export type { 
  Account, 
  TokenData, 
  QuotaData, 
  ModelQuota, 
  DeviceProfile, 
  DeviceProfileVersion 
} from '@/entities/account';

// UI components
export * from './ui';
