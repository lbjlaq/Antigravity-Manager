import { readSse } from '../utils/sse';
import { isTauri } from '../utils/env';
import { textFilePrompt, toDataUrl, type LoadedAttachment } from './chatAttachments';

/** Client protocol used to reach the local proxy. Each one exercises a different adapter. */
export type ChatProtocol = 'openai' | 'anthropic' | 'gemini';

export const CHAT_PROTOCOLS: { id: ChatProtocol; label: string; endpoint: string }[] = [
    { id: 'openai', label: 'OpenAI', endpoint: '/v1/chat/completions' },
    { id: 'anthropic', label: 'Anthropic', endpoint: '/v1/messages' },
    { id: 'gemini', label: 'Gemini', endpoint: '/v1beta/models/{model}:streamGenerateContent' },
];

export interface ChatTurn {
    role: 'user' | 'assistant';
    content: string;
    attachments?: LoadedAttachment[];
}

export interface ChatUsage {
    input?: number;
    output?: number;
}

export interface StreamChatOptions {
    protocol: ChatProtocol;
    baseUrl: string;
    apiKey: string;
    model: string;
    messages: ChatTurn[];
    system?: string;
    temperature?: number;
    signal?: AbortSignal;
    onText: (delta: string) => void;
    onThinking?: (delta: string) => void;
}

export interface StreamChatResult {
    usage?: ChatUsage;
    finishReason?: string;
}

/** Error carrying the HTTP status of a failed proxy call. */
export class ChatRequestError extends Error {
    constructor(message: string, public status?: number) {
        super(message);
        this.name = 'ChatRequestError';
    }
}

/**
 * Resolve the proxy origin the page should talk to.
 * Desktop: the webview has its own origin, so target the local proxy port directly.
 * Web/headless: the frontend is served by the proxy itself, so same-origin works for any host/port.
 */
export function resolveProxyBaseUrl(port?: number): string {
    if (isTauri()) return `http://127.0.0.1:${port || 8045}`;
    return '';
}

function authHeaders(apiKey: string): Record<string, string> {
    if (!apiKey) return {};
    return { Authorization: `Bearer ${apiKey}`, 'x-api-key': apiKey };
}

/** List model IDs exposed by the proxy (`GET /v1/models`). */
export async function fetchProxyModels(baseUrl: string, apiKey: string, signal?: AbortSignal): Promise<string[]> {
    const res = await fetch(`${baseUrl}/v1/models`, { headers: authHeaders(apiKey), signal });
    if (!res.ok) throw new ChatRequestError(await readError(res), res.status);
    const json = await res.json();
    return Array.isArray(json?.data) ? json.data.map((m: { id: string }) => m.id).filter(Boolean) : [];
}

async function readError(res: Response): Promise<string> {
    const text = await res.text().catch(() => '');
    let message = text;
    try {
        const json = JSON.parse(text);
        message = json?.error?.message ?? json?.message ?? (typeof json?.error === 'string' ? json.error : text);
    } catch {
        // not JSON, keep raw text
    }
    return `HTTP ${res.status}${message ? `: ${message}` : ''}`;
}

interface ProtocolAdapter {
    url: (opts: StreamChatOptions) => string;
    headers?: Record<string, string>;
    body: (opts: StreamChatOptions) => unknown;
    /** Consume one parsed SSE payload, forwarding deltas; returns partial result info. */
    handle: (payload: any, opts: StreamChatOptions, result: StreamChatResult) => void;
}

/**
 * Build one message's content in a protocol's native shape: attachments first, then the text.
 * Text files are sent as plain text everywhere; images and PDFs use each protocol's media block.
 */
function buildParts<T>(
    turn: ChatTurn,
    make: { text: (text: string) => T; image: (a: LoadedAttachment) => T; pdf: (a: LoadedAttachment) => T }
): T[] {
    const parts = (turn.attachments ?? []).map((a) =>
        a.kind === 'image' ? make.image(a) : a.kind === 'pdf' ? make.pdf(a) : make.text(textFilePrompt(a))
    );
    if (turn.content) parts.push(make.text(turn.content));
    return parts;
}

