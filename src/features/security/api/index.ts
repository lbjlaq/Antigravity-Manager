// File: src/features/security/api/index.ts
// Public API for security feature

export { securityKeys } from './keys';

export {
  useBlacklist,
  useWhitelist,
  useAccessLogs,
  useSecurityConfig,
  useSecuritySettings, // Legacy alias
  useSecurityStats,
  useIpTokenStats,
} from './queries';

// Re-export types from entities
export type {
  IpBlacklistEntry,
  IpWhitelistEntry,
  AccessLogEntry,
  SecurityStats,
  SecurityMonitorConfig,
  BlacklistConfig,
  WhitelistConfig,
  AccessLogConfig,
  AddToBlacklistRequest,
  AddToWhitelistRequest,
  GetAccessLogsRequest,
  OperationResult,
} from '@/entities/security';

export {
  useAddToBlacklist,
  useAddToWhitelist,
  useRemoveFromBlacklist,
  useRemoveFromBlacklistById,
  useRemoveFromWhitelist,
  useRemoveFromWhitelistById,
  useClearAccessLogs,
  useCleanupAccessLogs,
  useUpdateSecurityConfig,
  useUpdateSecuritySettings, // Legacy alias
} from './mutations';
