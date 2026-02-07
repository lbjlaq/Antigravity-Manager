// File: src/pages/api-proxy/lib/constants.ts
// Types and constants for API Proxy page

export interface ProxyStatus {
    running: boolean;
    port: number;
    base_url: string;
    active_accounts: number;
}

export interface CloudflaredStatus {
    installed: boolean;
    version?: string;
    running: boolean;
    url?: string;
    error?: string;
}

export type ProtocolType = 'openai' | 'anthropic' | 'gemini';
export type CloudflaredMode = 'quick' | 'auth';

export const DEFAULT_PROXY_PORT = 8045;
export const DEFAULT_REQUEST_TIMEOUT = 120;

export const MODEL_PRESETS: Record<string, string> = {
    // OpenAI (wildcards)
    "gpt-4*": "gemini-3-pro-high",
    "gpt-4o*": "gemini-3-flash",
    "gpt-3.5*": "gemini-2.5-flash",
    "o1-*": "gemini-3-pro-high",
    "o3-*": "gemini-3-pro-high",
    // Claude (wildcards)
    "claude-3-5-sonnet-*": "claude-sonnet-4-5",
    "claude-3-opus-*": "claude-opus-4-6-thinking",
    "claude-opus-4-6-*": "claude-opus-4-6-thinking",
    "claude-opus-4-*": "claude-opus-4-5-thinking",
    "claude-haiku-*": "gemini-2.5-flash",
    "claude-3-haiku-*": "gemini-2.5-flash",
};
