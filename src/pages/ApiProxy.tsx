import { useState, useEffect, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { request as invoke } from '../utils/request';
import { isTauri } from '../utils/env';
import { copyToClipboard } from '../utils/clipboard';
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
    Puzzle,
    Zap,
    ArrowRight,
    Sparkles,
    Code,
    Check,
    X,
    Edit2,
    Save
} from 'lucide-react';
import { AppConfig, ProxyConfig, StickySessionConfig, ExperimentalConfig } from '../types/config';
import HelpTooltip from '../components/common/HelpTooltip';
import ModalDialog from '../components/common/ModalDialog';
import { showToast } from '../components/common/ToastContainer';
import { cn } from '../utils/cn';
import { useProxyModels } from '../hooks/useProxyModels';
import GroupedSelect, { SelectOption } from '../components/common/GroupedSelect';
import { CliSyncCard } from '../components/proxy/CliSyncCard';
import DebouncedSlider from '../components/common/DebouncedSlider';
import { listAccounts } from '../services/accountService';
import CircuitBreaker from '../components/settings/CircuitBreaker';
import AdvancedThinking from '../components/settings/AdvancedThinking';
import { CircuitBreakerConfig } from '../types/config';

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
        <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-gray-700/50 overflow-hidden transition-all duration-200 hover:shadow-md">
            <div
                className="px-5 py-4 flex items-center justify-between cursor-pointer bg-gray-50/50 dark:bg-gray-800/50 hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors"
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
                    <span className="font-medium text-sm text-gray-900 dark:text-gray-100">
                        {title}
                    </span>
                    {enabled !== undefined && (
                        <div className={cn('text-xs px-2 py-0.5 rounded-full', enabled ? 'bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-400' : 'bg-gray-100 text-gray-500 dark:bg-gray-600/50 dark:text-gray-300')}>
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
                                className="toggle toggle-sm bg-gray-200 dark:bg-gray-700 border-gray-300 dark:border-gray-600 checked:bg-blue-500 checked:border-blue-500"
                                checked={enabled}
                                onChange={(e) => onToggle(e.target.checked)}
                            />
                        </div>
                    )}

                    <button
                        className={cn('p-1 rounded-lg hover:bg-gray-200 dark:hover:bg-gray-700 transition-all duration-200', isExpanded ? 'rotate-180' : '')}
                    >
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                            <path d="m6 9 6 6 6-6" />
                        </svg>
                    </button>
                </div>
            </div>

            <div
                className={`transition-all duration-300 ease-in-out border-t border-gray-100 dark:border-base-200 ${isExpanded ? 'max-h-[2000px] opacity-100' : 'max-h-0 opacity-0 overflow-hidden'
                    }`}
            >
                <div className="p-5 relative">
                    {/* Overlay when disabled */}
                    {enabled === false && !allowInteractionWhenDisabled && (
                        <div className="absolute inset-0 bg-gray-100/40 dark:bg-black/30 z-10 cursor-not-allowed" />
                    )}
                    <div className={enabled === false && !allowInteractionWhenDisabled ? 'opacity-60 pointer-events-none select-none' : ''}>
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
    const [configLoading, setConfigLoading] = useState(true);
    const [configError, setConfigError] = useState<string | null>(null);
    const [loading, setLoading] = useState(false);
    const [copied, setCopied] = useState<string | null>(null);
    const [selectedProtocol, setSelectedProtocol] = useState<'openai' | 'anthropic' | 'gemini'>('openai');
    const [selectedModelId, setSelectedModelId] = useState('gemini-3-flash');
    const [zaiAvailableModels, setZaiAvailableModels] = useState<string[]>([]);
    const [zaiModelsLoading, setZaiModelsLoading] = useState(false);
    const [, setZaiModelsError] = useState<string | null>(null);
    const [zaiNewMappingFrom, setZaiNewMappingFrom] = useState('');
    const [zaiNewMappingTo, setZaiNewMappingTo] = useState('');
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

    const zaiModelOptions = useMemo(() => {
        const unique = new Set(zaiAvailableModels);
        return Array.from(unique).sort();
    }, [zaiAvailableModels]);

    const zaiModelMapping = useMemo(() => {
        return appConfig?.proxy.zai?.model_mapping || {};
    }, [appConfig?.proxy.zai?.model_mapping]);


    // 生成自定义映射表单的选项 (从 models 动态生成)
    const customMappingOptions: SelectOption[] = useMemo(() => {
        return models.map(model => ({
            value: model.id,
            label: `${model.id} (${model.name})`,
            group: model.group || 'Other'
        }));
    }, [models]);

    // 初始化加载
    useEffect(() => {
        loadConfig();
        loadStatus();
        loadAccounts();
        loadPreferredAccount();
        loadCfStatus();
        loadCustomPresets();
        const interval = setInterval(loadStatus, 3000);
        const cfInterval = setInterval(loadCfStatus, 5000);
        return () => {
            clearInterval(interval);
            clearInterval(cfInterval);
        };
    }, []);



    // [FIX #820] Load available accounts for fixed account mode
    const loadAccounts = async () => {
        try {
            const accounts = await listAccounts();
            setAvailableAccounts(accounts.map(a => ({ id: a.id, email: a.email })));
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
            setAppConfig(config);

            // 恢复 Cloudflared 持久化状态
            if (config.cloudflared) {
                setCfMode(config.cloudflared.mode || 'quick');
                setCfToken(config.cloudflared.token || '');
                setCfUseHttp2(config.cloudflared.use_http2 !== false); // 默认开启 HTTP/2
            }

            // 恢复 Cloudflared 状态并实现持久化同步
            if (config.cloudflared) {
                setCfMode(config.cloudflared.mode || 'quick');
                setCfToken(config.cloudflared.token || '');
                setCfUseHttp2(config.cloudflared.use_http2 !== false); // 默认 true
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

    // 专门处理模型映射的热更新 (全量)
    const handleMappingUpdate = async (type: 'custom', key: string, value: string) => {
        if (!appConfig) return;

        console.log('[DEBUG] handleMappingUpdate called:', { type, key, value });

        const newConfig = { ...appConfig.proxy };
        newConfig.custom_mapping = { ...(newConfig.custom_mapping || {}), [key]: value };

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

    const handleResetMapping = () => {
        if (!appConfig) return;
        setIsResetConfirmOpen(true);
    };

    const executeResetMapping = async () => {
        if (!appConfig) return;
        setIsResetConfirmOpen(false);

        // 恢复到默认映射值 (空映射)
        const newConfig = {
            ...appConfig.proxy,
            custom_mapping: {}
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
                "gpt-4o*": "gemini-3-flash",
                "gpt-3.5*": "gemini-2.5-flash",
                "o1-*": "gemini-3.1-pro-high",
                "o3-*": "gemini-3.1-pro-high",
                "claude-3-5-sonnet-*": "claude-sonnet-4-6",
                "claude-3-opus-*": "claude-opus-4-6-thinking",
                "claude-opus-4-6*": "claude-opus-4-6-thinking",
                "claude-haiku-*": "gemini-2.5-flash",
                "claude-3-haiku-*": "gemini-2.5-flash",
            }
        },
        {
            id: 'performance',
            name: t('proxy.router.preset_performance'),
            description: t('proxy.router.preset_performance_desc'),
            mappings: {
                "gpt-4*": "claude-opus-4-6-thinking",
                "gpt-4o*": "claude-sonnet-4-6",
                "gpt-3.5*": "gemini-3-flash",
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
                "gpt-4*": "gemini-3-flash",
                "gpt-4o*": "gemini-2.5-flash",
                "gpt-3.5*": "gemini-2.5-flash",
                "o1-*": "gemini-3-flash",
                "o3-*": "gemini-3-flash",
                "claude-3-5-sonnet-*": "gemini-3-flash",
                "claude-3-opus-*": "gemini-3-flash",
                "claude-opus-4-*": "gemini-3-flash", // Cost-effective: map all opus 4 to flash
                "claude-haiku-*": "gemini-2.5-flash",
                "claude-3-haiku-*": "gemini-2.5-flash",
            }
        },
        {
            id: 'balanced',
            name: t('proxy.router.preset_balanced'),
            description: t('proxy.router.preset_balanced_desc'),
            mappings: {
                "gpt-4*": "gemini-3.1-pro-high",
                "gpt-4o*": "gemini-3-flash",
                "gpt-3.5*": "gemini-2.5-flash",
                "o1-*": "claude-sonnet-4-6",
                "o3-*": "claude-sonnet-4-6",
                "claude-3-5-sonnet-*": "claude-sonnet-4-6",
                "claude-3-opus-*": "gemini-3.1-pro-high",
                "claude-opus-4-5*": "gemini-3.1-pro-high",
                "claude-opus-4-6*": "claude-opus-4-6-thinking", // Balanced: Keep 4.6 as itself (or map to high?) Let's map to itself for now to utilize header
                "claude-haiku-*": "gemini-2.5-flash",
                "claude-3-haiku-*": "gemini-2.5-flash",
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
        const newConfig = { ...appConfig.proxy, custom_mapping: newCustom };
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
                        enable_usage_scaling: true,
                        context_compression_threshold_l1: 0.4,
                        context_compression_threshold_l2: 0.55,
                        context_compression_threshold_l3: 0.7
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

    const refreshZaiModels = async () => {
        if (!appConfig?.proxy.zai) return;
        setZaiModelsLoading(true);
        setZaiModelsError(null);
        try {
            const models = await invoke<string[]>('fetch_zai_models', {
                zai: appConfig.proxy.zai,
                upstreamProxy: appConfig.proxy.upstream_proxy,
                requestTimeout: appConfig.proxy.request_timeout,
            });
            setZaiAvailableModels(models);
        } catch (error: any) {
            console.error('Failed to fetch z.ai models:', error);
            setZaiModelsError(error.toString());
        } finally {
            setZaiModelsLoading(false);
        }
    };

    const updateZaiDefaultModels = (updates: Partial<NonNullable<ProxyConfig['zai']>['models']>) => {
        if (!appConfig?.proxy.zai) return;
        const newConfig = {
            ...appConfig,
            proxy: {
                ...appConfig.proxy,
                zai: {
                    ...appConfig.proxy.zai,
                    models: { ...appConfig.proxy.zai.models, ...updates }
                }
            }
        };
        saveConfig(newConfig);
    };

    const upsertZaiModelMapping = (from: string, to: string) => {
        if (!appConfig?.proxy.zai) return;
        const currentMapping = appConfig.proxy.zai.model_mapping || {};
        const newMapping = { ...currentMapping, [from]: to };

        const newConfig = {
            ...appConfig,
            proxy: {
                ...appConfig.proxy,
                zai: {
                    ...appConfig.proxy.zai,
                    model_mapping: newMapping
                }
            }
        };
        saveConfig(newConfig);
    };

    const removeZaiModelMapping = (from: string) => {
        if (!appConfig?.proxy.zai) return;
        const currentMapping = appConfig.proxy.zai.model_mapping || {};
        const newMapping = { ...currentMapping };
        delete newMapping[from];

        const newConfig = {
            ...appConfig,
            proxy: {
                ...appConfig.proxy,
                zai: {
                    ...appConfig.proxy.zai,
                    model_mapping: newMapping
                }
            }
        };
        saveConfig(newConfig);
    };

    const updateZaiGeneralConfig = (updates: Partial<NonNullable<ProxyConfig['zai']>>) => {
        if (!appConfig?.proxy.zai) return;
        const newConfig = {
            ...appConfig,
            proxy: {
                ...appConfig.proxy,
                zai: {
                    ...appConfig.proxy.zai,
                    ...updates
                }
            }
        };
        saveConfig(newConfig);
    };

    const handleToggle = async () => {
        if (!appConfig) return;
        setLoading(true);
        try {
            if (status.running) {
                await invoke('stop_proxy_service');
            } else {
                // 使用当前的 appConfig.proxy 启动
                await invoke('start_proxy_service', { config: appConfig.proxy });
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
    # 推荐使用 127.0.0.1
    base_url="${`http://127.0.0.1:${port}`}",
    api_key="${apiKey}"
)

# 注意: Antigravity 支持使用 Anthropic SDK 调用任意模型
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
            return `# 需要安装: pip install google-generativeai
import google.generativeai as genai

# 使用 Antigravity 代理地址 (推荐 127.0.0.1)
genai.configure(
    api_key="${apiKey}",
    transport='rest',
    client_options={'api_endpoint': '${rawBaseUrl}'}
)

model = genai.GenerativeModel('${modelId}')
response = model.generate_content("Hello")
print(response.text)`;
        }

        // 3. OpenAI Protocol
        if (modelId.startsWith('gemini-3-pro-image')) {
            return `from openai import OpenAI

client = OpenAI(
    base_url="${baseUrl}",
    api_key="${apiKey}"
)

response = client.chat.completions.create(
    model="${modelId}",
    # 方式 1: 使用 size 参数 (推荐)
    # 支持: "1024x1024" (1:1), "1280x720" (16:9), "720x1280" (9:16), "1216x896" (4:3)
    extra_body={ "size": "1024x1024" },
    
    # 方式 2: 使用模型后缀
    # 例如: gemini-3-pro-image-16-9, gemini-3-pro-image-4-3
    # model="gemini-3-pro-image-16-9",
    messages=[{
        "role": "user",
        "content": "Draw a futuristic city"
    }]
)

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

    return (
        <div className="h-full w-full overflow-y-auto overflow-x-hidden">
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
                    <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200">
                        <div className="px-4 py-2.5 border-b border-gray-100 dark:border-base-200 flex items-center justify-between">
                            <div className="flex items-center gap-4">
                                <h2 className="text-base font-semibold flex items-center gap-2 text-gray-900 dark:text-base-content">
                                    <Settings size={18} />
                                    {t('proxy.config.title')}
                                </h2>
                                {/* 状态指示器 */}
                                <div className="flex items-center gap-2 pl-4 border-l border-gray-200 dark:border-base-300">
                                    <div className={`w-2 h-2 rounded-full ${status.running ? 'bg-green-500 animate-pulse' : 'bg-gray-400'}`} />
                                    <span className={`text-xs font-medium ${status.running ? 'text-green-600' : 'text-gray-500'}`}>
                                        {status.running
                                            ? `${t('proxy.status.running')} (${status.active_accounts} ${t('common.accounts')})`
                                            : t('proxy.status.stopped')}
                                    </span>
                                </div>
                            </div>

                            {/* 控制按钮 */}
                            <div className="flex items-center gap-2">
                                <button
                                    onClick={handleToggle}
                                    disabled={loading || !appConfig}
                                    className={`px-3 py-1 rounded-lg text-xs font-medium transition-colors flex items-center gap-2 ${status.running
                                        ? 'bg-red-50 to-red-600 text-red-600 hover:bg-red-100 border border-red-200'
                                        : 'bg-blue-600 hover:bg-blue-700 text-white shadow-sm shadow-blue-500/30'
                                        } ${(loading || !appConfig) ? 'opacity-50 cursor-not-allowed' : ''}`}
                                >
                                    <Power size={14} />
                                    {loading ? t('proxy.status.processing') : (status.running ? t('proxy.action.stop') : t('proxy.action.start'))}
                                </button>
                            </div>
                        </div>
                        <div className="p-3 space-y-3">
                            {/* 监听端口、超时和自启动 */}
                            <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
                                <div>
                                    <label className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1">
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
                                        value={appConfig.proxy.port}
                                        onChange={(e) => updateProxyConfig({ port: parseInt(e.target.value) })}
                                        min={8000}
                                        max={65535}
                                        disabled={status.running}
                                        className="w-full px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 text-xs text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50 disabled:cursor-not-allowed"
                                    />
                                    <p className="mt-0.5 text-[10px] text-gray-500 dark:text-gray-400">
                                        {t('proxy.config.port_hint')}
                                    </p>
                                </div>
                                <div>
                                    <label className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1">
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
                                        className="w-full px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 text-xs text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50 disabled:cursor-not-allowed"
                                    />
                                    <p className="mt-0.5 text-[10px] text-gray-500 dark:text-gray-400">
                                        {t('proxy.config.request_timeout_hint')}
                                    </p>
                                </div>
                                <div className="flex items-center">
                                    <label className="flex items-center cursor-pointer gap-3">
                                        <input
                                            type="checkbox"
                                            className="toggle toggle-sm bg-gray-200 dark:bg-gray-700 border-gray-300 dark:border-gray-600 checked:bg-blue-500 checked:border-blue-500 disabled:opacity-50 disabled:bg-gray-100 dark:disabled:bg-gray-800"
                                            checked={appConfig.proxy.auto_start}
                                            onChange={(e) => updateProxyConfig({ auto_start: e.target.checked })}
                                        />
                                        <span className="text-xs font-medium text-gray-900 dark:text-base-content inline-flex items-center gap-1">
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


                            {/* 局域网访问 & 访问授权 - 合并到同一行 */}
                            <div className="border-t border-gray-200 dark:border-base-300 pt-3 mt-3">
                                <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                                    {/* 允许局域网访问 */}
                                    <div className="space-y-2">
                                        <div className="flex items-center justify-between">
                                            <span className="text-xs font-medium text-gray-700 dark:text-gray-300 inline-flex items-center gap-1">
                                                {t('proxy.config.allow_lan_access')}
                                                <HelpTooltip
                                                    text={t('proxy.config.allow_lan_access_tooltip')}
                                                    ariaLabel={t('proxy.config.allow_lan_access')}
                                                    placement="right"
                                                />
                                            </span>
                                            <input
                                                type="checkbox"
                                                className="toggle toggle-sm bg-gray-200 dark:bg-gray-700 border-gray-300 dark:border-gray-600 checked:bg-blue-500 checked:border-blue-500"
                                                checked={appConfig.proxy.allow_lan_access || false}
                                                onChange={(e) => updateProxyConfig({ allow_lan_access: e.target.checked })}
                                            />
                                        </div>
                                        <p className="text-[10px] text-gray-500 dark:text-gray-400">
                                            {(appConfig.proxy.allow_lan_access || false)
                                                ? t('proxy.config.allow_lan_access_hint_enabled')
                                                : t('proxy.config.allow_lan_access_hint_disabled')}
                                        </p>
                                        {(appConfig.proxy.allow_lan_access || false) && (
                                            <p className="text-[10px] text-amber-600 dark:text-amber-500">
                                                {t('proxy.config.allow_lan_access_warning')}
                                            </p>
                                        )}
                                        {status.running && (
                                            <p className="text-[10px] text-blue-600 dark:text-blue-400">
                                                {t('proxy.config.allow_lan_access_restart_hint')}
                                            </p>
                                        )}
                                    </div>

                                    {/* 访问授权 */}
                                    <div className="space-y-2">
                                        <div className="flex items-center justify-between">
                                            <label className="text-xs font-medium text-gray-700 dark:text-gray-300">
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
                                                <span className="text-[11px] text-gray-600 dark:text-gray-400 inline-flex items-center gap-1">
                                                    {(appConfig.proxy.auth_mode || 'off') !== 'off' ? t('proxy.config.auth.enabled') : t('common.disabled')}
                                                    <HelpTooltip
                                                        text={t('proxy.config.auth.enabled_tooltip')}
                                                        ariaLabel={t('proxy.config.auth.enabled')}
                                                        placement="left"
                                                    />
                                                </span>
                                                <input
                                                    type="checkbox"
                                                    className="toggle toggle-sm bg-gray-200 dark:bg-gray-700 border-gray-300 dark:border-gray-600 checked:bg-blue-500 checked:border-blue-500 disabled:opacity-50 disabled:bg-gray-100 dark:disabled:bg-gray-800"
                                                    checked={(appConfig.proxy.auth_mode || 'off') !== 'off'}
                                                    onChange={(e) => {
                                                        const nextMode = e.target.checked ? 'all_except_health' : 'off';
                                                        updateProxyConfig({ auth_mode: nextMode });
                                                    }}
                                                />
                                            </label>
                                        </div>

                                        <div>
                                            <label className="block text-[11px] text-gray-600 dark:text-gray-400 mb-1">
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
                                                className="w-full px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 text-xs text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                                            >
                                                <option value="off">{t('proxy.config.auth.modes.off')}</option>
                                                <option value="strict">{t('proxy.config.auth.modes.strict')}</option>
                                                <option value="all_except_health">{t('proxy.config.auth.modes.all_except_health')}</option>
                                                <option value="auto">{t('proxy.config.auth.modes.auto')}</option>
                                            </select>
                                            <p className="mt-0.5 text-[10px] text-gray-500 dark:text-gray-400">
                                                {t('proxy.config.auth.hint')}
                                            </p>
                                        </div>
                                    </div>
                                </div>
                            </div>

                            {/* API 密钥 */}
                            <div>
                                <label className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1">
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
                                        className={`flex-1 px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg text-xs font-mono ${isEditingApiKey
                                            ? 'bg-white dark:bg-base-200 text-gray-900 dark:text-base-content'
                                            : 'bg-gray-50 dark:bg-base-300 text-gray-600 dark:text-gray-400'
                                            }`}
                                    />
                                    {isEditingApiKey ? (
                                        <>
                                            <button
                                                onClick={handleSaveApiKey}
                                                className="px-2.5 py-1.5 border border-green-300 dark:border-green-700 rounded-lg bg-green-50 dark:bg-green-900/20 hover:bg-green-100 dark:hover:bg-green-900/30 transition-colors text-green-600 dark:text-green-400"
                                                title={t('proxy.config.btn_save')}
                                            >
                                                <CheckCircle size={14} />
                                            </button>
                                            <button
                                                onClick={handleCancelEditApiKey}
                                                className="px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors"
                                                title={t('common.cancel')}
                                            >
                                                <X size={14} />
                                            </button>
                                        </>
                                    ) : (
                                        <>
                                            <button
                                                onClick={handleEditApiKey}
                                                className="px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors"
                                                title={t('proxy.config.btn_edit')}
                                            >
                                                <Edit2 size={14} />
                                            </button>
                                            <button
                                                onClick={handleGenerateApiKey}
                                                className="px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors"
                                                title={t('proxy.config.btn_regenerate')}
                                            >
                                                <RefreshCw size={14} />
                                            </button>
                                            <button
                                                onClick={() => copyToClipboardHandler(appConfig.proxy.api_key, 'api_key')}
                                                className="px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors"
                                                title={t('proxy.config.btn_copy')}
                                            >
                                                {copied === 'api_key' ? (
                                                    <CheckCircle size={14} className="text-green-500" />
                                                ) : (
                                                    <Copy size={14} />
                                                )}
                                            </button>
                                        </>
                                    )}
                                </div>
                                <p className="mt-0.5 text-[10px] text-amber-600 dark:text-amber-500">
                                    {t('proxy.config.warning_key')}
                                </p>
                            </div>

                            {/* Web UI 管理密码 */}
                            <div className="border-t border-gray-200 dark:border-base-300 pt-3 mt-3">
                                <label className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1">
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
                                        className={`flex-1 px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg text-xs font-mono ${isEditingAdminPassword
                                            ? 'bg-white dark:bg-base-200 text-gray-900 dark:text-base-content'
                                            : 'bg-gray-50 dark:bg-base-300 text-gray-600 dark:text-gray-400'
                                            }`}
                                    />
                                    {isEditingAdminPassword ? (
                                        <>
                                            <button
                                                onClick={handleSaveAdminPassword}
                                                className="px-2.5 py-1.5 border border-green-300 dark:border-green-700 rounded-lg bg-green-50 dark:bg-green-900/20 hover:bg-green-100 dark:hover:bg-green-900/30 transition-colors text-green-600 dark:text-green-400"
                                                title={t('proxy.config.btn_save')}
                                            >
                                                <CheckCircle size={14} />
                                            </button>
                                            <button
                                                onClick={handleCancelEditAdminPassword}
                                                className="px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors"
                                                title={t('common.cancel')}
                                            >
                                                <X size={14} />
                                            </button>
                                        </>
                                    ) : (
                                        <>
                                            <button
                                                onClick={handleEditAdminPassword}
                                                className="px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors"
                                                title={t('proxy.config.btn_edit')}
                                            >
                                                <Edit2 size={14} />
                                            </button>
                                            <button
                                                onClick={() => copyToClipboardHandler(appConfig.proxy.admin_password || appConfig.proxy.api_key, 'admin_password')}
                                                className="px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors"
                                                title={t('proxy.config.btn_copy')}
                                            >
                                                {copied === 'admin_password' ? (
                                                    <CheckCircle size={14} className="text-green-500" />
                                                ) : (
                                                    <Copy size={14} />
                                                )}
                                            </button>
                                        </>
                                    )}
                                </div>
                                <p className="mt-0.5 text-[10px] text-gray-500 dark:text-gray-400">
                                    {t('proxy.config.admin_password_hint', { defaultValue: 'For safety in Docker/Browser environments, you can set a separate login password from your API Key.' })}
                                </p>
                            </div>

                            {/* User-Agent Overrides */}
                            <div className="border-t border-gray-200 dark:border-base-300 pt-3 mt-3">
                                <div className="flex items-center justify-between mb-2">
                                    <label className="text-xs font-medium text-gray-700 dark:text-gray-300 inline-flex items-center gap-1">
                                        {t('proxy.config.request.user_agent', { defaultValue: 'User-Agent Override' })}
                                        <HelpTooltip text={t('proxy.config.request.user_agent_tooltip', { defaultValue: 'Override the User-Agent header sent to upstream APIs.' })} />
                                    </label>
                                    <input
                                        type="checkbox"
                                        className="toggle toggle-sm bg-gray-200 dark:bg-gray-700 border-gray-300 dark:border-gray-600 checked:bg-blue-500 checked:border-blue-500"
                                        checked={!!appConfig.proxy.user_agent_override}
                                        onChange={(e) => {
                                            const enabled = e.target.checked;
                                            if (enabled) {
                                                // Restore saved override from config or use default
                                                const restoredValue = appConfig.proxy.saved_user_agent || 'antigravity/1.15.8 darwin/arm64';
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
                                    <div className="space-y-2 animate-in fade-in slide-in-from-top-1 duration-200">
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
                                            className="w-full px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 text-xs font-mono text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                                            placeholder={t('proxy.config.request.user_agent_placeholder', { defaultValue: 'Enter custom User-Agent string...' })}
                                        />
                                        <div className="bg-gray-50 dark:bg-base-300 rounded p-2 text-[10px] text-gray-500 font-mono break-all">
                                            <span className="font-bold select-none mr-2">{t('common.example', { defaultValue: 'Example' })}:</span>
                                            antigravity/1.15.8 darwin/arm64
                                        </div>
                                    </div>
                                )}
                            </div>


                        </div>
                    </div>
                )}

                {/* External Providers Integration */}
                {
                    !configLoading && !configError && appConfig && (
                        <div className="space-y-4">
                            <CollapsibleCard
                                title={t('proxy.cli_sync.title', { defaultValue: 'CLI Sync' })}
                                icon={<Terminal size={18} className="text-gray-500" />}
                                defaultExpanded={false}
                            >
                                <CliSyncCard
                                    proxyUrl={status.running ? status.base_url : `http://127.0.0.1:${appConfig.proxy.port || 8045}`}
                                    apiKey={appConfig.proxy.api_key}
                                />
                            </CollapsibleCard>

                            {/* z.ai (GLM) Dispatcher */}
                            <CollapsibleCard
                                title={t('proxy.config.zai.title')}
                                icon={<Zap size={18} className="text-amber-500" />}
                                enabled={!!appConfig.proxy.zai?.enabled}
                                onToggle={(checked) => updateZaiGeneralConfig({ enabled: checked })}
                            >
                                <div className="space-y-4">
                                    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                                        <div className="space-y-1">
                                            <label className="text-[11px] font-medium text-gray-500 dark:text-gray-400">
                                                {t('proxy.config.zai.base_url')}
                                            </label>
                                            <input
                                                type="text"
                                                value={appConfig.proxy.zai?.base_url || 'https://api.z.ai/api/anthropic'}
                                                onChange={(e) => updateZaiGeneralConfig({ base_url: e.target.value })}
                                                className="input input-sm input-bordered w-full font-mono text-xs"
                                            />
                                        </div>
                                        <div className="space-y-1">
                                            <label className="text-[11px] font-medium text-gray-500 dark:text-gray-400">
                                                {t('proxy.config.zai.dispatch_mode')}
                                            </label>
                                            <select
                                                className="select select-sm select-bordered w-full text-xs"
                                                value={appConfig.proxy.zai?.dispatch_mode || 'off'}
                                                onChange={(e) => updateZaiGeneralConfig({ dispatch_mode: e.target.value as any })}
                                            >
                                                <option value="off">{t('proxy.config.zai.modes.off')}</option>
                                                <option value="exclusive">{t('proxy.config.zai.modes.exclusive')}</option>
                                                <option value="pooled">{t('proxy.config.zai.modes.pooled')}</option>
                                                <option value="fallback">{t('proxy.config.zai.modes.fallback')}</option>
                                            </select>
                                        </div>
                                    </div>

                                    <div className="space-y-1">
                                        <label className="text-[11px] font-medium text-gray-500 dark:text-gray-400 flex items-center justify-between">
                                            <span>{t('proxy.config.zai.api_key')}</span>
                                            {!(appConfig.proxy.zai?.api_key) && (
                                                <span className="text-amber-500 text-[10px] flex items-center gap-1">
                                                    <HelpTooltip text={t('proxy.config.zai.warning')} />
                                                    {t('common.required')}
                                                </span>
                                            )}
                                        </label>
                                        <input
                                            type="password"
                                            value={appConfig.proxy.zai?.api_key || ''}
                                            onChange={(e) => updateZaiGeneralConfig({ api_key: e.target.value })}
                                            placeholder="sk-..."
                                            className="input input-sm input-bordered w-full font-mono text-xs"
                                        />
                                    </div>

                                    {/* Model Mapping Section */}
                                    <div className="pt-4 border-t border-gray-100 dark:border-base-200">
                                        <div className="flex items-center justify-between mb-3">
                                            <h4 className="text-[11px] font-bold text-gray-400 uppercase tracking-widest">
                                                {t('proxy.config.zai.models.title')}
                                            </h4>
                                            <button
                                                onClick={refreshZaiModels}
                                                disabled={zaiModelsLoading || !appConfig.proxy.zai?.api_key}
                                                className="btn btn-ghost btn-xs gap-1"
                                            >
                                                <RefreshCw size={12} className={zaiModelsLoading ? 'animate-spin' : ''} />
                                                {t('proxy.config.zai.models.refresh')}
                                            </button>
                                        </div>

                                        <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
                                            {['opus', 'sonnet', 'haiku'].map((family) => (
                                                <div key={family} className="space-y-1">
                                                    <label className="text-[10px] text-gray-500 capitalize">{family}</label>
                                                    <div className="flex gap-1">
                                                        {zaiModelOptions.length > 0 && (
                                                            <select
                                                                className="select select-xs select-bordered max-w-[80px]"
                                                                value=""
                                                                onChange={(e) => e.target.value && updateZaiDefaultModels({ [family]: e.target.value })}
                                                            >
                                                                <option value="">{t('proxy.config.zai.models.select_placeholder')}</option>
                                                                {zaiModelOptions.map(m => <option key={m} value={m}>{m}</option>)}
                                                            </select>
                                                        )}
                                                        <input
                                                            type="text"
                                                            className="input input-xs input-bordered w-full font-mono"
                                                            value={appConfig.proxy.zai?.models?.[family as keyof typeof appConfig.proxy.zai.models] || ''}
                                                            onChange={(e) => updateZaiDefaultModels({ [family]: e.target.value })}
                                                        />
                                                    </div>
                                                </div>
                                            ))}
                                        </div>

                                        <details className="mt-3 group">
                                            <summary className="cursor-pointer text-[10px] text-gray-500 hover:text-blue-500 transition-colors inline-flex items-center gap-1 select-none">
                                                <Settings size={12} />
                                                {t('proxy.config.zai.models.advanced_title')}
                                            </summary>
                                            <div className="mt-2 space-y-2 p-2 bg-gray-50 dark:bg-base-200/50 rounded-lg">
                                                {/* Advanced Mapping Table */}
                                                {Object.entries(zaiModelMapping).map(([from, to]) => (
                                                    <div key={from} className="flex items-center gap-2">
                                                        <div className="flex-1 bg-white dark:bg-base-100 px-2 py-1 rounded border border-gray-200 dark:border-base-300 text-[10px] font-mono truncate" title={from}>{from}</div>
                                                        <ArrowRight size={10} className="text-gray-400" />
                                                        <div className="flex-[1.5] flex gap-1">
                                                            {zaiModelOptions.length > 0 && (
                                                                <select
                                                                    className="select select-xs select-ghost h-6 min-h-0 px-1"
                                                                    value=""
                                                                    onChange={(e) => e.target.value && upsertZaiModelMapping(from, e.target.value)}
                                                                >
                                                                    <option value="">▼</option>
                                                                    {zaiModelOptions.map(m => <option key={m} value={m}>{m}</option>)}
                                                                </select>
                                                            )}
                                                            <input
                                                                type="text"
                                                                className="input input-xs input-bordered w-full font-mono h-6"
                                                                value={to}
                                                                onChange={(e) => upsertZaiModelMapping(from, e.target.value)}
                                                            />
                                                        </div>
                                                        <button onClick={() => removeZaiModelMapping(from)} className="text-gray-400 hover:text-red-500"><Trash2 size={12} /></button>
                                                    </div>
                                                ))}

                                                <div className="flex items-center gap-2 pt-2 border-t border-gray-200/50">
                                                    <input
                                                        className="input input-xs input-bordered flex-1 font-mono"
                                                        placeholder={t('proxy.config.zai.models.from_placeholder') || "From (e.g. claude-3-opus)"}
                                                        value={zaiNewMappingFrom}
                                                        onChange={e => setZaiNewMappingFrom(e.target.value)}
                                                    />
                                                    <input
                                                        className="input input-xs input-bordered flex-1 font-mono"
                                                        placeholder={t('proxy.config.zai.models.to_placeholder') || "To (e.g. glm-4)"}
                                                        value={zaiNewMappingTo}
                                                        onChange={e => setZaiNewMappingTo(e.target.value)}
                                                    />
                                                    <button
                                                        className="btn btn-xs btn-primary"
                                                        onClick={() => {
                                                            if (zaiNewMappingFrom && zaiNewMappingTo) {
                                                                upsertZaiModelMapping(zaiNewMappingFrom, zaiNewMappingTo);
                                                                setZaiNewMappingFrom('');
                                                                setZaiNewMappingTo('');
                                                            }
                                                        }}
                                                    >
                                                        <Plus size={12} />
                                                    </button>
                                                </div>
                                            </div>
                                        </details>
                                    </div>
                                </div>
                            </CollapsibleCard>

                            {/* MCP System */}
                            <CollapsibleCard
                                title={t('proxy.config.zai.mcp.title')}
                                icon={<Puzzle size={18} className="text-blue-500" />}
                                enabled={!!appConfig.proxy.zai?.mcp?.enabled}
                                onToggle={(checked) => updateZaiGeneralConfig({ mcp: { ...(appConfig.proxy.zai?.mcp || {}), enabled: checked } as any })}
                                rightElement={
                                    <div className="flex gap-2 text-[10px]">
                                        {['web_search', 'web_reader', 'vision'].map(f =>
                                            appConfig.proxy.zai?.mcp?.[(f + '_enabled') as keyof typeof appConfig.proxy.zai.mcp] && (
                                                <span key={f} className="bg-blue-500 dark:bg-blue-600 px-1.5 py-0.5 rounded text-white font-semibold shadow-sm">
                                                    {t(`proxy.config.zai.mcp.${f}`).split(' ')[0]}
                                                </span>
                                            )
                                        )}
                                    </div>
                                }
                            >
                                <div className="space-y-3">
                                    <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
                                        <label className="flex items-center gap-2 border border-gray-100 dark:border-base-200 p-2 rounded-lg cursor-pointer hover:bg-gray-50 dark:hover:bg-base-200/50 transition-colors">
                                            <input
                                                type="checkbox"
                                                className="checkbox checkbox-xs rounded border-2 border-gray-400 dark:border-gray-500 checked:border-blue-600 checked:bg-blue-600 [--chkbg:theme(colors.blue.600)] [--chkfg:white]"
                                                checked={!!appConfig.proxy.zai?.mcp?.web_search_enabled}
                                                onChange={(e) => updateZaiGeneralConfig({ mcp: { ...(appConfig.proxy.zai?.mcp || {}), web_search_enabled: e.target.checked } as any })}
                                            />
                                            <span className="text-xs">{t('proxy.config.zai.mcp.web_search')}</span>
                                        </label>
                                        <label className="flex items-center gap-2 border border-gray-100 dark:border-base-200 p-2 rounded-lg cursor-pointer hover:bg-gray-50 dark:hover:bg-base-200/50 transition-colors">
                                            <input
                                                type="checkbox"
                                                className="checkbox checkbox-xs rounded border-2 border-gray-400 dark:border-gray-500 checked:border-blue-600 checked:bg-blue-600 [--chkbg:theme(colors.blue.600)] [--chkfg:white]"
                                                checked={!!appConfig.proxy.zai?.mcp?.web_reader_enabled}
                                                onChange={(e) => updateZaiGeneralConfig({ mcp: { ...(appConfig.proxy.zai?.mcp || {}), web_reader_enabled: e.target.checked } as any })}
                                            />
                                            <span className="text-xs">{t('proxy.config.zai.mcp.web_reader')}</span>
                                        </label>
                                        <label className="flex items-center gap-2 border border-gray-100 dark:border-base-200 p-2 rounded-lg cursor-pointer hover:bg-gray-50 dark:hover:bg-base-200/50 transition-colors">
                                            <input
                                                type="checkbox"
                                                className="checkbox checkbox-xs rounded border-2 border-gray-400 dark:border-gray-500 checked:border-blue-600 checked:bg-blue-600 [--chkbg:theme(colors.blue.600)] [--chkfg:white]"
                                                checked={!!appConfig.proxy.zai?.mcp?.vision_enabled}
                                                onChange={(e) => updateZaiGeneralConfig({ mcp: { ...(appConfig.proxy.zai?.mcp || {}), vision_enabled: e.target.checked } as any })}
                                            />
                                            <span className="text-xs">{t('proxy.config.zai.mcp.vision')}</span>
                                        </label>
                                    </div>

                                    {appConfig.proxy.zai?.mcp?.enabled && (
                                        <div className="bg-slate-100 dark:bg-slate-800/80 rounded-lg p-3 text-[10px] font-mono text-slate-600 dark:text-slate-400">
                                            <div className="mb-1 font-bold text-gray-400 uppercase tracking-wider">{t('proxy.config.zai.mcp.local_endpoints')}</div>
                                            <div className="space-y-0.5 select-all">
                                                {appConfig.proxy.zai?.mcp?.web_search_enabled && <div>http://127.0.0.1:{status.running ? status.port : (appConfig.proxy.port || 8045)}/mcp/web_search_prime/mcp</div>}
                                                {appConfig.proxy.zai?.mcp?.web_reader_enabled && <div>http://127.0.0.1:{status.running ? status.port : (appConfig.proxy.port || 8045)}/mcp/web_reader/mcp</div>}
                                                {appConfig.proxy.zai?.mcp?.vision_enabled && <div>http://127.0.0.1:{status.running ? status.port : (appConfig.proxy.port || 8045)}/mcp/zai-mcp-server/mcp</div>}
                                            </div>
                                        </div>
                                    )}
                                </div>
                            </CollapsibleCard>

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
                                            <div className="bg-slate-100 dark:bg-slate-800/80 rounded-xl p-4 border border-slate-200 dark:border-slate-700">
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

                            {/* Advanced Thinking & Global Config */}
                            <CollapsibleCard
                                title={t('settings.advanced_thinking.title', { defaultValue: 'Advanced Thinking & Global Config' })}
                                icon={<BrainCircuit size={18} className="text-pink-500" />}
                            >
                                <AdvancedThinking
                                    config={appConfig.proxy}
                                    onChange={(newProxyConfig) => updateProxyConfig(newProxyConfig)}
                                />
                            </CollapsibleCard>

                            {/* 实验性设置 */}
                            <CollapsibleCard
                                title={t('proxy.config.experimental.title')}
                                icon={<Sparkles size={18} className="text-purple-500" />}
                            >
                                <div className="space-y-4">
                                    <div className="flex items-center justify-between p-4 bg-gray-50 dark:bg-base-200 rounded-xl border border-gray-100 dark:border-base-300">
                                        <div className="space-y-1">
                                            <div className="flex items-center gap-2">
                                                <span className="text-sm font-bold text-gray-900 dark:text-base-content">
                                                    {t('proxy.config.experimental.enable_usage_scaling')}
                                                </span>
                                                <HelpTooltip text={t('proxy.config.experimental.enable_usage_scaling_tooltip')} />
                                                <span className="px-1.5 py-0.5 rounded bg-purple-100 dark:bg-purple-900/30 text-[10px] text-purple-600 dark:text-purple-400 font-bold border border-purple-200 dark:border-purple-800">
                                                    Claude
                                                </span>
                                            </div>
                                            <p className="text-[10px] text-gray-500 dark:text-gray-400 max-w-lg">
                                                {t('proxy.config.experimental.enable_usage_scaling_tooltip')}
                                            </p>
                                        </div>
                                        <label className="relative inline-flex items-center cursor-pointer">
                                            <input
                                                type="checkbox"
                                                className="sr-only peer"
                                                checked={!!appConfig.proxy.experimental?.enable_usage_scaling}
                                                onChange={(e) => updateExperimentalConfig({ enable_usage_scaling: e.target.checked })}
                                            />
                                            <div className="w-11 h-6 bg-gray-200 dark:bg-base-300 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-purple-500 shadow-inner"></div>
                                        </label>
                                    </div>

                                    {/* L1 Threshold */}
                                    <div className="flex flex-col gap-2 p-4 bg-gray-50 dark:bg-base-200 rounded-xl border border-gray-100 dark:border-base-300">
                                        <div className="flex items-center justify-between w-full">
                                            <div className="flex items-center gap-2">
                                                <span className="text-sm font-bold text-gray-900 dark:text-base-content">
                                                    {t('proxy.config.experimental.context_compression_threshold_l1')}
                                                </span>
                                                <HelpTooltip text={t('proxy.config.experimental.context_compression_threshold_l1_tooltip')} />
                                            </div>
                                        </div>
                                        <DebouncedSlider
                                            min={0.1}
                                            max={1}
                                            step={0.05}
                                            className="range range-purple range-xs"
                                            value={appConfig.proxy.experimental?.context_compression_threshold_l1 || 0.4}
                                            onChange={(val) => updateExperimentalConfig({ context_compression_threshold_l1: val })}
                                        />
                                    </div>

                                    {/* L2 Threshold */}
                                    <div className="flex flex-col gap-2 p-4 bg-gray-50 dark:bg-base-200 rounded-xl border border-gray-100 dark:border-base-300">
                                        <div className="flex items-center justify-between w-full">
                                            <div className="flex items-center gap-2">
                                                <span className="text-sm font-bold text-gray-900 dark:text-base-content">
                                                    {t('proxy.config.experimental.context_compression_threshold_l2')}
                                                </span>
                                                <HelpTooltip text={t('proxy.config.experimental.context_compression_threshold_l2_tooltip')} />
                                            </div>
                                        </div>
                                        <DebouncedSlider
                                            min={0.1}
                                            max={1}
                                            step={0.05}
                                            className="range range-purple range-xs"
                                            value={appConfig.proxy.experimental?.context_compression_threshold_l2 || 0.55}
                                            onChange={(val) => updateExperimentalConfig({ context_compression_threshold_l2: val })}
                                        />
                                    </div>

                                    {/* L3 Threshold */}
                                    <div className="flex flex-col gap-2 p-4 bg-gray-50 dark:bg-base-200 rounded-xl border border-gray-100 dark:border-base-300">
                                        <div className="flex items-center justify-between w-full">
                                            <div className="flex items-center gap-2">
                                                <span className="text-sm font-bold text-gray-900 dark:text-base-content">
                                                    {t('proxy.config.experimental.context_compression_threshold_l3')}
                                                </span>
                                                <HelpTooltip text={t('proxy.config.experimental.context_compression_threshold_l3_tooltip')} />
                                            </div>
                                        </div>
                                        <DebouncedSlider
                                            min={0.1}
                                            max={1}
                                            step={0.05}
                                            className="range range-purple range-xs"
                                            value={appConfig.proxy.experimental?.context_compression_threshold_l3 || 0.7}
                                            onChange={(val) => updateExperimentalConfig({ context_compression_threshold_l3: val })}
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

                {/* 模型路由中心 */}
                {
                    !configLoading && !configError && appConfig && (
                        <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200 overflow-hidden">
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
                                    <div className="flex flex-wrap items-center gap-2 bg-white dark:bg-base-100 p-1.5 rounded-xl border border-gray-100 dark:border-gray-700/50 shadow-sm">
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
                                            className="px-3 md:px-4 py-1.5 rounded-lg text-xs font-bold transition-all flex items-center gap-1.5 bg-blue-600 hover:bg-blue-700 text-white shadow-sm hover:shadow active:scale-95 h-9"
                                            title={presetOptions.find(p => p.id === selectedPreset)?.description}
                                        >
                                            <Sparkles size={14} className="fill-white/20" />
                                            {t('proxy.router.apply_selected')}
                                        </button>

                                        <div className="w-[1px] h-5 bg-gray-200 dark:bg-gray-700 mx-1"></div>

                                        {/* 添加映射预设 */}
                                        <button
                                            onClick={() => setIsPresetManagerOpen(true)}
                                            className="p-2 rounded-lg text-gray-500 hover:text-green-600 hover:bg-green-50 dark:hover:bg-green-900/20 transition-all h-9 w-9 flex items-center justify-center border border-transparent hover:border-green-100 dark:hover:border-green-900/30"
                                            title={t('proxy.router.add_preset')}
                                        >
                                            <Plus size={16} />
                                        </button>

                                        {/* 删除当前预设（仅自定义预设） */}
                                        <button
                                            onClick={() => {
                                                if (selectedPreset.startsWith('custom_')) {
                                                    handleDeletePreset(selectedPreset);
                                                } else {
                                                    showToast(t('proxy.router.cannot_delete_builtin'), 'warning');
                                                }
                                            }}
                                            className={`p-2 rounded-lg transition-all h-9 w-9 flex items-center justify-center border border-transparent ${selectedPreset.startsWith('custom_')
                                                ? 'text-gray-500 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-900/20 hover:border-red-100 dark:hover:border-red-900/30'
                                                : 'text-gray-300 dark:text-gray-600 cursor-not-allowed'
                                                }`}
                                            title={selectedPreset.startsWith('custom_')
                                                ? t('proxy.router.delete_preset')
                                                : t('proxy.router.cannot_delete_builtin')}
                                            disabled={!selectedPreset.startsWith('custom_')}
                                        >
                                            <Trash2 size={16} />
                                        </button>

                                        <div className="w-[1px] h-5 bg-gray-200 dark:bg-gray-700 mx-1"></div>

                                        <button
                                            onClick={handleResetMapping}
                                            className="p-2 rounded-lg text-gray-500 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-900/20 transition-all h-9 w-9 flex items-center justify-center border border-transparent hover:border-red-100 dark:hover:border-red-900/30"
                                            title={t('proxy.router.reset_mapping')}
                                        >
                                            <RefreshCw size={16} />
                                        </button>
                                    </div>
                                </div>
                            </div>

                            <div className="p-3 space-y-3">
                                {/* 精确映射管理 */}
                                <div>
                                    {/* 后台任务模型配置 (Compact Mode) */}
                                    <div className="mb-4 pb-4 border-b border-gray-100 dark:border-base-200">
                                        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                                            <div className="flex-1">
                                                <h3 className="text-xs font-bold text-gray-700 dark:text-gray-300 flex items-center gap-2">
                                                    <Sparkles size={14} className="text-blue-500" />
                                                    {t('proxy.router.background_task_title')}
                                                </h3>
                                                <p className="text-[10px] text-gray-500 dark:text-gray-400 mt-0.5">
                                                    {t('proxy.router.background_task_desc')}
                                                </p>
                                            </div>

                                            <div className="flex items-center gap-2 w-full sm:w-auto min-w-[200px] max-w-sm">
                                                <div className="relative flex-1">
                                                    <GroupedSelect
                                                        value={appConfig.proxy.custom_mapping?.['internal-background-task'] || ''}
                                                        onChange={(val) => handleMappingUpdate('custom', 'internal-background-task', val)}
                                                        options={[
                                                            { value: '', label: 'Default (gemini-2.5-flash)', group: 'System' },
                                                            ...customMappingOptions
                                                        ]}
                                                        placeholder="Default (gemini-2.5-flash)"
                                                        className="font-mono text-[11px] h-8 dark:bg-base-200 w-full"
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
                                                                <div className="flex items-center gap-2.5 overflow-hidden flex-1">
                                                                    <span className="font-mono text-[10px] font-bold text-blue-600 dark:text-blue-400 truncate max-w-[140px]" title={key}>{key}</span>
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
                        </div>
                    )
                }

                {/* 多协议支持信息 */}
                {
                    !configLoading && !configError && appConfig && (
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
                                        onClick={() => setSelectedProtocol('openai')}
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
                                        onClick={() => setSelectedProtocol('anthropic')}
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
                                        onClick={() => setSelectedProtocol('gemini')}
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
                    )
                }


                {/* 支持模型与集成 */}
                {
                    !configLoading && !configError && appConfig && (
                        <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200 overflow-hidden mt-4">
                            <div className="px-4 py-2.5 border-b border-gray-100 dark:border-base-200">
                                <h2 className="text-base font-bold text-gray-900 dark:text-base-content flex items-center gap-2">
                                    <Terminal size={18} />
                                    {t('proxy.supported_models.title')}
                                </h2>
                            </div>

                            <div className="grid grid-cols-1 lg:grid-cols-3 gap-0 lg:divide-x dark:divide-gray-700">
                                {/* 左侧：模型列表 */}
                                <div className="col-span-2 p-0">
                                    <div className="overflow-x-auto">
                                        <table className="table w-full">
                                            <thead className="bg-gray-50/50 dark:bg-gray-800/50 text-gray-500 dark:text-gray-400">
                                                <tr>
                                                    <th className="w-10 pl-3"></th>
                                                    <th className="text-[11px] font-medium">{t('proxy.supported_models.model_name')}</th>
                                                    <th className="text-[11px] font-medium">{t('proxy.supported_models.model_id')}</th>
                                                    <th className="text-[11px] hidden sm:table-cell font-medium">{t('proxy.supported_models.description')}</th>
                                                    <th className="text-[11px] w-20 text-center font-medium">{t('proxy.supported_models.action')}</th>
                                                </tr>
                                            </thead>
                                            <tbody>
                                                {filteredModels.map((m) => (
                                                    <tr
                                                        key={m.id}
                                                        className={`hover:bg-blue-50/50 dark:hover:bg-blue-900/10 cursor-pointer transition-colors ${selectedModelId === m.id ? 'bg-blue-50/80 dark:bg-blue-900/20' : ''}`}
                                                        onClick={() => setSelectedModelId(m.id)}
                                                    >
                                                        <td className="pl-4 text-blue-500">{m.icon}</td>
                                                        <td className="font-bold text-xs">{m.name}</td>
                                                        <td className="font-mono text-[10px] text-gray-500">{m.id}</td>
                                                        <td className="text-[10px] text-gray-400 hidden sm:table-cell">{m.desc}</td>
                                                        <td className="text-center">
                                                            <button
                                                                className="btn btn-ghost btn-xs text-blue-500"
                                                                onClick={(e) => {
                                                                    e.stopPropagation();
                                                                    copyToClipboardHandler(m.id, `model-${m.id}`);
                                                                }}
                                                            >
                                                                {copied === `model-${m.id}` ? <CheckCircle size={14} /> : <div className="flex items-center gap-1 text-[10px] font-bold tracking-tight"><Copy size={12} /> {t('common.copy')}</div>}
                                                            </button>
                                                        </td>
                                                    </tr>
                                                ))}
                                            </tbody>
                                        </table>
                                    </div>
                                </div>

                                {/* 右侧：代码预览 */}
                                <div className="col-span-1 bg-gray-900 text-blue-100 flex flex-col h-[400px] lg:h-auto">
                                    <div className="p-3 border-b border-gray-800 flex items-center justify-between">
                                        <span className="text-xs font-bold text-gray-400 uppercase tracking-wider">{t('proxy.multi_protocol.quick_integration')}</span>
                                        <div className="flex gap-2">
                                            {/* 这里可以放 cURL/Python 切换，或者直接默认显示 Python，根据 selectedProtocol 决定 */}
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
            </div >
        </div >
    );
}
