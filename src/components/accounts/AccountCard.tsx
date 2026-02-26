import { useMemo, useState } from 'react';
import { ArrowRightLeft, RefreshCw, Trash2, Download, Info, Lock, Ban, Diamond, Gem, Circle, ToggleLeft, ToggleRight, Fingerprint, Sparkles, Tag, X, Check, Clock, Bot } from 'lucide-react';
import { Account } from '../../types/account';
import { cn } from '../../utils/cn';
import { useTranslation } from 'react-i18next';
import { useConfigStore } from '../../stores/useConfigStore';
import { QuotaItem } from './QuotaItem';
import { MODEL_CONFIG, sortModels } from '../../config/modelConfig';

interface AccountCardProps {
    account: Account;
    selected: boolean;
    onSelect: () => void;
    isCurrent: boolean;
    isRefreshing: boolean;
    isSwitching?: boolean;
    onSwitch: () => void;
    onRefresh: () => void;
    onViewDevice: () => void;
    onViewDetails: () => void;
    onExport: () => void;
    onDelete: () => void;
    onToggleProxy: () => void;
    onWarmup?: () => void;
    onUpdateLabel?: (label: string) => void;
    onViewError: () => void;
}

// 使用统一的模型配置
const DEFAULT_MODELS = Object.entries(MODEL_CONFIG).map(([id, config]) => ({
    id,
    label: config.label,
    protectedKey: config.protectedKey,
    Icon: config.Icon
}));

