// File: src/features/settings/ui/CircuitBreaker.tsx
// Circuit Breaker configuration component

import { useTranslation } from "react-i18next";
import { ShieldAlert, Trash2, Plus, Minus, Clock, Zap } from "lucide-react";
import { cn } from "@/shared/lib";
import { motion, AnimatePresence } from "framer-motion";
import type { CircuitBreakerConfig } from "@/entities/config";

interface CircuitBreakerProps {
    config: CircuitBreakerConfig;
    onChange: (config: CircuitBreakerConfig) => void;
    onClearRateLimits?: () => void;
}

export default function CircuitBreaker({
    config,
    onChange,
    onClearRateLimits,
}: CircuitBreakerProps) {
    const { t } = useTranslation();

    const handleLevelChange = (index: number, val: string) => {
        let num = parseInt(val, 10);
        if (isNaN(num)) num = 0;

        const newSteps = [...config.backoff_steps];
        newSteps[index] = Math.max(0, num);
        onChange({ ...config, backoff_steps: newSteps });
    };

    const addLevel = () => {
        const lastVal = config.backoff_steps[config.backoff_steps.length - 1] || 60;
        onChange({
            ...config,
            backoff_steps: [...config.backoff_steps, lastVal * 2],
        });
    };

    const removeLevel = (index: number) => {
        if (config.backoff_steps.length <= 1) return;
        const newSteps = config.backoff_steps.filter((_, i) => i !== index);
        onChange({ ...config, backoff_steps: newSteps });
    };

    const getStepColorCls = (index: number) => {
        if (index === 0) return "border-yellow-500/30 bg-yellow-500/10 text-yellow-500 shadow-[0_0_15px_rgba(234,179,8,0.1)]";
        if (index === 1) return "border-orange-500/30 bg-orange-500/10 text-orange-500 shadow-[0_0_15px_rgba(249,115,22,0.1)]";
        if (index === 2) return "border-red-500/30 bg-red-500/10 text-red-500 shadow-[0_0_15px_rgba(239,68,68,0.1)]";
        return "border-rose-600/30 bg-rose-600/10 text-rose-500 shadow-[0_0_15px_rgba(225,29,72,0.1)]";
    };

    return (
        <div className="space-y-6 animate-in fade-in duration-500">
            {/* Header / Info Card */}
            <div className="bg-zinc-900/50 border border-orange-500/20 rounded-2xl p-5 flex items-start gap-4 backdrop-blur-sm relative overflow-hidden group">
                <div className="absolute inset-0 bg-gradient-to-r from-orange-500/5 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500" />
                
                <div className="w-10 h-10 rounded-xl bg-orange-500/10 flex items-center justify-center shrink-0 border border-orange-500/20 shadow-[0_0_15px_rgba(249,115,22,0.15)]">
                    <ShieldAlert className="w-5 h-5 text-orange-500" />
                </div>
                <div className="space-y-1 relative z-10">
                    <h4 className="font-bold text-base text-gray-100 flex items-center gap-2">
                        {t("settings.proxy.circuit_breaker.title")}
                        <span className="px-2 py-0.5 rounded-full bg-orange-500/20 text-orange-400 text-[10px] uppercase tracking-wider font-bold border border-orange-500/10">Adaptive</span>
                    </h4>
                    <p className="text-xs text-zinc-400 leading-relaxed max-w-lg">
                        {t("settings.proxy.circuit_breaker.tooltip")}
                    </p>
                </div>
            </div>

            <div className="space-y-4">
                <div className="flex justify-between items-center px-1">
                    <label className="text-sm font-bold text-zinc-300 flex items-center gap-2">
                        <Clock className="w-4 h-4 text-blue-500" />
                        {t("settings.proxy.circuit_breaker.backoff_levels")}
                    </label>
                    <button
                        onClick={(e) => { e.stopPropagation(); addLevel(); }}
                        className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-blue-500/10 text-blue-400 text-xs font-bold border border-blue-500/20 hover:bg-blue-500/20 hover:border-blue-500/40 transition-all hover:scale-105 active:scale-95"
                    >
                        <Plus size={14} />
                        {t("common.add")}
                    </button>
                </div>

                <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
                    <AnimatePresence initial={false}>
                        {config.backoff_steps.map((seconds, idx) => (
                            <motion.div
                                key={idx}
                                layout
                                initial={{ opacity: 0, scale: 0.8 }}
                                animate={{ opacity: 1, scale: 1 }}
                                exit={{ opacity: 0, scale: 0.8 }}
                                className={cn(
                                    "p-4 rounded-xl border transition-all group relative overflow-hidden",
                                    getStepColorCls(idx)
                                )}
                            >
                                <div className="flex flex-col gap-3 relative z-10">
                                    <div className="flex justify-between items-center">
                                        <div className="flex items-center gap-1.5">
                                            <Zap size={10} className="opacity-70" />
                                            <span className="text-[10px] font-black uppercase tracking-widest opacity-80">
                                                {t("settings.proxy.circuit_breaker.level", { level: idx + 1 })}
                                            </span>
                                        </div>
                                        {config.backoff_steps.length > 1 && (
                                            <button
                                                onClick={(e) => { e.stopPropagation(); removeLevel(idx); }}
                                                className="opacity-0 group-hover:opacity-100 p-1 hover:bg-black/20 rounded text-inherit transition-all"
                                                title={t("common.delete")}
                                            >
                                                <Minus size={12} />
                                            </button>
                                        )}
                                    </div>
                                    <div className="relative">
                                        <input
                                            type="number"
                                            value={seconds}
                                            onChange={(e) => handleLevelChange(idx, e.target.value)}
                                            className="w-full bg-black/20 border-white/10 border rounded-lg px-3 py-2 text-sm font-mono font-bold focus:ring-2 focus:ring-white/20 outline-none transition-all text-center [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none shadow-inner"
                                            min="0"
                                        />
                                        <span className="absolute right-3 top-1/2 -translate-y-1/2 text-[10px] font-bold opacity-50 select-none pointer-events-none">S</span>
                                    </div>
                                </div>
                            </motion.div>
                        ))}
                    </AnimatePresence>
                </div>
            </div>

            {onClearRateLimits && (
                <div className="pt-4 border-t border-white/5">
                    <button
                        onClick={onClearRateLimits}
                        className="group flex items-center gap-3 px-4 py-3 rounded-xl w-full bg-zinc-900/30 border border-white/5 hover:bg-red-500/10 hover:border-red-500/30 hover:text-red-400 text-zinc-500 transition-all duration-300"
                    >
                        <div className="w-8 h-8 rounded-lg bg-black/20 flex items-center justify-center group-hover:bg-red-500/20 transition-colors">
                            <Trash2 className="w-4 h-4" />
                        </div>
                        <span className="text-xs font-bold">
                            {t("settings.proxy.circuit_breaker.clear_records")}
                        </span>
                    </button>
                </div>
            )}
        </div>
    );
}
