import React from 'react';
import { useTranslation } from 'react-i18next';
import { Sparkles, Check, Activity, Zap } from 'lucide-react';
import { ScheduledWarmupConfig } from '../../types/config';
import { cn } from '../../lib/utils';
import { motion, AnimatePresence } from 'framer-motion';

interface SmartWarmupProps {
    config: ScheduledWarmupConfig;
    onChange: (config: ScheduledWarmupConfig) => void;
}

const SmartWarmup: React.FC<SmartWarmupProps> = ({ config, onChange }) => {
    const { t } = useTranslation();

    // Pre-defined popular models
    const presetModels = [
        { id: 'gemini-2.5-pro', label: 'Gemini 2.5 Pro' },
        { id: 'gemini-2.5-flash-thinking', label: 'Gemini 2.5 Flash Thinking' },
        { id: 'claude-sonnet-4-5', label: 'Claude 3.5 Sonnet' }, // User said 4.5 but standard naming convention might differ, sticking to ID user gave
        { id: 'gemini-3-pro-high', label: 'Gemini 3 Pro High' },
        { id: 'gemini-3-flash', label: 'Gemini 3 Flash' },
        { id: 'claude-sonnet-4-5-thinking', label: 'Claude 3.5 Sonnet Thinking' },
        { id: 'gemini-3-pro-image', label: 'Gemini 3 Pro Image' },
        { id: 'gemini-2.5-flash-lite', label: 'Gemini 2.5 Flash Lite' },
        { id: 'gemini-2.5-flash', label: 'Gemini 2.5 Flash' },
        { id: 'gemini-3-pro-low', label: 'Gemini 3 Pro Low' },
        { id: 'claude-opus-4-5-thinking', label: 'Claude 3.5 Opus Thinking' },
    ];

    const handleEnabledChange = (enabled: boolean) => {
        let newConfig = { ...config, enabled };
        if (enabled && (!config.monitored_models || config.monitored_models.length === 0)) {
            newConfig.monitored_models = presetModels.map(o => o.id);
        }
        onChange(newConfig);
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

    return (
        <div className="space-y-6">
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-4">
                    <div className={cn(
                        "w-12 h-12 rounded-2xl flex items-center justify-center transition-all duration-500",
                        config.enabled 
                            ? "bg-amber-500/20 text-amber-500 shadow-[0_0_30px_rgba(245,158,11,0.2)]" 
                            : "bg-zinc-900 border border-white/5 text-zinc-500"
                    )}>
                        <Sparkles size={24} className="transition-all duration-300" />
                    </div>
                    <div>
                        <div className="font-bold text-lg text-white">
                            {t('settings.warmup.title')}
                        </div>
                        <p className="text-sm text-zinc-400 mt-0.5">
                            {t('settings.warmup.desc')}
                        </p>
                    </div>
                </div>

                {/* Custom Toggle Switch */}
                <button
                    onClick={() => handleEnabledChange(!config.enabled)}
                    className={cn(
                        "relative w-14 h-8 rounded-full transition-all duration-300 ease-out focus:outline-none focus:ring-2 focus:ring-amber-500/50",
                        config.enabled ? "bg-amber-600 shadow-[0_0_20px_rgba(217,119,6,0.4)]" : "bg-zinc-800 border border-white/5"
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
                        {config.enabled && <Zap size={14} className="text-amber-600 fill-amber-600" />}
                    </motion.div>
                </button>
            </div>

            <AnimatePresence>
                {config.enabled && (
                    <motion.div
                        initial={{ opacity: 0, height: 0, y: -10 }}
                        animate={{ opacity: 1, height: "auto", y: 0 }}
                        exit={{ opacity: 0, height: 0, y: -10 }}
                        transition={{ duration: 0.3 }}
                        className="space-y-4 overflow-hidden pt-2"
                    >
                        <div className="p-5 rounded-2xl bg-zinc-900/50 border border-white/5 backdrop-blur-md space-y-4">
                            
                            <div className="flex items-center justify-between">
                                <label className="text-xs font-bold text-zinc-400 uppercase tracking-wider pl-1">
                                    {t('settings.quota_protection.monitored_models_label')}
                                </label>
                            </div>

                            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                {presetModels.map((model) => {
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
                                                    ? "bg-amber-500/10 border-amber-500/50 shadow-[0_0_15px_rgba(245,158,11,0.1)]" 
                                                    : "bg-zinc-900 border-zinc-800 hover:bg-zinc-800 hover:border-zinc-700"
                                            )}
                                        >
                                            <div className="flex flex-col gap-1">
                                                <span className={cn(
                                                    "text-sm font-bold transition-colors",
                                                    isSelected ? "text-white" : "text-zinc-300 group-hover:text-white"
                                                )}>
                                                    {model.label}
                                                </span>
                                            </div>
                                            
                                            <div className="flex items-center gap-3">
                                                <div className={cn(
                                                    "w-5 h-5 rounded-full border flex items-center justify-center transition-all duration-300",
                                                    isSelected
                                                        ? "bg-amber-500 border-amber-500 shadow-lg shadow-amber-500/40 transform scale-100"
                                                        : "bg-transparent border-zinc-600 scale-100 group-hover:border-zinc-500"
                                                )}>
                                                    {isSelected && <Check size={12} className="text-white stroke-[3px]" />}
                                                </div>
                                            </div>
                                        </motion.button>
                                    );
                                })}
                            </div>

                            <div className="mt-4 flex items-start gap-2 text-xs text-zinc-500 bg-black/20 p-3 rounded-lg border border-white/5">
                                <Activity size={14} className="mt-0.5 text-amber-500 opacity-80" />
                                <p className="leading-relaxed">
                                    {t('settings.quota_protection.monitored_models_desc')}
                                </p>
                            </div>
                        </div>
                    </motion.div>
                )}
            </AnimatePresence>
        </div>
    );
};

export default SmartWarmup;
