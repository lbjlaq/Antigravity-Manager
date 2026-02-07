// File: src/features/stats/api/queries.ts
// React Query hooks for stats

import { useQuery } from '@tanstack/react-query';
import { invoke } from '@/shared/api';
import { statsKeys } from './keys';

// Types
export interface TokenStat {
  timestamp: number;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  requests: number;
}

export interface TokenStatByAccount {
  account_id: string;
  account_email: string;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  requests: number;
}

export interface TokenStatByModel {
  model: string;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  requests: number;
}

export interface TokenSummary {
  total_input_tokens: number;
  total_output_tokens: number;
  total_tokens: number;
  total_requests: number;
  avg_tokens_per_request: number;
}

export interface TrendData {
  timestamp: number;
  value: number;
  label: string;
}

// Query hooks
export function useTokenStatsHourly() {
  return useQuery({
    queryKey: statsKeys.tokenHourly(),
    queryFn: () => invoke<TokenStat[]>('get_token_stats_hourly'),
  });
}

export function useTokenStatsDaily() {
  return useQuery({
    queryKey: statsKeys.tokenDaily(),
    queryFn: () => invoke<TokenStat[]>('get_token_stats_daily'),
  });
}

export function useTokenStatsWeekly() {
  return useQuery({
    queryKey: statsKeys.tokenWeekly(),
    queryFn: () => invoke<TokenStat[]>('get_token_stats_weekly'),
  });
}

export function useTokenStatsByAccount() {
  return useQuery({
    queryKey: statsKeys.tokenByAccount(),
    queryFn: () => invoke<TokenStatByAccount[]>('get_token_stats_by_account'),
  });
}

export function useTokenStatsByModel() {
  return useQuery({
    queryKey: statsKeys.tokenByModel(),
    queryFn: () => invoke<TokenStatByModel[]>('get_token_stats_by_model'),
  });
}

export function useTokenStatsSummary() {
  return useQuery({
    queryKey: statsKeys.tokenSummary(),
    queryFn: () => invoke<TokenSummary>('get_token_stats_summary'),
  });
}

export function useModelTrendHourly() {
  return useQuery({
    queryKey: statsKeys.modelTrendHourly(),
    queryFn: () => invoke<TrendData[]>('get_token_stats_model_trend_hourly'),
  });
}

export function useModelTrendDaily() {
  return useQuery({
    queryKey: statsKeys.modelTrendDaily(),
    queryFn: () => invoke<TrendData[]>('get_token_stats_model_trend_daily'),
  });
}

export function useAccountTrendHourly() {
  return useQuery({
    queryKey: statsKeys.accountTrendHourly(),
    queryFn: () => invoke<TrendData[]>('get_token_stats_account_trend_hourly'),
  });
}

export function useAccountTrendDaily() {
  return useQuery({
    queryKey: statsKeys.accountTrendDaily(),
    queryFn: () => invoke<TrendData[]>('get_token_stats_account_trend_daily'),
  });
}
