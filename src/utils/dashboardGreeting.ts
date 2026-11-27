import type { Account, ActiveClientsStatus } from '../types/account';

export type ClientTarget = 'classic' | 'ide' | 'agy' | 'acp';

const CLIENT_ORDER: ClientTarget[] = ['classic', 'ide', 'agy', 'acp'];

export function getClientAccountId(
    target: ClientTarget,
    activeClients?: ActiveClientsStatus
): string | null | undefined {
    if (!activeClients) return undefined;
    switch (target) {
        case 'classic':
            return activeClients.classic_account_id;
        case 'ide':
            return activeClients.ide_account_id;
        case 'agy':
            return activeClients.agy_account_id;
        case 'acp':
            return activeClients.acp_account_id;
    }
}

/** Pick initial active client tab, prioritizing the first client with a bound account. */
export function resolveInitialClientTarget(activeClients?: ActiveClientsStatus): ClientTarget {
    for (const target of CLIENT_ORDER) {
        if (getClientAccountId(target, activeClients)) {
            return target;
        }
    }
    return 'classic';
}

/** Resolve the displayed Account for a client target, falling back to `fallbackAccount` for Classic. */
export function resolveActiveClientAccount(
    target: ClientTarget,
    accounts: Account[],
    activeClients?: ActiveClientsStatus,
    fallbackAccount?: Account | null
): Account | null {
    const boundId = getClientAccountId(target, activeClients);
    if (boundId && accounts.length > 0) {
        const found = accounts.find((acc) => acc.id === boundId);
        if (found) return found;
    }
    if (target === 'classic' && fallbackAccount) {
        return fallbackAccount;
    }
    return null;
}

/** Extract a clean display name from `account.name` or the email local-part. */
export function extractAccountDisplayName(
    account?: Pick<Account, 'name' | 'email'> | null
): string | null {
    if (!account) return null;
    const named = account.name?.trim();
    if (named) return named;

    const email = account.email?.trim();
    if (!email) return null;
    const localPart = email.split('@')[0]?.trim();
    return localPart || email;
}
