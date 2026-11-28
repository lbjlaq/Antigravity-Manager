/**
 * Unit tests for HTTP status classification helpers (including HTTP 101 Switching Protocols).
 * Run: npx tsx src/utils/__tests__/httpStatus.test.ts
 */
import { isProxySuccessStatus, isStrictProxySuccessStatus } from '../httpStatus';
import { getStatusText } from '../harExport';

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

test('isProxySuccessStatus treats 101 and 2xx/3xx as success and 4xx/5xx as non-success', () => {
    assertEqual(isProxySuccessStatus(101), true, '101 Switching Protocols');
    assertEqual(isProxySuccessStatus(200), true, '200 OK');
    assertEqual(isProxySuccessStatus(204), true, '204 No Content');
    assertEqual(isProxySuccessStatus(304), true, '304 Not Modified');
    assertEqual(isProxySuccessStatus(100), false, '100 Continue');
    assertEqual(isProxySuccessStatus(400), false, '400 Bad Request');
    assertEqual(isProxySuccessStatus(429), false, '429 Too Many Requests');
    assertEqual(isProxySuccessStatus(500), false, '500 Internal Server Error');
    assertEqual(isProxySuccessStatus(0), false, '0');
    assertEqual(isProxySuccessStatus(undefined), false, 'undefined');
    assertEqual(isProxySuccessStatus(null), false, 'null');
});

test('isStrictProxySuccessStatus treats 101 and 2xx as success and 3xx/4xx/5xx as non-success', () => {
    assertEqual(isStrictProxySuccessStatus(101), true, '101 Switching Protocols');
    assertEqual(isStrictProxySuccessStatus(200), true, '200 OK');
    assertEqual(isStrictProxySuccessStatus(299), true, '299');
    assertEqual(isStrictProxySuccessStatus(301), false, '301');
    assertEqual(isStrictProxySuccessStatus(400), false, '400');
    assertEqual(isStrictProxySuccessStatus(502), false, '502');
});

test('getStatusText returns Switching Protocols for 101', () => {
    assertEqual(getStatusText(101), 'Switching Protocols');
});

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) {
    throw new Error(`${failed} test(s) failed`);
}
