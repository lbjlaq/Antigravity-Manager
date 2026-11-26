import type { Account, TokenData } from '../types/account';

/**
 * Strip secret OAuth credentials before persisting UI account snapshots into browser localStorage.
 * Preserves non-secret metadata (project_id, oauth_client_key, is_gcp_tos, expiry) for initial UI paint.
 */
export function sanitizeAccountForCache(account: Account): Account {
    if (!account.token || typeof account.token !== 'object') {
        return account;
    }

    const {
        id_token: _idToken,
        session_id: _sessionId,
        ...restToken
    } = account.token as TokenData & { id_token?: unknown; session_id?: unknown };

    return {
        ...account,
        token: {
            ...restToken,
            access_token: '',
            refresh_token: '',
        },
    };
}

export function sanitizeAccountsForCache(accounts: Account[]): Account[] {
    return accounts.map(sanitizeAccountForCache);
}
