// File: src/features/stats/api/keys.ts
// Query keys for stats feature

export const statsKeys = {
  all: ['stats'] as const,
  token: () => [...statsKeys.all, 'token'] as const,
  tokenHourly: () => [...statsKeys.token(), 'hourly'] as const,
  tokenDaily: () => [...statsKeys.token(), 'daily'] as const,
  tokenWeekly: () => [...statsKeys.token(), 'weekly'] as const,
  tokenByAccount: () => [...statsKeys.token(), 'by-account'] as const,
  tokenByModel: () => [...statsKeys.token(), 'by-model'] as const,
  tokenSummary: () => [...statsKeys.token(), 'summary'] as const,
  modelTrendHourly: () => [...statsKeys.token(), 'model-trend', 'hourly'] as const,
  modelTrendDaily: () => [...statsKeys.token(), 'model-trend', 'daily'] as const,
  accountTrendHourly: () => [...statsKeys.token(), 'account-trend', 'hourly'] as const,
  accountTrendDaily: () => [...statsKeys.token(), 'account-trend', 'daily'] as const,
};
