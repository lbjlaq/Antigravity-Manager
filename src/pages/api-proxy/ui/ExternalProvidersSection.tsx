// File: src/pages/api-proxy/ui/ExternalProvidersSection.tsx
// External providers section with Z.AI, MCP, Scheduling, Experimental, Cloudflared cards

import { useTranslation } from 'react-i18next';
import {
    Layers,
    Zap,
    Puzzle,
    RefreshCw,
    Sparkles,
    Trash2,
    Plus,
    ArrowRight,
    Settings,
    CheckCircle,
    Copy
} from 'lucide-react';
import { isTauri, cn } from '@/shared/lib';
import { CollapsibleCard } from './CollapsibleCard';
import HelpTooltip from '@/components/common/HelpTooltip';
import DebouncedSlider from '@/components/common/DebouncedSlider';
import CircuitBreaker from '@/components/settings/CircuitBreaker';
import SchedulingSettings from '@/components/settings/SchedulingSettings';
import { CliSyncCard } from '@/components/proxy/CliSyncCard';
import type { AppConfig, ProxyConfig, StickySessionConfig, ExperimentalConfig, CircuitBreakerConfig } from '@/entities/config';
import type { ProxyStatus, CloudflaredStatus, CloudflaredMode } from '../lib/constants';

interface ExternalProvidersSectionProps {
    appConfig: AppConfig;
    status: ProxyStatus;
    cfStatus: CloudflaredStatus;
    cfLoading: boolean;
    cfMode: CloudflaredMode;
    cfToken: string;
    cfUseHttp2: boolean;
    copied: string | null;
    zaiModelOptions: string[];
    zaiModelMapping: Record<string, string>;
    zaiModelsLoading: boolean;
    zaiNewMappingFrom: string;
    zaiNewMappingTo: string;
    // Setters
    setCfMode: (mode: CloudflaredMode) => void;
    setCfToken: (token: string) => void;
    setCfUseHttp2: (value: boolean) => void;
    setZaiNewMappingFrom: (value: string) => void;
    setZaiNewMappingTo: (value: string) => void;
    // Actions
    updateSchedulingConfig: (updates: Partial<StickySessionConfig>) => void;
    updateExperimentalConfig: (updates: Partial<ExperimentalConfig>) => void;
    updateCircuitBreakerConfig: (config: CircuitBreakerConfig) => void;
    updateZaiGeneralConfig: (updates: Partial<NonNullable<ProxyConfig['zai']>>) => void;
    updateZaiDefaultModels: (updates: Partial<NonNullable<ProxyConfig['zai']>['models']>) => void;
    upsertZaiModelMapping: (from: string, to: string) => void;
    removeZaiModelMapping: (from: string) => void;
    refreshZaiModels: () => void;
    handleCfInstall: () => void;
    handleCfToggle: (enable: boolean) => void;
    handleCfCopyUrl: () => void;
    onClearSessionBindings: () => void;
    onClearRateLimits: () => void;
}

