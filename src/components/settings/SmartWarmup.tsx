import React from 'react';
import { useTranslation } from 'react-i18next';
import { Sparkles, Check, Clock, Zap, ShieldCheck } from 'lucide-react';
import { ScheduledWarmupConfig } from '../../types/config';
import { MODEL_CONFIG } from '../../config/modelConfig';

interface SmartWarmupProps {
    config: ScheduledWarmupConfig;
    onChange: (config: ScheduledWarmupConfig) => void;
}

const SmartWarmup: React.FC<SmartWarmupProps> = ({ config, onChange }) => {
    const { t } = useTranslation();

    const currentMode = config.mode || 'smart';
    const currentInterval = config.interval_minutes || 120;

    const uniqueLabels = new Set<string>();
    const warmupModelsOptions = Object.entries(MODEL_CONFIG)
        .filter(([id, config]) => {
            if (id.includes('thinking')) return false;
            const label = config.shortLabel || config.label;
            if (uniqueLabels.has(label)) return false;
            uniqueLabels.add(label);
            return true;
        })
        .map(([id, config]) => ({
            id,
            label: config.shortLabel || config.label
        }));

    const handleEnabledChange = (enabled: boolean) => {
        let newConfig: ScheduledWarmupConfig = {
            ...config,
            enabled,
            mode: config.mode || 'smart',
            interval_minutes: config.interval_minutes || 120,
        };
        // 如果开启预热且勾选列表为空，则默认勾选所有核心模型
        if (enabled && (!config.monitored_models || config.monitored_models.length === 0)) {
            newConfig.monitored_models = warmupModelsOptions.map(o => o.id);
        }
        onChange(newConfig);
    };

    const handleModeChange = (mode: 'smart' | 'timer' | 'quota_full') => {
        onChange({ ...config, mode });
    };

    const handleIntervalChange = (minutes: number) => {
        const interval_minutes = Math.max(5, Math.min(minutes, 1440));
        onChange({ ...config, interval_minutes });
    };

    const toggleModel = (model: string) => {
        const currentModels = config.monitored_models || [];
        let newModels: string[];

        if (currentModels.includes(model)) {
            // 必须勾选其中一个，不能全取消
            if (currentModels.length <= 1) return;
            newModels = currentModels.filter(m => m !== model);
        } else {
            newModels = [...currentModels, model];
        }

        onChange({ ...config, monitored_models: newModels });
    };

    const intervalPresets = [
        { label: t('settings.warmup.interval_30m', '30 分钟'), value: 30 },
        { label: t('settings.warmup.interval_1h', '1 小时'), value: 60 },
        { label: t('settings.warmup.interval_2h', '2 小时 (推荐)'), value: 120 },
        { label: t('settings.warmup.interval_4h', '4 小时'), value: 240 },
        { label: t('settings.warmup.interval_8h', '8 小时'), value: 480 },
    ];

    return (
        <div className="space-y-4">
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-4">
                    <div className={`w-10 h-10 rounded-xl flex items-center justify-center transition-all duration-300 ${config.enabled
                        ? 'bg-orange-500 text-white shadow-lg shadow-orange-500/20'
                        : 'bg-orange-50 dark:bg-orange-900/20 text-orange-500 group-hover:bg-orange-500 group-hover:text-white'
                        }`}>
                        <Sparkles size={20} />
                    </div>
                    <div>
                        <div className="font-bold text-gray-900 dark:text-gray-100 flex items-center gap-2">
                            {t('settings.warmup.title', '自动预热与定时预热')}
                            {config.enabled && (
                                <span className="text-[10px] px-2 py-0.5 rounded-full bg-orange-100 dark:bg-orange-900/40 text-orange-600 dark:text-orange-300 font-semibold">
                                    {currentMode === 'timer'
                                        ? t('settings.warmup.mode_timer', '自动计时预热')
                                        : currentMode === 'quota_full'
                                            ? t('settings.warmup.mode_quota_full', '满额度恢复预热')
                                            : t('settings.warmup.mode_smart', '混合智能')}
                                </span>
                            )}
                        </div>
                        <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                            {t('settings.warmup.desc', '自动监控与管理模型预热，保持模型热状态，降低首包延迟')}
                        </p>
                    </div>
                </div>
                <label className="relative inline-flex items-center cursor-pointer">
                    <input
                        type="checkbox"
                        className="sr-only peer"
                        checked={config.enabled}
                        onChange={(e) => handleEnabledChange(e.target.checked)}
                    />
                    <div className="w-11 h-6 bg-gray-200 dark:bg-base-300 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-orange-500 shadow-inner"></div>
                </label>
            </div>

            {config.enabled && (
                <div className="mt-4 pt-4 border-t border-gray-100 dark:border-base-300 animate-in slide-in-from-top-2 duration-300 space-y-4">
                    {/* 预热模式选择 */}
                    <div>
                        <label className="text-[10px] font-bold text-gray-400 dark:text-gray-500 uppercase tracking-widest block mb-2">
                            {t('settings.warmup.mode_label', '预热触发模式')}
                        </label>
                        <div className="grid grid-cols-1 md:grid-cols-3 gap-2">
                            {/* 混合智能模式 */}
                            <div
                                onClick={() => handleModeChange('smart')}
                                className={`p-3 rounded-xl border cursor-pointer transition-all duration-200 flex flex-col justify-between gap-1.5 ${currentMode === 'smart'
                                    ? 'bg-orange-50/70 dark:bg-orange-950/20 border-orange-300 dark:border-orange-700/60 ring-1 ring-orange-400/30'
                                    : 'bg-gray-50/50 dark:bg-base-200/50 border-gray-100 dark:border-base-300 hover:border-gray-200'
                                    }`}
                            >
                                <div className="flex items-center justify-between">
                                    <span className="text-xs font-bold flex items-center gap-1.5 text-gray-900 dark:text-gray-100">
                                        <Zap size={14} className="text-orange-500" />
                                        {t('settings.warmup.mode_smart', '混合智能 (满额+定时)')}
                                    </span>
                                    <div className={`w-3.5 h-3.5 rounded-full flex items-center justify-center ${currentMode === 'smart' ? 'bg-orange-500 text-white' : 'border border-gray-300'}`}>
                                        {currentMode === 'smart' && <Check size={8} strokeWidth={4} />}
                                    </div>
                                </div>
                                <p className="text-[10px] text-gray-500 dark:text-gray-400 leading-tight">
                                    {t('settings.warmup.mode_smart_desc', '配额恢复 100% 或达到了定时周期时自动触发预热')}
                                </p>
                            </div>

                            {/* 自动计时预热 */}
                            <div
                                onClick={() => handleModeChange('timer')}
                                className={`p-3 rounded-xl border cursor-pointer transition-all duration-200 flex flex-col justify-between gap-1.5 ${currentMode === 'timer'
                                    ? 'bg-orange-50/70 dark:bg-orange-950/20 border-orange-300 dark:border-orange-700/60 ring-1 ring-orange-400/30'
                                    : 'bg-gray-50/50 dark:bg-base-200/50 border-gray-100 dark:border-base-300 hover:border-gray-200'
                                    }`}
                            >
                                <div className="flex items-center justify-between">
                                    <span className="text-xs font-bold flex items-center gap-1.5 text-gray-900 dark:text-gray-100">
                                        <Clock size={14} className="text-orange-500" />
                                        {t('settings.warmup.mode_timer', '自动计时预热')}
                                    </span>
                                    <div className={`w-3.5 h-3.5 rounded-full flex items-center justify-center ${currentMode === 'timer' ? 'bg-orange-500 text-white' : 'border border-gray-300'}`}>
                                        {currentMode === 'timer' && <Check size={8} strokeWidth={4} />}
                                    </div>
                                </div>
                                <p className="text-[10px] text-gray-500 dark:text-gray-400 leading-tight">
                                    {t('settings.warmup.mode_timer_desc', '按照固定计时间隔对选中的模型自动进行循环预热')}
                                </p>
                            </div>

                            {/* 满额度恢复预热 */}
                            <div
                                onClick={() => handleModeChange('quota_full')}
                                className={`p-3 rounded-xl border cursor-pointer transition-all duration-200 flex flex-col justify-between gap-1.5 ${currentMode === 'quota_full'
                                    ? 'bg-orange-50/70 dark:bg-orange-950/20 border-orange-300 dark:border-orange-700/60 ring-1 ring-orange-400/30'
                                    : 'bg-gray-50/50 dark:bg-base-200/50 border-gray-100 dark:border-base-300 hover:border-gray-200'
                                    }`}
                            >
                                <div className="flex items-center justify-between">
                                    <span className="text-xs font-bold flex items-center gap-1.5 text-gray-900 dark:text-gray-100">
                                        <ShieldCheck size={14} className="text-orange-500" />
                                        {t('settings.warmup.mode_quota_full', '满额度恢复预热')}
                                    </span>
                                    <div className={`w-3.5 h-3.5 rounded-full flex items-center justify-center ${currentMode === 'quota_full' ? 'bg-orange-500 text-white' : 'border border-gray-300'}`}>
                                        {currentMode === 'quota_full' && <Check size={8} strokeWidth={4} />}
                                    </div>
                                </div>
                                <p className="text-[10px] text-gray-500 dark:text-gray-400 leading-tight">
                                    {t('settings.warmup.mode_quota_full_desc', '仅在模型利用率恢复到 100% 时触发预热')}
                                </p>
                            </div>
                        </div>
                    </div>

                    {/* 预热计时间隔 (timer 及 smart 模式可用) */}
                    {currentMode !== 'quota_full' && (
                        <div className="bg-gray-50/60 dark:bg-base-200/40 p-3 rounded-xl border border-gray-100 dark:border-base-300/60 space-y-2">
                            <div className="flex items-center justify-between">
                                <label className="text-[11px] font-bold text-gray-700 dark:text-gray-300 flex items-center gap-1.5">
                                    <Clock size={13} className="text-orange-500" />
                                    {t('settings.warmup.interval_label', '预热计时间隔')}
                                </label>
                                <div className="flex items-center gap-2">
                                    <input
                                        type="number"
                                        min={5}
                                        max={1440}
                                        value={currentInterval}
                                        onChange={(e) => handleIntervalChange(parseInt(e.target.value) || 60)}
                                        className="w-20 px-2 py-1 bg-white dark:bg-base-100 border border-gray-200 dark:border-base-300 rounded-md text-xs text-center font-semibold text-orange-600 focus:outline-none focus:ring-1 focus:ring-orange-500"
                                    />
                                    <span className="text-xs text-gray-500 dark:text-gray-400">
                                        {t('settings.warmup.interval_unit', '分钟')}
                                    </span>
                                </div>
                            </div>
                            <div className="flex flex-wrap gap-1.5">
                                {intervalPresets.map((preset) => (
                                    <button
                                        key={preset.value}
                                        type="button"
                                        onClick={() => handleIntervalChange(preset.value)}
                                        className={`px-2.5 py-1 text-[11px] rounded-lg transition-all font-medium ${currentInterval === preset.value
                                            ? 'bg-orange-500 text-white font-semibold shadow-sm'
                                            : 'bg-white dark:bg-base-100 border border-gray-200 dark:border-base-300 text-gray-600 dark:text-gray-300 hover:border-orange-300'
                                            }`}
                                    >
                                        {preset.label}
                                    </button>
                                ))}
                            </div>
                        </div>
                    )}

                    {/* 监控模型选择 */}
                    <div>
                        <label className="text-[10px] font-bold text-gray-400 dark:text-gray-500 uppercase tracking-widest block mb-2">
                            {t('settings.quota_protection.monitored_models_label', '监控预热模型')}
                        </label>
                        <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
                            {warmupModelsOptions.map((model) => {
                                const isSelected = config.monitored_models?.includes(model.id);
                                return (
                                    <div
                                        key={model.id}
                                        onClick={() => toggleModel(model.id)}
                                        className={`
                                            flex items-center justify-between p-2 rounded-lg border cursor-pointer transition-all duration-200
                                            ${isSelected
                                                ? 'bg-orange-50 dark:bg-orange-900/10 border-orange-200 dark:border-orange-800/50 text-orange-700 dark:text-orange-400'
                                                : 'bg-gray-50/50 dark:bg-base-200/50 border-gray-100 dark:border-base-300/50 text-gray-500 hover:border-gray-200 dark:hover:border-base-300'}
                                        `}
                                    >
                                        <span className="text-[11px] font-medium truncate pr-2">
                                            {model.label}
                                        </span>
                                        <div className={`
                                            w-4 h-4 rounded-full flex items-center justify-center transition-all duration-300
                                            ${isSelected ? 'bg-orange-500 text-white scale-100' : 'bg-gray-200 dark:bg-base-300 text-transparent scale-75 opacity-0'}
                                        `}>
                                            <Check size={10} strokeWidth={4} />
                                        </div>
                                    </div>
                                );
                            })}
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
};

export default SmartWarmup;
