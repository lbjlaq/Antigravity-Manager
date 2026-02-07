// File: src/features/settings/ui/SchedulingSettings.tsx
// Scheduling Settings configuration component

import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Clock, Users, ArrowRightLeft, Target, Check, Search, ChevronDown, ChevronUp, Shuffle } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';

import { cn } from '@/shared/lib';
import { useAccounts } from '@/features/accounts';
import type { StickySessionConfig, SchedulingMode } from '@/entities/config';

interface SchedulingSettingsProps {
    config?: StickySessionConfig;
    onChange: (config: StickySessionConfig) => void;
}

const SchedulingSettings: React.FC<SchedulingSettingsProps> = ({ config, onChange }) => {
    const { t } = useTranslation();
    const { data: accounts = [], refetch: fetchAccounts } = useAccounts();
    
    useEffect(() => {
        if (accounts.length === 0) fetchAccounts();
    }, []);

    const currentMode = config?.mode || 'Balance';
    const maxWaitSeconds = config?.max_wait_seconds || 60;
    const selectedAccounts = new Set(config?.selected_accounts || []);
    const selectedModels = config?.selected_models || {};
    const strictSelected = config?.strict_selected || false;

    const [expandedAccount, setExpandedAccount] = useState<string | null>(null);
    const [searchTerm, setSearchTerm] = useState('');

    const handleChangeMode = (mode: SchedulingMode) => {
        onChange({
            mode,
            max_wait_seconds: maxWaitSeconds,
            selected_accounts: Array.from(selectedAccounts),
            selected_models: selectedModels,
            strict_selected: strictSelected
        });
    };

    const handleChangeWait = (seconds: number) => {
        onChange({
            mode: currentMode,
            max_wait_seconds: seconds,
            selected_accounts: Array.from(selectedAccounts),
            selected_models: selectedModels,
            strict_selected: strictSelected
        });
    };

    const handleToggleStrict = (strict: boolean) => {
        onChange({
            mode: currentMode,
            max_wait_seconds: maxWaitSeconds,
            selected_accounts: Array.from(selectedAccounts),
            selected_models: selectedModels,
            strict_selected: strict
        });
    };

    const toggleAccount = (accountId: string) => {
        const newSet = new Set(selectedAccounts);
        if (newSet.has(accountId)) {
            newSet.delete(accountId);
        } else {
            newSet.add(accountId);
        }
        onChange({
            mode: currentMode,
            max_wait_seconds: maxWaitSeconds,
            selected_accounts: Array.from(newSet),
            selected_models: selectedModels,
            strict_selected: strictSelected
        });
    };

    const toggleAllAccounts = () => {
        if (selectedAccounts.size === accounts.length) {
            onChange({
                mode: currentMode,
                max_wait_seconds: maxWaitSeconds,
                selected_accounts: [],
                selected_models: selectedModels,
                strict_selected: strictSelected
            });
        } else {
            onChange({
                mode: currentMode,
                max_wait_seconds: maxWaitSeconds,
                selected_accounts: accounts.map(a => a.id),
                selected_models: selectedModels,
                strict_selected: strictSelected
            });
        }
    };

    const toggleModel = (accountId: string, modelName: string) => {
        const currentList = selectedModels[accountId] || [];
        let newList: string[];
        
        if (currentList.includes(modelName)) {
            newList = currentList.filter(m => m !== modelName);
        } else {
            newList = [...currentList, modelName];
        }

        onChange({
            mode: currentMode,
            max_wait_seconds: maxWaitSeconds,
            selected_accounts: Array.from(selectedAccounts),
            selected_models: { ...selectedModels, [accountId]: newList },
            strict_selected: strictSelected
        });
    };

    const modes = [
        {
            value: 'Balance' as SchedulingMode,
            icon: ArrowRightLeft,
            label: t('settings.proxy.scheduling.modes.Balance'),
            desc: t('settings.proxy.scheduling.modes_desc.Balance')
        },
        {
            value: 'CacheFirst' as SchedulingMode,
            icon: Clock,
            label: t('settings.proxy.scheduling.modes.CacheFirst'),
            desc: t('settings.proxy.scheduling.modes_desc.CacheFirst')
        },
        {
            value: 'PerformanceFirst' as SchedulingMode,
            icon: Users,
            label: t('settings.proxy.scheduling.modes.PerformanceFirst'),
            desc: t('settings.proxy.scheduling.modes_desc.PerformanceFirst')
        },
        {
            value: 'P2C' as SchedulingMode,
            icon: Shuffle,
            label: t('settings.proxy.scheduling.modes.P2C', { defaultValue: 'P2C' }),
            desc: t('settings.proxy.scheduling.modes_desc.P2C', { defaultValue: 'Power-of-2-Choices: randomly picks 2 accounts, selects the one with higher quota. Reduces hot-spot issues.' })
        },
        {
            value: 'Selected' as SchedulingMode,
            icon: Target,
            label: t('settings.proxy.scheduling.modes.Selected'),
            desc: t('settings.proxy.scheduling.modes_desc.Selected')
        }
    ];

    const filteredAccounts = accounts.filter(a => 
        a.email.toLowerCase().includes(searchTerm.toLowerCase())
    );

    return (
        <div className="space-y-4 animate-in fade-in duration-500">
            {/* Compact Mode Selection Tabs */}
            <div className="bg-zinc-900/50 p-1 rounded-xl border border-white/5 flex gap-1 overflow-x-auto custom-scrollbar">
                {modes.map((mode) => {
                    const isSelected = currentMode === mode.value;
                    return (
                        <button
                            key={mode.value}
                            onClick={() => handleChangeMode(mode.value)}
                            className={cn(
                                "relative flex-1 flex items-center justify-center gap-2 px-3 py-2 rounded-lg text-xs font-bold transition-all whitespace-nowrap min-w-[120px]",
                                isSelected 
                                    ? "text-white shadow-sm" 
                                    : "text-zinc-500 hover:text-zinc-300 hover:bg-white/5"
                            )}
                        >
                            {isSelected && (
                                <motion.div
                                    layoutId="activeTab"
                                    className="absolute inset-0 bg-zinc-800 rounded-lg border border-white/10 shadow-sm"
                                    transition={{ type: "spring", bounce: 0.2, duration: 0.6 }}
                                />
                            )}
                            <div className="relative z-10 flex items-center gap-1.5">
                                <mode.icon size={14} className={cn(isSelected ? "text-blue-400" : "opacity-70")} />
                                <span>{mode.label}</span>
                            </div>
                        </button>
                    )
                })}
            </div>

            {/* Active Mode Description */}
            <motion.div 
                key={currentMode}
                initial={{ opacity: 0, y: -2 }}
                animate={{ opacity: 1, y: 0 }}
                className="bg-blue-500/5 border border-blue-500/10 rounded-lg p-2.5 flex gap-3 text-[11px] text-blue-200/80 leading-relaxed"
            >
                <div className="shrink-0 mt-0.5">
                    {(() => {
                        const Icon = modes.find(m => m.value === currentMode)?.icon;
                        return Icon ? <Icon size={14} className="text-blue-400" /> : null;
                    })()}
                </div>
                <p>{modes.find(m => m.value === currentMode)?.desc}</p>
            </motion.div>

            <AnimatePresence mode="wait">
                {/* Max Wait Config - only for CacheFirst mode */}
                {currentMode === 'CacheFirst' && (
                    <motion.div
                        initial={{ opacity: 0, height: 0 }}
                        animate={{ opacity: 1, height: "auto" }}
                        exit={{ opacity: 0, height: 0 }}
                        className="overflow-hidden"
                    >
                        <div className="p-3 rounded-xl bg-zinc-900/30 border border-white/5 flex items-center justify-between">
                            <div className="flex gap-3 items-center">
                                <div className="p-1.5 bg-amber-500/10 rounded-lg text-amber-500">
                                    <Clock size={14} />
                                </div>
                                <div>
                                    <label className="text-xs font-bold text-zinc-300 block">
                                        {t('settings.proxy.scheduling.max_wait')}
                                    </label>
                                    <p className="text-[10px] text-zinc-500 hidden sm:block">
                                        {t('settings.proxy.scheduling.max_wait_tooltip')}
                                    </p>
                                </div>
                            </div>
                            <div className="flex items-center gap-2 bg-black/40 p-1 rounded-lg border border-white/10">
                                <input
                                    type="number"
                                    min="0"
                                    max="300"
                                    className="w-12 bg-transparent text-center font-mono font-bold text-white text-sm outline-none"
                                    value={maxWaitSeconds}
                                    onChange={(e) => handleChangeWait(parseInt(e.target.value) || 0)}
                                />
                                <span className="text-[10px] font-bold text-zinc-600 pr-2">SEC</span>
                            </div>
                        </div>
                    </motion.div>
                )}
            </AnimatePresence>

            {/* Selected Accounts Picker */}
            <AnimatePresence>
                {currentMode === 'Selected' && (
                    <motion.div
                        initial={{ opacity: 0, y: 10 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, y: 10 }}
                        className="space-y-3 pt-2"
                    >
                        {/* Strict Mode Toggle */}
                        <div className="p-3 rounded-xl bg-zinc-900/30 border border-white/5 flex items-center justify-between">
                            <div className="flex gap-3 items-center">
                                <div className="p-1.5 bg-red-500/10 rounded-lg text-red-500">
                                    <Target size={14} />
                                </div>
                                <div>
                                    <label className="text-xs font-bold text-zinc-300 block">
                                        {t('settings.proxy.scheduling.strict_mode', { defaultValue: 'Strict Mode' })}
                                    </label>
                                    <p className="text-[10px] text-zinc-500 hidden sm:block">
                                        {t('settings.proxy.scheduling.strict_mode_tooltip', { defaultValue: 'Fail request if no selected account available (no fallback)' })}
                                    </p>
                                </div>
                            </div>
                            <label className="relative inline-flex items-center cursor-pointer">
                                <input
                                    type="checkbox"
                                    checked={strictSelected}
                                    onChange={(e) => handleToggleStrict(e.target.checked)}
                                    className="sr-only peer"
                                />
                                <div className="w-9 h-5 bg-zinc-700 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-red-500"></div>
                            </label>
                        </div>

                        <div className="flex items-center justify-between">
                            <h4 className="text-xs font-bold text-zinc-400 flex items-center gap-2 uppercase tracking-wider">
                                <Target size={14} />
                                Target Accounts
                                <span className={cn(
                                    "px-1.5 py-0.5 rounded text-[10px]",
                                    selectedAccounts.size > 0 ? "bg-blue-500/20 text-blue-400" : "bg-red-500/20 text-red-400"
                                )}>
                                    {selectedAccounts.size} / {accounts.length}
                                </span>
                            </h4>
                            <div className="flex items-center gap-2">
                                {/* Compact Search */}
                                <div className="relative group">
                                    <Search size={12} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-zinc-600 group-focus-within:text-blue-500" />
                                    <input 
                                        type="text"
                                        placeholder="Filter..."
                                        value={searchTerm}
                                        onChange={(e) => setSearchTerm(e.target.value)}
                                        className="w-28 bg-zinc-900 border border-white/5 rounded-md py-1 pl-7 pr-2 text-[10px] text-white focus:outline-none focus:border-blue-500/30 transition-all placeholder:text-zinc-700"
                                    />
                                </div>
                                <button 
                                    onClick={toggleAllAccounts}
                                    className="text-[10px] font-bold text-zinc-500 hover:text-white transition-colors px-2 py-1 hover:bg-white/5 rounded"
                                >
                                    {selectedAccounts.size === accounts.length ? 'NONE' : 'ALL'}
                                </button>
                            </div>
                        </div>

                        <div className="max-h-[400px] overflow-y-auto pr-1 space-y-1.5 custom-scrollbar bg-zinc-900/20 p-1 rounded-xl border border-white/5">
                             {filteredAccounts.length === 0 ? (
                                <div className="py-6 text-center">
                                    <p className="text-zinc-600 text-[10px]">No accounts found</p>
                                </div>
                            ) : (
                                filteredAccounts.map((account) => {
                                    const isSelected = selectedAccounts.has(account.id);
                                    const isExpanded = expandedAccount === account.id;
                                    const accountModels = account.quota?.models || [];
                                    const currentAllowedModels = selectedModels[account.id] || [];

                                    return (
                                        <motion.div 
                                            key={account.id}
                                            layout
                                            initial={{ opacity: 0 }}
                                            animate={{ opacity: 1 }}
                                            className={cn(
                                                "rounded-lg border transition-all overflow-hidden group",
                                                isSelected 
                                                    ? "bg-blue-500/5 border-blue-500/20"
                                                    : "bg-zinc-900/40 border-transparent hover:border-white/5 hover:bg-zinc-900/60"
                                            )}
                                        >
                                            <div 
                                                className="flex items-center gap-3 p-2.5 cursor-pointer"
                                                onClick={() => toggleAccount(account.id)}
                                            >
                                                {/* Checkbox */}
                                                <div className={cn(
                                                    "w-4 h-4 rounded border flex items-center justify-center transition-all duration-200",
                                                    isSelected
                                                        ? "bg-blue-500 border-blue-500 text-white shadow-sm shadow-blue-500/20"
                                                        : "bg-black/20 border-zinc-700 group-hover:border-zinc-500"
                                                )}>
                                                    <Check size={10} strokeWidth={3} className={cn("transition-transform", isSelected ? "scale-100" : "scale-0")} />
                                                </div>

                                                {/* Account Info */}
                                                <div className="flex-1 min-w-0 flex flex-col justify-center">
                                                    <div className="flex items-baseline gap-2">
                                                        <span className={cn(
                                                            "text-xs font-bold truncate transition-colors",
                                                            isSelected ? "text-zinc-200" : "text-zinc-500 group-hover:text-zinc-400"
                                                        )}>
                                                            {account.email}
                                                        </span>
                                                        <span className={cn(
                                                            "text-[9px] font-bold px-1 rounded uppercase",
                                                            (account.quota?.subscription_tier === 'ULTRA') 
                                                                ? "text-purple-400 bg-purple-500/10"
                                                                : (account.quota?.subscription_tier === 'PRO')
                                                                    ? "text-blue-400 bg-blue-500/10"
                                                                    : "text-zinc-600 bg-zinc-800"
                                                        )}>
                                                            {account.quota?.subscription_tier || 'FREE'}
                                                        </span>
                                                    </div>
                                                    
                                                    {/* Status Line */}
                                                    {(isSelected || account.disabled) && (
                                                        <div className="flex items-center gap-2 mt-0.5">
                                                            {account.disabled && (
                                                                <span className="text-[9px] text-red-500 font-bold uppercase tracking-wider">DISABLED</span>
                                                            )}
                                                            {isSelected && (
                                                                <span className="text-[10px] text-blue-500/80 flex items-center gap-1">
                                                                    {currentAllowedModels.length === 0 ? 'Full Access' : `${currentAllowedModels.length} models`}
                                                                </span>
                                                            )}
                                                        </div>
                                                    )}
                                                </div>

                                                {/* Expand Button */}
                                                {isSelected && (
                                                    <button
                                                        onClick={(e) => {
                                                            e.stopPropagation();
                                                            setExpandedAccount(isExpanded ? null : account.id);
                                                        }}
                                                        className={cn(
                                                            "p-1.5 rounded-md transition-colors",
                                                            isExpanded ? "bg-blue-500/20 text-blue-400" : "hover:bg-white/5 text-zinc-600 hover:text-zinc-400"
                                                        )}
                                                    >
                                                        {isExpanded ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
                                                    </button>
                                                )}
                                            </div>

                                            {/* Expandable Model Config */}
                                            <AnimatePresence>
                                                {isSelected && isExpanded && (
                                                    <motion.div
                                                        initial={{ opacity: 0, height: 0 }}
                                                        animate={{ opacity: 1, height: "auto" }}
                                                        exit={{ opacity: 0, height: 0 }}
                                                        className="border-t border-blue-500/10 bg-blue-500/5 px-2 py-2"
                                                    >
                                                        <div className="grid grid-cols-2 gap-1.5">
                                                            {accountModels.map(m => {
                                                                const isChecked = currentAllowedModels.includes(m.name);
                                                                const remaining = m.percentage || 0;
                                                                const usage = Math.max(0, 100 - remaining); 
                                                                const isRateLimited = remaining <= 0;
                                                                
                                                                return (
                                                                    <button
                                                                        key={m.name}
                                                                        onClick={() => toggleModel(account.id, m.name)}
                                                                        className={cn(
                                                                            "relative flex flex-col gap-0.5 px-2 py-1.5 rounded text-[10px] border transition-all text-left group/btn",
                                                                            isChecked
                                                                                ? "bg-blue-500/20 border-blue-500/30 text-blue-200"
                                                                                : "bg-black/20 border-white/5 text-zinc-500 hover:bg-black/40 hover:text-zinc-400"
                                                                        )}
                                                                    >
                                                                        <div className="flex justify-between items-center w-full">
                                                                            <span className="font-semibold truncate pr-1">{m.name}</span>
                                                                            {isChecked && <Check size={10} className="text-blue-400" />}
                                                                        </div>
                                                                        
                                                                        {/* Nano Progress Bar */}
                                                                        <div className="flex items-center gap-1.5 mt-0.5 opacity-80">
                                                                            <div className="flex-1 h-1 bg-black/40 rounded-full overflow-hidden">
                                                                                <div 
                                                                                    className={cn(
                                                                                        "h-full rounded-full transition-all duration-500",
                                                                                         isRateLimited ? "bg-red-500" : usage > 80 ? "bg-amber-500" : "bg-emerald-500"
                                                                                    )}
                                                                                    style={{ width: `${Math.min(usage, 100)}%` }}
                                                                                />
                                                                            </div>
                                                                            <span className={cn(
                                                                                "text-[9px] font-mono",
                                                                                isRateLimited ? "text-red-400" : "opacity-70"
                                                                            )}>{remaining}% left</span>
                                                                        </div>
                                                                    </button>
                                                                );
                                                            })}
                                                            {accountModels.length === 0 && (
                                                                <div className="col-span-2 text-[10px] text-zinc-600 italic text-center py-2">
                                                                    No usage info
                                                                </div>
                                                            )}
                                                        </div>
                                                    </motion.div>
                                                )}
                                            </AnimatePresence>
                                        </motion.div>
                                    );
                                })
                            )}
                        </div>
                    </motion.div>
                )}
            </AnimatePresence>
        </div>
    );
};

export default SchedulingSettings;
