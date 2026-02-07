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
  IpTokenStats,
} from '@/entities/security';

import type {
  IpBlacklistEntry,
  IpWhitelistEntry,
  AccessLogEntry,
  SecurityMonitorConfig,
  SecurityStats,
  IpTokenStats,
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
    queryFn: () => invoke<AccessLogEntry[]>('security_get_access_logs', filters ?? {}),
  });
}

export function useSecurityConfig() {
  return useQuery({
    queryKey: securityKeys.settings(),
    queryFn: () => invoke<SecurityMonitorConfig>('get_security_config'),
  });
}

// Security statistics
export function useSecurityStats() {
  return useQuery({
    queryKey: securityKeys.stats(),
    queryFn: () => invoke<SecurityStats>('security_get_stats'),
    refetchInterval: 30000, // Auto-refresh every 30s
  });
}

// IP Token usage statistics
export function useIpTokenStats(hours: number = 24) {
  return useQuery({
    queryKey: securityKeys.ipTokenStats(hours),
    queryFn: () => invoke<IpTokenStats[]>('security_get_ip_token_stats', { limit: 50, hours }),
    refetchInterval: 60000, // Auto-refresh every 60s
  });
}

// Legacy alias for backward compatibility
export const useSecuritySettings = useSecurityConfig;
