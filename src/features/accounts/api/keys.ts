// File: src/features/accounts/api/keys.ts
// Query keys for accounts feature

export const accountKeys = {
  all: ['accounts'] as const,
  lists: () => [...accountKeys.all, 'list'] as const,
  list: (filters?: Record<string, unknown>) => [...accountKeys.lists(), filters] as const,
  details: () => [...accountKeys.all, 'detail'] as const,
  detail: (id: string) => [...accountKeys.details(), id] as const,
  current: () => [...accountKeys.all, 'current'] as const,
  deviceProfiles: (id: string) => [...accountKeys.detail(id), 'device-profiles'] as const,
};
