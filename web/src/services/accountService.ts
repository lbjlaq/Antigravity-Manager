import { invoke } from '../utils/invoke';
import { Account, QuotaData } from '../types/account';

// Web版本不需要检查Tauri环境

export async function listAccounts(): Promise<Account[]> {
    return await invoke('list_accounts');
}

export async function getCurrentAccount(): Promise<Account | null> {
    return await invoke('get_current_account');
}

export async function addAccount(email: string, refreshToken: string): Promise<Account> {
    return await invoke('add_account', { email, refreshToken });
}

export async function deleteAccount(accountId: string): Promise<void> {
    return await invoke('delete_account', { accountId });
}

export async function switchAccount(accountId: string): Promise<void> {
    return await invoke('switch_account', { accountId });
}

export async function fetchAccountQuota(accountId: string): Promise<QuotaData> {
    return await invoke('fetch_account_quota', { accountId });
}

export interface RefreshStats {
    total: number;
    success: number;
    failed: number;
    details: string[];
}

export async function refreshAllQuotas(): Promise<RefreshStats> {
    return await invoke('refresh_all_quotas');
}

// 以下功能仅在桌面端可用，Web端不支持
// OAuth登录需要本地浏览器和文件系统访问，Web端仅支持通过Refresh Token添加账号
// export async function startOAuthLogin(): Promise<Account> {
//     throw new Error('Web版本不支持OAuth登录。请使用Refresh Token添加账号。');
// }

// export async function cancelOAuthLogin(): Promise<void> {
//     throw new Error('Web版本不支持OAuth登录。');
// }

// 导入功能需要访问本地文件系统，Web端不支持
// export async function importV1Accounts(): Promise<Account[]> {
//     throw new Error('Web版本不支持从V1导入。请使用Refresh Token添加账号。');
// }

// export async function importFromDb(): Promise<Account> {
//     throw new Error('Web版本不支持从数据库导入。请使用Refresh Token添加账号。');
// }
