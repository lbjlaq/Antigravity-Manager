import { create } from 'zustand';
import { AppConfig } from '../types/config';
import * as configService from '../services/configService';

interface ConfigState {
    config: AppConfig | null;
    loading: boolean;
    error: string | null;

    // Actions
    loadConfig: () => Promise<void>;
    saveConfig: (config: AppConfig, silent?: boolean) => Promise<void>;
    updateTheme: (theme: string) => Promise<void>;
    updateLanguage: (language: string) => Promise<void>;
    toggleShowAllQuotas: () => void;
    showAllQuotas: boolean;
    toggleMenuItem: (path: string) => Promise<void>;
    isMenuItemHidden: (path: string) => boolean;
}

export const useConfigStore = create<ConfigState>((set, get) => ({
    config: null,
    loading: false,
    error: null,
    showAllQuotas: localStorage.getItem('antigravity_show_all_quotas') === 'true',

    loadConfig: async () => {
        set({ loading: true, error: null });
        try {
            const config = await configService.loadConfig();
            set({ config, loading: false });
        } catch (error) {
            set({ error: String(error), loading: false });
        }
    },

    saveConfig: async (config: AppConfig, silent: boolean = false) => {
        if (!silent) set({ loading: true, error: null });
        try {
            await configService.saveConfig(config);
            set({ config, loading: false });
            const { isTauri } = await import('../utils/env');
            if (isTauri()) {
                const { invoke } = await import('@tauri-apps/api/core');
                await invoke('set_window_theme', { theme: config.theme }).catch(() => {
                });
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

    toggleShowAllQuotas: () => {
        const current = get().showAllQuotas;
        const next = !current;
        localStorage.setItem('antigravity_show_all_quotas', String(next));
        set({ showAllQuotas: next });
    },

    toggleMenuItem: async (path: string) => {
        const { config } = get();
        if (!config) return;

        const hiddenItems = config.hidden_menu_items || [];
        const isHidden = hiddenItems.includes(path);

        const newHiddenItems = isHidden
            ? hiddenItems.filter(item => item !== path)
            : [...hiddenItems, path];

        const newConfig = { ...config, hidden_menu_items: newHiddenItems };
        await get().saveConfig(newConfig, true);
    },

    isMenuItemHidden: (path: string) => {
        const { config } = get();
        if (!config) return false;
        return (config.hidden_menu_items || []).includes(path);
    },
}));
