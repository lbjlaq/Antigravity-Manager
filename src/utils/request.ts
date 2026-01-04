import { invoke, InvokeArgs } from '@tauri-apps/api/core';

/**
 * Type-safe wrapper for Tauri invoke commands
 * Provides consistent error handling and logging for development
 */
export async function request<T>(cmd: string, args?: InvokeArgs): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (error) {
    // Only log in development mode
    if (import.meta.env.DEV) {
      console.error(`[Tauri API Error] Command: ${cmd}`, error);
    }
    throw error;
  }
}
