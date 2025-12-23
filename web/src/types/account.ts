export interface Account {
    id: string;
    email: string;
    name?: string;
    token: TokenData;
    quota?: QuotaData;
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
}

export interface ModelQuota {
    name: string;
    percentage: number;
    reset_time: string;
}
