// File: src/features/settings/ui/QuotaProtection.tsx
// Quota Protection configuration component

import { Shield, Check, AlertTriangle, Activity } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { cn } from '@/shared/lib';
import { motion, AnimatePresence } from 'framer-motion';
import type { QuotaProtectionConfig } from '@/entities/config';

interface QuotaProtectionProps {
    config: QuotaProtectionConfig;
    onChange: (config: QuotaProtectionConfig) => void;
}

const QuotaProtection = ({ config, onChange }: QuotaProtectionProps) => {
    const { t } = useTranslation();

    const handleEnabledChange = (enabled: boolean) => {
        let newConfig = { ...config, enabled };
        if (enabled && (!config.monitored_models || config.monitored_models.length === 0)) {
            newConfig.monitored_models = ['claude-sonnet-4-5'];
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
            if (currentModels.length <= 1) return;
            newModels = currentModels.filter(m => m !== model);
        } else {
            newModels = [...currentModels, model];
        }

        onChange({ ...config, monitored_models: newModels });
    };

    const monitoredModelsOptions = [
        { id: 'gemini-3-flash', label: 'Gemini 3 Flash' },
        { id: 'gemini-3-pro-high', label: 'Gemini 3 Pro High' },
        { id: 'claude-sonnet-4-5', label: 'Claude 4.5 Sonnet' },
        { id: 'claude-opus-4-6-thinking', label: 'Claude 4.6 Opus' },
        { id: 'gemini-3-pro-image', label: 'Gemini 3 Pro Image' }
    ];

    const exampleTotal = 150;
    const exampleThreshold = Math.floor(exampleTotal * config.threshold_percentage / 100);

    return (
        <div className="space-y-6">
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-4">
                    <div className={cn(
                        "w-12 h-12 rounded-2xl flex items-center justify-center transition-all duration-500",
                        config.enabled 
                            ? "bg-rose-500/20 text-rose-500 shadow-[0_0_30px_rgba(244,63,94,0.2)]" 
                            : "bg-zinc-900 border border-white/5 text-zinc-500"
                    )}>
                        <Shield size={24} className="transition-all duration-300" />
                    </div>
                    <div>
                        <div className="font-bold text-lg text-white">
                            {t('settings.quota_protection.title')}
                        </div>
                        <p className="text-sm text-zinc-400 mt-0.5">
                            {t('settings.quota_protection.enable_desc')}
                        </p>
                    </div>
                </div>

                {/* Custom Toggle Switch */}
                <button
                    onClick={() => handleEnabledChange(!config.enabled)}
                    className={cn(
                        "relative w-14 h-8 rounded-full transition-all duration-300 ease-out focus:outline-none focus:ring-2 focus:ring-rose-500/50",
                        config.enabled ? "bg-rose-600 shadow-[0_0_20px_rgba(225,29,72,0.4)]" : "bg-zinc-800 border border-white/5"
                    )}
                >
                    <motion.div
                        initial={false}
                        animate={{ x: config.enabled ? 26 : 4 }}
                        className={cn(
                            "absolute top-1 left-0 w-6 h-6 rounded-full shadow-md flex items-center justify-center transition-colors duration-300",
                            config.enabled ? "bg-white" : "bg-zinc-500"
                        )}
                    >
                        {config.enabled && <Activity size={14} className="text-rose-600" />}
                    </motion.div>
                </button>
            </div>

            <AnimatePresence>
                {config.enabled && (
                    <motion.div
                        initial={{ opacity: 0, height: 0, y: -20 }}
                        animate={{ opacity: 1, height: "auto", y: 0 }}
                        exit={{ opacity: 0, height: 0, y: -20 }}
                        transition={{ duration: 0.3 }}
                        className="space-y-6 overflow-hidden"
                    >
                        {/* Threshold Input */}
                        <div className="p-5 rounded-2xl bg-zinc-900/50 border border-white/5 backdrop-blur-md">
                            <div className="flex items-center justify-between mb-4">
                                <label className="text-sm font-bold text-zinc-300 flex items-center gap-2">
                                    <Activity className="w-4 h-4 text-rose-500" />
                                    {t('settings.quota_protection.threshold_label')}
                                </label>
                                <div className="relative group">
                                    <input
                                        type="number"
                                        className="w-24 pl-4 pr-8 py-2 bg-black/40 border-2 border-white/10 rounded-xl focus:border-rose-500 focus:ring-4 focus:ring-rose-500/10 outline-none text-white font-mono font-bold transition-all text-right"
                                        min="1"
                                        max="99"
                                        value={config.threshold_percentage}
                                        onChange={(e) => handlePercentageChange(e.target.value)}
                                    />
                                    <span className="absolute right-3 top-1/2 -translate-y-1/2 text-zinc-500 font-bold">%</span>
                                </div>
                            </div>
                            
                            {/* Visual Range Slider Indicator (optional visual flair) */}
                            <div className="h-1.5 w-full bg-zinc-800 rounded-full overflow-hidden">
                                <motion.div 
                                    className="h-full bg-gradient-to-r from-rose-600 to-rose-400"
                                    animate={{ width: `${config.threshold_percentage}%` }}
                                />
                            </div>
                        </div>

                        {/* Monitored Models Grid */}
                        <div className="space-y-3">
                            <div className="flex items-center justify-between">
                                <label className="text-xs font-bold text-zinc-400 uppercase tracking-wider pl-1">
                                    {t('settings.quota_protection.monitored_models_label')}
                                </label>
                            </div>
                            
                            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                {monitoredModelsOptions.map((model) => {
                                    const isSelected = config.monitored_models?.includes(model.id);
                                    return (
                                        <motion.button
                                            key={model.id}
                                            whileHover={{ scale: 1.02 }}
                                            whileTap={{ scale: 0.98 }}
                                            onClick={() => toggleModel(model.id)}
                                            className={cn(
                                                "relative flex items-center justify-between p-4 rounded-xl border text-left transition-all duration-300 group",
                                                isSelected 
                                                    ? "bg-rose-500/10 border-rose-500/50 shadow-[0_0_15px_rgba(244,63,94,0.1)]" 
                                                    : "bg-zinc-900 border-zinc-800 hover:bg-zinc-800 hover:border-zinc-700"
                                            )}
                                        >
                                            <span className={cn(
                                                "text-sm font-bold transition-colors",
                                                isSelected ? "text-white" : "text-zinc-300 group-hover:text-white"
                                            )}>
                                                {model.label}
                                            </span>
                                            
                                            <div className={cn(
                                                "w-5 h-5 rounded-full border flex items-center justify-center transition-all duration-300",
                                                isSelected
                                                    ? "bg-rose-500 border-rose-500 shadow-lg shadow-rose-500/40 transform scale-100"
                                                    : "bg-transparent border-zinc-600 scale-100 group-hover:border-zinc-500"
                                            )}>
                                                {isSelected && <Check size={12} className="text-white stroke-[3px]" />}
                                            </div>
                                        </motion.button>
                                    );
                                })}
                            </div>
                        </div>

                        {/* Example/Info Card */}
                        <div className="relative p-4 rounded-xl bg-gradient-to-br from-blue-500/10 to-indigo-500/10 border border-blue-500/20">
                            <div className="flex gap-4">
                                <div className="p-2 bg-blue-500/20 rounded-lg h-fit text-blue-400">
                                    <AlertTriangle size={18} />
                                </div>
                                <div className="space-y-1">
                                    <p className="text-sm text-blue-200/90 leading-relaxed font-medium">
                                        {t('settings.quota_protection.example', {
                                            percentage: config.threshold_percentage,
                                            total: exampleTotal,
                                            threshold: exampleThreshold
                                        })}
                                    </p>
                                    <div className="flex items-center gap-2 mt-2">
                                        <span className="inline-flex items-center gap-1.5 px-2 py-1 rounded-md bg-emerald-500/10 border border-emerald-500/20 text-emerald-400 text-[10px] uppercase font-bold tracking-wide">
                                            <Check size={10} />
                                            {t('settings.quota_protection.auto_restore_info')}
                                        </span>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </motion.div>
                )}
            </AnimatePresence>
        </div>
    );
};

export default QuotaProtection;
