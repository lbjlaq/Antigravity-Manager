import { create } from 'zustand';
import { AppConfig } from '../types/config';
import * as configService from '../services/configService';
import i18n from '../i18n';

// 规范化语言编码 (兼容方言与区域代码别名)
export function normalizeLanguageCode(code: string): string {
    if (!code) return 'en';
    if (code === 'zh-CN' || code === 'zh-Hans') return 'zh';
    if (code.startsWith('zh-TW') || code.startsWith('zh-HK') || code.startsWith('zh-Hant')) return 'zh-TW';
    if (code === 'ms' || code.startsWith('ms-')) return 'my';

    // 泛化支持如 en-US -> en, ja-JP -> ja, pt-BR -> pt 等常见 BCP 47 代码
    const primary = code.split(/[-_]/)[0].toLowerCase();
    const supported = ['en', 'zh', 'ja', 'tr', 'vi', 'pt', 'ko', 'ru', 'ar', 'es', 'my'];
    if (supported.includes(primary)) return primary;
    return code;
}

// 竞态隔离与尾随批处理锁：杜绝高频连续操作（如快速连续切换语言、主题或滑块）时后端 I/O 乱序回滚
let activeSavePromise: Promise<void> | null = null;
let queuedConfig: AppConfig | null = null;
let queuedWaiters: Array<{ resolve: () => void; reject: (err: unknown) => void }> = [];
let saveSequence = 0;

async function flushPersist(config: AppConfig): Promise<void> {
    await configService.saveConfig(config);
    const { isTauri } = await import('../utils/env');
    if (isTauri()) {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('set_window_theme', { theme: config.theme }).catch(() => {});
    }
}

async function executeSaveQueue(config: AppConfig): Promise<void> {
    if (activeSavePromise) {
        queuedConfig = config;
        return new Promise<void>((resolve, reject) => {
            queuedWaiters.push({ resolve, reject });
        });
    }

    activeSavePromise = (async () => {
        try {
            await flushPersist(config);
        } finally {
            while (queuedConfig) {
                const nextConfig = queuedConfig;
                const waiters = queuedWaiters;
                queuedConfig = null;
                queuedWaiters = [];
                try {
                    await flushPersist(nextConfig);
                    waiters.forEach(w => w.resolve());
                } catch (err) {
                    waiters.forEach(w => w.reject(err));
                }
            }
            activeSavePromise = null;
        }
    })();

    return activeSavePromise;
}

interface ConfigState {
    config: AppConfig | null;
    loading: boolean;
    error: string | null;

    // Actions
    loadConfig: () => Promise<void>;
    saveConfig: (config: AppConfig, silent?: boolean) => Promise<void>;
    setConfigLocal: (config: AppConfig) => void;
    syncConfigLocal: (updater: (cfg: AppConfig) => AppConfig) => void;
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
        const currentSeq = ++saveSequence;
        // 1. 同步立即乐观更新 Zustand store 状态
        // 确保所有订阅组件与后续读操作即刻获取最新配置，彻底消灭渲染时差与闭包陈旧值
        set({ config, loading: !silent, error: null });

        try {
            // 2. 串行/尾随有序持久化，杜绝连续点击触发并发写竞态
            await executeSaveQueue(config);
            if (currentSeq === saveSequence) {
                set({ loading: false });
            }
        } catch (error) {
            if (currentSeq === saveSequence) {
                set({ error: String(error), loading: false });
            }
            throw error;
        }
    },

    setConfigLocal: (config: AppConfig) => {
        set({ config });
    },

    syncConfigLocal: (updater: (cfg: AppConfig) => AppConfig) => {
        const current = get().config;
        if (!current) return;
        set({ config: updater(current) });
    },

    updateTheme: async (theme: string) => {
        const current = get().config;
        if (!current || current.theme === theme) return;

        localStorage.setItem('app-theme-preference', theme);
        const latest = await configService.loadConfig().catch(() => get().config || current);
        const newConfig = { ...latest, theme };
        await get().saveConfig(newConfig, true);

        const { isTauri } = await import('../utils/env');
        if (isTauri()) {
            const { emit } = await import('@tauri-apps/api/event');
            await emit('app://theme-changed', { theme }).catch(() => {});
        }
    },

    updateLanguage: async (language: string) => {
        const current = get().config;
        if (!current) return;

        const normalized = normalizeLanguageCode(language);
        if (current.language === normalized && i18n.language === normalized) return;

        // 1. 同步切换物理布局方向
        document.documentElement.dir = normalized === 'ar' ? 'rtl' : 'ltr';

        // 2. 同步乐观更新 store 内存态，落盘前拉取最新配置防止回写陈旧字段
        set({ config: { ...current, language: normalized } });
        const savePromise = (async () => {
            const latest = await configService.loadConfig().catch(() => get().config || current);
            await get().saveConfig({ ...latest, language: normalized }, true);
        })();

        // 3. 切换 i18n 并等待落盘完成
        await i18n.changeLanguage(normalized);
        await savePromise;

        const { isTauri } = await import('../utils/env');
        if (isTauri()) {
            const { emit } = await import('@tauri-apps/api/event');
            await emit('app://language-changed', { language: normalized }).catch(() => {});
        }
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

        const latest = await configService.loadConfig().catch(() => get().config || config);
        const hiddenItems = latest.hidden_menu_items || [];
        const isHidden = hiddenItems.includes(path);

        const newHiddenItems = isHidden
            ? hiddenItems.filter(item => item !== path)
            : [...hiddenItems, path];

        const newConfig = { ...latest, hidden_menu_items: newHiddenItems };
        await get().saveConfig(newConfig, true);
    },

    isMenuItemHidden: (path: string) => {
        const { config } = get();
        if (!config) return false;
        return (config.hidden_menu_items || []).includes(path);
    },
}));
