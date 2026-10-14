/**
 * Chat attachments: file classification, reading and persistence.
 *
 * Message metadata lives in the chat store (localStorage); the payloads are kept in
 * IndexedDB because localStorage is far too small for images and documents. An
 * in-memory cache fronts IndexedDB so the page keeps working when it is unavailable.
 */

export type AttachmentKind = 'image' | 'pdf' | 'text';

export interface ChatAttachment {
    id: string;
    name: string;
    mime: string;
    size: number;
    kind: AttachmentKind;
}

/** Attachment with its payload: base64 (no data: prefix) for image/pdf, UTF-8 text for text. */
export interface LoadedAttachment extends ChatAttachment {
    data: string;
}

export const MAX_ATTACHMENT_BYTES = 20 * 1024 * 1024;
export const MAX_ATTACHMENTS = 10;

/** Image formats accepted by every upstream protocol (and by the proxy's inline-data validation). */
const IMAGE_MIMES = new Set(['image/png', 'image/jpeg', 'image/webp', 'image/gif', 'image/heic', 'image/heif']);

export type AttachmentErrorCode = 'unsupported' | 'too_large';

export class AttachmentError extends Error {
    constructor(public code: AttachmentErrorCode, public fileName: string) {
        super(`${code}: ${fileName}`);
        this.name = 'AttachmentError';
    }
}

const TEXT_SNIFF_BYTES = 64 * 1024;

/** Text files are recognised by content, not extension, so any source/config/log format works. */
async function isTextFile(file: File): Promise<boolean> {
    if (file.type.startsWith('text/')) return true;
    const head = new Uint8Array(await file.slice(0, TEXT_SNIFF_BYTES).arrayBuffer());
    if (head.includes(0)) return false;
    try {
        // A multi-byte sequence may be cut at the sniff boundary, so decode in streaming mode.
        new TextDecoder('utf-8', { fatal: true }).decode(head, { stream: file.size > TEXT_SNIFF_BYTES });
        return true;
    } catch {
        return false;
    }
}

async function classify(file: File): Promise<{ kind: AttachmentKind; mime: string } | null> {
    if (IMAGE_MIMES.has(file.type)) return { kind: 'image', mime: file.type };
    if (file.type === 'application/pdf' || /\.pdf$/i.test(file.name)) return { kind: 'pdf', mime: 'application/pdf' };
    if (await isTextFile(file)) return { kind: 'text', mime: file.type || 'text/plain' };
    return null;
}

function toBase64(buffer: ArrayBuffer): string {
    const bytes = new Uint8Array(buffer);
    let binary = '';
    const CHUNK = 0x8000;
    for (let i = 0; i < bytes.length; i += CHUNK) {
        binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
    }
    return btoa(binary);
}

export async function readAttachment(file: File, id: string): Promise<LoadedAttachment> {
    if (file.size > MAX_ATTACHMENT_BYTES) throw new AttachmentError('too_large', file.name);
    const type = await classify(file);
    if (!type) throw new AttachmentError('unsupported', file.name);
    const name = file.name || (type.kind === 'image' ? 'image' : 'file');
    const data = type.kind === 'text' ? await file.text() : toBase64(await file.arrayBuffer());
    return { id, name, mime: type.mime, size: file.size, kind: type.kind, data };
}

export const toDataUrl = (a: Pick<LoadedAttachment, 'mime' | 'data'>) => `data:${a.mime};base64,${a.data}`;

/** How text files are presented to the model, identical across protocols. */
export const textFilePrompt = (a: Pick<LoadedAttachment, 'name' | 'data'>) => `[File: ${a.name}]\n${a.data}`;

export function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

// ── Persistence ────────────────────────────────────────────────────────────

const DB_NAME = 'abv_chat';
const STORE = 'attachments';
const memory = new Map<string, string>();
let dbPromise: Promise<IDBDatabase> | null = null;

function openDb(): Promise<IDBDatabase> {
    if (!dbPromise) {
        dbPromise = new Promise<IDBDatabase>((resolve, reject) => {
            const req = indexedDB.open(DB_NAME, 1);
            req.onupgradeneeded = () => req.result.createObjectStore(STORE);
            req.onsuccess = () => resolve(req.result);
            req.onerror = () => reject(req.error);
        });
        dbPromise.catch(() => {
            dbPromise = null;
        });
    }
    return dbPromise;
}

async function withStore<T>(mode: IDBTransactionMode, run: (store: IDBObjectStore) => IDBRequest<T> | void): Promise<T | undefined> {
    const db = await openDb();
    return new Promise<T | undefined>((resolve, reject) => {
        const tx = db.transaction(STORE, mode);
        const req = run(tx.objectStore(STORE));
        tx.oncomplete = () => resolve(req ? req.result : undefined);
        tx.onerror = () => reject(tx.error);
        tx.onabort = () => reject(tx.error);
    });
}

export async function saveAttachmentData(id: string, data: string): Promise<void> {
    memory.set(id, data);
    try {
        await withStore('readwrite', (s) => s.put(data, id));
    } catch (err) {
        console.warn('[Chat] Attachment kept in memory only (IndexedDB unavailable):', err);
    }
}

export async function loadAttachmentData(id: string): Promise<string | undefined> {
    const cached = memory.get(id);
    if (cached !== undefined) return cached;
    try {
        const data = await withStore<string>('readonly', (s) => s.get(id));
        if (typeof data === 'string') memory.set(id, data);
        return typeof data === 'string' ? data : undefined;
    } catch {
        return undefined;
    }
}

export async function deleteAttachmentData(ids: string[]): Promise<void> {
    if (ids.length === 0) return;
    ids.forEach((id) => memory.delete(id));
    try {
        await withStore('readwrite', (s) => {
            ids.forEach((id) => s.delete(id));
        });
    } catch {
        // nothing persisted, nothing to clean
    }
}
