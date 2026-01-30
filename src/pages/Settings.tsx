// File: src/pages/Settings.tsx
import { useState, useEffect, memo } from 'react';
import { 
  Settings as SettingsIcon, 
  User, 
  Globe, 
  Shield, 
  Zap, 
  Save, 
  Monitor,
  RefreshCcw,
  Info,
  Terminal,
  MessageCircle,
  Github,
  RefreshCw,
  ChevronRight
} from "lucide-react";
import { motion, AnimatePresence } from 'framer-motion';
import { request as invoke } from '../utils/request';
import { useConfigStore } from '../stores/useConfigStore';
import { AppConfig } from '../types/config';
import { showToast } from '../components/common/ToastContainer';
import QuotaProtection from '../components/settings/QuotaProtection';
import SmartWarmup from '../components/settings/SmartWarmup';
import PinnedQuotaModels from '../components/settings/PinnedQuotaModels';

import { useTranslation } from 'react-i18next';
import { isTauri } from '../utils/env';
import { cn } from '../lib/utils';
import { Button } from '../components/ui/button';
import { Switch } from '../components/ui/switch';
import { Label } from '../components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select';
import { Input } from '../components/ui/input';

// --- Premium UI Components ---

const SidebarItem = memo(({ 
    active, 
    icon: Icon, 
    label, 
    onClick 
}: { 
    active: boolean; 
    icon: any; 
    label: string; 
    onClick: () => void 
}) => (
    <button
        onClick={onClick}
        className={cn(
            "w-full flex items-center gap-3 px-4 py-3 text-sm font-medium rounded-xl transition-all duration-200 group relative overflow-hidden",
            active 
                ? "text-white shadow-lg shadow-indigo-500/20" 
                : "text-zinc-500 hover:text-zinc-300 hover:bg-white/5"
        )}
    >
        {/* Active Gradient Background */}
        {active && (
            <motion.div 
                layoutId="sidebarActiveItem"
                className="absolute inset-0 bg-gradient-to-r from-indigo-500 to-purple-500" 
                transition={{ type: "spring", stiffness: 300, damping: 30 }}
            />
        )}

        {/* Shine Effect for Active State */}
        {active && (
            <motion.div
                initial={{ x: '-100%' }}
                animate={{ x: '200%' }}
                transition={{ repeat: Infinity, duration: 2, ease: "linear" }}
                className="absolute inset-0 bg-gradient-to-r from-transparent via-white/10 to-transparent skew-x-12"
            />
        )}
        
        {/* Icon Container */}
        <div className={cn(
            "relative z-10 p-2 rounded-lg transition-colors duration-200",
            active ? "bg-white/20 text-white" : "bg-zinc-800/50 text-zinc-500 group-hover:text-zinc-300 group-hover:bg-zinc-800"
        )}>
            <Icon className="h-4 w-4" />
        </div>

        {/* Label */}
        <span className="relative z-10">{label}</span>

        {/* Arrow (Active only) */}
        {active && (
            <motion.div 
                initial={{ opacity: 0, x: -10 }} 
                animate={{ opacity: 1, x: 0 }}
                transition={{ duration: 0.2 }}
                className="absolute right-3 text-white/50 z-10"
            >
                <ChevronRight className="h-4 w-4" />
            </motion.div>
        )}
    </button>
));

const SettingsCard = ({ title, icon: Icon, children, className, description }: { title: string, icon: any, children: React.ReactNode, className?: string, description?: string }) => (
    <motion.div 
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.3 }}
        className={cn(
            "rounded-2xl border border-zinc-200 dark:border-white/5 bg-white/50 dark:bg-zinc-900/30 backdrop-blur-md p-6 shadow-sm hover:shadow-md transition-all duration-300 relative overflow-hidden group",
            className
        )}
    >
        {/* Background Decoration */}
        <div className="absolute top-0 right-0 w-32 h-32 bg-indigo-500/5 rounded-bl-full -mr-10 -mt-10 transition-transform group-hover:scale-110" />

        <div className="flex items-start gap-4 mb-6 relative z-10">
            <div className="p-3 rounded-xl bg-indigo-50 dark:bg-zinc-800/50 border border-indigo-100 dark:border-white/5 text-indigo-600 dark:text-indigo-400 shadow-sm">
                <Icon className="h-5 w-5" />
            </div>
            <div className="space-y-1">
                <h3 className="text-lg font-bold text-zinc-900 dark:text-white tracking-tight">{title}</h3>
                {description && <p className="text-sm text-zinc-500">{description}</p>}
            </div>
        </div>
        <div className="relative z-10">
            {children}
        </div>
    </motion.div>
);

