import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';
import { AlertTriangle, PanelLeft, Paperclip, SlidersHorizontal } from 'lucide-react';
import { request as invoke } from '../utils/request';
import { cn } from '../utils/cn';
import { useConfigStore } from '../stores/useConfigStore';
import { newId, useActiveSession, useChatStore, type ChatMessage } from '../stores/useChatStore';
import { useProxyModels } from '../hooks/useProxyModels';
import {
    CHAT_PROTOCOLS,
    ChatRequestError,
    fetchProxyModels,
    resolveProxyBaseUrl,
    streamChat,
    type ChatTurn,
} from '../services/chatService';
import { loadAttachmentData, saveAttachmentData, type LoadedAttachment } from '../services/chatAttachments';
import { ChatSidebar } from '../components/chat/ChatSidebar';
import { ChatComposer, type ChatComposerHandle } from '../components/chat/ChatComposer';
import { ChatMessageView } from '../components/chat/ChatMessageView';
import { ModelPicker, toPickerModels } from '../components/chat/ModelPicker';
import { ChatSettingsPopover } from '../components/chat/ChatSettingsPopover';

interface ProxyStatus {
    running: boolean;
    port: number;
    base_url: string;
}

const NARROW_BREAKPOINT = 768;

function Chat() {
    const { t } = useTranslation();
    const config = useConfigStore((s) => s.config);
    const { sessions, activeId, defaults, newChat, selectSession, deleteSession, createSession, updateSettings, addMessage, patchMessage, truncateFrom } =
        useChatStore();
    const session = useActiveSession();
    const settings = session ?? defaults;

    const [sidebarOpen, setSidebarOpen] = useState(() => window.innerWidth >= NARROW_BREAKPOINT);
    const [status, setStatus] = useState<ProxyStatus | null>(null);
    const [proxyModelIds, setProxyModelIds] = useState<string[] | null>(null);
    const [modelsLoading, setModelsLoading] = useState(true);
    const [streamingId, setStreamingId] = useState<string | null>(null);
    const [dragging, setDragging] = useState(false);

    const abortRef = useRef<AbortController | null>(null);
    const scrollRef = useRef<HTMLDivElement>(null);
    const stickToBottom = useRef(true);
    const composerRef = useRef<ChatComposerHandle>(null);

    const { models: fallbackModels } = useProxyModels();

    const apiKey = config?.proxy.api_key ?? '';
    const port = status?.running && status.port ? status.port : config?.proxy.port;
    const baseUrl = resolveProxyBaseUrl(port);
    const proxyStopped = status !== null && !status.running;

    // Proxy status: on mount and whenever the window regains focus (user may start it elsewhere).
    useEffect(() => {
        const load = () =>
            invoke<ProxyStatus>('get_proxy_status')
                .then(setStatus)
                .catch(() => setStatus(null));
        load();
        window.addEventListener('focus', load);
        return () => window.removeEventListener('focus', load);
    }, []);

    // Model list straight from the proxy, so custom mappings and dynamic models are included.
    useEffect(() => {
        if (!config) return;
        if (proxyStopped) {
            setModelsLoading(false);
            return;
        }
        const controller = new AbortController();
        setModelsLoading(true);
        fetchProxyModels(baseUrl, apiKey, controller.signal)
            .then(setProxyModelIds)
            .catch((err) => {
                if (!controller.signal.aborted) {
                    console.warn('[Chat] Failed to load /v1/models, using local model list:', err);
                    setProxyModelIds(null);
                }
            })
            .finally(() => !controller.signal.aborted && setModelsLoading(false));
        return () => controller.abort();
    }, [baseUrl, apiKey, config, proxyStopped]);

    const models = useMemo(
        () => toPickerModels(proxyModelIds?.length ? proxyModelIds : fallbackModels.map((m) => m.id)),
        [proxyModelIds, fallbackModels]
    );

    // Pick a model automatically once the proxy's list has settled.
    useEffect(() => {
        if (!settings.model && !modelsLoading && models.length > 0) updateSettings({ model: models[0].id });
    }, [settings.model, modelsLoading, models, updateSettings]);

    // Abort any in-flight request when leaving the page.
    useEffect(() => () => abortRef.current?.abort(), []);

    const messages = session?.messages ?? [];

    useEffect(() => {
        const el = scrollRef.current;
        if (el && stickToBottom.current) el.scrollTop = el.scrollHeight;
    }, [messages]);

    useEffect(() => {
        stickToBottom.current = true;
        composerRef.current?.focus();
    }, [activeId]);

    const runAssistant = useCallback(
        async (sessionId: string, history: ChatTurn[], opts: { model: string; protocol: typeof settings.protocol; systemPrompt: string; temperature?: number }) => {
            const assistantId = newId();
            const started = performance.now();
            addMessage(sessionId, {
                id: assistantId,
                role: 'assistant',
                content: '',
                pending: true,
                model: opts.model,
                protocol: opts.protocol,
                createdAt: Date.now(),
            });

            const controller = new AbortController();
            abortRef.current = controller;
            setStreamingId(sessionId);
            stickToBottom.current = true;

            // Coalesce token deltas into at most one store update per animation frame.
            let text = '';
            let thinking = '';
            let frame = 0;
            const flush = () => {
                frame = 0;
                patchMessage(sessionId, assistantId, () => ({ content: text, thinking: thinking || undefined }));
            };
            const schedule = () => {
                if (!frame) frame = requestAnimationFrame(flush);
            };

            const finalPatch: Partial<ChatMessage> = {};
            try {
                const result = await streamChat({
                    protocol: opts.protocol,
                    baseUrl,
                    apiKey,
                    model: opts.model,
                    messages: history,
                    system: opts.systemPrompt.trim() || undefined,
                    temperature: opts.temperature,
                    signal: controller.signal,
                    onText: (d) => {
                        text += d;
                        schedule();
                    },
                    onThinking: (d) => {
                        thinking += d;
                        schedule();
                    },
                });
                finalPatch.usage = result.usage;
                if (!text && !thinking) finalPatch.error = t('chat.empty_response', 'The model returned an empty response.');
            } catch (err) {
                if (!controller.signal.aborted) {
                    const status = err instanceof ChatRequestError ? err.status : undefined;
                    finalPatch.error =
                        status === 503
                            ? `${t('chat.proxy_stopped', 'The API proxy service is not running.')}\n${(err as Error).message}`
                            : err instanceof TypeError
                                ? `${t('chat.network_error', 'Could not reach the proxy at')} ${baseUrl || window.location.origin}\n${err.message}`
                                : (err as Error).message || String(err);
                }
            } finally {
                if (frame) cancelAnimationFrame(frame);
                patchMessage(sessionId, assistantId, () => ({
                    ...finalPatch,
                    content: text,
                    thinking: thinking || undefined,
                    pending: false,
                    durationMs: Math.round(performance.now() - started),
                }));
                if (abortRef.current === controller) abortRef.current = null;
                setStreamingId(null);
            }
        },
        [addMessage, patchMessage, baseUrl, apiKey, t]
    );

    /** Rebuild the request history, re-attaching stored payloads; missing ones become a text note. */
    const toHistory = async (list: ChatMessage[]): Promise<ChatTurn[]> =>
        Promise.all(
            list
                .filter((m) => !m.pending && (m.content || m.attachments?.length))
                .map(async (m) => {
                    if (!m.attachments?.length) return { role: m.role, content: m.content };
                    const attachments: LoadedAttachment[] = [];
                    const missing: string[] = [];
                    for (const a of m.attachments) {
                        const data = await loadAttachmentData(a.id);
                        if (data === undefined) missing.push(a.name);
                        else attachments.push({ ...a, data });
                    }
                    const note = missing.map((name) => `[Attachment unavailable: ${name}]`).join('\n');
                    return { role: m.role, content: [note, m.content].filter(Boolean).join('\n'), attachments };
                })
        );

    const handleSend = async (text: string, files: LoadedAttachment[]) => {
        if (!settings.model) return;
        const opts = { model: settings.model, protocol: settings.protocol, systemPrompt: settings.systemPrompt, temperature: settings.temperature };
        const prior = session?.messages ?? [];
        const sessionId = session?.id ?? createSession(text || files[0]?.name || '');
        // Payloads land in the in-memory cache synchronously, so the message can render right away.
        const saved = Promise.all(files.map((f) => saveAttachmentData(f.id, f.data)));
        const userMessage: ChatMessage = {
            id: newId(),
            role: 'user',
            content: text,
            attachments: files.length ? files.map(({ data: _data, ...meta }) => meta) : undefined,
            createdAt: Date.now(),
        };
        addMessage(sessionId, userMessage);
        await saved;
        runAssistant(sessionId, await toHistory([...prior, userMessage]), opts);
    };

    const handleRegenerate = async (assistant: ChatMessage) => {
        if (!session) return;
        const idx = session.messages.findIndex((m) => m.id === assistant.id);
        if (idx === -1) return;
        truncateFrom(session.id, assistant.id);
        runAssistant(session.id, await toHistory(session.messages.slice(0, idx)), {
            model: settings.model,
            protocol: settings.protocol,
            systemPrompt: settings.systemPrompt,
            temperature: settings.temperature,
        });
    };

    const handleStop = () => abortRef.current?.abort();

    const closeSidebarOnNarrow = () => {
        if (window.innerWidth < NARROW_BREAKPOINT) setSidebarOpen(false);
    };

    const isStreamingHere = streamingId !== null && streamingId === activeId;
    const lastMessage = messages[messages.length - 1];

    const composer = (
        <ChatComposer
            ref={composerRef}
            onSend={handleSend}
            onStop={handleStop}
            streaming={isStreamingHere}
            disabled={streamingId !== null || !settings.model}
        />
    );

    return (
        <div className="h-full w-full max-w-7xl mx-auto px-3 sm:px-8 pt-2 pb-5">
            <div className="h-full flex overflow-hidden relative rounded-2xl border border-gray-100 dark:border-base-200 bg-white dark:bg-base-100 shadow-sm">
                {sidebarOpen && (
                    <div className="md:hidden absolute inset-0 z-20 bg-black/30 rounded-2xl" onClick={() => setSidebarOpen(false)} />
                )}
                {sidebarOpen && (
                    <aside className="w-60 shrink-0 border-r border-gray-100 dark:border-base-200 bg-gray-50 dark:bg-base-100 md:dark:bg-white/[0.02] max-md:absolute max-md:inset-y-0 max-md:left-0 max-md:z-30 max-md:shadow-xl">
                        <ChatSidebar
                            sessions={sessions}
                            activeId={activeId}
                            onNewChat={() => {
                                newChat();
                                closeSidebarOnNarrow();
                            }}
                            onSelect={(id) => {
                                selectSession(id);
                                closeSidebarOnNarrow();
                            }}
                            onDelete={(id) => {
                                if (streamingId === id) handleStop();
                                deleteSession(id);
                            }}
                        />
                    </aside>
                )}

                <section
                    className="flex-1 min-w-0 flex flex-col relative"
                    onDragOver={(e) => {
                        if (!e.dataTransfer.types.includes('Files')) return;
                        e.preventDefault();
                        setDragging(true);
                    }}
                    onDragLeave={(e) => {
                        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDragging(false);
                    }}
                    onDrop={(e) => {
                        if (!e.dataTransfer.files.length) return;
                        e.preventDefault();
                        setDragging(false);
                        composerRef.current?.addFiles(Array.from(e.dataTransfer.files));
                    }}
                >
                    {dragging && (
                        <div className="absolute inset-2 z-40 rounded-2xl border-2 border-dashed border-blue-400 bg-blue-50/80 dark:bg-blue-950/50 flex flex-col items-center justify-center gap-2 text-blue-600 dark:text-blue-300 pointer-events-none">
                            <Paperclip size={28} />
                            <span className="text-sm font-medium">{t('chat.drop_files', 'Drop files to attach')}</span>
                        </div>
                    )}
                    <header className="flex items-center gap-2 px-3 py-2">
                        <button
                            type="button"
                            onClick={() => setSidebarOpen((o) => !o)}
                            title={t('chat.toggle_sidebar', 'Toggle sidebar')}
                            className="p-2 rounded-lg text-gray-500 hover:bg-gray-100 dark:hover:bg-base-200"
                        >
                            <PanelLeft size={18} />
                        </button>

                        <ModelPicker
                            models={models}
                            value={settings.model}
                            loading={modelsLoading}
                            onChange={(model) => updateSettings({ model })}
                        />

                        <div className="flex-1" />

                        <div className="flex items-center gap-0.5 bg-gray-200/70 dark:bg-base-200 p-1 rounded-xl" title={t('chat.protocol', 'Protocol')}>
                            {CHAT_PROTOCOLS.map((p) => (
                                <button
                                    key={p.id}
                                    type="button"
                                    title={p.endpoint}
                                    onClick={() => updateSettings({ protocol: p.id })}
                                    className={cn(
                                        'px-2.5 sm:px-3 py-1 rounded-lg text-xs font-semibold transition-all',
                                        settings.protocol === p.id
                                            ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-sm'
                                            : 'text-gray-500 dark:text-gray-400 hover:text-gray-800 dark:hover:text-gray-200'
                                    )}
                                >
                                    {p.label}
                                </button>
                            ))}
                        </div>

                        <ChatSettingsPopover
                            systemPrompt={settings.systemPrompt}
                            temperature={settings.temperature}
                            onChange={updateSettings}
                            trigger={<SlidersHorizontal size={18} />}
                        />
                    </header>

                    {proxyStopped && (
                        <div className="mx-4 mb-2 flex items-center gap-2 rounded-xl border border-amber-200 dark:border-amber-900/60 bg-amber-50 dark:bg-amber-950/30 px-4 py-2 text-sm text-amber-800 dark:text-amber-300">
                            <AlertTriangle size={16} className="shrink-0" />
                            <span className="flex-1">{t('chat.proxy_stopped', 'The API proxy service is not running.')}</span>
                            <Link to="/api-proxy" className="font-semibold underline whitespace-nowrap">
                                {t('chat.open_proxy', 'Open API Proxy')}
                            </Link>
                        </div>
                    )}

                    {messages.length === 0 ? (
                        <div className="flex-1 flex flex-col items-center justify-center px-4 pb-[12vh]">
                            <h1 className="mb-8 text-2xl sm:text-3xl font-medium text-gray-800 dark:text-gray-100 text-center">
                                {t('chat.empty_title', "What's on the agenda today?")}
                            </h1>
                            <div className="w-full max-w-3xl">{composer}</div>
                            <p className="mt-3 text-xs text-gray-400 text-center">
                                {t('chat.hint', 'Requests go through the local API proxy, using the selected model and protocol.')}
                            </p>
                        </div>
                    ) : (
                        <>
                            <div
                                ref={scrollRef}
                                onScroll={(e) => {
                                    const el = e.currentTarget;
                                    stickToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
                                }}
                                className="flex-1 overflow-y-auto"
                            >
                                <div className="max-w-3xl mx-auto px-4 py-6 flex flex-col gap-8">
                                    {messages.map((m) => (
                                        <ChatMessageView
                                            key={m.id}
                                            message={m}
                                            isLast={m === lastMessage}
                                            onRegenerate={
                                                m.role === 'assistant' && m === lastMessage && streamingId === null
                                                    ? () => handleRegenerate(m)
                                                    : undefined
                                            }
                                        />
                                    ))}
                                </div>
                            </div>
                            <div className="px-4 pb-4 pt-1">
                                <div className="max-w-3xl mx-auto">{composer}</div>
                            </div>
                        </>
                    )}
                </section>
            </div>
        </div>
    );
}

export default Chat;
