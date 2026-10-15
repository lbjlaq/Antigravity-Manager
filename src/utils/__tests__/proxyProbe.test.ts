/**
 * pingModel must classify upstream replies into ok / fail / limited / skipped.
 * Run: npx tsx src/utils/__tests__/proxyProbe.test.ts
 */
import { classifyReply, pingModel, sendProbe, ProbeInputError } from '../proxyProbe';

let passed = 0;
let failed = 0;
let fetchCalls = 0;

async function test(description: string, fn: () => Promise<void>): Promise<void> {
    try {
        await fn();
        passed += 1;
        console.log(`  ok  ${description}`);
    } catch (err) {
        failed += 1;
        console.error(`  FAIL  ${description}`);
        console.error(err);
    }
}

function assertEqual<T>(actual: T, expected: T): void {
    if (actual !== expected) {
        throw new Error(`expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
    }
}

function mockFetch(status: number, body: unknown): void {
    fetchCalls = 0;
    globalThis.fetch = (async () => {
        fetchCalls += 1;
        return new Response(typeof body === 'string' ? body : JSON.stringify(body), { status });
    }) as typeof fetch;
}

const BASE = 'http://127.0.0.1:8045';

async function main(): Promise<void> {
    await test('200 with content → ok', async () => {
        mockFetch(200, { choices: [{ message: { content: 'pong' } }] });
        const r = await pingModel(BASE, 'sk', 'gemini-3.8-flash');
        assertEqual(r.state, 'ok');
        assertEqual(r.status, 200);
    });

    await test('200 with a retirement notice instead of pong → fail with that text', async () => {
        const notice = 'Gemini 3.5 Flash is no longer available. Please switch to Gemini 3.7 Flash in the latest version of Antigravity.';
        mockFetch(200, { choices: [{ message: { content: notice } }] });
        const r = await pingModel(BASE, 'sk', 'gemini-3.5-flash-low');
        assertEqual(r.state, 'fail');
        assertEqual(r.status, 200);
        assertEqual(r.message, notice);
    });

    await test('200 with empty content → fail', async () => {
        mockFetch(200, { choices: [{ message: { content: '' } }] });
        const r = await pingModel(BASE, 'sk', 'gemini-3.8-flash');
        assertEqual(r.state, 'fail');
        assertEqual(r.status, 200);
    });

    await test('429 → limited', async () => {
        mockFetch(429, { error: { message: 'quota exhausted' } });
        const r = await pingModel(BASE, 'sk', 'claude-opus-4-6-thinking');
        assertEqual(r.state, 'limited');
        assertEqual(r.status, 429);
    });

    await test('404 with error.message → fail with that message', async () => {
        mockFetch(404, { error: { message: 'model not available, switch to gemini-3.8-flash' } });
        const r = await pingModel(BASE, 'sk', 'gemini-3.5-flash-low');
        assertEqual(r.state, 'fail');
        assertEqual(r.status, 404);
        assertEqual(r.message, 'model not available, switch to gemini-3.8-flash');
    });

    await test('non-JSON error body → fail with truncated body text', async () => {
        mockFetch(502, 'x'.repeat(1000));
        const r = await pingModel(BASE, 'sk', 'gemini-3.8-flash');
        assertEqual(r.state, 'fail');
        assertEqual(r.message.length, 300);
    });

    await test('*-image → skipped without a request', async () => {
        mockFetch(200, {});
        const r = await pingModel(BASE, 'sk', 'gemini-3-pro-image');
        assertEqual(r.state, 'skipped');
        assertEqual(fetchCalls, 0);
    });

    await test('sendProbe: invalid JSON is a local error, no request sent', async () => {
        mockFetch(200, {});
        let error: unknown = null;
        try {
            await sendProbe(BASE, 'sk', 'openai', 'm', '{"model":', undefined);
        } catch (e) {
            error = e;
        }
        assertEqual(error instanceof ProbeInputError, true);
        assertEqual(fetchCalls, 0);
    });

    await test('pingModel reports the model that actually answered', async () => {
        mockFetch(200, { model: 'gemini-3.5-flash-low', choices: [{ message: { content: 'no longer available' } }] });
        const r = await pingModel(BASE, 'sk', 'gemini-3-flash');
        assertEqual(r.state, 'fail');
        assertEqual(r.servedModel, 'gemini-3.5-flash-low');
    });

    await test('pingModel keeps the full exchange for the probe, with a single request', async () => {
        mockFetch(200, { choices: [{ message: { content: 'pong' } }] });
        const r = await pingModel(BASE, 'sk', 'gemini-3.8-flash');
        assertEqual(fetchCalls, 1);
        assertEqual(r.probe?.status, 200);
        assertEqual(r.probe?.body.includes('pong'), true);
    });

    await test('classifyReply: Anthropic and Gemini replies with pong → ok', async () => {
        assertEqual(classifyReply('anthropic', 200, JSON.stringify({ content: [{ type: 'text', text: 'pong' }] }))?.state, 'ok');
        assertEqual(classifyReply('gemini', 200, JSON.stringify({ candidates: [{ content: { parts: [{ text: 'Pong.' }] } }] }))?.state, 'ok');
    });

    await test('classifyReply: raw SSE body has no verdict', async () => {
        assertEqual(classifyReply('openai', 200, 'data: {"choices":[]}\n\ndata: [DONE]\n'), null);
    });

    console.log(`\n${passed} passed, ${failed} failed`);
    if (failed > 0) {
        throw new Error(`${failed} test(s) failed`);
    }
}

// An unhandled rejection exits non-zero.
void main();
