import { invoke } from '@tauri-apps/api/core';

export async function request<T>(cmd: string, args?: any): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (error) {
    console.error(`API Error [${cmd}]:`, error);
    throw error;
  }
}