function AccountCard({ account, selected, onSelect, isCurrent: propIsCurrent, isRefreshing, isSwitching = false, onSwitch, onRefresh, onViewDetails, onExport, onDelete, onToggleProxy, onViewDevice, onWarmup, onUpdateLabel, onViewError }: AccountCardProps) {
    const { t } = useTranslation();
    const { config, showAllQuotas } = useConfigStore();
    const isDisabled = Boolean(account.disabled);

    // 自定义标签编辑状态
    const [isEditingLabel, setIsEditingLabel] = useState(false);
    const [labelInput, setLabelInput] = useState(account.custom_label || '');

    // Use the prop directly from parent component
    const isCurrent = propIsCurrent;

    const handleSaveLabel = () => {
        if (onUpdateLabel) {
            onUpdateLabel(labelInput.trim());
        }
        setIsEditingLabel(false);
    };

    const handleCancelLabel = () => {
        setLabelInput(account.custom_label || '');
        setIsEditingLabel(false);
    };

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === 'Enter') {
            handleSaveLabel();
        } else if (e.key === 'Escape') {
            handleCancelLabel();
        }
    };

    const displayModels = useMemo(() => {
        // Build map of friendly labels and icons from DEFAULT_MODELS
        const iconMap = new Map(DEFAULT_MODELS.map(m => [m.id, m.Icon]));

        // Get all models from account (source of truth)
        const accountModels = account.quota?.models?.map(m => {
            // 注意：DEFAULT_MODELS 现在应该包含 shortLabel，我们需要确保它被正确映射
            // 但 DEFAULT_MODELS 是从 MODEL_CONFIG 生成的，我们需要确保它包含 shortLabel
            // 这里为了安全，直接从 MODEL_CONFIG 获取
            const fullConfig = MODEL_CONFIG[m.name.toLowerCase()];
            return {
                id: m.name,
                label: m.display_name || fullConfig?.shortLabel || fullConfig?.label || m.name,
                protectedKey: fullConfig?.protectedKey || m.name,
                Icon: iconMap.get(m.name) || Bot,
                data: m
            };
        }) || [];

        let models: typeof accountModels;

        if (showAllQuotas) {
            models = accountModels;
        } else {
            // Filter for pinned or defaults
            const pinned = config?.pinned_quota_models?.models;
            if (pinned && pinned.length > 0) {
                models = accountModels.filter(m => pinned.includes(m.id));
            } else {
                // Default fallback: show known default models, plus we show all dynamic pinned models
                // 暂时退化：如果没有 config 就不阻拦了？不，没有 pinned 就显示内置+有 display_name 的。
                models = accountModels.filter(m => DEFAULT_MODELS.some(d => d.id === m.id) || m.data.display_name);
            }
        }

        // 应用排序并过滤过期模型
        return sortModels(models).filter(m => m.id !== 'claude-sonnet-4-6-thinking' && m.id !== 'claude-sonnet-4-5-thinking' && m.id !== 'claude-opus-4-5-thinking');
    }, [config, account, showAllQuotas]);

    const isModelProtected = (key?: string) => {
        if (!key) return false;
        return account.protected_models?.includes(key);
    };

    return (
        <div className={cn(
            "flex flex-col p-3 rounded-xl border transition-all hover:shadow-md",
            isCurrent
                ? "bg-blue-50/30 border-blue-200 dark:bg-blue-900/10 dark:border-blue-900/30"
                : "bg-white dark:bg-base-100 border-gray-200 dark:border-base-300",
            (isRefreshing || isDisabled) && "opacity-70"
        )}>

            {/* Header: Checkbox + Email + Badges */}
            <div className="flex-none flex items-start gap-3 mb-2">
                <input
                    type="checkbox"
                    className="mt-1 checkbox checkbox-xs rounded border-2 border-gray-400 dark:border-gray-500 checked:border-blue-600 checked:bg-blue-600 [--chkbg:theme(colors.blue.600)] [--chkfg:white]"
                    checked={selected}
                    onChange={() => onSelect()}
                    onClick={(e) => e.stopPropagation()}
                />
                <div className="flex-1 min-w-0 flex flex-col gap-1.5">
                    <h3 className={cn(
                        "font-semibold text-sm truncate w-full",
                        isCurrent ? "text-blue-700 dark:text-blue-400" : "text-gray-900 dark:text-base-content"
                    )} title={account.email}>
                        {account.email}
                    </h3>
                    <div className="flex items-center justify-between w-full gap-2">
                        <div className="flex items-center gap-1.5 flex-wrap">
                            {isCurrent && (
                                <span className="px-1.5 py-0.5 rounded-md bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300 text-[9px] font-bold shadow-sm border border-blue-200/50">
                                    {t('accounts.current').toUpperCase()}
                                </span>
                            )}
                            {isDisabled && (
                                <span
                                    className="px-1.5 py-0.5 rounded-md bg-rose-100 dark:bg-rose-900/40 text-rose-700 dark:text-rose-300 text-[9px] font-bold flex items-center gap-1 shadow-sm border border-rose-200/50"
                                >
                                    <Ban className="w-2.5 h-2.5" />
                                    {t('accounts.disabled').toUpperCase()}
                                </span>
                            )}
                            {account.proxy_disabled && (
                                <span
                                    className="px-1.5 py-0.5 rounded-md bg-orange-100 dark:bg-orange-900/40 text-orange-700 dark:text-orange-300 text-[9px] font-bold flex items-center gap-1 shadow-sm border border-orange-200/50"
                                >
                                    <Ban className="w-2.5 h-2.5" />
                                    {t('accounts.proxy_disabled').toUpperCase()}
                                </span>
                            )}
                            {account.quota?.is_forbidden && (
                                <span className="px-1.5 py-0.5 rounded-md bg-red-100 dark:bg-red-900/40 text-red-600 dark:text-red-400 text-[9px] font-bold flex items-center gap-1 shadow-sm border border-red-200/50">
                                    <Lock className="w-2.5 h-2.5" />
                                    {t('accounts.forbidden').toUpperCase()}
                                </span>
                            )}
                            {account.validation_blocked && (
                                <span className="px-1.5 py-0.5 rounded-md bg-amber-100 dark:bg-amber-900/40 text-amber-700 dark:text-amber-400 text-[9px] font-bold flex items-center gap-1 shadow-sm border border-amber-200/50">
                                    <Clock className="w-2.5 h-2.5" />
                                    {t('accounts.status.validation_required').toUpperCase()}
                                </span>
                            )}
                            {/* 订阅类型徽章 */}
                            {account.quota?.subscription_tier && (() => {
                                const tier = account.quota.subscription_tier.toLowerCase();
                                if (tier.includes('ultra')) {
                                    return (
                                        <span className="flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-gradient-to-r from-purple-600 to-pink-600 text-white text-[9px] font-bold shadow-sm">
                                            <Gem className="w-2.5 h-2.5 fill-current" />
                                            ULTRA
                                        </span>
                                    );
                                } else if (tier.includes('pro')) {
                                    return (
                                        <span className="flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-gradient-to-r from-blue-600 to-indigo-600 text-white text-[9px] font-bold shadow-sm">
                                            <Diamond className="w-2.5 h-2.5 fill-current" />
                                            PRO
                                        </span>
                                    );
                                } else {
                                    return (
                                        <span className="flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-gray-100 dark:bg-white/10 text-gray-500 dark:text-gray-400 text-[9px] font-bold shadow-sm border border-gray-200 dark:border-white/10">
                                            <Circle className="w-2.5 h-2.5" />
                                            FREE
                                        </span>
                                    );
                                }
                            })()}
                            {/* 自定义标签 */}
                            {account.custom_label && (
                                <span className="flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-orange-100 dark:bg-orange-900/40 text-orange-700 dark:text-orange-300 text-[9px] font-bold shadow-sm border border-orange-200/50 dark:border-orange-800/50">
                                    <Tag className="w-2.5 h-2.5" />
                                    {account.custom_label}
                                </span>
                            )}
                        </div>
                        <span className="text-[10px] text-gray-400 dark:text-gray-500 font-mono shrink-0 whitespace-nowrap">
                            {new Date(account.last_used * 1000).toLocaleString([], { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' })}
                        </span>
                    </div>
                </div>
            </div>


            {/* 配额展示 */}
            <div className="flex-1 px-2 mb-2 overflow-y-auto scrollbar-none">
                {isDisabled || account.quota?.is_forbidden || account.proxy_disabled || account.validation_blocked ? (
                    <div className="flex flex-wrap items-center justify-center gap-x-3 gap-y-1 h-full py-4 text-center">
                        <div className={cn(
                            "flex items-center gap-1.5",
                            account.validation_blocked ? "text-amber-600 dark:text-amber-400" : "text-red-600 dark:text-red-400"
                        )}>
                            {account.validation_blocked ? <Clock className="w-4 h-4" /> : (isDisabled || account.proxy_disabled ? <Ban className="w-4 h-4" /> : <Lock className="w-4 h-4" />)}
                            <span className="text-[11px] font-bold">
                                {account.validation_blocked ? t('accounts.status.validation_required') : (isDisabled ? t('accounts.status.disabled') : account.proxy_disabled ? t('accounts.status.proxy_disabled') : t('accounts.forbidden_msg'))}
                            </span>
                        </div>
                        <div className={cn(
                            "w-px h-3 hidden sm:block",
                            account.validation_blocked ? "bg-amber-200 dark:bg-amber-800/50" : "bg-red-200 dark:bg-red-800/50"
                        )} />
                        <button
                            onClick={(e) => { e.stopPropagation(); onViewError(); }}
                            className="text-[10px] text-blue-600 dark:text-blue-400 hover:underline font-medium"
                        >
                            {t('accounts.view_error')}
                        </button>
                    </div>
                ) : (
                    <div className="grid grid-cols-1 gap-2 content-start">
                        {displayModels.map((model) => (
                            <QuotaItem
                                key={model.id}
                                label={model.label}
                                percentage={model.data?.percentage || 0}
                                resetTime={model.data?.reset_time}
                                isProtected={isModelProtected(model.protectedKey)}
                                Icon={model.Icon}
                            />
                        ))}
                    </div>
                )}
            </div>

            {/* Footer: Actions Only */}
            <div className="flex-none flex items-center justify-center pt-2 pb-1 border-t border-gray-100 dark:border-base-200">
                {/* 标签编辑弹出框 */}
                {isEditingLabel && (
                    <div className="absolute inset-0 bg-white/95 dark:bg-base-100/95 rounded-xl z-10 flex items-center justify-center p-4">
                        <div className="flex items-center gap-2 w-full max-w-xs">
                            <input
                                type="text"
                                className="flex-1 px-2 py-1 text-sm border border-orange-300 dark:border-orange-700 rounded-md focus:outline-none focus:ring-2 focus:ring-orange-500 bg-white dark:bg-base-200"
                                placeholder={t('accounts.custom_label_placeholder', 'Enter custom label')}
                                value={labelInput}
                                onChange={(e) => setLabelInput(e.target.value)}
                                onKeyDown={handleKeyDown}
                                autoFocus
                                maxLength={15}
                            />
                            <button
                                className="p-1.5 text-green-600 hover:bg-green-50 dark:hover:bg-green-900/30 rounded-lg transition-all"
                                onClick={handleSaveLabel}
                                title={t('common.save', 'Save')}
                            >
                                <Check className="w-4 h-4" />
                            </button>
                            <button
                                className="p-1.5 text-gray-400 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-all"
                                onClick={handleCancelLabel}
                                title={t('common.cancel', 'Cancel')}
                            >
                                <X className="w-4 h-4" />
                            </button>
                        </div>
                    </div>
                )}
                <div className="flex flex-wrap items-center justify-center gap-1 w-full">
                    <button
                        className="p-1.5 text-gray-400 hover:text-sky-600 dark:hover:text-sky-400 hover:bg-sky-50 dark:hover:bg-sky-900/30 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); onViewDetails(); }}
                        title={t('common.details')}
                    >
                        <Info className="w-3.5 h-3.5" />
                    </button>
                    <button
                        className="p-1.5 text-gray-400 hover:text-indigo-600 dark:hover:text-indigo-400 hover:bg-indigo-50 dark:hover:bg-indigo-900/30 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); onViewDevice(); }}
                        title={t('accounts.device_fingerprint')}
                    >
                        <Fingerprint className="w-3.5 h-3.5" />
                    </button>
                    {/* 自定义标签按钮 */}
                    {onUpdateLabel && (
                        <button
                            className={cn(
                                "p-1.5 rounded-lg transition-all",
                                account.custom_label
                                    ? "text-orange-500 hover:text-orange-600 hover:bg-orange-50 dark:hover:bg-orange-900/30"
                                    : "text-gray-400 hover:text-orange-500 hover:bg-orange-50 dark:hover:bg-orange-900/30"
                            )}
                            onClick={(e) => { e.stopPropagation(); setIsEditingLabel(true); }}
                            title={t('accounts.edit_label', 'Edit Label')}
                        >
                            <Tag className="w-3.5 h-3.5" />
                        </button>
                    )}
                    <button
                        className={`p-1.5 rounded-lg transition-all ${(isSwitching || isDisabled) ? 'text-blue-600 bg-blue-50 dark:text-blue-400 dark:bg-blue-900/10 cursor-not-allowed' : 'text-gray-400 hover:text-blue-600 dark:hover:text-blue-400 hover:bg-blue-50 dark:hover:bg-blue-900/30'}`}
                        onClick={(e) => { e.stopPropagation(); onSwitch(); }}
                        title={isDisabled ? t('accounts.disabled_tooltip') : (isSwitching ? t('common.loading') : t('common.switch'))}
                        disabled={isSwitching || isDisabled}
                    >
                        <ArrowRightLeft className={`w-3.5 h-3.5 ${isSwitching ? 'animate-spin' : ''}`} />
                    </button>
                    {onWarmup && (
                        <button
                            className={`p-1.5 rounded-lg transition-all ${(isRefreshing || isDisabled) ? 'text-orange-600 bg-orange-50 dark:bg-orange-900/10 cursor-not-allowed' : 'text-gray-400 hover:text-orange-500 hover:bg-orange-50 dark:hover:bg-orange-900/30'}`}
                            onClick={(e) => { e.stopPropagation(); onWarmup(); }}
                            title={isDisabled ? t('accounts.disabled_tooltip') : (isRefreshing ? t('common.loading') : t('accounts.warmup_this', '预热该账号'))}
                            disabled={isRefreshing || isDisabled}
                        >
                            <Sparkles className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-pulse' : ''}`} />
                        </button>
                    )}
                    <button
                        className={`p-1.5 rounded-lg transition-all ${isRefreshing
                            ? 'text-green-600 bg-green-50'
                            : 'text-gray-400 hover:text-green-600 hover:bg-green-50'}`}
                        onClick={(e) => { e.stopPropagation(); onRefresh(); }}
                        disabled={isRefreshing || isDisabled}
                        title={isDisabled ? t('accounts.disabled_tooltip') : t('common.refresh')}
                    >
                        <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin' : ''}`} />
                    </button>
                    <button
                        className="p-1.5 text-gray-400 hover:text-indigo-600 hover:bg-indigo-50 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); onExport(); }}
                        title={t('common.export')}
                    >
                        <Download className="w-3.5 h-3.5" />
                    </button>
                    <button
                        className={cn(
                            "p-1.5 rounded-lg transition-all",
                            account.proxy_disabled
                                ? "text-gray-400 hover:text-green-600 hover:bg-green-50"
                                : "text-gray-400 hover:text-orange-600 hover:bg-orange-50"
                        )}
                        onClick={(e) => { e.stopPropagation(); onToggleProxy(); }}
                        title={account.proxy_disabled ? t('accounts.enable_proxy') : t('accounts.disable_proxy')}
                    >
                        {account.proxy_disabled ? (
                            <ToggleRight className="w-3.5 h-3.5" />
                        ) : (
                            <ToggleLeft className="w-3.5 h-3.5" />
                        )}
                    </button>
                    <button
                        className="p-1.5 text-gray-400 hover:text-red-600 hover:bg-red-50 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); onDelete(); }}
                        title={t('common.delete')}
                    >
                        <Trash2 className="w-3.5 h-3.5" />
                    </button>
                </div>
            </div>
        </div >
    );
}

export default AccountCard;
