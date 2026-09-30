import { useTranslation } from "react-i18next";
import { CircuitBreakerConfig } from "../../types/config";
import { ShieldAlert, Trash2, Plus, Minus, Clock } from "lucide-react";

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
        if (index === 0) return "border-yellow-200 dark:border-yellow-700/50 bg-yellow-50/30 dark:bg-yellow-900/10";
        if (index === 1) return "border-orange-200 dark:border-orange-700/50 bg-orange-50/30 dark:bg-orange-900/10";
        if (index === 2) return "border-red-200 dark:border-red-700/50 bg-red-50/30 dark:bg-red-900/10";
        return "border-rose-200 dark:border-rose-700/50 bg-rose-50/30 dark:bg-rose-900/10";
    };

    return (
        <div className="space-y-6">
            <div className="bg-orange-50/70 dark:bg-orange-950/30 border border-orange-200/80 dark:border-orange-900/40 rounded-xl p-4">
                <div className="flex gap-3">
                    <ShieldAlert className="w-5 h-5 text-orange-500 shrink-0 mt-0.5" />
                    <div className="space-y-1">
                        <h4 className="font-bold text-sm text-gray-900 dark:text-white">
                            {t("proxy.config.circuit_breaker.title", { defaultValue: "Adaptive Circuit Breaker" })}
                        </h4>
                        <p className="text-xs text-gray-600 dark:text-gray-300 leading-relaxed">
                            {t("proxy.config.circuit_breaker.tooltip", {
                                defaultValue: "Automatically increases lockout duration for accounts that repeatedly fail with quota exhaustion. This prevents wasting API calls on dead accounts while allowing transient errors to recover quickly.",
                            })}
                        </p>
                    </div>
                </div>
            </div>

            {/* 零配额持续熔断开关 */}
            <div className="flex items-center justify-between p-3.5 bg-gray-50/90 dark:bg-base-200 rounded-xl border border-gray-200/80 dark:border-base-300">
                <div className="space-y-0.5">
                    <div className="text-xs font-bold text-gray-900 dark:text-white">
                        {t("proxy.config.circuit_breaker.lock_on_zero_quota", { defaultValue: "Lock on Zero 5h Quota" })}
                    </div>
                    <div className="text-xs text-gray-500 dark:text-gray-400">
                        {t("proxy.config.circuit_breaker.lock_on_zero_quota_desc", { defaultValue: "With the circuit breaker enabled, locks exhausted 5h quota until its official reset. Exhausted weekly quota always blocks the affected model group, independently of these switches, until the official reset or a fresh quota refresh confirms recovery." })}
                    </div>
                </div>
                <label className="relative inline-flex items-center cursor-pointer">
                    <input
                        type="checkbox"
                        className="sr-only peer"
                        checked={!!config.lock_on_zero_quota}
                        onChange={(e) => onChange({ ...config, lock_on_zero_quota: e.target.checked })}
                    />
                    <div className="w-10 h-5 bg-gray-200 dark:bg-base-300 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 dark:after:border-gray-600 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-orange-500 shadow-inner"></div>
                </label>
            </div>

            <div className="space-y-4">
                <div className="flex justify-between items-center">
                    <label className="text-xs font-bold text-gray-800 dark:text-gray-200 flex items-center gap-1.5">
                        <Clock className="w-4 h-4 text-blue-500" />
                        {t("proxy.config.circuit_breaker.backoff_levels", { defaultValue: "Backoff Levels (Seconds)" })}
                    </label>
                    <button
                        onClick={(e) => { e.stopPropagation(); addLevel(); }}
                        className="btn btn-xs btn-ghost text-blue-600 dark:text-blue-400 hover:bg-blue-50 dark:hover:bg-blue-900/30 gap-1 h-7 min-h-0 px-2.5 rounded-lg border border-blue-200 dark:border-blue-800/60 shadow-2xs font-semibold"
                    >
                        <Plus size={13} />
                        {t("common.add", { defaultValue: "Add" })}
                    </button>
                </div>

                <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
                    {config.backoff_steps.map((seconds, idx) => (
                        <div
                            key={idx}
                            className={`p-3 rounded-xl border transition-all hover:shadow-sm group relative ${getStepColorCls(idx)}`}
                        >
                            <div className="flex flex-col gap-2">
                                <div className="flex justify-between items-center">
                                    <span className="text-xs font-bold uppercase tracking-wider opacity-70">
                                        {t("proxy.config.circuit_breaker.level", { level: idx + 1, defaultValue: `Lv ${idx + 1}` })}
                                    </span>
                                    {config.backoff_steps.length > 1 && (
                                        <button
                                            onClick={(e) => { e.stopPropagation(); removeLevel(idx); }}
                                            className="opacity-0 group-hover:opacity-100 p-1 text-gray-400 hover:text-rose-500 transition-opacity"
                                            title={t("common.delete", { defaultValue: "Delete" })}
                                        >
                                            <Minus size={13} />
                                        </button>
                                    )}
                                </div>
                                <div className="relative">
                                    <input
                                        type="number"
                                        defaultValue={seconds}
                                        key={`${idx}-${seconds}`}
                                        onBlur={(e) => handleLevelChange(idx, e.target.value)}
                                        className="w-full bg-white dark:bg-base-200 border border-gray-300 dark:border-base-300 text-gray-900 dark:text-white rounded-lg px-3 py-1.5 text-xs font-mono focus:ring-2 focus:ring-blue-500/20 focus:border-blue-500 outline-none transition-all [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none"
                                        min="0"
                                    />
                                    <span className="absolute right-3 top-1/2 -translate-y-1/2 text-[10px] font-bold opacity-40 select-none pointer-events-none">S</span>
                                </div>
                            </div>
                        </div>
                    ))}
                </div>
            </div>

            {onClearRateLimits && (
                <div className="pt-2 border-t border-gray-200/80 dark:border-base-200">
                    <button
                        onClick={onClearRateLimits}
                        className="btn btn-sm btn-ghost text-gray-500 dark:text-gray-400 hover:text-rose-600 dark:hover:text-rose-400 hover:bg-rose-50 dark:hover:bg-rose-900/20 gap-2 w-full justify-start h-auto py-2 px-1 font-medium"
                    >
                        <Trash2 className="w-4 h-4" />
                        <span className="text-xs">
                            {t("proxy.config.circuit_breaker.clear_records", { defaultValue: "Clear All Rate Limit Records" })}
                        </span>
                    </button>
                </div>
            )}
        </div>
    );
}