// --- Sections Configuration ---
const SECTIONS = [
  { id: "general", label: "settings.tabs.general", icon: SettingsIcon, desc: "Appearance & Language" },
  { id: "account", label: "settings.tabs.account", icon: User, desc: "Sync & Refresh Config" },
  { id: "proxy", label: "settings.tabs.proxy", icon: Globe, desc: "Network & Traffic" },
  { id: "security", label: "settings.tabs.security", icon: Shield, desc: "Protection Rules" },
  { id: "performance", label: "settings.tabs.performance", icon: Zap, desc: "Optimization & Warmup" },
  { id: "advanced", label: "settings.tabs.advanced", icon: Terminal, desc: "Developer Options" },
  { id: "about", label: "settings.tabs.about", icon: Info, desc: "App Info & Support" },
];

export const Settings = () => {
    const { t, i18n } = useTranslation();
    const { config, loadConfig, saveConfig, updateLanguage, updateTheme } = useConfigStore();
    const [activeTab, setActiveTab] = useState("general");
    
    // Config State
    const [formData, setFormData] = useState<AppConfig>({
        language: 'zh',
        theme: 'system',
        auto_refresh: false,
        refresh_interval: 15,
        auto_sync: false,
        sync_interval: 5,
        proxy: {
            enabled: false,
            port: 8080,
            api_key: '',
            auto_start: false,
            request_timeout: 120,
            enable_logging: false,
            upstream_proxy: { enabled: false, url: '' },
            debug_logging: { enabled: false, output_dir: undefined }
        },
        scheduled_warmup: { enabled: false, monitored_models: [] },
        quota_protection: { enabled: false, threshold_percentage: 10, monitored_models: [] },
        pinned_quota_models: { models: [] },
        circuit_breaker: { enabled: false, backoff_steps: [] }
    });

    // Auxiliary State
    const [dataDirPath, setDataDirPath] = useState<string>('~/.antigravity_tools/');

    // Initial Load
    useEffect(() => {
        loadConfig();
        invoke<string>('get_data_dir_path').then(setDataDirPath).catch(console.error);
        invoke<{ auto_check: boolean; check_interval_hours: number }>('get_update_settings')
            .then(s => setFormData(p => ({ ...p, auto_check_update: s.auto_check, update_check_interval: s.check_interval_hours })))
            .catch(console.error);
        invoke<boolean>('is_auto_launch_enabled')
            .then(e => setFormData(p => ({ ...p, auto_launch: e })))
            .catch(console.error);
    }, [loadConfig]);

    // Sync Config to Form
    useEffect(() => {
        if (config) setFormData(config);
    }, [config]);

    // Handlers
    const handleSave = async () => {
        try {
            const proxyEnabled = formData.proxy?.upstream_proxy?.enabled;
            const proxyUrl = formData.proxy?.upstream_proxy?.url?.trim();
            if (proxyEnabled && !proxyUrl) {
                showToast(t('proxy.config.upstream_proxy.validation_error'), 'error');
                return;
            }
            await saveConfig({ ...formData, auto_refresh: true }); // Enforce auto_refresh logic
            showToast(t('common.saved'), 'success');
            if (proxyEnabled && proxyUrl) showToast(t('proxy.config.upstream_proxy.restart_hint'), 'info');
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    return (
        <div className="h-full flex flex-col p-5 gap-4 max-w-7xl mx-auto w-full">
            {/* Main Glass Card */}
            <div className="flex-1 h-full min-h-0 relative flex flex-col">
                <div className="h-full bg-white dark:bg-zinc-900/40 backdrop-blur-xl rounded-2xl border border-zinc-200 dark:border-white/5 flex overflow-hidden shadow-2xl">
                    
                    {/* 1. SIDEBAR navigation */}
                    <aside className="w-64 flex-shrink-0 border-r border-zinc-200 dark:border-white/5 bg-zinc-50/50 dark:bg-black/20 flex flex-col">
                        <div className="p-6">
                            <h2 className="px-4 text-xs font-bold text-zinc-400 uppercase tracking-widest mb-4">
                                {t('settings.configuration', 'SYSTEM CONFIG')}
                            </h2>
                            <div className="space-y-1">
                                {SECTIONS.map((section) => (
                                   <SidebarItem 
                                        key={section.id} 
                                        active={activeTab === section.id} 
                                        icon={section.icon} 
                                        label={t(section.label)} 
                                        onClick={() => setActiveTab(section.id)} 
                                    />
                                ))}
                            </div>
                        </div>
                    </aside>

                     {/* 2. MAIN CONTENT AREA */}
                    <main className="flex-1 h-full overflow-hidden relative flex flex-col bg-white/50 dark:bg-transparent">
                         {/* Header inside content area */}
                        <header className="flex-shrink-0 px-8 py-8 border-b border-zinc-200 dark:border-white/5 flex items-center justify-between bg-white/50 dark:bg-transparent backdrop-blur-sm sticky top-0 z-10">
                            <div>
                                 <motion.h2 
                                    key={activeTab}
                                    initial={{ opacity: 0, x: -10 }}
                                    animate={{ opacity: 1, x: 0 }}
                                    className="text-2xl font-bold text-zinc-900 dark:text-white tracking-tight"
                                >
                                    {t(SECTIONS.find(s => s.id === activeTab)?.label || 'Settings')}
                                </motion.h2>
                                <p className="text-zinc-500 text-sm mt-1">
                                    {SECTIONS.find(s => s.id === activeTab)?.desc}
                                </p>
                            </div>
                            <Button 
                                onClick={handleSave} 
                                className="group relative px-5 py-2.5 rounded-xl bg-indigo-500 hover:bg-indigo-600 active:scale-95 transition-all text-white font-medium shadow-[0_0_20px_rgba(99,102,241,0.3)] hover:shadow-[0_0_30px_rgba(99,102,241,0.5)] overflow-hidden border-none"
                            >
                                <div className="absolute inset-0 bg-white/20 translate-y-full group-hover:translate-y-0 transition-transform duration-300" />
                                <div className="relative flex items-center gap-2">
                                     <Save className="h-4 w-4" />
                                     <span>{t('settings.save')}</span>
                                </div>
                            </Button>
                        </header>

                {/* Scrollable Content */}
                <div className="flex-1 overflow-y-auto p-8 relative z-10 custom-scrollbar">
                    <div className="max-w-4xl mx-auto pb-20">
                        <AnimatePresence mode="wait">
                            <motion.div
                                key={activeTab}
                                initial={{ opacity: 0, x: 20 }}
                                animate={{ opacity: 1, x: 0 }}
                                exit={{ opacity: 0, x: -20 }}
                                transition={{ duration: 0.2 }}
                                className="space-y-6"
                            >
                                {/* --- GENERAL TAB --- */}
                                {activeTab === 'general' && (
                                    <>
                                        <SettingsCard title={t('settings.general.title')} icon={Monitor} description={t('settings.general.desc', 'Customize look and feel')} className="z-20">
                                            <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                                                <div className="space-y-3">
                                                    <Label className="text-zinc-400">{t('settings.general.language')}</Label>
                                                    <Select 
                                                        value={formData.language} 
                                                        onValueChange={(val) => {
                                                            setFormData({ ...formData, language: val });
                                                            i18n.changeLanguage(val);
                                                            updateLanguage(val);
                                                        }}
                                                    >
                                                        <SelectTrigger className="bg-zinc-50 dark:bg-zinc-900/50 border-zinc-200 dark:border-white/10 text-zinc-900 dark:text-white">
                                                            <SelectValue />
                                                        </SelectTrigger>
                                                        <SelectContent className="bg-white dark:bg-zinc-900 border-zinc-200 dark:border-white/10 text-zinc-900 dark:text-white">
                                                            <SelectItem value="zh">简体中文</SelectItem>
                                                            <SelectItem value="en">English</SelectItem>
                                                            <SelectItem value="ru">Русский</SelectItem>
                                                            {/* Add other languages as needed */}
                                                        </SelectContent>
                                                    </Select>
                                                </div>
                                                <div className="space-y-3">
                                                    <Label className="text-zinc-400">{t('settings.general.theme')}</Label>
                                                    <Select 
                                                        value={formData.theme} 
                                                        onValueChange={(val) => {
                                                            setFormData({ ...formData, theme: val });
                                                            updateTheme(val);
                                                        }}
                                                    >
                                                        <SelectTrigger className="bg-zinc-50 dark:bg-zinc-900/50 border-zinc-200 dark:border-white/10 text-zinc-900 dark:text-white">
                                                            <SelectValue />
                                                        </SelectTrigger>
                                                        <SelectContent className="bg-white dark:bg-zinc-900 border-zinc-200 dark:border-white/10 text-zinc-900 dark:text-white">
                                                            <SelectItem value="light">{t('settings.general.theme_light')}</SelectItem>
                                                            <SelectItem value="dark">{t('settings.general.theme_dark')}</SelectItem>
                                                            <SelectItem value="system">{t('settings.general.theme_system')}</SelectItem>
                                                        </SelectContent>
                                                    </Select>
                                                </div>
                                            </div>
                                        </SettingsCard>

                                        <SettingsCard title={t('settings.system')} icon={RefreshCcw} description="System-level behaviors" className="z-10">
                                            <div className="space-y-6">
                                                <div className="flex items-center justify-between p-2 rounded-lg hover:bg-white/5 transition-colors">
                                                    <div className="space-y-1">
                                                        <Label className="text-base text-zinc-200">{t('settings.general.auto_launch')}</Label>
                                                        <p className="text-sm text-zinc-500">{t('settings.general.auto_launch_desc')}</p>
                                                    </div>
                                                    <Switch 
                                                        disabled={!isTauri()}
                                                        checked={formData.auto_launch}
                                                        onCheckedChange={(c) => {
                                                            invoke('toggle_auto_launch', { enable: c }).catch(e => showToast(String(e), 'error'));
                                                            setFormData({ ...formData, auto_launch: c });
                                                        }}
                                                    />
                                                </div>
                                                
                                                <div className="h-px bg-white/5 w-full" />

                                                <div className="flex items-center justify-between p-2 rounded-lg hover:bg-white/5 transition-colors">
                                                    <div className="space-y-1">
                                                        <Label className="text-base text-zinc-200">{t('settings.general.auto_check_update')}</Label>
                                                        <p className="text-sm text-zinc-500">{t('settings.general.auto_check_update_desc')}</p>
                                                    </div>
                                                    <Switch 
                                                        checked={formData.auto_check_update ?? true}
                                                        onCheckedChange={(c) => {
                                                            invoke('save_update_settings', { settings: { auto_check: c, last_check_time: 0, check_interval_hours: formData.update_check_interval ?? 24 } }).catch(e => showToast(String(e), 'error'));
                                                            setFormData({ ...formData, auto_check_update: c });
                                                        }}
                                                    />
                                                </div>
                                            </div>
                                        </SettingsCard>
                                    </>
                                )}

                                {/* --- ACCOUNT TAB --- */}
                                {activeTab === 'account' && (
                                    <SettingsCard title={t('settings.account.sync_settings')} icon={RefreshCw} description="Manage how accounts are synchronized">
                                        <div className="space-y-6">
                                            <div className="flex items-center justify-between p-2 rounded-lg hover:bg-white/5 transition-colors">
                                                <div className="space-y-1">
                                                    <Label className="text-base text-zinc-200">{t('settings.account.auto_refresh')}</Label>
                                                    <p className="text-sm text-zinc-500">{t('settings.account.auto_refresh_desc')}</p>
                                                </div>
                                                <div className="flex items-center gap-2">
                                                    <span className="text-xs font-bold text-indigo-400 uppercase tracking-widest">{t('settings.account.always_on')}</span>
                                                    <div className="w-2 h-2 rounded-full bg-indigo-500 animate-pulse shadow-[0_0_10px_rgba(99,102,241,0.5)]"></div>
                                                </div>
                                            </div>
                                            <div className="flex items-center gap-4 pt-2 pl-2">
                                                <Label className="w-40 text-zinc-400">{t('settings.account.refresh_interval')}</Label>

                                                <Input 
                                                    type="number" 
                                                    className="w-24 bg-zinc-50 dark:bg-zinc-900/50 border-zinc-200 dark:border-white/10 text-zinc-900 dark:text-white"
                                                    value={formData.refresh_interval}
                                                    onChange={(e) => setFormData({ ...formData, refresh_interval: Number(e.target.value) })}
                                                />
                                            </div>
                                            
                                            <div className="h-px bg-white/5 w-full" />

                                            <div className="flex items-center justify-between p-2 rounded-lg hover:bg-white/5 transition-colors">
                                                <div className="space-y-1">
                                                    <Label className="text-base text-zinc-200">{t('settings.account.auto_sync')}</Label>
                                                    <p className="text-sm text-zinc-500">{t('settings.account.auto_sync_desc')}</p>
                                                </div>
                                                <Switch 
                                                    checked={formData.auto_sync}
                                                    onCheckedChange={(c) => setFormData({ ...formData, auto_sync: c })}
                                                />
                                            </div>
                                            {formData.auto_sync && (
                                                <div className="flex items-center gap-4 pt-2 pl-2">
                                                    <Label className="w-40 text-zinc-400">{t('settings.account.sync_interval')}</Label>
                                                    <Input 
                                                        type="number" 
                                                        className="w-24 bg-zinc-50 dark:bg-zinc-900/50 border-zinc-200 dark:border-white/10 text-zinc-900 dark:text-white"
                                                        value={formData.sync_interval}
                                                        onChange={(e) => setFormData({ ...formData, sync_interval: Number(e.target.value) })} 
                                                    />
                                                </div>
                                            )}
                                        </div>
                                    </SettingsCard>
                                )}

                                {/* --- PROXY TAB (Partial Implementation for Brevity) --- */}
                                {activeTab === 'proxy' && (
                                    <>
                                        <SettingsCard title={t('proxy.config.upstream_proxy.title')} icon={Globe}>
                                            <div className="space-y-6">
                                                <div className="flex items-center justify-between">
                                                    <div className="space-y-1">
                                                        <Label className="text-base text-zinc-200">{t('proxy.config.upstream_proxy.enable')}</Label>
                                                        <p className="text-sm text-zinc-500">{t('proxy.config.upstream_proxy.desc')}</p>
                                                    </div>
                                                    <Switch 
                                                        checked={formData.proxy?.upstream_proxy?.enabled || false}
                                                        onCheckedChange={(c) => setFormData({
                                                            ...formData,
                                                            proxy: { ...formData.proxy, upstream_proxy: { ...formData.proxy.upstream_proxy, enabled: c } }
                                                        })}
                                                    />
                                                </div>
                                                <div className="pt-2 space-y-3">
                                                    <Label className="text-zinc-400">{t('proxy.config.upstream_proxy.url')}</Label>
                                                    <Input 
                                                        value={formData.proxy?.upstream_proxy?.url || ''}
                                                        onChange={(e) => setFormData({
                                                            ...formData,
                                                            proxy: { ...formData.proxy, upstream_proxy: { ...formData.proxy.upstream_proxy, url: e.target.value } }
                                                        })}
                                                        className="bg-zinc-50 dark:bg-zinc-900/50 border-zinc-200 dark:border-white/10 text-zinc-900 dark:text-white"
                                                        placeholder={t('proxy.config.upstream_proxy.url_placeholder')}
                                                    />
                                                </div>
                                            </div>
                                        </SettingsCard>

                                    </>
                                )}

                                {/* --- SECURITY TAB --- */}
                                {activeTab === 'security' && (
                                    <>
                                        <SettingsCard title={t('settings.security.quota_protection')} icon={Shield}>
                                            <QuotaProtection
                                                config={formData.quota_protection}
                                                onChange={(newConfig) => setFormData({ ...formData, quota_protection: newConfig })}
                                            />
                                        </SettingsCard>
                                        <SettingsCard title={t('settings.security.monitoring')} icon={Monitor}>
                                            <PinnedQuotaModels
                                                config={formData.pinned_quota_models}
                                                onChange={(newConfig) => setFormData({ ...formData, pinned_quota_models: newConfig })}
                                            />
                                        </SettingsCard>
                                    </>
                                )}

                                {/* --- PERFORMANCE / ADVANCED / ABOUT (Simplified for logic) --- */}
                                {activeTab === 'performance' && (
                                    <SettingsCard title={t('settings.performance.smart_warmup')} icon={Zap}>
                                        <SmartWarmup
                                            config={formData.scheduled_warmup}
                                            onChange={(newConfig) => setFormData({ ...formData, scheduled_warmup: newConfig })}
                                        />
                                    </SettingsCard>
                                )}

                                {activeTab === 'advanced' && (
                                    <SettingsCard title={t('settings.advanced.paths')} icon={Terminal}>
                                        <div className="space-y-6">
                                             {/* Standard Advanced Inputs Re-styled */}
                                              <div>
                                                <Label className="mb-2 block text-zinc-400">{t('settings.advanced.data_dir')}</Label>
                                                <div className="flex gap-2">
                                                    <Input readOnly value={dataDirPath} className="bg-zinc-50 dark:bg-zinc-900/50 border-zinc-200 dark:border-white/10 text-zinc-500 dark:text-zinc-400" />
                                                    {isTauri() && (
                                                        <Button variant="outline" className="border-white/10 bg-white/5 hover:bg-white/10" onClick={() => invoke('open_data_folder')}>
                                                            {t('settings.advanced.open_btn')}
                                                        </Button>
                                                    )}
                                                </div>
                                            </div>
                                        </div>
                                    </SettingsCard>
                                )}

                                {activeTab === 'about' && (
                                     <div className="flex flex-col items-center justify-center space-y-8 animate-in fade-in zoom-in-95 duration-500 py-10">
                                        <div className="text-center space-y-6 relative">
                                            <div className="absolute inset-0 bg-indigo-500/20 rounded-full blur-[60px] animate-pulse"></div>
                                            <div className="relative inline-block group">
                                                <div className="absolute inset-0 bg-indigo-500/30 rounded-3xl blur-xl group-hover:blur-2xl transition-all duration-500"></div>
                                                <img src="/icon.png" alt="Logo" className="relative w-32 h-32 rounded-3xl shadow-2xl transform group-hover:scale-105 transition-all duration-500 bg-zinc-900 object-cover ring-1 ring-white/10" />
                                            </div>
                                            <div>
                                                <h3 className="text-4xl font-black text-white tracking-tighter mb-2">Antigravity Tools</h3>
                                                <div className="flex items-center justify-center gap-3 text-sm">
                                                    <span className="px-3 py-1 rounded-full bg-indigo-500/20 text-indigo-400 font-medium border border-indigo-500/30 shadow-[0_0_15px_rgba(99,102,241,0.2)]">v5.0.0</span>
                                                </div>
                                            </div>
                                        </div>
                                        
                                        <div className="grid grid-cols-2 lg:grid-cols-4 gap-4 w-full max-w-3xl">
                                             {[
                                                { icon: User, label: t('settings.about.author'), value: "GofMan5", color: "text-blue-400", bg: "bg-blue-500/10" },
                                                { icon: MessageCircle, label: t('settings.about.telegram'), value: "@GofMan5", color: "text-blue-400", bg: "bg-blue-500/10", href: "https://t.me/GofMan5" },
                                                { icon: Github, label: t('settings.about.github'), value: "GitHub", color: "text-white", bg: "bg-zinc-800", href: "https://github.com/GofMan5/Antigravity-Manager" }
                                            ].map((item, i) => (
                                                <div 
                                                    key={i} 
                                                    className={cn(
                                                        "group flex flex-col items-center justify-center p-6 rounded-2xl border border-white/5 bg-zinc-900/50 hover:bg-zinc-900 hover:border-indigo-500/30 transition-all duration-300 cursor-pointer relative overflow-hidden",
                                                        item.href ? "cursor-pointer" : ""
                                                    )}
                                                >
                                                    {item.href ? (
                                                        <a href={item.href} target="_blank" rel="noreferrer" className="flex flex-col items-center gap-4 w-full h-full relative z-10">
                                                            <div className={cn("p-4 rounded-2xl transition-transform group-hover:scale-110 shadow-lg", item.bg)}><item.icon className={cn("w-8 h-8", item.color)} /></div>
                                                            <div className="text-center space-y-1">
                                                                <div className="text-[10px] text-zinc-500 uppercase tracking-widest font-bold">{item.label}</div>
                                                                <div className="font-bold text-lg text-white group-hover:text-indigo-300 transition-colors">{item.value}</div>
                                                            </div>
                                                        </a>
                                                    ) : (
                                                        <div className="flex flex-col items-center gap-4 w-full h-full relative z-10">
                                                            <div className={cn("p-4 rounded-2xl transition-transform group-hover:scale-110 shadow-lg", item.bg)}><item.icon className={cn("w-8 h-8", item.color)} /></div>
                                                            <div className="text-center space-y-1">
                                                                <div className="text-[10px] text-zinc-500 uppercase tracking-widest font-bold">{item.label}</div>
                                                                <div className="font-bold text-lg text-white group-hover:text-indigo-300 transition-colors">{item.value}</div>
                                                            </div>
                                                        </div>
                                                    )}
                                                    {/* Glow effect on hover */}
                                                    <div className="absolute inset-0 bg-gradient-to-tr from-white/0 via-white/0 to-white/5 opacity-0 group-hover:opacity-100 transition-opacity duration-500" />
                                                </div>
                                            ))}
                                        </div>
                                    </div>
                                )}
                            </motion.div>
                        </AnimatePresence>
                    </div>
                </div>
            </main>
                </div>
            </div>

            {/* --- MODALS (Preserved) --- */}

        </div>
    );
};
export default Settings;
