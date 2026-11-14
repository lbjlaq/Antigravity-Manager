import React, { useState, useEffect, useMemo, useRef, startTransition } from 'react';
import { useTranslation } from 'react-i18next';
import { request as invoke } from '../utils/request';
import { isTauri } from '../utils/env';
import { copyToClipboard } from '../utils/clipboard';
import { inferModelGroup } from '../config/modelConfig';
import {
    Power,
    Copy,
    RefreshCw,
    CheckCircle,
    Settings,
    Target,
    Plus,
    Terminal,
    Trash2,
    BrainCircuit,
    ArrowRight,
    Sparkles,
    Code,
    Check,
    X,
    Edit2,
    Save,
    Share2,
    Bot,
    Timer,
    Zap,
    Square,
    ChevronDown,
    ChevronRight,
    Layers,
    Eye,
    EyeOff,
    AlertTriangle,
    ShieldAlert
} from 'lucide-react';
import { AppConfig, ProxyConfig, StickySessionConfig, ExperimentalConfig, RECOMMENDED_PROMPT_REWRITE_PRESETS } from '../types/config';
import HelpTooltip from '../components/common/HelpTooltip';
import ModalDialog from '../components/common/ModalDialog';
import { showToast } from '../components/common/ToastContainer';
import { cn } from '../utils/cn';
import { useProxyModels, getModelIcon } from '../hooks/useProxyModels';
import GroupedSelect, { SelectOption } from '../components/common/GroupedSelect';
import { CliSyncCard } from '../components/proxy/CliSyncCard';
import { listAccounts } from '../services/accountService';
import { motion } from 'framer-motion';
import CircuitBreaker from '../components/settings/CircuitBreaker';
import GlobalSystemPrompt from '../components/settings/GlobalSystemPrompt';
import PromptRewriteSettings from '../components/settings/PromptRewriteSettings';
import ImageThinkingMode from '../components/settings/ImageThinkingMode';
import ThinkingBudget from '../components/settings/ThinkingBudget';
import MultimodalSettings from '../components/settings/MultimodalSettings';
import TimeoutSettings from '../components/settings/TimeoutSettings';import AgentSettings from '../components/settings/AgentSettings';
import { CircuitBreakerConfig } from '../types/config';
import { RequestDetailModal, ProxyRequestLog } from '../components/proxy/RequestDetailModal';

interface ProxyStatus {
    running: boolean;
    port: number;
    base_url: string;
    active_accounts: number;
}

interface CustomPreset {
    id: string;
    name: string;
    description: string;
    mappings: Record<string, string>;
}

export const DEFAULT_RECOMMENDED_HUB_UA = 'antigravity/hub/2.28.0 (aidev_client; os_type=windows; arch=amd64; cl=1008300195)';
export const RECOMMENDED_HUB_VERSION = '2.28.0';


interface CollapsibleCardProps {
    title: string;
    icon: React.ReactNode;
    enabled?: boolean;
    onToggle?: (enabled: boolean) => void;
    children: React.ReactNode;
    defaultExpanded?: boolean;
    rightElement?: React.ReactNode;
    allowInteractionWhenDisabled?: boolean;
}

function CollapsibleCard({
    title,
    icon,
    enabled,
    onToggle,
    children,
    defaultExpanded = false,
    rightElement,
    allowInteractionWhenDisabled = false,
}: CollapsibleCardProps) {
    const [isExpanded, setIsExpanded] = useState(defaultExpanded);
    const { t } = useTranslation();

    return (
        <div className="bg-white dark:bg-base-100 rounded-xl shadow-xs border border-gray-200/80 dark:border-base-200 overflow-hidden transition-all duration-200 hover:shadow-sm">
            <div
                className="px-5 py-3.5 flex items-center justify-between cursor-pointer bg-gray-50/70 dark:bg-base-200 hover:bg-gray-100/70 dark:hover:bg-base-300/70 transition-colors"
                onClick={(e) => {
                    // Prevent toggle when clicking the switch or right element
                    if ((e.target as HTMLElement).closest('.no-expand')) return;
                    setIsExpanded(!isExpanded);
                }}
            >
                <div className="flex items-center gap-3">
                    <div className="text-gray-500 dark:text-gray-400">
                        {icon}
                    </div>
                    <span className="font-semibold text-sm text-gray-900 dark:text-white">
                        {title}
                    </span>
                    {enabled !== undefined && (
                        <div className={cn('text-xs px-2.5 py-0.5 rounded-full font-medium border', enabled ? 'bg-emerald-50 text-emerald-700 border-emerald-200 dark:bg-emerald-950/50 dark:text-emerald-400 dark:border-emerald-800/60' : 'bg-gray-100 text-gray-600 border-gray-200 dark:bg-base-200 dark:text-gray-400 dark:border-base-300')}>
                            {enabled ? t('common.enabled') : t('common.disabled')}
                        </div>
                    )}
                </div>

                <div className="flex items-center gap-4 no-expand">
                    {rightElement}

                    {enabled !== undefined && onToggle && (
                        <div className="flex items-center" onClick={(e) => e.stopPropagation()}>
                            <input
                                type="checkbox"
                                className="toggle toggle-sm bg-gray-200 dark:bg-base-300 border-gray-300 dark:border-base-300 checked:bg-blue-600 checked:border-blue-600"
                                checked={enabled}
                                onChange={(e) => onToggle(e.target.checked)}
                            />
                        </div>
                    )}

                    <button
                        className={cn('p-1.5 rounded-lg text-gray-500 hover:text-gray-800 hover:bg-gray-200/70 dark:text-gray-400 dark:hover:text-gray-200 dark:hover:bg-base-200 transition-all duration-200', isExpanded ? 'rotate-180' : '')}
                    >
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                            <path d="m6 9 6 6 6-6" />
                        </svg>
                    </button>
                </div>
            </div>

            <div
                className={`transition-all duration-300 ease-in-out border-t border-gray-200/80 dark:border-base-200 ${isExpanded ? 'max-h-[2000px] opacity-100' : 'max-h-0 opacity-0 overflow-hidden'
                    }`}
            >
                <div className="p-5 relative">
                    {/* Overlay when disabled */}
                    {enabled === false && !allowInteractionWhenDisabled && (
                        <div className="absolute inset-0 bg-gray-900/10 dark:bg-black/40 backdrop-blur-[0.5px] z-10 cursor-not-allowed" />
                    )}
                    <div className={enabled === false && !allowInteractionWhenDisabled ? 'opacity-50 pointer-events-none select-none' : ''}>
                        {children}
                    </div>
                </div>
            </div>
        </div>
    );
}

