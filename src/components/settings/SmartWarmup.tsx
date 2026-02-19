import React from 'react';
import { useTranslation } from 'react-i18next';
import { Sparkles, Check } from 'lucide-react';
import { ScheduledWarmupConfig } from '../../types/config';
import { MODEL_CONFIG } from '../../config/modelConfig';

interface SmartWarmupProps {
    config: ScheduledWarmupConfig;
    onChange: (config: ScheduledWarmupConfig) => void;
}

const SmartWarmup: React.FC<SmartWarmupProps> = ({ config, onChange }) => {
    const { t } = useTranslation();

    const warmupModelsOptions = Object.entries(MODEL_CONFIG).map(([id, config]) => ({
        id,
        label: config.shortLabel
    }));

    const handleEnabledChange = (enabled: boolean) => {
        let newConfig = { ...config, enabled };
        // 如果开启预热且勾选列表为空，则默认勾选所有核心模型
        if (enabled && (!config.monitored_models || config.monitored_models.length === 0)) {
            newConfig.monitored_models = warmupModelsOptions.map(o => o.id);
        }
        onChange(newConfig);
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

    return (
        <div className="space-y-4">
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-4">
                    <div className={`w-10 h-10 rounded-xl flex items-center justify-center transition-all duration-300 ${config.enabled
                        ? 'bg-orange-500 text-white'
                        : 'bg-orange-50 dark:bg-orange-900/20 text-orange-500 group-hover:bg-orange-500 group-hover:text-white'
                        }`}>
                        <Sparkles size={20} />
                    </div>
                    <div>
                        <div className="font-bold text-gray-900 dark:text-gray-100">
                            {t('settings.warmup.title', '智能预热')}
                        </div>
                        <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                            {t('settings.warmup.desc')}
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
                <div className="mt-4 pt-4 border-t border-gray-50 dark:border-base-300 animate-in slide-in-from-top-2 duration-300">
                    <div className="space-y-3">
                        <div>
                            <label className="text-[10px] font-bold text-gray-400 dark:text-gray-500 uppercase tracking-widest block mb-2">
                                {t('settings.quota_protection.monitored_models_label', '监控模型')}
                            </label>
                            <div className="grid grid-cols-4 gap-2">
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
                            <p className="text-[10px] text-gray-400 dark:text-gray-500 mt-2 leading-relaxed">
                                {t('settings.quota_protection.monitored_models_desc', '勾选需要监控的模型。当选中的任一模型利用率跌破阈值时，将触发保护')}
                            </p>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
};

export default SmartWarmup;
