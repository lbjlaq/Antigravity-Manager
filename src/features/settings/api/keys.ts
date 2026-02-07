// File: src/features/settings/api/keys.ts
// Query keys for settings feature

export const settingsKeys = {
  all: ['settings'] as const,
  config: () => [...settingsKeys.all, 'config'] as const,
  updateSettings: () => [...settingsKeys.all, 'update'] as const,
  httpApiSettings: () => [...settingsKeys.all, 'http-api'] as const,
  autoLaunch: () => [...settingsKeys.all, 'auto-launch'] as const,
  dataDir: () => [...settingsKeys.all, 'data-dir'] as const,
};
