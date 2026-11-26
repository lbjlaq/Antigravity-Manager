/**
 * Unit tests for redacting OAuth secret tokens from localStorage account caches.
 * Run: npx tsx src/utils/__tests__/accountCacheSanitize.test.ts
 */
import type { Account } from '../../types/account';
import {
    sanitizeAccountForCache,
    sanitizeAccountsForCache,
} from '../accountCacheSanitize';

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

test('sanitizeAccountForCache scrubs secret tokens while keeping display metadata', () => {
    const raw = {
        id: 'acc-1',
        email: 'user@example.com',
        token: {
            access_token: 'ya29.secret-access-token',
            refresh_token: '1//secret-refresh-token',
            id_token: 'eyJhbGciOiJSUzI1NiJ9.secret-id-token',
            session_id: 'secret-session-id',
            expires_in: 3600,
            expiry_timestamp: 2000000000,
            token_type: 'Bearer',
            project_id: 'proj-123',
            oauth_client_key: 'ide',
            is_gcp_tos: true,
        },
        created_at: 1700000000,
        last_used: 1700000000,
    } as Account;

    const sanitized = sanitizeAccountForCache(raw);

    assertEqual(sanitized.token.access_token, '', 'access_token must be scrubbed');
    assertEqual(sanitized.token.refresh_token, '', 'refresh_token must be scrubbed');
    assertEqual('id_token' in sanitized.token, false, 'id_token must be removed');
    assertEqual('session_id' in sanitized.token, false, 'session_id must be removed');
    assertEqual(sanitized.token.project_id, 'proj-123');
    assertEqual(sanitized.token.oauth_client_key, 'ide');
    assertEqual(sanitized.token.is_gcp_tos, true);

    // Original in-memory object must not be mutated
    assertEqual(raw.token.access_token, 'ya29.secret-access-token');
    assertEqual(raw.token.refresh_token, '1//secret-refresh-token');
});

test('sanitizeAccountsForCache scrubs every item in a list', () => {
    const list = sanitizeAccountsForCache([
        {
            id: 'acc-2',
            email: 'two@example.com',
            token: {
                access_token: 'ya29.two',
                refresh_token: '1//two',
                expires_in: 3600,
                expiry_timestamp: 2000000000,
                token_type: 'Bearer',
            },
            created_at: 1700000000,
            last_used: 1700000000,
        },
    ]);

    assertEqual(list.length, 1);
    assertEqual(list[0].token.access_token, '');
    assertEqual(list[0].token.refresh_token, '');
});

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) {
    throw new Error(`${failed} test(s) failed`);
}
