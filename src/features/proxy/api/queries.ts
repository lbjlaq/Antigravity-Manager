// File: src/features/proxy/api/queries.ts
// React Query hooks for proxy

import { useQuery } from '@tanstack/react-query';
import { invoke } from '@/shared/api';
import { proxyKeys } from './keys';

// Types
export interface ProxyStatus {
  running: boolean;
  port: number;
  api_key?: string;
  uptime_seconds?: number;
  total_requests?: number;
}

export interface ProxyStats {
  total_requests: number;
  success_requests: number;
  failed_requests: number;
  avg_latency_ms: number;
  requests_per_minute: number;
}

export interface ProxyLog {
  id: string;
  timestamp: number;
  method: string;
  path: string;
  status: number;
  latency_ms: number;
  account_id?: string;
  model?: string;
  error?: string;
}

export interface CloudflaredStatus {
  installed: boolean;
  version?: string;
  running: boolean;
  url?: string;
  error?: string;
}

// Query hooks
export function useProxyStatus() {
  return useQuery({
    queryKey: proxyKeys.status(),
    queryFn: () => invoke<ProxyStatus>('get_proxy_status'),
    refetchInterval: 5000, // Poll every 5 seconds
  });
}

export function useProxyStats() {
  return useQuery({
    queryKey: proxyKeys.stats(),
    queryFn: () => invoke<ProxyStats>('get_proxy_stats'),
    refetchInterval: 10000,
  });
}

export function useProxyLogs(filters?: { limit?: number; offset?: number; errorsOnly?: boolean; filter?: string }) {
  return useQuery({
    queryKey: proxyKeys.logs(filters),
    // [FIX] Backend expects separate parameters: filter, errors_only, limit, offset
    queryFn: () => invoke<ProxyLog[]>('get_proxy_logs_filtered', {
      filter: filters?.filter ?? '',
      errors_only: filters?.errorsOnly ?? false,
      limit: filters?.limit ?? 100,
      offset: filters?.offset ?? 0,
    }),
  });
}

export function useProxyLogDetail(logId: string) {
  return useQuery({
    queryKey: proxyKeys.logDetail(logId),
    // [FIX] Backend expects snake_case parameter 'log_id'
    queryFn: () => invoke<ProxyLog>('get_proxy_log_detail', { log_id: logId }),
    enabled: !!logId,
  });
}

export function useCloudflaredStatus() {
  return useQuery({
    queryKey: proxyKeys.cloudflared(),
    queryFn: () => invoke<CloudflaredStatus>('cloudflared_get_status'),
    refetchInterval: 10000,
  });
}
