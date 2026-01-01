import { useState, useEffect, useMemo, useRef } from 'react';
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
    Code,
    Image as ImageIcon,
    BrainCircuit,
    Sparkles,
    Zap,
    Cpu,
    Puzzle,
    Wind,
    ArrowRight,
    Trash2,
    Layers,
    Activity
} from 'lucide-react';
import { AppConfig, ProxyConfig } from '../types/config';
import HelpTooltip from '../components/common/HelpTooltip';
import Switch from '../components/common/Switch';

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
        <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200 overflow-hidden transition-all duration-200 hover:shadow-md">
            <div
                className="px-5 py-4 flex items-center justify-between cursor-pointer bg-gray-50/50 dark:bg-base-200/30 hover:bg-gray-50 dark:hover:bg-base-200/50 transition-colors"
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
                        <div className={`text-xs px-2 py-0.5 rounded-full ${enabled ? 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400' : 'bg-gray-100 text-gray-500 dark:bg-gray-700 dark:text-gray-400'}`}>
                            {enabled ? t('common.enabled') : t('common.disabled')}
                        </div>
                    )}
                </div>

                <div className="flex items-center gap-4 no-expand">
                    {rightElement}

                    {enabled !== undefined && onToggle && (
                        <div className="flex items-center" onClick={(e) => e.stopPropagation()}>
                            <Switch checked={enabled} onCheckedChange={onToggle} ariaLabel={title} />
                        </div>
                    )}

                    <button
                        className={`p-1 rounded-lg hover:bg-gray-200 dark:hover:bg-gray-700 transition-all duration-200 ${isExpanded ? 'rotate-180' : ''}`}
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
                        <div className="absolute inset-0 bg-white/60 dark:bg-base-100/60 z-10 cursor-not-allowed" />
                    )}
                    <div className={enabled === false ? 'opacity-50 pointer-events-none select-none filter blur-[0.5px]' : ''}>
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

    const models = [
        // Gemini 3 Series
        {
            id: 'gemini-3-flash',
            name: 'Gemini 3 Flash',
            desc: t('proxy.model.flash_preview'),
            icon: <Zap size={16} />
        },
        {
            id: 'gemini-3-pro-high',
            name: 'Gemini 3 Pro High',
            desc: t('proxy.model.pro_high'),
            icon: <Cpu size={16} />
        },
        {
            id: 'gemini-3-pro-low',
            name: 'Gemini 3 Pro Low',
            desc: t('proxy.model.flash_lite'),
            icon: <Zap size={16} />
        },
        {
            id: 'gemini-3-pro-image',
            name: 'Gemini 3 Pro (Image)',
            desc: t('proxy.model.pro_image_1_1'),
            icon: <ImageIcon size={16} />
        },

        // Gemini 2.5 Series
        {
            id: 'gemini-2.5-flash',
            name: 'Gemini 2.5 Flash',
            desc: t('proxy.model.flash'),
            icon: <Zap size={16} />
        },
        {
            id: 'gemini-2.5-flash-lite',
            name: 'Gemini 2.5 Flash Lite',
            desc: t('proxy.model.flash_lite'),
            icon: <Zap size={16} />
        },
        {
            id: 'gemini-2.5-pro',
            name: 'Gemini 2.5 Pro',
            desc: t('proxy.model.pro_legacy'),
            icon: <Cpu size={16} />
        },
        {
            id: 'gemini-2.5-flash-thinking',
            name: 'Gemini 2.5 Flash (Thinking)',
            desc: t('proxy.model.claude_sonnet_thinking'),
            icon: <BrainCircuit size={16} />
        },

        // Claude Series
        {
            id: 'claude-sonnet-4-5',
            name: 'Claude 4.5 Sonnet',
            desc: t('proxy.model.claude_sonnet'),
            icon: <Sparkles size={16} />
        },
        {
            id: 'claude-sonnet-4-5-thinking',
            name: 'Claude 4.5 Sonnet (Thinking)',
            desc: t('proxy.model.claude_sonnet_thinking'),
            icon: <BrainCircuit size={16} />
        },
        {
            id: 'claude-opus-4-5-thinking',
            name: 'Claude 4.5 Opus (Thinking)',
            desc: t('proxy.model.claude_opus_thinking'),
            icon: <Cpu size={16} />
        }
    ];

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
    const [zaiModelsError, setZaiModelsError] = useState<string | null>(null);
    const [zaiDefaultModelIsCustom, setZaiDefaultModelIsCustom] = useState({
        opus: false,
        sonnet: false,
        haiku: false,
    });
    const [zaiOverrideToIsCustom, setZaiOverrideToIsCustom] = useState<Record<string, boolean>>({});
    const [zaiNewMappingFrom, setZaiNewMappingFrom] = useState('');
    const [zaiNewMappingTo, setZaiNewMappingTo] = useState('');
    const [zaiNewMappingToIsCustom, setZaiNewMappingToIsCustom] = useState(false);

    const zaiModelOptions = useMemo(() => {
        const unique = new Set(zaiAvailableModels);
        return Array.from(unique).sort();
    }, [zaiAvailableModels]);

    const zaiModelMapping = useMemo(() => {
        return appConfig?.proxy.zai?.model_mapping || {};
    }, [appConfig?.proxy.zai?.model_mapping]);

    useEffect(() => {
        if (!appConfig?.proxy?.zai) return;
        if (zaiModelOptions.length === 0) return;

        setZaiDefaultModelIsCustom({
            opus: !!appConfig.proxy.zai.models?.opus && !zaiModelOptions.includes(appConfig.proxy.zai.models.opus),
            sonnet: !!appConfig.proxy.zai.models?.sonnet && !zaiModelOptions.includes(appConfig.proxy.zai.models.sonnet),
            haiku: !!appConfig.proxy.zai.models?.haiku && !zaiModelOptions.includes(appConfig.proxy.zai.models.haiku),
        });
    }, [
        zaiModelOptions,
        appConfig?.proxy?.zai?.models?.opus,
        appConfig?.proxy?.zai?.models?.sonnet,
        appConfig?.proxy?.zai?.models?.haiku,
    ]);

    const effectiveAuthMode = useMemo(() => {
        if (!appConfig) return 'off' as const;
        const mode = (appConfig.proxy.auth_mode || 'off') as NonNullable<ProxyConfig['auth_mode']>;
        if (mode !== 'auto') return mode;
        return appConfig.proxy.allow_lan_access ? 'all_except_health' : 'off';
    }, [appConfig]);

    const saveSeqRef = useRef(0);

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
        const prevConfig = appConfig;
        const seq = ++saveSeqRef.current;
        // Optimistic UI update so switches/sliders feel responsive.
        setAppConfig(newConfig);
        try {
            await invoke('save_config', { config: newConfig });
        } catch (error) {
            console.error('保存配置失败:', error);
            // Revert only if this save is still the latest attempted write.
            if (saveSeqRef.current === seq && prevConfig) {
                setAppConfig(prevConfig);
            }
            alert('保存配置失败: ' + error);
        }
    };

    // 专门处理模型映射的热更新 (全量)
    const handleMappingUpdate = async (type: 'anthropic' | 'openai' | 'custom', key: string, value: string) => {
        if (!appConfig) return;

        const newConfig = { ...appConfig.proxy };
        if (type === 'anthropic') {
            newConfig.anthropic_mapping = { ...(newConfig.anthropic_mapping || {}), [key]: value };
        } else if (type === 'openai') {
            newConfig.openai_mapping = { ...(newConfig.openai_mapping || {}), [key]: value };
        } else if (type === 'custom') {
            newConfig.custom_mapping = { ...(newConfig.custom_mapping || {}), [key]: value };
        }

        try {
            await invoke('update_model_mapping', { config: newConfig });
            setAppConfig({ ...appConfig, proxy: newConfig });
        } catch (error) {
            console.error('Failed to update mapping:', error);
        }
    };

    const handleResetMapping = async () => {
        if (!appConfig || !confirm('确定要重置所有模型映射为系统默认吗？')) return;

        const newConfig = {
            ...appConfig.proxy,
            anthropic_mapping: {},
            openai_mapping: {},
            custom_mapping: {}
        };

        try {
            await invoke('update_model_mapping', { config: newConfig });
            setAppConfig({ ...appConfig, proxy: newConfig });
        } catch (error) {
            console.error('Failed to reset mapping:', error);
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
            setZaiAvailableModels([]);
            setZaiModelsError(String(error));
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
        setZaiOverrideToIsCustom((prev) => {
            if (!prev[from]) return prev;
            const next = { ...prev };
            delete next[from];
            return next;
        });
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
            alert(t('proxy.dialog.operate_failed', { error }));
        } finally {
            setLoading(false);
        }
    };

    const handleGenerateApiKey = async () => {
        if (confirm(t('proxy.dialog.confirm_regenerate'))) {
            try {
                const newKey = await invoke<string>('generate_api_key');
                updateProxyConfig({ api_key: newKey });
            } catch (error) {
                console.error('生成 API Key 失败:', error);
                alert(t('proxy.dialog.operate_failed', { error }));
            }
        }
    };

    const copyToClipboard = (text: string, label: string) => {
        navigator.clipboard.writeText(text).then(() => {
            setCopied(label);
            setTimeout(() => setCopied(null), 2000);
        });
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
                                                placement="top"
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
                                            const timeout = Math.max(30, Math.min(600, value));
                                            updateProxyConfig({ request_timeout: timeout });
                                        }}
                                        min={30}
                                        max={600}
                                        disabled={status.running}
                                        className="w-full px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 text-xs text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50 disabled:cursor-not-allowed"
                                    />
                                    <p className="mt-0.5 text-[10px] text-gray-500 dark:text-gray-400">
                                        {t('proxy.config.request_timeout_hint')}
                                    </p>
                                </div>
                                <div className="flex items-center">
                                    <div className="flex items-center gap-3">
                                        <Switch
                                            checked={appConfig.proxy.auto_start}
                                            onCheckedChange={(next) => updateProxyConfig({ auto_start: next })}
                                            ariaLabel={t('proxy.config.auto_start')}
                                        />
                                        <span className="text-xs font-medium text-gray-900 dark:text-base-content inline-flex items-center gap-1 cursor-default">
                                            {t('proxy.config.auto_start')}
                                            <HelpTooltip
                                                text={t('proxy.config.auto_start_tooltip')}
                                                ariaLabel={t('proxy.config.auto_start')}
                                                placement="right"
                                            />
                                        </span>
                                    </div>
                                </div>
                            </div>


                            {/* 局域网访问 & 访问授权 - 合并到同一行 */}
                            <div className="border-t border-gray-200 dark:border-base-300 pt-3 mt-3">
                                <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                                    {/* 允许局域网访问 */}
                                    <div className="space-y-2">
                                        <div className="flex items-center justify-between gap-3">
                                            <span className="text-xs font-medium text-gray-700 dark:text-gray-300 inline-flex items-center gap-1">
                                                {t('proxy.config.allow_lan_access')}
                                                <HelpTooltip
                                                    text={t('proxy.config.allow_lan_access_tooltip')}
                                                    ariaLabel={t('proxy.config.allow_lan_access')}
                                                    placement="right"
                                                />
                                            </span>
                                            <Switch
                                                checked={appConfig.proxy.allow_lan_access || false}
                                                onCheckedChange={(next) => updateProxyConfig({ allow_lan_access: next })}
                                                disabled={status.running}
                                                ariaLabel={t('proxy.config.allow_lan_access')}
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
                                        <div className="flex items-center justify-between gap-3">
                                            <label className="text-xs font-medium text-gray-700 dark:text-gray-300">
                                                <span className="inline-flex items-center gap-1">
                                                    {t('proxy.config.auth.title')}
                                                    <HelpTooltip
                                                        text={t('proxy.config.auth.title_tooltip')}
                                                        ariaLabel={t('proxy.config.auth.title')}
                                                        placement="right"
                                                    />
                                                </span>
                                            </label>
                                            <div className="flex items-center gap-2">
                                                <span className="text-[11px] text-gray-600 dark:text-gray-400 inline-flex items-center gap-1 cursor-default">
                                                    {t('proxy.config.auth.enabled')}
                                                    <HelpTooltip
                                                        text={t('proxy.config.auth.enabled_tooltip')}
                                                        ariaLabel={t('proxy.config.auth.enabled')}
                                                        placement="right"
                                                    />
                                                </span>
                                                <Switch
                                                    checked={(appConfig.proxy.auth_mode || 'off') !== 'off'}
                                                    onCheckedChange={(next) => {
                                                        const nextMode = next ? 'all_except_health' : 'off';
                                                        updateProxyConfig({ auth_mode: nextMode });
                                                    }}
                                                    ariaLabel={t('proxy.config.auth.enabled')}
                                                />
                                            </div>
                                        </div>

                                        <div>
                                            <label className="block text-[11px] text-gray-600 dark:text-gray-400 mb-1">
                                                <span className="inline-flex items-center gap-1">
                                                    {t('proxy.config.auth.mode')}
                                                    <HelpTooltip
                                                        text={t('proxy.config.auth.mode_tooltip')}
                                                        ariaLabel={t('proxy.config.auth.mode')}
                                                        placement="right"
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
                                            <p className="mt-0.5 text-[10px] text-gray-500 dark:text-gray-400">
                                                {t('proxy.config.auth.effective', {
                                                    mode: t(`proxy.config.auth.modes.${effectiveAuthMode}`),
                                                })}
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
                                            placement="top"
                                        />
                                    </span>
                                </label>
                                <div className="flex gap-2">
                                    <input
                                        type="text"
                                        value={appConfig.proxy.api_key}
                                        readOnly
                                        className="flex-1 px-2.5 py-1.5 border border-gray-300 dark:border-base-200 rounded-lg bg-gray-50 dark:bg-base-300 text-xs text-gray-600 dark:text-gray-400 font-mono"
                                    />
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
                                </div>
                                <p className="mt-0.5 text-[10px] text-amber-600 dark:text-amber-500">
                                    {t('proxy.config.warning_key')}
                                </p>
                            </div>

                            {/* Request Logging (access log) */}
                            <div className="pt-2">
                                <div className="flex items-center justify-between mb-1">
                                    <label className="block text-xs font-medium text-gray-700 dark:text-gray-300">
                                        <span className="inline-flex items-center gap-1">
                                            {t('proxy.config.access_log.title')}
                                            <HelpTooltip
                                                text={t('proxy.config.access_log.title_tooltip')}
                                                ariaLabel={t('proxy.config.access_log.title')}
                                                placement="right"
                                            />
                                        </span>
                                    </label>
                                    <label className="flex items-center cursor-pointer gap-2">
                                        <span className="text-[11px] text-gray-600 dark:text-gray-400 inline-flex items-center gap-1">
                                            {t('proxy.config.access_log.enabled')}
                                            <HelpTooltip
                                                text={t('proxy.config.access_log.enabled_tooltip')}
                                                ariaLabel={t('proxy.config.access_log.enabled')}
                                                placement="right"
                                            />
                                        </span>
                                        <Switch
                                            checked={!!appConfig.proxy.access_log_enabled}
                                            onCheckedChange={(next) => updateProxyConfig({ access_log_enabled: next })}
                                            ariaLabel={t('proxy.config.access_log.enabled')}
                                        />
                                    </label>
                                </div>
                                <p className="mt-0.5 text-[10px] text-gray-500 dark:text-gray-400">
                                    {t('proxy.config.access_log.hint')}
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
                                        <div className="flex items-start justify-between gap-3">
                                            <div>
                                                <h4 className="text-[11px] font-bold text-gray-400 uppercase tracking-widest inline-flex items-center gap-1">
                                                    {t('proxy.config.zai.models.title')}
                                                    <HelpTooltip
                                                        text={t('proxy.config.zai.models.title_tooltip')}
                                                        ariaLabel={t('proxy.config.zai.models.title')}
                                                        placement="right"
                                                    />
                                                </h4>
                                                <p className="mt-1 text-[10px] text-gray-500 dark:text-gray-400">
                                                    {t('proxy.config.zai.models.hint', { count: zaiModelOptions.length })}
                                                </p>
                                                {zaiModelsError && (
                                                    <p className="mt-1 text-[10px] text-amber-600 dark:text-amber-500">
                                                        {t('proxy.config.zai.models.error', { error: zaiModelsError })}
                                                    </p>
                                                )}
                                            </div>
                                            <button
                                                onClick={refreshZaiModels}
                                                disabled={zaiModelsLoading || !(appConfig.proxy.zai?.api_key || '').trim()}
                                                className="btn btn-ghost btn-xs gap-1"
                                            >
                                                <RefreshCw size={12} className={zaiModelsLoading ? 'animate-spin' : ''} />
                                                {zaiModelsLoading ? t('proxy.config.zai.models.refreshing') : t('proxy.config.zai.models.refresh')}
                                            </button>
                                        </div>

                                        <div className="grid grid-cols-1 md:grid-cols-3 gap-3 mt-3">
                                            {(
                                                [
                                                    {
                                                        id: 'opus' as const,
                                                        labelKey: 'proxy.config.zai.models.opus',
                                                        tooltipKey: 'proxy.config.zai.models.opus_tooltip',
                                                    },
                                                    {
                                                        id: 'sonnet' as const,
                                                        labelKey: 'proxy.config.zai.models.sonnet',
                                                        tooltipKey: 'proxy.config.zai.models.sonnet_tooltip',
                                                    },
                                                    {
                                                        id: 'haiku' as const,
                                                        labelKey: 'proxy.config.zai.models.haiku',
                                                        tooltipKey: 'proxy.config.zai.models.haiku_tooltip',
                                                    },
                                                ] as const
                                            ).map(({ id, labelKey, tooltipKey }) => (
                                                <div key={id} className="space-y-1">
                                                    <label className="text-[10px] text-gray-500 dark:text-gray-400 inline-flex items-center gap-1">
                                                        {t(labelKey)}
                                                        <HelpTooltip text={t(tooltipKey)} ariaLabel={t(labelKey)} placement="right" iconSize={12} />
                                                    </label>

                                                    {zaiModelOptions.length > 0 ? (
                                                        <div className="space-y-1">
                                                            <select
                                                                className="select select-xs select-bordered w-full font-mono"
                                                                value={
                                                                    zaiDefaultModelIsCustom[id]
                                                                        ? '__custom__'
                                                                        : (appConfig.proxy.zai?.models?.[id] || '')
                                                                }
                                                                onChange={(e) => {
                                                                    const next = e.target.value;
                                                                    if (next === '__custom__') {
                                                                        setZaiDefaultModelIsCustom((prev) => ({ ...prev, [id]: true }));
                                                                        return;
                                                                    }
                                                                    setZaiDefaultModelIsCustom((prev) => ({ ...prev, [id]: false }));
                                                                    updateZaiDefaultModels({ [id]: next } as any);
                                                                }}
                                                                disabled={zaiModelsLoading}
                                                            >
                                                                <option value="">{t('proxy.config.zai.models.select_placeholder')}</option>
                                                                {zaiModelOptions.map((modelId) => (
                                                                    <option key={modelId} value={modelId}>
                                                                        {modelId}
                                                                    </option>
                                                                ))}
                                                                <option value="__custom__">{t('proxy.config.zai.models.custom_option')}</option>
                                                            </select>

                                                            {zaiDefaultModelIsCustom[id] && (
                                                                <input
                                                                    type="text"
                                                                    className="input input-xs input-bordered w-full font-mono"
                                                                    value={appConfig.proxy.zai?.models?.[id] || ''}
                                                                    onChange={(e) => updateZaiDefaultModels({ [id]: e.target.value } as any)}
                                                                    placeholder={t('proxy.config.zai.models.to_placeholder')}
                                                                />
                                                            )}
                                                        </div>
                                                    ) : (
                                                        <input
                                                            type="text"
                                                            className="input input-xs input-bordered w-full font-mono"
                                                            value={appConfig.proxy.zai?.models?.[id] || ''}
                                                            onChange={(e) => updateZaiDefaultModels({ [id]: e.target.value } as any)}
                                                            placeholder={t('proxy.config.zai.models.to_placeholder')}
                                                        />
                                                    )}
                                                </div>
                                            ))}
                                        </div>

                                        <details className="mt-3 group">
                                            <summary className="cursor-pointer text-[10px] text-gray-500 hover:text-blue-500 transition-colors inline-flex items-center gap-1 select-none">
                                                {t('proxy.config.zai.models.advanced_title')}
                                                <HelpTooltip
                                                    text={t('proxy.config.zai.models.advanced_tooltip')}
                                                    ariaLabel={t('proxy.config.zai.models.advanced_title')}
                                                    placement="right"
                                                    iconSize={12}
                                                />
                                            </summary>
                                            <div className="mt-2 space-y-2 p-2 bg-gray-50 dark:bg-base-200/50 rounded-lg">
                                                {Object.keys(zaiModelMapping).length === 0 ? (
                                                    <p className="text-[10px] text-gray-500 dark:text-gray-400">
                                                        {t('proxy.config.zai.models.empty')}
                                                    </p>
                                                ) : (
                                                    <div className="space-y-3">
                                                        {Object.entries(zaiModelMapping)
                                                            .sort(([a], [b]) => a.localeCompare(b))
                                                            .map(([from, to]) => (
                                                                <div key={from} className="grid grid-cols-1 md:grid-cols-12 gap-2 items-end">
                                                                    <div className="md:col-span-5">
                                                                        <label className="block text-[10px] text-gray-500 dark:text-gray-400 mb-1">
                                                                            {t('proxy.config.zai.models.from_label')}
                                                                        </label>
                                                                        <input
                                                                            type="text"
                                                                            value={from}
                                                                            readOnly
                                                                            className="input input-xs input-bordered w-full font-mono bg-gray-50 dark:bg-base-300"
                                                                        />
                                                                    </div>
                                                                    <div className="md:col-span-6">
                                                                        <label className="block text-[10px] text-gray-500 dark:text-gray-400 mb-1">
                                                                            {t('proxy.config.zai.models.to_label')}
                                                                        </label>
                                                                        {zaiModelOptions.length > 0 ? (
                                                                            <div className="space-y-1">
                                                                                <select
                                                                                    className="select select-xs select-bordered w-full font-mono"
                                                                                    value={
                                                                                        (zaiOverrideToIsCustom[from] ??
                                                                                            (!to || !zaiModelOptions.includes(to)))
                                                                                            ? '__custom__'
                                                                                            : to
                                                                                    }
                                                                                    onChange={(e) => {
                                                                                        const next = e.target.value;
                                                                                        if (next === '__custom__') {
                                                                                            setZaiOverrideToIsCustom((prev) => ({
                                                                                                ...prev,
                                                                                                [from]: true,
                                                                                            }));
                                                                                            return;
                                                                                        }
                                                                                        setZaiOverrideToIsCustom((prev) => ({
                                                                                            ...prev,
                                                                                            [from]: false,
                                                                                        }));
                                                                                        upsertZaiModelMapping(from, next);
                                                                                    }}
                                                                                    disabled={zaiModelsLoading}
                                                                                >
                                                                                    <option value="">
                                                                                        {t('proxy.config.zai.models.select_placeholder')}
                                                                                    </option>
                                                                                    {zaiModelOptions.map((modelId) => (
                                                                                        <option key={modelId} value={modelId}>
                                                                                            {modelId}
                                                                                        </option>
                                                                                    ))}
                                                                                    <option value="__custom__">
                                                                                        {t('proxy.config.zai.models.custom_option')}
                                                                                    </option>
                                                                                </select>
                                                                                {(zaiOverrideToIsCustom[from] ??
                                                                                    (!to || !zaiModelOptions.includes(to))) && (
                                                                                    <input
                                                                                        type="text"
                                                                                        className="input input-xs input-bordered w-full font-mono"
                                                                                        value={to}
                                                                                        onChange={(e) =>
                                                                                            upsertZaiModelMapping(from, e.target.value)
                                                                                        }
                                                                                        placeholder={t('proxy.config.zai.models.to_placeholder')}
                                                                                    />
                                                                                )}
                                                                            </div>
                                                                        ) : (
                                                                            <input
                                                                                type="text"
                                                                                className="input input-xs input-bordered w-full font-mono"
                                                                                value={to}
                                                                                onChange={(e) => upsertZaiModelMapping(from, e.target.value)}
                                                                                placeholder={t('proxy.config.zai.models.to_placeholder')}
                                                                            />
                                                                        )}
                                                                    </div>
                                                                    <div className="md:col-span-1 flex md:justify-end">
                                                                        <button
                                                                            type="button"
                                                                            className="btn btn-ghost btn-xs text-gray-500 hover:text-red-500"
                                                                            onClick={() => removeZaiModelMapping(from)}
                                                                            title={t('common.delete')}
                                                                        >
                                                                            <Trash2 size={14} />
                                                                        </button>
                                                                    </div>
                                                                </div>
                                                            ))}
                                                    </div>
                                                )}

                                                <div className="pt-3 border-t border-gray-200/50">
                                                    <div className="grid grid-cols-1 md:grid-cols-12 gap-2 items-end">
                                                        <div className="md:col-span-5">
                                                            <label className="block text-[10px] text-gray-500 dark:text-gray-400 mb-1">
                                                                {t('proxy.config.zai.models.from_label')}
                                                            </label>
                                                            <input
                                                                className="input input-xs input-bordered w-full font-mono"
                                                                placeholder={t('proxy.config.zai.models.from_placeholder')}
                                                                value={zaiNewMappingFrom}
                                                                onChange={(e) => setZaiNewMappingFrom(e.target.value)}
                                                            />
                                                        </div>
                                                        <div className="md:col-span-6">
                                                            <label className="block text-[10px] text-gray-500 dark:text-gray-400 mb-1">
                                                                {t('proxy.config.zai.models.to_label')}
                                                            </label>
                                                            {zaiModelOptions.length > 0 ? (
                                                                <div className="space-y-1">
                                                                    <select
                                                                        className="select select-xs select-bordered w-full font-mono"
                                                                        value={
                                                                            zaiNewMappingToIsCustom ? '__custom__' : (zaiNewMappingTo || '')
                                                                        }
                                                                        onChange={(e) => {
                                                                            const next = e.target.value;
                                                                            if (next === '__custom__') {
                                                                                setZaiNewMappingToIsCustom(true);
                                                                                return;
                                                                            }
                                                                            setZaiNewMappingToIsCustom(false);
                                                                            setZaiNewMappingTo(next);
                                                                        }}
                                                                        disabled={zaiModelsLoading}
                                                                    >
                                                                        <option value="">
                                                                            {t('proxy.config.zai.models.select_placeholder')}
                                                                        </option>
                                                                        {zaiModelOptions.map((modelId) => (
                                                                            <option key={modelId} value={modelId}>
                                                                                {modelId}
                                                                            </option>
                                                                        ))}
                                                                        <option value="__custom__">
                                                                            {t('proxy.config.zai.models.custom_option')}
                                                                        </option>
                                                                    </select>
                                                                    {zaiNewMappingToIsCustom && (
                                                                        <input
                                                                            type="text"
                                                                            className="input input-xs input-bordered w-full font-mono"
                                                                            value={zaiNewMappingTo}
                                                                            onChange={(e) => setZaiNewMappingTo(e.target.value)}
                                                                            placeholder={t('proxy.config.zai.models.to_placeholder')}
                                                                        />
                                                                    )}
                                                                </div>
                                                            ) : (
                                                                <input
                                                                    type="text"
                                                                    className="input input-xs input-bordered w-full font-mono"
                                                                    value={zaiNewMappingTo}
                                                                    onChange={(e) => setZaiNewMappingTo(e.target.value)}
                                                                    placeholder={t('proxy.config.zai.models.to_placeholder')}
                                                                />
                                                            )}
                                                        </div>
                                                        <div className="md:col-span-1 flex md:justify-end">
                                                            <button
                                                                type="button"
                                                                className="btn btn-xs btn-primary"
                                                                onClick={() => {
                                                                    const from = zaiNewMappingFrom.trim();
                                                                    const to = zaiNewMappingTo.trim();
                                                                    if (!from || !to) return;
                                                                    upsertZaiModelMapping(from, to);
                                                                    setZaiNewMappingFrom('');
                                                                    setZaiNewMappingTo('');
                                                                }}
                                                            >
                                                                {t('proxy.config.zai.models.add_rule')}
                                                            </button>
                                                        </div>
                                                    </div>
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
                                    <div className="flex gap-2 text-[10px] text-gray-400">
                                        {['web_search', 'web_reader', 'zread', 'vision'].map(f =>
                                            appConfig.proxy.zai?.mcp?.[(f + '_enabled') as keyof typeof appConfig.proxy.zai.mcp] && (
                                                <span key={f} className="bg-gray-100 dark:bg-base-200 px-1.5 py-0.5 rounded text-gray-600 dark:text-gray-400">
                                                    {t(`proxy.config.zai.mcp.${f}`).split(' ')[0]}
                                                </span>
                                            )
                                        )}
                                    </div>
                                }
                            >
                                <div className="space-y-3">
                                    <div className="space-y-1">
                                        <label className="text-[11px] font-medium text-gray-500 dark:text-gray-400 inline-flex items-center gap-1">
                                            {t('proxy.config.zai.mcp.api_key_override')}
                                            <HelpTooltip
                                                text={t('proxy.config.zai.mcp.api_key_override_tooltip')}
                                                ariaLabel={t('proxy.config.zai.mcp.api_key_override')}
                                                placement="right"
                                                iconSize={12}
                                            />
                                        </label>
                                        <input
                                            type="password"
                                            className="input input-sm input-bordered w-full font-mono text-xs"
                                            value={appConfig.proxy.zai?.mcp?.api_key_override || ''}
                                            onChange={(e) =>
                                                updateZaiGeneralConfig({
                                                    mcp: {
                                                        ...(appConfig.proxy.zai?.mcp || {}),
                                                        api_key_override: e.target.value,
                                                    } as any,
                                                })
                                            }
                                            placeholder={t('proxy.config.zai.mcp.api_key_override_placeholder')}
                                            disabled={!appConfig.proxy.zai?.mcp?.enabled}
                                        />
                                        <p className="text-[10px] text-gray-500 dark:text-gray-400">
                                            {t('proxy.config.zai.mcp.api_key_override_hint')}
                                        </p>
                                    </div>

                                    <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
                                        <label className="flex items-center gap-2 border border-gray-100 dark:border-base-200 p-2 rounded-lg cursor-pointer hover:bg-gray-50 dark:hover:bg-base-200/50 transition-colors">
                                            <input
                                                type="checkbox"
                                                className="checkbox checkbox-xs checkbox-primary rounded-md"
                                                checked={!!appConfig.proxy.zai?.mcp?.web_search_enabled}
                                                onChange={(e) => updateZaiGeneralConfig({ mcp: { ...(appConfig.proxy.zai?.mcp || {}), web_search_enabled: e.target.checked } as any })}
                                            />
                                            <span className="text-xs">{t('proxy.config.zai.mcp.web_search')}</span>
                                        </label>
                                        <label className="flex items-center gap-2 border border-gray-100 dark:border-base-200 p-2 rounded-lg cursor-pointer hover:bg-gray-50 dark:hover:bg-base-200/50 transition-colors">
                                            <input
                                                type="checkbox"
                                                className="checkbox checkbox-xs checkbox-primary rounded-md"
                                                checked={!!appConfig.proxy.zai?.mcp?.web_reader_enabled}
                                                onChange={(e) => updateZaiGeneralConfig({ mcp: { ...(appConfig.proxy.zai?.mcp || {}), web_reader_enabled: e.target.checked } as any })}
                                            />
                                            <span className="text-xs">{t('proxy.config.zai.mcp.web_reader')}</span>
                                        </label>
                                        <label className="flex items-center gap-2 border border-gray-100 dark:border-base-200 p-2 rounded-lg cursor-pointer hover:bg-gray-50 dark:hover:bg-base-200/50 transition-colors">
                                            <input
                                                type="checkbox"
                                                className="checkbox checkbox-xs checkbox-primary rounded-md"
                                                checked={!!appConfig.proxy.zai?.mcp?.zread_enabled}
                                                onChange={(e) => updateZaiGeneralConfig({ mcp: { ...(appConfig.proxy.zai?.mcp || {}), zread_enabled: e.target.checked } as any })}
                                            />
                                            <span className="text-xs">{t('proxy.config.zai.mcp.zread')}</span>
                                        </label>
                                        <label className="flex items-center gap-2 border border-gray-100 dark:border-base-200 p-2 rounded-lg cursor-pointer hover:bg-gray-50 dark:hover:bg-base-200/50 transition-colors">
                                            <input
                                                type="checkbox"
                                                className="checkbox checkbox-xs checkbox-primary rounded-md"
                                                checked={!!appConfig.proxy.zai?.mcp?.vision_enabled}
                                                onChange={(e) => updateZaiGeneralConfig({ mcp: { ...(appConfig.proxy.zai?.mcp || {}), vision_enabled: e.target.checked } as any })}
                                            />
                                            <span className="text-xs">{t('proxy.config.zai.mcp.vision')}</span>
                                        </label>
                                    </div>

                                    <div className="space-y-1">
                                        <label className="text-[11px] font-medium text-gray-500 dark:text-gray-400 inline-flex items-center gap-1">
                                            {t('proxy.config.zai.mcp.web_reader_url_normalization')}
                                            <HelpTooltip
                                                text={t('proxy.config.zai.mcp.web_reader_url_normalization_tooltip')}
                                                ariaLabel={t('proxy.config.zai.mcp.web_reader_url_normalization')}
                                                placement="right"
                                                iconSize={12}
                                            />
                                        </label>
                                        <select
                                            className="select select-sm select-bordered w-full text-xs"
                                            value={appConfig.proxy.zai?.mcp?.web_reader_url_normalization || 'off'}
                                            onChange={(e) =>
                                                updateZaiGeneralConfig({
                                                    mcp: {
                                                        ...(appConfig.proxy.zai?.mcp || {}),
                                                        web_reader_url_normalization: e.target.value as any,
                                                    } as any,
                                                })
                                            }
                                            disabled={!appConfig.proxy.zai?.mcp?.web_reader_enabled}
                                        >
                                            <option value="off">
                                                {t('proxy.config.zai.mcp.web_reader_url_normalization_options.off')}
                                            </option>
                                            <option value="strip_tracking_query">
                                                {t('proxy.config.zai.mcp.web_reader_url_normalization_options.strip_tracking_query')}
                                            </option>
                                            <option value="strip_query">
                                                {t('proxy.config.zai.mcp.web_reader_url_normalization_options.strip_query')}
                                            </option>
                                        </select>
                                        <p className="text-[10px] text-gray-500 dark:text-gray-400">
                                            {t('proxy.config.zai.mcp.web_reader_url_normalization_hint')}
                                        </p>
                                    </div>

                                    {(appConfig.proxy.zai?.mcp?.enabled &&
                                        (appConfig.proxy.zai?.mcp?.web_search_enabled ||
                                            appConfig.proxy.zai?.mcp?.web_reader_enabled ||
                                            appConfig.proxy.zai?.mcp?.zread_enabled ||
                                            appConfig.proxy.zai?.mcp?.vision_enabled)) && (
                                        <div className="bg-gray-50 dark:bg-base-200/50 rounded-lg p-3 text-[10px] font-mono text-gray-500">
                                            <div className="mb-1 font-bold text-gray-400 uppercase tracking-wider">
                                                {t('proxy.config.zai.mcp.local_endpoints')}
                                            </div>
                                            <div className="space-y-0.5 select-all">
                                                {appConfig.proxy.zai?.mcp?.web_search_enabled && (
                                                    <div>
                                                        http://127.0.0.1:
                                                        {status.running ? status.port : (appConfig.proxy.port || 8045)}
                                                        /mcp/web_search_prime/mcp
                                                    </div>
                                                )}
                                                {appConfig.proxy.zai?.mcp?.web_reader_enabled && (
                                                    <div>
                                                        http://127.0.0.1:
                                                        {status.running ? status.port : (appConfig.proxy.port || 8045)}
                                                        /mcp/web_reader/mcp
                                                    </div>
                                                )}
                                                {appConfig.proxy.zai?.mcp?.zread_enabled && (
                                                    <div>
                                                        http://127.0.0.1:
                                                        {status.running ? status.port : (appConfig.proxy.port || 8045)}
                                                        /mcp/zread/mcp
                                                    </div>
                                                )}
                                                {appConfig.proxy.zai?.mcp?.vision_enabled && (
                                                    <div>
                                                        http://127.0.0.1:
                                                        {status.running ? status.port : (appConfig.proxy.port || 8045)}
                                                        /mcp/zai-mcp-server/mcp
                                                    </div>
                                                )}
                                            </div>
                                        </div>
                                    )}
                                </div>
                            </CollapsibleCard>
                        </div>
                    )
                }

                {/* 模型路由中心 */}
                {
                    appConfig && (
                        <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200 overflow-hidden">
                            <div className="px-4 py-2.5 border-b border-gray-100 dark:border-base-200 bg-gray-50/50 dark:bg-base-200/50">
                                <div className="flex items-center justify-between">
                                    <div>
                                        <h2 className="text-base font-bold flex items-center gap-2 text-gray-900 dark:text-base-content">
                                            <BrainCircuit size={18} className="text-blue-500" />
                                            {t('proxy.router.title')}
                                        </h2>
                                        <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                                            {t('proxy.router.subtitle')}
                                        </p>
                                    </div>
                                    <button
                                        onClick={handleResetMapping}
                                        className="px-3 py-1 rounded-lg text-xs font-medium transition-colors flex items-center gap-2 bg-white dark:bg-base-100 border border-gray-200 dark:border-gray-700 text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-base-200 hover:text-blue-600 dark:hover:text-blue-400 hover:border-blue-200 dark:hover:border-blue-800 shadow-sm"
                                    >
                                        <RefreshCw size={14} />
                                        {t('proxy.router.reset_mapping')}
                                    </button>
                                </div>
                            </div>

                            <div className="p-3 space-y-3">
                                {/* 分组映射区域 */}
                                <div>
                                    <h3 className="text-[10px] font-bold text-gray-400 uppercase tracking-widest mb-2 flex items-center gap-2">
                                        <Layers size={14} /> {t('proxy.router.group_title')}
                                    </h3>
                                    <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-5 gap-3">
                                        {/* Claude 4.5 系列 */}
                                        <div className="bg-gradient-to-br from-blue-50 to-indigo-50 dark:from-blue-900/10 dark:to-indigo-900/10 p-3 rounded-xl border border-blue-100 dark:border-blue-800/30 relative overflow-hidden group hover:border-blue-400 transition-all duration-300">
                                            <div className="flex items-center gap-3 mb-3">
                                                <div className="w-8 h-8 rounded-lg bg-blue-600 flex items-center justify-center text-white shadow-lg shadow-blue-500/30">
                                                    <BrainCircuit size={16} />
                                                </div>
                                                <div>
                                                    <div className="text-xs font-bold text-gray-900 dark:text-base-content">{t('proxy.router.groups.claude_45.name')}</div>
                                                    <div className="text-[10px] text-gray-500 line-clamp-1">{t('proxy.router.groups.claude_45.desc')}</div>
                                                </div>
                                            </div>
                                            <select
                                                className="select select-sm select-bordered w-full font-mono text-[11px] bg-white/80 dark:bg-base-100/80 backdrop-blur-sm"
                                                value={appConfig.proxy.anthropic_mapping?.["claude-4.5-series"] || ""}
                                                onChange={(e) => handleMappingUpdate('anthropic', 'claude-4.5-series', e.target.value)}
                                            >
                                                <option value="">gemini-3-pro-high{t('proxy.router.default_suffix', ' (Default)')}</option>
                                                <optgroup label="Claude 4.5">
                                                    <option value="claude-opus-4-5-thinking">claude-opus-4-5-thinking</option>
                                                    <option value="claude-sonnet-4-5">claude-sonnet-4-5</option>
                                                    <option value="claude-sonnet-4-5-thinking">claude-sonnet-4-5-thinking</option>
                                                </optgroup>
                                                <optgroup label="Gemini 3">
                                                    <option value="gemini-3-pro-high">gemini-3-pro-high</option>
                                                    <option value="gemini-3-pro-low">gemini-3-pro-low</option>
                                                    <option value="gemini-3-flash">gemini-3-flash</option>
                                                </optgroup>
                                                <optgroup label="Gemini 2.5">
                                                    <option value="gemini-2.5-pro">gemini-2.5-pro</option>
                                                    <option value="gemini-2.5-flash">gemini-2.5-flash</option>
                                                    <option value="gemini-2.5-flash-thinking">gemini-2.5-flash-thinking</option>
                                                    <option value="gemini-2.5-flash-lite">gemini-2.5-flash-lite</option>
                                                </optgroup>
                                            </select>
                                        </div>

                                        {/* Claude 3.5 系列 */}
                                        <div className="bg-gradient-to-br from-purple-50 to-pink-50 dark:from-purple-900/10 dark:to-pink-900/10 p-3 rounded-xl border border-purple-100 dark:border-purple-800/30 relative overflow-hidden group hover:border-purple-400 transition-all duration-300">
                                            <div className="flex items-center gap-3 mb-3">
                                                <div className="w-8 h-8 rounded-lg bg-purple-600 flex items-center justify-center text-white shadow-lg shadow-purple-500/30">
                                                    <Puzzle size={16} />
                                                </div>
                                                <div>
                                                    <div className="text-xs font-bold text-gray-900 dark:text-base-content">{t('proxy.router.groups.claude_35.name')}</div>
                                                    <div className="text-[10px] text-gray-500 line-clamp-1">{t('proxy.router.groups.claude_35.desc')}</div>
                                                </div>
                                            </div>
                                            <select
                                                className="select select-sm select-bordered w-full font-mono text-[11px] bg-white/80 dark:bg-base-100/80 backdrop-blur-sm"
                                                value={appConfig.proxy.anthropic_mapping?.["claude-3.5-series"] || ""}
                                                onChange={(e) => handleMappingUpdate('anthropic', 'claude-3.5-series', e.target.value)}
                                            >
                                                <option value="">claude-sonnet-4-5-thinking{t('proxy.router.default_suffix', ' (Default)')}</option>
                                                <optgroup label="Claude 4.5">
                                                    <option value="claude-opus-4-5-thinking">claude-opus-4-5-thinking</option>
                                                    <option value="claude-sonnet-4-5">claude-sonnet-4-5</option>
                                                    <option value="claude-sonnet-4-5-thinking">claude-sonnet-4-5-thinking</option>
                                                </optgroup>
                                                <optgroup label="Gemini 3">
                                                    <option value="gemini-3-pro-high">gemini-3-pro-high</option>
                                                    <option value="gemini-3-pro-low">gemini-3-pro-low</option>
                                                    <option value="gemini-3-flash">gemini-3-flash</option>
                                                </optgroup>
                                                <optgroup label="Gemini 2.5">
                                                    <option value="gemini-2.5-pro">gemini-2.5-pro</option>
                                                    <option value="gemini-2.5-flash">gemini-2.5-flash</option>
                                                    <option value="gemini-2.5-flash-thinking">gemini-2.5-flash-thinking</option>
                                                    <option value="gemini-2.5-flash-lite">gemini-2.5-flash-lite</option>
                                                </optgroup>
                                            </select>
                                        </div>

                                        {/* GPT-4 系列 */}
                                        <div className="bg-gradient-to-br from-indigo-50 to-blue-50 dark:from-indigo-900/10 dark:to-blue-900/10 p-3 rounded-xl border border-indigo-100 dark:border-indigo-800/30 relative overflow-hidden group hover:border-indigo-400 transition-all duration-300">
                                            <div className="flex items-center gap-3 mb-3">
                                                <div className="w-8 h-8 rounded-lg bg-indigo-600 flex items-center justify-center text-white shadow-lg shadow-indigo-500/30">
                                                    <Zap size={16} />
                                                </div>
                                                <div>
                                                    <div className="text-xs font-bold text-gray-900 dark:text-base-content">{t('proxy.router.groups.gpt_4.name')}</div>
                                                    <div className="text-[10px] text-gray-500 line-clamp-1">{t('proxy.router.groups.gpt_4.desc')}</div>
                                                </div>
                                            </div>
                                            <select
                                                className="select select-sm select-bordered w-full font-mono text-[11px] bg-white/80 dark:bg-base-100/80 backdrop-blur-sm"
                                                value={appConfig.proxy.openai_mapping?.["gpt-4-series"] || ""}
                                                onChange={(e) => handleMappingUpdate('openai', 'gpt-4-series', e.target.value)}
                                            >
                                                <option value="">gemini-3-pro-high{t('proxy.router.default_suffix', ' (Default)')}</option>
                                                <optgroup label="Gemini 3 (推荐)">
                                                    <option value="gemini-3-pro-high">gemini-3-pro-high (高质量)</option>
                                                    <option value="gemini-3-pro-low">gemini-3-pro-low (均衡)</option>
                                                    <option value="gemini-3-flash">gemini-3-flash (快速)</option>
                                                </optgroup>
                                            </select>
                                            <p className="mt-1 text-[9px] text-indigo-500">{t('proxy.router.gemini3_only_warning')}</p>
                                        </div>

                                        {/* GPT-4o / 3.5 系列 */}
                                        <div className="bg-gradient-to-br from-emerald-50 to-green-50 dark:from-emerald-900/10 dark:to-green-900/10 p-3 rounded-xl border border-emerald-100 dark:border-emerald-800/30 relative overflow-hidden group hover:border-emerald-400 transition-all duration-300">
                                            <div className="flex items-center gap-3 mb-3">
                                                <div className="w-8 h-8 rounded-lg bg-emerald-600 flex items-center justify-center text-white shadow-lg shadow-emerald-500/30">
                                                    <Wind size={16} />
                                                </div>
                                                <div>
                                                    <div className="text-xs font-bold text-gray-900 dark:text-base-content">{t('proxy.router.groups.gpt_4o.name')}</div>
                                                    <div className="text-[10px] text-gray-500 line-clamp-1">{t('proxy.router.groups.gpt_4o.desc')}</div>
                                                </div>
                                            </div>
                                            <select
                                                className="select select-sm select-bordered w-full font-mono text-[11px] bg-white/80 dark:bg-base-100/80 backdrop-blur-sm"
                                                value={appConfig.proxy.openai_mapping?.["gpt-4o-series"] || ""}
                                                onChange={(e) => handleMappingUpdate('openai', 'gpt-4o-series', e.target.value)}
                                            >
                                                <option value="">gemini-3-flash{t('proxy.router.default_suffix', ' (Default)')}</option>
                                                <optgroup label="Gemini 3 (推荐)">
                                                    <option value="gemini-3-flash">gemini-3-flash (快速)</option>
                                                    <option value="gemini-3-pro-high">gemini-3-pro-high (高质量)</option>
                                                    <option value="gemini-3-pro-low">gemini-3-pro-low (均衡)</option>
                                                </optgroup>
                                            </select>
                                            <p className="mt-1 text-[9px] text-emerald-600">{t('proxy.router.gemini3_only_warning')}</p>
                                        </div>

                                        {/* GPT-5 系列 */}
                                        <div className="bg-gradient-to-br from-amber-50 to-orange-50 dark:from-amber-900/10 dark:to-orange-900/10 p-3 rounded-xl border border-amber-100 dark:border-amber-800/30 relative overflow-hidden group hover:border-amber-400 transition-all duration-300">
                                            <div className="flex items-center gap-3 mb-3">
                                                <div className="w-8 h-8 rounded-lg bg-amber-600 flex items-center justify-center text-white shadow-lg shadow-amber-500/30">
                                                    <Zap size={16} />
                                                </div>
                                                <div>
                                                    <div className="text-xs font-bold text-gray-900 dark:text-base-content">{t('proxy.router.groups.gpt_5.name')}</div>
                                                    <div className="text-[10px] text-gray-500 line-clamp-1">{t('proxy.router.groups.gpt_5.desc')}</div>
                                                </div>
                                            </div>
                                            <select
                                                className="select select-sm select-bordered w-full font-mono text-[11px] bg-white/80 dark:bg-base-100/80 backdrop-blur-sm"
                                                value={appConfig.proxy.openai_mapping?.["gpt-5-series"] || ""}
                                                onChange={(e) => handleMappingUpdate('openai', 'gpt-5-series', e.target.value)}
                                            >
                                                <option value="">gemini-3-flash{t('proxy.router.default_suffix', ' (Default)')}</option>
                                                <optgroup label="Gemini 3 (推荐)">
                                                    <option value="gemini-3-flash">gemini-3-flash (快速)</option>
                                                    <option value="gemini-3-pro-high">gemini-3-pro-high (高质量)</option>
                                                    <option value="gemini-3-pro-low">gemini-3-pro-low (均衡)</option>
                                                </optgroup>
                                            </select>
                                            <p className="mt-1 text-[9px] text-amber-600">{t('proxy.router.gemini3_only_warning')}</p>
                                        </div>
                                    </div>
                                </div>

                                {/* 精确映射管理 */}
                                <div className="pt-4 border-t border-gray-100 dark:border-base-200">
                                    <div className="flex items-center justify-between mb-4">
                                        <h3 className="text-[10px] font-bold text-gray-400 uppercase tracking-widest flex items-center gap-2">
                                            <ArrowRight size={14} /> {t('proxy.router.expert_title')}
                                        </h3>
                                    </div>
                                    <div className="flex flex-col lg:flex-row gap-6">
                                        {/* 添加映射表单 */}
                                        <div className="flex-1 flex flex-col gap-3">
                                            <div className="flex items-center gap-2 text-[10px] font-bold text-gray-400 uppercase tracking-wider">
                                                <Target size={12} />
                                                <span>{t('proxy.router.add_mapping')}</span>
                                            </div>
                                            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                                <input
                                                    id="custom-key"
                                                    type="text"
                                                    placeholder="Original (e.g. gpt-4)"
                                                    className="input input-xs input-bordered w-full font-mono text-[11px] bg-white dark:bg-base-100 border border-gray-200 dark:border-gray-700 shadow-sm focus:border-blue-500 focus:ring-1 focus:ring-blue-500 transition-all placeholder:text-gray-400"
                                                />
                                                <input
                                                    id="custom-val"
                                                    type="text"
                                                    placeholder="Target (e.g. gemini-2.5-pro)"
                                                    className="input input-xs input-bordered w-full font-mono text-[11px] bg-white dark:bg-base-100 border border-gray-200 dark:border-gray-700 shadow-sm focus:border-blue-500 focus:ring-1 focus:ring-blue-500 transition-all placeholder:text-gray-400"
                                                />
                                            </div>
                                            <button
                                                className="btn btn-xs w-full gap-2 shadow-md hover:shadow-lg transition-all bg-blue-600 hover:bg-blue-700 text-white border-none"
                                                onClick={() => {
                                                    const k = (document.getElementById('custom-key') as HTMLInputElement).value;
                                                    const v = (document.getElementById('custom-val') as HTMLInputElement).value;
                                                    if (k && v) {
                                                        handleMappingUpdate('custom', k, v);
                                                        (document.getElementById('custom-key') as HTMLInputElement).value = '';
                                                        (document.getElementById('custom-val') as HTMLInputElement).value = '';
                                                    }
                                                }}
                                            >
                                                <Plus size={14} />
                                                {t('common.add')}
                                            </button>
                                        </div>
                                        {/* 自定义精确映射表格 */}
                                        <div className="flex-1 min-w-[300px] flex flex-col">
                                            <div className="flex items-center justify-between mb-2">
                                                <span className="text-[10px] font-bold text-gray-400 uppercase tracking-wider">
                                                    {t('proxy.router.current_list')}
                                                </span>
                                            </div>
                                            <div className="flex-1 overflow-y-auto max-h-[140px] border border-gray-100 dark:border-base-200 rounded-lg bg-gray-50/30 dark:bg-base-200/30">
                                                <table className="table table-xs w-full bg-white dark:bg-base-100">
                                                    <thead className="sticky top-0 bg-gray-50/95 dark:bg-base-200/95 backdrop-blur shadow-sm z-10 text-gray-500 dark:text-gray-400">
                                                        <tr>
                                                            <th className="text-[10px] py-2 font-medium">{t('proxy.router.original_id')}</th>
                                                            <th className="text-[10px] py-2 font-medium">{t('proxy.router.route_to')}</th>
                                                            <th className="text-[10px] w-12 text-center py-2 font-medium">{t('common.action')}</th>
                                                        </tr>
                                                    </thead>
                                                    <tbody className="font-mono text-[10px]">
                                                        {appConfig.proxy.custom_mapping && Object.entries(appConfig.proxy.custom_mapping).length > 0 ? (
                                                            Object.entries(appConfig.proxy.custom_mapping).map(([key, val]) => (
                                                                <tr key={key} className="hover:bg-gray-100 dark:hover:bg-base-300 transition-colors">
                                                                    <td className="font-bold text-blue-600 dark:text-blue-400">{key}</td>
                                                                    <td>{val}</td>
                                                                    <td className="text-center">
                                                                        <button
                                                                            className="btn btn-ghost btn-xs text-error p-0 h-auto min-h-0"
                                                                            onClick={() => handleRemoveCustomMapping(key)}
                                                                        >
                                                                            <Trash2 size={12} />
                                                                        </button>
                                                                    </td>
                                                                </tr>
                                                            ))
                                                        ) : (
                                                            <tr>
                                                                <td colSpan={3} className="text-center py-2 text-gray-400 italic">{t('proxy.router.no_custom_mapping')}</td>
                                                            </tr>
                                                        )}
                                                    </tbody>
                                                </table>
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
                                            <thead className="bg-gray-50/50 dark:bg-base-200/50 text-gray-500 dark:text-gray-400">
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
            </div >
        </div>
    );
}
