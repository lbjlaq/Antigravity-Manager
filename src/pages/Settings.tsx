import { useState, useEffect } from 'react';
import { Save, Github, User, MessageCircle, ExternalLink, RefreshCw, Heart, Coffee, LayoutDashboard, Users, Network, Activity, BarChart3, Settings as SettingsIcon, Lock, CheckCircle2, Globe, Send } from 'lucide-react';
import { request as invoke } from '../utils/request';
import { open } from '@tauri-apps/plugin-dialog';
import { useConfigStore } from '../stores/useConfigStore';
import { AppConfig } from '../types/config';
import ModalDialog from '../components/common/ModalDialog';
import { showToast } from '../components/common/ToastContainer';
import QuotaProtection from '../components/settings/QuotaProtection';
// import SmartWarmup from '../components/settings/SmartWarmup';
import PinnedQuotaModels from '../components/settings/PinnedQuotaModels';
import { useDebugConsole } from '../stores/useDebugConsole';

import { useTranslation } from 'react-i18next';
import { isTauri } from '../utils/env';
import { relaunch } from '@tauri-apps/plugin-process';

import DebugConsole from '../components/debug/DebugConsole';
import ProxyPoolSettings from '../components/settings/ProxyPoolSettings';


function Settings() {
    const { t, i18n } = useTranslation();
    const { config, loadConfig, saveConfig, updateLanguage, updateTheme } = useConfigStore();
    const { enable, disable, isEnabled } = useDebugConsole();
    const [activeTab, setActiveTab] = useState<'general' | 'account' | 'proxy' | 'advanced' | 'debug' | 'about'>('general');
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
            upstream_proxy: {
                enabled: false,
                url: ''
            },
            debug_logging: {
                enabled: false,
                output_dir: undefined
            } as { enabled: boolean; output_dir?: string },
            proxy_pool: {
                enabled: false,
                proxies: [],
                health_check_interval: 300,
                auto_failover: true,
                strategy: 'priority',
                account_bindings: {}
            }
        },
        scheduled_warmup: {
            enabled: false,
            monitored_models: []
        },
        quota_protection: {
            enabled: false,
            threshold_percentage: 10,
            monitored_models: []
        },
        pinned_quota_models: {
            models: ['gemini-3-pro-high', 'gemini-3-flash', 'gemini-3-pro-image', 'claude-opus-4-6-thinking']
        },
        cloudflared: {
            enabled: false,
            mode: 'quick',
            port: 7860,
            use_http2: true
        },
        circuit_breaker: {
            enabled: false,
            backoff_steps: [30, 60, 120, 300, 600]
        },
        hidden_menu_items: [],  // 菜单显示设置：默认不隐藏任何菜单项

    });

    // Dialog state
    // Dialog state
    const [isClearLogsOpen, setIsClearLogsOpen] = useState(false);
    const [isSupportModalOpen, setIsSupportModalOpen] = useState(false);
    const [dataDirPath, setDataDirPath] = useState<string>('~/.antigravity_tools/');

    // Antigravity cache clearing state
    const [isClearCacheOpen, setIsClearCacheOpen] = useState(false);
    const [cachePaths, setCachePaths] = useState<string[]>([]);
    const [isClearingCache, setIsClearingCache] = useState(false);

    // Update check state
    const [isCheckingUpdate, setIsCheckingUpdate] = useState(false);
    const [updateInfo, setUpdateInfo] = useState<{
        hasUpdate: boolean;
        latestVersion: string;
        currentVersion: string;
        downloadUrl: string;
        source?: string;
    } | null>(null);

    // Homebrew Cask state
    const [isBrewInstalled, setIsBrewInstalled] = useState(false);
    const [isBrewUpgrading, setIsBrewUpgrading] = useState(false);
    const [isBrewConfirmOpen, setIsBrewConfirmOpen] = useState(false);
    const [isBrewSuccessOpen, setIsBrewSuccessOpen] = useState(false);


    useEffect(() => {
        loadConfig();

        // 获取真实数据目录路径
        invoke<string>('get_data_dir_path')
            .then(path => setDataDirPath(path))
            .catch(err => console.error('Failed to get data dir:', err));

        // 加载更新设置
        invoke<{ auto_check: boolean; last_check_time: number; check_interval_hours: number }>('get_update_settings')
            .then(settings => {
                setFormData(prev => ({
                    ...prev,
                    auto_check_update: settings.auto_check,
                    update_check_interval: settings.check_interval_hours
                }));
            })
            .catch(err => console.error('Failed to load update settings:', err));

        // 获取真实的开机自启状态
        invoke<boolean>('is_auto_launch_enabled')
            .then(enabled => {
                setFormData(prev => ({ ...prev, auto_launch: enabled }));
            })
            .catch(err => console.error('Failed to get auto launch status:', err));

        // 检测是否通过 Homebrew Cask 安装 (仅 Tauri 环境)
        if (isTauri()) {
            invoke<boolean>('check_homebrew_installation')
                .then(installed => setIsBrewInstalled(installed))
                .catch(err => console.error('Failed to check Homebrew installation:', err));
        }

    }, [loadConfig]);

    useEffect(() => {
        if (config) {
            setFormData(config);
        }
    }, [config]);

    // 删除自动启用调试控制台的逻辑 - 改为用户手动控制

    const handleSave = async () => {
        try {
            // 校验：如果启用了上游代理但没有填写地址，给出提示
            const proxyEnabled = formData.proxy?.upstream_proxy?.enabled;
            const proxyUrl = formData.proxy?.upstream_proxy?.url?.trim();
            if (proxyEnabled && !proxyUrl) {
                showToast(t('proxy.config.upstream_proxy.validation_error'), 'error');
                return;
            }

            await saveConfig(formData);
            showToast(t('common.saved'), 'success');

            // 如果修改了代理配置，提示用户需要重启
            if (proxyEnabled && proxyUrl) {
                showToast(t('proxy.config.upstream_proxy.restart_hint'), 'info');
            }
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const confirmClearLogs = async () => {
        try {
            await invoke('clear_log_cache');
            showToast(t('settings.advanced.logs_cleared'), 'success');
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
        setIsClearLogsOpen(false);
    };

    const handleOpenDataDir = async () => {
        try {
            await invoke('open_data_folder');
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleSelectExportPath = async () => {
        try {
            // @ts-ignore
            const selected = await open({
                directory: true,
                multiple: false,
                title: t('settings.advanced.export_path'),
            });
            if (selected && typeof selected === 'string') {
                setFormData({ ...formData, default_export_path: selected });
            }
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleSelectAntigravityPath = async () => {
        try {
            const selected = await open({
                directory: false,
                multiple: false,
                title: t('settings.advanced.antigravity_path_select'),
            });
            if (selected && typeof selected === 'string') {
                setFormData({ ...formData, antigravity_executable: selected });
            }
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleSelectDebugLogDir = async () => {
        try {
            const selected = await open({
                directory: true,
                multiple: false,
                title: t('settings.advanced.debug_log_dir_select'),
            });
            if (selected && typeof selected === 'string') {
                setFormData({
                    ...formData,
                    proxy: {
                        ...formData.proxy,
                        debug_logging: {
                            enabled: formData.proxy?.debug_logging?.enabled ?? false,
                            output_dir: selected,
                        },
                    },
                });
            }
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleDetectAntigravityPath = async () => {
        try {
            const command = isTauri() ? 'get_antigravity_path' : 'get_antigravity_path'; // 后端已统一
            const path = await invoke<string>(command, { bypassConfig: true });
            setFormData({ ...formData, antigravity_executable: path });
            showToast(t('settings.advanced.antigravity_path_detected'), 'success');
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleCheckUpdate = async () => {
        setIsCheckingUpdate(true);
        setUpdateInfo(null);
        try {
            const result = await invoke<{
                has_update: boolean;
                latest_version: string;
                current_version: string;
                download_url: string;
                source?: string;
            }>('check_for_updates');

            setUpdateInfo({
                hasUpdate: result.has_update,
                latestVersion: result.latest_version,
                currentVersion: result.current_version,
                downloadUrl: result.download_url,
                source: result.source,
            });

            if (result.has_update) {
                const sourceMsg = result.source && result.source !== 'GitHub API' ? ` (via ${result.source})` : '';
                showToast(t('settings.about.new_version_available', { version: result.latest_version }) + sourceMsg, 'info');
            } else {
                showToast(t('settings.about.latest_version'), 'success');
            }
        } catch (error) {
            showToast(`${t('settings.about.update_check_failed')}: ${error}`, 'error');
        } finally {
            setIsCheckingUpdate(false);
        }
    };

    const handleBrewUpgrade = async () => {
        setIsBrewConfirmOpen(false);
        setIsBrewUpgrading(true);
        try {
            await invoke<string>('brew_upgrade_cask');
            setUpdateInfo(null);
            setIsBrewSuccessOpen(true);
        } catch (error) {
            const errKey = String(error);
            const errMsg = t(`settings.about.brew_error_${errKey}`, t('settings.about.brew_upgrade_failed'));
            showToast(errMsg, 'error');
        } finally {
            setIsBrewUpgrading(false);
        }
    };

    // Handle opening cache clear dialog
    const handleOpenClearCacheDialog = async () => {
        try {
            const paths = await invoke<string[]>('get_antigravity_cache_paths');
            setCachePaths(paths);
            setIsClearCacheOpen(true);
        } catch (error) {
            // If no cache paths found, still allow opening the dialog
            setCachePaths([]);
            setIsClearCacheOpen(true);
        }
    };

    // Handle clearing Antigravity cache
    const confirmClearAntigravityCache = async () => {
        setIsClearingCache(true);
        try {
            const result = await invoke<{
                cleared_paths: string[];
                total_size_freed: number;
                errors: string[];
            }>('clear_antigravity_cache');

            const sizeMB = (result.total_size_freed / 1024 / 1024).toFixed(2);

            if (result.cleared_paths.length > 0) {
                showToast(t('settings.advanced.cache_cleared_success', { size: sizeMB }), 'success');
            } else if (result.errors.length > 0) {
                showToast(`${t('common.error')}: ${result.errors[0]}`, 'error');
            } else {
                showToast(t('settings.advanced.cache_not_found'), 'info');
            }
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        } finally {
            setIsClearingCache(false);
            setIsClearCacheOpen(false);
        }
    };

    return (
        <div className="h-full w-full overflow-y-auto">
            <div className="p-5 space-y-4 max-w-7xl mx-auto">
                {/* 顶部工具栏：Tab 导航和保存按钮 */}
                <div className="flex justify-between items-center">
                    {/* Tab 导航 - 采用顶部导航栏样式：外层灰色容器 */}
                    <div className="flex items-center gap-1 bg-gray-100 dark:bg-base-200 rounded-full p-1 w-fit">
                        <button
                            className={`px-6 py-2 rounded-full text-sm font-medium transition-all ${activeTab === 'general'
                                ? 'bg-gray-200 dark:bg-gray-700 text-gray-900 dark:text-gray-100 shadow-sm'
                                : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200'
                                }`}
                            onClick={() => setActiveTab('general')}
                        >
                            {t('settings.tabs.general')}
                        </button>
                        <button
                            className={`px-6 py-2 rounded-full text-sm font-medium transition-all ${activeTab === 'account'
                                ? 'bg-gray-200 dark:bg-gray-700 text-gray-900 dark:text-gray-100 shadow-sm'
                                : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200'
                                }`}
                            onClick={() => setActiveTab('account')}
                        >
                            {t('settings.tabs.account')}
                        </button>
                        <button
                            className={`px-6 py-2 rounded-full text-sm font-medium transition-all ${activeTab === 'proxy'
                                ? 'bg-gray-200 dark:bg-gray-700 text-gray-900 dark:text-gray-100 shadow-sm'
                                : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200'
                                }`}
                            onClick={() => setActiveTab('proxy')}
                        >
                            {t('settings.tabs.proxy')}
                        </button>
                        <button
                            className={`px-6 py-2 rounded-full text-sm font-medium transition-all ${activeTab === 'advanced'
                                ? 'bg-gray-200 dark:bg-gray-700 text-gray-900 dark:text-gray-100 shadow-sm'
                                : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200'
                                }`}
                            onClick={() => setActiveTab('advanced')}
                        >
                            {t('settings.tabs.advanced')}
                        </button>
                        <button
                            className={`px-6 py-2 rounded-full text-sm font-medium transition-all ${activeTab === 'debug'
                                ? 'bg-gray-200 dark:bg-gray-700 text-gray-900 dark:text-gray-100 shadow-sm'
                                : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200'
                                }`}
                            onClick={() => setActiveTab('debug')}
                        >
                            {t('settings.tabs.debug')}
                        </button>
                        <button
                            className={`px-6 py-2 rounded-full text-sm font-medium transition-all ${activeTab === 'about'
                                ? 'bg-gray-200 dark:bg-gray-700 text-gray-900 dark:text-gray-100 shadow-sm'
                                : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200'
                                }`}
                            onClick={() => setActiveTab('about')}
                        >
                            {t('settings.tabs.about')}
                        </button>
                    </div>

                    <button
                        className="px-4 py-2 bg-blue-500 text-white text-sm rounded-lg hover:bg-blue-600 transition-colors flex items-center gap-2 shadow-sm"
                        onClick={handleSave}
                    >
                        <Save className="w-4 h-4" />
                        {t('settings.save')}
                    </button>
                </div>

                {/* 设置表单 */}
                <div className="bg-white dark:bg-base-100 rounded-2xl p-6 shadow-sm border border-gray-100 dark:border-base-200">
                    {/* 通用设置 */}
                    {activeTab === 'general' && (
                        <div className="space-y-6">
                            <h2 className="text-lg font-semibold text-gray-900 dark:text-base-content">{t('settings.general.title')}</h2>

                            {/* 语言选择 */}
                            <div>
                                <label className="block text-sm font-medium text-gray-900 dark:text-base-content mb-2">{t('settings.general.language')}</label>
                                <select
                                    className="w-full px-4 py-4 border border-gray-200 dark:border-base-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent text-gray-900 dark:text-base-content bg-gray-50 dark:bg-base-200"
                                    value={formData.language}
                                    onChange={(e) => {
                                        const newLang = e.target.value;
                                        setFormData({ ...formData, language: newLang });
                                        i18n.changeLanguage(newLang);
                                        updateLanguage(newLang);
                                    }}
                                >
                                    <option value="zh">简体中文</option>
                                    <option value="zh-TW">繁體中文</option>
                                    <option value="en">English</option>
                                    <option value="ja">日本語</option>
                                    <option value="tr">Türkçe</option>
                                    <option value="vi">Tiếng Việt</option>
                                    <option value="pt">Português</option>
                                    <option value="ko">한국어</option>
                                    <option value="ru">Русский</option>
                                    <option value="ar">العربية</option>
                                </select>
                            </div>

                            {/* 主题选择 */}
                            <div>
                                <label className="block text-sm font-medium text-gray-900 dark:text-base-content mb-2">{t('settings.general.theme')}</label>
                                <select
                                    className="w-full px-4 py-4 border border-gray-200 dark:border-base-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent text-gray-900 dark:text-base-content bg-gray-50 dark:bg-base-200"
                                    value={formData.theme}
                                    onChange={(e) => {
                                        const newTheme = e.target.value;
                                        setFormData({ ...formData, theme: newTheme });
                                        updateTheme(newTheme);
                                    }}
                                >
                                    <option value="light">{t('settings.general.theme_light')}</option>
                                    <option value="dark">{t('settings.general.theme_dark')}</option>
                                    <option value="system">{t('settings.general.theme_system')}</option>
                                </select>
                            </div>

                            {/* 开机自动启动 */}
                            <div>
                                <div className="flex justify-between items-center mb-2">
                                    <label className="block text-sm font-medium text-gray-900 dark:text-base-content">{t('settings.general.auto_launch')}</label>
                                    {!isTauri() && (
                                        <span className="text-xs text-orange-500 dark:text-orange-400">
                                            {t('settings.web_mode_limitation', '(Web 模式不支持)')}
                                        </span>
                                    )}
                                </div>
                                <select
                                    className="w-full px-4 py-4 border border-gray-200 dark:border-base-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent text-gray-900 dark:text-base-content bg-gray-50 dark:bg-base-200"
                                    value={formData.auto_launch ? 'enabled' : 'disabled'}
                                    onChange={async (e) => {
                                        const enabled = e.target.value === 'enabled';
                                        try {
                                            await invoke('toggle_auto_launch', { enable: enabled });
                                            setFormData({ ...formData, auto_launch: enabled });
                                            showToast(enabled ? t('settings.general.auto_launch_enabled') : t('settings.general.auto_launch_disabled'), 'success');
                                        } catch (error) {
                                            showToast(`${t('common.error')}: ${error}`, 'error');
                                        }
                                    }}
                                >
                                    <option value="disabled">{t('settings.general.auto_launch_disabled')}</option>
                                    <option value="enabled" disabled={!isTauri()}>{t('settings.general.auto_launch_enabled')}</option>

                                </select>
                                <p className="text-sm text-gray-500 dark:text-gray-400 mt-2">{t('settings.general.auto_launch_desc')}</p>
                            </div>

                            {/* 自动检查更新 */}
                            <>
                                <div className="flex items-center justify-between p-4 bg-gray-50 dark:bg-base-200 rounded-lg border border-gray-100 dark:border-base-300">
                                    <div>
                                        <div className="font-medium text-gray-900 dark:text-base-content">{t('settings.general.auto_check_update')}</div>
                                        <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">{t('settings.general.auto_check_update_desc')}</p>
                                    </div>
                                    <label className="relative inline-flex items-center cursor-pointer">
                                        <input
                                            type="checkbox"
                                            className="sr-only peer"
                                            checked={formData.auto_check_update ?? true}
                                            onChange={async (e) => {
                                                const enabled = e.target.checked;
                                                try {
                                                    await invoke('save_update_settings', {
                                                        settings: {
                                                            auto_check: enabled,
                                                            last_check_time: 0,
                                                            check_interval_hours: formData.update_check_interval ?? 24
                                                        }
                                                    });
                                                    setFormData({ ...formData, auto_check_update: enabled });
                                                    showToast(enabled ? t('settings.general.auto_check_update_enabled') : t('settings.general.auto_check_update_disabled'), 'success');
                                                } catch (error) {
                                                    showToast(`${t('common.error')}: ${error}`, 'error');
                                                }
                                            }}
                                        />
                                        <div className="w-11 h-6 bg-gray-200 dark:bg-base-300 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-blue-300 dark:peer-focus:ring-blue-800 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-500"></div>
                                    </label>
                                </div>

                                {/* 检查间隔 */}
                                {formData.auto_check_update && (
                                    <div className="ml-4">
                                        <label className="block text-sm font-medium text-gray-900 dark:text-base-content mb-2">{t('settings.general.update_check_interval')}</label>
                                        <input
                                            type="number"
                                            className="w-32 px-4 py-4 border border-gray-200 dark:border-base-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent text-gray-900 dark:text-base-content bg-gray-50 dark:bg-base-200"
                                            min="1"
                                            max="168"
                                            value={formData.update_check_interval ?? 24}
                                            onChange={(e) => setFormData({ ...formData, update_check_interval: parseInt(e.target.value) })}
                                            onBlur={async () => {
                                                try {
                                                    await invoke('save_update_settings', {
                                                        settings: {
                                                            auto_check: formData.auto_check_update ?? true,
                                                            last_check_time: 0,
                                                            check_interval_hours: formData.update_check_interval ?? 24
                                                        }
                                                    });
                                                    showToast(t('settings.general.update_check_interval_saved'), 'success');
                                                } catch (error) {
                                                    showToast(`${t('common.error')}: ${error}`, 'error');
                                                }
                                            }}
                                        />
                                        <p className="text-sm text-gray-500 dark:text-gray-400 mt-2">{t('settings.general.update_check_interval_desc')}</p>
                                    </div>
                                )}

                                {/* 菜单显示设置 */}
                                <div className="border-t border-gray-200 dark:border-base-200 pt-6 mt-6">
                                    <h3 className="font-medium text-gray-900 dark:text-base-content mb-3">{t('settings.menu.title')}</h3>
                                    <p className="text-sm text-gray-600 dark:text-gray-400 mb-4">
                                        {t('settings.menu.desc')}
                                    </p>
                                    <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
                                        {[
                                            { path: '/', label: t('nav.dashboard'), icon: LayoutDashboard },
                                            { path: '/accounts', label: t('nav.accounts'), icon: Users },
                                            { path: '/api-proxy', label: t('nav.proxy'), icon: Network },
                                            { path: '/monitor', label: t('nav.call_records'), icon: Activity },
                                            { path: '/token-stats', label: t('nav.token_stats'), icon: BarChart3 },
                                            { path: '/user-token', label: t('nav.user_token', 'User Tokens'), icon: Users },
                                            { path: '/security', label: t('nav.security'), icon: Lock },
                                            { path: '/settings', label: t('nav.settings'), icon: SettingsIcon },
                                        ].map((item) => {
                                            const hiddenItems = formData.hidden_menu_items || [];
                                            const isVisible = !hiddenItems.includes(item.path);
                                            const isSettings = item.path === '/settings';

                                            return (
                                                <div
                                                    key={item.path}
                                                    onClick={async () => {
                                                        if (!isSettings) {
                                                            const originalConfig = { ...formData };
                                                            const hiddenItems = formData.hidden_menu_items || [];
                                                            const newHiddenItems = isVisible
                                                                ? [...hiddenItems, item.path]
                                                                : hiddenItems.filter(p => p !== item.path);

                                                            // 乐观更新 UI
                                                            const newConfig = {
                                                                ...formData,
                                                                hidden_menu_items: newHiddenItems
                                                            };
                                                            setFormData(newConfig);

                                                            // 尝试保存
                                                            try {
                                                                await saveConfig(newConfig);
                                                            } catch (error) {
                                                                // 保存失败，回滚到原始快照
                                                                setFormData(originalConfig);
                                                                showToast(`保存失败，已恢复设置: ${error}`, 'error');
                                                            }
                                                        }
                                                    }}
                                                    className={`
                                                        relative flex flex-col items-center justify-center gap-3 p-4 rounded-xl border-2 transition-all cursor-pointer select-none
                                                        ${isSettings
                                                            ? 'bg-gray-50 dark:bg-base-200 border-gray-100 dark:border-base-300 opacity-60 cursor-not-allowed'
                                                            : isVisible
                                                                ? 'bg-blue-50/50 dark:bg-blue-900/10 border-blue-500 dark:border-blue-500 shadow-sm'
                                                                : 'bg-white dark:bg-base-100 border-gray-200 dark:border-base-300 hover:border-gray-300 dark:hover:border-base-content/20 text-gray-500'
                                                        }
                                                    `}
                                                >
                                                    {/* 选中标记 */}
                                                    {isVisible && (
                                                        <div className="absolute top-2 right-2 text-blue-500">
                                                            <CheckCircle2 size={16} fill="currentColor" className="text-white dark:text-base-100" />
                                                        </div>
                                                    )}

                                                    {isSettings && (
                                                        <div className="absolute top-2 right-2 text-xs font-bold text-gray-400 bg-gray-200 dark:bg-base-300 px-1.5 py-0.5 rounded">
                                                            {t('settings.menu.required')}
                                                        </div>
                                                    )}

                                                    <div className={`
                                                        p-3 rounded-xl transition-colors
                                                        ${isVisible
                                                            ? 'bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400'
                                                            : 'bg-gray-100 dark:bg-base-200 text-gray-400 dark:text-base-content/50'
                                                        }
                                                    `}>
                                                        <item.icon size={24} />
                                                    </div>

                                                    <span className={`font-medium text-sm ${isVisible ? 'text-blue-900 dark:text-blue-100' : 'text-gray-500'}`}>
                                                        {item.label}
                                                    </span>
                                                </div>
                                            );
                                        })}
                                    </div>
                                    <p className="text-xs text-gray-500 dark:text-gray-400 mt-4 flex items-center gap-1.5">
                                        <div className="w-1.5 h-1.5 rounded-full bg-gray-400"></div>
                                        {t('settings.menu.selected_items_note')}
                                    </p>
                                </div>
                            </>
                        </div>
                    )}

                    {/* 账号设置 */}
                    {activeTab === 'account' && (
                        <div className="space-y-4 animate-in fade-in duration-500">
                            {/* 自动刷新配额 */}
                            <div className="group bg-white dark:bg-base-100 rounded-xl p-5 border border-gray-100 dark:border-base-200 hover:border-blue-200 transition-all duration-300 shadow-sm">
                                <div className="flex items-center justify-between">
                                    <div className="flex items-center gap-4">
                                        <div className="w-10 h-10 rounded-xl bg-blue-50 dark:bg-blue-900/20 flex items-center justify-center text-blue-500 group-hover:bg-blue-500 group-hover:text-white transition-all duration-300">
                                            <RefreshCw size={20} />
                                        </div>
                                        <div>
                                            <div className="font-bold text-gray-900 dark:text-gray-100">{t('settings.account.auto_refresh')}</div>
                                            <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">{t('settings.account.auto_refresh_desc')}</p>
                                        </div>
                                    </div>
                                    <label className={`relative inline-flex items-center ${formData.quota_protection.enabled ? 'cursor-not-allowed opacity-80' : 'cursor-pointer'}`}>
                                        <input
                                            type="checkbox"
                                            className="sr-only peer"
                                            checked={formData.auto_refresh}
                                            disabled={formData.quota_protection.enabled}
                                            onChange={async (e) => {
                                                const enabled = e.target.checked;
                                                const newConfig = { ...formData, auto_refresh: enabled };
                                                setFormData(newConfig);
                                                // Hot Save
                                                try {
                                                    await saveConfig(newConfig);
                                                } catch (error) {
                                                    showToast(`${t('common.error')}: ${error}`, 'error');
                                                }
                                            }}
                                        />
                                        <div className={`w-11 h-6 bg-gray-200 dark:bg-base-300 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-500 shadow-inner ${formData.quota_protection.enabled ? 'peer-checked:bg-blue-500' : ''}`}></div>
                                    </label>
                                </div>

                                <div className="mt-5 pt-5 border-t border-gray-50 dark:border-base-300 flex items-center gap-4 animate-in slide-in-from-top-1 duration-200">
                                    <label className="text-xs font-bold text-gray-500 dark:text-gray-400 uppercase tracking-wider">{t('settings.account.refresh_interval')}</label>
                                    <div className="relative">
                                        <input
                                            type="number"
                                            className="w-24 px-3 py-2 bg-gray-50 dark:bg-base-200 border border-gray-100 dark:border-base-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none text-sm font-bold text-blue-600 dark:text-blue-400"
                                            min="1"
                                            max="35791"
                                            value={formData.refresh_interval}
                                            onChange={(e) => setFormData({ ...formData, refresh_interval: isNaN(parseInt(e.target.value)) ? 1 : Math.min(Math.max(parseInt(e.target.value), 1), 35791) })}
                                        />
                                    </div>
                                </div>
                            </div>

                            {/* 自动获取当前账号 */}
                            <div className="group bg-white dark:bg-base-100 rounded-xl p-5 border border-gray-100 dark:border-base-200 hover:border-emerald-200 transition-all duration-300 shadow-sm">
                                <div className="flex items-center justify-between">
                                    <div className="flex items-center gap-4">
                                        <div className="w-10 h-10 rounded-xl bg-emerald-50 dark:bg-emerald-900/20 flex items-center justify-center text-emerald-500 group-hover:bg-emerald-500 group-hover:text-white transition-all duration-300">
                                            <User size={20} />
                                        </div>
                                        <div>
                                            <div className="font-bold text-gray-900 dark:text-gray-100">{t('settings.account.auto_sync')}</div>
                                            <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">{t('settings.account.auto_sync_desc')}</p>
                                        </div>
                                    </div>
                                    <label className="relative inline-flex items-center cursor-pointer">
                                        <input
                                            type="checkbox"
                                            className="sr-only peer"
                                            checked={formData.auto_sync}
                                            onChange={(e) => setFormData({ ...formData, auto_sync: e.target.checked })}
                                        />
                                        <div className="w-11 h-6 bg-gray-200 dark:bg-base-300 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-emerald-500 shadow-inner"></div>
                                    </label>
                                </div>

                                {formData.auto_sync && (
                                    <div className="mt-5 pt-5 border-t border-gray-50 dark:border-base-300 flex items-center gap-4 animate-in slide-in-from-top-1 duration-200">
                                        <label className="text-xs font-bold text-gray-500 dark:text-gray-400 uppercase tracking-wider">{t('settings.account.sync_interval')}</label>
                                        <input
                                            type="number"
                                            className="w-24 px-3 py-2 bg-gray-50 dark:bg-base-200 border border-gray-100 dark:border-base-300 rounded-lg focus:ring-2 focus:ring-emerald-500 outline-none text-sm font-bold text-emerald-600 dark:text-emerald-400"
                                            min="1"
                                            max="35791"
                                            value={formData.sync_interval}
                                            onChange={(e) => setFormData({ ...formData, sync_interval: isNaN(parseInt(e.target.value)) ? 1 : Math.min(Math.max(parseInt(e.target.value), 1), 35791) })}
                                        />
                                    </div>
                                )}
                            </div>

                            {/* 智能预热 (Smart Warmup) - [DISABLED] Backend scheduler commented out as per user request */}
                            {/* <div className="group bg-white dark:bg-base-100 rounded-xl p-5 border border-gray-100 dark:border-base-200 hover:border-orange-200 transition-all duration-300 shadow-sm">
                                <SmartWarmup
                                    config={formData.scheduled_warmup}
                                    onChange={async (newConfig) => {
                                        const newFormData = {
                                            ...formData,
                                            scheduled_warmup: newConfig
                                        };
                                        setFormData(newFormData);
                                        // Hot Save
                                        try {
                                            await saveConfig(newFormData);
                                        } catch (error) {
                                            showToast(`${t('common.error')}: ${error}`, 'error');
                                        }
                                    }}
                                />
                            </div> */}

                            {/* 配额保护 (Quota Protection) */}
                            <div className="group bg-white dark:bg-base-100 rounded-xl p-5 border border-gray-100 dark:border-base-200 hover:border-rose-200 transition-all duration-300 shadow-sm">
                                <QuotaProtection
                                    config={formData.quota_protection}
                                    onChange={async (newConfig) => {
                                        const updates: any = {
                                            quota_protection: newConfig
                                        };
                                        // 联动逻辑：开启配额保护时，强制开启后台自动刷新 (不仅仅是预热)
                                        if (newConfig.enabled) {
                                            updates.auto_refresh = true;
                                        }

                                        const newFormData = {
                                            ...formData,
                                            ...updates
                                        };
                                        setFormData(newFormData);

                                        // Hot Save
                                        try {
                                            await saveConfig(newFormData);
                                        } catch (error) {
                                            showToast(`${t('common.error')}: ${error}`, 'error');
                                        }
                                    }}
                                />
                            </div>

                            {/* 配额关注列表 (Pinned Quota Models) */}
                            <div className="group bg-white dark:bg-base-100 rounded-xl p-5 border border-gray-100 dark:border-base-200 hover:border-indigo-200 transition-all duration-300 shadow-sm">
                                <PinnedQuotaModels
                                    config={formData.pinned_quota_models}
                                    onChange={(newConfig) => setFormData({
                                        ...formData,
                                        pinned_quota_models: newConfig
                                    })}
                                />
                            </div>
                        </div>
                    )}

                    {/* 高级设置 */}
                    {activeTab === 'advanced' && (
                        <>
                            <div className="space-y-4">
                                {/* 默认导出路径 */}
                                <div>
                                    <label className="block text-sm font-medium text-gray-900 dark:text-base-content mb-1">{t('settings.advanced.export_path')}</label>
                                    <div className="flex gap-2">
                                        <input
                                            type="text"
                                            className="flex-1 px-4 py-4 border border-gray-200 dark:border-base-300 rounded-lg bg-gray-50 dark:bg-base-200 text-gray-900 dark:text-base-content font-medium"
                                            value={formData.default_export_path || t('settings.advanced.export_path_placeholder')}
                                            readOnly
                                        />
                                        {formData.default_export_path && (
                                            <button
                                                className="px-4 py-2 border border-gray-200 dark:border-base-300 text-red-600 dark:text-red-400 rounded-lg hover:bg-red-50 dark:hover:bg-red-900/10 transition-colors"
                                                onClick={() => setFormData({ ...formData, default_export_path: undefined })}
                                            >
                                                {t('common.clear')}
                                            </button>
                                        )}
                                        {isTauri() ? (
                                            <button
                                                className="px-4 py-2 border border-gray-200 dark:border-base-300 text-gray-700 dark:text-gray-300 rounded-lg hover:bg-gray-50 dark:hover:bg-base-200 hover:text-gray-900 dark:hover:text-base-content transition-colors"
                                                onClick={handleSelectExportPath}
                                            >
                                                {t('settings.advanced.select_btn')}
                                            </button>
                                        ) : (
                                            <span className="self-center text-xs text-gray-400 dark:text-gray-500 italic px-2">
                                                {t('settings.web_mode_limitation', '(Web 模式不支持)')}
                                            </span>
                                        )}
                                    </div>
                                    <p className="text-sm text-gray-500 dark:text-gray-400 mt-2">{t('settings.advanced.default_export_path_desc')}</p>
                                </div>

                                {/* 数据目录 */}
                                <div>
                                    <label className="block text-sm font-medium text-gray-900 dark:text-base-content mb-1">{t('settings.advanced.data_dir')}</label>
                                    <div className="flex gap-2">
                                        <input
                                            type="text"
                                            className="flex-1 px-4 py-4 border border-gray-200 dark:border-base-300 rounded-lg bg-gray-50 dark:bg-base-200 text-gray-900 dark:text-base-content font-medium"
                                            value={dataDirPath}
                                            readOnly
                                        />
                                        {isTauri() ? (
                                            <button
                                                className="px-4 py-2 border border-gray-200 dark:border-base-300 text-gray-700 dark:text-gray-300 rounded-lg hover:bg-gray-50 dark:hover:bg-base-200 hover:text-gray-900 dark:hover:text-base-content transition-colors"
                                                onClick={handleOpenDataDir}
                                            >
                                                {t('settings.advanced.open_btn')}
                                            </button>
                                        ) : (
                                            <span className="self-center text-xs text-gray-400 dark:text-gray-500 italic px-2">
                                                {t('settings.web_mode_limitation', '(Web 模式不支持)')}
                                            </span>
                                        )}
                                    </div>
                                    <p className="text-sm text-gray-500 dark:text-gray-400 mt-2">{t('settings.advanced.data_dir_desc')}</p>
                                </div>

                                {/* 反重力程序路径 */}
                                <div>
                                    <label className="block text-sm font-medium text-gray-900 dark:text-base-content mb-1">
                                        {t('settings.advanced.antigravity_path')}
                                    </label>
                                    <div className="flex gap-2">
                                        <input
                                            type="text"
                                            className="flex-1 px-4 py-4 border border-gray-200 dark:border-base-300 rounded-lg bg-gray-50 dark:bg-base-200 text-gray-900 dark:text-base-content font-medium"
                                            value={formData.antigravity_executable || ''}
                                            placeholder={t('settings.advanced.antigravity_path_placeholder')}
                                            onChange={(e) => setFormData({ ...formData, antigravity_executable: e.target.value })}
                                        />
                                        {formData.antigravity_executable && (
                                            <button
                                                className="px-4 py-2 border border-gray-200 dark:border-base-300 text-red-600 dark:text-red-400 rounded-lg hover:bg-red-50 dark:hover:bg-red-900/10 transition-colors"
                                                onClick={() => setFormData({ ...formData, antigravity_executable: undefined })}
                                            >
                                                {t('common.clear')}
                                            </button>
                                        )}
                                        <button
                                            className="px-4 py-2 border border-gray-200 dark:border-base-300 text-gray-700 dark:text-gray-300 rounded-lg hover:bg-gray-50 dark:hover:bg-base-200 transition-colors"
                                            onClick={handleDetectAntigravityPath}
                                        >
                                            {t('settings.advanced.detect_btn')}
                                        </button>
                                        {isTauri() ? (
                                            <button
                                                className="px-4 py-2 border border-gray-200 dark:border-base-300 text-gray-700 dark:text-gray-300 rounded-lg hover:bg-gray-50 dark:hover:bg-base-200 transition-colors"
                                                onClick={handleSelectAntigravityPath}
                                            >
                                                {t('settings.advanced.select_btn')}
                                            </button>
                                        ) : (
                                            <span className="self-center text-xs text-gray-400 dark:text-gray-500 italic px-2">
                                                {t('settings.web_mode_limitation', '(Web 模式不支持)')}
                                            </span>
                                        )}
                                    </div>
                                    <p className="text-sm text-gray-500 dark:text-gray-400 mt-2">
                                        {t('settings.advanced.antigravity_path_desc')}
                                    </p>
                                </div>

                                {/* 反重力程序启动参数 */}
                                <div>
                                    <label className="block text-sm font-medium text-gray-900 dark:text-base-content mb-1">
                                        {t('settings.advanced.antigravity_args')}
                                    </label>
                                    <div className="flex gap-2">
                                        <input
                                            type="text"
                                            className="flex-1 px-4 py-4 border border-gray-200 dark:border-base-300 rounded-lg bg-gray-50 dark:bg-base-200 text-gray-900 dark:text-base-content font-medium"
                                            value={formData.antigravity_args ? formData.antigravity_args.join(' ') : ''}
                                            placeholder={t('settings.advanced.antigravity_args_placeholder')}
                                            onChange={(e) => {
                                                const args = e.target.value.trim() === '' ? [] : e.target.value.split(' ').map(arg => arg.trim()).filter(arg => arg !== '');
                                                setFormData({ ...formData, antigravity_args: args });
                                            }}
                                        />
                                        <button
                                            className="px-4 py-2 border border-gray-200 dark:border-base-300 text-gray-700 dark:text-gray-300 rounded-lg hover:bg-gray-100 dark:hover:bg-base-200 transition-colors"
                                            onClick={async () => {
                                                try {
                                                    const args = await invoke<string[]>('get_antigravity_args');
                                                    setFormData({ ...formData, antigravity_args: args });
                                                    showToast(t('settings.advanced.antigravity_args_detected'), 'success');
                                                } catch (error) {
                                                    showToast(`${t('settings.advanced.antigravity_args_detect_error')}: ${error}`, 'error');
                                                }
                                            }}
                                        >
                                            {t('settings.advanced.detect_args_btn')}
                                        </button>
                                    </div>
                                    <p className="text-sm text-gray-500 dark:text-gray-400 mt-2">
                                        {t('settings.advanced.antigravity_args_desc')}
                                    </p>
                                </div>

                                {/* 日志缓存清理 */}
                                <div className="border-t border-gray-200 dark:border-base-200 pt-4">
                                    <h3 className="font-medium text-gray-900 dark:text-base-content mb-3">{t('settings.advanced.logs_title')}</h3>
                                    <div className="bg-gray-50 dark:bg-base-200 border border-gray-200 dark:border-base-300 rounded-lg p-3 mb-3">
                                        <p className="text-sm text-gray-600 dark:text-gray-400">{t('settings.advanced.logs_desc')}</p>
                                    </div>
                                    <div className="flex items-center gap-4">
                                        <button
                                            className="px-4 py-2 border border-gray-300 dark:border-base-300 text-gray-700 dark:text-gray-300 rounded-lg hover:bg-gray-100 dark:hover:bg-base-200 transition-colors"
                                            onClick={() => setIsClearLogsOpen(true)}
                                        >
                                            {t('settings.advanced.clear_logs')}
                                        </button>
                                    </div>
                                </div>

                                {/* Antigravity 缓存清理 */}
                                <div className="border-t border-gray-200 dark:border-base-200 pt-4">
                                    <h3 className="font-medium text-gray-900 dark:text-base-content mb-3">{t('settings.advanced.antigravity_cache_title')}</h3>
                                    <div className="bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700/30 rounded-lg p-3 mb-3">
                                        <p className="text-sm text-amber-700 dark:text-amber-400">{t('settings.advanced.antigravity_cache_warning')}</p>
                                    </div>
                                    <div className="bg-gray-50 dark:bg-base-200 border border-gray-200 dark:border-base-300 rounded-lg p-3 mb-3">
                                        <p className="text-sm text-gray-600 dark:text-gray-400">{t('settings.advanced.antigravity_cache_desc')}</p>
                                    </div>
                                    <div className="flex items-center gap-4">
                                        <button
                                            className="px-4 py-2 border border-orange-300 dark:border-orange-700 text-orange-700 dark:text-orange-400 rounded-lg hover:bg-orange-50 dark:hover:bg-orange-900/20 transition-colors"
                                            onClick={handleOpenClearCacheDialog}
                                        >
                                            {t('settings.advanced.clear_antigravity_cache')}
                                        </button>
                                    </div>
                                </div>



                                <div className="border-t border-gray-200 dark:border-base-200 pt-4">
                                    <div className="space-y-3">
                                        <div className="flex items-center justify-between p-4 bg-gray-50 dark:bg-base-200 rounded-lg border border-gray-100 dark:border-base-300">
                                            <div>
                                                <div className="font-medium text-gray-900 dark:text-base-content">
                                                    {t('settings.advanced.debug_logs_title')}
                                                </div>
                                                <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">
                                                    {t('settings.advanced.debug_logs_enable_desc')}
                                                </p>
                                            </div>
                                            <label className="relative inline-flex items-center cursor-pointer">
                                                <input
                                                    type="checkbox"
                                                    className="sr-only peer"
                                                    checked={formData.proxy?.debug_logging?.enabled ?? false}
                                                    onChange={(e: React.ChangeEvent<HTMLInputElement>) => setFormData({
                                                        ...formData,
                                                        proxy: {
                                                            ...formData.proxy,
                                                            debug_logging: {
                                                                enabled: e.target.checked,
                                                                output_dir: formData.proxy?.debug_logging?.output_dir,
                                                            },
                                                        },
                                                    })}
                                                />
                                                <div className="w-11 h-6 bg-gray-200 dark:bg-base-300 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-blue-300 dark:peer-focus:ring-blue-800 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-500"></div>
                                            </label>
                                        </div>
                                        {(formData.proxy?.debug_logging?.enabled ?? false) && (
                                            <>
                                                <div className="bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700/30 rounded-lg p-3">
                                                    <p className="text-sm text-amber-700 dark:text-amber-400">
                                                        {t('settings.advanced.debug_logs_desc')}
                                                    </p>
                                                </div>
                                                <div>
                                                    <label className="block text-sm font-medium text-gray-900 dark:text-base-content mb-1">
                                                        {t('settings.advanced.debug_log_dir')}
                                                    </label>
                                                    <div className="flex gap-2">
                                                        <input
                                                            type="text"
                                                            className="flex-1 px-4 py-3 border border-gray-200 dark:border-base-300 rounded-lg bg-gray-50 dark:bg-base-200 text-gray-900 dark:text-base-content font-medium"
                                                            value={formData.proxy?.debug_logging?.output_dir || ''}
                                                            placeholder={`${dataDirPath.replace(/\/$/, '')}/debug_logs`}
                                                            onChange={(e: React.ChangeEvent<HTMLInputElement>) => setFormData({
                                                                ...formData,
                                                                proxy: {
                                                                    ...formData.proxy,
                                                                    debug_logging: {
                                                                        enabled: formData.proxy?.debug_logging?.enabled ?? false,
                                                                        output_dir: e.target.value || undefined,
                                                                    },
                                                                },
                                                            })}
                                                        />
                                                        {isTauri() && (
                                                            <button
                                                                className="px-4 py-2 border border-gray-200 dark:border-base-300 text-gray-700 dark:text-gray-300 rounded-lg hover:bg-gray-50 dark:hover:bg-base-200 transition-colors"
                                                                onClick={handleSelectDebugLogDir}
                                                            >
                                                                {t('settings.advanced.select_btn')}
                                                            </button>
                                                        )}
                                                    </div>
                                                    <p className="text-xs text-gray-500 dark:text-gray-400 mt-2">
                                                        {t('settings.advanced.debug_log_dir_hint', { path: dataDirPath.replace(/\/$/, '') })}
                                                    </p>
                                                </div>
                                            </>
                                        )}
                                    </div>
                                </div>

                            </div>
                        </>
                    )}


                    {/* 调试设置 */}
                    {activeTab === 'debug' && (
                        <div className="space-y-4 animate-in fade-in duration-500">
                            {/* 标题和开关 */}
                            <div className="flex items-center justify-between">
                                <div>
                                    <h2 className="text-lg font-semibold text-gray-900 dark:text-base-content">
                                        {t('settings.debug.title')}
                                    </h2>
                                    <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
                                        {t('settings.debug.desc')}
                                    </p>
                                </div>
                                <label className="relative inline-flex items-center cursor-pointer">
                                    <input
                                        type="checkbox"
                                        className="sr-only peer"
                                        checked={isEnabled}
                                        onChange={(e) => e.target.checked ? enable() : disable()}
                                    />
                                    <div className="w-11 h-6 bg-gray-200 dark:bg-base-300 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-blue-300 dark:peer-focus:ring-blue-800 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-500"></div>
                                    <span className="ml-3 text-sm font-medium text-gray-700 dark:text-gray-300">
                                        {isEnabled ? t('settings.debug.enabled') : t('settings.debug.disabled')}
                                    </span>
                                </label>
                            </div>

                            {/* 控制台或提示 */}
                            {isEnabled ? (
                                <div className="h-[calc(100vh-320px)] min-h-[400px]">
                                    <DebugConsole embedded />
                                </div>
                            ) : (
                                <div className="h-[calc(100vh-320px)] min-h-[400px] flex items-center justify-center bg-gray-50 dark:bg-base-200 rounded-xl border border-gray-200 dark:border-base-300">
                                    <div className="text-center">
                                        <p className="text-gray-500 dark:text-gray-400 text-lg font-medium">
                                            {t('settings.debug.disabled_hint')}
                                        </p>
                                        <p className="text-gray-400 dark:text-gray-500 text-sm mt-2">
                                            {t('settings.debug.disabled_desc')}
                                        </p>
                                    </div>
                                </div>
                            )}
                        </div>
                    )}

                    {/* 代理设置 */}
                    {activeTab === 'proxy' && (
                        <div className="space-y-4 animate-in fade-in duration-300">
                            <ProxyPoolSettings
                                config={formData.proxy?.proxy_pool || {
                                    enabled: false,
                                    proxies: [],
                                    health_check_interval: 300,
                                    auto_failover: true,
                                    strategy: 'priority'
                                }}
                                onChange={(newConfig, silent = false) => {
                                    const updatedFormData = {
                                        ...formData,
                                        proxy: {
                                            ...formData.proxy,
                                            proxy_pool: newConfig
                                        }
                                    };
                                    setFormData(updatedFormData);

                                    // [FIX] Silent updates (like health polling) should NOT trigger saveConfig
                                    // to prevent race conditions where old memory state rolls back new manual changes
                                    if (silent) {
                                        console.log('Proxy status sync (silent)');
                                        return;
                                    }

                                    // Hot reload: save immediately for manual changes
                                    saveConfig({ ...updatedFormData, auto_refresh: true })
                                        .then(() => {
                                            console.log('Proxy config saved');
                                        })
                                        .catch(err => console.error('Save failed:', err));
                                }}
                            />

                            {/* [FIX #1701] 恢复全局上游代理设置 */}
                            <div className="group bg-white dark:bg-base-100 rounded-xl p-5 border border-gray-100 dark:border-base-200 hover:border-blue-200 transition-all duration-300 shadow-sm relative overflow-hidden">
                                <div className="absolute top-0 right-0 w-24 h-24 bg-blue-500/5 -mr-12 -mt-12 rounded-full blur-2xl group-hover:bg-blue-500/10 transition-colors"></div>
                                <div className="flex items-center justify-between mb-5 relative z-10">
                                    <div className="flex items-center gap-4">
                                        <div className="w-10 h-10 rounded-xl bg-blue-50 dark:bg-blue-900/20 flex items-center justify-center text-blue-500 group-hover:bg-blue-500 group-hover:text-white transition-all duration-300 shadow-sm">
                                            <Globe size={18} />
                                        </div>
                                        <div>
                                            <div className="font-bold text-gray-900 dark:text-gray-100 text-sm">{t('proxy.config.upstream_proxy.title')}</div>
                                            <p className="text-[11px] text-gray-500 dark:text-gray-400 mt-0.5 leading-tight max-w-[280px]">
                                                {t('proxy.config.upstream_proxy.desc_short')}
                                            </p>
                                        </div>
                                    </div>
                                    <label className="relative inline-flex items-center cursor-pointer scale-90">
                                        <input
                                            type="checkbox"
                                            className="sr-only peer"
                                            checked={formData.proxy?.upstream_proxy?.enabled ?? false}
                                            onChange={(e) => setFormData({
                                                ...formData,
                                                proxy: {
                                                    ...formData.proxy,
                                                    upstream_proxy: {
                                                        ...formData.proxy?.upstream_proxy,
                                                        enabled: e.target.checked
                                                    }
                                                }
                                            })}
                                        />
                                        <div className="w-11 h-6 bg-gray-200 dark:bg-base-300 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-500 shadow-inner"></div>
                                    </label>
                                </div>

                                {formData.proxy?.upstream_proxy?.enabled && (
                                    <div className="space-y-4 animate-in slide-in-from-top-2 duration-300 relative z-10">
                                        <div className="pt-4 border-t border-gray-50 dark:border-base-300">
                                            <label className="block text-[10px] font-bold text-gray-400 dark:text-gray-500 uppercase tracking-widest mb-2.5">
                                                {t('proxy.config.upstream_proxy.url')}
                                            </label>
                                            <div className="relative group/input">
                                                <input
                                                    type="text"
                                                    className="w-full px-4 py-2.5 bg-gray-50 dark:bg-base-200 border border-gray-100 dark:border-base-300 rounded-xl focus:ring-2 focus:ring-blue-500/20 focus:border-blue-500 outline-none text-sm font-medium transition-all shadow-inner"
                                                    placeholder={t('proxy.config.upstream_proxy.url_placeholder')}
                                                    value={formData.proxy?.upstream_proxy?.url || ''}
                                                    onChange={(e) => setFormData({
                                                        ...formData,
                                                        proxy: {
                                                            ...formData.proxy,
                                                            upstream_proxy: {
                                                                ...formData.proxy?.upstream_proxy,
                                                                url: e.target.value
                                                            }
                                                        }
                                                    })}
                                                />
                                            </div>
                                            <div className="mt-4 bg-amber-50/40 dark:bg-amber-900/10 rounded-xl p-3.5 border border-amber-100/50 dark:border-amber-800/20 text-[11px] text-amber-700 dark:text-amber-400 flex items-start gap-3 transition-colors hover:bg-amber-50/60">
                                                <div className="mt-0.5 p-1 bg-amber-100/80 dark:bg-amber-800/40 rounded-lg shadow-sm">
                                                    <Network size={12} className="text-amber-600 dark:text-amber-400" />
                                                </div>
                                                <div className="leading-relaxed">
                                                    <span className="font-bold mr-1.5 opacity-80 uppercase tracking-tighter">Tip:</span>
                                                    {t('proxy.config.upstream_proxy.socks5h_hint')}
                                                </div>
                                            </div>
                                        </div>
                                    </div>
                                )}
                            </div>
                        </div>
                    )}

                    {activeTab === 'about' && (
                        <div className="flex flex-col h-full animate-in fade-in duration-500">
                            <div className="flex-1 flex flex-col justify-center items-center space-y-8">
                                {/* Branding Section */}
                                <div className="text-center space-y-4">
                                    <div className="relative inline-block group">
                                        <div className="absolute inset-0 bg-blue-500/20 rounded-3xl blur-xl group-hover:blur-2xl transition-all duration-500"></div>
                                        <img
                                            src="/icon.png"
                                            alt="Antigravity Logo"
                                            className="relative w-24 h-24 rounded-3xl shadow-2xl transform group-hover:scale-105 transition-all duration-500 rotate-3 group-hover:rotate-6 object-cover bg-white dark:bg-black"
                                        />
                                    </div>

                                    <div>
                                        <h3 className="text-3xl font-black text-gray-900 dark:text-base-content tracking-tight mb-2">{t('common.app_name', 'Antigravity Tools')}</h3>
                                        <div className="flex items-center justify-center gap-2 text-sm">
                                            v4.1.25
                                            <span className="text-gray-400 dark:text-gray-600">•</span>
                                            <span className="text-gray-500 dark:text-gray-400">{t('settings.branding.subtitle')}</span>
                                        </div>
                                    </div>
                                </div>

                                {/* Cards Grid - Now 5 columns */}
                                <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-4 w-full max-w-6xl px-4">
                                    {/* Author Card */}
                                    <div className="bg-white dark:bg-base-100 p-4 rounded-2xl border border-gray-100 dark:border-base-300 shadow-sm hover:shadow-md hover:border-blue-200 dark:hover:border-blue-800 transition-all group flex flex-col items-center text-center gap-3">
                                        <div className="p-3 bg-blue-50 dark:bg-blue-900/20 rounded-xl group-hover:scale-110 transition-transform duration-300">
                                            <User className="w-6 h-6 text-blue-500" />
                                        </div>
                                        <div>
                                            <div className="text-xs text-gray-400 uppercase tracking-wider font-semibold mb-1">{t('settings.about.author')}</div>
                                            <div className="font-bold text-gray-900 dark:text-base-content">Ctrler</div>
                                        </div>
                                    </div>

                                    {/* WeChat Card */}
                                    <div className="bg-white dark:bg-base-100 p-4 rounded-2xl border border-gray-100 dark:border-base-300 shadow-sm hover:shadow-md hover:border-green-200 dark:hover:border-green-800 transition-all group flex flex-col items-center text-center gap-3">
                                        <div className="p-3 bg-green-50 dark:bg-green-900/20 rounded-xl group-hover:scale-110 transition-transform duration-300">
                                            <MessageCircle className="w-6 h-6 text-green-500" />
                                        </div>
                                        <div>
                                            <div className="text-xs text-gray-400 uppercase tracking-wider font-semibold mb-1">{t('settings.about.wechat')}</div>
                                            <div className="font-bold text-gray-900 dark:text-base-content">Ctrler</div>
                                        </div>
                                    </div>

                                    {/* Telegram Card */}
                                    <a
                                        href="https://t.me/AntigravityManager"
                                        target="_blank"
                                        rel="noreferrer"
                                        className="bg-white dark:bg-base-100 p-4 rounded-2xl border border-gray-100 dark:border-base-300 shadow-sm hover:shadow-md hover:border-sky-200 dark:hover:border-sky-800 transition-all group flex flex-col items-center text-center gap-3 cursor-pointer"
                                    >
                                        <div className="p-3 bg-sky-50 dark:bg-sky-900/20 rounded-xl group-hover:scale-110 transition-transform duration-300">
                                            <Send className="w-6 h-6 text-sky-500" />
                                        </div>
                                        <div>
                                            <div className="text-xs text-gray-400 uppercase tracking-wider font-semibold mb-1">{t('settings.about.telegram')}</div>
                                            <div className="font-bold text-gray-900 dark:text-base-content whitespace-nowrap overflow-hidden text-ellipsis w-full">Channel</div>
                                        </div>
                                    </a>

                                    {/* GitHub Card */}
                                    <a
                                        href="https://github.com/lbjlaq/Antigravity-Manager"
                                        target="_blank"
                                        rel="noreferrer"
                                        className="bg-white dark:bg-base-100 p-4 rounded-2xl border border-gray-100 dark:border-base-300 shadow-sm hover:shadow-md hover:border-gray-300 dark:hover:border-gray-600 transition-all group flex flex-col items-center text-center gap-3 cursor-pointer"
                                    >
                                        <div className="p-3 bg-gray-50 dark:bg-gray-800 rounded-xl group-hover:scale-110 transition-transform duration-300">
                                            <Github className="w-6 h-6 text-gray-900 dark:text-white" />
                                        </div>
                                        <div>
                                            <div className="text-xs text-gray-400 uppercase tracking-wider font-semibold mb-1">{t('settings.about.github')}</div>
                                            <div className="flex items-center gap-1 font-bold text-gray-900 dark:text-base-content">
                                                <span>{t('settings.about.view_code')}</span>
                                                <ExternalLink className="w-3 h-3 text-gray-400" />
                                            </div>
                                        </div>
                                    </a>

                                    {/* Support Card */}
                                    <div
                                        onClick={() => setIsSupportModalOpen(true)}
                                        className="bg-white dark:bg-base-100 p-4 rounded-2xl border border-gray-100 dark:border-base-300 shadow-sm hover:shadow-md hover:border-pink-200 dark:hover:border-pink-800 transition-all group flex flex-col items-center text-center gap-3 cursor-pointer"
                                    >
                                        <div className="p-3 bg-pink-50 dark:bg-pink-900/20 rounded-xl group-hover:scale-110 transition-transform duration-300">
                                            <Heart className="w-6 h-6 text-pink-500 fill-pink-500" />
                                        </div>
                                        <div>
                                            <div className="text-xs text-gray-400 uppercase tracking-wider font-semibold mb-1">{t('settings.about.support_title')}</div>
                                            <div className="font-bold text-gray-900 dark:text-base-content">{t('settings.about.support_btn')}</div>
                                        </div>
                                    </div>
                                </div>

                                {/* Tech Stack Badges */}
                                <div className="flex gap-2 justify-center">
                                    <div className="px-3 py-1 bg-gray-50 dark:bg-base-200 rounded-lg text-xs font-medium text-gray-500 dark:text-gray-400 border border-gray-100 dark:border-base-300">
                                        Tauri v2
                                    </div>
                                    <div className="px-3 py-1 bg-gray-50 dark:bg-base-200 rounded-lg text-xs font-medium text-gray-500 dark:text-gray-400 border border-gray-100 dark:border-base-300">
                                        React 19
                                    </div>
                                    <div className="px-3 py-1 bg-gray-50 dark:bg-base-200 rounded-lg text-xs font-medium text-gray-500 dark:text-gray-400 border border-gray-100 dark:border-base-300">
                                        TypeScript
                                    </div>
                                </div>

                                {/* Check for Updates */}
                                <div className="flex flex-col items-center gap-3">
                                    <button
                                        onClick={handleCheckUpdate}
                                        disabled={isCheckingUpdate}
                                        className="px-6 py-2.5 bg-blue-500 hover:bg-blue-600 disabled:bg-gray-300 dark:disabled:bg-gray-700 text-white rounded-lg transition-all flex items-center gap-2 shadow-sm hover:shadow-md disabled:cursor-not-allowed"
                                    >
                                        <RefreshCw className={`w-4 h-4 ${isCheckingUpdate ? 'animate-spin' : ''}`} />
                                        {isCheckingUpdate ? t('settings.about.checking_update') : t('settings.about.check_update')}
                                    </button>

                                    {/* Update Status */}
                                    {updateInfo && !isCheckingUpdate && (
                                        <div className="text-center">
                                            {updateInfo.hasUpdate ? (
                                                <div className="flex flex-col items-center gap-2">
                                                    <div className="text-sm text-orange-600 dark:text-orange-400 font-medium">
                                                        {t('settings.about.new_version_available', { version: updateInfo.latestVersion })}
                                                    </div>
                                                    <div className="flex items-center gap-2">
                                                        {isBrewInstalled && (
                                                            <button
                                                                onClick={() => setIsBrewConfirmOpen(true)}
                                                                disabled={isBrewUpgrading}
                                                                className="px-4 py-1.5 bg-green-500 hover:bg-green-600 disabled:bg-gray-300 dark:disabled:bg-gray-700 text-white text-sm rounded-lg transition-colors flex items-center gap-1.5 disabled:cursor-not-allowed"
                                                            >
                                                                {isBrewUpgrading ? (
                                                                    <>
                                                                        <RefreshCw className="w-3.5 h-3.5 animate-spin" />
                                                                        {t('settings.about.brew_upgrading')}
                                                                    </>
                                                                ) : (
                                                                    t('settings.about.brew_upgrade')
                                                                )}
                                                            </button>
                                                        )}
                                                        <a
                                                            href={updateInfo.downloadUrl}
                                                            target="_blank"
                                                            rel="noreferrer"
                                                            className="px-4 py-1.5 bg-orange-500 hover:bg-orange-600 text-white text-sm rounded-lg transition-colors flex items-center gap-1.5"
                                                        >
                                                            {t('settings.about.download_update')}
                                                            <ExternalLink className="w-3.5 h-3.5" />
                                                        </a>
                                                    </div>
                                                </div>
                                            ) : (
                                                <div className="text-sm text-green-600 dark:text-green-400 font-medium">
                                                    ✓ {t('settings.about.latest_version')}
                                                </div>
                                            )}
                                        </div>
                                    )}
                                </div>
                            </div>

                            <div className="text-center text-[10px] text-gray-300 dark:text-gray-600 mt-auto pb-2">
                                {t('settings.about.copyright')}
                            </div>
                        </div>
                    )
                    }
                </div >

                <ModalDialog
                    isOpen={isClearLogsOpen}
                    title={t('settings.advanced.clear_logs_title')}
                    message={t('settings.advanced.clear_logs_msg')}
                    type="confirm"
                    confirmText={t('common.clear')}
                    cancelText={t('common.cancel')}
                    isDestructive={true}
                    onConfirm={confirmClearLogs}
                    onCancel={() => setIsClearLogsOpen(false)}
                />

                {/* Antigravity Cache Clear Modal */}
                <ModalDialog
                    isOpen={isClearCacheOpen}
                    title={t('settings.advanced.clear_cache_confirm_title')}
                    type="confirm"
                    confirmText={isClearingCache ? t('common.clearing') : t('common.clear')}
                    cancelText={t('common.cancel')}
                    isDestructive={true}
                    onConfirm={confirmClearAntigravityCache}
                    onCancel={() => setIsClearCacheOpen(false)}
                >
                    <div className="space-y-3">
                        <p className="text-sm text-gray-600 dark:text-gray-400">
                            {t('settings.advanced.clear_cache_confirm_msg')}
                        </p>
                        {cachePaths.length > 0 ? (
                            <div className="bg-gray-50 dark:bg-base-200 rounded-lg p-3 max-h-40 overflow-y-auto">
                                <ul className="text-xs font-mono text-gray-600 dark:text-gray-400 space-y-1">
                                    {cachePaths.map((path, index) => (
                                        <li key={index} className="truncate">• {path}</li>
                                    ))}
                                </ul>
                            </div>
                        ) : (
                            <div className="bg-gray-50 dark:bg-base-200 rounded-lg p-3">
                                <p className="text-xs text-gray-500 dark:text-gray-400">
                                    {t('settings.advanced.cache_not_found')}
                                </p>
                            </div>
                        )}
                        <div className="bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700/30 rounded-lg p-2">
                            <p className="text-xs text-amber-700 dark:text-amber-400">
                                {t('settings.advanced.antigravity_cache_warning')}
                            </p>
                        </div>
                    </div>
                </ModalDialog>

                {/* Homebrew Upgrade Confirm Modal */}
                <ModalDialog
                    isOpen={isBrewConfirmOpen}
                    title={t('settings.about.brew_confirm_title')}
                    type="confirm"
                    confirmText={t('settings.about.brew_confirm_btn')}
                    cancelText={t('common.cancel')}
                    onConfirm={handleBrewUpgrade}
                    onCancel={() => setIsBrewConfirmOpen(false)}
                >
                    <div className="space-y-3">
                        <p className="text-sm text-gray-600 dark:text-gray-400">
                            {t('settings.about.brew_confirm_desc')}
                        </p>
                        <div className="bg-gray-50 dark:bg-base-200 rounded-lg p-3">
                            <div className="flex items-center justify-between gap-2">
                                <code className="text-xs text-gray-700 dark:text-gray-300 break-all">brew upgrade --cask antigravity-tools</code>
                                <button
                                    className="shrink-0 px-2 py-1 text-xs text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200 border border-gray-200 dark:border-base-300 rounded hover:bg-gray-100 dark:hover:bg-base-300 transition-colors"
                                    onClick={() => {
                                        navigator.clipboard.writeText('brew upgrade --cask antigravity-tools');
                                        showToast(t('common.copied', 'Copied'), 'success');
                                    }}
                                >
                                    {t('common.copy', 'Copy')}
                                </button>
                            </div>
                        </div>
                        <div className="bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700/30 rounded-lg p-3">
                            <p className="text-xs text-amber-700 dark:text-amber-400 mb-2">{t('settings.about.brew_quarantine_hint')}</p>
                            <div className="flex items-center justify-between gap-2">
                                <code className="text-xs text-amber-800 dark:text-amber-300 break-all">sudo xattr -rd com.apple.quarantine "/Applications/Antigravity Tools.app"</code>
                                <button
                                    className="shrink-0 px-2 py-1 text-xs text-amber-600 hover:text-amber-800 dark:text-amber-400 dark:hover:text-amber-200 border border-amber-200 dark:border-amber-700 rounded hover:bg-amber-100 dark:hover:bg-amber-900/30 transition-colors"
                                    onClick={() => {
                                        navigator.clipboard.writeText('sudo xattr -rd com.apple.quarantine "/Applications/Antigravity Tools.app"');
                                        showToast(t('common.copied', 'Copied'), 'success');
                                    }}
                                >
                                    {t('common.copy', 'Copy')}
                                </button>
                            </div>
                        </div>
                    </div>
                </ModalDialog>

                {/* Homebrew Upgrade Success Modal */}
                <ModalDialog
                    isOpen={isBrewSuccessOpen}
                    title={t('settings.about.brew_success_title')}
                    type="success"
                    confirmText={t('settings.about.brew_restart_btn')}
                    onConfirm={async () => {
                        try {
                            await relaunch();
                        } catch {
                            setIsBrewSuccessOpen(false);
                            showToast(t('settings.about.brew_restart_failed'), 'error');
                        }
                    }}
                >
                    <p className="text-sm text-gray-600 dark:text-gray-400">
                        {t('settings.about.brew_upgrade_success')}
                    </p>
                </ModalDialog>

                {/* Support Modal */}
                <div className={`modal ${isSupportModalOpen ? 'modal-open' : ''} z-[100]`}>
                    <div data-tauri-drag-region className="fixed top-0 left-0 right-0 h-8 z-[110]" />
                    <div className="modal-box relative max-w-2xl bg-white dark:bg-base-100 shadow-2xl rounded-3xl p-0 overflow-hidden transform transition-all animate-in fade-in zoom-in-95 duration-300">
                        <div className="flex flex-col items-center p-8">
                            <div className="w-16 h-16 bg-pink-50 dark:bg-pink-900/20 rounded-2xl flex items-center justify-center mb-6 shadow-sm">
                                <Coffee className="w-8 h-8 text-pink-500" />
                            </div>

                            <h3 className="text-2xl font-black text-gray-900 dark:text-base-content mb-3">{t('settings.about.support_title')}</h3>
                            <p className="text-gray-500 dark:text-gray-400 text-sm text-center mb-8 max-w-md leading-relaxed">
                                {t('settings.about.support_desc')}
                            </p>

                            {/* QR Codes Grid */}
                            <div className="grid grid-cols-1 md:grid-cols-3 gap-6 w-full mb-8">
                                {/* Alipay */}
                                <div className="flex flex-col items-center gap-3 p-4 rounded-2xl bg-gray-50 dark:bg-base-200 border border-gray-100 dark:border-base-300">
                                    <div className="w-full aspect-square relative bg-white rounded-xl overflow-hidden shadow-sm border border-gray-100">
                                        <img src="/images/donate/alipay.png" alt="Alipay" className="w-full h-full object-contain" />
                                    </div>
                                    <span className="text-xs font-bold text-gray-700 dark:text-gray-300">{t('settings.about.support_alipay')}</span>
                                </div>

                                {/* WeChat */}
                                <div className="flex flex-col items-center gap-3 p-4 rounded-2xl bg-gray-50 dark:bg-base-200 border border-gray-100 dark:border-base-300">
                                    <div className="w-full aspect-square relative bg-white rounded-xl overflow-hidden shadow-sm border border-gray-100">
                                        <img src="/images/donate/wechat.png" alt="WeChat" className="w-full h-full object-contain" />
                                    </div>
                                    <span className="text-xs font-bold text-gray-700 dark:text-gray-300">{t('settings.about.support_wechat')}</span>
                                </div>

                                {/* Buy Me a Coffee */}
                                <div className="flex flex-col items-center gap-3 p-4 rounded-2xl bg-gray-50 dark:bg-base-200 border border-gray-100 dark:border-base-300">
                                    <div className="w-full aspect-square relative bg-white rounded-xl overflow-hidden shadow-sm border border-gray-100">
                                        <img src="/images/donate/coffee.png" alt="Buy Me A Coffee" className="w-full h-full object-contain" />
                                    </div>
                                    <span className="text-xs font-bold text-gray-700 dark:text-gray-300">{t('settings.about.support_buymeacoffee')}</span>
                                </div>
                            </div>

                            <button
                                onClick={() => setIsSupportModalOpen(false)}
                                className="w-full md:w-auto px-12 py-3 bg-gray-100 dark:bg-base-300 text-gray-700 dark:text-gray-200 font-bold rounded-xl hover:bg-gray-200 dark:hover:bg-base-200 transition-all"
                            >
                                {t('common.close') || 'Close'}
                            </button>
                        </div>
                    </div>
                    <div className="modal-backdrop bg-black/60 backdrop-blur-md fixed inset-0 z-[-1]" onClick={() => setIsSupportModalOpen(false)}></div>
                </div>
            </div >
        </div >
    );
}

export default Settings;
