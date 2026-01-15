import { useState, useEffect, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
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
    Layers,
    Puzzle,
    Zap,
    ArrowRight,
    Sparkles,
    Code,
    Activity,
    Check,
    X,
    Edit2
} from 'lucide-react';
import { AppConfig, ProxyConfig, StickySessionConfig, ExperimentalConfig } from '../types/config';
import HelpTooltip from '../components/common/HelpTooltip';
import ModalDialog from '../components/common/ModalDialog';
import { showToast } from '../components/common/ToastContainer';
import { cn } from '../utils/cn';
import { useProxyModels } from '../hooks/useProxyModels';
import GroupedSelect, { SelectOption } from '../components/common/GroupedSelect';

interface ProxyStatus {
    running: boolean;
    port: number;
    base_url: string;
    active_accounts: number;
}


interface CollapsibleCardProps {
    title: string;
    icon: React.ReactNode;
    enabled?: boolean;
    onToggle?: (enabled: boolean) => void;
    children: React.ReactNode;
    defaultExpanded?: boolean;
    rightElement?: React.ReactNode;
}

function CollapsibleCard({
    title,
    icon,
    enabled,
    onToggle,
    children,
    defaultExpanded = false,
    rightElement
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
                    {enabled === false && (
                        <div className="absolute inset-0 bg-gray-100/40 dark:bg-black/30 z-10 cursor-not-allowed" />
                    )}
                    <div className={enabled === false ? 'opacity-60 pointer-events-none select-none' : ''}>
                        {children}
                    </div>
                </div>
            </div>
        </div>
    );
}

