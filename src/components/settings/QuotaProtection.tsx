import { Shield, Check } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { QuotaProtectionConfig } from '../../types/config';
import { MODEL_CONFIG } from '../../config/modelConfig';

interface QuotaProtectionProps {
    config: QuotaProtectionConfig;
    onChange: (config: QuotaProtectionConfig) => void;
}

const QuotaProtection = ({ config, onChange }: QuotaProtectionProps) => {
    const { t } = useTranslation();

    const handleEnabledChange = (enabled: boolean) => {
        let newConfig = { ...config, enabled };
        // 如果开启保护且勾选列表为空，则默认勾选 claude
        if (enabled && (!config.monitored_models || config.monitored_models.length === 0)) {
            newConfig.monitored_models = ['claude'];
        }
        onChange(newConfig);
    };

    const handlePercentageChange = (value: string) => {
        const percentage = parseInt(value) || 10;
        const clampedPercentage = Math.max(1, Math.min(99, percentage));
        onChange({ ...config, threshold_percentage: clampedPercentage });
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

    const monitoredModelsOptions = Object.entries(MODEL_CONFIG).map(([id, config]) => ({
        id,
        label: config.shortLabel
    }));

    // 计算示例值
    const exampleTotal = 150;
    const exampleThreshold = Math.floor(exampleTotal * config.threshold_percentage / 100);

    return (
        <div className="animate-in fade-in duration-500">
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-4">
                    {/* 图标部分 - 使用红色/玫瑰色调表示保护/警示 */}
                    <div className="w-10 h-10 rounded-xl bg-rose-50 dark:bg-rose-900/20 flex items-center justify-center text-rose-500 group-hover:bg-rose-500 group-hover:text-white transition-all duration-300">
                        <Shield size={20} />
                    </div>
                    <div>
                        <div className="font-bold text-gray-900 dark:text-gray-100">
                            {t('settings.quota_protection.title')}
                        </div>
                        <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                            {t('settings.quota_protection.enable_desc')}
                        </p>
                    </div>
                </div>

                {/* 开关部分 */}
                <label className="relative inline-flex items-center cursor-pointer">
                    <input
                        type="checkbox"
                        className="sr-only peer"
                        checked={config.enabled}
                        onChange={(e) => handleEnabledChange(e.target.checked)}
                    />
                    <div className="w-11 h-6 bg-gray-200 dark:bg-base-300 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-rose-500 shadow-inner"></div>
                </label>
            </div>

            {/* 展开的详情设置部分 */}
            {config.enabled && (
                <div className="mt-5 pt-5 border-t border-gray-100 dark:border-base-200 space-y-6 animate-in slide-in-from-top-1 duration-200">
                    {/* 百分比设置 */}
                    <div className="flex items-center gap-4">
                        <label className="text-xs font-bold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                            {t('settings.quota_protection.threshold_label')}
                        </label>
                        <div className="relative flex items-center gap-2">
                            <input
                                type="number"
                                className="w-24 px-3 py-2 bg-gray-50 dark:bg-base-200 border border-gray-200 dark:border-base-300 rounded-lg focus:ring-2 focus:ring-rose-500 outline-none text-sm font-bold text-rose-600 dark:text-rose-400"
                                min="1"
                                max="99"
                                value={config.threshold_percentage}
                                onChange={(e) => handlePercentageChange(e.target.value)}
                            />
                            <span className="text-sm font-bold text-gray-400 dark:text-gray-500">%</span>
                        </div>
                    </div>

                    {/* 监控模型勾选 */}
                    <div className="space-y-3">
                        <div className="flex flex-col gap-1">
                            <label className="text-xs font-bold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                                {t('settings.quota_protection.monitored_models_label')}
                            </label>
                            <p className="text-[10px] text-gray-400 dark:text-gray-500">
                                {t('settings.quota_protection.monitored_models_desc')}
                            </p>
                        </div>
                        <div className="grid grid-cols-4 gap-2">
                            {monitoredModelsOptions.map((model) => {
                                const isSelected = config.monitored_models?.includes(model.id);
                                return (
                                    <div
                                        key={model.id}
                                        onClick={() => toggleModel(model.id)}
                                        className={`
                                            flex items-center justify-between p-2 rounded-lg border cursor-pointer transition-all duration-200
                                            ${isSelected
                                                ? 'bg-rose-50 dark:bg-rose-900/10 border-rose-200 dark:border-rose-800/50 text-rose-700 dark:text-rose-400'
                                                : 'bg-gray-50/50 dark:bg-base-200/50 border-gray-100 dark:border-base-300/50 text-gray-500 hover:border-gray-200 dark:hover:border-base-300'}
                                        `}
                                    >
                                        <span className="text-[11px] font-medium truncate pr-2">
                                            {model.label}
                                        </span>
                                        <div className={`
                                            w-4 h-4 rounded-full flex items-center justify-center transition-all duration-300
                                            ${isSelected ? 'bg-rose-500 text-white scale-100' : 'bg-gray-200 dark:bg-base-300 text-transparent scale-75 opacity-0'}
                                        `}>
                                            <Check size={10} strokeWidth={4} />
                                        </div>
                                    </div>
                                );
                            })}
                        </div>
                    </div>

                    {/* 示例提示卡片 */}
                    <div className="flex items-start gap-3 p-3 bg-blue-50/50 dark:bg-gray-800/50 rounded-xl border border-blue-100/50 dark:border-base-300">
                        <div className="text-blue-500 mt-0.5">
                            <span className="text-sm">💡</span>
                        </div>
                        <div className="flex flex-col gap-1">
                            <p className="text-xs text-blue-700/80 dark:text-gray-300/90 leading-relaxed font-medium">
                                {t('settings.quota_protection.example', {
                                    percentage: config.threshold_percentage,
                                    total: exampleTotal,
                                    threshold: exampleThreshold
                                })}
                            </p>
                            <span className="block font-bold text-emerald-600 dark:text-emerald-400 text-[10px] uppercase tracking-wide">
                                ✓ {t('settings.quota_protection.auto_restore_info')}
                            </span>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
};

export default QuotaProtection;
