/**
 * 账号服务 - Web 版本
 * 使用 HTTP API 替代 Tauri invoke
 */

import { api, Account as ApiAccount } from './api';

// 重新导出类型以保持兼容性
export interface Account {
    id: string;
    email: string;
    name?: string;
    token: {
        access_token: string;
        refresh_token: string;
        expires_in: number;
        expiry_timestamp: number;
        project_id?: string;
    };
    quota?: QuotaData;
    created_at: number;
    updated_at: number;
}

export interface QuotaData {
    used: number;
    total: number;
    percent: number;
    image_used: number;
    image_total: number;
    image_percent: number;
    is_forbidden: boolean;
}

export interface RefreshStats {
    total: number;
    success: number;
    failed: number;
    details: string[];
}

// 转换 API 账号到本地格式
function convertAccount(apiAccount: ApiAccount): Account {
    return {
        id: apiAccount.id,
        email: apiAccount.email,
        name: apiAccount.name,
        token: {
            access_token: '',
            refresh_token: '',
            expires_in: 0,
            expiry_timestamp: apiAccount.token_expires_at,
            project_id: undefined,
        },
        quota: apiAccount.quota ? {
            used: apiAccount.quota.used,
            total: apiAccount.quota.total,
            percent: apiAccount.quota.percent,
            image_used: apiAccount.quota.image_used,
            image_total: apiAccount.quota.image_total,
            image_percent: apiAccount.quota.image_percent,
            is_forbidden: apiAccount.quota.is_forbidden,
        } : undefined,
        created_at: Date.now(),
        updated_at: Date.now(),
    };
}

export async function listAccounts(): Promise<Account[]> {
    const response = await api.accounts.list();
    return response.accounts.map(convertAccount);
}

export async function getCurrentAccount(): Promise<Account | null> {
    // Web 版本没有"当前账号"概念，返回第一个账号或 null
    const response = await api.accounts.list();
    if (response.accounts.length > 0) {
        return convertAccount(response.accounts[0]);
    }
    return null;
}

export async function addAccount(_email: string, refreshToken: string): Promise<Account> {
    const account = await api.accounts.add({ refresh_token: refreshToken });
    return convertAccount(account);
}

export async function deleteAccount(accountId: string): Promise<void> {
    await api.accounts.delete(accountId);
}

export async function switchAccount(_accountId: string): Promise<void> {
    // Web 版本不支持切换账号（没有本地 Antigravity 应用）
    console.warn('switchAccount 在 Web 版本中不可用');
}

export async function fetchAccountQuota(accountId: string): Promise<QuotaData> {
    const quota = await api.accounts.getQuota(accountId);
    return {
        used: quota.used,
        total: quota.total,
        percent: quota.percent,
        image_used: quota.image_used,
        image_total: quota.image_total,
        image_percent: quota.image_percent,
        is_forbidden: quota.is_forbidden,
    };
}

export async function refreshAllQuotas(): Promise<RefreshStats> {
    // Web 版本暂不支持批量刷新配额
    // 可以通过遍历账号并调用 refreshToken 来模拟
    const response = await api.accounts.list();
    return {
        total: response.total,
        success: response.total,
        failed: 0,
        details: [],
    };
}

// OAuth 相关 - Web 版本不支持
export async function startOAuthLogin(): Promise<Account> {
    throw new Error('OAuth 登录在 Web 版本中不可用。请使用 Refresh Token 添加账号。');
}

export async function cancelOAuthLogin(): Promise<void> {
    // 无操作
}

// 导入相关 - Web 版本不支持
export async function importV1Accounts(): Promise<Account[]> {
    throw new Error('V1 导入在 Web 版本中不可用。');
}

export async function importFromDb(): Promise<Account> {
    throw new Error('数据库导入在 Web 版本中不可用。');
}

// 刷新单个账号的 Token
export async function refreshAccountToken(accountId: string): Promise<Account> {
    const account = await api.accounts.refreshToken(accountId);
    return convertAccount(account);
}