export default function ApiProxy() {
    const { t } = useTranslation();
    const navigate = useNavigate();

    const { models } = useProxyModels();

    const [status, setStatus] = useState<ProxyStatus>({
        running: false,
        port: 0,
        base_url: '',
        active_accounts: 0,
    });

    const [appConfig, setAppConfig] = useState<AppConfig | null>(null);
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

    // Modal states
    const [isResetConfirmOpen, setIsResetConfirmOpen] = useState(false);
    const [isRegenerateKeyConfirmOpen, setIsRegenerateKeyConfirmOpen] = useState(false);
    const [isClearBindingsConfirmOpen, setIsClearBindingsConfirmOpen] = useState(false);

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
        const interval = setInterval(loadStatus, 3000);
        return () => clearInterval(interval);
    }, []);

    const loadConfig = async () => {
        try {
            const config = await invoke<AppConfig>('load_config');
            setAppConfig(config);
        } catch (error) {
            console.error('加载配置失败:', error);
        }
    };

    const loadStatus = async () => {
        try {
            const s = await invoke<ProxyStatus>('get_proxy_status');
            setStatus(s);
        } catch (error) {
            console.error('获取状态失败:', error);
        }
    };

    const saveConfig = async (newConfig: AppConfig) => {
        try {
            await invoke('save_config', { config: newConfig });
            setAppConfig(newConfig);
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


    // 应用预设映射 (通配符)
    const handleApplyPresets = async () => {
        if (!appConfig) return;

        const presets: Record<string, string> = {
            // OpenAI (通配符)
            "gpt-4*": "gemini-3-pro-high",
            "gpt-4o*": "gemini-3-flash",
            "gpt-3.5*": "gemini-2.5-flash",
            "o1-*": "gemini-3-pro-high",
            "o3-*": "gemini-3-pro-high",

            // Claude (通配符)
            "claude-3-5-sonnet-*": "claude-sonnet-4-5",
            "claude-3-opus-*": "claude-opus-4-5-thinking",
            "claude-opus-4-*": "claude-opus-4-5-thinking",
            "claude-haiku-*": "gemini-2.5-flash-lite",
            "claude-3-haiku-*": "gemini-2.5-flash-lite",
        };

        const newConfig = {
            ...appConfig.proxy,
            custom_mapping: { ...appConfig.proxy.custom_mapping, ...presets }
        };

        try {
            await invoke('update_model_mapping', { config: newConfig });
            setAppConfig({ ...appConfig, proxy: newConfig });
            showToast(t('proxy.router.presets_applied'), 'success');
        } catch (error) {
            console.error('Failed to apply presets:', error);
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
                    ...(appConfig.proxy.experimental || { enable_usage_scaling: true }),
                    ...updates
                }
            }
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

    const copyToClipboard = (text: string, label: string) => {
        navigator.clipboard.writeText(text).then(() => {
            setCopied(label);
            setTimeout(() => setCopied(null), 2000);
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


                {/* 配置区 */}
                {appConfig && (
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
                                            ? `${t('proxy.status.running')} (${status.active_accounts} ${t('common.accounts') || 'Accounts'})`
                                            : t('proxy.status.stopped')}
                                    </span>
                                </div>
                            </div>

                            {/* 控制按钮 */}
                            <div className="flex items-center gap-2">
                                {status.running && (
                                    <button
                                        onClick={() => navigate('/monitor')}
                                        className="px-3 py-1 rounded-lg text-xs font-medium transition-colors flex items-center gap-2 border bg-white text-gray-600 border-gray-200 hover:bg-gray-50 hover:text-blue-600"
                                    >
                                        <Activity size={14} />
                                        {t('monitor.open_monitor')}
                                    </button>
                                )}
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
                                            const timeout = Math.max(30, Math.min(3600, value));
                                            updateProxyConfig({ request_timeout: timeout });
                                        }}
                                        min={30}
                                        max={3600}
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
                                                    {t('proxy.config.auth.enabled')}
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
                                                onClick={() => copyToClipboard(appConfig.proxy.api_key, 'api_key')}
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


                        </div>
                    </div>
                )}

                {/* External Providers Integration */}
                {
                    appConfig && (
                        <div className="space-y-4">
                            <div className="px-1 flex items-center gap-2 text-gray-400">
                                <Layers size={14} />
                                <span className="text-[10px] font-bold uppercase tracking-widest">
                                    {t('proxy.config.external_providers.title', { defaultValue: 'External Providers' })}
                                </span>
                            </div>

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
                                                                <option value="">Select</option>
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
                                                        placeholder="From (e.g. claude-3-opus)"
                                                        value={zaiNewMappingFrom}
                                                        onChange={e => setZaiNewMappingFrom(e.target.value)}
                                                    />
                                                    <input
                                                        className="input input-xs input-bordered flex-1 font-mono"
                                                        placeholder="To (e.g. glm-4)"
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
                                                <button
                                                    onClick={handleClearSessionBindings}
                                                    className="text-[10px] text-indigo-500 hover:text-indigo-600 transition-colors flex items-center gap-1"
                                                >
                                                    <Trash2 size={12} />
                                                    {t('proxy.config.scheduling.clear_bindings')}
                                                </button>
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
                                        </div>
                                    </div>
                                </div>
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
                                </div>
                            </CollapsibleCard>
                        </div>
                    )
                }

                {/* 模型路由中心 */}
                {
                    appConfig && (
                        <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200 overflow-hidden">
                            <div className="px-4 py-2.5 border-b border-gray-100 dark:border-gray-700/50 bg-gray-50/50 dark:bg-gray-800/50">
                                <div className="flex items-center justify-between">
                                    <div>
                                        <h2 className="text-base font-bold flex items-center gap-2 text-gray-900 dark:text-base-content">
                                            <BrainCircuit size={18} className="text-blue-500" />
                                            {t('proxy.router.title')}
                                        </h2>
                                        <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                                            {t('proxy.router.subtitle_simple')}
                                        </p>
                                    </div>
                                    <div className="flex items-center gap-2">
                                        <button
                                            onClick={handleApplyPresets}
                                            className="px-3 py-1 rounded-lg text-xs font-medium transition-colors flex items-center gap-2 bg-blue-500 text-white hover:bg-blue-600 shadow-sm"
                                        >
                                            <Sparkles size={14} />
                                            {t('proxy.router.apply_presets')}
                                        </button>
                                        <button
                                            onClick={handleResetMapping}
                                            className="px-3 py-1 rounded-lg text-xs font-medium transition-colors flex items-center gap-2 bg-white dark:bg-base-100 border border-gray-200 dark:border-gray-700 text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-base-200 hover:text-blue-600 dark:hover:text-blue-400 hover:border-blue-200 dark:hover:border-blue-800 shadow-sm"
                                        >
                                            <RefreshCw size={14} />
                                            {t('proxy.router.reset_mapping')}
                                        </button>
                                    </div>
                                </div>
                            </div>

                            <div className="p-3 space-y-3">
                                {/* 精确映射管理 */}
                                <div>
                                    <div className="flex items-center justify-between mb-3">
                                        <h3 className="text-[10px] font-bold text-gray-400 uppercase tracking-widest flex items-center gap-2">
                                            <ArrowRight size={14} /> {t('proxy.router.custom_mappings')}
                                        </h3>
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
                                                        placeholder="Original (e.g. gpt-4 or gpt-4*)"
                                                        className="input input-xs input-bordered flex-1 font-mono text-[11px] bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 shadow-sm focus:border-blue-500 focus:ring-1 focus:ring-blue-500 transition-all placeholder:text-gray-400 dark:placeholder:text-gray-600 h-8"
                                                    />
                                                    <div className="w-full sm:w-48">
                                                        <GroupedSelect
                                                            value={customMappingValue}
                                                            onChange={setCustomMappingValue}
                                                            options={customMappingOptions}
                                                            placeholder={t('proxy.router.select_target_model') || 'Select Target Model'}
                                                            className="font-mono text-[11px] h-8 dark:bg-gray-800"
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
                    appConfig && status.running && (
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
                                            <button onClick={(e) => { e.stopPropagation(); copyToClipboard(`${status.base_url}/v1`, 'openai'); }} className="btn btn-ghost btn-xs">
                                                {copied === 'openai' ? <CheckCircle size={14} /> : <div className="flex items-center gap-1 text-[10px]"><Copy size={12} /> Base</div>}
                                            </button>
                                        </div>
                                        <div className="space-y-1">
                                            <div className="flex items-center justify-between hover:bg-black/5 dark:hover:bg-white/5 rounded p-0.5 group">
                                                <code className="text-[10px] opacity-70">/v1/chat/completions</code>
                                                <button onClick={(e) => { e.stopPropagation(); copyToClipboard(`${status.base_url}/v1/chat/completions`, 'openai-chat'); }} className="opacity-0 group-hover:opacity-100 transition-opacity">
                                                    {copied === 'openai-chat' ? <CheckCircle size={10} className="text-green-500" /> : <Copy size={10} />}
                                                </button>
                                            </div>
                                            <div className="flex items-center justify-between hover:bg-black/5 dark:hover:bg-white/5 rounded p-0.5 group">
                                                <code className="text-[10px] opacity-70">/v1/completions</code>
                                                <button onClick={(e) => { e.stopPropagation(); copyToClipboard(`${status.base_url}/v1/completions`, 'openai-compl'); }} className="opacity-0 group-hover:opacity-100 transition-opacity">
                                                    {copied === 'openai-compl' ? <CheckCircle size={10} className="text-green-500" /> : <Copy size={10} />}
                                                </button>
                                            </div>
                                            <div className="flex items-center justify-between hover:bg-black/5 dark:hover:bg-white/5 rounded p-0.5 group">
                                                <code className="text-[10px] opacity-70 font-bold text-blue-500">/v1/responses (Codex)</code>
                                                <button onClick={(e) => { e.stopPropagation(); copyToClipboard(`${status.base_url}/v1/responses`, 'openai-resp'); }} className="opacity-0 group-hover:opacity-100 transition-opacity">
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
                                            <button onClick={(e) => { e.stopPropagation(); copyToClipboard(`${status.base_url}/v1/messages`, 'anthropic'); }} className="btn btn-ghost btn-xs">
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
                                            <button onClick={(e) => { e.stopPropagation(); copyToClipboard(`${status.base_url}/v1beta/models`, 'gemini'); }} className="btn btn-ghost btn-xs">
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
                    appConfig && (
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
                                                                    copyToClipboard(m.id, `model-${m.id}`);
                                                                }}
                                                            >
                                                                {copied === `model-${m.id}` ? <CheckCircle size={14} /> : <div className="flex items-center gap-1 text-[10px]"><Copy size={12} /> Copy</div>}
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
                                            onClick={() => copyToClipboard(getPythonExample(selectedModelId), 'example-code')}
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
            </div >
        </div >
    );
}
