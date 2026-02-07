// File: src/features/proxy/api/keys.ts
// Query keys for proxy feature

export const proxyKeys = {
  all: ['proxy'] as const,
  status: () => [...proxyKeys.all, 'status'] as const,
  stats: () => [...proxyKeys.all, 'stats'] as const,
  logs: (filters?: Record<string, unknown>) => [...proxyKeys.all, 'logs', filters] as const,
  logDetail: (id: string) => [...proxyKeys.all, 'logs', 'detail', id] as const,
  cliStatus: () => [...proxyKeys.all, 'cli', 'status'] as const,
  cloudflared: () => [...proxyKeys.all, 'cloudflared'] as const,
};
