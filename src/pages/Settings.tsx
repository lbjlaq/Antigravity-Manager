import { useState, useEffect } from 'react';
import { Save, Github, User, MessageCircle, ExternalLink, RefreshCw, Sparkles } from 'lucide-react';
import { request as invoke } from '../utils/request';
import { open } from '@tauri-apps/plugin-dialog';
import { useConfigStore } from '../stores/useConfigStore';
import { AppConfig } from '../types/config';
import ModalDialog from '../components/common/ModalDialog';
import { showToast } from '../components/common/ToastContainer';

import { useTranslation } from 'react-i18next';

function Settings() {
    const { t } = useTranslation();
    const { config, loadConfig, saveConfig } = useConfigStore();
    const [activeTab, setActiveTab] = useState<'general' | 'account' | 'proxy' | 'advanced' | 'about'>('general');
    const [formData, setFormData] = useState<AppConfig>({
        language: 'zh',
        theme: 'system',
        auto_refresh: false,
        refresh_interval: 15,
        auto_sync: false,
        sync_interval: 5,
        scheduled_warmup: {
            enabled: false,
            schedules: {
                'default': [
                    { start: '10:00', end: '10:00', enabled: true },
                    { start: '15:00', end: '15:00', enabled: true },
                    { start: '21:00', end: '21:00', enabled: true }
                ]
            }
        },
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
            }
        }
    });

    // Dialog state
    // Dialog state
    const [isClearLogsOpen, setIsClearLogsOpen] = useState(false);
    const [dataDirPath, setDataDirPath] = useState<string>('~/.antigravity_tools/');

    // Update check state
    const [isCheckingUpdate, setIsCheckingUpdate] = useState(false);
    const [updateInfo, setUpdateInfo] = useState<{
        hasUpdate: boolean;
        latestVersion: string;
        currentVersion: string;
        downloadUrl: string;
    } | null>(null);

    useEffect(() => {
        loadConfig();

        // 获取真实数据目录路径
        invoke<string>('get_data_dir_path')
            .then(path => setDataDirPath(path))
            .catch(err => console.error('Failed to get data dir:', err));
    }, [loadConfig]);

    useEffect(() => {
        if (config) {
            setFormData(config);
        }
    }, [config]);

    const handleSave = async () => {
        try {
            await saveConfig(formData);
            showToast(t('common.saved'), 'success');
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


    const handleDetectAntigravityPath = async () => {
        try {
            const path = await invoke<string>('get_antigravity_path', { bypassConfig: true });
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
            }>('check_for_updates');

            setUpdateInfo({
                hasUpdate: result.has_update,
                latestVersion: result.latest_version,
                currentVersion: result.current_version,
                downloadUrl: result.download_url,
            });

            if (result.has_update) {
                showToast(t('settings.about.new_version_available', { version: result.latest_version }), 'info');
            } else {
                showToast(t('settings.about.latest_version'), 'success');
            }
        } catch (error) {
            showToast(`${t('settings.about.update_check_failed')}: ${error}`, 'error');
        } finally {
            setIsCheckingUpdate(false);
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
                                    onChange={(e) => setFormData({ ...formData, language: e.target.value })}
                                >
                                    <option value="zh">简体中文</option>
                                    <option value="en">English</option>
                                </select>
                            </div>

                            {/* 主题选择 */}
                            <div>
                                <label className="block text-sm font-medium text-gray-900 dark:text-base-content mb-2">{t('settings.general.theme')}</label>
                                <select
                                    className="w-full px-4 py-4 border border-gray-200 dark:border-base-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent text-gray-900 dark:text-base-content bg-gray-50 dark:bg-base-200"
                                    value={formData.theme}
                                    onChange={(e) => setFormData({ ...formData, theme: e.target.value })}
                                >
                                    <option value="light">{t('settings.general.theme_light')}</option>
                                    <option value="dark">{t('settings.general.theme_dark')}</option>
                                    <option value="system">{t('settings.general.theme_system')}</option>
                                </select>
                            </div>

                            {/* 开机自动启动 */}
                            <div>
                                <label className="block text-sm font-medium text-gray-900 dark:text-base-content mb-2">{t('settings.general.auto_launch')}</label>
                                <select
                                    className="w-full px-4 py-4 border border-gray-200 dark:border-base-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent text-gray-900 dark:text-base-content bg-gray-50 dark:bg-base-200"
                                    value={formData.auto_launch ? 'enabled' : 'disabled'}
                                    onChange={async (e) => {
                                        const enabled = e.target.value === 'enabled';
                                        try {
                                            await invoke('toggle_auto_launch', { enable: enabled });
                                            setFormData({ ...formData, auto_launch: enabled });
                                            showToast(enabled ? '已启用开机自动启动' : '已禁用开机自动启动', 'success');
                                        } catch (error) {
                                            showToast(`${t('common.error')}: ${error}`, 'error');
                                        }
                                    }}
                                >
                                    <option value="disabled">{t('settings.general.auto_launch_disabled')}</option>
                                    <option value="enabled">{t('settings.general.auto_launch_enabled')}</option>
                                </select>
                                <p className="text-sm text-gray-500 dark:text-gray-400 mt-2">{t('settings.general.auto_launch_desc')}</p>
                            </div>
                        </div>
                    )}

                    {/* 账号设置 */}
                    {activeTab === 'account' && (
                        <div className="space-y-6">
                            <h2 className="text-lg font-semibold text-gray-900 dark:text-base-content">{t('settings.account.title')}</h2>

                            {/* 自动刷新配额 */}
                            <div className="flex items-center justify-between p-4 bg-gray-50 dark:bg-base-200 rounded-lg border border-gray-100 dark:border-base-300">
                                <div>
                                    <div className="font-medium text-gray-900 dark:text-base-content">{t('settings.account.auto_refresh')}</div>
                                    <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">{t('settings.account.auto_refresh_desc')}</p>
                                </div>
                                <label className="relative inline-flex items-center cursor-pointer">
                                    <input
                                        type="checkbox"
                                        className="sr-only peer"
                                        checked={formData.auto_refresh}
                                        onChange={(e) => setFormData({ ...formData, auto_refresh: e.target.checked })}
                                    />
                                    <div className="w-11 h-6 bg-gray-200 dark:bg-base-300 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-blue-300 dark:peer-focus:ring-blue-800 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-500"></div>
                                </label>
                            </div>

                            {/* 刷新间隔 */}
                            {formData.auto_refresh && (
                                <div className="ml-4">
                                    <label className="block text-sm font-medium text-gray-900 dark:text-base-content mb-2">{t('settings.account.refresh_interval')}</label>
                                    <input
                                        type="number"
                                        className="w-32 px-4 py-4 border border-gray-200 dark:border-base-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent text-gray-900 dark:text-base-content bg-gray-50 dark:bg-base-200"
                                        min="1"
                                        max="60"
                                        value={formData.refresh_interval}
                                        onChange={(e) => setFormData({ ...formData, refresh_interval: parseInt(e.target.value) })}
                                    />
                                </div>
                            )}

                            {/* 自动获取当前账号 */}
                            <div className="flex items-center justify-between p-4 bg-gray-50 dark:bg-base-200 rounded-lg border border-gray-100 dark:border-base-300">
                                <div>
                                    <div className="font-medium text-gray-900 dark:text-base-content">{t('settings.account.auto_sync')}</div>
                                    <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">{t('settings.account.auto_sync_desc')}</p>
                                </div>
                                <label className="relative inline-flex items-center cursor-pointer">
                                    <input
                                        type="checkbox"
                                        className="sr-only peer"
                                        checked={formData.auto_sync}
                                        onChange={(e) => setFormData({ ...formData, auto_sync: e.target.checked })}
                                    />
                                    <div className="w-11 h-6 bg-gray-200 dark:bg-base-300 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-blue-300 dark:peer-focus:ring-blue-800 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-500"></div>
                                </label>
                            </div>

                            {/* 同步间隔 */}
                            {formData.auto_sync && (
                                <div className="ml-4">
                                    <label className="block text-sm font-medium text-gray-900 dark:text-base-content mb-2">{t('settings.account.sync_interval')}</label>
                                    <input
                                        type="number"
                                        className="w-32 px-4 py-4 border border-gray-200 dark:border-base-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent text-gray-900 dark:text-base-content bg-gray-50 dark:bg-base-200"
                                        min="1"
                                        max="60"
                                        value={formData.sync_interval}
                                        onChange={(e) => setFormData({ ...formData, sync_interval: parseInt(e.target.value) })}
                                    />
                                </div>
                            )}

                            {/* 智能高峰期预热调度器 */}
                            <div className="mt-6 pt-6 border-t border-gray-200 dark:border-base-300">
                                <h3 className="text-md font-semibold text-gray-900 dark:text-base-content mb-4 flex items-center gap-2">
                                    <Sparkles className="w-5 h-5 text-amber-500" />
                                    {t('settings.account.scheduled_warmup.title')}
                                </h3>

                                {/* 启用开关 */}
                                <div className="flex items-center justify-between p-4 bg-gray-50 dark:bg-base-200 rounded-lg border border-gray-100 dark:border-base-300 mb-4">
                                    <div>
                                        <div className="font-medium text-gray-900 dark:text-base-content">{t('settings.account.scheduled_warmup.enable')}</div>
                                        <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">{t('settings.account.scheduled_warmup.enable_desc')}</p>
                                    </div>
                                    <label className="relative inline-flex items-center cursor-pointer">
                                        <input
                                            type="checkbox"
                                            className="sr-only peer"
                                            checked={formData.scheduled_warmup?.enabled || false}
                                            onChange={(e) => {
                                                const defaultSchedules = {
                                                    'default': [
                                                        { start: '10:00', end: '10:00', enabled: true },
                                                        { start: '15:00', end: '15:00', enabled: true },
                                                        { start: '21:00', end: '21:00', enabled: true }
                                                    ]
                                                };
                                                const existingSchedules = formData.scheduled_warmup?.schedules;
                                                const hasExistingSchedules = existingSchedules &&
                                                    existingSchedules['default'] &&
                                                    existingSchedules['default'].length > 0 &&
                                                    existingSchedules['default'].some(s => s.start);
                                                setFormData({
                                                    ...formData,
                                                    scheduled_warmup: {
                                                        ...formData.scheduled_warmup,
                                                        enabled: e.target.checked,
                                                        schedules: hasExistingSchedules ? existingSchedules : defaultSchedules
                                                    }
                                                });
                                            }}
                                        />
                                        <div className="w-11 h-6 bg-gray-200 dark:bg-base-300 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-blue-300 dark:peer-focus:ring-blue-800 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-amber-500"></div>
                                    </label>
                                </div>

                                {/* 时间点配置 */}
                                {formData.scheduled_warmup?.enabled && (
                                    <div className="ml-4 space-y-4">
                                        <p className="text-sm text-gray-600 dark:text-gray-400">
                                            {t('settings.account.scheduled_warmup.schedule_desc')}
                                        </p>

                                        {/* 3个时间点输入 */}
                                        <div className="space-y-3">
                                            {[0, 1, 2].map((idx) => {
                                                // 获取当前时间点 (使用schedules的第一天作为通用配置)
                                                const times = formData.scheduled_warmup?.schedules?.['default'] || [];
                                                const currentTimeRange = times[idx];
                                                const currentTime = currentTimeRange?.start || '';
                                                const isTimeEnabled = currentTimeRange?.enabled !== false; // 默认为 true

                                                // 计算预热触发时间（高峰时间前5小时）
                                                const getWarmupTime = (peakTime: string) => {
                                                    if (!peakTime) return '--:--';
                                                    const [h, m] = peakTime.split(':').map(Number);
                                                    let warmupHour = h - 5;
                                                    if (warmupHour < 0) warmupHour += 24;
                                                    return `${warmupHour.toString().padStart(2, '0')}:${m.toString().padStart(2, '0')}`;
                                                };

                                                // 更新时间点配置
                                                const updateTimePoint = (updates: { start?: string; enabled?: boolean }) => {
                                                    const newTimes = [...(formData.scheduled_warmup?.schedules?.['default'] || [
                                                        { start: '10:00', end: '10:00', enabled: true },
                                                        { start: '15:00', end: '15:00', enabled: true },
                                                        { start: '21:00', end: '21:00', enabled: true }
                                                    ])];
                                                    newTimes[idx] = {
                                                        ...newTimes[idx],
                                                        start: updates.start ?? newTimes[idx]?.start ?? '',
                                                        end: updates.start ?? newTimes[idx]?.end ?? '',
                                                        enabled: updates.enabled ?? newTimes[idx]?.enabled ?? true
                                                    };
                                                    setFormData({
                                                        ...formData,
                                                        scheduled_warmup: {
                                                            enabled: true,
                                                            schedules: { 'default': newTimes }
                                                        }
                                                    });
                                                };

                                                return (
                                                    <div key={idx} className={`flex items-center gap-4 p-3 rounded-lg transition-all ${isTimeEnabled ? 'bg-gray-50 dark:bg-base-200' : 'bg-gray-100/50 dark:bg-base-300/50 opacity-60'}`}>
                                                        {/* 独立开关 */}
                                                        <label className="relative inline-flex items-center cursor-pointer shrink-0">
                                                            <input
                                                                type="checkbox"
                                                                className="sr-only peer"
                                                                checked={isTimeEnabled}
                                                                onChange={(e) => updateTimePoint({ enabled: e.target.checked })}
                                                            />
                                                            <div className="w-9 h-5 bg-gray-200 dark:bg-base-300 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-emerald-500"></div>
                                                        </label>
                                                        <span className="text-sm font-medium text-gray-700 dark:text-gray-300 w-20">
                                                            {t('settings.account.scheduled_warmup.peak_time')} {idx + 1}
                                                        </span>
                                                        <input
                                                            type="time"
                                                            className={`px-3 py-2 border border-gray-300 dark:border-base-300 rounded-lg bg-white dark:bg-base-100 text-sm ${!isTimeEnabled ? 'opacity-50 cursor-not-allowed' : ''}`}
                                                            value={currentTime}
                                                            disabled={!isTimeEnabled}
                                                            onChange={(e) => updateTimePoint({ start: e.target.value })}
                                                        />
                                                        {currentTime && isTimeEnabled && (
                                                            <span className="text-xs text-amber-600 dark:text-amber-400 flex items-center gap-1">
                                                                ⚡ {t('settings.account.scheduled_warmup.warmup_at')} <strong>{getWarmupTime(currentTime)}</strong> {t('settings.account.scheduled_warmup.warmup_suffix')}
                                                            </span>
                                                        )}
                                                        {(!currentTime || !isTimeEnabled) && (
                                                            <span className="text-xs text-gray-400">
                                                                {!isTimeEnabled ? t('common.disabled') : t('settings.account.scheduled_warmup.not_set')}
                                                            </span>
                                                        )}
                                                    </div>
                                                );
                                            })}
                                        </div>

                                        <div className="mt-3 p-3 bg-amber-50 dark:bg-amber-900/20 rounded-lg border border-amber-200 dark:border-amber-800/50">
                                            <p className="text-xs text-amber-700 dark:text-amber-400">
                                                ⚠️ {t('settings.account.scheduled_warmup.quota_hint')}
                                            </p>
                                        </div>

                                        <p className="text-xs text-gray-500 dark:text-gray-400 mt-2">
                                            💡 {t('settings.account.scheduled_warmup.hint')}
                                        </p>
                                    </div>
                                )}
                            </div>
                        </div>
                    )}

                    {/* 高级设置 */}
                    {activeTab === 'advanced' && (
                        <div className="space-y-4">
                            <h2 className="text-lg font-semibold text-gray-900 dark:text-base-content">{t('settings.advanced.title')}</h2>

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
                                    <button
                                        className="px-4 py-2 border border-gray-200 dark:border-base-300 text-gray-700 dark:text-gray-300 rounded-lg hover:bg-gray-50 dark:hover:bg-base-200 hover:text-gray-900 dark:hover:text-base-content transition-colors"
                                        onClick={handleSelectExportPath}
                                    >
                                        {t('settings.advanced.select_btn')}
                                    </button>
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
                                    <button
                                        className="px-4 py-2 border border-gray-200 dark:border-base-300 text-gray-700 dark:text-gray-300 rounded-lg hover:bg-gray-50 dark:hover:bg-base-200 hover:text-gray-900 dark:hover:text-base-content transition-colors"
                                        onClick={handleOpenDataDir}
                                    >
                                        {t('settings.advanced.open_btn')}
                                    </button>
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
                                    <button
                                        className="px-4 py-2 border border-gray-200 dark:border-base-300 text-gray-700 dark:text-gray-300 rounded-lg hover:bg-gray-50 dark:hover:bg-base-200 transition-colors"
                                        onClick={handleSelectAntigravityPath}
                                    >
                                        {t('settings.advanced.select_btn')}
                                    </button>
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
                                        className="px-4 py-2 border border-gray-200 dark:border-base-300 text-gray-700 dark:text-gray-300 rounded-lg hover:bg-gray-50 dark:hover:bg-base-200 transition-colors"
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

                            <div className="border-t border-gray-200 dark:border-base-200 pt-4">
                                <h3 className="font-medium text-gray-900 dark:text-base-content mb-3">{t('settings.advanced.logs_title')}</h3>
                                <div className="bg-gray-50 dark:bg-base-200 border border-gray-200 dark:border-base-300 rounded-lg p-3 mb-3">
                                    <p className="text-sm text-gray-600 dark:text-gray-400">{t('settings.advanced.logs_desc')}</p>
                                </div>
                                <div className="badge badge-primary badge-outline gap-2 font-mono">
                                    v3.3.15
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
                        </div>
                    )}

                    {/* 代理设置 */}
                    {activeTab === 'proxy' && (
                        <div className="space-y-6">
                            <h2 className="text-lg font-semibold text-gray-900 dark:text-base-content">{t('settings.proxy.title')}</h2>

                            <div className="p-4 bg-gray-50 dark:bg-base-200 rounded-lg border border-gray-100 dark:border-base-300">
                                <h3 className="text-md font-semibold text-gray-900 dark:text-base-content mb-3 flex items-center gap-2">
                                    <Sparkles size={18} className="text-blue-500" />
                                    {t('proxy.config.upstream_proxy.title')}
                                </h3>
                                <p className="text-sm text-gray-600 dark:text-gray-400 mb-4">
                                    {t('proxy.config.upstream_proxy.desc')}
                                </p>

                                <div className="space-y-4">
                                    <div className="flex items-center">
                                        <label className="flex items-center cursor-pointer gap-3">
                                            <div className="relative">
                                                <input
                                                    type="checkbox"
                                                    className="sr-only"
                                                    checked={formData.proxy?.upstream_proxy?.enabled || false}
                                                    onChange={(e) => setFormData({
                                                        ...formData,
                                                        proxy: {
                                                            ...formData.proxy,
                                                            upstream_proxy: {
                                                                ...formData.proxy.upstream_proxy,
                                                                enabled: e.target.checked
                                                            }
                                                        }
                                                    })}
                                                />
                                                <div className={`block w-14 h-8 rounded-full transition-colors ${formData.proxy?.upstream_proxy?.enabled ? 'bg-blue-500' : 'bg-gray-300 dark:bg-base-300'}`}></div>
                                                <div className={`dot absolute left-1 top-1 bg-white w-6 h-6 rounded-full transition-transform ${formData.proxy?.upstream_proxy?.enabled ? 'transform translate-x-6' : ''}`}></div>
                                            </div>
                                            <span className="text-sm font-medium text-gray-900 dark:text-base-content">
                                                {t('proxy.config.upstream_proxy.enable')}
                                            </span>
                                        </label>
                                    </div>

                                    <div>
                                        <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                                            {t('proxy.config.upstream_proxy.url')}
                                        </label>
                                        <input
                                            type="text"
                                            value={formData.proxy?.upstream_proxy?.url || ''}
                                            onChange={(e) => setFormData({
                                                ...formData,
                                                proxy: {
                                                    ...formData.proxy,
                                                    upstream_proxy: {
                                                        ...formData.proxy.upstream_proxy,
                                                        url: e.target.value
                                                    }
                                                }
                                            })}
                                            placeholder={t('proxy.config.upstream_proxy.url_placeholder')}
                                            className="w-full px-4 py-4 border border-gray-200 dark:border-base-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent text-gray-900 dark:text-base-content bg-gray-50 dark:bg-base-200"
                                        />
                                        <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                                            {t('proxy.config.upstream_proxy.tip')}
                                        </p>
                                    </div>
                                </div>
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
                                        <h3 className="text-3xl font-black text-gray-900 dark:text-base-content tracking-tight mb-2">Antigravity Tools</h3>
                                        <div className="flex items-center justify-center gap-2 text-sm">
                                            <span className="px-2.5 py-0.5 rounded-full bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 font-medium border border-blue-200 dark:border-blue-800">
                                                v3.3.15
                                            </span>
                                            <span className="text-gray-400 dark:text-gray-600">•</span>
                                            <span className="text-gray-500 dark:text-gray-400">Professional Account Management</span>
                                        </div>
                                    </div>
                                </div>

                                {/* Cards Grid - Now 3 columns */}
                                <div className="grid grid-cols-1 md:grid-cols-3 gap-4 w-full max-w-3xl px-4">
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
                    )}
                </div>

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
            </div>
        </div>
    );
}

export default Settings;
