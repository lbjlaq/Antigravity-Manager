import { create } from 'zustand';
import { persist, createJSONStorage, type StateStorage } from 'zustand/middleware';
import type { ChatProtocol, ChatUsage } from '../services/chatService';
import { deleteAttachmentData, type ChatAttachment } from '../services/chatAttachments';

export interface ChatMessage {
    id: string;
    role: 'user' | 'assistant';
    content: string;
    /** Metadata only; payloads live in IndexedDB (see chatAttachments). */
    attachments?: ChatAttachment[];
    thinking?: string;
    error?: string;
    /** True while the assistant reply is still streaming. */
    pending?: boolean;
    model?: string;
    protocol?: ChatProtocol;
    durationMs?: number;
    usage?: ChatUsage;
    createdAt: number;
}

export interface ChatSettings {
    model: string;
    protocol: ChatProtocol;
    systemPrompt: string;
    temperature?: number;
}

export interface ChatSession extends ChatSettings {
    id: string;
    title: string;
    messages: ChatMessage[];
    createdAt: number;
    updatedAt: number;
}

interface ChatState {
    sessions: ChatSession[];
    /** `null` means a fresh, not yet persisted chat (the "New chat" screen). */
    activeId: string | null;
    /** Settings applied to the next new chat; tracks the last choice the user made. */
    defaults: ChatSettings;

    newChat: () => void;
    selectSession: (id: string) => void;
    deleteSession: (id: string) => void;
    createSession: (firstMessage: string) => string;
    updateSettings: (patch: Partial<ChatSettings>) => void;
    addMessage: (sessionId: string, message: ChatMessage) => void;
    patchMessage: (sessionId: string, messageId: string, patch: (m: ChatMessage) => Partial<ChatMessage>) => void;
    /** Drop `messageId` and everything after it (used by regenerate). */
    truncateFrom: (sessionId: string, messageId: string) => void;
}

export const newId = () =>
    typeof crypto !== 'undefined' && 'randomUUID' in crypto
        ? crypto.randomUUID()
        : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;

const TITLE_MAX = 40;
const makeTitle = (text: string) => {
    const line = text.trim().replace(/\s+/g, ' ');
    return line.length > TITLE_MAX ? `${line.slice(0, TITLE_MAX)}…` : line || 'New chat';
};

/**
 * localStorage wrapper that tolerates blocked storage and coalesces the
 * high-frequency writes produced while a reply is streaming.
 */
const WRITE_DELAY_MS = 400;
const pendingWrites = new Map<string, string>();
let writeTimer: ReturnType<typeof setTimeout> | null = null;
const flushWrites = () => {
    writeTimer = null;
    pendingWrites.forEach((value, key) => {
        try {
            localStorage.setItem(key, value);
        } catch {
            // storage full or unavailable; chat keeps working in memory
        }
    });
    pendingWrites.clear();
};
if (typeof window !== 'undefined') window.addEventListener('beforeunload', flushWrites);

const safeStorage: StateStorage = {
    getItem: (key) => {
        try {
            return localStorage.getItem(key);
        } catch {
            return null;
        }
    },
    setItem: (key, value) => {
        pendingWrites.set(key, value);
        if (!writeTimer) writeTimer = setTimeout(flushWrites, WRITE_DELAY_MS);
    },
    removeItem: (key) => {
        pendingWrites.delete(key);
        try {
            localStorage.removeItem(key);
        } catch {
            // ignore
        }
    },
};

const touch = (s: ChatSession, patch: Partial<ChatSession>): ChatSession => ({ ...s, ...patch, updatedAt: Date.now() });

export const useChatStore = create<ChatState>()(
    persist(
        (set, get) => ({
            sessions: [],
            activeId: null,
            defaults: { model: '', protocol: 'openai', systemPrompt: '' },

            newChat: () => set({ activeId: null }),

            selectSession: (id) => set({ activeId: id }),

            deleteSession: (id) => {
                const removed = get().sessions.find((s) => s.id === id);
                set((state) => ({
                    sessions: state.sessions.filter((s) => s.id !== id),
                    activeId: state.activeId === id ? null : state.activeId,
                }));
                if (removed) deleteAttachmentData(removed.messages.flatMap((m) => m.attachments?.map((a) => a.id) ?? []));
            },

            createSession: (firstMessage) => {
                const now = Date.now();
                const session: ChatSession = {
                    ...get().defaults,
                    id: newId(),
                    title: makeTitle(firstMessage),
                    messages: [],
                    createdAt: now,
                    updatedAt: now,
                };
                set((state) => ({ sessions: [session, ...state.sessions], activeId: session.id }));
                return session.id;
            },

            updateSettings: (patch) =>
                set((state) => ({
                    defaults: { ...state.defaults, ...patch },
                    sessions: state.activeId
                        ? state.sessions.map((s) => (s.id === state.activeId ? { ...s, ...patch } : s))
                        : state.sessions,
                })),

            addMessage: (sessionId, message) =>
                set((state) => ({
                    sessions: state.sessions.map((s) =>
                        s.id === sessionId ? touch(s, { messages: [...s.messages, message] }) : s
                    ),
                })),

            patchMessage: (sessionId, messageId, patch) =>
                set((state) => ({
                    sessions: state.sessions.map((s) =>
                        s.id === sessionId
                            ? {
                                ...s,
                                messages: s.messages.map((m) => (m.id === messageId ? { ...m, ...patch(m) } : m)),
                            }
                            : s
                    ),
                })),

            truncateFrom: (sessionId, messageId) =>
                set((state) => ({
                    sessions: state.sessions.map((s) => {
                        if (s.id !== sessionId) return s;
                        const idx = s.messages.findIndex((m) => m.id === messageId);
                        return idx === -1 ? s : touch(s, { messages: s.messages.slice(0, idx) });
                    }),
                })),
        }),
        {
            name: 'abv_chat_sessions',
            version: 1,
            storage: createJSONStorage(() => safeStorage),
            partialize: (state) => ({ sessions: state.sessions, activeId: state.activeId, defaults: state.defaults }),
            // A reply that was streaming when the page closed can never finish; mark it as interrupted.
            merge: (persisted, current) => {
                const p = (persisted ?? {}) as Partial<ChatState>;
                const sessions = (p.sessions ?? []).map((s) => ({
                    ...s,
                    messages: s.messages.map((m) => (m.pending ? { ...m, pending: false } : m)),
                }));
                return { ...current, ...p, sessions, defaults: { ...current.defaults, ...p.defaults } };
            },
        }
    )
);

export const useActiveSession = () =>
    useChatStore((state) => state.sessions.find((s) => s.id === state.activeId) ?? null);
