import { create } from 'zustand';
import { request } from '../utils/request';

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
    pollInterval: null,

    open: () => set({ isOpen: true }),
    close: () => set({ isOpen: false }),
    toggle: () => set((state) => ({ isOpen: !state.isOpen })),

    enable: async () => {
        try {
            await request('enable_debug_console');
            set({ isEnabled: true });
            await get().loadLogs();
            get().startPolling();
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
            await request('disable_debug_console');
            get().stopPolling();
            set({ isEnabled: false });
        } catch (error) {
            console.error('Failed to disable debug console:', error);
        }
    },

    loadLogs: async () => {
        try {
            const logs = await request<LogEntry[]>('get_debug_console_logs');
            set({ logs });
        } catch (error) {
            console.error('Failed to load logs:', error);
        }
    },

    clearLogs: async () => {
        console.log('[DebugConsole] Clearing logs...');
        set({ logs: [] }); // Clear immediately in frontend
        try {
            await request('clear_debug_console_logs');
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

    checkEnabled: async () => {
        try {
            const isEnabled = await request<boolean>('is_debug_console_enabled');
            set({ isEnabled });
            if (isEnabled) {
                await get().loadLogs();
                get().startPolling();
            }
        } catch (error) {
            console.error('Failed to check debug console status:', error);
        }
    },
}));