export function ExternalProvidersSection({
    appConfig,
    status,
    cfStatus,
    cfLoading,
    cfMode,
    cfToken,
    cfUseHttp2,
    copied,
    zaiModelOptions,
    zaiModelMapping,
    zaiModelsLoading,
    zaiNewMappingFrom,
    zaiNewMappingTo,
    setCfMode,
    setCfToken,
    setCfUseHttp2,
    setZaiNewMappingFrom,
    setZaiNewMappingTo,
    updateSchedulingConfig,
    updateExperimentalConfig,
    updateCircuitBreakerConfig,
    updateZaiGeneralConfig,
    updateZaiDefaultModels,
    upsertZaiModelMapping,
    removeZaiModelMapping,
    refreshZaiModels,
    handleCfInstall,
    handleCfToggle,
    handleCfCopyUrl,
    onClearSessionBindings,
    onClearRateLimits,
}: ExternalProvidersSectionProps) {
    const { t } = useTranslation();

    return (
        <div className="space-y-4">
            <div className="px-1 flex items-center gap-2 text-gray-400">
                <Layers size={14} />
                <span className="text-[10px] font-bold uppercase tracking-widest">
                    {t('proxy.config.external_providers.title', { defaultValue: 'External Providers' })}
                </span>
            </div>

            {/* CLI Sync Card */}
            <CliSyncCard
                proxyUrl={status.running ? status.base_url : `http://127.0.0.1:${appConfig.proxy.port || 8045}`}
                apiKey={appConfig.proxy.api_key}
            />

            {/* Z.AI Card */}
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

            {/* MCP System Card */}
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
                        {['web_search', 'web_reader', 'vision'].map(feature => (
                            <label key={feature} className="flex items-center gap-2 border border-gray-100 dark:border-base-200 p-2 rounded-lg cursor-pointer hover:bg-gray-50 dark:hover:bg-base-200/50 transition-colors">
                                <input
                                    type="checkbox"
                                    className="checkbox checkbox-xs rounded border-2 border-gray-400 dark:border-gray-500 checked:border-blue-600 checked:bg-blue-600 [--chkbg:theme(colors.blue.600)] [--chkfg:white]"
                                    checked={!!appConfig.proxy.zai?.mcp?.[(feature + '_enabled') as keyof typeof appConfig.proxy.zai.mcp]}
                                    onChange={(e) => updateZaiGeneralConfig({ mcp: { ...(appConfig.proxy.zai?.mcp || {}), [feature + '_enabled']: e.target.checked } as any })}
                                />
                                <span className="text-xs">{t(`proxy.config.zai.mcp.${feature}`)}</span>
                            </label>
                        ))}
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

            {/* Scheduling Card */}
            <CollapsibleCard
                title={t('proxy.config.scheduling.title')}
                icon={<RefreshCw size={18} className="text-indigo-500" />}
                rightElement={
                    <button
                        onClick={(e) => { e.stopPropagation(); onClearSessionBindings(); }}
                        className="text-[10px] text-indigo-500 hover:text-indigo-600 transition-colors flex items-center gap-1 bg-indigo-50 dark:bg-indigo-900/30 px-2 py-1 rounded-md border border-indigo-100 dark:border-indigo-800"
                        title={t('proxy.config.scheduling.clear_bindings_tooltip')}
                    >
                        <Trash2 size={12} />
                        {t('proxy.config.scheduling.clear_bindings')}
                    </button>
                }
            >
                <div className="space-y-4">
                    <SchedulingSettings config={appConfig.proxy.scheduling} onChange={updateSchedulingConfig} />

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
                                    onClearRateLimits={onClearRateLimits}
                                />
                            )}
                        </div>
                    )}
                </div>
            </CollapsibleCard>

            {/* Experimental Card */}
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

                    {/* L1-L3 Thresholds */}
                    {[1, 2, 3].map(level => (
                        <div key={level} className="flex flex-col gap-2 p-4 bg-gray-50 dark:bg-base-200 rounded-xl border border-gray-100 dark:border-base-300">
                            <div className="flex items-center justify-between w-full">
                                <div className="flex items-center gap-2">
                                    <span className="text-sm font-bold text-gray-900 dark:text-base-content">
                                        {t(`proxy.config.experimental.context_compression_threshold_l${level}`)}
                                    </span>
                                    <HelpTooltip text={t(`proxy.config.experimental.context_compression_threshold_l${level}_tooltip`)} />
                                </div>
                            </div>
                            <DebouncedSlider
                                min={0.1}
                                max={1}
                                step={0.05}
                                className="range range-purple range-xs"
                                value={appConfig.proxy.experimental?.[`context_compression_threshold_l${level}` as keyof ExperimentalConfig] as number || [0.4, 0.55, 0.7][level - 1]}
                                onChange={(val) => updateExperimentalConfig({ [`context_compression_threshold_l${level}`]: val })}
                            />
                        </div>
                    ))}
                </div>
            </CollapsibleCard>

            {/* Cloudflared Card - Desktop only */}
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
                                <div className="flex items-center gap-2 text-xs text-gray-500 dark:text-gray-400">
                                    <CheckCircle size={14} className="text-green-500" />
                                    {t('proxy.cloudflared.installed', { defaultValue: 'Installed' })}: {cfStatus.version || 'Unknown'}
                                </div>

                                <div className="grid grid-cols-2 gap-3">
                                    <button
                                        onClick={() => setCfMode('quick')}
                                        disabled={cfStatus.running}
                                        className={cn(
                                            "p-3 rounded-lg border-2 text-left transition-all",
                                            cfMode === 'quick' ? "border-orange-500 bg-orange-50 dark:bg-orange-900/20" : "border-gray-200 dark:border-gray-700 hover:border-gray-300 dark:hover:border-gray-600",
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
                                        onClick={() => setCfMode('auth')}
                                        disabled={cfStatus.running}
                                        className={cn(
                                            "p-3 rounded-lg border-2 text-left transition-all",
                                            cfMode === 'auth' ? "border-orange-500 bg-orange-50 dark:bg-orange-900/20" : "border-gray-200 dark:border-gray-700 hover:border-gray-300 dark:hover:border-gray-600",
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

                                {cfMode === 'auth' && (
                                    <div className="space-y-2">
                                        <label className="text-sm font-medium text-gray-700 dark:text-gray-300">
                                            {t('proxy.cloudflared.token', { defaultValue: 'Tunnel Token' })}
                                        </label>
                                        <input
                                            type="password"
                                            value={cfToken}
                                            onChange={(e) => setCfToken(e.target.value)}
                                            disabled={cfStatus.running}
                                            placeholder="eyJhIjoiNj..."
                                            className="w-full px-3 py-2 rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-base-200 text-sm font-mono disabled:opacity-60"
                                        />
                                    </div>
                                )}

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
                                        onChange={(e) => setCfUseHttp2(e.target.checked)}
                                        disabled={cfStatus.running}
                                    />
                                </div>

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
                                                <button onClick={handleCfCopyUrl} className="p-2 rounded-lg bg-green-500 text-white hover:bg-green-600 transition-colors">
                                                    {copied === 'cf-url' ? <CheckCircle size={16} /> : <Copy size={16} />}
                                                </button>
                                            </div>
                                        )}
                                    </div>
                                )}

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
    );
}
