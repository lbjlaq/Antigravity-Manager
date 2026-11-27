/**
 * Unit tests for dashboard greeting name extraction and client target resolution.
 * Run: npx tsx src/utils/__tests__/dashboardGreeting.test.ts
 */
import type { Account, ActiveClientsStatus } from '../../types/account';
import {
    extractAccountDisplayName,
    resolveActiveClientAccount,
    resolveInitialClientTarget,
} from '../dashboardGreeting';

function assertEqual<T>(actual: T, expected: T, label?: string): void {
    if (actual !== expected) {
        throw new Error(
            `${label ? `${label}: ` : ''}expected ${String(expected)}, got ${String(actual)}`
        );
    }
}

const makeAccount = (id: string, email: string, name?: string): Account => ({
    id,
    email,
    name,
    token: {
        access_token: 'tok',
        refresh_token: 'ref',
        expires_in: 3600,
        expiry_timestamp: 9999999999,
        token_type: 'Bearer',
    },
    created_at: 1,
    last_used: 1,
});

// extractAccountDisplayName
assertEqual(extractAccountDisplayName(null), null);
assertEqual(
    extractAccountDisplayName({ email: 'triyudan354313@gmail.com', name: 'Jizdan' }),
    'Jizdan'
);
assertEqual(
    extractAccountDisplayName({ email: 'triyudan354313@gmail.com', name: '   ' }),
    'triyudan354313'
);
assertEqual(
    extractAccountDisplayName({ email: 'triyudan354313@gmail.com' }),
    'triyudan354313'
);

// resolveInitialClientTarget
assertEqual(resolveInitialClientTarget(undefined), 'classic');
assertEqual(resolveInitialClientTarget({ ide_account_id: 'a2' }), 'ide');
assertEqual(resolveInitialClientTarget({ agy_account_id: 'a3', acp_account_id: 'a4' }), 'agy');
assertEqual(
    resolveInitialClientTarget({ classic_account_id: 'a1', ide_account_id: 'a2' }),
    'classic'
);

// resolveActiveClientAccount
const acc1 = makeAccount('a1', 'classic@example.com');
const acc2 = makeAccount('a2', 'ide@example.com');
const clients: ActiveClientsStatus = { classic_account_id: 'a1', ide_account_id: 'a2' };

assertEqual(resolveActiveClientAccount('ide', [acc1, acc2], clients, acc1)?.id, 'a2');
assertEqual(resolveActiveClientAccount('agy', [acc1, acc2], clients, acc1), null);
assertEqual(resolveActiveClientAccount('classic', [acc1, acc2], {}, acc1)?.id, 'a1');

console.log('dashboardGreeting tests passed');
