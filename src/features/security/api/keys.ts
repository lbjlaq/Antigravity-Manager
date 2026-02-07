// File: src/features/security/api/keys.ts
// Query keys for security feature

export const securityKeys = {
  all: ['security'] as const,
  blacklist: () => [...securityKeys.all, 'blacklist'] as const,
  whitelist: () => [...securityKeys.all, 'whitelist'] as const,
  accessLogs: (filters?: Record<string, unknown>) => [...securityKeys.all, 'logs', filters] as const,
  settings: () => [...securityKeys.all, 'settings'] as const,
  stats: () => [...securityKeys.all, 'stats'] as const,
  ipTokenStats: (hours: number) => [...securityKeys.all, 'ipTokenStats', hours] as const,
};
