/**
 * Minimal Server-Sent Events reader for `fetch` response bodies.
 *
 * Yields one `{ event, data }` per SSE message (blank-line delimited).
 * Multiple `data:` lines inside a message are joined with `\n` per the spec.
 */
export interface SseMessage {
    event?: string;
    data: string;
}

export async function* readSse(body: ReadableStream<Uint8Array>): AsyncGenerator<SseMessage> {
    const reader = body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';
    let event: string | undefined;
    let dataLines: string[] = [];

    const flush = (): SseMessage | null => {
        if (dataLines.length === 0) {
            event = undefined;
            return null;
        }
        const msg = { event, data: dataLines.join('\n') };
        event = undefined;
        dataLines = [];
        return msg;
    };

    try {
        while (true) {
            const { value, done } = await reader.read();
            buffer += done ? decoder.decode() : decoder.decode(value, { stream: true });

            let newline: number;
            while ((newline = buffer.indexOf('\n')) !== -1) {
                let line = buffer.slice(0, newline);
                buffer = buffer.slice(newline + 1);
                if (line.endsWith('\r')) line = line.slice(0, -1);

                if (line === '') {
                    const msg = flush();
                    if (msg) yield msg;
                    continue;
                }
                if (line.startsWith(':')) continue; // comment / keep-alive

                const colon = line.indexOf(':');
                const field = colon === -1 ? line : line.slice(0, colon);
                let value = colon === -1 ? '' : line.slice(colon + 1);
                if (value.startsWith(' ')) value = value.slice(1);

                if (field === 'data') dataLines.push(value);
                else if (field === 'event') event = value;
            }

            if (done) {
                if (buffer) {
                    if (buffer.startsWith('data:')) dataLines.push(buffer.slice(5).trimStart());
                    buffer = '';
                }
                const msg = flush();
                if (msg) yield msg;
                return;
            }
        }
    } finally {
        reader.releaseLock();
    }
}
