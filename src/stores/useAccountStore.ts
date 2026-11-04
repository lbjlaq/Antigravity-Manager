import { create } from 'zustand';
import { Account, ActiveClientsStatus } from '../types/account';
import * as accountService from '../services/accountService';

interface AccountState {
    accounts: Account[];
    currentAccount: Account | null;
    activeClients: ActiveClientsStatus;
    loading: boolean;
    error: string | null;

    // Actions
    fetchAccounts: () => Promise<void>;
    fetchCurrentAccount: () => Promise<void>;
    fetchActiveClients: () => Promise<void>;
    addAccount: (email: string, refreshToken: string) => Promise<void>;
    deleteAccount: (accountId: string) => Promise<void>;
    deleteAccounts: (accountIds: string[]) => Promise<void>;
    switchAccount: (accountId: string, targetIde?: string) => Promise<void>;
    refreshQuota: (accountId: string) => Promise<void>;
    refreshAllQuotas: () => Promise<accountService.RefreshStats>;
    reorderAccounts: (accountIds: string[]) => Promise<void>;

    // 新增 actions
    startOAuthLogin: (oauthClientKey?: string) => Promise<void>;
    completeOAuthLogin: () => Promise<void>;
    cancelOAuthLogin: () => Promise<void>;
    importV1Accounts: () => Promise<void>;
    batchImportAccounts: (entries: accountService.ImportAccountEntry[]) => Promise<accountService.BatchImportResult>;
    importFromDb: () => Promise<void>;
    importFromCustomDb: (path: string) => Promise<void>;
    syncAccountFromDb: () => Promise<void>;
    toggleProxyStatus: (accountId: string, enable: boolean, reason?: string) => Promise<void>;
    warmUpAccounts: () => Promise<string>;
    warmUpAccount: (accountId: string) => Promise<string>;
    updateAccountLabel: (accountId: string, label: string) => Promise<void>;
    updateAccountPriority: (accountId: string, priority: number) => Promise<void>;
}

const ACCOUNTS_CACHE_KEY = 'antigravity_accounts_cache';
const CURRENT_ACCOUNT_CACHE_KEY = 'antigravity_current_account_cache';
const ACTIVE_CLIENTS_CACHE_KEY = 'antigravity_active_clients_cache';

const getInitialAccounts = (): Account[] => {
    try {
        const saved = localStorage.getItem(ACCOUNTS_CACHE_KEY);
        if (saved) {
            const parsed = JSON.parse(saved);
            if (Array.isArray(parsed) && parsed.length > 0) {
                return parsed;
            }
        }
    } catch (_) {}
    return [];
};

const getInitialCurrentAccount = (): Account | null => {
    try {
        const saved = localStorage.getItem(CURRENT_ACCOUNT_CACHE_KEY);
        if (saved) {
            const parsed = JSON.parse(saved);
            if (parsed && typeof parsed === 'object') {
                return parsed;
            }
        }
    } catch (_) {}
    return null;
};

const getInitialActiveClients = (): ActiveClientsStatus => {
    try {
        const saved = localStorage.getItem(ACTIVE_CLIENTS_CACHE_KEY);
        if (saved) {
            const parsed = JSON.parse(saved);
            if (parsed && typeof parsed === 'object') {
                return parsed;
            }
        }
    } catch (_) {}
    return {};
};

