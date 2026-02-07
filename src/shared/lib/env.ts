// File: src/shared/lib/env.ts
// Environment detection utilities

/**
 * Detect if running in Tauri desktop environment
 */
export const isTauri = (): boolean => {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const win = window as any;
  return typeof window !== 'undefined' &&
    (!!win.__TAURI_INTERNALS__ || !!win.__TAURI__);
};

/**
 * Detect if running on Linux
 */
export const isLinux = (): boolean => {
  return navigator.userAgent.toLowerCase().includes('linux');
};

/**
 * Detect if running on macOS
 */
export const isMac = (): boolean => {
  return navigator.userAgent.toLowerCase().includes('mac');
};

/**
 * Detect if running on Windows
 */
export const isWindows = (): boolean => {
  return navigator.userAgent.toLowerCase().includes('win');
};
