import { useMemo, useState } from 'react';
import { motion } from 'framer-motion';
import { ArrowRightLeft, RefreshCw, Trash2, Download, Info, Lock, Ban, Diamond, Gem, Circle, ToggleLeft, ToggleRight, Fingerprint, Sparkles, Tag, X, Check, Clock, Repeat2, Terminal } from 'lucide-react';
import { Account, ActiveClientsStatus, getAccountTier, hasAccountArgonCapability } from '../../types/account';
import { cn } from '../../utils/cn';
import { useTranslation } from 'react-i18next';
import { useConfigStore } from '../../stores/useConfigStore';
import { useAccountStore } from '../../stores/useAccountStore';
import { GroupQuotaCard } from './GroupQuotaCard';
import { getValidationBlockedStatusLabel } from './accountValidationStatus';
import { showToast } from '../common/ToastContainer';
import { releaseAccountQoSJail } from '../../services/accountService';
import { QoSStatusBadge } from './QoSStatusBadge';
import { getLiveLimitForModel } from '../../utils/liveLimit';
import { getGroupQuotaSummaries } from '../../utils/quotaDisplay';

interface AccountCardProps {
    account: Account;
    selected: boolean;
    onSelect: () => void;
    isCurrent: boolean;
    activeClients?: ActiveClientsStatus;
    isRefreshing: boolean;
    isSwitching?: boolean;
    onSwitch: (targetIde?: string) => void;
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

function AccountCard({ account, selected, onSelect, isCurrent: propIsCurrent, activeClients, isRefreshing, isSwitching = false, onSwitch, onRefresh, onViewDetails, onExport, onDelete, onToggleProxy, onViewDevice, onWarmup, onUpdateLabel, onViewError }: AccountCardProps) {
    const { t } = useTranslation();
    const { config } = useConfigStore();
    const isDisabled = Boolean(account.disabled);
    const validationBlockedLabel = getValidationBlockedStatusLabel(account.validation_blocked_reason, t);

    // 自定义标签编辑状态
    const [isEditingLabel, setIsEditingLabel] = useState(false);
    const [labelInput, setLabelInput] = useState(account.custom_label || '');

    const isClassic = activeClients?.classic_account_id === account.id;
    const isIde = activeClients?.ide_account_id === account.id;
    const isAgy = activeClients?.agy_account_id === account.id;
    const isCurrent = propIsCurrent || isClassic || isIde || isAgy;

    const handleSaveLabel = () => {
        if (onUpdateLabel) {
            onUpdateLabel(labelInput.trim());
        }
        setIsEditingLabel(false);
    };

    const handleReleaseJail = async () => {
        try {
            await releaseAccountQoSJail(account.id);
            showToast(t('accounts.qos.release_success', '已解除该账号的小黑屋限制'), 'success');
            // 解除小黑屋只影响运行时 QoS 状态，与配额无关：直接重拉账号列表立即回显。
            // 若复用 onRefresh 的配额刷新链路，上游 429/503 时 fetchAccountQuota 会抛错，
            // 列表刷新被跳过，界面上的「限速小黑屋中」徽章就会一直残留。
            await useAccountStore.getState().fetchAccounts();
        } catch (err: any) {
            showToast(err?.message || '解除失败', 'error');
        }
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

    const tier = useMemo(() => getAccountTier(account), [account]);
    const groupSummaries = useMemo(() => {
        return getGroupQuotaSummaries(account.quota?.quota_groups, account.quota?.models, tier);
    }, [account.quota?.quota_groups, account.quota?.models, tier]);

    const modelCapabilities = useMemo(() => {
        const supported = Array.from(new Set(groupSummaries.flatMap(s => s.supportedModels || [])));
        const hasArgon = hasAccountArgonCapability(account) || groupSummaries.some(s => s.key === 'argon') || supported.some(m => m.includes('argon'));
        const hasClaude55 = supported.some(m => m.includes('5-5') || m.includes('5.5'));
        const hasClaude46 = supported.some(m => m.includes('4-6') || m.includes('4.6'));
        const hasGpt = supported.some(m => m.startsWith('gpt'));
        return { hasArgon, hasClaude55, hasClaude46, hasGpt };
    }, [account, groupSummaries]);

    const isGroupProtected = (key: 'gemini' | 'claude' | 'argon') => {
        if (!config?.quota_protection?.enabled) return false;
        if (!account.protected_models || account.protected_models.length === 0) return false;
        if (key === 'argon') {
            return account.protected_models.some(m => m.toLowerCase().includes('argon'));
        }
        if (key === 'gemini') {
            return account.protected_models.some(m => m.toLowerCase().startsWith('gemini') && !m.toLowerCase().includes('argon'));
        } else {
            return account.protected_models.some(m => m.toLowerCase().startsWith('claude') || m.toLowerCase().startsWith('gpt'));
        }
    };

    return (
        <motion.div
            whileHover={{ y: -2 }}
            transition={{ duration: 0.15 }}
            className={cn(
                "flex flex-col p-3 rounded-xl border transition-all hover:shadow-md",
                isCurrent
                    ? "bg-blue-50/30 border-blue-200 dark:bg-blue-900/10 dark:border-blue-900/30"
                    : "bg-white dark:bg-base-100 border-gray-200 dark:border-base-300",
                (isRefreshing || isDisabled) && "opacity-70"
            )}
        >

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
                            {isClassic && (
                                <span
                                    className="px-1.5 py-0.5 rounded-md bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300 text-[9px] font-bold shadow-sm border border-blue-200/50"
                                    title={t('accounts.client_classic_tooltip', '当前 Antigravity 经典款客户端登录账号')}
                                >
                                    {t('accounts.client_classic', '经典款')}
                                </span>
                            )}
                            {isIde && (
                                <span
                                    className="px-1.5 py-0.5 rounded-md bg-sky-100 dark:bg-sky-900/40 text-sky-700 dark:text-sky-300 text-[9px] font-bold shadow-sm border border-sky-200/50"
                                    title={t('accounts.client_ide_tooltip', '当前 Antigravity IDE 客户端登录账号')}
                                >
                                    {t('accounts.client_ide', 'IDE')}
                                </span>
                            )}
                            {isAgy && (
                                <span
                                    className="px-1.5 py-0.5 rounded-md bg-emerald-100 dark:bg-emerald-900/40 text-emerald-700 dark:text-emerald-300 text-[9px] font-bold shadow-sm border border-emerald-200/50"
                                    title={t('accounts.client_cli_tooltip', '当前 Antigravity CLI (agy) 登录账号')}
                                >
                                    {t('accounts.client_cli', 'CLI')}
                                </span>
                            )}
                            {!isClassic && !isIde && !isAgy && propIsCurrent && (
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
                            {account.proxy_disabled && !isDisabled && (
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
                                    {validationBlockedLabel.toUpperCase()}
                                </span>
                            )}
                            <QoSStatusBadge
                                status={account.qos_status}
                                resetSeconds={account.qos_reset_seconds}
                                size="sm"
                                onRelease={handleReleaseJail}
                            />
                            {/* 订阅类型徽章 */}
                            {(() => {
                                const tier = getAccountTier(account);
                                if (tier === 'ultra') {
                                    return (
                                        <span className="flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-gradient-to-r from-purple-600 to-pink-600 text-white text-[9px] font-bold shadow-sm">
                                            <Gem className="w-2.5 h-2.5 fill-current" />
                                            ULTRA
                                        </span>
                                    );
                                } else if (tier === 'pro') {
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
                            <span className="px-1.5 py-0.5 rounded-md bg-gray-100 dark:bg-white/10 text-gray-500 dark:text-gray-400 text-[9px] font-bold" title={t('accounts.priority_hint')}>
                                {t('accounts.priority')}: {account.priority ?? 50}
                            </span>
                            {account.custom_label && (
                                <span className="flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-orange-100 dark:bg-orange-900/40 text-orange-700 dark:text-orange-300 text-[9px] font-bold shadow-sm border border-orange-200/50 dark:border-orange-800/50">
                                    <Tag className="w-2.5 h-2.5" />
                                    {account.custom_label}
                                </span>
                            )}
                            {/* 核心模型能力直观徽章 */}
                            {modelCapabilities.hasArgon && (
                                <span
                                    className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-gradient-to-r from-purple-500/15 to-indigo-500/15 text-purple-700 dark:text-purple-300 text-[9px] font-bold border border-purple-200/60 dark:border-purple-800/50"
                                >
                                    <Sparkles className="w-2.5 h-2.5 text-purple-500" />
                                    Argon
                                </span>
                            )}
                            {modelCapabilities.hasClaude55 && (
                                <span
                                    className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-amber-500/15 text-amber-700 dark:text-amber-300 text-[9px] font-bold border border-amber-200/60 dark:border-amber-800/50"
                                >
                                    Claude 5.5
                                </span>
                            )}
                            {modelCapabilities.hasClaude46 && !modelCapabilities.hasClaude55 && (
                                <span
                                    className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-amber-500/10 text-amber-600/90 dark:text-amber-400/90 text-[9px] font-bold border border-amber-200/40 dark:border-amber-800/30"
                                >
                                    Claude 4.6
                                </span>
                            )}
                            {modelCapabilities.hasGpt && (
                                <span
                                    className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-emerald-500/15 text-emerald-700 dark:text-emerald-300 text-[9px] font-bold border border-emerald-200/60 dark:border-emerald-800/50"
                                >
                                    GPT-120B
                                </span>
                            )}
                        </div>
                        <span className="text-[10px] text-gray-400 dark:text-gray-500 font-mono shrink-0 whitespace-nowrap">
                            {new Date(account.last_used * 1000).toLocaleString([], { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' })}
                        </span>
                    </div>
                </div>
            </div>


            {/* 配额展示 (统一分组展示) */}
            <div className="flex-1 px-1 mb-2 overflow-y-auto scrollbar-none">
                {isDisabled || account.quota?.is_forbidden || account.validation_blocked ? (
                    <div className="flex flex-wrap items-center justify-center gap-x-3 gap-y-1 h-full py-4 text-center">
                        <div className={cn(
                            "flex items-center gap-1.5",
                            account.validation_blocked ? "text-amber-600 dark:text-amber-400" : "text-red-600 dark:text-red-400"
                        )}>
                            {account.validation_blocked ? <Clock className="w-4 h-4" /> : (isDisabled ? <Ban className="w-4 h-4" /> : <Lock className="w-4 h-4" />)}
                            <span className="text-[11px] font-bold">
                                {account.validation_blocked ? validationBlockedLabel : (isDisabled ? t('accounts.status.disabled') : t('accounts.forbidden_msg'))}
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
                    <div className="flex flex-col gap-1.5 content-start">
                        <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                            {groupSummaries.filter(s => s.key !== 'argon').map((summary) => (
                                <GroupQuotaCard
                                    key={summary.key}
                                    summary={summary}
                                    isProtected={isGroupProtected(summary.key)}
                                    liveLimit={summary.key === 'gemini' ? getLiveLimitForModel(account, 'gemini-pro') : getLiveLimitForModel(account, 'claude')}
                                />
                            ))}
                        </div>
                        {(() => {
                            const argon = groupSummaries.find(s => s.key === 'argon');
                            if (!argon) return null;
                            return (
                                <div className="w-full">
                                    <GroupQuotaCard
                                        summary={argon}
                                        isProtected={isGroupProtected('argon')}
                                        liveLimit={getLiveLimitForModel(account, 'gemini-4-argon')}
                                        fullWidth
                                    />
                                </div>
                            );
                        })()}
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
                        onClick={(e) => { e.stopPropagation(); onSwitch('classic'); }}
                        title={isDisabled ? t('accounts.disabled_tooltip') : (isSwitching ? t('common.loading') : t('accounts.switch_to_classic', '切换到 Antigravity (经典版)'))}
                        disabled={isSwitching || isDisabled}
                    >
                        <ArrowRightLeft className={`w-3.5 h-3.5 ${isSwitching ? 'animate-spin' : ''}`} />
                    </button>
                    <button
                        className={`p-1.5 rounded-lg transition-all ${(isSwitching || isDisabled) ? 'text-blue-600 bg-blue-50 dark:text-blue-400 dark:bg-blue-900/10 cursor-not-allowed' : 'text-gray-400 hover:text-sky-600 dark:hover:text-sky-400 hover:bg-sky-50 dark:hover:bg-sky-900/30'}`}
                        onClick={(e) => { e.stopPropagation(); onSwitch('ide'); }}
                        title={isDisabled ? t('accounts.disabled_tooltip') : (isSwitching ? t('common.loading') : t('accounts.switch_to_ide', '切换到 Antigravity IDE'))}
                        disabled={isSwitching || isDisabled}
                    >
                        <Repeat2 className={`w-3.5 h-3.5 ${isSwitching ? 'animate-spin' : ''}`} />
                    </button>
                    <button
                        className={`p-1.5 rounded-lg transition-all ${(isSwitching || isDisabled) ? 'text-blue-600 bg-blue-50 dark:text-blue-400 dark:bg-blue-900/10 cursor-not-allowed' : 'text-gray-400 hover:text-emerald-600 dark:hover:text-emerald-400 hover:bg-emerald-50 dark:hover:bg-emerald-900/30'}`}
                        onClick={(e) => { e.stopPropagation(); onSwitch('agy'); }}
                        title={isDisabled ? t('accounts.disabled_tooltip') : (isSwitching ? t('common.loading') : t('accounts.switch_to_agy', '切换到 Antigravity CLI (agy)'))}
                        disabled={isSwitching || isDisabled}
                    >
                        <Terminal className={`w-3.5 h-3.5 ${isSwitching ? 'animate-spin' : ''}`} />
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
                        className="p-1.5 text-gray-400 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); onDelete(); }}
                        title={t('common.delete')}
                    >
                        <Trash2 className="w-3.5 h-3.5" />
                    </button>
                </div>
            </div>
        </motion.div>
    );
}

export default AccountCard;
