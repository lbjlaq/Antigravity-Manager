export interface Account {
    id: string;
    email: string;
    name?: string;
    token: TokenData;
    quota?: QuotaData;
    disabled?: boolean;
    disabled_reason?: string;
    disabled_at?: number;
    created_at: number;
    last_used: number;
}

export interface TokenData {
    access_token: string;
    refresh_token: string;
    expires_in: number;
    expiry_timestamp: number;
    token_type: string;
    email?: string;
}

export interface QuotaData {
    models: ModelQuota[];
    last_updated: number;
    is_forbidden?: boolean;
    subscription_tier?: string;  // 订阅类型: FREE/PRO/ULTRA
}

export interface ModelQuota {
    name: string;
    percentage: number;
    reset_time: string;
}
