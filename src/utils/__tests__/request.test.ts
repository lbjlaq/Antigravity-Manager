/**
 * Unit tests for Web API request URL & parameter serialization.
 * Run: npx --yes tsx src/utils/__tests__/request.test.ts
 */
import { buildWebRequestPayload } from '../request';

let passed = 0;
let failed = 0;

function test(description: string, fn: () => void): void {
    try {
        fn();
        passed += 1;
        console.log(`  ok  ${description}`);
    } catch (err) {
        failed += 1;
        console.error(`  FAIL  ${description}`);
        console.error(err);
    }
}

function assertEqual<T>(actual: T, expected: T, msg?: string): void {
    if (actual !== expected) {
        throw new Error(
            `${msg ? msg + ': ' : ''}expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`
        );
    }
}

test('keeps GET query parameters whose values appear as substrings in the URL path', () => {
    const built = buildWebRequestPayload('/api/logs', 'GET', {
        filter: 'api',
        errorsOnly: false,
        limit: 20,
        offset: 0,
    });
    assertEqual(built.url, '/api/logs?filter=api&errorsOnly=false&limit=20&offset=0');
});

test('keeps numeric query parameters matching path characters', () => {
    const built = buildWebRequestPayload('/api/accounts/:accountId/quota', 'GET', {
        accountId: 'acc-1',
        limit: 1,
    });
    assertEqual(built.url, '/api/accounts/acc-1/quota?limit=1');
});

test('strips substituted path parameter from POST body', () => {
    const built = buildWebRequestPayload('/api/accounts/:accountId/remove-forbidden-model', 'POST', {
        accountId: 'acc-42',
        model: 'claude-sonnet',
    });
    assertEqual(built.url, '/api/accounts/acc-42/remove-forbidden-model');
    assertEqual(built.body, JSON.stringify({ model: 'claude-sonnet' }));
});

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) {
    throw new Error(`${failed} test(s) failed`);
}