export default function ApiProxy() {
    const { t } = useTranslation();

    const { models } = useProxyModels();

    const [status, setStatus] = useState<ProxyStatus>({
        running: false,
        port: 0,
        base_url: '',
        active_accounts: 0,
    });

    const [appConfig, setAppConfig] = useState<AppConfig | null>(null);
    const [activeMenuTab, setActiveMenuTab] = useState<'settings' | 'models' | 'guard' | 'cli' | 'protocols'>('settings');
    const [configLoading, setConfigLoading] = useState(true);
    const [configError, setConfigError] = useState<string | null>(null);
    const [loading, setLoading] = useState(false);
    const [copied, setCopied] = useState<string | null>(null);
    const [selectedProtocol, setSelectedProtocol] = useState<'openai' | 'anthropic' | 'gemini'>('openai');
    const [selectedModelId, setSelectedModelId] = useState('gemini-3.8-flash-tiered');
    const [customMappingValue, setCustomMappingValue] = useState(''); // 自定义映射表单的选中值
    const [editingKey, setEditingKey] = useState<string | null>(null);
    const [editingValue, setEditingValue] = useState<string>('');

    // API Key editing states
    const [isEditingApiKey, setIsEditingApiKey] = useState(false);
    const [tempApiKey, setTempApiKey] = useState('');

    const [isEditingAdminPassword, setIsEditingAdminPassword] = useState(false);
    const [tempAdminPassword, setTempAdminPassword] = useState('');

    // Preset selection state
    const [selectedPreset, setSelectedPreset] = useState<string>('default');
    const [customPresets, setCustomPresets] = useState<CustomPreset[]>([]);
    const [isPresetManagerOpen, setIsPresetManagerOpen] = useState(false);
    const [newPresetName, setNewPresetName] = useState('');

    // Modal states

    // Modal states
    const [isResetConfirmOpen, setIsResetConfirmOpen] = useState(false);
    const [isRegenerateKeyConfirmOpen, setIsRegenerateKeyConfirmOpen] = useState(false);
    const [isClearBindingsConfirmOpen, setIsClearBindingsConfirmOpen] = useState(false);
    const [isClearRateLimitsConfirmOpen, setIsClearRateLimitsConfirmOpen] = useState(false);
    // Dynamic Hub Fingerprint from launch detection
    const [latestHubVersion, setLatestHubVersion] = useState<string>('2.28.0');
    const [latestHubUa, setLatestHubUa] = useState<string>(DEFAULT_RECOMMENDED_HUB_UA);

    // User-Agent & Argon Sandbox Tester states
    const [uaSandboxModel, setUaSandboxModel] = useState('gemini-4-argon');
    const [uaSandboxTesting, setUaSandboxTesting] = useState(false);
    const [uaSandboxResult, setUaSandboxResult] = useState<{
        testedUa: string;
        model: string;
        status?: number;
        durationMs?: number;
        success: boolean;
        error?: string;
        message?: string;
        logId?: string;
        gatewayDiagnosis?: string;
        gatewaySuggestion?: string;
        gatewayCode?: string;
    } | null>(null);

    // [FIX #820] Fixed account mode states
    const [preferredAccountId, setPreferredAccountId] = useState<string | null>(null);
    const [availableAccounts, setAvailableAccounts] = useState<Array<{ id: string; email: string }>>([]);

    // Cloudflared (CF隧道) states
    const [cfStatus, setCfStatus] = useState<{ installed: boolean; version?: string; running: boolean; url?: string; error?: string }>({
        installed: false,
        running: false,
    });
    const [cfLoading, setCfLoading] = useState(false);
    const [cfMode, setCfMode] = useState<'quick' | 'auth'>('quick');
    const [cfToken, setCfToken] = useState('');
    const [cfUseHttp2, setCfUseHttp2] = useState(true); // 默认启用HTTP/2，更稳定

    // 生成自定义映射表单的选项 (从 models 动态生成，统一纯正 Model ID 风格)
    const customMappingOptions: SelectOption[] = useMemo(() => {
        return models.map(model => ({
            value: model.id,
            label: model.id,
            group: model.group || 'Other'
        }));
    }, [models]);

    // Model Connectivity Test states
    interface ModelTestStatus {
        loading?: boolean;
        success?: boolean;
        status?: number;
        durationMs?: number;
        accountEmail?: string;
        accountId?: string;
        logId?: string;
        isRetired?: boolean;
        error?: string;
        isHistorical?: boolean;
    }
    const [modelTestResults, setModelTestResults] = useState<Record<string, ModelTestStatus>>({});
    const [testTargetAccountId, setTestTargetAccountId] = useState<string>('');
    const [testingAll, setTestingAll] = useState(false);
    const stopTestingRef = useRef(false);

    // 模型多级分组与账号选择器状态
    const [selectedSeriesTab, setSelectedSeriesTab] = useState<string>('all');
    const [expandedFamilies, setExpandedFamilies] = useState<Record<string, boolean>>({});
    const [accountDropdownOpen, setAccountDropdownOpen] = useState(false);
    const accountDropdownRef = useRef<HTMLDivElement>(null);

    // 点击外部自动收起账号下拉窗
    useEffect(() => {
        const handleClickOutside = (event: MouseEvent) => {
            if (accountDropdownRef.current && !accountDropdownRef.current.contains(event.target as Node)) {
                setAccountDropdownOpen(false);
            }
        };
        document.addEventListener('mousedown', handleClickOutside);
        return () => document.removeEventListener('mousedown', handleClickOutside);
    }, []);

    // Request detail modal for probe logs
    const [selectedDetailLog, setSelectedDetailLog] = useState<ProxyRequestLog | null>(null);
    const [loadingDetailLog, setLoadingDetailLog] = useState(false);

    // 初始化加载
    useEffect(() => {
        loadConfig();
        loadStatus();
        loadAccounts();
        loadPreferredAccount();
        loadCfStatus();
        loadCustomPresets();
        loadRecentModelStatuses();
        loadHubFingerprintInfo();
        const interval = setInterval(loadStatus, 3000);
        const cfInterval = setInterval(loadCfStatus, 5000);
        return () => {
            clearInterval(interval);
            clearInterval(cfInterval);
        };
    }, []);

    // 动态获取应用启动时探测到的官方 Hub 版本信息
    const loadHubFingerprintInfo = async () => {
        try {
            const updateStatus = await invoke<{ current: string; latest?: string; update_available: boolean }>('check_hub_update');
            const fp = await invoke<{ version: string; user_agent: string }>('get_hub_fingerprint');
            const targetVersion = (updateStatus && updateStatus.latest) || (fp && fp.version) || '2.28.0';
            setLatestHubVersion(targetVersion);
            if (fp && fp.user_agent) {
                setLatestHubUa(fp.user_agent);
            }
        } catch (e) {
            console.debug('Failed to load dynamic hub fingerprint:', e);
        }
    };

    // 预填最近请求日志的模型连通性状态
    const loadRecentModelStatuses = async () => {
        try {
            const logs = await invoke<ProxyRequestLog[]>('get_proxy_logs_paginated', { limit: 100, offset: 0 });
            if (Array.isArray(logs)) {
                setModelTestResults(prev => {
                    const next = { ...prev };
                    for (const log of logs) {
                        const mId = log.model || log.mapped_model;
                        if (mId && !next[mId]) {
                            next[mId] = {
                                loading: false,
                                success: log.status >= 200 && log.status < 300,
                                status: log.status,
                                durationMs: log.duration,
                                accountEmail: log.account_email,
                                logId: log.id,
                                isHistorical: true
                            };
                        }
                    }
                    return next;
                });
            }
        } catch {
            // ignore
        }
    };

    const handleTestModel = async (modelId: string) => {
        setModelTestResults(prev => ({
            ...prev,
            [modelId]: { ...prev[modelId], loading: true, error: undefined }
        }));
        try {
            const res = await invoke<{
                success: boolean;
                message: string;
                log_id?: string;
                duration_ms?: number;
                status?: number;
                account_email?: string;
                account_id?: string;
                is_retired?: boolean;
            }>('test_model_connectivity', {
                model: modelId,
                targetAccountId: testTargetAccountId || null
            });

            setModelTestResults(prev => ({
                ...prev,
                [modelId]: {
                    loading: false,
                    success: res.success,
                    status: res.status,
                    durationMs: res.duration_ms,
                    accountEmail: res.account_email,
                    accountId: res.account_id,
                    logId: res.log_id,
                    isRetired: res.is_retired,
                    error: res.success ? undefined : res.message,
                    isHistorical: false
                }
            }));
        } catch (err) {
            setModelTestResults(prev => ({
                ...prev,
                [modelId]: {
                    loading: false,
                    success: false,
                    error: String(err),
                    isHistorical: false
                }
            }));
        }
    };

    const handleBatchTest = async () => {
        if (testingAll) {
            stopTestingRef.current = true;
            setTestingAll(false);
            return;
        }
        setTestingAll(true);
        stopTestingRef.current = false;

        for (const m of filteredModels) {
            if (stopTestingRef.current) break;
            await handleTestModel(m.id);
        }
        setTestingAll(false);
    };

    const handleOpenLogDetail = async (logId: string) => {
        setLoadingDetailLog(true);
        try {
            const detail = await invoke<ProxyRequestLog>('get_proxy_log_detail', { logId, log_id: logId });
            setSelectedDetailLog(detail);
        } catch (e) {
            showToast(String(e), 'error');
        } finally {
            setLoadingDetailLog(false);
        }
    };



    // [FIX #820] Load available accounts for fixed account mode
    const loadAccounts = async () => {
        try {
            const accounts = await listAccounts();
            setAvailableAccounts(accounts.filter(a => !a.banned).map(a => ({ id: a.id, email: a.email })));
        } catch (error) {
            console.error('Failed to load accounts:', error);
        }
    };

    // Cloudflared: 检查状态
    const loadCfStatus = async () => {
        try {
            const status = await invoke<typeof cfStatus>('cloudflared_get_status');
            setCfStatus(status);
        } catch (error) {
            // 忽略错误，可能是manager未初始化
        }
    };

    // Cloudflared: 安装
    const handleCfInstall = async () => {
        console.log('[Cloudflared] Install button clicked');
        setCfLoading(true);
        try {
            console.log('[Cloudflared] Calling cloudflared_install...');
            const status = await invoke<typeof cfStatus>('cloudflared_install');
            console.log('[Cloudflared] Install result:', status);
            setCfStatus(status);
            showToast(t('proxy.cloudflared.install_success', { defaultValue: 'Cloudflared installed successfully' }), 'success');
        } catch (error) {
            console.error('[Cloudflared] Install error:', error);
            showToast(String(error), 'error');
        } finally {
            setCfLoading(false);
        }
    };

    // Cloudflared: 启动/停止
    const handleCfToggle = async (enable: boolean) => {
        if (enable && !status.running) {
            showToast(
                t('proxy.cloudflared.require_proxy_running', { defaultValue: 'Please start the local proxy service first' }),
                'warning'
            );
            return;
        }
        setCfLoading(true);
        try {
            if (enable) {
                if (!cfStatus.installed) {
                    const installStatus = await invoke<typeof cfStatus>('cloudflared_install');
                    setCfStatus(installStatus);
                    if (!installStatus.installed) {
                        throw new Error('Cloudflared install failed');
                    }
                    showToast(t('proxy.cloudflared.install_success', { defaultValue: 'Cloudflared installed successfully' }), 'success');
                }

                const config = {
                    enabled: true,
                    mode: cfMode,
                    port: appConfig?.proxy.port || 8045,
                    token: cfMode === 'auth' ? cfToken : null,
                    use_http2: cfUseHttp2,
                };
                const status = await invoke<typeof cfStatus>('cloudflared_start', { config });
                setCfStatus(status);
                showToast(t('proxy.cloudflared.started', { defaultValue: 'Tunnel started' }), 'success');

                // 持久化“启用”状态
                if (appConfig) {
                    const newConfig = {
                        ...appConfig,
                        cloudflared: {
                            ...appConfig.cloudflared,
                            enabled: true,
                            mode: cfMode,
                            token: cfToken,
                            use_http2: cfUseHttp2,
                            port: appConfig.proxy.port || 8045
                        }
                    };
                    saveConfig(newConfig);
                }
            } else {
                const status = await invoke<typeof cfStatus>('cloudflared_stop');
                setCfStatus(status);
                showToast(t('proxy.cloudflared.stopped', { defaultValue: 'Tunnel stopped' }), 'success');

                // 持久化“禁用”状态
                if (appConfig) {
                    const newConfig = {
                        ...appConfig,
                        cloudflared: {
                            ...appConfig.cloudflared,
                            enabled: false
                        }
                    };
                    saveConfig(newConfig);
                }
            }
        } catch (error) {
            showToast(String(error), 'error');
        } finally {
            setCfLoading(false);
        }
    };

    // Cloudflared: 复制URL
    const handleCfCopyUrl = async () => {
        if (cfStatus.url) {
            const success = await copyToClipboard(cfStatus.url);
            if (success) {
                setCopied('cf-url');
                setTimeout(() => setCopied(null), 2000);
            }
        }
    };

    // [FIX #820] Load current preferred account
    const loadPreferredAccount = async () => {
        try {
            const prefId = await invoke<string | null>('get_preferred_account');
            setPreferredAccountId(prefId);
        } catch (error) {
            // Service not running, ignore
        }
    };

    // [FIX #820] Set preferred account
    const handleSetPreferredAccount = async (accountId: string | null) => {
        try {
            const wasEnabled = preferredAccountId !== null;
            await invoke('set_preferred_account', { accountId });
            setPreferredAccountId(accountId);

            // Determine appropriate message
            let message: string;
            if (accountId === null) {
                message = t('proxy.config.scheduling.round_robin_set', { defaultValue: 'Round-robin mode enabled' });
            } else if (wasEnabled) {
                // Changed account while already in fixed mode
                const account = availableAccounts.find(a => a.id === accountId);
                message = t('proxy.config.scheduling.account_changed', {
                    defaultValue: `Switched to ${account?.email || accountId}`,
                    email: account?.email || accountId
                });
            } else {
                // Just enabled fixed mode
                message = t('proxy.config.scheduling.fixed_account_set', { defaultValue: 'Fixed account mode enabled' });
            }

            showToast(message, 'success');
        } catch (error) {
            showToast(String(error), 'error');
        }
    };

    const loadConfig = async () => {
        setConfigLoading(true);
        setConfigError(null);
        try {
            const config = await invoke<AppConfig>('load_config');
            if (config && typeof config === 'object' && config.proxy) {
                setAppConfig(config);

                // 恢复 Cloudflared 持久化状态
                if (config.cloudflared) {
                    setCfMode(config.cloudflared.mode || 'quick');
                    setCfToken(config.cloudflared.token || '');
                    setCfUseHttp2(config.cloudflared.use_http2 !== false); // 默认开启 HTTP/2
                }
            } else {
                console.warn('load_config 返回的配置无效或非对象:', config);
            }
        } catch (error) {
            console.error('加载配置失败:', error);
            setConfigError(String(error));
        } finally {
            setConfigLoading(false);
        }
    };

    const loadStatus = async () => {
        try {
            const s = await invoke<ProxyStatus>('get_proxy_status');
            // 如果后端返回 starting 或 busy，则在 UI 上表现为加载中
            if (s.base_url === 'starting' || s.base_url === 'busy') {
                // 如果当前已经是运行状态，不要被覆盖为 false
                setStatus(prev => ({ ...s, running: prev.running }));
            } else {
                setStatus(s);
            }
        } catch (error) {
            console.error('获取状态失败:', error);
        }
    };


    const saveConfig = async (newConfig: AppConfig) => {
        // 1. 立即更新 UI 状态，确保流畅
        setAppConfig(newConfig);
        try {
            await invoke('save_config', { config: newConfig });
        } catch (error) {
            console.error('保存配置失败:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleSaveProxySettings = async () => {
        if (!appConfig) return;
        try {
            await invoke('save_config', { config: appConfig });
            const refreshed = await invoke<AppConfig>('load_config');
            if (refreshed) {
                setAppConfig(refreshed);
            }
            showToast(t('common.saved'), 'success');
        } catch (error) {
            console.error('保存配置失败:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleRunUaSandboxTest = async (overrideUa?: string, modelToTest?: string) => {
        const targetModel = modelToTest || uaSandboxModel;
        const effectiveUa = overrideUa !== undefined ? overrideUa : (appConfig?.proxy.user_agent_override || latestHubUa);
        setUaSandboxTesting(true);
        setUaSandboxResult(null);

        // 1. 如果传入了特定 UA，先热更新到配置并同步至服务端
        if (appConfig && overrideUa !== undefined) {
            updateProxyConfig({
                user_agent_override: overrideUa,
                saved_user_agent: overrideUa
            });
            try {
                await invoke('save_config', {
                    config: {
                        ...appConfig,
                        proxy: {
                            ...appConfig.proxy,
                            user_agent_override: overrideUa,
                            saved_user_agent: overrideUa
                        }
                    }
                });
            } catch (e) {
                console.error('Failed to sync UA config before test:', e);
            }
        }

        try {
            const res = await invoke<{
                success: boolean;
                message: string;
                log_id?: string;
                duration_ms?: number;
                status?: number;
                error?: string;
                error_type?: string;
                gatewayCode?: string;
                gatewayDiagnosis?: string;
                gatewaySuggestion?: string;
            }>('test_model_connectivity', {
                model: targetModel,
                targetAccountId: testTargetAccountId || null,
                override_ua: effectiveUa
            });

            let gatewayDiag = res.gatewayDiagnosis || '';
            let gatewaySug = res.gatewaySuggestion || '';
            let gatewayCode = res.gatewayCode || res.error_type || '';

            if (!gatewayDiag && res.message && res.message.includes('【上游指纹门禁异常】')) {
                gatewayCode = 'outdated_user_agent';
                gatewayDiag = res.message;
                if (res.error && res.error.includes('建议:')) {
                    const parts = res.error.split('建议:');
                    gatewaySug = parts[1]?.trim() || '';
                }
            } else if (!gatewayDiag && res.message && res.message.includes('【模型全池临时熔断】')) {
                gatewayCode = 'model_pool_suspended';
                gatewayDiag = res.message;
                if (res.error && res.error.includes('建议:')) {
                    const parts = res.error.split('建议:');
                    gatewaySug = parts[1]?.trim() || '';
                }
            } else if (!gatewayDiag && (res.message?.includes('【网络请求异常') || res.error?.includes('网络请求异常') || res.error_type === 'upstream_network_error' || res.status === 502)) {
                gatewayCode = 'upstream_network_error';
                gatewayDiag = res.message || res.error || '请求上游接口时网络连接失败，未收到上游请求回报（非网关服务端故障，请排查网络或代理）。';
                if (res.error && res.error.includes('建议:')) {
                    const parts = res.error.split('建议:');
                    gatewaySug = parts[1]?.trim() || '';
                } else {
                    gatewaySug = '请检查系统代理、上游节点连通性或本地网络设置，确保能正常连接上游服务，并在「超时设置」中调整分阶段传输预算。';
                }
            }

            setUaSandboxResult({
                testedUa: effectiveUa,
                model: targetModel,
                status: res.status,
                durationMs: res.duration_ms,
                success: res.success,
                message: res.message,
                error: res.error,
                logId: res.log_id,
                gatewayDiagnosis: gatewayDiag,
                gatewaySuggestion: gatewaySug,
                gatewayCode
            });
        } catch (err) {
            const errStr = String(err);
            setUaSandboxResult({
                testedUa: effectiveUa,
                model: targetModel,
                status: 500,
                success: false,
                error: errStr,
                message: `请求发起失败: ${errStr}`
            });
        } finally {
            setUaSandboxTesting(false);
        }
    };

    // 专门处理模型映射的热更新 (全量)
    const handleMappingUpdate = async (type: 'custom', key: string, value: string, expose?: boolean) => {
        if (!appConfig) return;

        const trimmedKey = key.trim();
        console.log('[DEBUG] handleMappingUpdate called:', { type, key: trimmedKey, value });

        const newConfig = { ...appConfig.proxy };
        newConfig.custom_mapping = { ...(newConfig.custom_mapping || {}), [trimmedKey]: value };

        const newExpose = { ...(newConfig.custom_mapping_expose || {}) };
        if (expose !== undefined) {
            newExpose[trimmedKey] = expose;
        } else if (!(trimmedKey in newExpose)) {
            // 新增映射默认：通配符或 auto: 默认不公开 (false)，常规精确映射默认公开 (true)
            const isAuto = value.startsWith('auto:');
            const defaultExp = !trimmedKey.includes('*') && !isAuto;
            newExpose[trimmedKey] = defaultExp;
        }
        newConfig.custom_mapping_expose = newExpose;

        try {
            await invoke('update_model_mapping', { config: newConfig });
            setAppConfig({ ...appConfig, proxy: newConfig });
            console.log('[DEBUG] Mapping updated successfully');
            showToast(t('common.saved'), 'success');
        } catch (error) {
            console.error('Failed to update mapping:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleToggleExpose = async (key: string) => {
        if (!appConfig || !appConfig.proxy.custom_mapping) return;
        const currentExpose = appConfig.proxy.custom_mapping_expose?.[key];
        const val = appConfig.proxy.custom_mapping[key] || '';
        const isAuto = val.startsWith('auto:');
        const effectiveCurrent = currentExpose !== undefined ? currentExpose : (!key.includes('*') && !isAuto);
        const newExposeVal = !effectiveCurrent;

        const newConfig = {
            ...appConfig.proxy,
            custom_mapping_expose: {
                ...(appConfig.proxy.custom_mapping_expose || {}),
                [key]: newExposeVal
            }
        };

        try {
            await invoke('update_model_mapping', { config: newConfig });
            setAppConfig({ ...appConfig, proxy: newConfig });
            showToast(t('common.saved'), 'success');
        } catch (error) {
            console.error('Failed to toggle expose:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleResetMapping = () => {
        if (!appConfig) return;
        setIsResetConfirmOpen(true);
    };

    const executeResetMapping = async () => {
        if (!appConfig) return;
        setIsResetConfirmOpen(false);

        // 恢复到官方自适应暗路由默认预设 (防复活且保留内置 3 条自适应规则)
        const defaultCustomMapping: Record<string, string> = {
            'claude-fable-*': 'auto:claude-opus',
            'claude-opus-4-7*': 'auto:claude-opus-4-7',
            'claude-haiku-*': 'auto:gemini-flash-low',
        };
        const defaultCustomMappingExpose: Record<string, boolean> = {
            'claude-fable-*': false,
            'claude-opus-4-7*': false,
            'claude-haiku-*': false,
        };

        const newConfig = {
            ...appConfig.proxy,
            custom_mapping: defaultCustomMapping,
            custom_mapping_expose: defaultCustomMappingExpose,
            builtin_mapping_version: 1,
        };

        try {
            await invoke('update_model_mapping', { config: newConfig });
            setAppConfig({ ...appConfig, proxy: newConfig });
            showToast(t('common.success'), 'success');
        } catch (error) {
            console.error('Failed to reset mapping:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };


    // 定义多个预设方案
    const defaultPresets = useMemo(() => [
        {
            id: 'default',
            name: t('proxy.router.preset_default'),
            description: t('proxy.router.preset_default_desc'),
            mappings: {
                "gpt-4*": "gemini-3.1-pro-high",
                "gpt-4o*": "gemini-3.8-flash-high",
                "gpt-3.5*": "gemini-3.8-flash-medium",
                "o1-*": "gemini-3.1-pro-high",
                "o3-*": "gemini-3.1-pro-high",
                "claude-3-5-sonnet-*": "claude-sonnet-4-6",
                "claude-3-opus-*": "claude-opus-4-6-thinking",
                "claude-opus-4-6*": "claude-opus-4-6-thinking",
                "claude-haiku-*": "gemini-3.8-flash-medium",
                "claude-3-haiku-*": "gemini-3.8-flash-medium",
            }
        },
        {
            id: 'performance',
            name: t('proxy.router.preset_performance'),
            description: t('proxy.router.preset_performance_desc'),
            mappings: {
                "gpt-4*": "claude-opus-4-6-thinking",
                "gpt-4o*": "claude-sonnet-4-6",
                "gpt-3.5*": "gemini-3.8-flash-tiered",
                "o1-*": "claude-opus-4-6-thinking",
                "o3-*": "claude-opus-4-6-thinking",
                "claude-3-5-sonnet-*": "claude-sonnet-4-6",
                "claude-3-opus-*": "claude-opus-4-6-thinking",
                "claude-opus-4-6*": "claude-opus-4-6-thinking",
                "claude-haiku-*": "claude-sonnet-4-6",
                "claude-3-haiku-*": "claude-sonnet-4-6",
            }
        },
        {
            id: 'cost-effective',
            name: t('proxy.router.preset_cost'),
            description: t('proxy.router.preset_cost_desc'),
            mappings: {
                "gpt-4*": "gemini-3.8-flash-high",
                "gpt-4o*": "gemini-3.8-flash-medium",
                "gpt-3.5*": "gemini-3.1-flash-lite",
                "o1-*": "gemini-3.8-flash-high",
                "o3-*": "gemini-3.8-flash-high",
                "claude-3-5-sonnet-*": "gemini-3.8-flash-high",
                "claude-3-opus-*": "gemini-3.8-flash-high",
                "claude-opus-4-*": "gemini-3.8-flash-high",
                "claude-haiku-*": "gemini-3.1-flash-lite",
                "claude-3-haiku-*": "gemini-3.1-flash-lite",
            }
        },
        {
            id: 'balanced',
            name: t('proxy.router.preset_balanced'),
            description: t('proxy.router.preset_balanced_desc'),
            mappings: {
                "gpt-4*": "gemini-3.1-pro-high",
                "gpt-4o*": "gemini-3.8-flash-high",
                "gpt-3.5*": "gemini-3.8-flash-medium",
                "o1-*": "claude-sonnet-4-6",
                "o3-*": "claude-sonnet-4-6",
                "claude-3-5-sonnet-*": "claude-sonnet-4-6",
                "claude-3-opus-*": "gemini-3.1-pro-high",
                "claude-opus-4-5*": "gemini-3.1-pro-high",
                "claude-opus-4-6*": "claude-opus-4-6-thinking",
                "claude-haiku-*": "gemini-3.8-flash-medium",
                "claude-3-haiku-*": "gemini-3.8-flash-medium",
            }
        },
    ], [t]);

    const presetOptions = useMemo(() => {
        return [...defaultPresets, ...customPresets];
    }, [defaultPresets, customPresets]);

    // Custom Presets Logic
    const loadCustomPresets = () => {
        try {
            const saved = localStorage.getItem('antigravity_custom_presets');
            if (saved) {
                setCustomPresets(JSON.parse(saved));
            }
        } catch (error) {
            console.error('Failed to load custom presets:', error);
        }
    };

    const saveCustomPresetsToStorage = (presets: CustomPreset[]) => {
        try {
            localStorage.setItem('antigravity_custom_presets', JSON.stringify(presets));
            setCustomPresets(presets);
        } catch (error) {
            console.error('Failed to save custom presets:', error);
            showToast('Failed to save preset', 'error');
        }
    };

    const handleSaveCurrentAsPreset = () => {
        if (!appConfig?.proxy.custom_mapping || Object.keys(appConfig.proxy.custom_mapping).length === 0) {
            showToast(t('proxy.router.no_mapping_to_save'), 'warning');
            return;
        }
        if (!newPresetName.trim()) {
            showToast(t('proxy.router.preset_name_required'), 'warning');
            return;
        }

        const newPreset: CustomPreset = {
            id: `custom_${Date.now()}`,
            name: newPresetName,
            description: t('proxy.router.custom_preset_desc'),
            mappings: { ...appConfig.proxy.custom_mapping }
        };

        const updatedPresets = [...customPresets, newPreset];
        saveCustomPresetsToStorage(updatedPresets);
        setNewPresetName('');
        setIsPresetManagerOpen(false);
        showToast(t('proxy.router.preset_saved', { defaultValue: 'Preset saved successfully' }), 'success');
        // Auto select the new preset
        setSelectedPreset(newPreset.id);
    };

    const handleDeletePreset = (id: string) => {
        const updatedPresets = customPresets.filter(p => p.id !== id);
        saveCustomPresetsToStorage(updatedPresets);
        if (selectedPreset === id) {
            setSelectedPreset('default');
        }
    };

    // 应用预设映射 (通配符)
    const handleApplyPresets = async () => {
        if (!appConfig) return;

        const selectedPresetData = presetOptions.find(p => p.id === selectedPreset);
        if (!selectedPresetData) return;

        // 构造新配置
        const newConfig = {
            ...appConfig.proxy,
            // 策略:覆盖同名 key,保留其他自定义 key
            // [FIX #1738] Type assertion to ensure Record<string, string> compatibility
            custom_mapping: { ...appConfig.proxy.custom_mapping, ...selectedPresetData.mappings } as Record<string, string>
        };

        // 备份旧配置用于回滚
        const oldConfig = { ...appConfig };

        try {
            // 1. 乐观更新：立即更新 UI
            setAppConfig({ ...appConfig, proxy: newConfig });
            showToast(t('proxy.router.presets_applied') + ` (${selectedPresetData.name})`, 'success');

            // 2. 后台异步保存
            await invoke('update_model_mapping', { config: newConfig });

            // 3. 重新加载配置以确保一致性
            await loadConfig();
        } catch (error) {
            console.error('Failed to apply presets:', error);
            // 3. 失败回滚
            setAppConfig(oldConfig);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleRemoveCustomMapping = async (key: string) => {
        if (!appConfig || !appConfig.proxy.custom_mapping) return;
        const newCustom = { ...appConfig.proxy.custom_mapping };
        delete newCustom[key];
        const newExpose = { ...(appConfig.proxy.custom_mapping_expose || {}) };
        delete newExpose[key];
        const newConfig = { ...appConfig.proxy, custom_mapping: newCustom, custom_mapping_expose: newExpose };
        try {
            await invoke('update_model_mapping', { config: newConfig });
            setAppConfig({ ...appConfig, proxy: newConfig });
        } catch (error) {
            console.error('Failed to remove custom mapping:', error);
        }
    };

    const updateProxyConfig = (updates: Partial<ProxyConfig>) => {
        if (!appConfig) return;
        const newConfig = {
            ...appConfig,
            proxy: {
                ...appConfig.proxy,
                ...updates
            }
        };
        saveConfig(newConfig);
    };

    const updateSchedulingConfig = (updates: Partial<StickySessionConfig>) => {
        if (!appConfig) return;
        const currentScheduling = appConfig.proxy.scheduling || { mode: 'Balance', max_wait_seconds: 60 };
        const newScheduling = { ...currentScheduling, ...updates };

        const newAppConfig = {
            ...appConfig,
            proxy: {
                ...appConfig.proxy,
                scheduling: newScheduling
            }
        };
        saveConfig(newAppConfig);
    };

    const updateExperimentalConfig = (updates: Partial<ExperimentalConfig>) => {
        if (!appConfig) return;
        const newConfig = {
            ...appConfig,
            proxy: {
                ...appConfig.proxy,
                experimental: {
                    ...(appConfig.proxy.experimental || {
                        enable_usage_scaling: false,
                    }),
                    ...updates
                }
            }
        };
        saveConfig(newConfig);
    };

    const updateCircuitBreakerConfig = (newBreakerConfig: CircuitBreakerConfig) => {
        if (!appConfig) return;
        const newConfig = {
            ...appConfig,
            circuit_breaker: newBreakerConfig
        };
        saveConfig(newConfig);
    };

    const handleClearSessionBindings = () => {
        setIsClearBindingsConfirmOpen(true);
    };

    const executeClearSessionBindings = async () => {
        setIsClearBindingsConfirmOpen(false);
        try {
            await invoke('clear_proxy_session_bindings');
            showToast(t('common.success'), 'success');
        } catch (error) {
            console.error('Failed to clear session bindings:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleClearRateLimits = () => {
        setIsClearRateLimitsConfirmOpen(true);
    };

    const executeClearRateLimits = async () => {
        setIsClearRateLimitsConfirmOpen(false);
        try {
            await invoke('clear_all_proxy_rate_limits');
            showToast(t('common.success'), 'success');
        } catch (error) {
            console.error('Failed to clear rate limits:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleToggle = async () => {
        if (!appConfig) return;
        setLoading(true);
        try {
            if (status.running) {
                await invoke('stop_proxy_service');
                const newConfig = {
                    ...appConfig,
                    proxy: {
                        ...appConfig.proxy,
                        auto_start: false
                    }
                };
                setAppConfig(newConfig);
                await invoke('save_config', { config: newConfig });
            } else {
                const newConfig = {
                    ...appConfig,
                    proxy: {
                        ...appConfig.proxy,
                        auto_start: true
                    }
                };
                setAppConfig(newConfig);
                await invoke('save_config', { config: newConfig });
                // 使用当前的 appConfig.proxy 启动
                await invoke('start_proxy_service', { config: newConfig.proxy });
            }
            await loadStatus();
        } catch (error: any) {
            showToast(t('proxy.dialog.operate_failed', { error: error.toString() }), 'error');
        } finally {
            setLoading(false);
        }
    };

    const handleGenerateApiKey = () => {
        setIsRegenerateKeyConfirmOpen(true);
    };

    const executeGenerateApiKey = async () => {
        setIsRegenerateKeyConfirmOpen(false);
        try {
            const newKey = await invoke<string>('generate_api_key');
            updateProxyConfig({ api_key: newKey });
            showToast(t('common.success'), 'success');
        } catch (error: any) {
            console.error('生成 API Key 失败:', error);
            showToast(t('proxy.dialog.operate_failed', { error: error.toString() }), 'error');
        }
    };

    const copyToClipboardHandler = (text: string, label: string) => {
        copyToClipboard(text).then((success) => {
            if (success) {
                setCopied(label);
                setTimeout(() => setCopied(null), 2000);
            }
        });
    };

    // API Key editing functions
    const validateApiKey = (key: string): boolean => {
        // Must start with 'sk-' and be at least 10 characters long
        return key.startsWith('sk-') && key.length >= 10;
    };

    const handleEditApiKey = () => {
        setTempApiKey(appConfig?.proxy.api_key || '');
        setIsEditingApiKey(true);
    };

    const handleSaveApiKey = () => {
        if (!validateApiKey(tempApiKey)) {
            showToast(t('proxy.config.api_key_invalid'), 'error');
            return;
        }
        updateProxyConfig({ api_key: tempApiKey });
        setIsEditingApiKey(false);
        showToast(t('proxy.config.api_key_updated'), 'success');
    };

    const handleCancelEditApiKey = () => {
        setTempApiKey('');
        setIsEditingApiKey(false);
    };

    // Admin Password editing functions
    const handleEditAdminPassword = () => {
        setTempAdminPassword(appConfig?.proxy.admin_password || '');
        setIsEditingAdminPassword(true);
    };

    const handleSaveAdminPassword = () => {
        // Validation: can be empty (meaning fallback to api_key) or at least 4 chars
        if (tempAdminPassword && tempAdminPassword.length < 4) {
            showToast(t('proxy.config.admin_password_short', { defaultValue: 'Password is too short (min 4 chars)' }), 'error');
            return;
        }
        updateProxyConfig({ admin_password: tempAdminPassword || undefined });
        setIsEditingAdminPassword(false);
        showToast(t('proxy.config.admin_password_updated', { defaultValue: 'Web UI password updated' }), 'success');
    };

    const handleCancelEditAdminPassword = () => {
        setTempAdminPassword('');
        setIsEditingAdminPassword(false);
    };


    const getPythonExample = (modelId: string) => {
        const port = status.running ? status.port : (appConfig?.proxy.port || 8045);
        // 推荐使用 127.0.0.1 以避免部分环境 IPv6 解析延迟问题
        const baseUrl = `http://127.0.0.1:${port}/v1`;
        const apiKey = appConfig?.proxy.api_key || 'YOUR_API_KEY';

        // 1. Anthropic Protocol
        if (selectedProtocol === 'anthropic') {
            return `from anthropic import Anthropic

client = Anthropic(
    # Recommended: use 127.0.0.1
    base_url="${`http://127.0.0.1:${port}`}",
    api_key="${apiKey}"
)

# Note: Antigravity lets you call any model via the Anthropic SDK
response = client.messages.create(
    model="${modelId}",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Hello"}]
)

print(response.content[0].text)`;
        }

        // 2. Gemini Protocol (Native)
        if (selectedProtocol === 'gemini') {
            const rawBaseUrl = `http://127.0.0.1:${port}`;
            return `# Requires: pip install google-generativeai
import google.generativeai as genai

# Use the Antigravity proxy address (recommended: 127.0.0.1)
genai.configure(
    api_key="${apiKey}",
    transport='rest',
    client_options={'api_endpoint': '${rawBaseUrl}'}
)

model = genai.GenerativeModel('${modelId}')
response = model.generate_content("Hello")
print(response.text)`;
        }

        // 3. OpenAI Protocol — image generation models (any *-image model)
        if (modelId.toLowerCase().includes('-image')) {
            return `from openai import OpenAI

client = OpenAI(
    base_url="${baseUrl}",
    api_key="${apiKey}"
)

# IMPORTANT — model availability:
#   "${modelId}" must be an image model your selected account actually has.
#   Check the model list on the left: not every account exposes every image model
#   (e.g. some accounts only have gemini-3.1-flash-image, not gemini-3-pro-image).
#   Requesting a model the account lacks fails with:
#     404 "Requested entity was not found"
#   To keep using a different name, add an explicit mapping in the Model Routing Center.

response = client.chat.completions.create(
    model="${modelId}",

    # Aspect ratio — Option 1: the size parameter (recommended)
    #   "1024x1024" = 1:1   |   "1280x720" = 16:9
    #   "720x1280"  = 9:16  |   "1216x896" = 4:3
    extra_body={ "size": "1024x1024" },

    # Aspect ratio — Option 2: a model-name suffix instead of size
    #   model="${modelId}-16-9"   (also: -9-16, -4-3, -3-4)
    messages=[{
        "role": "user",
        "content": "Draw a futuristic city"
    }]
)

# The generated image is returned INSIDE the message content
# (as a base64 data URL / markdown image), not as a hosted URL.
# Extract the base64 payload from here to save the file.
print(response.choices[0].message.content)`;
        }

        return `from openai import OpenAI

client = OpenAI(
    base_url="${baseUrl}",
    api_key="${apiKey}"
)

response = client.chat.completions.create(
    model="${modelId}",
    messages=[{"role": "user", "content": "Hello"}]
)

print(response.choices[0].message.content)`;
    };

    // 在 filter 逻辑中，当选择 openai 协议时，允许显示所有模型
    const filteredModels = models.filter(model => {
        if (selectedProtocol === 'openai') {
            return true;
        }
        // Anthropic 协议下隐藏不支持的图片模型
        if (selectedProtocol === 'anthropic') {
            return !model.id.includes('image');
        }
        return true;
    });

    const renderModelRouterSection = () => {
        if (!appConfig) return null;
        return (
            <div className="bg-white dark:bg-base-100 rounded-xl shadow-xs border border-gray-200/80 dark:border-base-200 overflow-hidden">
                <div className="px-4 py-3 border-b border-gray-100 dark:border-gray-700/50 bg-gray-50/50 dark:bg-gray-800/50">
                    <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
                        <div className="flex-1">
                            <h2 className="text-base font-bold flex items-center gap-2 text-gray-900 dark:text-base-content">
                                <BrainCircuit size={18} className="text-blue-500" />
                                {t('proxy.router.title')}
                            </h2>
                            <p className="text-xs text-gray-500 dark:text-gray-400 mt-1 max-w-xl leading-relaxed">
                                {t('proxy.router.subtitle_simple')}
                            </p>
                        </div>
                        <div className="flex flex-wrap items-center gap-2.5 bg-white dark:bg-base-100 p-1.5 rounded-xl border border-gray-100 dark:border-gray-700/50 shadow-sm">
                            {/* 仅暴露真实配额模型开关 */}
                            <label
                                className="flex items-center gap-2 px-3 py-1 rounded-lg border border-gray-200 dark:border-gray-700 bg-gray-50/80 dark:bg-base-200 cursor-pointer hover:bg-gray-100 dark:hover:bg-base-300 transition-colors h-9 select-none"
                                title={t('proxy.router.only_raw_quota_models_tooltip')}
                            >
                                <span className="text-xs font-medium text-gray-700 dark:text-gray-300 whitespace-nowrap">
                                    {t('proxy.router.only_raw_quota_models')}
                                </span>
                                <input
                                    type="checkbox"
                                    className="toggle toggle-sm bg-gray-300 dark:bg-gray-700 border-gray-400 dark:border-gray-600 checked:bg-blue-600 checked:border-blue-600 cursor-pointer"
                                    checked={appConfig.proxy.only_raw_quota_models ?? false}
                                    onChange={(e) => updateProxyConfig({ only_raw_quota_models: e.target.checked })}
                                />
                            </label>

                            {/* 预设选择下拉框 */}
                            <div className="relative min-w-[140px]">
                                <select
                                    value={selectedPreset}
                                    onChange={(e) => setSelectedPreset(e.target.value)}
                                    className="select select-sm w-full bg-gray-50 dark:bg-base-200 border-gray-200 dark:border-gray-700 text-xs font-medium focus:ring-1 focus:ring-blue-500 h-9 min-h-0 rounded-lg"
                                >
                                    <optgroup label={t('proxy.router.built_in_presets')}>
                                        {defaultPresets.map(preset => (
                                            <option key={preset.id} value={preset.id}>
                                                {preset.name}
                                            </option>
                                        ))}
                                    </optgroup>
                                    {customPresets.length > 0 && (
                                        <optgroup label={t('proxy.router.custom_presets')}>
                                            {customPresets.map(preset => (
                                                <option key={preset.id} value={preset.id}>
                                                    {preset.name}
                                                </option>
                                            ))}
                                        </optgroup>
                                    )}
                                </select>
                            </div>

                            <button
                                onClick={handleApplyPresets}
                                className="btn btn-sm btn-primary h-9 min-h-0 gap-1.5 shadow-sm text-xs"
                            >
                                <Check size={14} />
                                {t('proxy.router.apply_selected')}
                            </button>

                            <button
                                onClick={() => {
                                    setNewPresetName('');
                                    setIsPresetManagerOpen(true);
                                }}
                                className="btn btn-sm btn-outline border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800 text-gray-700 dark:text-gray-300 h-9 min-h-0 text-xs gap-1.5"
                                title={t('proxy.router.add_preset')}
                            >
                                <Plus size={14} />
                            </button>

                            {selectedPreset && !['default', 'performance', 'cost-effective', 'balanced'].includes(selectedPreset) ? (
                                <button
                                    onClick={() => handleDeletePreset(selectedPreset)}
                                    className="btn btn-sm btn-ghost text-error hover:bg-red-50 dark:hover:bg-red-900/20 h-9 min-h-0 p-2 text-xs"
                                    title={t('proxy.router.delete_preset')}
                                >
                                    <Trash2 size={14} />
                                </button>
                            ) : null}

                            <button
                                onClick={handleResetMapping}
                                className="btn btn-sm btn-ghost text-gray-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 h-9 min-h-0 p-2 text-xs"
                                title={t('proxy.router.reset_mapping')}
                            >
                                <RefreshCw size={14} />
                            </button>
                        </div>
                    </div>
                </div>

                <div className="p-4 space-y-4">
                    {/* Background Task Model Mapping */}
                    <div className="bg-gray-50/50 dark:bg-white/5 p-3 rounded-xl border border-gray-100 dark:border-white/5">
                        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                            <div>
                                <h3 className="text-xs font-bold text-gray-900 dark:text-white flex items-center gap-1.5">
                                    <Sparkles size={14} className="text-blue-500" />
                                    {t('proxy.router.background_task_title')}
                                </h3>
                                <p className="text-[11px] text-gray-500 dark:text-gray-400 mt-0.5">
                                    {t('proxy.router.background_task_desc')}
                                </p>
                            </div>
                            <div className="flex items-center gap-2">
                                <div className="w-48 sm:w-60">
                                    <GroupedSelect
                                        value={appConfig.proxy.custom_mapping?.['internal-background-task'] || ''}
                                        onChange={(val) => handleMappingUpdate('custom', 'internal-background-task', val)}
                                        options={customMappingOptions}
                                        placeholder={t('proxy.router.select_target_model') || 'Select Target Model'}
                                        className="font-mono text-xs h-8 dark:bg-gray-800"
                                        allowCustomInput={true}
                                    />
                                </div>
                                {appConfig.proxy.custom_mapping && appConfig.proxy.custom_mapping['internal-background-task'] && (
                                    <button
                                        onClick={() => handleRemoveCustomMapping('internal-background-task')}
                                        className="p-1.5 text-gray-400 hover:text-blue-500 hover:bg-blue-50 dark:hover:bg-blue-900/30 rounded transition-colors"
                                        title={t('proxy.router.use_default')}
                                    >
                                        <RefreshCw size={12} />
                                    </button>
                                )}
                            </div>
                        </div>
                    </div>

                    <div className="flex items-center justify-between mb-3">
                        <div className="flex flex-col gap-1">
                            <h3 className="text-[10px] font-bold text-gray-400 uppercase tracking-widest flex items-center gap-2">
                                <ArrowRight size={14} /> {t('proxy.router.custom_mappings')}
                            </h3>
                            <p className="text-[9px] text-gray-500 dark:text-gray-400 leading-relaxed">
                                {t('proxy.router.custom_mapping_tip')}
                                <span className="text-amber-600 dark:text-amber-400">{t('proxy.router.custom_mapping_warning')}</span>
                            </p>
                        </div>
                    </div>
                    <div className="flex flex-col gap-4">
                        {/* 当前映射列表 (置顶 2 列) */}
                        <div className="w-full flex flex-col">
                            <div className="flex items-center justify-between mb-2">
                                <span className="text-[10px] font-bold text-gray-400 dark:text-gray-500 uppercase tracking-wider">
                                    {t('proxy.router.current_list')}
                                </span>
                            </div>
                            <div className="overflow-y-auto max-h-[180px] border border-gray-100 dark:border-white/5 rounded-lg bg-gray-50/10 dark:bg-white/5 p-3" data-custom-mapping-list>
                                <div className="grid grid-cols-1 md:grid-cols-2 gap-x-6 gap-y-2">
                                    {appConfig.proxy.custom_mapping && Object.entries(appConfig.proxy.custom_mapping).length > 0 ? (
                                        Object.entries(appConfig.proxy.custom_mapping).map(([key, val]) => (
                                            <div key={key} className={`flex items-center justify-between p-1.5 rounded-md transition-all border group ${editingKey === key ? 'bg-blue-50/80 dark:bg-blue-900/15 border-blue-300/50 dark:border-blue-500/30 shadow-sm' : 'border-transparent hover:bg-gray-100 dark:hover:bg-white/5 hover:border-gray-200 dark:hover:border-white/10'}`}>
                                                <div className="flex items-center gap-2 overflow-hidden flex-1">
                                                    <div className="flex items-center gap-1.5 min-w-0">
                                                        <span className="font-mono text-[10px] font-bold text-blue-600 dark:text-blue-400 truncate max-w-[140px]" title={key}>{key}</span>
                                                    </div>
                                                    <ArrowRight size={10} className="text-gray-300 dark:text-gray-600 shrink-0" />

                                                    {editingKey === key ? (
                                                        <div className="flex-1 mr-2">
                                                            <GroupedSelect
                                                                value={editingValue}
                                                                onChange={setEditingValue}
                                                                options={customMappingOptions}
                                                                placeholder="Select..."
                                                                className="font-mono text-[10px] h-7 dark:bg-gray-800 border-blue-200 dark:border-blue-800"
                                                                allowCustomInput={true}
                                                            />
                                                        </div>
                                                    ) : (
                                                        <span className="font-mono text-[10px] text-gray-500 dark:text-gray-400 truncate cursor-pointer hover:text-blue-500"
                                                            onClick={() => { setEditingKey(key); setEditingValue(val); }}
                                                            title={val}>{val}</span>
                                                    )}
                                                </div>

                                                <div className="flex items-center gap-1.5 shrink-0">
                                                    {(() => {
                                                        const isAuto = val.startsWith('auto:');
                                                        const isExp = appConfig.proxy.custom_mapping_expose?.[key] ?? (!key.includes('*') && !isAuto);
                                                        return (
                                                            <button
                                                                type="button"
                                                                className={`px-1.5 py-0.5 rounded text-[9px] font-mono font-medium flex items-center gap-1 transition-colors border ${
                                                                    isExp
                                                                        ? 'bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 border-emerald-200 dark:border-emerald-800/60 hover:bg-emerald-100 dark:hover:bg-emerald-900/40'
                                                                        : 'bg-gray-100 dark:bg-white/5 text-gray-400 dark:text-gray-500 border-gray-200 dark:border-white/10 hover:bg-gray-200 dark:hover:bg-white/10'
                                                                }`}
                                                                onClick={() => handleToggleExpose(key)}
                                                                title={isExp ? (t('proxy.router.expose_public_tip') || '对外公开') : (t('proxy.router.expose_hidden_tip') || '暗访')}
                                                            >
                                                                {isExp ? <Eye size={10} /> : <EyeOff size={10} />}
                                                                <span>{isExp ? (t('proxy.router.expose_public') || '公开') : (t('proxy.router.expose_hidden') || '暗访')}</span>
                                                            </button>
                                                        );
                                                    })()}

                                                    {editingKey === key ? (
                                                        <div className="flex items-center gap-1 bg-white dark:bg-gray-800 rounded-md border border-blue-200 dark:border-blue-800 p-0.5 shadow-sm">
                                                            <button
                                                                className="btn btn-ghost btn-xs text-primary hover:bg-blue-50 dark:hover:bg-blue-900/30 p-0 h-6 w-6 min-h-0"
                                                                onClick={() => {
                                                                    handleMappingUpdate('custom', key, editingValue);
                                                                    setEditingKey(null);
                                                                }}
                                                                title={t('common.save') || 'Save'}
                                                            >
                                                                <Check size={14} strokeWidth={3} />
                                                            </button>
                                                            <div className="w-[1px] h-3 bg-gray-200 dark:bg-gray-700" />
                                                            <button
                                                                className="btn btn-ghost btn-xs text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700 p-0 h-6 w-6 min-h-0"
                                                                onClick={() => setEditingKey(null)}
                                                                title={t('common.cancel') || 'Cancel'}
                                                            >
                                                                <X size={14} strokeWidth={3} />
                                                            </button>
                                                        </div>
                                                    ) : (
                                                        <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                                                            <button
                                                                className="btn btn-ghost btn-xs text-gray-400 hover:text-blue-500 hover:bg-blue-50 dark:hover:bg-white/10 p-0 h-6 w-6 min-h-0"
                                                                onClick={() => { setEditingKey(key); setEditingValue(val); }}
                                                                title={t('common.edit') || 'Edit'}
                                                            >
                                                                <Edit2 size={12} />
                                                            </button>
                                                            <button
                                                                className="btn btn-ghost btn-xs text-error hover:bg-red-50 dark:hover:bg-red-900/20 p-0 h-6 w-6 min-h-0"
                                                                onClick={() => handleRemoveCustomMapping(key)}
                                                                title={t('common.delete') || 'Delete'}
                                                            >
                                                                <Trash2 size={12} />
                                                            </button>
                                                        </div>
                                                    )}
                                                </div>
                                            </div>
                                        ))
                                    ) : (
                                        <div className="col-span-full text-center py-4 text-gray-400 dark:text-gray-600 italic text-[11px]">{t('proxy.router.no_custom_mapping')}</div>
                                    )}
                                </div>
                            </div>
                        </div>

                        {/* 添加映射表单 (置底单行) */}
                        <div className="w-full bg-gray-50/50 dark:bg-white/5 p-2.5 rounded-xl border border-gray-100 dark:border-white/5 shadow-inner">
                            <div className="flex flex-col sm:flex-row items-center gap-3">
                                <div className="flex items-center gap-1.5 shrink-0">
                                    <Target size={14} className="text-gray-400 dark:text-gray-500" />
                                    <span className="text-[10px] font-bold text-gray-400 dark:text-gray-500 uppercase tracking-wider">{t('proxy.router.add_mapping')}</span>
                                </div>
                                <div className="flex-1 flex flex-col sm:flex-row gap-2 w-full">
                                    <input
                                        id="custom-key"
                                        type="text"
                                        placeholder={t('proxy.router.original_placeholder') || "Original (e.g. gpt-4 or gpt-4*)"}
                                        className="input input-xs input-bordered flex-1 font-mono text-[11px] bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 shadow-sm focus:border-blue-500 focus:ring-1 focus:ring-blue-500 transition-all placeholder:text-gray-400 dark:placeholder:text-gray-600 h-8"
                                    />
                                    <div className="w-full sm:w-48">
                                        <GroupedSelect
                                            value={customMappingValue}
                                            onChange={setCustomMappingValue}
                                            options={customMappingOptions}
                                            placeholder={t('proxy.router.select_target_model') || 'Select Target Model'}
                                            className="font-mono text-[11px] h-8 dark:bg-gray-800"
                                            allowCustomInput={true}
                                        />
                                    </div>
                                </div>
                                <button
                                    className="btn btn-xs sm:w-20 gap-1.5 shadow-md hover:shadow-lg transition-all bg-blue-600 hover:bg-blue-700 text-white border-none h-8"
                                    onClick={() => {
                                        const k = (document.getElementById('custom-key') as HTMLInputElement).value;
                                        const v = customMappingValue;
                                        if (k && v) {
                                            handleMappingUpdate('custom', k, v);
                                            (document.getElementById('custom-key') as HTMLInputElement).value = '';
                                            setCustomMappingValue(''); // 清空选择
                                        }
                                    }}
                                >
                                    <Plus size={14} />
                                    {t('common.add')}
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        );
    };

    const renderMultiProtocolSection = () => {
        if (!appConfig) return null;
        return (
            <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200 overflow-hidden">
                <div className="p-3">
                    <div className="flex items-center gap-3 mb-3">
                        <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-blue-500 to-purple-600 flex items-center justify-center shadow-md">
                            <Code size={16} className="text-white" />
                        </div>
                        <div>
                            <h3 className="text-base font-bold text-gray-900 dark:text-base-content">
                                🔗 {t('proxy.multi_protocol.title')}
                            </h3>
                            <p className="text-[10px] text-gray-500 dark:text-gray-400">
                                {t('proxy.multi_protocol.subtitle')}
                            </p>
                        </div>
                    </div>

                    <p className="text-xs text-gray-700 dark:text-gray-300 mb-4 leading-relaxed">
                        {t('proxy.multi_protocol.description')}
                    </p>

                    <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
                        {/* OpenAI Card */}
                        <div
                            className={`p-3 rounded-xl border-2 transition-all cursor-pointer ${selectedProtocol === 'openai' ? 'border-blue-500 bg-blue-50/30 dark:bg-blue-900/10' : 'border-gray-100 dark:border-base-200 hover:border-blue-200'}`}
                            onClick={() => startTransition(() => setSelectedProtocol('openai'))}
                        >
                            <div className="flex items-center justify-between mb-2">
                                <span className="text-xs font-bold text-blue-600">{t('proxy.multi_protocol.openai_label')}</span>
                                <button onClick={(e) => {
                                    e.stopPropagation();
                                    const baseUrl = status.running ? status.base_url : `http://127.0.0.1:${appConfig.proxy.port || 8045}`;
                                    copyToClipboardHandler(`${baseUrl}/v1`, 'openai');
                                }} className="btn btn-ghost btn-xs">
                                    {copied === 'openai' ? <CheckCircle size={14} /> : <div className="flex items-center gap-1 text-[10px] uppercase font-bold tracking-tighter"><Copy size={12} /> {t('proxy.multi_protocol.copy_base', { defaultValue: 'Base' })}</div>}
                                </button>
                            </div>
                            <div className="space-y-1">
                                <div className="flex items-center justify-between hover:bg-black/5 dark:hover:bg-white/5 rounded p-0.5 group">
                                    <code className="text-[10px] opacity-70">/v1/chat/completions</code>
                                    <button onClick={(e) => {
                                        e.stopPropagation();
                                        const baseUrl = status.running ? status.base_url : `http://127.0.0.1:${appConfig.proxy.port || 8045}`;
                                        copyToClipboardHandler(`${baseUrl}/v1/chat/completions`, 'openai-chat');
                                    }} className="opacity-0 group-hover:opacity-100 transition-opacity">
                                        {copied === 'openai-chat' ? <CheckCircle size={10} className="text-green-500" /> : <Copy size={10} />}
                                    </button>
                                </div>
                                <div className="flex items-center justify-between hover:bg-black/5 dark:hover:bg-white/5 rounded p-0.5 group">
                                    <code className="text-[10px] opacity-70">/v1/completions</code>
                                    <button onClick={(e) => {
                                        e.stopPropagation();
                                        const baseUrl = status.running ? status.base_url : `http://127.0.0.1:${appConfig.proxy.port || 8045}`;
                                        copyToClipboardHandler(`${baseUrl}/v1/completions`, 'openai-compl');
                                    }} className="opacity-0 group-hover:opacity-100 transition-opacity">
                                        {copied === 'openai-compl' ? <CheckCircle size={10} className="text-green-500" /> : <Copy size={10} />}
                                    </button>
                                </div>
                                <div className="flex items-center justify-between hover:bg-black/5 dark:hover:bg-white/5 rounded p-0.5 group">
                                    <code className="text-[10px] opacity-70 font-bold text-blue-500">/v1/responses (Codex)</code>
                                    <button onClick={(e) => {
                                        e.stopPropagation();
                                        const baseUrl = status.running ? status.base_url : `http://127.0.0.1:${appConfig.proxy.port || 8045}`;
                                        copyToClipboardHandler(`${baseUrl}/v1/responses`, 'openai-resp');
                                    }} className="opacity-0 group-hover:opacity-100 transition-opacity">
                                        {copied === 'openai-resp' ? <CheckCircle size={10} className="text-green-500" /> : <Copy size={10} />}
                                    </button>
                                </div>
                            </div>
                        </div>

                        {/* Anthropic Card */}
                        <div
                            className={`p-3 rounded-xl border-2 transition-all cursor-pointer ${selectedProtocol === 'anthropic' ? 'border-purple-500 bg-purple-50/30 dark:bg-purple-900/10' : 'border-gray-100 dark:border-base-200 hover:border-purple-200'}`}
                            onClick={() => startTransition(() => setSelectedProtocol('anthropic'))}
                        >
                            <div className="flex items-center justify-between mb-2">
                                <span className="text-xs font-bold text-purple-600">{t('proxy.multi_protocol.anthropic_label')}</span>
                                <button onClick={(e) => {
                                    e.stopPropagation();
                                    const baseUrl = status.running ? status.base_url : `http://127.0.0.1:${appConfig.proxy.port || 8045}`;
                                    copyToClipboardHandler(`${baseUrl}/v1/messages`, 'anthropic');
                                }} className="btn btn-ghost btn-xs">
                                    {copied === 'anthropic' ? <CheckCircle size={14} /> : <Copy size={14} />}
                                </button>
                            </div>
                            <code className="text-[10px] block truncate bg-black/5 dark:bg-white/5 p-1 rounded">/v1/messages</code>
                        </div>

                        {/* Gemini Card */}
                        <div
                            className={`p-3 rounded-xl border-2 transition-all cursor-pointer ${selectedProtocol === 'gemini' ? 'border-green-500 bg-green-50/30 dark:bg-green-900/10' : 'border-gray-100 dark:border-base-200 hover:border-green-200'}`}
                            onClick={() => startTransition(() => setSelectedProtocol('gemini'))}
                        >
                            <div className="flex items-center justify-between mb-2">
                                <span className="text-xs font-bold text-green-600">{t('proxy.multi_protocol.gemini_label')}</span>
                                <button onClick={(e) => {
                                    e.stopPropagation();
                                    const baseUrl = status.running ? status.base_url : `http://127.0.0.1:${appConfig.proxy.port || 8045}`;
                                    copyToClipboardHandler(`${baseUrl}/v1beta/models`, 'gemini');
                                }} className="btn btn-ghost btn-xs">
                                    {copied === 'gemini' ? <CheckCircle size={14} /> : <Copy size={14} />}
                                </button>
                            </div>
                            <code className="text-[10px] block truncate bg-black/5 dark:bg-white/5 p-1 rounded">/v1beta/models/...</code>
                        </div>
                    </div>
                </div>
            </div>
        );
    };

    // 档位后缀与分类帮助函数
    const TIER_SUFFIXES = ['-tiered', '-xhigh', '-high', '-medium', '-low', '-extra-low', '-thinking'];

    const getModelFamilyId = (modelId: string): string => {
        const id = modelId.toLowerCase().trim();
        for (const suffix of TIER_SUFFIXES) {
            if (id.endsWith(suffix)) {
                return id.slice(0, -suffix.length);
            }
        }
        return id;
    };

    const getTierLabel = (modelId: string, familyId: string): string => {
        const id = modelId.toLowerCase().trim();
        if (id === familyId) return 'Default (无后缀)';
        if (id.endsWith('-xhigh')) return 'X-High';
        if (id.endsWith('-high')) return 'High';
        if (id.endsWith('-medium')) return 'Medium';
        if (id.endsWith('-low')) return 'Low';
        if (id.endsWith('-extra-low')) return 'Extra-Low';
        if (id.endsWith('-tiered')) return 'Tiered';
        if (id.endsWith('-thinking')) return 'Thinking';
        const rem = id.replace(familyId, '').replace(/^[-_]/, '');
        return rem ? rem.toUpperCase() : 'Default';
    };

    const getTierBadgeClass = (tierLabel: string): string => {
        if (tierLabel.includes('Default') || tierLabel.includes('无后缀')) {
            return 'bg-gray-100 text-gray-700 border-gray-200/90 dark:bg-base-200 dark:text-gray-300 dark:border-base-300';
        }
        if (tierLabel.includes('X-High')) {
            return 'bg-purple-50 text-purple-700 border-purple-200/90 dark:bg-purple-950/40 dark:text-purple-300 dark:border-purple-800/60';
        }
        if (tierLabel.includes('High')) {
            return 'bg-emerald-50 text-emerald-700 border-emerald-200/90 dark:bg-emerald-950/40 dark:text-emerald-300 dark:border-emerald-800/60';
        }
        if (tierLabel.includes('Medium')) {
            return 'bg-blue-50 text-blue-700 border-blue-200/90 dark:bg-blue-950/40 dark:text-blue-300 dark:border-blue-800/60';
        }
        if (tierLabel.includes('Low')) {
            return 'bg-amber-50 text-amber-700 border-amber-200/90 dark:bg-amber-950/40 dark:text-amber-300 dark:border-amber-800/60';
        }
        if (tierLabel.includes('Tiered')) {
            return 'bg-cyan-50 text-cyan-700 border-cyan-200/90 dark:bg-cyan-950/40 dark:text-cyan-300 dark:border-cyan-800/60';
        }
        return 'bg-indigo-50 text-indigo-700 border-indigo-200/90 dark:bg-indigo-950/40 dark:text-indigo-300 dark:border-indigo-800/60';
    };

    const renderSupportedModelsSection = () => {
        if (!appConfig) return null;

        // 统一提取状态渲染标签
        const renderTestStatusBadge = (testStatus?: ModelTestStatus) => {
            if (testStatus?.loading) {
                return (
                    <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-[10px] font-mono font-medium whitespace-nowrap bg-blue-50 text-blue-700 border border-blue-200/80 dark:bg-blue-950/40 dark:text-blue-300 dark:border-blue-800/60 shadow-2xs">
                        <span className="loading loading-spinner loading-[10px] text-blue-600 dark:text-blue-400"></span>
                        <span>{t('proxy.supported_models.testing')}</span>
                    </span>
                );
            }
            if (testStatus?.isRetired) {
                return (
                    <span
                        onClick={(e) => {
                            e.stopPropagation();
                            if (testStatus.logId) handleOpenLogDetail(testStatus.logId);
                        }}
                        className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-[10px] font-mono font-medium whitespace-nowrap bg-rose-50 text-rose-700 border border-rose-200/80 dark:bg-rose-950/40 dark:text-rose-300 dark:border-rose-800/60 shadow-2xs transition-all ${
                            testStatus.logId ? 'cursor-pointer hover:shadow-xs hover:border-rose-400 dark:hover:border-rose-600 hover:scale-[1.02] active:scale-95' : ''
                        }`}
                        title={testStatus.logId ? t('proxy.supported_models.view_detail') : undefined}
                    >
                        <span className="w-1.5 h-1.5 rounded-full bg-rose-500 shrink-0"></span>
                        <span>{t('proxy.supported_models.retired')}</span>
                    </span>
                );
            }
            if (testStatus?.success) {
                return (
                    <span
                        onClick={(e) => {
                            e.stopPropagation();
                            if (testStatus.logId) handleOpenLogDetail(testStatus.logId);
                        }}
                        className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-[10px] font-mono font-medium whitespace-nowrap border shadow-2xs transition-all ${
                            testStatus.isHistorical
                                ? 'bg-gray-100/80 text-gray-700 border-gray-200/90 dark:bg-base-200 dark:text-gray-300 dark:border-base-300'
                                : 'bg-emerald-50 text-emerald-700 border-emerald-200/90 dark:bg-emerald-950/40 dark:text-emerald-300 dark:border-emerald-800/60'
                        } ${testStatus.logId ? 'cursor-pointer hover:shadow-xs hover:border-emerald-400 dark:hover:border-emerald-600 hover:scale-[1.02] active:scale-95' : ''}`}
                        title={testStatus.logId ? `${testStatus.accountEmail ? `账号: ${testStatus.accountEmail}\n` : ''}${t('proxy.supported_models.view_detail')}` : undefined}
                    >
                        <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 shrink-0"></span>
                        <span>{testStatus.durationMs != null ? `${testStatus.durationMs}ms` : '200 OK'}</span>
                    </span>
                );
            }
            if (testStatus?.status === 429) {
                return (
                    <span
                        onClick={(e) => {
                            e.stopPropagation();
                            if (testStatus.logId) handleOpenLogDetail(testStatus.logId);
                        }}
                        className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-[10px] font-mono font-medium whitespace-nowrap bg-amber-50 text-amber-700 border border-amber-200/90 dark:bg-amber-950/40 dark:text-amber-300 dark:border-amber-800/60 shadow-2xs transition-all ${
                            testStatus.logId ? 'cursor-pointer hover:shadow-xs hover:border-amber-400 dark:hover:border-amber-600 hover:scale-[1.02] active:scale-95' : ''
                        }`}
                        title={testStatus.logId ? t('proxy.supported_models.view_detail') : undefined}
                    >
                        <span className="w-1.5 h-1.5 rounded-full bg-amber-500 shrink-0"></span>
                        <span>{t('proxy.supported_models.rate_limited')}{testStatus.durationMs != null ? ` (${testStatus.durationMs}ms)` : ''}</span>
                    </span>
                );
            }
            if (testStatus && !testStatus.success && !testStatus.loading) {
                return (
                    <span
                        onClick={(e) => {
                            e.stopPropagation();
                            if (testStatus.logId) handleOpenLogDetail(testStatus.logId);
                        }}
                        className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-[10px] font-mono font-medium whitespace-nowrap bg-rose-50 text-rose-700 border border-rose-200/90 dark:bg-rose-950/40 dark:text-rose-300 dark:border-rose-800/60 shadow-2xs transition-all ${
                            testStatus.logId ? 'cursor-pointer hover:shadow-xs hover:border-rose-400 dark:hover:border-rose-600 hover:scale-[1.02] active:scale-95' : ''
                        }`}
                        title={testStatus.error || (testStatus.logId ? t('proxy.supported_models.view_detail') : undefined)}
                    >
                        <span className="w-1.5 h-1.5 rounded-full bg-rose-500 shrink-0"></span>
                        <span>{testStatus.status ? `${testStatus.status}` : 'Err'}</span>
                    </span>
                );
            }
            return <span className="text-[10px] text-gray-400 dark:text-gray-500 font-mono select-none px-2 py-0.5">--</span>;
        };

        // 按基础模型家族聚合（二级层级数据结构）
        interface ModelVariantItem {
            id: string;
            name: string;
            group: string;
            icon: React.ReactNode;
            tierLabel: string;
            isBase: boolean;
        }

        interface ModelFamilyItem {
            familyId: string;
            familyName: string;
            group: string;
            icon: React.ReactNode;
            baseModel: ModelVariantItem;
            variants: ModelVariantItem[];
        }

        const familyMap = new Map<string, {
            familyId: string;
            familyName: string;
            group: string;
            icon: React.ReactNode;
            variants: ModelVariantItem[];
        }>();

        for (const m of filteredModels) {
            const famId = getModelFamilyId(m.id);
            const tierLabel = getTierLabel(m.id, famId);
            const isBase = m.id.toLowerCase().trim() === famId.toLowerCase().trim();

            if (!familyMap.has(famId)) {
                const group = m.group || inferModelGroup(famId);
                const icon = getModelIcon(famId, group);

                familyMap.set(famId, {
                    familyId: famId,
                    familyName: famId,
                    group,
                    icon,
                    variants: []
                });
            }

            const fam = familyMap.get(famId)!;
            fam.variants.push({
                id: m.id,
                name: m.name,
                group: m.group,
                icon: m.icon || getModelIcon(m.id, m.group),
                tierLabel,
                isBase
            });
        }

        const tierPriority = (v: ModelVariantItem) => {
            if (v.isBase) return 0;
            const l = v.tierLabel.toLowerCase();
            if (l.includes('x-high')) return 1;
            if (l.includes('high')) return 2;
            if (l.includes('medium')) return 3;
            if (l.includes('low')) return 4;
            if (l.includes('extra-low')) return 5;
            if (l.includes('tiered')) return 6;
            if (l.includes('thinking')) return 7;
            return 8;
        };

        const modelFamilyGroups: ModelFamilyItem[] = [];
        for (const fam of familyMap.values()) {
            fam.variants.sort((a, b) => tierPriority(a) - tierPriority(b));
            const baseModel = fam.variants.find(v => v.isBase) || fam.variants[0];
            modelFamilyGroups.push({
                familyId: fam.familyId,
                familyName: fam.familyName,
                group: fam.group,
                icon: fam.icon,
                baseModel,
                variants: fam.variants
            });
        }

        // 按系列过滤
        const displayedFamilies = modelFamilyGroups.filter(f => {
            if (selectedSeriesTab === 'all') return true;
            const g = (f.group || '').toLowerCase();
            const fid = f.familyId.toLowerCase();
            if (selectedSeriesTab === 'Gemini 4') {
                return g.includes('gemini 4') || fid.startsWith('gemini-4');
            }
            if (selectedSeriesTab === 'Gemini 3') {
                return g.includes('gemini 3') || fid.startsWith('gemini-3');
            }
            if (selectedSeriesTab === 'Claude') {
                return g.includes('claude') || fid.startsWith('claude');
            }
            if (selectedSeriesTab === 'Other') {
                return !fid.startsWith('gemini-') && !fid.startsWith('claude-');
            }
            return true;
        });

        const expandableFamilies = displayedFamilies.filter(f => f.variants.length > 1);
        const allExpanded = expandableFamilies.length > 0 && expandableFamilies.every(f => expandedFamilies[f.familyId]);

        const toggleAllFamilies = () => {
            if (allExpanded) {
                setExpandedFamilies({});
            } else {
                const next: Record<string, boolean> = {};
                for (const f of expandableFamilies) {
                    next[f.familyId] = true;
                }
                setExpandedFamilies(next);
            }
        };

        const toggleFamily = (familyId: string) => {
            setExpandedFamilies(prev => ({
                ...prev,
                [familyId]: !prev[familyId]
            }));
        };

        return (
            <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200 overflow-hidden">
                <div className="px-4 py-3 border-b border-gray-100 dark:border-base-200 flex flex-col sm:flex-row sm:items-center justify-between gap-2.5 bg-gray-50/40 dark:bg-base-200/40">
                    <h2 className="text-sm font-bold text-gray-900 dark:text-base-content flex items-center gap-2">
                        <Terminal size={16} className="text-blue-500" />
                        <span>{t('proxy.supported_models.title')}</span>
                    </h2>
                    <div className="flex items-center gap-2 flex-wrap shrink-0">
                        {/* 自定义账号选择器（彻底解决原生 select 在部分系统/WKWebView 下弹窗失效问题） */}
                        <div className="relative inline-flex items-center" ref={accountDropdownRef}>
                            <button
                                type="button"
                                onClick={() => setAccountDropdownOpen(prev => !prev)}
                                className="flex items-center gap-1.5 h-7 px-2.5 rounded-lg bg-gray-100 hover:bg-gray-200/70 dark:bg-base-200 dark:hover:bg-base-300 border border-gray-200/90 dark:border-base-300 shadow-2xs transition-all cursor-pointer text-[11px] font-semibold text-gray-700 dark:text-gray-200 select-none active:scale-95"
                                title={t('proxy.supported_models.select_account')}
                            >
                                <Bot size={13} className="text-amber-500 shrink-0" />
                                <span className="max-w-[170px] truncate text-left">
                                    {testTargetAccountId
                                        ? availableAccounts.find(a => a.id === testTargetAccountId)?.email || testTargetAccountId
                                        : t('proxy.supported_models.auto_dispatch')}
                                </span>
                                <ChevronDown size={11} className={cn("text-gray-400 shrink-0 transition-transform duration-200", accountDropdownOpen && "rotate-180")} />
                            </button>

                            {accountDropdownOpen && (
                                <div className="absolute left-0 sm:right-0 sm:left-auto top-full mt-1.5 w-64 max-h-64 overflow-y-auto bg-white dark:bg-base-100 rounded-xl shadow-xl border border-gray-200 dark:border-base-300 py-1 z-50 animate-in fade-in zoom-in-95 duration-100">
                                    <div
                                        onClick={() => {
                                            setTestTargetAccountId('');
                                            setAccountDropdownOpen(false);
                                        }}
                                        className={cn(
                                            "flex items-center justify-between px-3 py-2 text-xs cursor-pointer transition-colors hover:bg-amber-50 dark:hover:bg-amber-950/30",
                                            !testTargetAccountId ? "text-amber-600 dark:text-amber-400 font-bold bg-amber-50/60 dark:bg-amber-950/20" : "text-gray-700 dark:text-gray-200"
                                        )}
                                    >
                                        <div className="flex items-center gap-2 truncate">
                                            <Bot size={13} className="text-amber-500 shrink-0" />
                                            <span className="truncate">{t('proxy.supported_models.auto_dispatch')}</span>
                                        </div>
                                        {!testTargetAccountId && <Check size={13} className="text-amber-600 dark:text-amber-400 shrink-0" />}
                                    </div>
                                    {availableAccounts.length > 0 && <div className="h-px bg-gray-100 dark:bg-base-200 my-1" />}
                                    {availableAccounts.map(acc => {
                                        const isSelected = testTargetAccountId === acc.id;
                                        return (
                                            <div
                                                key={acc.id}
                                                onClick={() => {
                                                    setTestTargetAccountId(acc.id);
                                                    setAccountDropdownOpen(false);
                                                }}
                                                className={cn(
                                                    "flex items-center justify-between px-3 py-2 text-xs cursor-pointer transition-colors hover:bg-gray-100 dark:hover:bg-base-200",
                                                    isSelected ? "text-blue-600 dark:text-blue-400 font-bold bg-blue-50/60 dark:bg-blue-950/20" : "text-gray-700 dark:text-gray-200"
                                                )}
                                            >
                                                <span className="truncate" title={acc.email}>{acc.email}</span>
                                                {isSelected && <Check size={13} className="text-blue-600 dark:text-blue-400 shrink-0" />}
                                            </div>
                                        );
                                    })}
                                </div>
                            )}
                        </div>

                        {/* 批量测活按钮 */}
                        <button
                            className={`btn btn-xs h-7 min-h-0 px-2.5 rounded-lg gap-1.5 font-semibold text-[11px] shadow-2xs transition-all active:scale-95 ${
                                testingAll
                                    ? 'btn-error btn-outline'
                                    : 'bg-amber-500 hover:bg-amber-600 text-white border-transparent'
                            }`}
                            onClick={handleBatchTest}
                            title={testingAll ? t('proxy.supported_models.stop_test') : t('proxy.supported_models.batch_test')}
                        >
                            {testingAll ? (
                                <>
                                    <Square size={11} className="fill-current" />
                                    <span>{t('proxy.supported_models.stop_test')}</span>
                                </>
                            ) : (
                                <>
                                    <Zap size={11} className="fill-current" />
                                    <span>{t('proxy.supported_models.batch_test')}</span>
                                </>
                            )}
                        </button>
                    </div>
                </div>

                {/* 系列分类快速切换与全部展开/折叠栏 */}
                <div className="flex items-center gap-1.5 px-4 py-2 bg-gray-50/80 dark:bg-base-200/60 border-b border-gray-100 dark:border-base-200 text-xs flex-wrap">
                    <span className="text-[11px] font-medium text-gray-400 dark:text-gray-500 mr-1 select-none">系列:</span>
                    {[
                        { id: 'all', label: '全部' },
                        { id: 'Gemini 4', label: 'Gemini 4' },
                        { id: 'Gemini 3', label: 'Gemini 3' },
                        { id: 'Claude', label: 'Claude' },
                        { id: 'Other', label: 'OpenAI / 其他' },
                    ].map(tab => (
                        <button
                            key={tab.id}
                            type="button"
                            onClick={() => setSelectedSeriesTab(tab.id)}
                            className={cn(
                                "px-2.5 py-1 rounded-md font-medium text-[11px] transition-all select-none cursor-pointer",
                                selectedSeriesTab === tab.id
                                    ? "bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-2xs font-semibold border border-gray-200/80 dark:border-base-300"
                                    : "text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200 hover:bg-gray-200/50 dark:hover:bg-base-300/50"
                            )}
                        >
                            {tab.label}
                        </button>
                    ))}
                    {expandableFamilies.length > 0 && (
                        <div className="ml-auto flex items-center gap-2">
                            <button
                                type="button"
                                onClick={toggleAllFamilies}
                                className="text-[11px] text-gray-500 hover:text-blue-600 dark:text-gray-400 dark:hover:text-blue-400 flex items-center gap-1 px-2 py-0.5 rounded hover:bg-gray-200/50 dark:hover:bg-base-300/50 transition-colors select-none cursor-pointer"
                            >
                                <Layers size={12} />
                                <span>{allExpanded ? '全部折叠' : '全部展开'}</span>
                            </button>
                        </div>
                    )}
                </div>

                <div className="grid grid-cols-1 lg:grid-cols-3 gap-0 lg:divide-x dark:divide-gray-700">
                    {/* 左侧：多级模型家族列表 */}
                    <div className="col-span-2 p-0">
                        <div className="overflow-x-auto">
                            <table className="table w-full">
                                <thead className="bg-gray-50/70 dark:bg-gray-800/70 text-gray-500 dark:text-gray-400 select-none">
                                    <tr className="border-b border-gray-100 dark:border-base-200">
                                        <th className="w-8 pl-3 pr-0"></th>
                                        <th className="w-8 px-1"></th>
                                        <th className="text-[11px] font-medium whitespace-nowrap min-w-[150px]">{t('proxy.supported_models.model_name')}</th>
                                        <th className="text-[11px] font-medium whitespace-nowrap min-w-[140px]">{t('proxy.supported_models.model_id')}</th>
                                        <th className="text-[11px] font-medium whitespace-nowrap min-w-[130px]">{t('proxy.supported_models.status')}</th>
                                        <th className="text-[11px] w-36 text-center font-medium whitespace-nowrap">{t('proxy.supported_models.action')}</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {displayedFamilies.map((family) => {
                                        const isExpanded = !!expandedFamilies[family.familyId];
                                        const hasVariants = family.variants.length > 1;
                                        const baseTestStatus = modelTestResults[family.baseModel.id];
                                        const isBaseSelected = selectedModelId === family.baseModel.id;

                                        return (
                                            <React.Fragment key={family.familyId}>
                                                {/* 一级界面：模型家族基准模型（默认测活无后缀模型） */}
                                                <tr
                                                    className={cn(
                                                        "h-11 border-b border-gray-100/70 dark:border-base-200/60 hover:bg-blue-50/40 dark:hover:bg-blue-900/10 cursor-pointer transition-colors",
                                                        isBaseSelected ? "bg-blue-50/70 dark:bg-blue-900/20" : ""
                                                    )}
                                                    onClick={() => setSelectedModelId(family.baseModel.id)}
                                                >
                                                    {/* 折叠/展开箭头 */}
                                                    <td className="pl-3 pr-0 py-0 align-middle">
                                                        {hasVariants ? (
                                                            <button
                                                                type="button"
                                                                onClick={(e) => {
                                                                    e.stopPropagation();
                                                                    toggleFamily(family.familyId);
                                                                }}
                                                                className="w-6 h-6 flex items-center justify-center rounded hover:bg-gray-200/70 dark:hover:bg-base-300 text-gray-400 hover:text-blue-600 transition-colors cursor-pointer"
                                                                title={isExpanded ? '折叠档位' : `展开 ${family.variants.length} 个档位`}
                                                            >
                                                                <ChevronRight
                                                                    size={13}
                                                                    className={cn(
                                                                        "transition-transform duration-200",
                                                                        isExpanded && "rotate-90 text-blue-600 dark:text-blue-400"
                                                                    )}
                                                                />
                                                            </button>
                                                        ) : (
                                                            <div className="w-6 h-6" />
                                                        )}
                                                    </td>

                                                    {/* 模型图标 */}
                                                    <td className="px-1 py-0 align-middle text-blue-500">
                                                        {family.icon}
                                                    </td>

                                                    {/* 模型名称与档位计数徽章 */}
                                                    <td className="py-0 align-middle whitespace-nowrap">
                                                        <div className="flex items-center gap-2">
                                                            <span className="font-bold text-xs text-gray-900 dark:text-gray-100">
                                                                {family.familyName}
                                                            </span>
                                                            {hasVariants && (
                                                                <button
                                                                    type="button"
                                                                    onClick={(e) => {
                                                                        e.stopPropagation();
                                                                        toggleFamily(family.familyId);
                                                                    }}
                                                                    className="px-1.5 py-0.5 rounded text-[10px] font-semibold bg-blue-50 hover:bg-blue-100 text-blue-600 dark:bg-blue-950/40 dark:hover:bg-blue-900/50 dark:text-blue-300 border border-blue-200/70 dark:border-blue-800/50 shadow-2xs transition-colors cursor-pointer select-none"
                                                                    title="点击展开/折叠各个档位"
                                                                >
                                                                    {family.variants.length} 档位
                                                                </button>
                                                            )}
                                                        </div>
                                                    </td>

                                                    {/* 模型 ID（一级界面为无后缀模型 ID） */}
                                                    <td className="py-0 align-middle whitespace-nowrap">
                                                        <div className="flex items-center gap-1.5">
                                                            <span className="font-mono text-[10px] text-gray-500 dark:text-gray-400">
                                                                {family.baseModel.id}
                                                            </span>
                                                            {hasVariants && (
                                                                <span className="text-[9px] px-1 py-0.2 rounded bg-gray-100 dark:bg-base-200 text-gray-500 dark:text-gray-400 font-mono">
                                                                    Base
                                                                </span>
                                                            )}
                                                        </div>
                                                    </td>

                                                    {/* 状态 / 连通性（显示无后缀基准模型状态） */}
                                                    <td className="py-0 align-middle whitespace-nowrap">
                                                        {renderTestStatusBadge(baseTestStatus)}
                                                    </td>

                                                    {/* 操作列：一级界面默认激活无后缀模型 */}
                                                    <td className="py-0 text-center align-middle whitespace-nowrap px-2">
                                                        <div className="inline-flex items-center justify-center gap-1.5">
                                                            <button
                                                                className={`btn btn-xs h-6.5 min-h-0 px-2 rounded-md font-medium text-[11px] transition-all shadow-2xs ${
                                                                    baseTestStatus?.loading
                                                                        ? 'btn-disabled opacity-60'
                                                                        : 'bg-amber-500/10 hover:bg-amber-500/20 text-amber-700 dark:text-amber-300 border border-amber-500/25 active:scale-95'
                                                                }`}
                                                                disabled={baseTestStatus?.loading || testingAll}
                                                                title={`测活无后缀基准模型 (${family.baseModel.id})`}
                                                                onClick={(e) => {
                                                                    e.stopPropagation();
                                                                    handleTestModel(family.baseModel.id);
                                                                }}
                                                            >
                                                                {baseTestStatus?.loading ? (
                                                                    <span className="loading loading-spinner loading-[10px]"></span>
                                                                ) : (
                                                                    <div className="flex items-center gap-1 font-semibold">
                                                                        <Zap size={11} className="fill-current" />
                                                                        <span>{t('proxy.supported_models.test_connectivity')}</span>
                                                                    </div>
                                                                )}
                                                            </button>
                                                            <button
                                                                className="btn btn-xs h-6.5 min-h-0 px-2 rounded-md font-medium text-[11px] bg-gray-100 hover:bg-gray-200 dark:bg-base-200 dark:hover:bg-base-300 text-gray-700 dark:text-gray-300 border border-gray-200/70 dark:border-base-300 transition-all active:scale-95 shadow-2xs"
                                                                onClick={(e) => {
                                                                    e.stopPropagation();
                                                                    copyToClipboardHandler(family.baseModel.id, `model-${family.baseModel.id}`);
                                                                }}
                                                                title={t('common.copy')}
                                                            >
                                                                {copied === `model-${family.baseModel.id}` ? (
                                                                    <div className="flex items-center gap-1 font-semibold text-emerald-600">
                                                                        <CheckCircle size={11} />
                                                                        <span>{t('common.copied')}</span>
                                                                    </div>
                                                                ) : (
                                                                    <div className="flex items-center gap-1 font-semibold">
                                                                        <Copy size={11} />
                                                                        <span>{t('common.copy')}</span>
                                                                    </div>
                                                                )}
                                                            </button>
                                                        </div>
                                                    </td>
                                                </tr>

                                                {/* 二级界面：各档位子模型列表（展开时渲染，可分别对各个模型进行激活） */}
                                                {isExpanded && hasVariants && family.variants.map((variant, vIdx) => {
                                                    const variantTestStatus = modelTestResults[variant.id];
                                                    const isVariantSelected = selectedModelId === variant.id;
                                                    const isLast = vIdx === family.variants.length - 1;

                                                    return (
                                                        <tr
                                                            key={variant.id}
                                                            className={cn(
                                                                "h-9 border-b border-gray-100/50 dark:border-base-200/40 bg-gray-50/50 hover:bg-blue-50/40 dark:bg-base-200/25 dark:hover:bg-blue-900/15 cursor-pointer transition-colors text-xs",
                                                                isVariantSelected ? "bg-blue-50/70 dark:bg-blue-900/25" : ""
                                                            )}
                                                            onClick={() => setSelectedModelId(variant.id)}
                                                        >
                                                            {/* 缩进与树状连线符号 */}
                                                            <td className="pl-5 pr-0 py-0 align-middle text-gray-300 dark:text-gray-600 font-mono text-[11px] select-none">
                                                                {isLast ? '└' : '├'}
                                                            </td>

                                                            {/* 档位图标 */}
                                                            <td className="px-1 py-0 align-middle text-gray-400 dark:text-gray-500 scale-90">
                                                                {variant.icon}
                                                            </td>

                                                            {/* 档位徽章 */}
                                                            <td className="py-0 align-middle whitespace-nowrap pl-1">
                                                                <div className="flex items-center gap-2">
                                                                    <span className={cn(
                                                                        "px-2 py-0.5 rounded text-[10px] font-semibold border shadow-2xs font-mono",
                                                                        getTierBadgeClass(variant.tierLabel)
                                                                    )}>
                                                                        {variant.tierLabel}
                                                                    </span>
                                                                </div>
                                                            </td>

                                                            {/* 完整 Model ID */}
                                                            <td className="py-0 align-middle whitespace-nowrap">
                                                                <span className={cn(
                                                                    "font-mono text-[10px]",
                                                                    isVariantSelected ? "font-bold text-blue-600 dark:text-blue-400" : "text-gray-600 dark:text-gray-300"
                                                                )}>
                                                                    {variant.id}
                                                                </span>
                                                            </td>

                                                            {/* 档位专属连通性状态 */}
                                                            <td className="py-0 align-middle whitespace-nowrap">
                                                                {renderTestStatusBadge(variantTestStatus)}
                                                            </td>

                                                            {/* 档位专属操作：分别激活此模型 */}
                                                            <td className="py-0 text-center align-middle whitespace-nowrap px-2">
                                                                <div className="inline-flex items-center justify-center gap-1.5">
                                                                    <button
                                                                        className={`btn btn-xs h-6 min-h-0 px-2 rounded font-medium text-[10px] transition-all shadow-2xs ${
                                                                            variantTestStatus?.loading
                                                                                ? 'btn-disabled opacity-60'
                                                                                : 'bg-amber-500/10 hover:bg-amber-500/20 text-amber-700 dark:text-amber-300 border border-amber-500/25 active:scale-95'
                                                                        }`}
                                                                        disabled={variantTestStatus?.loading || testingAll}
                                                                        title={`测活档位模型 (${variant.id})`}
                                                                        onClick={(e) => {
                                                                            e.stopPropagation();
                                                                            handleTestModel(variant.id);
                                                                        }}
                                                                    >
                                                                        {variantTestStatus?.loading ? (
                                                                            <span className="loading loading-spinner loading-[10px]"></span>
                                                                        ) : (
                                                                            <div className="flex items-center gap-1 font-semibold">
                                                                                <Zap size={10} className="fill-current" />
                                                                                <span>{t('proxy.supported_models.test_connectivity')}</span>
                                                                            </div>
                                                                        )}
                                                                    </button>
                                                                    <button
                                                                        className="btn btn-xs h-6 min-h-0 px-2 rounded font-medium text-[10px] bg-gray-100 hover:bg-gray-200 dark:bg-base-200 dark:hover:bg-base-300 text-gray-700 dark:text-gray-300 border border-gray-200/70 dark:border-base-300 transition-all active:scale-95 shadow-2xs"
                                                                        onClick={(e) => {
                                                                            e.stopPropagation();
                                                                            copyToClipboardHandler(variant.id, `model-${variant.id}`);
                                                                        }}
                                                                        title={t('common.copy')}
                                                                    >
                                                                        {copied === `model-${variant.id}` ? (
                                                                            <div className="flex items-center gap-1 font-semibold text-emerald-600">
                                                                                <CheckCircle size={10} />
                                                                                <span>{t('common.copied')}</span>
                                                                            </div>
                                                                        ) : (
                                                                            <div className="flex items-center gap-1 font-semibold">
                                                                                <Copy size={10} />
                                                                                <span>{t('common.copy')}</span>
                                                                            </div>
                                                                        )}
                                                                    </button>
                                                                </div>
                                                            </td>
                                                        </tr>
                                                    );
                                                })}
                                            </React.Fragment>
                                        );
                                    })}
                                </tbody>
                            </table>
                        </div>
                    </div>

                    {/* 右侧：代码预览 */}
                    <div className="col-span-1 bg-gray-900 text-blue-100 flex flex-col h-[400px] lg:h-auto">
                        <div className="p-3 border-b border-gray-800 flex items-center justify-between">
                            <span className="text-xs font-bold text-gray-400 uppercase tracking-wider">{t('proxy.multi_protocol.quick_integration')}</span>
                            <div className="flex gap-2">
                                <span className="text-[10px] px-2 py-0.5 rounded bg-blue-500/20 text-blue-400 border border-blue-500/30">
                                    {selectedProtocol === 'anthropic' ? 'Python (Anthropic SDK)' : (selectedProtocol === 'gemini' ? 'Python (Google GenAI)' : 'Python (OpenAI SDK)')}
                                </span>
                            </div>
                        </div>
                        <div className="flex-1 relative overflow-hidden group">
                            <div className="absolute inset-0 overflow-auto scrollbar-thin scrollbar-thumb-gray-700 scrollbar-track-transparent">
                                <pre className="p-4 text-[10px] font-mono leading-relaxed">
                                    {getPythonExample(selectedModelId)}
                                </pre>
                            </div>
                            <button
                                onClick={() => copyToClipboardHandler(getPythonExample(selectedModelId), 'example-code')}
                                className="absolute top-4 right-4 p-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors text-white opacity-0 group-hover:opacity-100"
                            >
                                {copied === 'example-code' ? <CheckCircle size={16} /> : <Copy size={16} />}
                            </button>
                        </div>
                        <div className="p-3 bg-gray-800/50 border-t border-gray-800 text-[10px] text-gray-400">
                            {t('proxy.multi_protocol.click_tip')}
                        </div>
                    </div>
                </div>

                {selectedDetailLog && (
                    <RequestDetailModal
                        log={selectedDetailLog}
                        onClose={() => setSelectedDetailLog(null)}
                        loadingDetail={loadingDetailLog}
                    />
                )}
            </div>
        );
    };

    return (
        <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.2, ease: "easeOut" }}
            className="h-full w-full overflow-y-auto overflow-x-hidden"
        >
            <div className="p-5 space-y-4 max-w-7xl mx-auto">

                {/* Loading State */}
                {configLoading && (
                    <div className="flex items-center justify-center py-20">
                        <div className="flex flex-col items-center gap-4">
                            <RefreshCw size={32} className="animate-spin text-blue-500" />
                            <span className="text-sm text-gray-500 dark:text-gray-400">
                                {t('common.loading')}
                            </span>
                        </div>
                    </div>
                )}

                {/* Error State */}
                {!configLoading && configError && (
                    <div className="flex items-center justify-center py-20">
                        <div className="flex flex-col items-center gap-4 text-center">
                            <div className="w-16 h-16 rounded-full bg-red-100 dark:bg-red-900/30 flex items-center justify-center">
                                <Settings size={32} className="text-red-500" />
                            </div>
                            <div className="space-y-2">
                                <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                                    {t('proxy.error.load_failed')}
                                </h3>
                                <p className="text-sm text-gray-500 dark:text-gray-400 max-w-md">
                                    {configError}
                                </p>
                            </div>
                            <button
                                onClick={loadConfig}
                                className="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg text-sm font-medium flex items-center gap-2 transition-colors"
                            >
                                <RefreshCw size={16} />
                                {t('common.retry')}
                            </button>
                        </div>
                    </div>
                )}

                {/* 配置区 */}
                {!configLoading && !configError && appConfig && (
                    <div className="bg-white dark:bg-base-100 rounded-xl shadow-xs border border-gray-200/80 dark:border-base-200">
                        <div className="px-4 sm:px-5 py-2.5 border-b border-gray-100 dark:border-base-200 bg-gray-50/60 dark:bg-base-200/80 flex flex-wrap items-center justify-between gap-3">
                            {/* 菜单栏：服务配置、模型配置、模型大全与协议、CLI 一键配置 */}
                            <div className="flex items-center gap-1.5 bg-gray-200/70 dark:bg-base-300/60 p-1 rounded-xl">
                                <button
                                    type="button"
                                    onClick={() => setActiveMenuTab('settings')}
                                    className={`px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5 transition-all ${
                                        activeMenuTab === 'settings'
                                            ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-xs'
                                            : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                                    }`}
                                >
                                    <Settings size={14} className={activeMenuTab === 'settings' ? 'text-blue-600 dark:text-blue-400' : 'text-gray-400'} />
                                    {t('proxy.config.title')}
                                </button>
                                <button
                                    type="button"
                                    onClick={() => setActiveMenuTab('models')}
                                    className={`px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5 transition-all ${
                                        activeMenuTab === 'models'
                                            ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-xs'
                                            : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                                    }`}
                                >
                                    <BrainCircuit size={14} className={activeMenuTab === 'models' ? 'text-blue-600 dark:text-blue-400' : 'text-gray-400'} />
                                    {t('proxy.model_config.title', { defaultValue: '模型配置' })}
                                </button>
                                <button
                                    type="button"
                                    onClick={() => setActiveMenuTab('guard')}
                                    className={`px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5 transition-all ${
                                        activeMenuTab === 'guard'
                                            ? 'bg-white dark:bg-base-100 text-amber-600 dark:text-amber-400 shadow-xs'
                                            : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                                    }`}
                                >
                                    <ShieldAlert size={14} className={activeMenuTab === 'guard' ? 'text-amber-600 dark:text-amber-400' : 'text-gray-400'} />
                                    {t('proxy.prompt_rewrite.title', { defaultValue: '风控提示词清理方案' })}
                                </button>
                                <button
                                    type="button"
                                    onClick={() => setActiveMenuTab('cli')}
                                    className={`px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5 transition-all ${
                                        activeMenuTab === 'cli'
                                            ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-xs'
                                            : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                                    }`}
                                >
                                    <Terminal size={14} className={activeMenuTab === 'cli' ? 'text-blue-600 dark:text-blue-400' : 'text-gray-400'} />
                                    {t('proxy.cli_sync.title', { defaultValue: 'Agent工具一键配置' })}
                                </button>
                                <button
                                    type="button"
                                    onClick={() => setActiveMenuTab('protocols')}
                                    className={`px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5 transition-all ${
                                        activeMenuTab === 'protocols'
                                            ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-xs'
                                            : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                                    }`}
                                >
                                    <Share2 size={14} className={activeMenuTab === 'protocols' ? 'text-blue-600 dark:text-blue-400' : 'text-gray-400'} />
                                    {t('proxy.multi_protocol.title')}
                                </button>
                            </div>

                            {/* 控制按钮 */}
                            <div className="flex items-center gap-2.5 ml-auto">
                                <button
                                    onClick={handleSaveProxySettings}
                                    disabled={!appConfig}
                                    className={`px-3 py-1.5 rounded-lg text-xs font-semibold transition-all flex items-center gap-1.5 bg-blue-600 hover:bg-blue-700 text-white shadow-xs active:scale-95 ${!appConfig ? 'opacity-50 cursor-not-allowed' : ''}`}
                                >
                                    <Save size={14} />
                                    {t('settings.save')}
                                </button>
                                <button
                                    onClick={handleToggle}
                                    disabled={loading || !appConfig}
                                    className={`px-3 py-1.5 rounded-lg text-xs font-semibold transition-all flex items-center gap-1.5 shadow-xs active:scale-95 ${status.running
                                        ? 'bg-rose-600 hover:bg-rose-700 text-white border border-rose-600'
                                        : 'bg-emerald-600 hover:bg-emerald-700 text-white border border-emerald-600'
                                        } ${(loading || !appConfig) ? 'opacity-50 cursor-not-allowed' : ''}`}
                                >
                                    <Power size={14} />
                                    {loading ? t('proxy.status.processing') : (status.running ? t('proxy.action.stop') : t('proxy.action.start'))}
                                </button>
                            </div>
                        </div>

                        {/* TAB 1: 服务配置 (settings) */}
                        {activeMenuTab === 'settings' && (
                            <div className="p-4 space-y-4">
                            {/* 监听端口、超时和自启动 */}
                            <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                                <div>
                                    <label className="block text-xs font-semibold text-gray-700 dark:text-gray-200 mb-1.5">
                                        <span className="inline-flex items-center gap-1">
                                            {t('proxy.config.port')}
                                            <HelpTooltip
                                                text={t('proxy.config.port_tooltip')}
                                                ariaLabel={t('proxy.config.port')}
                                                placement="right"
                                            />
                                        </span>
                                    </label>
                                    <input
                                        type="number"
                                        value={appConfig?.proxy?.port ?? 8045}
                                        onChange={(e) => updateProxyConfig({ port: parseInt(e.target.value) || 8045 })}
                                        min={8000}
                                        max={65535}
                                        disabled={status.running}
                                        className="w-full px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-xs font-mono text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500/20 focus:border-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
                                    />
                                    <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                                        {t('proxy.config.port_hint')}
                                    </p>

                                    {/* 监听端口下方空位：服务运行状态指示卡片 */}
                                    <div className="mt-2.5 p-2 rounded-lg border border-gray-200/80 dark:border-base-300/80 bg-gray-50/70 dark:bg-base-200/50 flex items-center justify-between">
                                        <div className="flex items-center gap-2">
                                            <div className={`w-2.5 h-2.5 rounded-full ${status.running ? 'bg-emerald-500 shadow-xs shadow-emerald-500/50 animate-pulse' : 'bg-gray-400 dark:bg-gray-500'}`} />
                                            <span className={`text-xs font-semibold ${status.running ? 'text-emerald-600 dark:text-emerald-400' : 'text-gray-500 dark:text-gray-400'}`}>
                                                {status.running
                                                    ? `${t('proxy.status.running')} (${status.active_accounts} ${t('common.accounts')})`
                                                    : t('proxy.status.stopped')}
                                            </span>
                                        </div>
                                        {status.running && (
                                            <span className="text-[10px] font-mono text-gray-400 dark:text-gray-500 truncate max-w-[130px]" title={status.base_url || `http://127.0.0.1:${appConfig.proxy.port || 8045}`}>
                                                {status.base_url || `http://127.0.0.1:${appConfig.proxy.port || 8045}`}
                                            </span>
                                        )}
                                    </div>
                                </div>
                                <div>
                                    <label className="block text-xs font-semibold text-gray-700 dark:text-gray-200 mb-1.5">
                                        <span className="inline-flex items-center gap-1">
                                            {t('proxy.config.request_timeout')}
                                            <HelpTooltip
                                                text={t('proxy.config.request_timeout_tooltip')}
                                                ariaLabel={t('proxy.config.request_timeout')}
                                                placement="top"
                                            />
                                        </span>
                                    </label>
                                    <input
                                        type="number"
                                        value={appConfig.proxy.request_timeout || 120}
                                        onChange={(e) => {
                                             const value = parseInt(e.target.value);
                                             const timeout = Math.max(30, Math.min(7200, value));
                                             updateProxyConfig({ request_timeout: timeout });
                                        }}
                                        min={30}
                                        max={7200}
                                        disabled={status.running}
                                        className="w-full px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-xs font-mono text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500/20 focus:border-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
                                    />
                                    <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                                        {t('proxy.config.request_timeout_hint')}
                                    </p>
                                </div>
                                <div className="flex items-center pt-5">
                                    <label className="flex items-center cursor-pointer gap-3 select-none">
                                        <input
                                            type="checkbox"
                                            className="toggle toggle-sm bg-gray-200 dark:bg-base-300 border-gray-300 dark:border-base-300 checked:bg-blue-600 checked:border-blue-600 disabled:opacity-50 disabled:bg-gray-100 dark:disabled:bg-base-200"
                                            checked={appConfig.proxy.auto_start}
                                            onChange={(e) => updateProxyConfig({ auto_start: e.target.checked })}
                                        />
                                        <span className="text-xs font-semibold text-gray-700 dark:text-gray-200 inline-flex items-center gap-1">
                                            {t('proxy.config.auto_start')}
                                            <HelpTooltip
                                                text={t('proxy.config.auto_start_tooltip')}
                                                ariaLabel={t('proxy.config.auto_start')}
                                                placement="right"
                                            />
                                        </span>
                                    </label>
                                </div>
                            </div>


                            {/* 局域网访问 & 访问授权 */}
                            <div className="border-t border-gray-100 dark:border-base-200 pt-3.5 mt-3.5">
                                <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                                    {/* 允许局域网访问 */}
                                    <div className="space-y-1.5">
                                        <div className="flex items-center justify-between">
                                            <span className="text-xs font-semibold text-gray-700 dark:text-gray-200 inline-flex items-center gap-1">
                                                {t('proxy.config.allow_lan_access')}
                                                <HelpTooltip
                                                    text={t('proxy.config.allow_lan_access_tooltip')}
                                                    ariaLabel={t('proxy.config.allow_lan_access')}
                                                    placement="right"
                                                />
                                            </span>
                                            <input
                                                type="checkbox"
                                                className="toggle toggle-sm bg-gray-200 dark:bg-base-300 border-gray-300 dark:border-base-300 checked:bg-blue-600 checked:border-blue-600"
                                                checked={appConfig.proxy.allow_lan_access || false}
                                                onChange={(e) => updateProxyConfig({ allow_lan_access: e.target.checked })}
                                            />
                                        </div>
                                        <p className="text-xs text-gray-500 dark:text-gray-400">
                                            {(appConfig.proxy.allow_lan_access || false)
                                                ? t('proxy.config.allow_lan_access_hint_enabled')
                                                : t('proxy.config.allow_lan_access_hint_disabled')}
                                        </p>
                                        {(appConfig.proxy.allow_lan_access || false) && (
                                            <p className="text-xs text-amber-600 dark:text-amber-400 font-medium">
                                                {t('proxy.config.allow_lan_access_warning')}
                                            </p>
                                        )}
                                        {status.running && (
                                            <p className="text-xs text-blue-600 dark:text-blue-400 font-medium">
                                                {t('proxy.config.allow_lan_access_restart_hint')}
                                            </p>
                                        )}
                                    </div>

                                    {/* 访问授权 */}
                                    <div className="space-y-1.5">
                                        <div className="flex items-center justify-between">
                                            <label className="text-xs font-semibold text-gray-700 dark:text-gray-200">
                                                <span className="inline-flex items-center gap-1">
                                                    {t('proxy.config.auth.title')}
                                                    <HelpTooltip
                                                        text={t('proxy.config.auth.title_tooltip')}
                                                        ariaLabel={t('proxy.config.auth.title')}
                                                        placement="top"
                                                    />
                                                </span>
                                            </label>
                                            <label className="flex items-center cursor-pointer gap-2">
                                                <span className="text-xs text-gray-600 dark:text-gray-300 inline-flex items-center gap-1">
                                                    {(appConfig.proxy.auth_mode || 'off') !== 'off' ? t('proxy.config.auth.enabled') : t('common.disabled')}
                                                    <HelpTooltip
                                                        text={t('proxy.config.auth.enabled_tooltip')}
                                                        ariaLabel={t('proxy.config.auth.enabled')}
                                                        placement="left"
                                                    />
                                                </span>
                                                <input
                                                    type="checkbox"
                                                    className="toggle toggle-sm bg-gray-200 dark:bg-base-300 border-gray-300 dark:border-base-300 checked:bg-blue-600 checked:border-blue-600 disabled:opacity-50 disabled:bg-gray-100 dark:disabled:bg-base-200"
                                                    checked={(appConfig.proxy.auth_mode || 'off') !== 'off'}
                                                    onChange={(e) => {
                                                        const nextMode = e.target.checked ? 'all_except_health' : 'off';
                                                        updateProxyConfig({ auth_mode: nextMode });
                                                    }}
                                                />
                                            </label>
                                        </div>

                                        <div>
                                            <label className="block text-xs text-gray-600 dark:text-gray-400 mb-1">
                                                <span className="inline-flex items-center gap-1">
                                                    {t('proxy.config.auth.mode')}
                                                    <HelpTooltip
                                                        text={t('proxy.config.auth.mode_tooltip')}
                                                        ariaLabel={t('proxy.config.auth.mode')}
                                                        placement="top"
                                                    />
                                                </span>
                                            </label>
                                            <select
                                                value={appConfig.proxy.auth_mode || 'off'}
                                                onChange={(e) =>
                                                    updateProxyConfig({
                                                        auth_mode: e.target.value as ProxyConfig['auth_mode'],
                                                    })
                                                }
                                                className="w-full px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-xs text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500/20 focus:border-blue-500"
                                            >
                                                <option value="off">{t('proxy.config.auth.modes.off')}</option>
                                                <option value="strict">{t('proxy.config.auth.modes.strict')}</option>
                                                <option value="all_except_health">{t('proxy.config.auth.modes.all_except_health')}</option>
                                                <option value="auto">{t('proxy.config.auth.modes.auto')}</option>
                                            </select>
                                            <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                                                {t('proxy.config.auth.hint')}
                                            </p>
                                        </div>
                                    </div>
                                </div>
                            </div>

                            {/* API 密钥 */}
                            <div className="border-t border-gray-100 dark:border-base-200 pt-3.5 mt-3.5">
                                <label className="block text-xs font-semibold text-gray-700 dark:text-gray-200 mb-1.5">
                                    <span className="inline-flex items-center gap-1">
                                        {t('proxy.config.api_key')}
                                        <HelpTooltip
                                            text={t('proxy.config.api_key_tooltip')}
                                            ariaLabel={t('proxy.config.api_key')}
                                            placement="right"
                                        />
                                    </span>
                                </label>
                                <div className="flex gap-2">
                                    <input
                                        type="text"
                                        value={isEditingApiKey ? tempApiKey : (appConfig.proxy.api_key)}
                                        onChange={(e) => isEditingApiKey && setTempApiKey(e.target.value)}
                                        readOnly={!isEditingApiKey}
                                        className={`flex-1 px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg text-xs font-mono transition-colors ${isEditingApiKey
                                            ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500/20 focus:border-blue-500'
                                            : 'bg-gray-50 dark:bg-base-200 text-gray-700 dark:text-gray-300'
                                            }`}
                                    />
                                    {isEditingApiKey ? (
                                        <>
                                            <button
                                                onClick={handleSaveApiKey}
                                                className="px-2.5 py-1.5 border border-emerald-300 dark:border-emerald-700 rounded-lg bg-emerald-50 dark:bg-emerald-950/50 hover:bg-emerald-100 dark:hover:bg-emerald-900/50 transition-colors text-emerald-600 dark:text-emerald-400 shadow-2xs"
                                                title={t('proxy.config.btn_save')}
                                            >
                                                <CheckCircle size={15} />
                                            </button>
                                            <button
                                                onClick={handleCancelEditApiKey}
                                                className="px-2.5 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors text-gray-600 dark:text-gray-300 shadow-2xs"
                                                title={t('common.cancel')}
                                            >
                                                <X size={15} />
                                            </button>
                                        </>
                                    ) : (
                                        <>
                                            <button
                                                onClick={handleEditApiKey}
                                                className="px-2.5 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors text-gray-600 dark:text-gray-300 shadow-2xs"
                                                title={t('proxy.config.btn_edit')}
                                            >
                                                <Edit2 size={15} />
                                            </button>
                                            <button
                                                onClick={handleGenerateApiKey}
                                                className="px-2.5 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors text-gray-600 dark:text-gray-300 shadow-2xs"
                                                title={t('proxy.config.btn_regenerate')}
                                            >
                                                <RefreshCw size={15} />
                                            </button>
                                            <button
                                                onClick={() => copyToClipboardHandler(appConfig.proxy.api_key, 'api_key')}
                                                className="px-2.5 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors text-gray-600 dark:text-gray-300 shadow-2xs"
                                                title={t('proxy.config.btn_copy')}
                                            >
                                                {copied === 'api_key' ? (
                                                    <CheckCircle size={15} className="text-emerald-500" />
                                                ) : (
                                                    <Copy size={15} />
                                                )}
                                            </button>
                                        </>
                                    )}
                                </div>
                                <p className="mt-1 text-xs text-amber-600 dark:text-amber-400 font-medium">
                                    {t('proxy.config.warning_key')}
                                </p>
                            </div>

                            {/* Web UI 管理密码 */}
                            <div className="border-t border-gray-100 dark:border-base-200 pt-3.5 mt-3.5">
                                <label className="block text-xs font-semibold text-gray-700 dark:text-gray-200 mb-1.5">
                                    <span className="inline-flex items-center gap-1">
                                        {t('proxy.config.admin_password', { defaultValue: 'Web UI Login Password' })}
                                        <HelpTooltip
                                            text={t('proxy.config.admin_password_tooltip', { defaultValue: 'Used for logging into the Web Management Console. If empty, it defaults to the API Key.' })}
                                            ariaLabel={t('proxy.config.admin_password')}
                                            placement="right"
                                        />
                                    </span>
                                </label>
                                <div className="flex gap-2">
                                    <input
                                        type="text"
                                        value={isEditingAdminPassword ? tempAdminPassword : (appConfig.proxy.admin_password || t('proxy.config.admin_password_default', { defaultValue: '(Same as API Key)' }))}
                                        onChange={(e) => isEditingAdminPassword && setTempAdminPassword(e.target.value)}
                                        readOnly={!isEditingAdminPassword}
                                        placeholder={t('proxy.config.admin_password_placeholder', { defaultValue: 'Enter new password or leave empty to use API Key' })}
                                        className={`flex-1 px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg text-xs font-mono transition-colors ${isEditingAdminPassword
                                            ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500/20 focus:border-blue-500'
                                            : 'bg-gray-50 dark:bg-base-200 text-gray-700 dark:text-gray-300'
                                            }`}
                                    />
                                    {isEditingAdminPassword ? (
                                        <>
                                            <button
                                                onClick={handleSaveAdminPassword}
                                                className="px-2.5 py-1.5 border border-emerald-300 dark:border-emerald-700 rounded-lg bg-emerald-50 dark:bg-emerald-950/50 hover:bg-emerald-100 dark:hover:bg-emerald-900/50 transition-colors text-emerald-600 dark:text-emerald-400 shadow-2xs"
                                                title={t('proxy.config.btn_save')}
                                            >
                                                <CheckCircle size={15} />
                                            </button>
                                            <button
                                                onClick={handleCancelEditAdminPassword}
                                                className="px-2.5 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors text-gray-600 dark:text-gray-300 shadow-2xs"
                                                title={t('common.cancel')}
                                            >
                                                <X size={15} />
                                            </button>
                                        </>
                                    ) : (
                                        <>
                                            <button
                                                onClick={handleEditAdminPassword}
                                                className="px-2.5 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors text-gray-600 dark:text-gray-300 shadow-2xs"
                                                title={t('proxy.config.btn_edit')}
                                            >
                                                <Edit2 size={15} />
                                            </button>
                                            <button
                                                onClick={() => copyToClipboardHandler(appConfig.proxy.admin_password || appConfig.proxy.api_key, 'admin_password')}
                                                className="px-2.5 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors text-gray-600 dark:text-gray-300 shadow-2xs"
                                                title={t('proxy.config.btn_copy')}
                                            >
                                                {copied === 'admin_password' ? (
                                                    <CheckCircle size={15} className="text-emerald-500" />
                                                ) : (
                                                    <Copy size={15} />
                                                )}
                                            </button>
                                        </>
                                    )}
                                </div>
                                <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                                    {t('proxy.config.admin_password_hint', { defaultValue: 'For safety in Docker/Browser environments, you can set a separate login password from your API Key.' })}
                                </p>
                            </div>

                            {/* User-Agent Overrides */}
                            <div className="border-t border-gray-100 dark:border-base-200 pt-3.5 mt-3.5">
                                <div className="flex items-center justify-between mb-2">
                                    <label className="text-xs font-semibold text-gray-700 dark:text-gray-200 inline-flex items-center gap-1">
                                        {t('proxy.config.request.user_agent', { defaultValue: 'User-Agent Override' })}
                                        <HelpTooltip text={t('proxy.config.request.user_agent_tooltip', { defaultValue: 'Override the User-Agent header sent to upstream APIs.' })} />
                                    </label>
                                    <input
                                        type="checkbox"
                                        className="toggle toggle-sm bg-gray-200 dark:bg-base-300 border-gray-300 dark:border-base-300 checked:bg-blue-600 checked:border-blue-600"
                                        checked={!!appConfig.proxy.user_agent_override}
                                        onChange={(e) => {
                                            const enabled = e.target.checked;
                                            if (enabled) {
                                                // 恢复保存的覆盖值，或默认使用官方现代 Hub 指纹
                                                let restoredValue = appConfig.proxy.saved_user_agent || DEFAULT_RECOMMENDED_HUB_UA;
                                                // 如果旧配置遗留了 4.3.0 或非 Hub 指纹，升级为官方 Hub 默认
                                                if (!restoredValue.startsWith('antigravity/hub/')) {
                                                    restoredValue = DEFAULT_RECOMMENDED_HUB_UA;
                                                }
                                                updateProxyConfig({
                                                    user_agent_override: restoredValue,
                                                    saved_user_agent: restoredValue
                                                });
                                            } else {
                                                // Disable active override but keep saved value
                                                updateProxyConfig({ user_agent_override: undefined });
                                            }
                                        }}
                                    />
                                </div>

                                {!!appConfig.proxy.user_agent_override && (
                                    <div className="space-y-2.5 animate-in fade-in slide-in-from-top-1 duration-200">
                                        <input
                                            type="text"
                                            value={appConfig.proxy.user_agent_override}
                                            onChange={(e) => {
                                                const newValue = e.target.value;
                                                updateProxyConfig({
                                                    user_agent_override: newValue,
                                                    saved_user_agent: newValue
                                                });
                                            }}
                                            className="w-full px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-xs font-mono text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500/20 focus:border-blue-500"
                                            placeholder={t('proxy.config.request.user_agent_placeholder', { defaultValue: 'Enter custom User-Agent string...' })}
                                        />
                                        <div className="bg-gray-50 dark:bg-base-200 border border-gray-200 dark:border-base-300 rounded-lg p-2.5 text-xs text-gray-600 dark:text-gray-400 font-mono break-all flex items-center justify-between">
                                            <div className="truncate mr-2">
                                                <span className="font-bold text-gray-700 dark:text-gray-300 select-none mr-2">{t('common.example', { defaultValue: 'Example' })}:</span>
                                                {latestHubUa}
                                            </div>
                                            <button
                                                type="button"
                                                onClick={() => {
                                                    updateProxyConfig({
                                                        user_agent_override: latestHubUa,
                                                        saved_user_agent: latestHubUa
                                                    });
                                                    showToast(t('proxy.config.request.user_agent_restored_success', { defaultValue: '已恢复官方默认 Hub User-Agent' }), 'success');
                                                }}
                                                className="px-2 py-0.5 text-[10px] bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 rounded hover:bg-blue-100 dark:hover:bg-blue-900/50 shrink-0 font-sans"
                                            >
                                                填入推荐值
                                            </button>
                                        </div>

                                        {/* 风险提示卡片 */}
                                        <div className="p-3 bg-amber-50/70 dark:bg-amber-950/20 border border-amber-200 dark:border-amber-800/40 rounded-xl flex items-start gap-2.5">
                                            <AlertTriangle size={16} className="text-amber-600 dark:text-amber-400 shrink-0 mt-0.5" />
                                            <div className="text-xs text-amber-900 dark:text-amber-200 space-y-1">
                                                <div className="font-semibold flex items-center gap-1.5">
                                                    {t('proxy.config.request.user_agent_risk_title', { defaultValue: '风险提示' })}
                                                </div>
                                                <p className="leading-relaxed opacity-90 text-[11px]">
                                                    {t('proxy.config.request.user_agent_risk_desc', {
                                                        version: latestHubVersion,
                                                        defaultValue: `自定义 User-Agent 可能会影响出站请求的协议兼容性。Google Cloud Code 上游对新一代旗舰模型（如 Gemini 4 Argon 等）实施了严格的客户端指纹门禁。若填入非 Hub 签名格式（如 antigravity/4.3.0 等旧 IDE 指纹）或版本过旧（低于官方最新基准 ${latestHubVersion}），上游将直接返回 HTTP 404 (Requested entity was not found)，并可能导致网关账号池被临时保护性熔断挂起 15 分钟。若不了解上游指纹协议细节，请谨慎操作或保持默认。`
                                                    })}
                                                </p>
                                            </div>
                                        </div>

                                        {/* 🧪 User-Agent & Argon 门禁沙盒联调测试组件 */}
                                        <div className="mt-3 p-3 bg-blue-50/40 dark:bg-blue-950/20 border border-blue-200/60 dark:border-blue-900/40 rounded-xl space-y-2.5">
                                            <div className="flex items-center justify-between">
                                                <div className="flex items-center gap-2">
                                                    <span className="text-xs font-bold text-blue-700 dark:text-blue-300 flex items-center gap-1.5">
                                                        <Sparkles size={14} className="text-blue-500" />
                                                        {t('proxy.config.request.user_agent_sandbox_test_title', { defaultValue: 'User-Agent 与 Argon 门禁沙盒联调测试' })}
                                                    </span>
                                                    <span className="text-[10px] px-1.5 py-0.5 rounded bg-blue-100 dark:bg-blue-900/40 text-blue-600 dark:text-blue-300">
                                                        实测组件
                                                    </span>
                                                </div>
                                            </div>

                                            <p className="text-[11px] text-gray-600 dark:text-gray-400">
                                                {t('proxy.config.request.user_agent_sandbox_test_desc', { defaultValue: '可实时模拟不同 User-Agent 指纹并向网关发送 Argon 连通性测试请求，观察网关的定向检测与诊断输出。' })}
                                            </p>

                                            {/* 快速指纹预设按钮 */}
                                            <div className="flex flex-wrap items-center gap-1.5">
                                                <span className="text-[10px] font-semibold text-gray-500 select-none">一键模拟:</span>
                                                <button
                                                    type="button"
                                                    onClick={() => handleRunUaSandboxTest('antigravity/4.3.0 darwin/arm64', 'gemini-4-argon')}
                                                    disabled={uaSandboxTesting}
                                                    className="px-2 py-1 text-[11px] bg-rose-50 hover:bg-rose-100 dark:bg-rose-950/30 dark:hover:bg-rose-900/40 text-rose-700 dark:text-rose-300 border border-rose-200 dark:border-rose-900/40 rounded-lg flex items-center gap-1 transition-all active:scale-95"
                                                >
                                                    <span>⚠️ 注入旧版 4.3.0 指纹 (触发 404 门禁诊断)</span>
                                                </button>
                                                <button
                                                    type="button"
                                                    onClick={() => handleRunUaSandboxTest(latestHubUa, 'gemini-4-argon')}
                                                    disabled={uaSandboxTesting}
                                                    className="px-2 py-1 text-[11px] bg-emerald-50 hover:bg-emerald-100 dark:bg-emerald-950/30 dark:hover:bg-emerald-900/40 text-emerald-700 dark:text-emerald-300 border border-emerald-200 dark:border-emerald-900/40 rounded-lg flex items-center gap-1 transition-all active:scale-95"
                                                >
                                                    <span>✅ 注入官方最新 Hub 指纹 (正常连通)</span>
                                                </button>
                                                <button
                                                    type="button"
                                                    onClick={() => handleRunUaSandboxTest('antigravity/hub/2.24.0 (aidev_client; os_type=windows; arch=amd64; cl=1008300195)', 'gemini-4-argon')}
                                                    disabled={uaSandboxTesting}
                                                    className="px-2 py-1 text-[11px] bg-blue-50 hover:bg-blue-100 dark:bg-blue-950/30 dark:hover:bg-blue-900/40 text-blue-700 dark:text-blue-300 border border-blue-200 dark:border-blue-900/40 rounded-lg flex items-center gap-1 transition-all active:scale-95"
                                                >
                                                    <span>⚡ 注入官方 Hub 2.24.0 (基准通过)</span>
                                                </button>
                                                <button
                                                    type="button"
                                                    onClick={() => handleRunUaSandboxTest(undefined, 'simulate_no_response')}
                                                    disabled={uaSandboxTesting}
                                                    className="px-2 py-1 text-[11px] bg-purple-50 hover:bg-purple-100 dark:bg-purple-950/30 dark:hover:bg-purple-900/40 text-purple-700 dark:text-purple-300 border border-purple-200 dark:border-purple-900/40 rounded-lg flex items-center gap-1 transition-all active:scale-95"
                                                >
                                                    <span>🌐 模拟无请求回报 (网络超时/上游未响应)</span>
                                                </button>
                                            </div>

                                            {/* 测试操作栏 */}
                                            <div className="flex items-center gap-2 pt-1">
                                                <select
                                                    value={uaSandboxModel}
                                                    onChange={(e) => setUaSandboxModel(e.target.value)}
                                                    className="select select-xs border-gray-300 dark:border-base-300 bg-white dark:bg-base-100 text-xs rounded-lg flex-1 font-mono"
                                                >
                                                    <option value="gemini-4-argon">gemini-4-argon (旗舰高阶推理)</option>
                                                    <option value="gemini-4-argon-low">gemini-4-argon-low (轻量推理)</option>
                                                    <option value="simulate_no_response">[模拟故障] 无请求回报 (上游网络超时与阶段中断)</option>
                                                    <option value="claude-opus-5-5">claude-opus-5-5</option>
                                                    <option value="claude-sonnet-4-6">claude-sonnet-4-6</option>
                                                    <option value="gemini-2.5-pro">gemini-2.5-pro</option>
                                                </select>

                                                <button
                                                    type="button"
                                                    onClick={() => handleRunUaSandboxTest()}
                                                    disabled={uaSandboxTesting}
                                                    className="btn btn-xs btn-primary text-white font-medium flex items-center gap-1 px-3 shadow-xs"
                                                >
                                                    {uaSandboxTesting ? (
                                                        <>
                                                            <RefreshCw size={12} className="animate-spin" />
                                                            正在发送测试...
                                                        </>
                                                    ) : (
                                                        <>
                                                            <Zap size={12} />
                                                            发送测试请求
                                                        </>
                                                    )}
                                                </button>
                                            </div>

                                            {/* 测试结果看板 */}
                                            {uaSandboxResult && (
                                                <div className="p-3 bg-white dark:bg-base-100 rounded-lg border border-gray-200 dark:border-base-300 space-y-2 text-xs animate-in fade-in duration-200">
                                                    <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-base-200">
                                                        <div className="flex items-center gap-2">
                                                            <span className="font-semibold text-gray-700 dark:text-gray-300">测试结果:</span>
                                                            <span
                                                                className={`px-2 py-0.5 rounded text-[11px] font-bold ${
                                                                    uaSandboxResult.success
                                                                        ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-950/50 dark:text-emerald-300'
                                                                        : uaSandboxResult.status === 502 || uaSandboxResult.gatewayCode === 'upstream_network_error'
                                                                        ? 'bg-purple-100 text-purple-700 dark:bg-purple-950/50 dark:text-purple-300'
                                                                        : 'bg-rose-100 text-rose-700 dark:bg-rose-950/50 dark:text-rose-300'
                                                                }`}
                                                            >
                                                                {uaSandboxResult.success ? 'HTTP 200 OK' : uaSandboxResult.status === 502 ? 'HTTP 502 (无请求回报)' : `HTTP ${uaSandboxResult.status || 'ERROR'}`}
                                                            </span>
                                                            {uaSandboxResult.durationMs !== undefined && (
                                                                <span className="text-[10px] text-gray-400">
                                                                    耗时: {uaSandboxResult.durationMs}ms
                                                                </span>
                                                            )}
                                                        </div>
                                                        <span className="text-[10px] font-mono text-gray-400">
                                                            模型: {uaSandboxResult.model}
                                                        </span>
                                                    </div>

                                                    <div className="text-[11px] text-gray-600 dark:text-gray-400 font-mono break-all">
                                                        <span className="font-bold text-gray-500 mr-1 select-none">出站指纹:</span>
                                                        {uaSandboxResult.testedUa}
                                                    </div>

                                                    {/* 网关双轨制定向诊断 (Highlight) */}
                                                    {uaSandboxResult.gatewayDiagnosis ? (
                                                        <div className="p-2.5 bg-amber-50 dark:bg-amber-950/30 border border-amber-300 dark:border-amber-800/60 rounded-lg space-y-1.5">
                                                            <div className="text-amber-800 dark:text-amber-200 font-bold flex items-center gap-1.5 text-xs">
                                                                <AlertTriangle size={14} className="text-amber-500" />
                                                                网关双轨制定向诊断 ({uaSandboxResult.gatewayCode || 'gateway_error'})
                                                            </div>
                                                            <div className="text-amber-900 dark:text-amber-100 text-[11px] leading-relaxed">
                                                                {uaSandboxResult.gatewayDiagnosis}
                                                            </div>
                                                            {uaSandboxResult.gatewaySuggestion && (
                                                                <div className="text-amber-700 dark:text-amber-300 text-[11px] pt-1 border-t border-amber-200 dark:border-amber-800/40">
                                                                    <span className="font-semibold">💡 修复建议: </span>
                                                                    {uaSandboxResult.gatewaySuggestion}
                                                                </div>
                                                            )}
                                                        </div>
                                                    ) : uaSandboxResult.success ? (
                                                        <div className="p-2 bg-emerald-50 dark:bg-emerald-950/30 border border-emerald-200 dark:border-emerald-800/40 rounded-lg text-emerald-800 dark:text-emerald-200 text-[11px] flex items-center gap-1.5">
                                                            <CheckCircle size={14} className="text-emerald-500 shrink-0" />
                                                            连通性测试通过！出站 User-Agent 指纹符合 Google Cloud Code 官方门禁规范。
                                                        </div>
                                                    ) : (
                                                        <div className="p-2 bg-gray-50 dark:bg-base-200 rounded-lg text-gray-700 dark:text-gray-300 text-[11px] font-mono break-all">
                                                            {uaSandboxResult.error || uaSandboxResult.message || '未知错误'}
                                                        </div>
                                                    )}
                                                </div>
                                            )}
                                        </div>
                                    </div>
                                )}
                            </div>


                            </div>
                        )}

                        {/* TAB: 模型配置 (models) */}
                        {activeMenuTab === 'models' && (
                            <div className="p-4 space-y-4">
                                {/* 超时设置 (全链路分阶段) - 默认收起 */}
                                <CollapsibleCard
                                    title={t('proxy.config.timeout_settings.title', { defaultValue: '超时设置' })}
                                    icon={<Timer size={18} className="text-blue-500" />}
                                    defaultExpanded={false}
                                >
                                    <TimeoutSettings
                                        streamConfig={appConfig.proxy.stream_timeouts}
                                        uploadConfig={appConfig.proxy.upload_timeouts}
                                        onStreamChange={(timeoutConfig) => updateProxyConfig({ stream_timeouts: timeoutConfig })}
                                        onUploadChange={(timeoutConfig) => updateProxyConfig({ upload_timeouts: timeoutConfig })}
                                        onSave={handleSaveProxySettings}
                                    />
                                </CollapsibleCard>

                                {/* 思考设置 (Thinking & Reasoning Settings) - 默认收起 */}
                                <CollapsibleCard
                                    title={t('proxy.config.thinking_settings.title', { defaultValue: '思考设置 (Thinking Settings)' })}
                                    icon={<BrainCircuit size={18} className="text-purple-500" />}
                                    defaultExpanded={false}
                                >
                                    <ThinkingBudget
                                        config={appConfig.proxy.thinking_budget}
                                        onChange={(tbConfig) => updateProxyConfig({ thinking_budget: tbConfig })}
                                        onSave={handleSaveProxySettings}
                                        thinkingStoreEnabled={appConfig.proxy.experimental?.thinking_store_enabled !== false}
                                        onThinkingStoreChange={(enabled) =>
                                            updateExperimentalConfig({ thinking_store_enabled: enabled })
                                        }
                                        thinkingMaxMemoryTurns={appConfig.proxy.experimental?.thinking_max_memory_turns ?? 600}
                                        onThinkingMaxMemoryTurnsChange={(turns: number) =>
                                            updateExperimentalConfig({ thinking_max_memory_turns: turns })
                                        }
                                        thinkingRetentionDays={appConfig.proxy.experimental?.thinking_retention_days ?? 15}
                                        onThinkingRetentionDaysChange={(days: number) =>
                                            updateExperimentalConfig({ thinking_retention_days: days })
                                        }
                                        globalCompactionStripEnabled={appConfig.proxy.experimental?.enable_global_compaction_strip === true}
                                        onGlobalCompactionStripChange={(enabled) =>
                                            updateExperimentalConfig({ enable_global_compaction_strip: enabled })
                                        }
                                    />
                                </CollapsibleCard>

                                {/* 多模态交互设置 (Multimodal Settings) - 默认收起 */}
                                <CollapsibleCard
                                    title={t('proxy.config.multimodal_settings.title', { defaultValue: '多模态交互设置 (Multimodal Settings)' })}
                                    icon={<Sparkles size={18} className="text-pink-500" />}
                                    defaultExpanded={false}
                                >
                                    <MultimodalSettings
                                        config={appConfig.proxy.multimodal}
                                        onChange={(mConfig) => updateProxyConfig({ multimodal: mConfig })}
                                        onSave={handleSaveProxySettings}
                                    />
                                </CollapsibleCard>

                                {/* 特定 Agent 特性配置 (Specific Agent Settings) - 默认收起 */}
                                <CollapsibleCard
                                    title={t('proxy.config.agent_settings.title', { defaultValue: '特定 Agent 配置 (Specific Agent Settings)' })}
                                    icon={<Bot size={18} className="text-cyan-500" />}
                                    defaultExpanded={false}
                                >
                                    <AgentSettings
                                        experimentalConfig={appConfig.proxy.experimental}
                                        onChange={updateExperimentalConfig}
                                        onSave={handleSaveProxySettings}
                                    />
                                </CollapsibleCard>

                                {/* 模型路由中心 紧随其后 */}
                                {renderModelRouterSection()}
                            </div>
                        )}

                        {/* TAB: 风控提示词清理方案 (guard) */}
                        {activeMenuTab === 'guard' && (
                            <div className="p-4">
                                <PromptRewriteSettings
                                    config={appConfig.proxy.prompt_rewrite || { enabled: true, normalize_identity: true, rules: RECOMMENDED_PROMPT_REWRITE_PRESETS }}
                                    onChange={(config) => updateProxyConfig({ prompt_rewrite: config })}
                                />
                            </div>
                        )}

                        {/* TAB 3: CLI 一键配置 (cli) */}
                        {activeMenuTab === 'cli' && (
                            <div className="p-4">
                                <div className="bg-gray-50/60 dark:bg-base-200/50 rounded-xl p-3 border border-gray-200/60 dark:border-base-300">
                                    <CliSyncCard
                                        proxyUrl={status.running ? status.base_url : `http://127.0.0.1:${appConfig.proxy.port || 8045}`}
                                        apiKey={appConfig.proxy.api_key}
                                    />
                                </div>
                            </div>
                        )}

                        {/* TAB 4: 模型大全与协议 (protocols) */}
                        {activeMenuTab === 'protocols' && (
                            <div className="p-4 space-y-6">
                                {renderMultiProtocolSection()}
                                {renderSupportedModelsSection()}
                            </div>
                        )}
                    </div>
                )}

                {/* External Providers Integration - 仅在服务配置 Tab 下展示 */}
                {
                    !configLoading && !configError && appConfig && activeMenuTab === 'settings' && (
                        <div className="space-y-4">
                            {/* Account Scheduling & Rotation */}
                            <CollapsibleCard
                                title={t('proxy.config.scheduling.title')}
                                icon={<RefreshCw size={18} className="text-indigo-500" />}
                            >
                                <div className="space-y-4">
                                    <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                                        <div className="space-y-3">
                                            <div className="flex items-center justify-between">
                                                <label className="text-xs font-medium text-gray-700 dark:text-gray-300 inline-flex items-center gap-1">
                                                    {t('proxy.config.scheduling.mode')}
                                                    <HelpTooltip
                                                        text={t('proxy.config.scheduling.mode_tooltip')}
                                                        placement="right"
                                                    />
                                                </label>
                                                <div className="flex items-center gap-3">
                                                    {/* [MOVED] Clear Rate Limit button moved to CircuitBreaker component */}
                                                    <button
                                                        onClick={handleClearSessionBindings}
                                                        className="text-[10px] text-indigo-500 hover:text-indigo-600 transition-colors flex items-center gap-1"
                                                        title={t('proxy.config.scheduling.clear_bindings_tooltip')}
                                                    >
                                                        <Trash2 size={12} />
                                                        {t('proxy.config.scheduling.clear_bindings')}
                                                    </button>
                                                </div>
                                            </div>
                                            <div className="grid grid-cols-1 gap-2">
                                                {(['CacheFirst', 'Balance', 'PerformanceFirst'] as const).map(mode => (
                                                    <label
                                                        key={mode}
                                                        className={`flex items-start gap-3 p-3 rounded-xl border cursor-pointer transition-all duration-200 ${(appConfig.proxy.scheduling?.mode || 'Balance') === mode
                                                            ? 'border-indigo-500 bg-indigo-50/30 dark:bg-indigo-900/10'
                                                            : 'border-gray-100 dark:border-base-200 hover:border-indigo-200'
                                                            }`}
                                                    >
                                                        <input
                                                            type="radio"
                                                            className="radio radio-xs radio-primary mt-1"
                                                            checked={(appConfig.proxy.scheduling?.mode || 'Balance') === mode}
                                                            onChange={() => updateSchedulingConfig({ mode })}
                                                        />
                                                        <div className="space-y-1">
                                                            <div className="text-xs font-bold text-gray-900 dark:text-base-content">
                                                                {t(`proxy.config.scheduling.modes.${mode}`)}
                                                            </div>
                                                            <div className="text-[10px] text-gray-500 line-clamp-2">
                                                                {t(`proxy.config.scheduling.modes_desc.${mode}`, {
                                                                    defaultValue: mode === 'CacheFirst' ? 'Binds session to account, waits precisely if limited (Maximizes Prompt Cache hits).' :
                                                                        mode === 'Balance' ? 'Binds session, auto-switches to available account if limited (Balanced cache & availability).' :
                                                                            'No session binding, pure round-robin rotation (Best for high concurrency).'
                                                                })}
                                                            </div>
                                                        </div>
                                                    </label>
                                                ))}
                                            </div>
                                        </div>

                                        <div className="space-y-4 pt-1">
                                            <div className="bg-gray-50/70 dark:bg-base-200 rounded-xl p-4 border border-gray-100 dark:border-base-300">
                                                <div className="flex items-center justify-between mb-2">
                                                    <label className="text-xs font-medium text-gray-700 dark:text-gray-300 inline-flex items-center gap-1">
                                                        {t('proxy.config.scheduling.max_wait')}
                                                        <HelpTooltip text={t('proxy.config.scheduling.max_wait_tooltip')} />
                                                    </label>
                                                    <span className="text-xs font-mono text-indigo-600 font-bold">
                                                        {appConfig.proxy.scheduling?.max_wait_seconds || 60}s
                                                    </span>
                                                </div>
                                                <input
                                                    type="range"
                                                    min="0"
                                                    max="300"
                                                    step="10"
                                                    disabled={(appConfig.proxy.scheduling?.mode || 'Balance') !== 'CacheFirst'}
                                                    className="range range-indigo range-xs"
                                                    value={appConfig.proxy.scheduling?.max_wait_seconds || 60}
                                                    onChange={(e) => updateSchedulingConfig({ max_wait_seconds: parseInt(e.target.value) })}
                                                />
                                                <div className="flex justify-between px-1 mt-1 text-[10px] text-gray-400 font-mono">
                                                    <span>0s</span>
                                                    <span>300s</span>
                                                </div>
                                            </div>

                                            <div className="bg-gray-50/70 dark:bg-base-200 rounded-xl p-4 border border-gray-100 dark:border-base-300">
                                                <div className="flex items-center justify-between mb-2">
                                                    <label className="text-xs font-medium text-gray-700 dark:text-gray-300 inline-flex items-center gap-1">
                                                        {t('proxy.config.scheduling.request_429_grace_ms', { defaultValue: '请求级 429 原地等待' })}
                                                        <HelpTooltip text={t('proxy.config.scheduling.request_429_grace_ms_tooltip', { defaultValue: '无 retryDelay 的请求级 429 首轮在当前账号原地等待的时间；默认 1500ms，范围 200–10000ms。' })} />
                                                    </label>
                                                    <span className="text-xs font-mono text-indigo-600 font-bold">
                                                        {appConfig.proxy.scheduling?.request_429_grace_ms ?? 1500}ms
                                                    </span>
                                                </div>
                                                <input
                                                    type="number"
                                                    min={200}
                                                    max={10000}
                                                    step={100}
                                                    value={appConfig.proxy.scheduling?.request_429_grace_ms ?? 1500}
                                                    onChange={(e) => updateSchedulingConfig({ request_429_grace_ms: Math.max(200, Math.min(10000, parseInt(e.target.value) || 1500)) })}
                                                    className="w-full px-3 py-1.5 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-xs font-mono text-gray-900 dark:text-base-content focus:ring-2 focus:ring-indigo-500/20 focus:border-indigo-500 outline-none"
                                                />
                                            </div>

                                            <div className="bg-gray-50/70 dark:bg-base-200 rounded-xl p-4 border border-gray-100 dark:border-base-300">
                                                <div className="flex items-center justify-between mb-2">
                                                    <label className="text-xs font-medium text-gray-700 dark:text-gray-300 inline-flex items-center gap-1">
                                                        {t('proxy.config.scheduling.max_spillover_accounts')}
                                                        <HelpTooltip text={t('proxy.config.scheduling.max_spillover_accounts_tooltip')} />
                                                    </label>
                                                    <span className="text-xs font-mono text-indigo-600 font-bold">
                                                        {(appConfig.proxy.scheduling?.max_spillover_accounts ?? 0) === 0
                                                            ? t('proxy.config.scheduling.spillover_unlimited')
                                                            : t('proxy.config.scheduling.spillover_count', { count: appConfig.proxy.scheduling?.max_spillover_accounts })}
                                                    </span>
                                                </div>
                                                <input
                                                    type="range"
                                                    min="0"
                                                    max="10"
                                                    step="1"
                                                    className="range range-indigo range-xs"
                                                    value={appConfig.proxy.scheduling?.max_spillover_accounts ?? 0}
                                                    onChange={(e) => updateSchedulingConfig({ max_spillover_accounts: parseInt(e.target.value) || 0 })}
                                                />
                                                <div className="flex justify-between px-1 mt-1 text-[10px] text-gray-400 font-mono">
                                                    <span>0 ({t('proxy.config.scheduling.spillover_unlimited')})</span>
                                                    <span>10</span>
                                                </div>
                                            </div>

                                            <div className="bg-gray-50/70 dark:bg-base-200 rounded-xl p-4 border border-gray-100 dark:border-base-300">
                                                <div className="flex items-center justify-between mb-2">
                                                    <label className="text-xs font-medium text-gray-700 dark:text-gray-300 inline-flex items-center gap-1">
                                                        {t('proxy.config.scheduling.affinity_scope', { defaultValue: 'Affinity Scope' })}
                                                        <HelpTooltip text={t('proxy.config.scheduling.affinity_scope_tooltip', { defaultValue: 'Session: Binds by session ID (default, auto-normalizes multi-agent subtasks); API-Key: Binds all requests from the same API-Key to a single account (prevents prompt cache misses in multi-agent frameworks).' })} />
                                                    </label>
                                                    <div className="flex bg-slate-200/70 dark:bg-base-300 p-0.5 rounded-lg border border-slate-300/50 dark:border-white/5">
                                                        <button
                                                            type="button"
                                                            onClick={() => updateSchedulingConfig({ affinity_scope: 'session' })}
                                                            className={`px-2.5 py-1 text-[11px] font-medium rounded-md transition-all ${
                                                                (appConfig.proxy.scheduling?.affinity_scope || 'session') === 'session'
                                                                    ? 'bg-white dark:bg-base-100 text-indigo-600 dark:text-indigo-400 shadow-sm'
                                                                    : 'text-gray-500 hover:text-gray-800 dark:hover:text-gray-200'
                                                            }`}
                                                        >
                                                            {t('proxy.config.scheduling.affinity_scopes.session', { defaultValue: 'Session ID' })}
                                                        </button>
                                                        <button
                                                            type="button"
                                                            onClick={() => updateSchedulingConfig({ affinity_scope: 'api_key' })}
                                                            className={`px-2.5 py-1 text-[11px] font-medium rounded-md transition-all ${
                                                                appConfig.proxy.scheduling?.affinity_scope === 'api_key'
                                                                    ? 'bg-white dark:bg-base-100 text-indigo-600 dark:text-indigo-400 shadow-sm'
                                                                    : 'text-gray-500 hover:text-gray-800 dark:hover:text-gray-200'
                                                            }`}
                                                        >
                                                            {t('proxy.config.scheduling.affinity_scopes.api_key', { defaultValue: 'API-Key' })}
                                                        </button>
                                                    </div>
                                                </div>
                                                <p className="text-[10px] text-gray-500 leading-relaxed">
                                                    {(appConfig.proxy.scheduling?.affinity_scope || 'session') === 'api_key'
                                                        ? t('proxy.config.scheduling.affinity_scopes_desc.api_key', { defaultValue: 'Locks all requests from the same API-Key to one upstream account, maximizing cache hits for AutoGPT/Cindy multi-agent fleets.' })
                                                        : t('proxy.config.scheduling.affinity_scopes_desc.session', { defaultValue: 'Binds accounts by session ID with automatic subtask prefix normalization (e.g. :step:N).' })}
                                                </p>
                                            </div>

                                            <div className="p-3 bg-amber-50 dark:bg-amber-900/10 border border-amber-100 dark:border-amber-900/20 rounded-xl">
                                                <p className="text-[10px] text-amber-700 dark:text-amber-500 leading-relaxed">
                                                    <strong>{t('common.info')}:</strong> {t('proxy.config.scheduling.subtitle')}
                                                </p>
                                            </div>

                                            {/* [FIX #820] Fixed Account Mode */}
                                            <div className="bg-indigo-50 dark:bg-indigo-900/20 rounded-xl p-4 border border-indigo-200 dark:border-indigo-800">
                                                <div className="flex items-center justify-between mb-3">
                                                    <label className="text-xs font-medium text-gray-700 dark:text-gray-300 inline-flex items-center gap-1">
                                                        🔒 {t('proxy.config.scheduling.fixed_account', { defaultValue: 'Fixed Account Mode' })}
                                                        <HelpTooltip text={t('proxy.config.scheduling.fixed_account_tooltip', { defaultValue: 'When enabled, all API requests will use only the selected account instead of rotating between accounts.' })} />
                                                    </label>
                                                    <input
                                                        type="checkbox"
                                                        className="toggle toggle-sm toggle-primary"
                                                        checked={preferredAccountId !== null}
                                                        onChange={(e) => {
                                                            if (e.target.checked) {
                                                                // Enable fixed mode with first available account
                                                                if (availableAccounts.length > 0) {
                                                                    handleSetPreferredAccount(availableAccounts[0].id);
                                                                }
                                                            } else {
                                                                // Disable fixed mode
                                                                handleSetPreferredAccount(null);
                                                            }
                                                        }}
                                                        disabled={!status.running}
                                                    />
                                                </div>
                                                {preferredAccountId !== null && (
                                                    <select
                                                        className="select select-bordered select-sm w-full text-xs"
                                                        value={preferredAccountId || ''}
                                                        onChange={(e) => handleSetPreferredAccount(e.target.value || null)}
                                                        disabled={!status.running}
                                                    >
                                                        {availableAccounts.map(account => (
                                                            <option key={account.id} value={account.id}>
                                                                {account.email}
                                                            </option>
                                                        ))}
                                                    </select>
                                                )}
                                                {!status.running && (
                                                    <p className="text-[10px] text-gray-500 mt-2">
                                                        {t('proxy.config.scheduling.start_proxy_first', { defaultValue: 'Start the proxy service to configure fixed account mode.' })}
                                                    </p>
                                                )}
                                            </div>
                                        </div>
                                    </div>

                                    {/* Circuit Breaker Section */}
                                    {appConfig.circuit_breaker && (
                                        <div className="pt-4 border-t border-gray-100 dark:border-gray-700/50">
                                            <div className="flex items-center justify-between mb-4">
                                                <label className="text-xs font-medium text-gray-700 dark:text-gray-300 inline-flex items-center gap-1">
                                                    {t('proxy.config.circuit_breaker.title', { defaultValue: 'Adaptive Circuit Breaker' })}
                                                    <HelpTooltip text={t('proxy.config.circuit_breaker.tooltip', { defaultValue: 'Prevent continuous failures by exponentially backing off when quota is exhausted.' })} />
                                                </label>
                                                <input
                                                    type="checkbox"
                                                    className="toggle toggle-sm toggle-warning"
                                                    checked={appConfig.circuit_breaker.enabled}
                                                    onChange={(e) => updateCircuitBreakerConfig({ ...appConfig.circuit_breaker, enabled: e.target.checked })}
                                                />
                                            </div>

                                            {appConfig.circuit_breaker.enabled && (
                                                <CircuitBreaker
                                                    config={appConfig.circuit_breaker}
                                                    onChange={updateCircuitBreakerConfig}
                                                    onClearRateLimits={handleClearRateLimits}
                                                />
                                            )}
                                        </div>
                                    )}
                                </div>
                            </CollapsibleCard>

                            {/* 全局提示词与多模态设置 (Global Prompt & Multimodal Config) */}
                            <CollapsibleCard
                                title={t('proxy.config.prompt_multimodal.title', { defaultValue: '全局提示词与多模态设置 (Prompt & Multimodal)' })}
                                icon={<Sparkles size={18} className="text-pink-500" />}
                            >
                                <div className="space-y-4">
                                    {/* 图像思维模式 */}
                                    <ImageThinkingMode
                                        value={appConfig.proxy.image_thinking_mode || 'enabled'}
                                        onChange={(newValue) => updateProxyConfig({ image_thinking_mode: newValue })}
                                    />
                                    {/* 全局系统提示词 */}
                                    <div className="pt-4 border-t border-gray-100 dark:border-base-300">
                                        <GlobalSystemPrompt
                                            config={appConfig.proxy.global_system_prompt || { enabled: false, content: '' }}
                                            onChange={(newConfig) => updateProxyConfig({ global_system_prompt: newConfig })}
                                        />
                                    </div>
                                </div>
                            </CollapsibleCard>

                            {/* 公网访问 (Cloudflared) - 仅在桌面端显示 */}
                            {isTauri() && (
                                <CollapsibleCard
                                    title={t('proxy.cloudflared.title', { defaultValue: 'Public Access (Cloudflared)' })}
                                    icon={<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-orange-500"><path d="M12 2L2 7l10 5 10-5-10-5z" /><path d="M2 17l10 5 10-5" /><path d="M2 12l10 5 10-5" /></svg>}
                                    enabled={cfStatus.running}
                                    onToggle={handleCfToggle}
                                    allowInteractionWhenDisabled={true}
                                    rightElement={
                                        cfLoading ? (
                                            <span className="loading loading-spinner loading-xs"></span>
                                        ) : cfStatus.running && cfStatus.url ? (
                                            <button
                                                onClick={(e) => { e.stopPropagation(); handleCfCopyUrl(); }}
                                                className="text-xs px-2 py-1 rounded bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400 hover:bg-green-200 dark:hover:bg-green-900/50 transition-colors flex items-center gap-1"
                                            >
                                                {copied === 'cf-url' ? <CheckCircle size={12} /> : <Copy size={12} />}
                                                {cfStatus.url.replace('https://', '').slice(0, 20)}...
                                            </button>
                                        ) : null
                                    }
                                >
                                    <div className="space-y-4">
                                        {/* 安装状态 */}
                                        {!cfStatus.installed ? (
                                            <div className="flex items-center justify-between p-4 bg-yellow-50 dark:bg-yellow-900/20 rounded-xl border border-yellow-200 dark:border-yellow-800">
                                                <div className="space-y-1">
                                                    <span className="text-sm font-bold text-yellow-800 dark:text-yellow-200">
                                                        {t('proxy.cloudflared.not_installed', { defaultValue: 'Cloudflared not installed' })}
                                                    </span>
                                                    <p className="text-xs text-yellow-600 dark:text-yellow-400">
                                                        {t('proxy.cloudflared.install_hint', { defaultValue: 'Click to download and install cloudflared binary' })}
                                                    </p>
                                                </div>
                                                <button
                                                    onClick={handleCfInstall}
                                                    disabled={cfLoading}
                                                    className="px-4 py-2 rounded-lg text-sm font-medium bg-yellow-500 text-white hover:bg-yellow-600 disabled:opacity-50 flex items-center gap-2"
                                                >
                                                    {cfLoading ? <span className="loading loading-spinner loading-xs"></span> : null}
                                                    {t('proxy.cloudflared.install', { defaultValue: 'Install' })}
                                                </button>
                                            </div>
                                        ) : (
                                            <>
                                                {/* 版本信息 */}
                                                <div className="flex items-center gap-2 text-xs text-gray-500 dark:text-gray-400">
                                                    <CheckCircle size={14} className="text-green-500" />
                                                    {t('proxy.cloudflared.installed', { defaultValue: 'Installed' })}: {cfStatus.version || 'Unknown'}
                                                </div>

                                                {/* 隧道模式选择 */}
                                                <div className="grid grid-cols-2 gap-3">
                                                    <button
                                                        onClick={() => {
                                                            setCfMode('quick');
                                                            if (appConfig) {
                                                                saveConfig({
                                                                    ...appConfig,
                                                                    cloudflared: { ...appConfig.cloudflared, mode: 'quick' }
                                                                });
                                                            }
                                                        }}
                                                        disabled={cfStatus.running}
                                                        className={cn(
                                                            "p-3 rounded-lg border-2 text-left transition-all",
                                                            cfMode === 'quick'
                                                                ? "border-orange-500 bg-orange-50 dark:bg-orange-900/20"
                                                                : "border-gray-200 dark:border-gray-700 hover:border-gray-300 dark:hover:border-gray-600",
                                                            cfStatus.running && "opacity-60 cursor-not-allowed"
                                                        )}
                                                    >
                                                        <div className="text-sm font-bold text-gray-900 dark:text-base-content">
                                                            {t('proxy.cloudflared.mode_quick', { defaultValue: 'Quick Tunnel' })}
                                                        </div>
                                                        <p className="text-[10px] text-gray-500 dark:text-gray-400 mt-1">
                                                            {t('proxy.cloudflared.mode_quick_desc', { defaultValue: 'Auto-generated temporary URL (*.trycloudflare.com)' })}
                                                        </p>
                                                    </button>
                                                    <button
                                                        onClick={() => {
                                                            setCfMode('auth');
                                                            if (appConfig) {
                                                                saveConfig({
                                                                    ...appConfig,
                                                                    cloudflared: { ...appConfig.cloudflared, mode: 'auth' }
                                                                });
                                                            }
                                                        }}
                                                        disabled={cfStatus.running}
                                                        className={cn(
                                                            "p-3 rounded-lg border-2 text-left transition-all",
                                                            cfMode === 'auth'
                                                                ? "border-orange-500 bg-orange-50 dark:bg-orange-900/20"
                                                                : "border-gray-200 dark:border-gray-700 hover:border-gray-300 dark:hover:border-gray-600",
                                                            cfStatus.running && "opacity-60 cursor-not-allowed"
                                                        )}
                                                    >
                                                        <div className="text-sm font-bold text-gray-900 dark:text-base-content">
                                                            {t('proxy.cloudflared.mode_auth', { defaultValue: 'Named Tunnel' })}
                                                        </div>
                                                        <p className="text-[10px] text-gray-500 dark:text-gray-400 mt-1">
                                                            {t('proxy.cloudflared.mode_auth_desc', { defaultValue: 'Use your Cloudflare account with custom domain' })}
                                                        </p>
                                                    </button>
                                                </div>

                                                {/* Token输入 (仅auth模式) */}
                                                {cfMode === 'auth' && (
                                                    <div className="space-y-2">
                                                        <label className="text-sm font-medium text-gray-700 dark:text-gray-300">
                                                            {t('proxy.cloudflared.token', { defaultValue: 'Tunnel Token' })}
                                                        </label>
                                                        <input
                                                            type="password"
                                                            value={cfToken}
                                                            onChange={(e) => setCfToken(e.target.value)}
                                                            onBlur={() => {
                                                                if (appConfig) {
                                                                    saveConfig({
                                                                        ...appConfig,
                                                                        cloudflared: { ...appConfig.cloudflared, token: cfToken }
                                                                    });
                                                                }
                                                            }}
                                                            disabled={cfStatus.running}
                                                            placeholder="eyJhIjoiNj..."
                                                            className="w-full px-3 py-2 rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-base-200 text-sm font-mono disabled:opacity-60"
                                                        />
                                                    </div>
                                                )}

                                                {/* HTTP2选项 */}
                                                <div className="flex items-center justify-between p-3 bg-gray-50 dark:bg-base-200 rounded-lg">
                                                    <div className="space-y-0.5">
                                                        <span className="text-sm font-medium text-gray-900 dark:text-base-content">
                                                            {t('proxy.cloudflared.use_http2', { defaultValue: 'Use HTTP/2' })}
                                                        </span>
                                                        <p className="text-[10px] text-gray-500 dark:text-gray-400">
                                                            {t('proxy.cloudflared.use_http2_desc', { defaultValue: 'More compatible, recommended for China mainland' })}
                                                        </p>
                                                    </div>
                                                    <input
                                                        type="checkbox"
                                                        className="toggle toggle-sm"
                                                        checked={cfUseHttp2}
                                                        onChange={(e) => {
                                                            const val = e.target.checked;
                                                            setCfUseHttp2(val);
                                                            if (appConfig) {
                                                                const newConfig = {
                                                                    ...appConfig,
                                                                    cloudflared: {
                                                                        ...appConfig.cloudflared,
                                                                        use_http2: val
                                                                    }
                                                                };
                                                                saveConfig(newConfig);
                                                            }
                                                        }}
                                                        disabled={cfStatus.running}
                                                    />
                                                </div>

                                                {/* 运行状态和URL */}
                                                {cfStatus.running && (
                                                    <div className="p-4 bg-green-50 dark:bg-green-900/20 rounded-xl border border-green-200 dark:border-green-800">
                                                        <div className="flex items-center gap-2 mb-2">
                                                            <div className="w-2 h-2 rounded-full bg-green-500 animate-pulse"></div>
                                                            <span className="text-sm font-bold text-green-800 dark:text-green-200">
                                                                {t('proxy.cloudflared.running', { defaultValue: 'Tunnel Running' })}
                                                            </span>
                                                        </div>
                                                        {cfStatus.url && (
                                                            <div className="flex items-center gap-2">
                                                                <code className="flex-1 px-3 py-2 bg-white dark:bg-base-100 rounded text-xs font-mono text-gray-800 dark:text-gray-200 border border-green-200 dark:border-green-800">
                                                                    {cfStatus.url}
                                                                </code>
                                                                <button
                                                                    onClick={handleCfCopyUrl}
                                                                    className="p-2 rounded-lg bg-green-500 text-white hover:bg-green-600 transition-colors"
                                                                >
                                                                    {copied === 'cf-url' ? <CheckCircle size={16} /> : <Copy size={16} />}
                                                                </button>
                                                            </div>
                                                        )}
                                                    </div>
                                                )}

                                                {/* 错误信息 */}
                                                {cfStatus.error && (
                                                    <div className="p-3 bg-red-50 dark:bg-red-900/20 rounded-lg border border-red-200 dark:border-red-800 text-sm text-red-700 dark:text-red-300">
                                                        {cfStatus.error}
                                                    </div>
                                                )}
                                            </>
                                        )}
                                    </div>
                                </CollapsibleCard>
                            )}
                        </div>
                    )
                }


                {/* 各种对话框 */}
                <ModalDialog
                    isOpen={isResetConfirmOpen}
                    title={t('proxy.dialog.reset_mapping_title') || '重置映射'}
                    message={t('proxy.dialog.reset_mapping_msg') || '确定要重置所有模型映射为系统默认吗？'}
                    type="confirm"
                    isDestructive={true}
                    onConfirm={executeResetMapping}
                    onCancel={() => setIsResetConfirmOpen(false)}
                />

                <ModalDialog
                    isOpen={isRegenerateKeyConfirmOpen}
                    title={t('proxy.dialog.regenerate_key_title') || t('proxy.dialog.confirm_regenerate')}
                    message={t('proxy.dialog.regenerate_key_msg') || t('proxy.dialog.confirm_regenerate')}
                    type="confirm"
                    isDestructive={true}
                    onConfirm={executeGenerateApiKey}
                    onCancel={() => setIsRegenerateKeyConfirmOpen(false)}
                />

                <ModalDialog
                    isOpen={isClearBindingsConfirmOpen}
                    title={t('proxy.dialog.clear_bindings_title') || '清除会话绑定'}
                    message={t('proxy.dialog.clear_bindings_msg') || '确定要清除所有会话与账号的绑定映射吗？'}
                    type="confirm"
                    isDestructive={true}
                    onConfirm={executeClearSessionBindings}
                    onCancel={() => setIsClearBindingsConfirmOpen(false)}
                />

                <ModalDialog
                    isOpen={isClearRateLimitsConfirmOpen}
                    title={t('proxy.dialog.clear_rate_limits_title') || '清除限流记录'}
                    message={t('proxy.dialog.clear_rate_limits_confirm') || '确定要清除所有本地限流记录吗？'}
                    type="confirm"
                    isDestructive={true}
                    onConfirm={executeClearRateLimits}
                    onCancel={() => setIsClearRateLimitsConfirmOpen(false)}
                />

                <ModalDialog
                    isOpen={isPresetManagerOpen}
                    title={t('proxy.router.manage_presets_title')}
                    onConfirm={() => setIsPresetManagerOpen(false)}
                    confirmText={t('common.close')}
                    type="info"
                >
                    <div className="space-y-6">
                        {/* Save Current Section */}
                        <div className="space-y-3 p-4 bg-blue-50/50 dark:bg-blue-900/10 rounded-xl border border-blue-100 dark:border-blue-900/20">
                            <h3 className="text-sm font-bold text-gray-800 dark:text-gray-200 flex items-center gap-2">
                                <Save size={16} className="text-blue-500" />
                                {t('proxy.router.save_current_as_preset')}
                            </h3>
                            <div className="flex gap-2">
                                <input
                                    type="text"
                                    value={newPresetName}
                                    onChange={(e) => setNewPresetName(e.target.value)}
                                    placeholder={t('proxy.router.preset_name_placeholder')}
                                    className="input input-sm flex-1 border-gray-300 focus:border-blue-500"
                                />
                                <button
                                    onClick={handleSaveCurrentAsPreset}
                                    disabled={!newPresetName.trim()}
                                    className="btn btn-sm btn-primary text-white"
                                >
                                    {t('common.save')}
                                </button>
                            </div>
                            <p className="text-[10px] text-gray-500 dark:text-gray-400">
                                {t('proxy.router.save_hint')}
                            </p>
                        </div>

                        {/* Existing Presets List */}
                        <div className="space-y-3">
                            <h3 className="text-sm font-bold text-gray-800 dark:text-gray-200 px-1">
                                {t('proxy.router.your_presets')}
                            </h3>
                            <div className="max-h-[300px] overflow-y-auto space-y-2 pr-1">
                                {customPresets.length === 0 ? (
                                    <div className="text-center py-8 text-gray-400 dark:text-gray-600 bg-gray-50 dark:bg-base-200 rounded-xl border border-dashed border-gray-200 dark:border-gray-700">
                                        <p>{t('proxy.router.no_custom_presets')}</p>
                                    </div>
                                ) : (
                                    customPresets.map(preset => (
                                        <div key={preset.id} className="flex items-center justify-between p-3 bg-white dark:bg-base-200 border border-gray-100 dark:border-gray-700 rounded-xl hover:shadow-sm transition-all group">
                                            <div className="flex-1 min-w-0">
                                                <div className="font-bold text-sm text-gray-800 dark:text-gray-200 truncate">{preset.name}</div>
                                                <div className="text-[10px] text-gray-400 dark:text-gray-500 truncate">
                                                    {Object.keys(preset.mappings).length} {t('proxy.router.mappings_count')}
                                                </div>
                                            </div>
                                            <div className="flex items-center gap-2 opacity-0 group-hover:opacity-100 transition-opacity">
                                                <button
                                                    onClick={() => handleDeletePreset(preset.id)}
                                                    className="p-1.5 text-gray-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 rounded-lg transition-colors"
                                                    title={t('common.delete')}
                                                >
                                                    <Trash2 size={16} />
                                                </button>
                                            </div>
                                        </div>
                                    ))
                                )}
                            </div>
                        </div>
                    </div>
                </ModalDialog>
            </div>
        </motion.div>
    );
}
