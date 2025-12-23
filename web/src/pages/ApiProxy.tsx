import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import {
    Power,
    Copy,
    RefreshCw,
    CheckCircle,
    Settings,
    Terminal,
    Code,
    Image as ImageIcon,
    BrainCircuit,
    Sparkles,
    Zap,
    Cpu
} from 'lucide-react';
import { AppConfig, ProxyConfig } from '../types/config';

interface ProxyStatus {
    running: boolean;
    port: number;
    base_url: string;
    active_accounts: number;
}


export default function ApiProxy() {
    const { t } = useTranslation();

    const models = [
        {
            id: 'gemini-3-flash',
            name: 'Gemini 3 Flash',
            desc: t('proxy.model.flash_preview'),
            icon: <Zap size={16} />
        },
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
            id: 'gemini-3-pro-high',
            name: 'Gemini 3 Pro (High)',
            desc: t('proxy.model.pro_high'),
            icon: <Cpu size={16} />
        },
        {
            id: 'gemini-3-pro-low',
            name: 'Gemini 3 Pro (Low)',
            desc: t('proxy.model.pro_low'),
            icon: <Cpu size={16} />
        },
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
            icon: <BrainCircuit size={16} />
        },
        {
            id: 'gemini-3-pro-image',
            name: 'Gemini 3 Pro (Image)',
            desc: t('proxy.model.pro_image_1_1'),
            icon: <ImageIcon size={16} />
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
    const [activeTab, setActiveTab] = useState('gemini-3-flash');
    const [selectedProtocol, setSelectedProtocol] = useState<'openai' | 'anthropic'>('openai');

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
            alert('保存配置失败: ' + error);
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

    // 专门处理模型映射的热更新
    const handleMappingUpdate = async (newMapping: Record<string, string>) => {
        if (!appConfig) return;
        try {
            // 1. 调用后端热更新指令
            await invoke('update_model_mapping', { mapping: newMapping });

            // 2. 更新本地状态 (避免整页重载)
            const newConfig = {
                ...appConfig,
                proxy: {
                    ...appConfig.proxy,
                    anthropic_mapping: newMapping
                }
            };
            setAppConfig(newConfig);

            // 可选：显示轻量提示 (Toast) 或仅仅 console
            console.log('模型映射已热更新');
        } catch (error) {
            console.error('更新模型映射失败:', error);
            alert(t('proxy.dialog.operate_failed', { error }));
        }
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

    const getCurlExample = (modelId: string) => {
        const port = status.running ? status.port : (appConfig?.proxy.port || 8045);
        const baseUrl = `http://localhost:${port}`;
        const apiKey = appConfig?.proxy.api_key || 'YOUR_API_KEY';

        // 1. Anthropic Protocol (使用 /v1/messages)
        if (selectedProtocol === 'anthropic') {
            return `curl ${baseUrl}/v1/messages \\
  -H "Content-Type: application/json" \\
  -H "x-api-key: ${apiKey}" \\
  -H "anthropic-version: 2023-06-01" \\
  -d '{
    "model": "${modelId}",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello"}]
  }'`;
        }

        // 2. OpenAI Protocol (使用 /v1/chat/completions)

        // Gemini 图像生成模型 (OpenAI Format) - 已添加 size 参数
        if (modelId.startsWith('gemini-3-pro-image')) {
            return `curl ${baseUrl}/v1/chat/completions \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer ${apiKey}" \\
  -d '{
    "model": "${modelId}",
    "size": "1024x1024",
    "messages": [
      {
        "role": "user", 
        "content": "Draw a cute cat"
      }
    ]
  }'`;
        }

        // 标准文本模型 (OpenAI Format) - 无论是 Gemini 还是 Claude
        return `curl ${baseUrl}/v1/chat/completions \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer ${apiKey}" \\
  -d '{
    "model": "${modelId}",
    "messages": [{"role": "user", "content": "Hello"}]
  }'`;
    };

    const getPythonExample = (modelId: string) => {
        const port = status.running ? status.port : (appConfig?.proxy.port || 8045);
        const baseUrl = `http://localhost:${port}/v1`;
        const apiKey = appConfig?.proxy.api_key || 'YOUR_API_KEY';

        // 1. Anthropic Protocol (使用 Anthropic SDK)
        if (selectedProtocol === 'anthropic') {
            return `from anthropic import Anthropic

client = Anthropic(
    base_url="${`http://localhost:${port}`}",
    api_key="${apiKey}"
)

# 注意: Antigravity 支持使用 Anthropic SDK 调用任意模型(包括 Gemini)
response = client.messages.create(
    model="${modelId}",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Hello"}]
)

print(response.content[0].text)`;
        }

        // 2. OpenAI Protocol (使用 OpenAI SDK)

        // Gemini 图像生成模型 - 已添加 extra_body size 参数
        if (modelId.startsWith('gemini-3-pro-image')) {
            return `from openai import OpenAI

client = OpenAI(
    base_url="${baseUrl}",
    api_key="${apiKey}"
)

response = client.chat.completions.create(
    model="${modelId}",
    extra_body={ "size": "1024x1024" },
    messages=[{
        "role": "user",
        "content": "Draw a futuristic city"
    }]
)

print(response.choices[0].message.content)`;
        }

        // 标准文本模型 (Gemini 或 Claude)
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
        <div className="h-full w-full overflow-y-auto">
            <div className="p-5 space-y-4 max-w-7xl mx-auto">
                <div className="flex items-center justify-between">
                    <h1 className="text-2xl font-bold text-gray-900 dark:text-base-content">{t('proxy.title')}</h1>
                </div>

                {/* 服务状态卡片 */}
                <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
                    <div className="flex items-center justify-between">
                        <div className="flex items-center gap-3">
                            <div className={`w-3 h-3 rounded-full ${status.running ? 'bg-green-500 animate-pulse' : 'bg-gray-400'}`} />
                            <div>
                                <h2 className="text-lg font-semibold text-gray-900 dark:text-base-content">
                                    {status.running ? t('proxy.status.running') : t('proxy.status.stopped')}
                                </h2>
                                {status.running && (
                                    <p className="text-sm text-gray-500 dark:text-gray-400">
                                        {t('proxy.status.accounts_available', { count: status.active_accounts })}
                                    </p>
                                )}
                            </div>
                        </div>
                        <button
                            onClick={handleToggle}
                            disabled={loading || !appConfig}
                            className={`px-4 py-2 rounded-lg font-medium transition-colors flex items-center gap-2 ${status.running
                                ? 'bg-red-500 hover:bg-red-600 text-white'
                                : 'bg-blue-500 hover:bg-blue-600 text-white'
                                } ${(loading || !appConfig) ? 'opacity-50 cursor-not-allowed' : ''}`}
                        >
                            <Power size={18} />
                            {loading ? t('proxy.status.processing') : (status.running ? t('proxy.action.stop') : t('proxy.action.start'))}
                        </button>
                    </div>
                </div>

                {/* 配置区 */}
                {appConfig && (
                    <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200">
                        <div className="p-4 border-b border-gray-100 dark:border-base-200">
                            <h2 className="text-lg font-semibold flex items-center gap-2 text-gray-900 dark:text-base-content">
                                <Settings size={20} />
                                {t('proxy.config.title')}
                            </h2>
                        </div>
                        <div className="p-4 space-y-4">
                            {/* 监听端口、超时和自启动 */}
                            <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                                <div>
                                    <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                                        {t('proxy.config.port')}
                                    </label>
                                    <input
                                        type="number"
                                        value={appConfig.proxy.port}
                                        onChange={(e) => updateProxyConfig({ port: parseInt(e.target.value) })}
                                        min={8000}
                                        max={65535}
                                        disabled={status.running}
                                        className="w-full px-3 py-2 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50 disabled:cursor-not-allowed"
                                    />
                                    <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                                        {t('proxy.config.port_hint')}
                                    </p>
                                </div>
                                <div>
                                    <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                                        {t('proxy.config.request_timeout')}
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
                                        className="w-full px-3 py-2 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50 disabled:cursor-not-allowed"
                                    />
                                    <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                                        {t('proxy.config.request_timeout_hint')}
                                    </p>
                                </div>
                                <div className="flex items-center">
                                    <label className="flex items-center cursor-pointer gap-3">
                                        <div className="relative">
                                            <input
                                                type="checkbox"
                                                className="sr-only"
                                                checked={appConfig.proxy.auto_start}
                                                onChange={(e) => updateProxyConfig({ auto_start: e.target.checked })}
                                            />
                                            <div className={`block w-14 h-8 rounded-full transition-colors ${appConfig.proxy.auto_start ? 'bg-blue-500' : 'bg-gray-300 dark:bg-base-300'}`}></div>
                                            <div className={`dot absolute left-1 top-1 bg-white w-6 h-6 rounded-full transition-transform ${appConfig.proxy.auto_start ? 'transform translate-x-6' : ''}`}></div>
                                        </div>
                                        <span className="text-sm font-medium text-gray-900 dark:text-base-content">
                                            {t('proxy.config.auto_start')}
                                        </span>
                                    </label>
                                </div>
                            </div>

                            {/* API 密钥 */}
                            <div>
                                <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                                    {t('proxy.config.api_key')}
                                </label>
                                <div className="flex gap-2">
                                    <input
                                        type="text"
                                        value={appConfig.proxy.api_key}
                                        readOnly
                                        className="flex-1 px-3 py-2 border border-gray-300 dark:border-base-200 rounded-lg bg-gray-50 dark:bg-base-300 text-gray-600 dark:text-gray-400 font-mono"
                                    />
                                    <button
                                        onClick={handleGenerateApiKey}
                                        className="px-3 py-2 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors"
                                        title={t('proxy.config.btn_regenerate')}
                                    >
                                        <RefreshCw size={18} />
                                    </button>
                                    <button
                                        onClick={() => copyToClipboard(appConfig.proxy.api_key, 'api_key')}
                                        className="px-3 py-2 border border-gray-300 dark:border-base-200 rounded-lg bg-white dark:bg-base-200 hover:bg-gray-50 dark:hover:bg-base-300 transition-colors"
                                        title={t('proxy.config.btn_copy')}
                                    >
                                        {copied === 'api_key' ? (
                                            <CheckCircle size={18} className="text-green-500" />
                                        ) : (
                                            <Copy size={18} />
                                        )}
                                    </button>
                                </div>
                                <p className="mt-1 text-xs text-amber-600 dark:text-amber-500">
                                    {t('proxy.config.warning_key')}
                                </p>
                            </div>
                        </div>
                    </div>
                )}


                {/* 模型映射配置 (3-Column Grid + i18n) */}
                {
                    appConfig && (
                        <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200">
                            <div className="p-5 border-b border-gray-100 dark:border-base-200">
                                <h2 className="text-lg font-bold flex items-center gap-2 text-gray-900 dark:text-base-content">
                                    <BrainCircuit size={20} className="text-purple-500" />
                                    {t('proxy.mapping.title', 'Claude Code Model Mapping')}
                                </h2>
                                <p className="text-sm text-gray-500 dark:text-gray-400 mt-2">
                                    {t('proxy.mapping.description', 'Map Claude Code models to Antigravity models. Optimize cost and speed by routing requests intelligently.')}
                                </p>
                            </div>

                            <div className="p-5">
                                {/* 3-Column Grid */}
                                <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-4">
                                    {/* Sonnet 4.5 (Default) */}
                                    <div className="bg-gradient-to-br from-blue-50 to-blue-100/50 dark:from-blue-950/30 dark:to-blue-900/20 rounded-lg p-4 border-2 border-blue-200 dark:border-blue-800/50 flex flex-col">
                                        <div className="flex items-center gap-2 mb-2">
                                            <div className="w-2 h-2 rounded-full bg-blue-500 animate-pulse"></div>
                                            <h3 className="font-bold text-gray-900 dark:text-base-content text-sm">
                                                Claude Sonnet 4.5 <span className="text-xs font-normal text-blue-600 dark:text-blue-400">({t('proxy.mapping.default', 'Default')})</span>
                                            </h3>
                                        </div>
                                        <code className="text-[10px] text-gray-500 mb-3 block">claude-sonnet-4-5-20250929</code>

                                        <div className="mt-auto">
                                            <label className="text-xs font-medium text-gray-600 dark:text-gray-400 mb-1.5 block">
                                                {t('proxy.mapping.maps_to', '映射到 Antigravity')}
                                            </label>
                                            <select
                                                className="select select-sm select-bordered w-full font-mono text-xs bg-white dark:bg-base-100"
                                                value={appConfig.proxy.anthropic_mapping?.["claude-sonnet-4-5-20250929"] || "claude-sonnet-4-5-thinking"}
                                                onChange={(e) => {
                                                    const newMapping = {
                                                        ...(appConfig.proxy.anthropic_mapping || {}),
                                                        "claude-sonnet-4-5-20250929": e.target.value
                                                    };
                                                    handleMappingUpdate(newMapping);
                                                }}
                                            >
                                                <option value="gemini-3-pro-high">gemini-3-pro-high - {t('proxy.models.pro_high')}</option>
                                                <option value="gemini-3-pro-low">gemini-3-pro-low - {t('proxy.models.pro_low')}</option>
                                                <option value="gemini-3-flash">gemini-3-flash - {t('proxy.models.flash_preview')}</option>
                                                <option value="gemini-2.5-pro">gemini-2.5-pro - {t('proxy.models.pro_legacy')}</option>
                                                <option value="gemini-2.5-flash">gemini-2.5-flash - {t('proxy.models.flash')}</option>
                                                <option value="gemini-2.5-flash-lite">gemini-2.5-flash-lite - {t('proxy.models.flash_lite')}</option>
                                                <option value="claude-sonnet-4-5">claude-sonnet-4-5 - {t('proxy.models.sonnet')}</option>
                                                <option value="claude-sonnet-4-5-thinking">claude-sonnet-4-5-thinking - {t('proxy.models.sonnet_thinking')}</option>
                                                <option value="claude-opus-4-5-thinking">claude-opus-4-5-thinking - {t('proxy.models.opus_thinking')}</option>
                                            </select>
                                        </div>
                                    </div>

                                    {/* Opus 4.5 */}
                                    <div className="bg-gradient-to-br from-purple-50 to-purple-100/50 dark:from-purple-950/30 dark:to-purple-900/20 rounded-lg p-4 border-2 border-purple-200 dark:border-purple-800/50 flex flex-col">
                                        <div className="flex items-center gap-2 mb-2">
                                            <div className="w-2 h-2 rounded-full bg-purple-500"></div>
                                            <h3 className="font-bold text-gray-900 dark:text-base-content text-sm">
                                                Claude Opus 4.5
                                            </h3>
                                        </div>
                                        <code className="text-[10px] text-gray-500 mb-3 block">claude-opus-4-5-*</code>

                                        <div className="mt-auto">
                                            <label className="text-xs font-medium text-gray-600 dark:text-gray-400 mb-1.5 block">
                                                {t('proxy.mapping.maps_to', '映射到 Antigravity')}
                                            </label>
                                            <select
                                                className="select select-sm select-bordered w-full font-mono text-xs bg-white dark:bg-base-100"
                                                value={appConfig.proxy.anthropic_mapping?.["opus"] || "claude-opus-4-5-thinking"}
                                                onChange={(e) => {
                                                    const newMapping = {
                                                        ...(appConfig.proxy.anthropic_mapping || {}),
                                                        "opus": e.target.value
                                                    };
                                                    handleMappingUpdate(newMapping);
                                                }}
                                            >
                                                <option value="gemini-3-pro-high">gemini-3-pro-high - {t('proxy.models.pro_high')}</option>
                                                <option value="gemini-3-pro-low">gemini-3-pro-low - {t('proxy.models.pro_low')}</option>
                                                <option value="gemini-3-flash">gemini-3-flash - {t('proxy.models.flash_preview')}</option>
                                                <option value="gemini-2.5-pro">gemini-2.5-pro - {t('proxy.models.pro_legacy')}</option>
                                                <option value="gemini-2.5-flash">gemini-2.5-flash - {t('proxy.models.flash')}</option>
                                                <option value="gemini-2.5-flash-lite">gemini-2.5-flash-lite - {t('proxy.models.flash_lite')}</option>
                                                <option value="claude-sonnet-4-5">claude-sonnet-4-5 - {t('proxy.models.sonnet')}</option>
                                                <option value="claude-sonnet-4-5-thinking">claude-sonnet-4-5-thinking - {t('proxy.models.sonnet_thinking')}</option>
                                                <option value="claude-opus-4-5-thinking">claude-opus-4-5-thinking - {t('proxy.models.opus_thinking')}</option>
                                            </select>
                                        </div>
                                    </div>

                                    {/* Haiku 4.5 */}
                                    <div className="bg-gradient-to-br from-green-50 to-green-100/50 dark:from-green-950/30 dark:to-green-900/20 rounded-lg p-4 border-2 border-green-200 dark:border-green-800/50 flex flex-col">
                                        <div className="flex items-center gap-2 mb-2">
                                            <div className="w-2 h-2 rounded-full bg-green-500"></div>
                                            <h3 className="font-bold text-gray-900 dark:text-base-content text-sm">
                                                Claude Haiku 4.5
                                            </h3>
                                        </div>
                                        <code className="text-[10px] text-gray-500 mb-3 block">claude-haiku-4-5-20251001</code>

                                        <div className="mt-auto">
                                            <label className="text-xs font-medium text-gray-600 dark:text-gray-400 mb-1.5 block">
                                                {t('proxy.mapping.maps_to', '映射到 Antigravity')}
                                            </label>
                                            <select
                                                className="select select-sm select-bordered w-full font-mono text-xs bg-white dark:bg-base-100"
                                                value={appConfig.proxy.anthropic_mapping?.["claude-haiku-4-5-20251001"] || "gemini-2.5-flash"}
                                                onChange={(e) => {
                                                    const newMapping = {
                                                        ...(appConfig.proxy.anthropic_mapping || {}),
                                                        "claude-haiku-4-5-20251001": e.target.value
                                                    };
                                                    handleMappingUpdate(newMapping);
                                                }}
                                            >
                                                <option value="gemini-3-pro-high">gemini-3-pro-high - {t('proxy.models.pro_high')}</option>
                                                <option value="gemini-3-pro-low">gemini-3-pro-low - {t('proxy.models.pro_low')}</option>
                                                <option value="gemini-3-flash">gemini-3-flash - {t('proxy.models.flash_preview')}</option>
                                                <option value="gemini-2.5-pro">gemini-2.5-pro - {t('proxy.models.pro_legacy')}</option>
                                                <option value="gemini-2.5-flash">gemini-2.5-flash - {t('proxy.models.flash')}</option>
                                                <option value="gemini-2.5-flash-lite">gemini-2.5-flash-lite - {t('proxy.models.flash_lite')}</option>
                                                <option value="claude-sonnet-4-5">claude-sonnet-4-5 - {t('proxy.models.sonnet')}</option>
                                                <option value="claude-sonnet-4-5-thinking">claude-sonnet-4-5-thinking - {t('proxy.models.sonnet_thinking')}</option>
                                                <option value="claude-opus-4-5-thinking">claude-opus-4-5-thinking - {t('proxy.models.opus_thinking')}</option>
                                            </select>
                                        </div>
                                    </div>
                                </div>

                                {/* Quick Actions */}
                                <div className="flex gap-2">
                                    <button
                                        className="btn btn-xs btn-outline"
                                        onClick={() => {
                                            const newMapping = {
                                                "claude-sonnet-4-5-20250929": "claude-sonnet-4-5-thinking",
                                                "opus": "claude-opus-4-5-thinking",
                                                "claude-haiku-4-5-20251001": "gemini-2.5-flash"
                                            };
                                            handleMappingUpdate(newMapping);
                                        }}
                                    >
                                        <Sparkles size={12} className="mr-1" />
                                        {t('proxy.mapping.restore_defaults', 'Restore Default Configuration')}
                                    </button>
                                </div>
                            </div>
                        </div>
                    )
                }

                {/* 多协议支持信息 */}
                {
                    appConfig && status.running && (
                        <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-200 dark:border-base-200">
                            <div className="p-5">
                                <div className="flex items-center gap-3 mb-4">
                                    <div className="flex-shrink-0">
                                        <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-blue-500 to-purple-600 flex items-center justify-center shadow-md">
                                            <Code size={20} className="text-white" />
                                        </div>
                                    </div>
                                    <div>
                                        <h3 className="text-lg font-bold text-gray-900 dark:text-base-content">
                                            🔗 {t('proxy.multi_protocol.title')}
                                        </h3>
                                        <p className="text-xs text-gray-500 dark:text-gray-400">
                                            {t('proxy.multi_protocol.subtitle')}
                                        </p>
                                    </div>
                                </div>

                                <p className="text-sm text-gray-700 dark:text-gray-300 mb-4 leading-relaxed">
                                    {t('proxy.multi_protocol.description')}
                                </p>

                                {/* 协议卡片 - 点击切换 */}
                                <div className="grid grid-cols-1 md:grid-cols-2 gap-4 mb-6">
                                    {/* OpenAI Protocol Card */}
                                    <div
                                        className={`bg-gradient-to-br from-blue-50 to-blue-100/50 dark:from-blue-950/30 dark:to-blue-900/20 rounded-lg p-4 border-2 transition-all cursor-pointer ${selectedProtocol === 'openai'
                                            ? 'border-blue-500 dark:border-blue-600 shadow-md'
                                            : 'border-blue-200 dark:border-blue-800/50 hover:border-blue-300'
                                            }`}
                                        onClick={() => setSelectedProtocol('openai')}
                                    >
                                        <div className="flex items-center justify-between mb-3">
                                            <div className="flex items-center gap-2">
                                                <div className={`w-2 h-2 rounded-full ${selectedProtocol === 'openai' ? 'bg-blue-500 animate-pulse' : 'bg-blue-400'}`}></div>
                                                <span className="text-sm font-bold text-blue-700 dark:text-blue-400">
                                                    {t('proxy.multi_protocol.openai_label')}
                                                </span>
                                            </div>
                                            <button
                                                onClick={(e) => {
                                                    e.stopPropagation();
                                                    copyToClipboard(`${status.base_url}/v1/chat/completions`, 'openai_endpoint');
                                                }}
                                                className="p-1.5 rounded-md hover:bg-blue-200 dark:hover:bg-blue-900/40 transition-colors"
                                                title={t('proxy.config.btn_copy')}
                                            >
                                                {copied === 'openai_endpoint' ?
                                                    <CheckCircle size={16} className="text-green-600 dark:text-green-400" /> :
                                                    <Copy size={16} className="text-blue-600 dark:text-blue-400" />
                                                }
                                            </button>
                                        </div>
                                        <div className="bg-white/60 dark:bg-gray-800/40 rounded px-3 py-2 mb-2 border border-blue-200/50 dark:border-blue-700/30">
                                            <code className="text-xs font-mono text-gray-800 dark:text-gray-200 break-all">
                                                POST /v1/chat/completions
                                            </code>
                                        </div>
                                        <p className="text-xs text-gray-600 dark:text-gray-400">
                                            💡 {t('proxy.multi_protocol.openai_tools')}
                                        </p>
                                    </div>

                                    {/* Anthropic Protocol Card */}
                                    <div
                                        className={`bg-gradient-to-br from-purple-50 to-purple-100/50 dark:from-purple-950/30 dark:to-purple-900/20 rounded-lg p-4 border-2 transition-all cursor-pointer ${selectedProtocol === 'anthropic'
                                            ? 'border-purple-500 dark:border-purple-600 shadow-md'
                                            : 'border-purple-200 dark:border-purple-800/50 hover:border-purple-300'
                                            }`}
                                        onClick={() => setSelectedProtocol('anthropic')}
                                    >
                                        <div className="flex items-center justify-between mb-3">
                                            <div className="flex items-center gap-2">
                                                <div className={`w-2 h-2 rounded-full ${selectedProtocol === 'anthropic' ? 'bg-purple-500 animate-pulse' : 'bg-purple-400'}`}></div>
                                                <span className="text-sm font-bold text-purple-700 dark:text-purple-400">
                                                    {t('proxy.multi_protocol.anthropic_label')}
                                                </span>
                                            </div>
                                            <button
                                                onClick={(e) => {
                                                    e.stopPropagation();
                                                    copyToClipboard(`${status.base_url}/v1/messages`, 'anthropic_endpoint');
                                                }}
                                                className="p-1.5 rounded-md hover:bg-purple-200 dark:hover:bg-purple-900/40 transition-colors"
                                                title={t('proxy.config.btn_copy')}
                                            >
                                                {copied === 'anthropic_endpoint' ?
                                                    <CheckCircle size={16} className="text-green-600 dark:text-green-400" /> :
                                                    <Copy size={16} className="text-purple-600 dark:text-purple-400" />
                                                }
                                            </button>
                                        </div>
                                        <div className="bg-white/60 dark:bg-gray-800/40 rounded px-3 py-2 mb-2 border border-purple-200/50 dark:border-purple-700/30">
                                            <code className="text-xs font-mono text-gray-800 dark:text-gray-200 break-all">
                                                POST /v1/messages
                                            </code>
                                        </div>
                                        <p className="text-xs text-gray-600 dark:text-gray-400">
                                            💡 {t('proxy.multi_protocol.anthropic_tools')}
                                        </p>
                                    </div>
                                </div>
                            </div>
                        </div>
                    )
                }

                {/* 使用说明 */}
                {
                    appConfig && (
                        <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200 overflow-hidden">
                            <div className="p-4 border-b border-gray-100 dark:border-base-200">
                                <h2 className="text-lg font-semibold text-gray-900 dark:text-base-content">{t('proxy.example.title')}</h2>
                            </div>

                            {/* Tabs */}
                            <div className="flex border-b border-gray-100 dark:border-base-200 overflow-x-auto">
                                {filteredModels.map((model) => (
                                    <button
                                        key={model.id}
                                        onClick={() => setActiveTab(model.id)}
                                        className={`flex items-center gap-2 px-4 py-3 text-sm font-medium transition-colors whitespace-nowrap ${activeTab === model.id
                                            ? 'text-blue-600 dark:text-blue-400 border-b-2 border-blue-600 dark:border-blue-400 bg-blue-50/50 dark:bg-blue-900/10'
                                            : 'text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-base-200'
                                            }`}
                                    >
                                        {model.icon}
                                        {model.name}
                                        <span className="text-xs opacity-60 ml-1">({model.desc})</span>
                                    </button>
                                ))}
                            </div>

                            <div className="p-4 space-y-4">
                                <div>
                                    <h3 className="flex items-center justify-between font-medium mb-2 text-gray-900 dark:text-base-content">
                                        <span className="flex items-center gap-2">
                                            <Terminal size={16} />
                                            {t('proxy.example.curl')}
                                        </span>
                                        <button
                                            onClick={() => copyToClipboard(getCurlExample(activeTab), 'curl')}
                                            className="text-xs flex items-center gap-1 text-blue-600 hover:text-blue-700"
                                        >
                                            {copied === 'curl' ? <CheckCircle size={14} /> : <Copy size={14} />}
                                            {copied === 'curl' ? t('proxy.config.btn_copied') : t('proxy.config.btn_copy')}
                                        </button>
                                    </h3>
                                    <pre className="p-3 bg-gray-900 rounded-lg text-sm overflow-x-auto text-gray-100 font-mono">
                                        {getCurlExample(activeTab)}
                                    </pre>
                                </div>

                                <div>
                                    <h3 className="flex items-center justify-between font-medium mb-2 text-gray-900 dark:text-base-content">
                                        <span className="flex items-center gap-2">
                                            <Code size={16} />
                                            {t('proxy.example.python')}
                                        </span>
                                        <button
                                            onClick={() => copyToClipboard(getPythonExample(activeTab), 'python')}
                                            className="text-xs flex items-center gap-1 text-blue-600 hover:text-blue-700"
                                        >
                                            {copied === 'python' ? <CheckCircle size={14} /> : <Copy size={14} />}
                                            {copied === 'python' ? t('proxy.config.btn_copied') : t('proxy.config.btn_copy')}
                                        </button>
                                    </h3>
                                    <pre className="p-3 bg-gray-900 rounded-lg text-sm overflow-x-auto text-gray-100 font-mono">
                                        {getPythonExample(activeTab)}
                                    </pre>
                                </div>
                            </div>
                        </div>
                    )
                }
            </div >
        </div >
    );
}
