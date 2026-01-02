import { create } from 'zustand';
import { Account } from '../types/account';
import * as accountService from '../services/accountService';

interface AccountState {
    accounts: Account[];
    currentAccount: Account | null;
    loading: boolean;
    error: string | null;

    // Actions
    fetchAccounts: () => Promise<void>;
    fetchCurrentAccount: () => Promise<void>;
    addAccount: (email: string, refreshToken: string) => Promise<void>;
    deleteAccount: (accountId: string) => Promise<void>;
    deleteAccounts: (accountIds: string[]) => Promise<void>;
    switchAccount: (accountId: string) => Promise<void>;
    refreshQuota: (accountId: string) => Promise<void>;
    refreshAllQuotas: () => Promise<accountService.RefreshStats>;

    // 新增 actions
    startOAuthLogin: () => Promise<void>;
    completeOAuthLogin: () => Promise<void>;
    cancelOAuthLogin: () => Promise<void>;
    importV1Accounts: () => Promise<void>;
    importFromDb: () => Promise<void>;
    importFromCustomDb: (path: string) => Promise<void>;
    syncAccountFromDb: () => Promise<void>;
    warmUpAccounts: () => Promise<string>;
}

export const useAccountStore = create<AccountState>((set, get) => ({
    accounts: [],
    currentAccount: null,
    loading: false,
    error: null,

    fetchAccounts: async () => {
        set({ loading: true, error: null });
        try {
            console.log('[Store] Fetching accounts...');
            const accounts = await accountService.listAccounts();
            set({ accounts, loading: false });
        } catch (error) {
            console.error('[Store] Fetch accounts failed:', error);
            set({ error: String(error), loading: false });
        }
    },

    fetchCurrentAccount: async () => {
        set({ loading: true, error: null });
        try {
            const account = await accountService.getCurrentAccount();
            set({ currentAccount: account, loading: false });
        } catch (error) {
            set({ error: String(error), loading: false });
        }
    },

    addAccount: async (email: string, refreshToken: string) => {
        set({ loading: true, error: null });
        try {
            await accountService.addAccount(email, refreshToken);
            await get().fetchAccounts();
            set({ loading: false });
        } catch (error) {
            set({ error: String(error), loading: false });
            throw error;
        }
    },

    deleteAccount: async (accountId: string) => {
        set({ loading: true, error: null });
        try {
            await accountService.deleteAccount(accountId);
            await Promise.all([
                get().fetchAccounts(),
                get().fetchCurrentAccount()
            ]);
            set({ loading: false });
        } catch (error) {
            set({ error: String(error), loading: false });
            throw error;
        }
    },

    deleteAccounts: async (accountIds: string[]) => {
        set({ loading: true, error: null });
        try {
            await accountService.deleteAccounts(accountIds);
            await Promise.all([
                get().fetchAccounts(),
                get().fetchCurrentAccount()
            ]);
            set({ loading: false });
        } catch (error) {
            set({ error: String(error), loading: false });
            throw error;
        }
    },

    switchAccount: async (accountId: string) => {
        set({ loading: true, error: null });
        try {
            await accountService.switchAccount(accountId);
            await get().fetchCurrentAccount();
            set({ loading: false });
        } catch (error) {
            set({ error: String(error), loading: false });
            throw error;
        }
    },

    refreshQuota: async (accountId: string) => {
        set({ loading: true, error: null });
        try {
            await accountService.fetchAccountQuota(accountId);
            await get().fetchAccounts();
            set({ loading: false });
        } catch (error) {
            set({ error: String(error), loading: false });
            throw error;
        }
    },

    refreshAllQuotas: async () => {
        set({ loading: true, error: null });
        try {
            const stats = await accountService.refreshAllQuotas();
            await get().fetchAccounts();
            set({ loading: false });
            return stats;
        } catch (error) {
            set({ error: String(error), loading: false });
            throw error;
        }
    },

    startOAuthLogin: async () => {
        set({ loading: true, error: null });
        try {
            await accountService.startOAuthLogin();
            await get().fetchAccounts();
            set({ loading: false });
        } catch (error) {
            set({ error: String(error), loading: false });
            throw error;
        }
    },

    completeOAuthLogin: async () => {
        set({ loading: true, error: null });
        try {
            await accountService.completeOAuthLogin();
            await get().fetchAccounts();
            set({ loading: false });
        } catch (error) {
            set({ error: String(error), loading: false });
            throw error;
        }
    },

    cancelOAuthLogin: async () => {
        try {
            await accountService.cancelOAuthLogin();
            set({ loading: false, error: null });
        } catch (error) {
            console.error('[Store] Cancel OAuth failed:', error);
        }
    },

    importV1Accounts: async () => {
        set({ loading: true, error: null });
        try {
            await accountService.importV1Accounts();
            await get().fetchAccounts();
            set({ loading: false });
        } catch (error) {
            set({ error: String(error), loading: false });
            throw error;
        }
    },

    importFromDb: async () => {
        set({ loading: true, error: null });
        try {
            await accountService.importFromDb();
            await Promise.all([
                get().fetchAccounts(),
                get().fetchCurrentAccount()
            ]);
            set({ loading: false });
        } catch (error) {
            set({ error: String(error), loading: false });
            throw error;
        }
    },

    importFromCustomDb: async (path: string) => {
        set({ loading: true, error: null });
        try {
            await accountService.importFromCustomDb(path);
            await Promise.all([
                get().fetchAccounts(),
                get().fetchCurrentAccount()
            ]);
            set({ loading: false });
        } catch (error) {
            set({ error: String(error), loading: false });
            throw error;
        }
    },

    syncAccountFromDb: async () => {
        try {
            const syncedAccount = await accountService.syncAccountFromDb();
            if (syncedAccount) {
                console.log('[AccountStore] Account synced from DB:', syncedAccount.email);
                await get().fetchAccounts();
                set({ currentAccount: syncedAccount });
            }
        } catch (error) {
            console.error('[AccountStore] Sync from DB failed:', error);
        }
    },

    warmUpAccounts: async () => {
        try {
            const result = await accountService.warmUpAccounts();
            console.log('[AccountStore] Warm-up started:', result);
            return result;
        } catch (error) {
            console.error('[AccountStore] Warm-up failed:', error);
            throw error;
        }
    },
}));
