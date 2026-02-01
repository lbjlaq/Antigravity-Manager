// File: src/entities/config/model/store.ts
// Config store using Zustand

import { create } from 'zustand';
import { invoke } from '@/shared/api';
import type { AppConfig } from '../types';
import { isTauri } from '@/shared/lib';

interface ConfigState {
  config: AppConfig | null;
  loading: boolean;
  error: string | null;

  // Actions
  loadConfig: () => Promise<void>;
  saveConfig: (config: AppConfig, silent?: boolean) => Promise<void>;
  updateTheme: (theme: string) => Promise<void>;
  updateLanguage: (language: string) => Promise<void>;
}

export const useConfigStore = create<ConfigState>((set, get) => ({
  config: null,
  loading: false,
  error: null,

  loadConfig: async () => {
    set({ loading: true, error: null });
    try {
      const config = await invoke<AppConfig>('load_config');
      set({ config, loading: false });
    } catch (error) {
      set({ error: String(error), loading: false });
    }
  },

  saveConfig: async (config: AppConfig, silent: boolean = false) => {
    if (!silent) set({ loading: true, error: null });
    try {
      await invoke<void>('save_config', { config });
      set({ config, loading: false });
      if (isTauri()) {
        const { invoke: tauriInvoke } = await import('@tauri-apps/api/core');
        await tauriInvoke('set_window_theme', { theme: config.theme }).catch(() => {});
      }
    } catch (error) {
      set({ error: String(error), loading: false });
      throw error;
    }
  },

  updateTheme: async (theme: string) => {
    const { config } = get();
    if (!config || config.theme === theme) return;

    const newConfig = { ...config, theme };
    await get().saveConfig(newConfig, true);
  },

  updateLanguage: async (language: string) => {
    const { config } = get();
    if (!config || config.language === language) return;

    const newConfig = { ...config, language };
    await get().saveConfig(newConfig, true);
  },
}));
