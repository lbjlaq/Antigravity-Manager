// File: src/shared/index.ts
// Main public API for shared layer

// API
export { invoke, request, queryClient } from './api';

// Lib
export { 
  cn, 
  formatRelativeTime, 
  formatBytes, 
  getQuotaColor, 
  formatTimeRemaining, 
  getTimeRemainingColor, 
  formatDate, 
  formatCompactNumber,
  copyToClipboard,
  isTauri,
  isLinux,
  isMac,
  isWindows,
} from './lib';

// Hooks
export { useProxyModels, type ProxyModel } from './hooks';

// Config
export { i18n } from './config';

// UI - re-export for convenience
export * from './ui';
