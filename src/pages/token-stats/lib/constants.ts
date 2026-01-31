// File: src/pages/token-stats/lib/constants.ts
// Types and constants for Token Stats page

export interface TokenStatsAggregated {
    period: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_tokens: number;
    request_count: number;
}

export interface AccountTokenStats {
    account_email: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_tokens: number;
    request_count: number;
}

export interface ModelTokenStats {
    model: string;
    total_input_tokens: number;
    total_output_tokens: number;
    total_tokens: number;
    request_count: number;
}

export interface ModelTrendPoint {
    period: string;
    model_data: Record<string, number>;
}

export interface AccountTrendPoint {
    period: string;
    account_data: Record<string, number>;
}

export interface TokenStatsSummary {
    total_input_tokens: number;
    total_output_tokens: number;
    total_tokens: number;
    total_requests: number;
    unique_accounts: number;
}

export type TimeRange = 'hourly' | 'daily' | 'weekly';
export type ViewMode = 'model' | 'account';

export const CHART_COLORS = ['#3b82f6', '#8b5cf6', '#ec4899', '#f59e0b', '#10b981', '#06b6d4', '#6366f1', '#f43f5e'];

export const TIME_RANGES: TimeRange[] = ['hourly', 'daily', 'weekly'];
