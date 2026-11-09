import { useMemo, useState } from 'react';
import { motion } from 'framer-motion';
import { RefreshCw, Lock, Ban, Diamond, Gem, Zap, Circle, Clock, Tag, Power, MoreHorizontal, BookCopy, Check, Trash2 } from 'lucide-react';
import { Account, ActiveClientsStatus, getAccountTier, getEffectiveModelPriority, hasAccountArgonCapability, hasCustomModelPriorities } from '../../types/account';
import { cn } from '../../utils/cn';
import { useTranslation } from 'react-i18next';
import { useConfigStore } from '../../stores/useConfigStore';
import { useAccountStore } from '../../stores/useAccountStore';
import { usePedalModeStore } from '../../stores/usePedalModeStore';
import { GroupQuotaCard } from './GroupQuotaCard';
import { getValidationBlockedStatusLabel } from './accountValidationStatus';
import { showToast } from '../common/ToastContainer';
import { exportAccounts, releaseAccountQoSJail } from '../../services/accountService';
import { copyToClipboard } from '../../utils/clipboard';
import AccountSwitchMenu from './AccountSwitchMenu';
import { QoSStatusBadge } from './QoSStatusBadge';
import { getLiveLimitForModel } from '../../utils/liveLimit';
import { getGroupQuotaSummaries } from '../../utils/quotaDisplay';
import { isValidationBlockActive } from '../../utils/accountStatus';
import { parseTags } from '../../utils/tagUtils';
import AccountActionsModal from './AccountActionsModal';

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
    const { active: isPedalActive, targetAccountIds } = usePedalModeStore();
    const isPedaling = isPedalActive && targetAccountIds.includes(account.id);
    const isDisabled = Boolean(account.disabled);
    const isBanned = Boolean(account.banned);
    const isValidationBlocked = isValidationBlockActive(account);
    const validationBlockedLabel = getValidationBlockedStatusLabel(account.validation_blocked_reason, t);

    // Modal states
    const [isActionsModalOpen, setIsActionsModalOpen] = useState(false);
    const [isCopied, setIsCopied] = useState(false);

    const handleCopyCredentials = async (e: React.MouseEvent) => {
        e.stopPropagation();
        try {
            const response = await exportAccounts([account.id]);
            if (response.accounts && response.accounts.length > 0) {
                const content = JSON.stringify(response.accounts, null, 2);
                const ok = await copyToClipboard(content);
                if (ok) {
                    setIsCopied(true);
                    showToast(t('accounts.credentials_copied', '账号凭据已复制到剪贴板'), 'success');
                    setTimeout(() => setIsCopied(false), 1500);
                }
            } else {
                showToast(t('accounts.no_credentials_found', '未获取到账号凭据'), 'warning');
            }
        } catch (err) {
            showToast(`${t('common.error')}: ${err}`, 'error');
        }
    };

    const isClassic = activeClients?.classic_account_id === account.id;
    const isIde = activeClients?.ide_account_id === account.id;
    const isAgy = activeClients?.agy_account_id === account.id;
    const isAcp = activeClients?.acp_account_id === account.id;
    const isCurrent = propIsCurrent || isClassic || isIde || isAgy || isAcp;

    const handleReleaseJail = async () => {
        try {
            await releaseAccountQoSJail(account.id);
            showToast(t('accounts.qos.release_success', '已解除该账号的小黑屋限制'), 'success');
            await useAccountStore.getState().fetchAccounts();
        } catch (err: any) {
            showToast(err?.message || '解除失败', 'error');
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

    const tags = useMemo(() => parseTags(account.custom_label), [account.custom_label]);

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
                isPedaling
                    ? "bg-amber-50/40 dark:bg-amber-950/20 border-amber-300 dark:border-amber-500/50 ring-2 ring-amber-400/60 shadow-xs"
                    : (isCurrent
                        ? "bg-blue-50/30 border-blue-200 dark:bg-blue-900/10 dark:border-blue-900/30"
                        : "bg-white dark:bg-base-100 border-gray-200 dark:border-base-300"),
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
                    <div className="flex items-center gap-1.5 min-w-0">
                        <h3 className={cn(
                            "font-semibold text-sm truncate flex-1",
                            isCurrent ? "text-blue-700 dark:text-blue-400" : "text-gray-900 dark:text-base-content"
                        )} title={account.email}>
                            {account.email}
                        </h3>
                        <button
                            type="button"
                            onClick={handleCopyCredentials}
                            className="p-1 rounded hover:bg-gray-100 dark:hover:bg-base-200 text-gray-400 hover:text-blue-600 dark:hover:text-blue-400 transition-colors shrink-0"
                            title={t('accounts.copy_credentials_tooltip', '复制账号凭据 (JSON)')}
                        >
                            {isCopied ? (
                                <Check className="w-3.5 h-3.5 text-emerald-500" />
                            ) : (
                                <BookCopy className="w-3.5 h-3.5" />
                            )}
                        </button>
                        <button
                            type="button"
                            onClick={(e) => {
                                e.stopPropagation();
                                onDelete();
                            }}
                            className="p-1 rounded hover:bg-red-50 dark:hover:bg-red-950/30 text-gray-400 hover:text-red-600 dark:hover:text-red-400 transition-colors shrink-0"
                            title={t('accounts.delete_account_tooltip', '删除此账号')}
                        >
                            <Trash2 className="w-3.5 h-3.5" />
                        </button>
                    </div>
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
                            {isAcp && (
                                <span
                                    className="px-1.5 py-0.5 rounded-md bg-violet-100 dark:bg-violet-900/40 text-violet-700 dark:text-violet-300 text-[9px] font-bold shadow-sm border border-violet-200/50"
                                    title={t('accounts.client_acp_tooltip', 'Active account in Antigravity IDE Plugin (ACP)')}
                                >
                                    {t('accounts.client_acp', 'Plugin')}
                                </span>
                            )}
                            {!isClassic && !isIde && !isAgy && !isAcp && propIsCurrent && (
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
                            {isBanned && (
                                <span
                                    className="px-1.5 py-0.5 rounded-md bg-red-100 dark:bg-red-900/40 text-red-700 dark:text-red-300 text-[9px] font-bold flex items-center gap-1 shadow-sm border border-red-200/50"
                                    title={account.banned_reason || t('accounts.banned_tooltip', '账号已被上游封禁（违反服务条款），请提交申诉')}
                                >
                                    <Ban className="w-2.5 h-2.5" />
                                    {t('accounts.banned', '已封禁').toUpperCase()}
                                </span>
                            )}
                            {account.proxy_disabled && !isDisabled && !isBanned && (
                                <span
                                    className="px-1.5 py-0.5 rounded-md bg-orange-100 dark:bg-orange-900/40 text-orange-700 dark:text-orange-300 text-[9px] font-bold flex items-center gap-1 shadow-sm border border-orange-200/50"
                                >
                                    <Ban className="w-2.5 h-2.5" />
                                    {t('accounts.proxy_disabled').toUpperCase()}
                                </span>
                            )}
                            {account.quota?.is_forbidden && !isBanned && (
                                <span className="px-1.5 py-0.5 rounded-md bg-red-100 dark:bg-red-900/40 text-red-600 dark:text-red-400 text-[9px] font-bold flex items-center gap-1 shadow-sm border border-red-200/50">
                                    <Lock className="w-2.5 h-2.5" />
                                    {t('accounts.forbidden').toUpperCase()}
                                </span>
                            )}
                            {isValidationBlocked && (
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
                                } else if (tier === 'enterprise') {
                                    return (
                                        <span className="flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-gradient-to-r from-emerald-600 to-teal-600 text-white text-[9px] font-bold shadow-sm" title={account.token?.project_id ? `GCP: ${account.token.project_id}` : 'GCP / Vertex AI Enterprise'}>
                                            <Diamond className="w-2.5 h-2.5 fill-current" />
                                            ENTERPRISE
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
                            {/* 蹬额度中高亮标识 */}
                            {isPedaling && (
                                <span
                                    className="px-1.5 py-0.5 rounded-md bg-amber-500/15 text-amber-800 dark:text-amber-300 text-[9px] font-bold flex items-center gap-1 shadow-xs border border-amber-500/40 animate-pulse motion-reduce:animate-none"
                                    title={t('pedal.account_pedaling_tooltip', '该账号当前处于蹬额度模式，已被设为最高调度优先级 (priority = 1)')}
                                >
                                    <Zap className="w-2.5 h-2.5 fill-current text-amber-500" />
                                    {t('pedal.account_pedaling_badge', '⚡ 蹬额度中')}
                                </span>
                            )}
                            {/* 优先级 */}
                            <span
                                className={cn(
                                    "px-1.5 py-0.5 rounded-md text-[9px] font-bold",
                                    isPedaling
                                        ? "bg-amber-100 dark:bg-amber-900/50 text-amber-800 dark:text-amber-200 border border-amber-300/80 dark:border-amber-700/80 shadow-2xs"
                                        : hasCustomModelPriorities(account)
                                            ? "bg-indigo-50 dark:bg-indigo-900/30 text-indigo-700 dark:text-indigo-300 border border-indigo-200/60 dark:border-indigo-800/50"
                                            : "bg-gray-100 dark:bg-white/10 text-gray-500 dark:text-gray-400"
                                )}
                                title={
                                    hasCustomModelPriorities(account)
                                        ? `Gemini: ${getEffectiveModelPriority(account, 'gemini')} | Claude: ${getEffectiveModelPriority(account, 'claude')} (${t('accounts.priority')}: ${account.priority ?? 50})`
                                        : t('accounts.priority_hint')
                                }
                            >
                                {hasCustomModelPriorities(account)
                                    ? `${t('accounts.priority')}: G:${getEffectiveModelPriority(account, 'gemini')} · C:${getEffectiveModelPriority(account, 'claude')}`
                                    : `${t('accounts.priority')}: ${account.priority ?? 50}`}
                            </span>
                            {/* 多标签平铺渲染 (最多显示 2 个，超出的显示 +N 徽章) */}
                            {tags.length > 0 && (
                                <>
                                    {tags.slice(0, 2).map((tag, idx) => (
                                        <span key={`${tag}-${idx}`} className="inline-flex items-center gap-1 text-[9px] px-1.5 py-0.5 rounded-md font-bold bg-orange-100 text-orange-700 dark:bg-orange-900/40 dark:text-orange-300 border border-orange-200/50 dark:border-orange-800/50 shadow-sm">
                                            <Tag className="w-2.5 h-2.5" />
                                            {tag}
                                        </span>
                                    ))}
                                    {tags.length > 2 && (
                                        <span className="text-[9px] px-1.5 py-0.5 rounded-md text-gray-500 bg-gray-100 dark:bg-base-200 border border-gray-200 dark:border-base-300 font-bold" title={tags.slice(2).join(', ')}>
                                            +{tags.length - 2}
                                        </span>
                                    )}
                                </>
                            )}
                            {/* 核心模型能力直观徽章 */}
                            {modelCapabilities.hasArgon && (
                                <span
                                    className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-gradient-to-r from-purple-500/15 to-indigo-500/15 text-purple-700 dark:text-purple-300 text-[9px] font-bold border border-purple-200/60 dark:border-purple-800/50"
                                >
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
                            {account.forbidden_models && account.forbidden_models.length > 0 && (
                                account.forbidden_models.map((fm) => (
                                    <span
                                        key={fm}
                                        className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-rose-500/10 text-rose-600 dark:text-rose-400 text-[9px] font-bold border border-rose-200/50 dark:border-rose-900/30"
                                        title={t('accounts.forbidden_model_tooltip', '该模型已被 403 冻结')}
                                    >
                                        ❄️ {fm}
                                    </span>
                                ))
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
                {isDisabled || account.banned || account.quota?.is_forbidden || isValidationBlocked ? (
                    <div className="flex flex-wrap items-center justify-center gap-x-3 gap-y-1 h-full py-4 text-center">
                        <div className={cn(
                            "flex items-center gap-1.5",
                            isValidationBlocked && !account.banned ? "text-amber-600 dark:text-amber-400" : "text-red-600 dark:text-red-400"
                        )}>
                            {isValidationBlocked && !account.banned ? <Clock className="w-4 h-4" /> : (account.banned ? <Ban className="w-4 h-4" /> : (isDisabled ? <Ban className="w-4 h-4" /> : <Lock className="w-4 h-4" />))}
                            <span className="text-[11px] font-bold">
                                {isValidationBlocked && !account.banned ? validationBlockedLabel : (account.banned ? t('accounts.banned_msg', '账号已被上游封禁，已停止自动刷新') : (isDisabled ? t('accounts.status.disabled') : t('accounts.forbidden_msg')))}
                            </span>
                        </div>
                        <div className={cn(
                            "w-px h-3 hidden sm:block",
                            isValidationBlocked && !account.banned ? "bg-amber-200 dark:bg-amber-800/50" : "bg-red-200 dark:bg-red-800/50"
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

            {/* Footer: 4 Action Buttons in 2x2 Grid */}
            <div className="grid grid-cols-2 gap-1.5 w-full pt-2 pb-1 border-t border-gray-100 dark:border-base-200">
                {/* 1. 切换（点击向上展开 4 个客户端微标选项） */}
                <AccountSwitchMenu
                    accountId={account.id}
                    disabled={Boolean(isDisabled || isBanned)}
                    isSwitching={isSwitching}
                    activeClients={activeClients}
                    onSwitch={onSwitch}
                    size="md"
                />

                {/* 2. 刷新 */}
                <button
                    type="button"
                    disabled={isRefreshing || isDisabled}
                    onClick={(e) => { e.stopPropagation(); onRefresh(); }}
                    className={cn(
                        "flex items-center justify-center gap-1.5 px-2 py-1.5 rounded-lg text-xs font-medium transition-all",
                        (isRefreshing || isDisabled)
                            ? "bg-gray-100 dark:bg-base-200 text-gray-400 dark:text-gray-600 cursor-not-allowed"
                            : "bg-gray-50 hover:bg-emerald-50 dark:bg-base-200/50 dark:hover:bg-emerald-900/30 text-gray-700 dark:text-gray-300 hover:text-emerald-600 dark:hover:text-emerald-400 border border-gray-200/60 dark:border-base-300"
                    )}
                    title={isDisabled ? t('accounts.disabled_tooltip') : t('accounts.refresh_quota_and_ban', '刷新配额并检测封禁')}
                >
                    <RefreshCw className={cn("w-3.5 h-3.5", isRefreshing && "animate-spin")} />
                    <span>{t('accounts.action_refresh', '刷新')}</span>
                </button>

                {/* 3. 启停 */}
                <button
                    type="button"
                    disabled={isBanned}
                    onClick={(e) => { e.stopPropagation(); onToggleProxy(); }}
                    className={cn(
                        "flex items-center justify-center gap-1.5 px-2 py-1.5 rounded-lg text-xs font-medium transition-all",
                        isBanned
                            ? "bg-gray-100 dark:bg-base-200 text-gray-400 dark:text-gray-600 cursor-not-allowed"
                            : account.proxy_disabled
                                ? "bg-amber-50 hover:bg-amber-100 dark:bg-amber-950/30 dark:hover:bg-amber-900/40 text-amber-700 dark:text-amber-300 border border-amber-200/80 dark:border-amber-800/50"
                                : "bg-emerald-50 hover:bg-emerald-100 dark:bg-emerald-950/30 dark:hover:bg-emerald-900/40 text-emerald-700 dark:text-emerald-300 border border-emerald-200/80 dark:border-emerald-800/50"
                    )}
                    title={isBanned ? t('accounts.banned_tooltip', '账号已被上游封禁') : (account.proxy_disabled ? t('accounts.enable_proxy', '启用反代') : t('accounts.disable_proxy', '禁用反代'))}
                >
                    <Power className="w-3.5 h-3.5" />
                    <span>{account.proxy_disabled ? t('accounts.action_enable', '启用') : t('accounts.action_disable', '停用')}</span>
                </button>

                {/* 4. 更多 */}
                <button
                    type="button"
                    onClick={(e) => { e.stopPropagation(); setIsActionsModalOpen(true); }}
                    className="flex items-center justify-center gap-1.5 px-2 py-1.5 rounded-lg text-xs font-medium transition-all bg-gray-50 hover:bg-indigo-50 dark:bg-base-200/50 dark:hover:bg-indigo-900/30 text-gray-700 dark:text-gray-300 hover:text-indigo-600 dark:hover:text-indigo-400 border border-gray-200/60 dark:border-base-300"
                    title={t('common.more', '更多操作')}
                >
                    <MoreHorizontal className="w-3.5 h-3.5" />
                    <span>{t('accounts.action_more', '更多')}</span>
                </button>
            </div>

            {/* Modals */}
            <AccountActionsModal
                isOpen={isActionsModalOpen}
                onClose={() => setIsActionsModalOpen(false)}
                account={account}
                onSwitch={onSwitch}
                onWarmup={onWarmup}
                onViewDevice={() => {
                    setIsActionsModalOpen(false);
                    onViewDevice();
                }}
                onViewDetails={() => {
                    setIsActionsModalOpen(false);
                    onViewDetails();
                }}
                onExport={onExport}
                onUpdateLabel={onUpdateLabel}
                onDelete={() => {
                    setIsActionsModalOpen(false);
                    onDelete();
                }}
                isSwitching={isSwitching}
                isRefreshing={isRefreshing}
            />
        </motion.div>
    );
}

export default AccountCard;
