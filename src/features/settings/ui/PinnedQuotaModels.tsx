// File: src/features/settings/ui/PinnedQuotaModels.tsx
// Pinned Quota Models selection component

import { Check, Sparkles } from 'lucide-react';
import { cn } from '@/shared/lib';
import { motion } from 'framer-motion';
import { useProxyModels } from '@/shared/hooks';
import type { PinnedQuotaModelsConfig } from '@/entities/config';

interface PinnedQuotaModelsProps {
    config: PinnedQuotaModelsConfig;
    onChange: (config: PinnedQuotaModelsConfig) => void;
}

const PinnedQuotaModels = ({ config, onChange }: PinnedQuotaModelsProps) => {
    const { models } = useProxyModels();

    const toggleModel = (model: string) => {
        const currentModels = config.models || [];
        let newModels: string[];

        if (currentModels.includes(model)) {
            // Keep at least one
            if (currentModels.length <= 1) return;
            newModels = currentModels.filter(m => m !== model);
        } else {
            newModels = [...currentModels, model];
        }

        onChange({ ...config, models: newModels });
    };

    const modelOptions = models.map(m => ({
        id: m.id,
        label: m.name,
        desc: m.desc
    }));

    return (
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            {modelOptions.map((model) => {
                const isSelected = config.models?.includes(model.id);
                return (
                    <motion.button
                        key={model.id}
                        whileHover={{ scale: 1.02 }}
                        whileTap={{ scale: 0.98 }}
                        onClick={() => toggleModel(model.id)}
                        className={cn(
                            "relative group flex items-start gap-3 p-4 rounded-xl border text-left transition-all duration-300 overflow-hidden",
                            isSelected 
                                ? "bg-indigo-500/10 border-indigo-500/50 shadow-[0_0_20px_rgba(99,102,241,0.15)]" 
                                : "bg-black/20 border-white/5 hover:bg-white/5 hover:border-white/10"
                        )}
                    >
                        {/* Background Glow for Active State */}
                        {isSelected && (
                            <div className="absolute inset-0 bg-gradient-to-r from-indigo-500/10 to-transparent opacity-50" />
                        )}

                        {/* Checkbox Indicator */}
                        <div className={cn(
                            "relative z-10 w-5 h-5 rounded-full border flex items-center justify-center transition-all duration-300 mt-0.5 shrink-0",
                            isSelected
                                ? "bg-indigo-500 border-indigo-500 shadow-lg shadow-indigo-500/40"
                                : "bg-transparent border-white/20 group-hover:border-white/40"
                        )}>
                            <motion.div
                                initial={false}
                                animate={{ scale: isSelected ? 1 : 0 }}
                            >
                                <Check size={12} className="text-white stroke-[3px]" />
                            </motion.div>
                        </div>

                        {/* Text Content */}
                        <div className="relative z-10 flex-1 min-w-0">
                            <div className={cn(
                                "text-sm font-bold transition-colors duration-300 flex items-center gap-2",
                                isSelected ? "text-white" : "text-zinc-400 group-hover:text-zinc-200"
                            )}>
                                {model.label}
                                {isSelected && <Sparkles className="w-3 h-3 text-indigo-400 animate-pulse" />}
                            </div>
                            <div className={cn(
                                "text-xs mt-1 truncate transition-colors duration-300",
                                isSelected ? "text-indigo-200/70" : "text-zinc-600 group-hover:text-zinc-500"
                            )}>
                                {model.desc}
                            </div>
                        </div>

                        {/* Decorative Icon Blob in bg */}
                        {isSelected && (
                             <div className="absolute -bottom-4 -right-4 w-16 h-16 bg-indigo-500/20 rounded-full blur-2xl pointer-events-none" />
                        )}
                    </motion.button>
                );
            })}
        </div>
    );
};

export default PinnedQuotaModels;
