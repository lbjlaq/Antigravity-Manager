/**
 * 统一事件订阅入口（双传输事件桥，见 docs/WEBUI_NO_WEBVIEW2_PLAN.md 阶段 1）
 *
 * - Tauri：走 @tauri-apps/api/event 的 listen（桌面行为不变）；
 * - 浏览器：全页共享一条 /api/events SSE 流（fetch + ReadableStream，
 *   可携带 Authorization 头，避免 query token 泄漏到日志）。
 *
 * SSE 断连自动重连（指数退避），连续失败后进入 dead 终态；
 * 调用方可用 onBridgeStateChange 监听状态、在 dead 时自行启用轮询兜底。
 */
import { isTauri } from './env';

type Handler = (payload: any) => void;

export type BridgeState = 'tauri' | 'idle' | 'connecting' | 'open' | 'dead';

const webHandlers = new Map<string, Set<Handler>>();
const stateListeners = new Set<(s: BridgeState) => void>();
let sseState: Exclude<BridgeState, 'tauri'> = 'idle';
let sseAbort: AbortController | null = null;

export function bridgeState(): BridgeState {
    return isTauri() ? 'tauri' : sseState;
}

/** 订阅事件桥状态变化（用于降级到轮询兜底） */
export function onBridgeStateChange(cb: (s: BridgeState) => void): () => void {
    stateListeners.add(cb);
    return () => {
        stateListeners.delete(cb);
    };
}

function setState(s: Exclude<BridgeState, 'tauri'>) {
    sseState = s;
    stateListeners.forEach((cb) => {
        try {
            cb(s);
        } catch {
            /* listener 错误不影响其他订阅者 */
        }
    });
}

function getApiKey(): string | null {
    return typeof window !== 'undefined' ? sessionStorage.getItem('abv_admin_api_key') : null;
}

const RECONNECT_DELAYS = [1000, 2000, 4000, 8000, 15000];

/** 连接主循环：同一时刻只允许一条连接；连续失败 5 次后置 dead */
async function connectLoop(): Promise<void> {
    if (sseState === 'connecting' || sseState === 'open') return;
    setState('connecting');
    let attempt = 0;
    let wasAborted = false;
    while (attempt < RECONNECT_DELAYS.length) {
        const controller = new AbortController();
        sseAbort = controller;
        try {
            const apiKey = getApiKey();
            const res = await fetch('/api/events', {
                headers: {
                    Accept: 'text/event-stream',
                    ...(apiKey
                        ? { Authorization: `Bearer ${apiKey}`, 'x-api-key': apiKey }
                        : {}),
                },
                signal: controller.signal,
            });
            if (res.status === 401) {
                window.dispatchEvent(new CustomEvent('abv-unauthorized'));
                break;
            }
            if (!res.ok || !res.body) throw new Error(`SSE HTTP ${res.status}`);
            setState('open');
            attempt = 0;
            await parseStream(res.body);
            // 服务端关闭了流 → 重连
        } catch (e: any) {
            if (e?.name === 'AbortError') {
                wasAborted = true;
                break;
            }
        }
        if (controller.signal.aborted) {
            wasAborted = true;
            break;
        }
        setState('connecting');
        await new Promise((r) => setTimeout(r, RECONNECT_DELAYS[Math.min(attempt, RECONNECT_DELAYS.length - 1)]));
        attempt += 1;
    }
    // 被主动断开（closeBridge）时不覆盖调用方设置的状态
    if (!wasAborted) setState('dead');
}

/** 解析 text/event-stream（SSE 规范的最小实现） */
async function parseStream(body: ReadableStream<Uint8Array>): Promise<void> {
    const reader = body.getReader();
    const decoder = new TextDecoder();
    let buf = '';
    let eventName = 'message';
    for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        let idx: number;
        while ((idx = buf.indexOf('\n')) >= 0) {
            const line = buf.slice(0, idx).replace(/\r$/, '');
            buf = buf.slice(idx + 1);
            if (line === '') {
                eventName = 'message'; // 空行 = 事件边界
            } else if (line.startsWith('event:')) {
                eventName = line.slice(6).trim();
            } else if (line.startsWith('data:')) {
                const raw = line.slice(5).trim();
                let payload: any = raw;
                try {
                    payload = JSON.parse(raw);
                } catch {
                    /* 非 JSON payload 保留原文 */
                }
                dispatch(eventName, payload);
            }
        }
    }
}

function dispatch(event: string, payload: any) {
    const set = webHandlers.get(event);
    if (!set) return;
    set.forEach((h) => {
        try {
            h(payload);
        } catch (e) {
            console.error(`[events] handler error for "${event}":`, e);
        }
    });
}

/**
 * 订阅后端事件。
 * 返回取消订阅函数（Promise 形式，与 Tauri listen 的返回签名对齐）。
 */
export async function subscribe<T = any>(
    event: string,
    handler: (payload: T) => void
): Promise<() => void> {
    if (isTauri()) {
        const { listen } = await import('@tauri-apps/api/event');
        const unlisten = await listen<T>(event, (e) => handler(e.payload));
        return unlisten;
    }
    let set = webHandlers.get(event);
    if (!set) {
        set = new Set();
        webHandlers.set(event, set);
    }
    set.add(handler as Handler);
    void connectLoop();
    return () => {
        const s = webHandlers.get(event);
        if (!s) return;
        s.delete(handler as Handler);
        if (s.size === 0) webHandlers.delete(event);
    };
}

/** 主动断开 SSE 连接（登出 / 页面卸载时调用） */
export function closeBridge(): void {
    sseAbort?.abort();
    sseAbort = null;
    setState('idle');
}