export const useAccountStore = create<AccountState>((set, get) => ({
    accounts: getInitialAccounts(),
    currentAccount: getInitialCurrentAccount(),
    activeClients: getInitialActiveClients(),
    loading: false,
    error: null,

    fetchAccounts: async () => {
        set({ loading: true, error: null });
        try {
            console.log('[Store] Fetching accounts...');
            const [accounts, activeClients] = await Promise.all([
                accountService.listAccounts(),
                accountService.getActiveClientsStatus().catch(() => ({} as ActiveClientsStatus))
            ]);
            try {
                localStorage.setItem(ACCOUNTS_CACHE_KEY, JSON.stringify(accounts));
                localStorage.setItem(ACTIVE_CLIENTS_CACHE_KEY, JSON.stringify(activeClients));
            } catch (_) {}
            set({ accounts, activeClients, loading: false });
        } catch (error) {
            console.error('[Store] Fetch accounts failed:', error);
            set({ error: String(error), loading: false });
        }
    },

    fetchCurrentAccount: async () => {
        set({ loading: true, error: null });
        try {
            const [account, activeClients] = await Promise.all([
                accountService.getCurrentAccount(),
                accountService.getActiveClientsStatus().catch(() => ({} as ActiveClientsStatus))
            ]);
            try {
                if (account) {
                    localStorage.setItem(CURRENT_ACCOUNT_CACHE_KEY, JSON.stringify(account));
                } else {
                    localStorage.removeItem(CURRENT_ACCOUNT_CACHE_KEY);
                }
                localStorage.setItem(ACTIVE_CLIENTS_CACHE_KEY, JSON.stringify(activeClients));
            } catch (_) {}
            set({ currentAccount: account, activeClients, loading: false });
        } catch (error) {
            set({ error: String(error), loading: false });
        }
    },

    fetchActiveClients: async () => {
        try {
            const activeClients = await accountService.getActiveClientsStatus();
            set({ activeClients });
        } catch (error) {
            console.error('[Store] Fetch active clients failed:', error);
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

    switchAccount: async (accountId: string, targetIde?: string) => {
        set({ loading: true, error: null });
        try {
            await accountService.switchAccount(accountId, targetIde);
            await Promise.all([
                get().fetchCurrentAccount(),
                get().fetchActiveClients(),
            ]);
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

    /**
     * 重新排序账号列表
     * 采用乐观更新策略：先更新本地状态再调用后端持久化，以提供流畅的拖拽体验
     */
    reorderAccounts: async (accountIds: string[]) => {
        const { accounts } = get();

        // 创建 ID 到账号的映射
        const accountMap = new Map(accounts.map(acc => [acc.id, acc]));

        // 按新顺序重建账号数组
        const reorderedAccounts = accountIds
            .map(id => accountMap.get(id))
            .filter((acc): acc is Account => acc !== undefined);

        // 添加未在新顺序中的账号（保持原有顺序）
        const remainingAccounts = accounts.filter(acc => !accountIds.includes(acc.id));
        const finalAccounts = [...reorderedAccounts, ...remainingAccounts];

        // 乐观更新本地状态
        set({ accounts: finalAccounts });

        try {
            await accountService.reorderAccounts(accountIds);
        } catch (error) {
            // 后端失败时回滚到原始顺序
            console.error('[AccountStore] Reorder accounts failed:', error);
            set({ accounts });
            throw error;
        }
    },

    startOAuthLogin: async (oauthClientKey?: string) => {
        set({ loading: true, error: null });
        try {
            await accountService.startOAuthLogin(oauthClientKey);
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

    batchImportAccounts: async (entries: accountService.ImportAccountEntry[]) => {
        set({ loading: true, error: null });
        try {
            const res = await accountService.batchImportAccounts(entries);
            await get().fetchAccounts();
            set({ loading: false });
            return res;
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

    toggleProxyStatus: async (accountId: string, enable: boolean, reason?: string) => {
        try {
            await accountService.toggleProxyStatus(accountId, enable, reason);
            await get().fetchAccounts();
        } catch (error) {
            console.error('[AccountStore] Toggle proxy status failed:', error);
            throw error;
        }
    },

    warmUpAccounts: async () => {
        set({ loading: true, error: null });
        try {
            const result = await accountService.warmUpAllAccounts();
            set({ loading: false });
            return result;
        } catch (error) {
            set({ error: String(error), loading: false });
            throw error;
        } finally {
            await get().fetchAccounts();
        }
    },

    warmUpAccount: async (accountId: string) => {
        set({ loading: true, error: null });
        try {
            const result = await accountService.warmUpAccount(accountId);
            set({ loading: false });
            return result;
        } catch (error) {
            set({ error: String(error), loading: false });
            throw error;
        } finally {
            await get().fetchAccounts();
        }
    },

    updateAccountPriority: async (accountId, priority) => {
        await accountService.updateAccountPriority(accountId, priority);
        set(state => ({
            accounts: state.accounts.map(account => account.id === accountId ? { ...account, priority } : account),
            currentAccount: state.currentAccount?.id === accountId ? { ...state.currentAccount, priority } : state.currentAccount,
        }));
    },

    updateAccountLabel: async (accountId: string, label: string) => {
        try {
            await accountService.updateAccountLabel(accountId, label);
            // 乐观更新本地状态
            const { accounts } = get();
            const updatedAccounts = accounts.map(acc =>
                acc.id === accountId ? { ...acc, custom_label: label || undefined } : acc
            );
            set({ accounts: updatedAccounts });
        } catch (error) {
            console.error('[AccountStore] Update label failed:', error);
            throw error;
        }
    },
}));
