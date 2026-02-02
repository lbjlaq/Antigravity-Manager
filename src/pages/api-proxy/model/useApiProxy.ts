// File: src/pages/api-proxy/model/useApiProxy.ts
// Business logic hook for API Proxy page

import { useState, useEffect, useMemo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@/shared/api';
import { copyToClipboard } from '@/shared/lib';
import type { AppConfig, ProxyConfig, StickySessionConfig, ExperimentalConfig, CircuitBreakerConfig } from '@/entities/config';
import { showToast } from '@/shared/ui';
import { useProxyModels } from '@/shared/hooks';
import type { ProxyStatus, CloudflaredStatus, ProtocolType, CloudflaredMode } from '../lib/constants';
import { MODEL_PRESETS } from '../lib/constants';

export function useApiProxy() {
    const { t } = useTranslation();
    const { models } = useProxyModels();

    // Core state
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

    // Protocol & Model selection
    const [selectedProtocol, setSelectedProtocol] = useState<ProtocolType>('openai');
    const [selectedModelId, setSelectedModelId] = useState('gemini-3-flash');

    // Z.AI state
    const [zaiAvailableModels, setZaiAvailableModels] = useState<string[]>([]);
    const [zaiModelsLoading, setZaiModelsLoading] = useState(false);
    const [zaiNewMappingFrom, setZaiNewMappingFrom] = useState('');
    const [zaiNewMappingTo, setZaiNewMappingTo] = useState('');

    // Custom mapping state
    const [customMappingValue, setCustomMappingValue] = useState('');
    const [editingKey, setEditingKey] = useState<string | null>(null);
    const [editingValue, setEditingValue] = useState<string>('');

    // API Key editing
    const [isEditingApiKey, setIsEditingApiKey] = useState(false);
    const [tempApiKey, setTempApiKey] = useState('');

    // Admin Password editing
    const [isEditingAdminPassword, setIsEditingAdminPassword] = useState(false);
    const [tempAdminPassword, setTempAdminPassword] = useState('');

    // Modal states
    const [isResetConfirmOpen, setIsResetConfirmOpen] = useState(false);
    const [isRegenerateKeyConfirmOpen, setIsRegenerateKeyConfirmOpen] = useState(false);
    const [isClearBindingsConfirmOpen, setIsClearBindingsConfirmOpen] = useState(false);
    const [isClearRateLimitsConfirmOpen, setIsClearRateLimitsConfirmOpen] = useState(false);

    // Cloudflared state
    const [cfStatus, setCfStatus] = useState<CloudflaredStatus>({
        installed: false,
        running: false,
    });
    const [cfLoading, setCfLoading] = useState(false);
    const [cfMode, setCfMode] = useState<CloudflaredMode>('quick');
    const [cfToken, setCfToken] = useState('');
    const [cfUseHttp2, setCfUseHttp2] = useState(true);

    // Computed values
    const zaiModelOptions = useMemo(() => {
        const unique = new Set(zaiAvailableModels);
        return Array.from(unique).sort();
    }, [zaiAvailableModels]);

    const zaiModelMapping = useMemo(() => {
        return appConfig?.proxy.zai?.model_mapping || {};
    }, [appConfig?.proxy.zai?.model_mapping]);

    const customMappingOptions = useMemo(() => {
        return models.map(model => ({
            value: model.id,
            label: `${model.id} (${model.name})`,
            group: model.group || 'Other'
        }));
    }, [models]);

    const filteredModels = useMemo(() => {
        return models.filter(model => {
            if (selectedProtocol === 'openai') return true;
            if (selectedProtocol === 'anthropic') return !model.id.includes('image');
            return true;
        });
    }, [models, selectedProtocol]);

    // Load functions
    const loadConfig = useCallback(async () => {
        setConfigLoading(true);
        setConfigError(null);
        try {
            const config = await invoke<AppConfig>('load_config');
            setAppConfig(config);
        } catch (error) {
            console.error('Failed to load config:', error);
            setConfigError(String(error));
        } finally {
            setConfigLoading(false);
        }
    }, []);

    const loadStatus = useCallback(async () => {
        try {
            const s = await invoke<ProxyStatus>('get_proxy_status');
            if (s.base_url === 'starting' || s.base_url === 'busy') {
                setStatus(prev => ({ ...s, running: prev.running }));
            } else {
                setStatus(s);
            }
        } catch (error) {
            console.error('Failed to get status:', error);
        }
    }, []);

    const loadCfStatus = useCallback(async () => {
        try {
            const status = await invoke<CloudflaredStatus>('cloudflared_get_status');
            setCfStatus(status);
        } catch {
            // Ignore - manager may not be initialized
        }
    }, []);

    // Initialize
    useEffect(() => {
        loadConfig();
        loadStatus();
        loadCfStatus();
        const interval = setInterval(loadStatus, 3000);
        const cfInterval = setInterval(loadCfStatus, 5000);
        return () => {
            clearInterval(interval);
            clearInterval(cfInterval);
        };
    }, [loadConfig, loadStatus, loadCfStatus]);

    // Save config
    const saveConfig = useCallback(async (newConfig: AppConfig) => {
        setAppConfig(newConfig);
        try {
            await invoke('save_config', { config: newConfig });
        } catch (error) {
            console.error('Failed to save config:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    }, [t]);

    // Update helpers
    const updateProxyConfig = useCallback((updates: Partial<ProxyConfig>) => {
        if (!appConfig) return;
        const newConfig = {
            ...appConfig,
            proxy: { ...appConfig.proxy, ...updates }
        };
        saveConfig(newConfig);
    }, [appConfig, saveConfig]);

    const updateSchedulingConfig = useCallback((updates: Partial<StickySessionConfig>) => {
        if (!appConfig) return;
        const currentScheduling = appConfig.proxy.scheduling || {
            mode: 'Balance',
            max_wait_seconds: 60,
            selected_accounts: [],
            selected_models: {},
            strict_selected: false
        };
        const newScheduling: StickySessionConfig = {
            ...currentScheduling,
            ...updates,
            selected_accounts: updates.selected_accounts ?? currentScheduling.selected_accounts ?? [],
            strict_selected: updates.strict_selected ?? currentScheduling.strict_selected ?? false
        };
        const newAppConfig = {
            ...appConfig,
            proxy: { ...appConfig.proxy, scheduling: newScheduling }
        };
        saveConfig(newAppConfig);
    }, [appConfig, saveConfig]);

    const updateExperimentalConfig = useCallback((updates: Partial<ExperimentalConfig>) => {
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
    }, [appConfig, saveConfig]);

    const updateCircuitBreakerConfig = useCallback((newBreakerConfig: CircuitBreakerConfig) => {
        if (!appConfig) return;
        saveConfig({ ...appConfig, circuit_breaker: newBreakerConfig });
    }, [appConfig, saveConfig]);

    // Mapping handlers
    const handleMappingUpdate = useCallback(async (_type: 'custom', key: string, value: string) => {
        if (!appConfig) return;
        const newConfig = { ...appConfig.proxy };
        newConfig.custom_mapping = { ...(newConfig.custom_mapping || {}), [key]: value };
        try {
            await invoke('update_model_mapping', { config: newConfig });
            setAppConfig({ ...appConfig, proxy: newConfig });
            showToast(t('common.saved'), 'success');
        } catch (error) {
            console.error('Failed to update mapping:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    }, [appConfig, t]);

    const handleRemoveCustomMapping = useCallback(async (key: string) => {
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
    }, [appConfig]);

    const handleApplyPresets = useCallback(async () => {
        if (!appConfig) return;
        const newConfig = {
            ...appConfig.proxy,
            custom_mapping: { ...appConfig.proxy.custom_mapping, ...MODEL_PRESETS }
        };
        try {
            await invoke('update_model_mapping', { config: newConfig });
            setAppConfig({ ...appConfig, proxy: newConfig });
            showToast(t('proxy.router.presets_applied'), 'success');
        } catch (error) {
            console.error('Failed to apply presets:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    }, [appConfig, t]);

    const executeResetMapping = useCallback(async () => {
        if (!appConfig) return;
        setIsResetConfirmOpen(false);
        const newConfig = { ...appConfig.proxy, custom_mapping: {} };
        try {
            await invoke('update_model_mapping', { config: newConfig });
            setAppConfig({ ...appConfig, proxy: newConfig });
            showToast(t('common.success'), 'success');
        } catch (error) {
            console.error('Failed to reset mapping:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    }, [appConfig, t]);

    // Proxy toggle
    const handleToggle = useCallback(async () => {
        if (!appConfig) return;
        setLoading(true);
        try {
            if (status.running) {
                await invoke('stop_proxy_service');
            } else {
                await invoke('start_proxy_service', { config: appConfig.proxy });
            }
            await loadStatus();
        } catch (error: any) {
            showToast(t('proxy.dialog.operate_failed', { error: error.toString() }), 'error');
        } finally {
            setLoading(false);
        }
    }, [appConfig, status.running, loadStatus, t]);

    // API Key handlers
    const validateApiKey = (key: string): boolean => key.startsWith('sk-') && key.length >= 10;

    const handleEditApiKey = useCallback(() => {
        setTempApiKey(appConfig?.proxy.api_key || '');
        setIsEditingApiKey(true);
    }, [appConfig]);

    const handleSaveApiKey = useCallback(() => {
        if (!validateApiKey(tempApiKey)) {
            showToast(t('proxy.config.api_key_invalid'), 'error');
            return;
        }
        updateProxyConfig({ api_key: tempApiKey });
        setIsEditingApiKey(false);
        showToast(t('proxy.config.api_key_updated'), 'success');
    }, [tempApiKey, updateProxyConfig, t]);

    const handleCancelEditApiKey = useCallback(() => {
        setTempApiKey('');
        setIsEditingApiKey(false);
    }, []);

    const executeGenerateApiKey = useCallback(async () => {
        setIsRegenerateKeyConfirmOpen(false);
        try {
            const newKey = await invoke<string>('generate_api_key');
            updateProxyConfig({ api_key: newKey });
            showToast(t('common.success'), 'success');
        } catch (error: any) {
            console.error('Failed to generate API Key:', error);
            showToast(t('proxy.dialog.operate_failed', { error: error.toString() }), 'error');
        }
    }, [updateProxyConfig, t]);

    // Admin Password handlers
    const handleEditAdminPassword = useCallback(() => {
        setTempAdminPassword(appConfig?.proxy.admin_password || '');
        setIsEditingAdminPassword(true);
    }, [appConfig]);

    const handleSaveAdminPassword = useCallback(() => {
        if (tempAdminPassword && tempAdminPassword.length < 4) {
            showToast(t('proxy.config.admin_password_short', { defaultValue: 'Password is too short (min 4 chars)' }), 'error');
            return;
        }
        updateProxyConfig({ admin_password: tempAdminPassword || undefined });
        setIsEditingAdminPassword(false);
        showToast(t('proxy.config.admin_password_updated', { defaultValue: 'Web UI password updated' }), 'success');
    }, [tempAdminPassword, updateProxyConfig, t]);

    const handleCancelEditAdminPassword = useCallback(() => {
        setTempAdminPassword('');
        setIsEditingAdminPassword(false);
    }, []);

    // Session/Rate limit handlers
    const executeClearSessionBindings = useCallback(async () => {
        setIsClearBindingsConfirmOpen(false);
        try {
            await invoke('clear_proxy_session_bindings');
            showToast(t('common.success'), 'success');
        } catch (error) {
            console.error('Failed to clear session bindings:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    }, [t]);

    const executeClearRateLimits = useCallback(async () => {
        setIsClearRateLimitsConfirmOpen(false);
        try {
            await invoke('clear_all_proxy_rate_limits');
            showToast(t('common.success'), 'success');
        } catch (error) {
            console.error('Failed to clear rate limits:', error);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    }, [t]);

    // Z.AI handlers
    const refreshZaiModels = useCallback(async () => {
        if (!appConfig?.proxy.zai) return;
        setZaiModelsLoading(true);
        try {
            const models = await invoke<string[]>('fetch_zai_models', {
                zai: appConfig.proxy.zai,
                upstreamProxy: appConfig.proxy.upstream_proxy,
                requestTimeout: appConfig.proxy.request_timeout,
            });
            setZaiAvailableModels(models);
        } catch (error: any) {
            console.error('Failed to fetch z.ai models:', error);
        } finally {
            setZaiModelsLoading(false);
        }
    }, [appConfig]);

    const updateZaiDefaultModels = useCallback((updates: Partial<NonNullable<ProxyConfig['zai']>['models']>) => {
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
    }, [appConfig, saveConfig]);

    const upsertZaiModelMapping = useCallback((from: string, to: string) => {
        if (!appConfig?.proxy.zai) return;
        const currentMapping = appConfig.proxy.zai.model_mapping || {};
        const newMapping = { ...currentMapping, [from]: to };
        const newConfig = {
            ...appConfig,
            proxy: {
                ...appConfig.proxy,
                zai: { ...appConfig.proxy.zai, model_mapping: newMapping }
            }
        };
        saveConfig(newConfig);
    }, [appConfig, saveConfig]);

    const removeZaiModelMapping = useCallback((from: string) => {
        if (!appConfig?.proxy.zai) return;
        const currentMapping = appConfig.proxy.zai.model_mapping || {};
        const newMapping = { ...currentMapping };
        delete newMapping[from];
        const newConfig = {
            ...appConfig,
            proxy: {
                ...appConfig.proxy,
                zai: { ...appConfig.proxy.zai, model_mapping: newMapping }
            }
        };
        saveConfig(newConfig);
    }, [appConfig, saveConfig]);

    const updateZaiGeneralConfig = useCallback((updates: Partial<NonNullable<ProxyConfig['zai']>>) => {
        if (!appConfig?.proxy.zai) return;
        const newConfig = {
            ...appConfig,
            proxy: {
                ...appConfig.proxy,
                zai: { ...appConfig.proxy.zai, ...updates }
            }
        };
        saveConfig(newConfig);
    }, [appConfig, saveConfig]);

    // Cloudflared handlers
    const handleCfInstall = useCallback(async () => {
        setCfLoading(true);
        try {
            const status = await invoke<CloudflaredStatus>('cloudflared_install');
            setCfStatus(status);
            showToast(t('proxy.cloudflared.install_success', { defaultValue: 'Cloudflared installed successfully' }), 'success');
        } catch (error) {
            console.error('[Cloudflared] Install error:', error);
            showToast(String(error), 'error');
        } finally {
            setCfLoading(false);
        }
    }, [t]);

    const handleCfToggle = useCallback(async (enable: boolean) => {
        if (enable && !status.running) {
            showToast(t('proxy.cloudflared.require_proxy_running', { defaultValue: 'Please start the local proxy service first' }), 'warning');
            return;
        }
        setCfLoading(true);
        try {
            if (enable) {
                if (!cfStatus.installed) {
                    const installStatus = await invoke<CloudflaredStatus>('cloudflared_install');
                    setCfStatus(installStatus);
                    if (!installStatus.installed) throw new Error('Cloudflared install failed');
                    showToast(t('proxy.cloudflared.install_success', { defaultValue: 'Cloudflared installed successfully' }), 'success');
                }
                const config = {
                    enabled: true,
                    mode: cfMode,
                    port: appConfig?.proxy.port || 8045,
                    token: cfMode === 'auth' ? cfToken : null,
                    use_http2: cfUseHttp2,
                };
                const newStatus = await invoke<CloudflaredStatus>('cloudflared_start', { config });
                setCfStatus(newStatus);
                showToast(t('proxy.cloudflared.started', { defaultValue: 'Tunnel started' }), 'success');
            } else {
                const newStatus = await invoke<CloudflaredStatus>('cloudflared_stop');
                setCfStatus(newStatus);
                showToast(t('proxy.cloudflared.stopped', { defaultValue: 'Tunnel stopped' }), 'success');
            }
        } catch (error) {
            showToast(String(error), 'error');
        } finally {
            setCfLoading(false);
        }
    }, [status.running, cfStatus.installed, cfMode, cfToken, cfUseHttp2, appConfig, t]);

    const handleCfCopyUrl = useCallback(async () => {
        if (cfStatus.url) {
            const success = await copyToClipboard(cfStatus.url);
            if (success) {
                setCopied('cf-url');
                setTimeout(() => setCopied(null), 2000);
            }
        }
    }, [cfStatus.url]);

    // Copy handler
    const copyToClipboardHandler = useCallback((text: string, label: string) => {
        copyToClipboard(text).then((success) => {
            if (success) {
                setCopied(label);
                setTimeout(() => setCopied(null), 2000);
            }
        });
    }, []);

    // Python example generator
    const getPythonExample = useCallback((modelId: string) => {
        const port = status.running ? status.port : (appConfig?.proxy.port || 8045);
        const baseUrl = `http://127.0.0.1:${port}/v1`;
        const apiKey = appConfig?.proxy.api_key || 'YOUR_API_KEY';

        if (selectedProtocol === 'anthropic') {
            return `from anthropic import Anthropic
 
client = Anthropic(
    base_url="http://127.0.0.1:${port}",
    api_key="${apiKey}"
)

response = client.messages.create(
    model="${modelId}",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Hello"}]
)

print(response.content[0].text)`;
        }

        if (selectedProtocol === 'gemini') {
            return `# pip install google-generativeai
import google.generativeai as genai

genai.configure(
    api_key="${apiKey}",
    transport='rest',
    client_options={'api_endpoint': 'http://127.0.0.1:${port}'}
)

model = genai.GenerativeModel('${modelId}')
response = model.generate_content("Hello")
print(response.text)`;
        }

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
    }, [status, appConfig, selectedProtocol]);

    return {
        // State
        status,
        appConfig,
        configLoading,
        configError,
        loading,
        copied,
        selectedProtocol,
        selectedModelId,
        zaiAvailableModels,
        zaiModelsLoading,
        zaiNewMappingFrom,
        zaiNewMappingTo,
        customMappingValue,
        editingKey,
        editingValue,
        isEditingApiKey,
        tempApiKey,
        isEditingAdminPassword,
        tempAdminPassword,
        isResetConfirmOpen,
        isRegenerateKeyConfirmOpen,
        isClearBindingsConfirmOpen,
        isClearRateLimitsConfirmOpen,
        cfStatus,
        cfLoading,
        cfMode,
        cfToken,
        cfUseHttp2,
        models,
        
        // Computed
        zaiModelOptions,
        zaiModelMapping,
        customMappingOptions,
        filteredModels,

        // Setters
        setSelectedProtocol,
        setSelectedModelId,
        setZaiNewMappingFrom,
        setZaiNewMappingTo,
        setCustomMappingValue,
        setEditingKey,
        setEditingValue,
        setTempApiKey,
        setTempAdminPassword,
        setIsResetConfirmOpen,
        setIsRegenerateKeyConfirmOpen,
        setIsClearBindingsConfirmOpen,
        setIsClearRateLimitsConfirmOpen,
        setCfMode,
        setCfToken,
        setCfUseHttp2,

        // Actions
        loadConfig,
        loadStatus,
        updateProxyConfig,
        updateSchedulingConfig,
        updateExperimentalConfig,
        updateCircuitBreakerConfig,
        handleMappingUpdate,
        handleRemoveCustomMapping,
        handleApplyPresets,
        executeResetMapping,
        handleToggle,
        handleEditApiKey,
        handleSaveApiKey,
        handleCancelEditApiKey,
        executeGenerateApiKey,
        handleEditAdminPassword,
        handleSaveAdminPassword,
        handleCancelEditAdminPassword,
        executeClearSessionBindings,
        executeClearRateLimits,
        refreshZaiModels,
        updateZaiDefaultModels,
        upsertZaiModelMapping,
        removeZaiModelMapping,
        updateZaiGeneralConfig,
        handleCfInstall,
        handleCfToggle,
        handleCfCopyUrl,
        copyToClipboardHandler,
        getPythonExample,
    };
}
