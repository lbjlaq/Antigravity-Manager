// File: src/features/security/api/queries.ts
// React Query hooks for security

import { useQuery } from '@tanstack/react-query';
import { invoke } from '@/shared/api';
import { securityKeys } from './keys';

// Re-export types from entities for convenience
export type {
  IpBlacklistEntry,
  IpWhitelistEntry,
  AccessLogEntry,
  SecurityStats,
  SecurityMonitorConfig,
  BlacklistConfig,
  WhitelistConfig,
  AccessLogConfig,
} from '@/entities/security';

import type {
  IpBlacklistEntry,
  IpWhitelistEntry,
  AccessLogEntry,
  SecurityMonitorConfig,
  GetAccessLogsRequest,
} from '@/entities/security';

// Query hooks
export function useBlacklist() {
  return useQuery({
    queryKey: securityKeys.blacklist(),
    queryFn: () => invoke<IpBlacklistEntry[]>('security_get_blacklist'),
  });
}

export function useWhitelist() {
  return useQuery({
    queryKey: securityKeys.whitelist(),
    queryFn: () => invoke<IpWhitelistEntry[]>('security_get_whitelist'),
  });
}

export function useAccessLogs(filters?: Partial<GetAccessLogsRequest>) {
  return useQuery({
    queryKey: securityKeys.accessLogs(filters),
    // [FIX] Pass filters directly as 'request' parameter (Rust expects GetAccessLogsRequest)
    queryFn: () => invoke<AccessLogEntry[]>('security_get_access_logs', filters ?? {}),
  });
}

export function useSecurityConfig() {
  return useQuery({
    queryKey: securityKeys.settings(),
    queryFn: () => invoke<SecurityMonitorConfig>('get_security_config'),
  });
}

// Legacy alias for backward compatibility
export const useSecuritySettings = useSecurityConfig;
