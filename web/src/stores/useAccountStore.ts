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
    switchAccount: (accountId: string) => Promise<void>;
    refreshQuota: (accountId: string) => Promise<void>;
    refreshAllQuotas: () => Promise<accountService.RefreshStats>;

    // 以下功能仅在桌面端可用，Web端已移除
    // startOAuthLogin: () => Promise<void>;
    // cancelOAuthLogin: () => Promise<void>;
    // importV1Accounts: () => Promise<void>;
    // importFromDb: () => Promise<void>;
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
            await get().fetchAccounts();
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

    // 以下功能仅在桌面端可用，Web端已移除实现
    // startOAuthLogin: async () => {
    //     throw new Error('OAuth登录仅在桌面端可用');
    // },
    // cancelOAuthLogin: async () => {
    //     throw new Error('OAuth登录仅在桌面端可用');
    // },
    // importV1Accounts: async () => {
    //     throw new Error('从V1导入仅在桌面端可用');
    // },
    // importFromDb: async () => {
    //     throw new Error('从数据库导入仅在桌面端可用');
    // },
}));
