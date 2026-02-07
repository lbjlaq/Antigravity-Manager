// File: src/widgets/debug-console/model/store.ts
// Debug Console state management

import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';

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
    unlistenFn: UnlistenFn | null;

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

    open: () => set({ isOpen: true }),
    close: () => set({ isOpen: false }),
    toggle: () => set((state) => ({ isOpen: !state.isOpen })),

    enable: async () => {
        try {
            await invoke('enable_debug_console');
            set({ isEnabled: true });
            await get().loadLogs();
            await get().startListening();
        } catch (error) {
            console.error('Failed to enable debug console:', error);
        }
    },

    disable: async () => {
        try {
            await invoke('disable_debug_console');
            get().stopListening();
            set({ isEnabled: false });
        } catch (error) {
            console.error('Failed to disable debug console:', error);
        }
    },

    loadLogs: async () => {
        try {
            const logs = await invoke<LogEntry[]>('get_debug_console_logs');
            set({ logs });
        } catch (error) {
            console.error('Failed to load logs:', error);
        }
    },

    clearLogs: async () => {
        try {
            await invoke('clear_debug_console_logs');
            set({ logs: [] });
        } catch (error) {
            console.error('Failed to clear logs:', error);
        }
    },

    addLog: (log: LogEntry) => {
        set((state) => {
            const newLogs = [...state.logs, log];
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
        if (unlistenFn) return;

        try {
            const unlisten = await listen<LogEntry>('log-event', (event) => {
                get().addLog(event.payload);
            });
            set({ unlistenFn: unlisten });
        } catch (error) {
            console.error('Failed to start listening for logs:', error);
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
            const isEnabled = await invoke<boolean>('is_debug_console_enabled');
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