const ADAPTERS: Record<ChatProtocol, ProtocolAdapter> = {
    openai: {
        url: (o) => `${o.baseUrl}/v1/chat/completions`,
        body: (o) => ({
            model: o.model,
            stream: true,
            stream_options: { include_usage: true },
            ...(o.temperature !== undefined && { temperature: o.temperature }),
            messages: [
                ...(o.system ? [{ role: 'system', content: o.system }] : []),
                ...o.messages.map((m) => ({
                    role: m.role,
                    content: m.attachments?.length
                        ? buildParts<Record<string, unknown>>(m, {
                            text: (text) => ({ type: 'text', text }),
                            image: (a) => ({ type: 'image_url', image_url: { url: toDataUrl(a) } }),
                            pdf: (a) => ({ type: 'file', file: { filename: a.name, file_data: toDataUrl(a) } }),
                        })
                        : m.content,
                })),
            ],
        }),
        handle: (p, o, r) => {
            const choice = p?.choices?.[0];
            const delta = choice?.delta;
            if (delta?.reasoning_content) o.onThinking?.(delta.reasoning_content);
            if (delta?.content) o.onText(delta.content);
            if (choice?.finish_reason) r.finishReason = choice.finish_reason;
            if (p?.usage) r.usage = { input: p.usage.prompt_tokens, output: p.usage.completion_tokens };
        },
    },
    anthropic: {
        url: (o) => `${o.baseUrl}/v1/messages`,
        headers: { 'anthropic-version': '2023-06-01' },
        body: (o) => ({
            model: o.model,
            stream: true,
            max_tokens: 8192,
            ...(o.temperature !== undefined && { temperature: o.temperature }),
            ...(o.system && { system: o.system }),
            messages: o.messages.map((m) => ({
                role: m.role,
                content: m.attachments?.length
                    ? buildParts<Record<string, unknown>>(m, {
                        text: (text) => ({ type: 'text', text }),
                        image: (a) => ({ type: 'image', source: { type: 'base64', media_type: a.mime, data: a.data } }),
                        pdf: (a) => ({ type: 'document', source: { type: 'base64', media_type: a.mime, data: a.data } }),
                    })
                    : m.content,
            })),
        }),
        handle: (p, o, r) => {
            switch (p?.type) {
                case 'message_start':
                    r.usage = { ...r.usage, input: p.message?.usage?.input_tokens };
                    break;
                case 'content_block_delta':
                    if (p.delta?.type === 'text_delta') o.onText(p.delta.text ?? '');
                    else if (p.delta?.type === 'thinking_delta') o.onThinking?.(p.delta.thinking ?? '');
                    break;
                case 'message_delta':
                    if (p.delta?.stop_reason) r.finishReason = p.delta.stop_reason;
                    if (p.usage) {
                        r.usage = {
                            input: p.usage.input_tokens ?? r.usage?.input,
                            output: p.usage.output_tokens,
                        };
                    }
                    break;
                case 'error':
                    throw new ChatRequestError(p.error?.message ?? JSON.stringify(p.error));
            }
        },
    },
    gemini: {
        url: (o) => `${o.baseUrl}/v1beta/models/${encodeURIComponent(o.model)}:streamGenerateContent?alt=sse`,
        body: (o) => ({
            contents: o.messages.map((m) => ({
                role: m.role === 'assistant' ? 'model' : 'user',
                parts: buildParts<Record<string, unknown>>(m, {
                    text: (text) => ({ text }),
                    image: (a) => ({ inlineData: { mimeType: a.mime, data: a.data } }),
                    pdf: (a) => ({ inlineData: { mimeType: a.mime, data: a.data } }),
                }),
            })),
            ...(o.system && { systemInstruction: { parts: [{ text: o.system }] } }),
            ...(o.temperature !== undefined && { generationConfig: { temperature: o.temperature } }),
        }),
        handle: (raw, o, r) => {
            const p = raw?.response ?? raw;
            const candidate = p?.candidates?.[0];
            for (const part of candidate?.content?.parts ?? []) {
                if (typeof part?.text !== 'string') continue;
                if (part.thought) o.onThinking?.(part.text);
                else o.onText(part.text);
            }
            if (candidate?.finishReason) r.finishReason = candidate.finishReason;
            if (p?.usageMetadata) {
                r.usage = { input: p.usageMetadata.promptTokenCount, output: p.usageMetadata.candidatesTokenCount };
            }
        },
    },
};

/** Send a streaming chat request through the local proxy using the chosen client protocol. */
export async function streamChat(opts: StreamChatOptions): Promise<StreamChatResult> {
    const adapter = ADAPTERS[opts.protocol];
    const res = await fetch(adapter.url(opts), {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
            Accept: 'text/event-stream',
            ...authHeaders(opts.apiKey),
            ...adapter.headers,
        },
        body: JSON.stringify(adapter.body(opts)),
        signal: opts.signal,
    });

    if (!res.ok) throw new ChatRequestError(await readError(res), res.status);

    const result: StreamChatResult = {};

    // Some upstream paths may still answer with a single JSON document.
    const contentType = res.headers.get('content-type') ?? '';
    if (!res.body || contentType.includes('application/json')) {
        const json = await res.json();
        handleFullResponse(opts, json, result);
        return result;
    }

    for await (const msg of readSse(res.body)) {
        if (msg.data === '[DONE]') break;
        let payload: any;
        try {
            payload = JSON.parse(msg.data);
        } catch {
            continue;
        }
        if (payload?.error && opts.protocol !== 'anthropic') {
            throw new ChatRequestError(payload.error.message ?? JSON.stringify(payload.error));
        }
        adapter.handle(payload, opts, result);
    }
    return result;
}

function handleFullResponse(opts: StreamChatOptions, json: any, result: StreamChatResult) {
    if (json?.error) throw new ChatRequestError(json.error.message ?? JSON.stringify(json.error));
    switch (opts.protocol) {
        case 'openai': {
            const msg = json?.choices?.[0]?.message;
            if (msg?.reasoning_content) opts.onThinking?.(msg.reasoning_content);
            if (msg?.content) opts.onText(msg.content);
            result.finishReason = json?.choices?.[0]?.finish_reason;
            if (json?.usage) result.usage = { input: json.usage.prompt_tokens, output: json.usage.completion_tokens };
            break;
        }
        case 'anthropic': {
            for (const block of json?.content ?? []) {
                if (block.type === 'text') opts.onText(block.text);
                else if (block.type === 'thinking') opts.onThinking?.(block.thinking);
            }
            result.finishReason = json?.stop_reason;
            if (json?.usage) result.usage = { input: json.usage.input_tokens, output: json.usage.output_tokens };
            break;
        }
        case 'gemini':
            for (const chunk of Array.isArray(json) ? json : [json]) ADAPTERS.gemini.handle(chunk, opts, result);
            break;
    }
}
