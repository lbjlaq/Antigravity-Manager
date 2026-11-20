/**
 * Unit tests for account 403 recovery guard and warm-up error formatting.
 * Run: npx tsx src/utils/__tests__/accountStatus.test.ts
 */
import type { Account } from '../../types/account';
import {
    canClearForbiddenLock,
    formatWarmUpErrorDetail,
    isClearableProxyLockReason,
    isLocationUnsupportedReason,
} from '../accountStatus';

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

function makeAccount(overrides: Partial<Account> = {}): Account {
    return {
        id: 'acc-1',
        email: 'user@example.com',
        token: {
            access_token: 'tok',
            refresh_token: 'rt',
            expires_in: 3600,
            expiry_timestamp: 2000000000,
            token_type: 'Bearer',
        },
        created_at: 1700000000,
        last_used: 1700000000,
        ...overrides,
    };
}

test('disabled account (invalid_grant) with proxy_disabled does NOT show 403 clear button', () => {
    const acc = makeAccount({
        disabled: true,
        disabled_reason: 'invalid_grant: Refresh failed for all OAuth clients',
        proxy_disabled: true,
        proxy_disabled_reason: 'dead_account_quarantine: disabled/invalid_grant',
    });
    assertEqual(canClearForbiddenLock(acc), false);
});

test('disabled account even with stale quota.is_forbidden does NOT show 403 clear button', () => {
    const acc = makeAccount({
        disabled: true,
        disabled_reason: 'invalid_grant',
        quota: {
            models: [],
            last_updated: 1700000000,
            is_forbidden: true,
            forbidden_reason: '403 Forbidden',
        },
    });
    assertEqual(canClearForbiddenLock(acc), false);
});

test('banned account (TOS_VIOLATION) does NOT show 403 clear button', () => {
    const acc = makeAccount({
        banned: true,
        banned_code: 'TOS_VIOLATION',
        banned_reason: 'This service has been disabled in this account for violation of Terms of Service.',
        proxy_disabled: true,
        proxy_disabled_reason: 'banned: TOS_VIOLATION',
        quota: {
            models: [],
            last_updated: 1700000000,
            is_forbidden: true,
        },
    });
    assertEqual(canClearForbiddenLock(acc), false);
});

test('manually proxy-disabled account without 403/validation lock does NOT show 403 clear button', () => {
    const manual = makeAccount({
        proxy_disabled: true,
        manual_proxy_disabled: true,
        proxy_disabled_reason: '用户手动禁用',
    });
    assertEqual(canClearForbiddenLock(manual), false, 'manual_proxy_disabled');

    const batch = makeAccount({
        proxy_disabled: true,
        proxy_disabled_reason: '批量禁用',
    });
    assertEqual(canClearForbiddenLock(batch), false, 'batch disable');

    const quotaProt = makeAccount({
        proxy_disabled: true,
        proxy_disabled_reason: 'quota_protection',
    });
    assertEqual(canClearForbiddenLock(quotaProt), false, 'quota_protection');

    const restricted = makeAccount({
        proxy_disabled: true,
        proxy_disabled_reason: 'paid_models_restricted',
    });
    assertEqual(canClearForbiddenLock(restricted), false, 'paid_models_restricted');
});

test('403 / validation / model-forbidden / location-locked accounts DO show 403 clear button', () => {
    assertEqual(
        canClearForbiddenLock(
            makeAccount({
                quota: {
                    models: [],
                    last_updated: 1700000000,
                    is_forbidden: true,
                    forbidden_reason: '403 Forbidden',
                },
            })
        ),
        true,
        'quota.is_forbidden'
    );

    assertEqual(
        canClearForbiddenLock(
            makeAccount({
                validation_blocked: true,
                validation_blocked_reason: 'VALIDATION_REQUIRED',
                proxy_disabled: true,
                proxy_disabled_reason: 'validation_required',
            })
        ),
        true,
        'validation_blocked'
    );

    assertEqual(
        canClearForbiddenLock(
            makeAccount({
                forbidden_models: ['claude-sonnet-5-5'],
            })
        ),
        true,
        'forbidden_models'
    );

    assertEqual(
        canClearForbiddenLock(
            makeAccount({
                proxy_disabled: true,
                proxy_disabled_reason: 'Forbidden (403): access denied',
            })
        ),
        true,
        'proxy_disabled with 403 reason'
    );

    assertEqual(
        canClearForbiddenLock(
            makeAccount({
                proxy_disabled: true,
                proxy_disabled_reason: 'UNSUPPORTED_LOCATION: User location not allowed',
            })
        ),
        true,
        'proxy_disabled with location error'
    );
});

test('isLocationUnsupportedReason matches all backend location-unsupported patterns', () => {
    assertEqual(isLocationUnsupportedReason('User location is not supported'), true);
    assertEqual(isLocationUnsupportedReason('Location not supported'), true);
    assertEqual(isLocationUnsupportedReason('location is not supported for API use'), true);
    assertEqual(isLocationUnsupportedReason('User location not allowed'), true);
    assertEqual(isLocationUnsupportedReason('UNSUPPORTED_LOCATION'), true);
    assertEqual(isLocationUnsupportedReason('invalid_grant'), false);
    assertEqual(isLocationUnsupportedReason('TOS_VIOLATION'), false);
});

test('isClearableProxyLockReason excludes banned and manual reasons', () => {
    assertEqual(isClearableProxyLockReason('Forbidden (403): denied'), true);
    assertEqual(isClearableProxyLockReason('validation_required'), true);
    assertEqual(isClearableProxyLockReason('Location not supported'), true);
    assertEqual(isClearableProxyLockReason('banned: TOS_VIOLATION'), false);
    assertEqual(isClearableProxyLockReason('quota_protection'), false);
    assertEqual(isClearableProxyLockReason('批量禁用'), false);
    assertEqual(isClearableProxyLockReason(''), false);
});

test('formatWarmUpErrorDetail surfaces plain, Error, JSON, and truncated messages', () => {
    assertEqual(
        formatWarmUpErrorDetail('Account requires validation (VALIDATION_REQUIRED)'),
        'Account requires validation (VALIDATION_REQUIRED)'
    );
    assertEqual(
        formatWarmUpErrorDetail(new Error('Account is disabled')),
        'Account is disabled'
    );
    assertEqual(
        formatWarmUpErrorDetail('{"error":{"code":403,"message":"Verify your account to continue."}}'),
        'Verify your account to continue.'
    );
    assertEqual(formatWarmUpErrorDetail('   '), '');
    assertEqual(formatWarmUpErrorDetail(null), '');
    const longMsg = 'x'.repeat(220);
    const formatted = formatWarmUpErrorDetail(longMsg, 50);
    assertEqual(formatted.length <= 50, true, 'truncated length <= 50');
    assertEqual(formatted.endsWith('...'), true, 'truncated ends with ...');
});

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) {
    throw new Error(`${failed} test(s) failed`);
}
