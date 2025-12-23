/**
 * HTTP API 客户端
 * 替代 Tauri invoke 调用
 */

// 从 localStorage 获取 API Key
const getApiKey = (): string => {
    return localStorage.getItem('antigravity_api_key') || 'sk-antigravity';
};

// 基础请求配置
const getHeaders = (): HeadersInit => ({
    'Content-Type': 'application/json',
    'Authorization': `Bearer ${getApiKey()}`,
});

// 基础 API URL (开发时代理到 localhost:8045)
const API_BASE = '';

// 通用请求函数
async function request<T>(
    method: string,
    path: string,
    body?: unknown
): Promise<T> {
    const response = await fetch(`${API_BASE}${path}`, {
        method,
        headers: getHeaders(),
        body: body ? JSON.stringify(body) : undefined,
    });

    if (!response.ok) {
        const error = await response.json().catch(() => ({ message: response.statusText }));
        throw new Error(error.error?.message || error.message || 'API 请求失败');
    }

    return response.json();
}

// ============ 账号 API ============

export interface Account {
    id: string;
    email: string;
    name?: string;
    quota?: QuotaData;
    token_expires_at: number;
    is_token_valid: boolean;
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

export interface AddAccountRequest {
    refresh_token: string;
    email?: string;
    name?: string;
}

export const accountsApi = {
    // 列出所有账号
    list: async (): Promise<{ accounts: Account[]; total: number }> => {
        return request('GET', '/api/accounts');
    },

    // 获取单个账号
    get: async (id: string): Promise<Account> => {
        return request('GET', `/api/accounts/${id}`);
    },

    // 添加账号
    add: async (data: AddAccountRequest): Promise<Account> => {
        return request('POST', '/api/accounts', data);
    },

    // 批量添加账号
    batchAdd: async (refreshTokens: string[]): Promise<{
        success: number;
        failed: number;
        accounts: Account[];
        errors: string[];
    }> => {
        return request('POST', '/api/accounts/batch', { refresh_tokens: refreshTokens });
    },

    // 删除账号
    delete: async (id: string): Promise<void> => {
        return request('DELETE', `/api/accounts/${id}`);
    },

    // 刷新账号 Token
    refreshToken: async (id: string): Promise<Account> => {
        return request('POST', `/api/accounts/${id}/refresh`);
    },

    // 获取账号配额
    getQuota: async (id: string): Promise<QuotaData> => {
        return request('GET', `/api/accounts/${id}/quota`);
    },

    // 重新加载账号
    reload: async (): Promise<{ success: boolean; accounts_loaded: number }> => {
        return request('POST', '/api/accounts/reload');
    },
};

// ============ 配置 API ============

export interface AppConfig {
    server: {
        port: number;
        host: string;
    };
    proxy: {
        request_timeout: number;
        model_mapping: Record<string, string>;
    };
}

export const configApi = {
    // 获取配置
    get: async (): Promise<AppConfig> => {
        return request('GET', '/api/config');
    },

    // 更新配置
    update: async (updates: Partial<AppConfig['proxy']>): Promise<{ success: boolean }> => {
        return request('POST', '/api/config', updates);
    },
};

// ============ 统计 API ============

export interface Stats {
    accounts: {
        total: number;
        active: number;
    };
    server: {
        version: string;
        uptime: string;
    };
}

export const statsApi = {
    // 获取统计信息
    get: async (): Promise<Stats> => {
        return request('GET', '/api/stats');
    },
};

// ============ 健康检查 ============

export const healthApi = {
    check: async (): Promise<{ status: string; version: string }> => {
        return request('GET', '/health');
    },
};

// 导出统一的 API 对象
export const api = {
    accounts: accountsApi,
    config: configApi,
    stats: statsApi,
    health: healthApi,
};

export default api;
