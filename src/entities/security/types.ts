// File: src/entities/security/types.ts
// Security domain types

export interface IpBlacklistEntry {
  id: number;
  ipPattern: string;
  reason: string;
  createdAt: number;
  expiresAt: number | null;
  createdBy: string;
  hitCount: number;
}

export interface IpWhitelistEntry {
  id: number;
  ipPattern: string;
  description: string;
  createdAt: number;
  createdBy: string;
}

export interface AccessLogEntry {
  id: number;
  ipAddress: string;
  path: string;
  method: string;
  statusCode: number;
  blocked: boolean;
  blockReason: string | null;
  timestamp: number;
  userAgent: string | null;
}

export interface SecurityStats {
  totalRequests: number;
  blockedRequests: number;
  uniqueIps: number;
  blacklistCount: number;
  whitelistCount: number;
  topBlockedIps: [string, number][];
}

export interface BlacklistConfig {
  enabled: boolean;
  autoBlockThreshold: number;
  autoBlockDuration: number;
}

export interface WhitelistConfig {
  enabled: boolean;
  strictMode: boolean;
  priorityOverBlacklist: boolean;
}

export interface AccessLogConfig {
  enabled: boolean;
  retentionDays: number;
  blockedOnly: boolean;
}

export interface SecurityMonitorConfig {
  blacklist: BlacklistConfig;
  whitelist: WhitelistConfig;
  accessLog: AccessLogConfig;
}

export interface AddToBlacklistRequest {
  ipPattern: string;
  reason: string;
  expiresInSeconds?: number;
  createdBy?: string;
}

export interface AddToWhitelistRequest {
  ipPattern: string;
  description?: string;
  createdBy?: string;
}

export interface GetAccessLogsRequest {
  limit?: number;
  offset?: number;
  blockedOnly?: boolean;
  ipFilter?: string;
}

export interface OperationResult {
  success: boolean;
  message: string;
  id?: number;
}

// Helper type for IP list type
export type IpListType = 'blacklist' | 'whitelist';

// Security tab type
export type SecurityTab = 'blacklist' | 'whitelist' | 'logs' | 'settings';
