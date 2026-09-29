import { create } from 'zustand';
import { request as invoke } from '../utils/request';
import { isTauri } from '../utils/env';
import { request } from '../utils/request';
import { subscribe, onBridgeStateChange, bridgeState } from '../utils/events';

export interface LogEntry {
    id: number;
    timestamp: number;
    level: 'ERROR' | 'WARN' | 'INFO' | 'DEBUG' | 'TRACE';
    target: string;
    message: string;
    fields: Record<string, string>;
}

export type LogLevel = 'ERROR' | 'WARN' | 'INFO' | 'DEBUG' | 'TRACE';

interface DebugConsoleState {
    isOpen: boolean;
    isEnabled: boolean;
    logs: LogEntry[];
    filter: LogLevel[];
    searchTerm: string;
    autoScroll: boolean;
    unlistenFn: (() => void) | null;
    pollInterval: number | null;

    // Actions
    open: () => void;
    close: () => void;
    toggle: () => void;
    enable: () => Promise<void>;
    disable: () => Promise<void>;
    loadLogs: () => Promise<void>;
    clearLogs: () => Promise<void>;
    addLog: (log: LogEntry) => void;
    setFilter: (levels: LogLevel[]) => void;
    setSearchTerm: (term: string) => void;
    setAutoScroll: (enabled: boolean) => void;
    startListening: () => Promise<void>;
    stopListening: () => void;
    startPolling: () => void;
    stopPolling: () => void;
    checkEnabled: () => Promise<void>;
}

const MAX_LOGS = 5000;

export const useDebugConsole = create<DebugConsoleState>((set, get) => ({
    isOpen: false,
    isEnabled: false,
    logs: [],
    filter: ['ERROR', 'WARN', 'INFO'],
    searchTerm: '',
    autoScroll: true,
    unlistenFn: null,
    pollInterval: null,

    open: () => set({ isOpen: true }),
    close: () => set({ isOpen: false }),
    toggle: () => set((state) => ({ isOpen: !state.isOpen })),

    enable: async () => {
        try {
            if (isTauri()) {
                await invoke('enable_debug_console');
            } else {
                await request('enable_debug_console');
            }
            set({ isEnabled: true });
            await get().loadLogs();
            await get().startListening();
        } catch (error) {
            console.error('Failed to enable debug console:', error);
        }
    },

    startPolling: () => {
        if (get().pollInterval) return;
        const interval = window.setInterval(async () => {
            if (get().isEnabled && get().isOpen) {
                await get().loadLogs();
            }
        }, 2000);
        set({ pollInterval: interval });
    },

    stopPolling: () => {
        const { pollInterval } = get();
        if (pollInterval) {
            clearInterval(pollInterval);
            set({ pollInterval: null });
        }
    },

    disable: async () => {
        try {
            if (isTauri()) {
                await invoke('disable_debug_console');
            } else {
                await request('disable_debug_console');
            }
            if (isTauri()) {
                get().stopListening();
            } else {
                get().stopPolling();
            }
            set({ isEnabled: false });
        } catch (error) {
            console.error('Failed to disable debug console:', error);
        }
    },

    loadLogs: async () => {
        try {
            let logs: LogEntry[];
            if (isTauri()) {
                logs = await invoke<LogEntry[]>('get_debug_console_logs');
            } else {
                logs = await request<LogEntry[]>('get_debug_console_logs');
            }
            set({ logs });
        } catch (error) {
            console.error('Failed to load logs:', error);
        }
    },

    clearLogs: async () => {
        console.log('[DebugConsole] Clearing logs...');
        set({ logs: [] }); // Clear immediately in frontend
        try {
            if (isTauri()) {
                await invoke('clear_debug_console_logs');
            } else {
                await request('clear_debug_console_logs');
            }
            console.log('[DebugConsole] Backend log buffer cleared');
        } catch (error) {
            console.error('[DebugConsole] Failed to clear background logs:', error);
        }
    },

    addLog: (log: LogEntry) => {
        set((state) => {
            const newLogs = [...state.logs, log];
            // Keep only last MAX_LOGS entries
            if (newLogs.length > MAX_LOGS) {
                return { logs: newLogs.slice(-MAX_LOGS) };
            }
            return { logs: newLogs };
        });
    },

    setFilter: (levels: LogLevel[]) => set({ filter: levels }),
    setSearchTerm: (term: string) => set({ searchTerm: term }),
    setAutoScroll: (enabled: boolean) => set({ autoScroll: enabled }),

    startListening: async () => {
        const { unlistenFn } = get();
        if (unlistenFn) return; // Already listening

        // 双轨事件桥：Tauri 走 listen，Web 走 SSE（/api/events）
        try {
            const unlisten = await subscribe<LogEntry>('log-event', (payload) => {
                get().addLog(payload);
            });
            set({ unlistenFn: unlisten });

            // Web 模式：SSE 连接不可用（dead）时降级为轮询兜底
            if (!isTauri()) {
                const off = onBridgeStateChange((s) => {
                    if (s === 'dead') {
                        off();
                        get().startPolling();
                    }
                });
                if (bridgeState() === 'dead') {
                    off();
                    get().startPolling();
                }
            }
        } catch (error) {
            console.error('Failed to start listening for logs:', error);
            if (!isTauri()) get().startPolling();
        }
    },

    stopListening: () => {
        const { unlistenFn } = get();
        if (unlistenFn) {
            unlistenFn();
            set({ unlistenFn: null });
        }
    },

    checkEnabled: async () => {
        try {
            let isEnabled: boolean;
            if (isTauri()) {
                isEnabled = await invoke<boolean>('is_debug_console_enabled');
            } else {
                isEnabled = await request<boolean>('is_debug_console_enabled');
            }
            set({ isEnabled });
            if (isEnabled) {
                await get().loadLogs();
                await get().startListening();
            }
        } catch (error) {
            console.error('Failed to check debug console status:', error);
        }
    },
}));
