import { invoke } from '@tauri-apps/api/core';

export interface CostSavings {
  total_saved: number;
  per_account: { [key: string]: number };
  per_user: { [key: string]: number };
  savings_percentage: number;
  hourly_savings: [number, number][];
  daily_savings: [number, number][];
}

export interface SignatureStats {
  signature: string;
  reuse_count: number;
  last_used: string;
  first_cached: string;
  cost_saved: number;
  avg_lookup_time: number;
  high_value: boolean;
}

export interface PerformanceMetrics {
  lookup_p50: number;
  lookup_p95: number;
  lookup_p99: number;
  write_p95: number;
  memory_usage: number;
  total_operations: number;
  degradation_alert: boolean;
}

export interface CacheMetrics {
  hit_count: number;
  miss_count: number;
  hit_rate: number;
  top_signatures: SignatureStats[];
  cost_savings: CostSavings;
  performance: PerformanceMetrics;
  updated_at: string;
}

export async function getCacheMetrics(): Promise<CacheMetrics> {
  return invoke('get_cache_metrics');
}

export async function getCacheHitRate(): Promise<number> {
  return invoke('get_cache_hit_rate');
}

export async function getTopSignatures(limit?: number): Promise<SignatureStats[]> {
  return invoke('get_top_cache_signatures', { limit: limit || 10 });
}

export async function getCostSavings(): Promise<CostSavings> {
  const metrics = await getCacheMetrics();
  return metrics.cost_savings;
}

export async function clearCacheMetrics(): Promise<void> {
  return invoke('clear_cache_metrics');
}
