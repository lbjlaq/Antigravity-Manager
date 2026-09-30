import React, { useState, useEffect, useCallback, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import {
    ExternalLink,
    Eye,
    EyeOff,
    Trash2,
    Copy,
    Pencil,
    X,
    RefreshCw,
    Activity,
    Settings,
    Layers,
    HelpCircle,
    DollarSign,
    Coins,
    Cpu,
    Flame,
    Hash,
    TrendingUp,
    Zap,
    Code,
    CodeXml,
    Wand2,
    CheckSquare,
    MinusSquare,
    Square
} from 'lucide-react';
import { motion } from 'framer-motion';
import { showToast } from '../components/common/ToastContainer';
import { copyToClipboard } from '../utils/clipboard';
import { request } from '../utils/request';
import { generateUUID } from '../utils/uuid';
import { getProfileInfo, type OpencodeProviderSummary } from '../utils/opencodeProfiles';

interface ManagedApiKey {
    id: string;
    key: string;
    name: string;
    baseUrl: string;
    createdAt: number;
    lastUsedAt: number;
    lastStatus?: 'ok' | 'bad' | 'unknown';
    lastRemaining?: string;
    models?: string[];
}

interface UsageSummary {
    remaining: string;
    used: string;
    todayRequests: string;
    todayTokens: string;
    totalRequests: string;
    totalTokens: string;
    unit: string;
    isValid: boolean;
}

const STORAGE_KEY = 'apikey_fun_managed_keys_local';
const DEFAULT_ENDPOINT = 'https://api.apikey.fan/v1';

function maskKey(value: string): string {
    const trimmed = value.trim();
    if (!trimmed) return '';
    if (trimmed.length <= 10) return `${trimmed.slice(0, 3)}••••${trimmed.slice(-3)}`;
    return `${trimmed.slice(0, 6)}••••${trimmed.slice(-4)}`;
}

function formatDate(ts: number | undefined): string {
    if (!ts) return '--';
    const d = new Date(ts);
    return `${d.getFullYear()}/${String(d.getMonth()+1).padStart(2, '0')}/${String(d.getDate()).padStart(2, '0')} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

export const ApiKeyFun: React.FC = () => {
    const { t } = useTranslation();
    
    const [apiKey, setApiKey] = useState('');
    const [baseUrl, setBaseUrl] = useState(DEFAULT_ENDPOINT);
    const [showApiKey, setShowApiKey] = useState(false);
    
    // Querying states
    const [querying, setQuerying] = useState(false);
    const [usage, setUsage] = useState<UsageSummary | null>(null);
    const [models, setModels] = useState<string[]>([]);
    const [modelsSource, setModelsSource] = useState<{ key: string; endpoint: string } | null>(null);
    const [queryError, setQueryError] = useState<string | null>(null);
    const [modelsError, setModelsError] = useState<string | null>(null);
    const [opencodeProviders, setOpencodeProviders] = useState<OpencodeProviderSummary[]>([]);
    const [syncingKey, setSyncingKey] = useState<string | null>(null);
    const isTogglingRef = useRef(false);
    const querySeqRef = useRef(0);

    const providersSeqRef = useRef(0);
    const fetchOpencodeProviders = useCallback(async () => {
        const seq = ++providersSeqRef.current;
        const providers = await request<OpencodeProviderSummary[]>('get_opencode_providers');
        if (!Array.isArray(providers)) throw new Error('Invalid OpenCode providers response');
        if (seq === providersSeqRef.current) setOpencodeProviders(providers);
        return providers;
    }, []);

    useEffect(() => {
        fetchOpencodeProviders().catch(err => console.warn('Failed to fetch opencode providers', err));
    }, [fetchOpencodeProviders]);
    
    // Key Management
    const [managedKeys, setManagedKeys] = useState<ManagedApiKey[]>(() => {
        try {
            const raw = localStorage.getItem(STORAGE_KEY);
            return raw ? JSON.parse(raw) : [];
        } catch {
            return [];
        }
    });
    
    // Inline Rename state
    const [editingId, setEditingId] = useState<string | null>(null);
    const [editNameValue, setEditNameValue] = useState('');
    const initialKeyLoaded = useRef(false);

    // Save keys to localStorage
    useEffect(() => {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(managedKeys));
    }, [managedKeys]);

    const handleCopy = async (text: string) => {
        const success = await copyToClipboard(text);
        if (success) {
            showToast(t('common.copied') || 'Copied to clipboard', 'success');
        }
    };

    // Auto balance & models query
    const runQuery = useCallback(async (keyToQuery: string, urlToQuery: string) => {
        const key = keyToQuery.trim();
        const endpoint = urlToQuery.trim().replace(/\/+$/, '');
        const seq = ++querySeqRef.current;
        if (!key) {
            setQuerying(false);
            return;
        }

        setQuerying(true);
        setQueryError(null);
        setUsage(null);
        setModels([]);
        setModelsSource(null);

        let fetchedModels: string[] = [];
        try {
            // 1. Fetch available models
            let rawText = '';
            setModelsError(null);
            try {
                rawText = await request<string>('query_transit_info', {
                    url: `${endpoint}/models`,
                    key
                });
                if (seq !== querySeqRef.current) return;
                const modelsData = JSON.parse(rawText);
                if (modelsData && Array.isArray(modelsData.data)) {
                    fetchedModels = modelsData.data.map((m: any) => typeof m === 'string' ? m : m?.id).filter((id: unknown): id is string => typeof id === 'string' && Boolean(id.trim())).map((id: string) => id.trim());
                } else if (Array.isArray(modelsData)) {
                    fetchedModels = modelsData.map((m: any) => typeof m === 'string' ? m : m?.id).filter((id: unknown): id is string => typeof id === 'string' && Boolean(id.trim())).map((id: string) => id.trim());
                } else {
                    setModelsError(t('apiKeyFun.errors.parseFormat', { defaultValue: '解析格式异常: {{err}}', err: Object.keys(modelsData).join(',') }));
                }
            } catch (err: any) {
                if (seq !== querySeqRef.current) return;
                console.warn('Failed to fetch models list', err);
                setModelsError(t('apiKeyFun.errors.fetchFailed', { defaultValue: '获取失败: {{err}}', err: err.message || String(err) }));
            }
            if (seq !== querySeqRef.current) return;
            setModels(fetchedModels);
            setModelsSource({ key, endpoint });

            // 2. Fetch balance (Try sub2api /usage first, then New API billing)
            let usageSummary: UsageSummary | null = null;
            
            try {
                const usageText = await request<string>('query_transit_info', {
                    url: `${endpoint}/usage`,
                    key
                });
                const data = JSON.parse(usageText);
                
                const unit = data.unit || data.quota?.unit || 'USD';
                const remaining = typeof data.remaining === 'number' ? data.remaining.toFixed(2) : 
                                  typeof data.balance === 'number' ? data.balance.toFixed(2) : '--';
                const usedRaw = data.quota?.used ?? data.usage?.total?.actual_cost ?? data.usage?.total?.cost;
                const used = typeof usedRaw === 'number' ? usedRaw.toFixed(2) : '--';
                
                usageSummary = {
                    remaining: remaining !== '--' ? (unit === 'USD' ? `$${remaining}` : `${remaining} ${unit}`) : '--',
                    used: used !== '--' ? (unit === 'USD' ? `$${used}` : `${used} ${unit}`) : '--',
                    todayRequests: String(data.usage?.today?.requests ?? '--'),
                    todayTokens: String(data.usage?.today?.total_tokens ?? '--'),
                    totalRequests: String(data.usage?.total?.requests ?? '--'),
                    totalTokens: String(data.usage?.total?.total_tokens ?? '--'),
                    unit,
                    isValid: data.is_active ?? data.isValid ?? true
                };
            } catch (e) {
                console.log('Skipping sub2api endpoint, trying standard billing endpoints...', e);
            }

            // Fallback to standard One-API / New-API dashboard billing
            if (!usageSummary) {
                try {
                    const subText = await request<string>('query_transit_info', {
                        url: `${endpoint}/dashboard/billing/subscription`,
                        key
                    });
                    const subData = JSON.parse(subText);
                    
                    const start = new Date(Date.now() - 100 * 24 * 3600 * 1000).toISOString().split('T')[0];
                    const end = new Date(Date.now() + 24 * 3600 * 1000).toISOString().split('T')[0];
                    const usageText = await request<string>('query_transit_info', {
                        url: `${endpoint}/dashboard/billing/usage?start_date=${start}&end_date=${end}`,
                        key
                    });
                    const usageData = JSON.parse(usageText);
                    
                    const totalUsageUSD = (usageData.total_usage ?? 0) / 100;
                    const limitUSD = subData.hard_limit_usd ?? 0;
                    const remainingUSD = (limitUSD - totalUsageUSD).toFixed(4);
                    
                    usageSummary = {
                        remaining: `$${remainingUSD}`,
                        used: `$${totalUsageUSD.toFixed(4)}`,
                        todayRequests: '--',
                        todayTokens: '--',
                        totalRequests: '--',
                        totalTokens: '--',
                        unit: 'USD',
                        isValid: true
                    };
                } catch (billingErr) {
                    console.log('Billing query failed', billingErr);
                }
            }

            if (seq !== querySeqRef.current) return;

            if (usageSummary) {
                setUsage(usageSummary);
                // Update or add to managed keys automatically
                setManagedKeys(prev => {
                    const existingIndex = prev.findIndex(item => item.key === key);
                    const now = Date.now();
                    if (existingIndex >= 0) {
                        const updated = [...prev];
                        updated[existingIndex] = {
                            ...updated[existingIndex],
                            lastRemaining: usageSummary?.remaining,
                            lastStatus: 'ok',
                            lastUsedAt: now,
                            baseUrl: endpoint, // optionally update baseUrl
                            models: fetchedModels.length > 0 ? fetchedModels
                                : updated[existingIndex].baseUrl === endpoint ? updated[existingIndex].models : undefined
                        };
                        return updated;
                    } else {
                        // Automatically save new key
                        return [{
                            id: generateUUID(),
                            key,
                            name: maskKey(key),
                            baseUrl: endpoint,
                            createdAt: now,
                            lastUsedAt: now,
                            lastStatus: 'ok',
                            lastRemaining: usageSummary?.remaining,
                            models: fetchedModels
                        }, ...prev];
                    }
                });
            } else {
                throw new Error(t('apiKeyFun.errors.queryFailed', { defaultValue: '无法获取有效的额度数据或模型列表，请确认 API Key 是否有效，以及接口地址是否正确。' }));
            }

        } catch (error: any) {
            if (seq !== querySeqRef.current) return;
            console.error('Balance query failed', error);
            setQueryError(error?.message || 'Query failed. Please verify network or key validity.');
            setManagedKeys(prev => {
                const existingIndex = prev.findIndex(item => item.key === key);
                const now = Date.now();
                if (existingIndex >= 0) {
                    const updated = [...prev];
                    updated[existingIndex] = {
                        ...updated[existingIndex],
                        lastStatus: 'bad',
                        lastUsedAt: now,
                        baseUrl: endpoint,
                        models: fetchedModels.length > 0 ? fetchedModels
                            : updated[existingIndex].baseUrl === endpoint ? updated[existingIndex].models : undefined
                    };
                    return updated;
                } else {
                    return [{
                        id: generateUUID(),
                        key,
                        name: maskKey(key),
                        baseUrl: endpoint,
                        createdAt: now,
                        lastUsedAt: now,
                        lastStatus: 'bad',
                        models: fetchedModels
                    }, ...prev];
                }
            });
        } finally {
            if (seq === querySeqRef.current) {
                setQuerying(false);
            }
        }
    }, [t]);

    // Load first key on mount and automatically run query
    useEffect(() => {
        if (!initialKeyLoaded.current) {
            initialKeyLoaded.current = true;
            if (managedKeys.length > 0) {
                const initialKey = managedKeys[0].key;
                const initialUrl = managedKeys[0].baseUrl || DEFAULT_ENDPOINT;
                setApiKey(initialKey);
                setBaseUrl(initialUrl);
                if (managedKeys[0].models && managedKeys[0].models.length > 0) {
                    setModels(managedKeys[0].models);
                    setModelsSource({ key: initialKey.trim(), endpoint: initialUrl.trim().replace(/\/+$/, '') });
                }
                runQuery(initialKey, initialUrl);
            }
        }
    }, [managedKeys, runQuery]);

    const handleSyncCli = async (app: 'Codex' | 'Claude' | 'Gemini') => {
        if (!apiKey.trim()) return;
        const rawKey = apiKey.trim();
        const cleanUrl = baseUrl.trim().replace(/\/+$/, '');
        const baseWithoutV1 = cleanUrl.replace(/\/v1$/i, '');
        const proxyUrl = app === 'Codex' ? `${baseWithoutV1}/v1` : baseWithoutV1;
        const syncKey = rawKey;

        try {
            await request('execute_cli_sync', { 
                appType: app, 
                proxyUrl: proxyUrl, 
                apiKey: syncKey
            });
            showToast(t('apiKeyFun.syncSuccess', { defaultValue: 'Successfully synced to {{app}}', app }), 'success');
        } catch (error: any) {
            showToast(t('apiKeyFun.syncError', { defaultValue: 'Failed to sync: {{error}}', error: error.toString() }), 'error');
        }
    };

    const getModelsForKey = useCallback((targetKey: string, targetUrl?: string): string[] | undefined => {
        const trimmed = targetKey.trim();
        if (!trimmed) return undefined;
        const normTarget = targetUrl ? targetUrl.trim().replace(/\/+$/, '') : null;
        if (modelsSource?.key === trimmed && (!normTarget || modelsSource.endpoint === normTarget) && models.length > 0) {
            return models.map(m => m.trim()).filter(Boolean);
        }
        const found = managedKeys.find(m => m.key.trim() === trimmed);
        if (found?.models && found.models.length > 0) {
            if (!normTarget || !found.baseUrl || found.baseUrl.trim().replace(/\/+$/, '') === normTarget) {
                return found.models.map(m => m.trim()).filter(Boolean);
            }
        }
        return undefined;
    }, [modelsSource, models, managedKeys]);

    const profileInfo = useCallback((key: string, url: string, modelIds?: string[]) =>
        getProfileInfo(opencodeProviders, key, url, modelIds), [opencodeProviders]);

    const handleToggleOpenCodeProfile = async (targetKey: string, targetUrl: string, explicitModels?: string[]) => {
        const trimmedKey = targetKey.trim();
        if (!trimmedKey || !targetUrl.trim() || isTogglingRef.current || (querying && trimmedKey === apiKey.trim())) return;
        isTogglingRef.current = true;
        setSyncingKey(trimmedKey);

        try {
            const keyModels = explicitModels ?? getModelsForKey(trimmedKey, targetUrl);
            const providers = await fetchOpencodeProviders();
            const info = getProfileInfo(providers, trimmedKey, targetUrl, keyModels);
            if (info.status === 'synced') {
                // Deactivate / remove profile
                await request('execute_opencode_remove_provider', {
                    providerId: info.providerId
                });
                showToast(t('apiKeyFun.opencode.removedToast', { defaultValue: 'Removed from OpenCode: {{name}}', name: info.providerName }), 'success');
            } else {
                // 'not_present' or 'partial' -> Sync / update profile
                const proxyUrl = targetUrl.trim().replace(/\/+$/, '');
                const cleaned = (keyModels ?? []).map(id => id.trim()).filter(Boolean);
                if (cleaned.length === 0 && !info.existing?.models.length) {
                    throw new Error(t('apiKeyFun.opencode.modelsRequired'));
                }
                const modelInputs = cleaned.length > 0
                    ? cleaned.map(id => ({ id }))
                    : undefined;

                await request('execute_opencode_openai_sync', {
                    proxyUrl,
                    apiKey: trimmedKey,
                    providerId: info.providerId,
                    providerName: info.providerName,
                    models: modelInputs
                });
                showToast(t('apiKeyFun.opencode.syncedToast', { defaultValue: 'Synced to OpenCode: {{name}}', name: info.providerName }), 'success');
            }
            await fetchOpencodeProviders();
        } catch (error: any) {
            showToast(t('apiKeyFun.syncError', { defaultValue: 'Failed to sync: {{error}}', error: error.toString() }), 'error');
        } finally {
            isTogglingRef.current = false;
            setSyncingKey(null);
        }
    };

    const handleSyncOpenCode = async () => {
        if (!apiKey.trim() || !baseUrl.trim() || querying || Boolean(syncingKey)) return;
        await handleToggleOpenCodeProfile(apiKey, baseUrl);
    };

    const handleDeleteKey = (id: string, e: React.MouseEvent) => {
        e.stopPropagation();
        setManagedKeys(prev => prev.filter(item => item.id !== id));
        showToast(t('common.delete_success') || 'Deleted', 'success');
    };

    const startRename = (item: ManagedApiKey, e: React.MouseEvent) => {
        e.stopPropagation();
        setEditingId(item.id);
        setEditNameValue(item.name);
    };

    const saveRename = (id: string) => {
        const trimmed = editNameValue.trim();
        if (trimmed) {
            setManagedKeys(prev => prev.map(item => item.id === id ? { ...item, name: trimmed } : item));
        }
        setEditingId(null);
    };

    const handleApiKeyChange = (value: string) => {
        ++querySeqRef.current;
        setQuerying(false);
        setUsage(null);
        setModels([]);
        setModelsSource(null);
        setQueryError(null);
        setModelsError(null);
        setApiKey(value);
    };

    const handleSelectKey = (item: ManagedApiKey) => {
        setApiKey(item.key);
        setBaseUrl(item.baseUrl || DEFAULT_ENDPOINT);
        if (item.models && item.models.length > 0) {
            setModels(item.models);
            setModelsSource({ key: item.key.trim(), endpoint: (item.baseUrl || DEFAULT_ENDPOINT).trim().replace(/\/+$/, '') });
        }
        runQuery(item.key, item.baseUrl || DEFAULT_ENDPOINT);
    };

    return (
        <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            className="h-full flex flex-col p-6 md:p-8 gap-6 overflow-y-auto max-w-[90rem] mx-auto w-full"
        >
            {/* Header Card */}
            <div
                className="w-full rounded-2xl border border-blue-100 dark:border-indigo-500/20 p-6 md:p-0 md:px-8 md:h-[140px] flex flex-col md:flex-row items-center justify-between gap-6 shadow-xl relative overflow-hidden shrink-0 bg-gradient-to-r from-blue-50 via-indigo-50/60 to-purple-50 dark:from-indigo-950 dark:via-purple-900/40 dark:to-slate-900 transition-colors duration-300"
            >
                {/* Decorative background elements for light mode */}
                <div className="absolute top-0 right-0 w-64 h-64 bg-purple-400/10 dark:bg-purple-500/10 rounded-full blur-3xl -translate-y-1/2 translate-x-1/2"></div>
                <div className="absolute bottom-0 left-0 w-64 h-64 bg-blue-400/10 dark:bg-blue-500/10 rounded-full blur-3xl translate-y-1/2 -translate-x-1/2"></div>

                <div className="flex flex-col md:flex-row items-center md:items-center gap-5 text-center md:text-left z-10 w-full md:w-auto">
                    {/* Branded Logo Box */}
                    <div className="w-14 h-14 md:w-16 md:h-16 bg-white dark:bg-base-100 rounded-2xl flex items-center justify-center shadow-[0_0_20px_rgba(59,130,246,0.15)] dark:shadow-[0_0_25px_rgba(59,130,246,0.3)] border border-blue-100 dark:border-blue-900/50 flex-shrink-0 select-none transform transition-transform duration-300 hover:scale-105">
                        <span className="text-xl md:text-2xl font-bold text-[#e05220] font-sans">
                            {"{AK}"}
                        </span>
                    </div>

                    {/* Info */}
                    <div className="flex flex-col gap-1.5 max-w-4xl">
                        <div className="flex flex-col md:flex-row items-center md:items-end gap-3">
                            <h1 className="text-xl md:text-2xl font-bold text-gray-900 dark:text-white tracking-wide leading-none">
                                {t('apiKeyFun.title', { defaultValue: 'APIKEY.FUN 中转站' })}
                            </h1>
                            <span className="bg-blue-100 text-blue-700 dark:bg-blue-900/50 dark:text-blue-300 border border-blue-200 dark:border-blue-800/40 px-2.5 py-0.5 rounded-full text-[10px] font-semibold tracking-wide uppercase">
                                {t('apiKeyFun.eyebrow', { defaultValue: '中转站' })}
                            </span>
                        </div>
                        <p className="text-xs md:text-sm text-gray-600 dark:text-gray-300/90 leading-relaxed font-normal mt-1">
                            {t('apiKeyFun.description', { defaultValue: 'Antigravity Tools 官方合作中转站，为用户提供稳定、开放、高性价比的大模型 API 接入服务。支持 Claude、OpenAI、Gemini 等主流模型，适合在 Codex、Gemini CLI、Claude Code 及其他开发工具中统一配置使用。通过 Antigravity Tools 专属链接注册，可享受最高充值永久 95 折优惠。' })}
                        </p>
                    </div>
                </div>

                <a
                    href="https://apikey.fan/register?aff=AntManager"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="bg-white hover:bg-blue-50 dark:bg-base-200 dark:hover:bg-base-300 text-blue-700 dark:text-blue-400 border border-blue-200 dark:border-blue-800/50 px-6 py-3 rounded-xl font-bold text-sm flex items-center gap-2 transition-all shadow-md shadow-blue-500/10 dark:shadow-none flex-shrink-0 hover:scale-[1.02] active:scale-[0.98] duration-200 z-10"
                >
                    <ExternalLink size={16} className="text-blue-500 dark:text-blue-400" />
                    <span>{t('apiKeyFun.viewNow', { defaultValue: '立即查看' })}</span>
                </a>
            </div>

            {/* Stats Grid - Full Width Dashboard Metrics */}
            <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-4 w-full">
                {/* Remaining */}
                <motion.div
                    whileHover={{ y: -2 }}
                    className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200 flex flex-row items-center gap-3.5 transition-all duration-300 hover:shadow-md hover:border-blue-100 dark:hover:border-blue-900/30 group"
                >
                    <div className="p-3 bg-green-50 dark:bg-green-950/20 rounded-xl flex-shrink-0 text-green-500 group-hover:scale-110 group-hover:rotate-3 transition-transform duration-300">
                        <Coins className="w-5 h-5" />
                    </div>
                    <div className="flex flex-col min-w-0">
                        <span className="text-xs font-medium text-gray-400 dark:text-gray-500 truncate">
                            {t('apiKeyFun.usage.remainingAmount', { defaultValue: '剩余额度' })}
                        </span>
                        <span className="text-xl font-bold text-gray-900 dark:text-white mt-0.5 tracking-tight truncate">
                            {usage ? usage.remaining : '$0.00'}
                        </span>
                    </div>
                </motion.div>

                {/* Used */}
                <motion.div
                    whileHover={{ y: -2 }}
                    className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200 flex flex-row items-center gap-3.5 transition-all duration-300 hover:shadow-md hover:border-blue-100 dark:hover:border-blue-900/30 group"
                >
                    <div className="p-3 bg-blue-50 dark:bg-blue-950/20 rounded-xl flex-shrink-0 text-blue-500 group-hover:scale-110 group-hover:rotate-3 transition-transform duration-300">
                        <DollarSign className="w-5 h-5" />
                    </div>
                    <div className="flex flex-col min-w-0">
                        <span className="text-xs font-medium text-gray-400 dark:text-gray-500 truncate">
                            {t('apiKeyFun.usage.usedAmount', { defaultValue: '已用额度' })}
                        </span>
                        <span className="text-xl font-bold text-gray-900 dark:text-white mt-0.5 tracking-tight truncate">
                            {usage ? usage.used : '--'}
                        </span>
                    </div>
                </motion.div>

                {/* Today Requests */}
                <motion.div
                    whileHover={{ y: -2 }}
                    className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200 flex flex-row items-center gap-3.5 transition-all duration-300 hover:shadow-md hover:border-blue-100 dark:hover:border-blue-900/30 group"
                >
                    <div className="p-3 bg-orange-50 dark:bg-orange-950/20 rounded-xl flex-shrink-0 text-orange-500 group-hover:scale-110 group-hover:rotate-3 transition-transform duration-300">
                        <Flame className="w-5 h-5" />
                    </div>
                    <div className="flex flex-col min-w-0">
                        <span className="text-xs font-medium text-gray-400 dark:text-gray-500 truncate">
                            {t('apiKeyFun.usage.todayRequests', { defaultValue: 'Today Requests' })}
                        </span>
                        <span className="text-xl font-bold text-gray-900 dark:text-white mt-0.5 tracking-tight truncate">
                            {usage ? usage.todayRequests : '--'}
                        </span>
                    </div>
                </motion.div>

                {/* Today Tokens */}
                <motion.div
                    whileHover={{ y: -2 }}
                    className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200 flex flex-row items-center gap-3.5 transition-all duration-300 hover:shadow-md hover:border-blue-100 dark:hover:border-blue-900/30 group"
                >
                    <div className="p-3 bg-purple-50 dark:bg-purple-950/20 rounded-xl flex-shrink-0 text-purple-500 group-hover:scale-110 group-hover:rotate-3 transition-transform duration-300">
                        <Cpu className="w-5 h-5" />
                    </div>
                    <div className="flex flex-col min-w-0">
                        <span className="text-xs font-medium text-gray-400 dark:text-gray-500 truncate">
                            {t('apiKeyFun.usage.todayTokens', { defaultValue: 'Today Tokens' })}
                        </span>
                        <span className="text-xl font-bold text-gray-900 dark:text-white mt-0.5 tracking-tight truncate">
                            {usage ? usage.todayTokens : '--'}
                        </span>
                    </div>
                </motion.div>

                {/* Total Requests */}
                <motion.div
                    whileHover={{ y: -2 }}
                    className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200 flex flex-row items-center gap-3.5 transition-all duration-300 hover:shadow-md hover:border-blue-100 dark:hover:border-blue-900/30 group"
                >
                    <div className="p-3 bg-indigo-50 dark:bg-indigo-950/20 rounded-xl flex-shrink-0 text-indigo-500 group-hover:scale-110 group-hover:-rotate-3 transition-transform duration-300">
                        <TrendingUp className="w-5 h-5" />
                    </div>
                    <div className="flex flex-col min-w-0">
                        <span className="text-xs font-medium text-gray-400 dark:text-gray-500 truncate">
                            {t('apiKeyFun.usage.totalRequests', { defaultValue: 'Total Requests' })}
                        </span>
                        <span className="text-xl font-bold text-gray-900 dark:text-white mt-0.5 tracking-tight truncate">
                            {usage ? usage.totalRequests : '--'}
                        </span>
                    </div>
                </motion.div>

                {/* Total Tokens */}
                <motion.div
                    whileHover={{ y: -2 }}
                    className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200 flex flex-row items-center gap-3.5 transition-all duration-300 hover:shadow-md hover:border-blue-100 dark:hover:border-blue-900/30 group"
                >
                    <div className="p-3 bg-pink-50 dark:bg-pink-950/20 rounded-xl flex-shrink-0 text-pink-500 group-hover:scale-110 group-hover:-rotate-3 transition-transform duration-300">
                        <Hash className="w-5 h-5" />
                    </div>
                    <div className="flex flex-col min-w-0">
                        <span className="text-xs font-medium text-gray-400 dark:text-gray-500 truncate">
                            {t('apiKeyFun.usage.totalTokens', { defaultValue: 'Total Tokens' })}
                        </span>
                        <span className="text-xl font-bold text-gray-900 dark:text-white mt-0.5 tracking-tight truncate">
                            {usage ? usage.totalTokens : '--'}
                        </span>
                    </div>
                </motion.div>
            </div>

            {/* Bottom Section Layout */}
            <div className="grid grid-cols-1 xl:grid-cols-12 lg:grid-cols-12 gap-6 items-start mt-2 pb-8">
                
                {/* Left Sidebar: Saved Keys List */}
                <div className="xl:col-span-4 lg:col-span-4 bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200 sticky top-5">
                    <h2 className="text-lg font-bold text-gray-900 dark:text-white mb-1 flex items-center gap-2">
                        <Settings size={18} className="text-blue-500" />
                        {t('apiKeyFun.keyManager.title', { defaultValue: 'Key Management' })}
                    </h2>
                    <p className="text-xs text-gray-400 mb-4">
                        {t('apiKeyFun.keyManager.desc', { defaultValue: 'Save frequently used keys, click to quickly switch and query balance.' })}
                    </p>

                    <div className="space-y-2 max-h-[500px] overflow-y-auto pr-1">
                        {managedKeys.map(item => {
                            const isActive = apiKey === item.key;
                            const isEditing = editingId === item.id;
                            
                            return (
                                <div
                                    key={item.id}
                                    onClick={() => !isEditing && handleSelectKey(item)}
                                    className={`p-4 rounded-2xl border text-left transition-all relative flex flex-col justify-between cursor-pointer group ${
                                        isActive
                                            ? 'border-blue-500 bg-blue-50/40 dark:bg-blue-500/10 shadow-sm shadow-blue-500/10'
                                            : 'border-slate-200 dark:border-white/5 hover:border-slate-300 dark:hover:border-white/10 bg-white dark:bg-white/[0.02]'
                                    }`}
                                >
                                    <div className="flex items-center justify-between w-full">
                                        <div className="flex flex-col gap-1.5 w-full min-w-0 pr-2">
                                            {isEditing ? (
                                                <input
                                                    type="text"
                                                    className="input input-sm input-bordered w-full max-w-[200px]"
                                                    value={editNameValue}
                                                    onChange={e => setEditNameValue(e.target.value)}
                                                    onBlur={() => saveRename(item.id)}
                                                    onKeyDown={e => {
                                                        if (e.key === 'Enter') {
                                                            saveRename(item.id);
                                                        } else if (e.key === 'Escape') {
                                                            e.preventDefault();
                                                            setEditingId(null);
                                                        }
                                                    }}
                                                    autoFocus
                                                    onClick={e => e.stopPropagation()}
                                                />
                                            ) : (
                                                <button type="button" className="text-left font-bold text-[13px] text-slate-800 dark:text-gray-200 truncate">
                                                    {item.name}
                                                </button>
                                            )}

                                            <div className="flex items-center gap-2 text-[11px] text-slate-500 dark:text-gray-400 font-medium">
                                                <span>
                                                    {t('apiKeyFun.keyManager.lastRemainingLabel', { defaultValue: '上次余额' })} {item.lastRemaining ? item.lastRemaining : '--'}
                                                </span>
                                                {item.lastStatus && (
                                                    <span className={`w-1.5 h-1.5 rounded-full ${item.lastStatus === 'ok' ? 'bg-green-500 shadow-[0_0_8px_rgba(34,197,94,0.6)]' : 'bg-red-500 shadow-[0_0_8px_rgba(239,68,68,0.6)]'}`} />
                                                )}
                                            </div>

                                            <span className="text-[10px] text-slate-400 dark:text-gray-500 font-normal">
                                                {t('apiKeyFun.keyManager.addedAt', { defaultValue: '添加于' })} {formatDate(item.createdAt)}
                                            </span>
                                        </div>
                                        
                                        {/* Hover Actions */}
                                        <div className="flex flex-row items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity shrink-0">
                                            <button
                                                onClick={e => { e.stopPropagation(); handleCopy(item.key); }}
                                                className="p-1.5 text-slate-400 hover:text-blue-500 hover:bg-blue-50 dark:hover:bg-white/10 rounded-lg transition-colors"
                                                title="Copy Key"
                                            >
                                                <Copy size={15} />
                                            </button>
                                            {!isEditing && (
                                                <button
                                                    onClick={e => startRename(item, e)}
                                                    className="p-1.5 text-slate-400 hover:text-blue-500 hover:bg-blue-50 dark:hover:bg-white/10 rounded-lg transition-colors"
                                                    title="Edit Name"
                                                >
                                                    <Pencil size={14} />
                                                </button>
                                            )}
                                            <button
                                                onClick={e => handleDeleteKey(item.id, e)}
                                                className="p-1.5 text-slate-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-500/10 rounded-lg transition-colors"
                                                title="Delete"
                                            >
                                                <Trash2 size={15} />
                                            </button>
                                        </div>
                                    </div>

                                    {/* OpenCode Profile Status & Activation */}
                                    {(() => {
                                        const isActiveKey = item.key.trim() === apiKey.trim();
                                        const effectiveUrl = (isActiveKey && baseUrl.trim()) ? baseUrl.trim() : (item.baseUrl || DEFAULT_ENDPOINT);
                                        const currentModels = getModelsForKey(item.key, effectiveUrl);
                                        const keyInfo = profileInfo(item.key, effectiveUrl, currentModels);
                                        const isThisKeySyncing = syncingKey === item.key.trim();

                                        return (
                                            <div
                                                onClick={e => e.stopPropagation()}
                                                className="flex items-center justify-between pt-2 mt-2 border-t border-slate-100 dark:border-white/5 text-[11px]"
                                            >
                                                <div
                                                    role="checkbox"
                                                    aria-checked={keyInfo.status === 'synced' ? true : keyInfo.status === 'partial' ? 'mixed' : false}
                                                    aria-label={`OpenCode ${keyInfo.suffix}`}
                                                    aria-busy={isThisKeySyncing}
                                                    aria-disabled={Boolean(syncingKey) || (isActiveKey && querying)}
                                                    tabIndex={0}
                                                    className="flex items-center gap-1.5 min-w-0 cursor-pointer select-none rounded focus:outline-none focus:ring-1 focus:ring-teal-500"
                                                    onClick={() => handleToggleOpenCodeProfile(item.key, effectiveUrl, currentModels)}
                                                    onKeyDown={(e) => {
                                                        if (e.key === 'Enter' || e.key === ' ') {
                                                            e.preventDefault();
                                                            handleToggleOpenCodeProfile(item.key, effectiveUrl, currentModels);
                                                        }
                                                    }}
                                                    title={
                                                        keyInfo.status === 'synced'
                                                            ? t('apiKeyFun.opencode.clickToRemove', { defaultValue: 'OpenCode profile is active and synced. Click to deactivate/remove' })
                                                            : keyInfo.status === 'partial'
                                                                ? t('apiKeyFun.opencode.clickToUpdate', { defaultValue: 'Settings or models differ. Click to update' })
                                                                : t('apiKeyFun.opencode.clickToActivate', { defaultValue: 'Click to activate separate profile in OpenCode' })
                                                    }
                                                >
                                                    {isThisKeySyncing ? (
                                                        <RefreshCw size={13} className="animate-spin text-teal-500 shrink-0" />
                                                    ) : keyInfo.status === 'synced' ? (
                                                        <CheckSquare size={14} className="text-emerald-500 shrink-0" />
                                                    ) : keyInfo.status === 'partial' ? (
                                                        <MinusSquare size={14} className="text-amber-500 shrink-0" />
                                                    ) : (
                                                        <Square size={14} className="text-slate-400 dark:text-gray-600 shrink-0" />
                                                    )}
                                                    <span className="font-mono text-slate-600 dark:text-gray-300 truncate">
                                                        OpenCode: <span className="font-semibold text-slate-800 dark:text-gray-100">{keyInfo.suffix}</span>
                                                    </span>
                                                </div>

                                                <div className="flex items-center gap-1 shrink-0">
                                                    {keyInfo.status === 'synced' && (
                                                        <span className="badge badge-xs badge-success text-[9px] font-medium gap-1 py-1 px-1.5">
                                                            {t('apiKeyFun.opencode.active', { defaultValue: 'Active' })}
                                                        </span>
                                                    )}
                                                    {keyInfo.status === 'partial' && (
                                                        <span className="badge badge-xs badge-warning text-[9px] font-medium gap-1 py-1 px-1.5">
                                                            {t('apiKeyFun.opencode.modified', { defaultValue: 'Modified' })}
                                                        </span>
                                                    )}
                                                    {keyInfo.status === 'not_present' && (
                                                        <span className="text-[10px] text-slate-400 dark:text-gray-500">
                                                            {t('apiKeyFun.opencode.inactive', { defaultValue: 'Inactive' })}
                                                        </span>
                                                    )}
                                                </div>
                                            </div>
                                        );
                                    })()}
                                </div>
                            );
                        })}

                        {managedKeys.length === 0 && (
                            <div className="text-center py-8 text-gray-400 text-xs flex flex-col items-center gap-2">
                                <HelpCircle size={28} className="opacity-20" />
                                <span>{t('apiKeyFun.keyManager.empty', { defaultValue: 'No saved keys.' })}</span>
                            </div>
                        )}
                    </div>
                </div>

                {/* Right Main Panel: Query and Models */}
                <div className="xl:col-span-8 lg:col-span-8 space-y-6">
                    {/* Query Form */}
                    <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
                        <h2 className="text-lg font-bold text-gray-900 dark:text-white mb-4 flex items-center gap-2">
                            <Activity size={18} className="text-blue-500" />
                            {t('apiKeyFun.queryTitle', { defaultValue: 'Key Quota Query' })}
                        </h2>

                        <div className="flex flex-col gap-4">
                            <div className="flex flex-col sm:flex-row gap-4 items-end w-full">
                                <div className="form-control w-full flex-1">
                                    <label className="label mb-1">
                                        <span className="label-text font-bold text-slate-700 dark:text-gray-300">{t('apiKeyFun.apiKeyLabel', { defaultValue: 'API Key' })} <span className="text-red-500 ml-0.5">*</span></span>
                                    </label>
                                    <div className="relative">
                                        <input
                                            type={showApiKey ? 'text' : 'password'}
                                            className="w-full h-14 pl-5 pr-36 font-mono text-sm bg-slate-50 dark:bg-black/20 border-2 border-slate-200 dark:border-white/10 rounded-2xl focus:bg-white dark:focus:bg-black/40 focus:border-blue-500 dark:focus:border-blue-500/80 focus:ring-4 focus:ring-blue-500/20 dark:focus:ring-blue-500/10 transition-all shadow-sm outline-none text-gray-800 dark:text-gray-200 placeholder-slate-400 dark:placeholder-gray-600"
                                            placeholder={t('apiKeyFun.apiKeyPlaceholder', { defaultValue: 'Paste your API Key...' })}
                                            value={apiKey}
                                            onChange={e => handleApiKeyChange(e.target.value)}
                                            onKeyDown={(e) => {
                                                if (e.key === 'Enter' && !querying && apiKey) {
                                                    runQuery(apiKey, baseUrl);
                                                }
                                            }}
                                        />
                                        <div className="absolute right-2.5 top-1/2 -translate-y-1/2 flex items-center gap-1">
                                            {apiKey && (
                                                <button
                                                    className="p-2 rounded-lg text-slate-400 hover:bg-slate-200 hover:text-slate-600 dark:text-gray-500 dark:hover:bg-white/10 dark:hover:text-gray-300 transition-colors"
                                                    onClick={() => handleApiKeyChange('')}
                                                    title="Clear"
                                                >
                                                    <X size={16} strokeWidth={2.5} />
                                                </button>
                                            )}
                                            <button
                                                className="p-2 rounded-lg text-slate-400 hover:bg-slate-200 hover:text-slate-600 dark:text-gray-500 dark:hover:bg-white/10 dark:hover:text-gray-300 transition-colors"
                                                onClick={() => setShowApiKey(!showApiKey)}
                                                title={showApiKey ? "Hide" : "Show"}
                                            >
                                                {showApiKey ? <EyeOff size={16} strokeWidth={2.5} /> : <Eye size={16} strokeWidth={2.5} />}
                                            </button>
                                            <div className="w-px h-5 bg-slate-200 dark:bg-white/10 mx-1"></div>
                                            <button
                                                className="p-2 rounded-xl bg-white dark:bg-white/5 hover:bg-blue-50 dark:hover:bg-blue-500/20 text-slate-500 dark:text-gray-400 hover:text-blue-600 dark:hover:text-blue-400 transition-all border border-slate-200 dark:border-white/5 shadow-sm"
                                                onClick={() => handleCopy(apiKey)}
                                                disabled={!apiKey}
                                                title="Copy"
                                            >
                                                <Copy size={16} strokeWidth={2.5} />
                                            </button>
                                        </div>
                                    </div>
                                </div>
                                <div className="flex gap-2 w-full sm:w-auto shrink-0 mt-8">
                                    <button
                                        onClick={() => runQuery(apiKey, baseUrl)}
                                        className={`btn h-14 min-h-0 rounded-2xl bg-blue-500 hover:bg-blue-600 text-white border-none flex items-center justify-center gap-2 shadow-md shadow-blue-500/20 transition-all sm:w-32 text-sm font-bold ${querying ? 'opacity-70' : ''}`}
                                        disabled={querying || !apiKey}
                                    >
                                        <RefreshCw size={16} className={querying ? 'animate-spin' : ''} />
                                        <span>{querying ? t('common.loading') : t('common.refresh')}</span>
                                    </button>
                                </div>
                            </div>
                            
                            {/* Premium CLI Actions Banner */}
                            <div className="mt-4 flex flex-col sm:flex-row items-start sm:items-center justify-between p-3.5 rounded-xl bg-gradient-to-r from-blue-50/80 to-purple-50/80 dark:from-blue-900/20 dark:to-purple-900/20 border border-blue-500/20 shadow-sm relative overflow-hidden group/banner transition-all hover:shadow-md">
                                {/* Subtle Background Effect */}
                                <div className="absolute -right-6 -top-6 text-purple-500/10 dark:text-purple-400/5 rotate-12 transition-transform group-hover/banner:rotate-45 duration-700">
                                    <Wand2 size={80} />
                                </div>
                                
                                <div className="flex items-center gap-3 z-10 mb-3 sm:mb-0">
                                    <div className="p-2 rounded-lg bg-base-100/80 shadow-sm border border-base-300 backdrop-blur-sm">
                                        <Zap size={16} className="text-amber-500" />
                                    </div>
                                    <div className="flex flex-col">
                                        <span className="text-sm font-bold text-base-content">
                                            {t('apiKeyFun.cli.quickConfig', { defaultValue: '一键配置本地开发环境' })}
                                        </span>
                                        <span className="text-[10px] text-base-content/60 font-medium">
                                            {t('apiKeyFun.cli.syncDesc', { defaultValue: '同步至官方标准配置' })}
                                        </span>
                                    </div>
                                </div>

                                {(() => {
                                    const hasModels = models.length > 0;
                                    const hasGpt = models.some(m => {
                                        const lower = m.toLowerCase();
                                        return lower.includes('gpt') || lower.includes('o1') || lower.includes('o3') || lower.includes('deepseek') || lower.includes('qwen');
                                    });
                                    const hasClaude = models.some(m => m.toLowerCase().includes('claude'));
                                    
                                    // 默认都显示，除非明确检测到只支持其中一种
                                    const showCodex = !hasModels || hasGpt || (!hasGpt && !hasClaude);
                                    const showClaude = !hasModels || hasClaude || (!hasGpt && !hasClaude);

                                    return (
                                        <div className="flex items-center gap-2 w-full sm:w-auto z-10 pl-11 sm:pl-0">
                                            {showCodex && (
                                                <button 
                                                    onClick={() => handleSyncCli('Codex')}
                                                    className="flex-1 sm:flex-none btn btn-sm px-5 font-medium rounded-full bg-blue-500 hover:bg-blue-600 text-white border-none shadow-md shadow-blue-500/20 transition-all group"
                                                    disabled={!apiKey.trim()}
                                                    title={t('apiKeyFun.cli.codexTooltip', { defaultValue: 'Sync API Key & BaseURL to Codex / ChatGPT CLI' })}
                                                >
                                                    <Code size={14} className="mr-1.5 opacity-90 group-hover:scale-110 group-hover:opacity-100 transition-all" />
                                                    Codex
                                                </button>
                                            )}
                                            {showClaude && (
                                                <button 
                                                    onClick={() => handleSyncCli('Claude')}
                                                    className="flex-1 sm:flex-none btn btn-sm px-5 font-medium rounded-full bg-purple-500 hover:bg-purple-600 text-white border-none shadow-md shadow-purple-500/20 transition-all group"
                                                    disabled={!apiKey.trim()}
                                                    title={t('apiKeyFun.cli.claudeTooltip', { defaultValue: 'Sync API Key & BaseURL to Claude Code' })}
                                                >
                                                    <Cpu size={14} className="mr-1.5 opacity-90 group-hover:scale-110 group-hover:opacity-100 transition-all" />
                                                    Claude
                                                </button>
                                            )}
                                            {(() => {
                                                const activeModels = getModelsForKey(apiKey, baseUrl);
                                                const activeInfo = profileInfo(apiKey, baseUrl, activeModels);
                                                const isCurrentKeySyncing = syncingKey === apiKey.trim();

                                                return (
                                                    <div className="flex items-center gap-1.5 flex-1 sm:flex-none">
                                                        <button
                                                            onClick={handleSyncOpenCode}
                                                            role="checkbox"
                                                            aria-checked={activeInfo.status === 'synced' ? true : activeInfo.status === 'partial' ? 'mixed' : false}
                                                            aria-label={`OpenCode ${activeInfo.suffix}`}
                                                            className={`btn btn-sm px-4 font-medium rounded-full border-none shadow-md transition-all group flex items-center gap-2 ${
                                                                activeInfo.status === 'synced'
                                                                    ? 'bg-teal-600 hover:bg-teal-700 text-white shadow-teal-600/20'
                                                                    : activeInfo.status === 'partial'
                                                                        ? 'bg-amber-500 hover:bg-amber-600 text-white shadow-amber-500/20'
                                                                        : 'bg-teal-500 hover:bg-teal-600 text-white shadow-teal-500/20'
                                                            }`}
                                                            disabled={!apiKey.trim() || !baseUrl.trim() || querying || Boolean(syncingKey)}
                                                            title={
                                                                activeInfo.status === 'synced'
                                                                    ? t('apiKeyFun.opencode.syncedActiveTooltip', { defaultValue: 'OpenCode: profile is active and fully synced. Click to deactivate' })
                                                                    : activeInfo.status === 'partial'
                                                                        ? t('apiKeyFun.opencode.partialTooltip', { defaultValue: 'OpenCode: settings or models differ. Click to sync' })
                                                                        : t('apiKeyFun.cli.opencodeTooltip', { defaultValue: 'Activate as separate profile in OpenCode' })
                                                            }
                                                        >
                                                            {isCurrentKeySyncing ? (
                                                                <RefreshCw size={13} className="animate-spin mr-0.5" />
                                                            ) : activeInfo.status === 'synced' ? (
                                                                <CheckSquare size={14} className="text-white shrink-0" />
                                                            ) : activeInfo.status === 'partial' ? (
                                                                <MinusSquare size={14} className="text-white shrink-0" />
                                                            ) : (
                                                                <Square size={14} className="text-white/70 shrink-0" />
                                                            )}
                                                            <CodeXml size={14} className="opacity-90 group-hover:scale-110 group-hover:opacity-100 transition-all" />
                                                            <span>OpenCode</span>
                                                            {activeInfo.suffix && (
                                                                <span className="text-[10px] opacity-85 font-mono">
                                                                    ({activeInfo.suffix})
                                                                </span>
                                                            )}
                                                        </button>
                                                    </div>
                                                );
                                            })()}
                                        </div>
                                    );
                                })()}
                            </div>
                        </div>

                        {queryError && (
                            <div className="alert alert-error text-xs p-3 rounded-lg mt-4 flex items-start gap-2 bg-red-50 dark:bg-red-950/20 text-red-600 dark:text-red-400 border border-red-100 dark:border-red-900/30">
                                <span>{queryError}</span>
                            </div>
                        )}
                    </div>

                    {/* Available Models */}
                    <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
                        <h2 className="text-base font-bold text-gray-900 dark:text-white mb-3 flex items-center gap-2">
                            <Layers size={16} className="text-blue-500" />
                            {t('apiKeyFun.models.title', { defaultValue: 'Available Models' })}
                            {models.length > 0 && <span className="text-xs font-normal text-gray-400">({models.length})</span>}
                        </h2>
                        
                        {modelsError ? (
                            <div className="bg-red-50 dark:bg-red-950/20 text-red-600 dark:text-red-400 p-3 rounded-lg border border-red-100 dark:border-red-900/30 text-xs mt-2">
                                {modelsError}
                            </div>
                        ) : models.length === 0 ? (
                            <p className="text-xs text-gray-400 mt-2">
                                {apiKey ? t('apiKeyFun.models.emptyFromKey', { defaultValue: 'No models returned yet. Query to fetch models.' }) 
                                       : t('apiKeyFun.models.empty', { defaultValue: 'Enter key and query to load available models.' })}
                            </p>
                        ) : (
                            <div className="flex flex-wrap gap-1.5 max-h-[400px] overflow-y-auto pt-2">
                                {models.map(m => (
                                    <span key={m} className="px-2.5 py-1 bg-gray-50 dark:bg-base-200 text-gray-700 dark:text-gray-300 text-xs rounded-md border border-gray-200 dark:border-base-300 font-mono hover:bg-gray-100 dark:hover:bg-base-300 transition-colors shadow-sm cursor-default">
                                        {m}
                                    </span>
                                ))}
                            </div>
                        )}
                    </div>
                </div>

            </div>
        </motion.div>
    );
};
