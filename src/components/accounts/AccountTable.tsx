/**
 * 账号表格组件
 * 支持拖拽排序功能，用户可以通过拖拽行来调整账号顺序
 */
import { useMemo, useState } from 'react';
import {
    DndContext,
    closestCenter,
    KeyboardSensor,
    PointerSensor,
    useSensor,
    useSensors,
    DragEndEvent,
    DragStartEvent,
    DragOverlay,
} from '@dnd-kit/core';
import {
    arrayMove,
    SortableContext,
    sortableKeyboardCoordinates,
    useSortable,
    verticalListSortingStrategy,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import {
    GripVertical,
    RefreshCw,
    Lock,
    Ban,
    Diamond,
    Gem,
    Circle,
    Sparkles,
    Tag,
    Clock,
    Repeat2,
    ArrowUpDown,
    ArrowUp,
    ArrowDown,
    Zap,
    Power,
    MoreHorizontal,
} from 'lucide-react';
import { type Account, type ActiveClientsStatus, getAccountTier, getEffectiveModelPriority, hasAccountArgonCapability, hasCustomModelPriorities } from '../../types/account';
import { useTranslation } from 'react-i18next';
import { cn } from '../../utils/cn';

import { useConfigStore } from '../../stores/useConfigStore';
import { useAccountStore } from '../../stores/useAccountStore';
import { usePedalModeStore } from '../../stores/usePedalModeStore';
import { GroupQuotaCard } from './GroupQuotaCard';
import { getGroupQuotaSummaries } from '../../utils/quotaDisplay';
import { getValidationBlockedStatusLabel } from './accountValidationStatus';
import { showToast } from '../common/ToastContainer';
import { releaseAccountQoSJail } from '../../services/accountService';
import { QoSStatusBadge } from './QoSStatusBadge';
import { getLiveLimitForModel } from '../../utils/liveLimit';
import { isValidationBlockActive } from '../../utils/accountStatus';
import { parseTags } from '../../utils/tagUtils';
import AccountActionsModal from './AccountActionsModal';

// ============================================================================
// 类型定义
// ============================================================================

interface AccountTableProps {
    accounts: Account[];
    selectedIds: Set<string>;
    refreshingIds: Set<string>;
    onToggleSelect: (id: string) => void;
    onToggleAll: () => void;
    currentAccountId: string | null;
    activeClients?: ActiveClientsStatus;
    switchingAccountId: string | null;
    onSwitch: (accountId: string, targetIde?: string) => void;
    onRefresh: (accountId: string) => void;
    onViewDevice: (accountId: string) => void;
    onViewDetails: (accountId: string) => void;
    onExport: (accountId: string) => void;
    onDelete: (accountId: string) => void;
    onToggleProxy: (accountId: string) => void;
    onWarmup?: (accountId: string) => void;
    onUpdateLabel?: (accountId: string, label: string) => void;
    /** 拖拽排序回调，当用户完成拖拽时触发 */
    onReorder?: (accountIds: string[]) => void;
    onViewError: (accountId: string) => void;
}

interface SortableRowProps {
    account: Account;
    selected: boolean;
    isRefreshing: boolean;
    isCurrent: boolean;
    activeClients?: ActiveClientsStatus;
    isSwitching: boolean;
    isDragging?: boolean;
    onSelect: () => void;
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
    isDragDisabled?: boolean;
}

interface AccountRowContentProps {
    account: Account;
    isCurrent: boolean;
    activeClients?: ActiveClientsStatus;
    isRefreshing: boolean;
    isSwitching: boolean;
    isDisabled: boolean;
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

// ============================================================================
// 辅助函数
// ============================================================================

function isGroupProtected(protectedModels: string[] | undefined, groupKey: 'gemini' | 'claude' | 'argon'): boolean {
    if (!protectedModels || protectedModels.length === 0) return false;
    if (groupKey === 'argon') {
        return protectedModels.some(m => m.toLowerCase().includes('argon'));
    } else if (groupKey === 'gemini') {
        return protectedModels.some(m => m.toLowerCase().startsWith('gemini') && !m.toLowerCase().includes('argon'));
    } else {
        return protectedModels.some(m => m.toLowerCase().startsWith('claude') || m.toLowerCase().startsWith('gpt'));
    }
}

/**
 * 提取账号的最快配额重置时间（毫秒时间戳）
 * 用于表格排序
 */
function extractAccountResetTime(account: Account): number | null {
    let earliestTime: number | null = null;

    // 优先从 quota_groups 的各个 bucket 获取最早重置时间
    const groups = account.quota?.quota_groups || [];
    for (const group of groups) {
        for (const bucket of group.buckets || []) {
            if (bucket.reset_time) {
                const t = new Date(bucket.reset_time).getTime();
                if (!isNaN(t) && (earliestTime === null || t < earliestTime)) {
                    earliestTime = t;
                }
            }
        }
    }

    // 也遍历 models 获取最早重置时间作为补充
    const models = account.quota?.models || [];
    for (const model of models) {
        if (model.reset_time) {
            const t = new Date(model.reset_time).getTime();
            if (!isNaN(t) && (earliestTime === null || t < earliestTime)) {
                earliestTime = t;
            }
        }
    }

    return earliestTime;
}

// ============================================================================
// 子组件
// ============================================================================

/**
 * 可拖拽的表格行组件
 * 使用 @dnd-kit/sortable 实现拖拽功能
 */
function SortableAccountRow({
    account,
    selected,
    isRefreshing,
    isCurrent,
    activeClients,
    isSwitching,
    isDragging,
    onSelect,
    onSwitch,
    onRefresh,
    onViewDevice,
    onViewDetails,
    onExport,
    onDelete,
    onToggleProxy,
    onWarmup,
    onUpdateLabel,
    onViewError,
    isDragDisabled = false,
}: SortableRowProps) {
    const { t } = useTranslation();
    const {
        attributes,
        listeners,
        setNodeRef,
        transform,
        transition,
        isDragging: isSortableDragging,
    } = useSortable({ id: account.id, disabled: isDragDisabled });

    const style = {
        transform: CSS.Transform.toString(transform),
        transition,
        opacity: isSortableDragging ? 0.5 : 1,
        zIndex: isSortableDragging ? 1000 : 'auto',
    };

    const isClassic = activeClients?.classic_account_id === account.id;
    const isIde = activeClients?.ide_account_id === account.id;
    const isAgy = activeClients?.agy_account_id === account.id;
    const isAcp = activeClients?.acp_account_id === account.id;
    const isRowCurrent = isCurrent || isClassic || isIde || isAgy || isAcp;

    const { active: isPedalActive, targetAccountIds } = usePedalModeStore();
    const isPedaling = isPedalActive && targetAccountIds.includes(account.id);

    return (
        <tr
            ref={setNodeRef}
            style={style as React.CSSProperties}
            className={cn(
                "group transition-colors border-b border-gray-100 dark:border-base-200",
                isPedaling && "bg-amber-50/50 dark:bg-amber-950/20 border-l-4 border-l-amber-500",
                !isPedaling && isRowCurrent && "bg-blue-50/50 dark:bg-blue-900/10",
                isDragging && "bg-blue-100 dark:bg-blue-900/30 shadow-lg",
                !isDragging && !isPedaling && "hover:bg-gray-50 dark:hover:bg-base-200",
                !isDragging && isPedaling && "hover:bg-amber-50/80 dark:hover:bg-amber-950/30"
            )}
        >
            {/* 拖拽手柄 */}
            <td className="pl-2 py-1 w-8 align-middle">
                <div
                    {...(!isDragDisabled ? attributes : {})}
                    {...(!isDragDisabled ? listeners : {})}
                    className={cn(
                        "flex items-center justify-center w-6 h-6 rounded transition-colors",
                        isDragDisabled
                            ? "text-gray-200 dark:text-gray-700 cursor-not-allowed opacity-40"
                            : "cursor-grab active:cursor-grabbing text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700"
                    )}
                    title={isDragDisabled ? t('accounts.drag_disabled_during_sort', '已激活列排序，拖拽排序已暂停') : t('accounts.drag_to_reorder')}
                >
                    <GripVertical className="w-4 h-4" />
                </div>
            </td>
            {/* 复选框 */}
            <td className="px-2 py-1 w-10 align-middle">
                <input
                    type="checkbox"
                    className="checkbox checkbox-sm rounded border-2 border-gray-400 dark:border-gray-500 checked:border-blue-600 checked:bg-blue-600 [--chkbg:theme(colors.blue.600)] [--chkfg:white]"
                    checked={selected}
                    onChange={onSelect}
                    disabled={isRefreshing}
                />
            </td>
            <AccountRowContent
                account={account}
                isCurrent={isCurrent}
                activeClients={activeClients}
                isRefreshing={isRefreshing}
                isSwitching={isSwitching}
                isDisabled={Boolean(account.disabled)}
                onSwitch={onSwitch}
                onRefresh={onRefresh}
                onViewDevice={onViewDevice}
                onViewDetails={onViewDetails}
                onExport={onExport}
                onDelete={onDelete}
                onToggleProxy={onToggleProxy}
                onWarmup={onWarmup}
                onUpdateLabel={onUpdateLabel}
                onViewError={onViewError}
            />
        </tr>
    );
}

/**
 * 账号行内容组件
 * 渲染邮箱、配额、最后使用时间和操作按钮等列
 */
function AccountRowContent({
    account,
    isCurrent,
    activeClients,
    isRefreshing,
    isSwitching,
    isDisabled,
    onSwitch,
    onRefresh,
    onViewDevice,
    onViewDetails,
    onExport,
    onDelete,
    onToggleProxy,
    onWarmup,
    onUpdateLabel,
    onViewError,
}: AccountRowContentProps) {
    const { t } = useTranslation();
    const { config } = useConfigStore();
    const { active: isPedalActive, targetAccountIds } = usePedalModeStore();
    const isPedaling = isPedalActive && targetAccountIds.includes(account.id);
    const isValidationBlocked = isValidationBlockActive(account);
    const validationBlockedLabel = getValidationBlockedStatusLabel(account.validation_blocked_reason, t);
    const isBanned = Boolean(account.banned);

    const [isActionsModalOpen, setIsActionsModalOpen] = useState(false);

    const isClassic = activeClients?.classic_account_id === account.id;
    const isIde = activeClients?.ide_account_id === account.id;
    const isAgy = activeClients?.agy_account_id === account.id;
    const isAcp = activeClients?.acp_account_id === account.id;
    const isRowActive = isCurrent || isClassic || isIde || isAgy || isAcp;

    const tags = useMemo(() => parseTags(account.custom_label), [account.custom_label]);

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
        const hasGemini38 = supported.some(m => m.includes('3.8') || m.includes('3-8'));
        return { hasArgon, hasClaude55, hasClaude46, hasGpt, hasGemini38, supportedCount: supported.length };
    }, [account, groupSummaries]);

    return (
        <>
            {/* 邮箱列 */}
            <td className="px-2 py-1.5 align-middle max-w-[320px]">
                <div className="flex flex-col gap-1 min-w-0">
                    <span className={cn(
                        "font-medium text-sm break-all transition-colors",
                        isRowActive ? "text-blue-700 dark:text-blue-400" : "text-gray-900 dark:text-base-content"
                    )} title={account.email}>
                        {account.email}
                    </span>

                    <div className="flex flex-wrap items-center gap-1.5 min-w-0">
                        {isClassic && (
                            <span
                                className="px-2 py-0.5 rounded-md bg-blue-100 dark:bg-blue-900/50 text-blue-700 dark:text-blue-300 text-[10px] font-bold shadow-sm border border-blue-200/50 dark:border-blue-800/50"
                                title={t('accounts.client_classic_tooltip', '当前 Antigravity 经典款客户端登录账号')}
                            >
                                {t('accounts.client_classic', '经典款')}
                            </span>
                        )}
                        {isIde && (
                            <span
                                className="px-2 py-0.5 rounded-md bg-sky-100 dark:bg-sky-900/50 text-sky-700 dark:text-sky-300 text-[10px] font-bold shadow-sm border border-sky-200/50 dark:border-sky-800/50"
                                title={t('accounts.client_ide_tooltip', '当前 Antigravity IDE 客户端登录账号')}
                            >
                                {t('accounts.client_ide', 'IDE')}
                            </span>
                        )}
                        {isAgy && (
                            <span
                                className="px-2 py-0.5 rounded-md bg-emerald-100 dark:bg-emerald-900/50 text-emerald-700 dark:text-emerald-300 text-[10px] font-bold shadow-sm border border-emerald-200/50 dark:border-emerald-800/50"
                                title={t('accounts.client_cli_tooltip', '当前 Antigravity CLI (agy) 登录账号')}
                            >
                                {t('accounts.client_cli', 'CLI')}
                            </span>
                        )}
                        {isAcp && (
                            <span
                                className="px-2 py-0.5 rounded-md bg-violet-100 dark:bg-violet-900/50 text-violet-700 dark:text-violet-300 text-[10px] font-bold shadow-sm border border-violet-200/50 dark:border-violet-800/50"
                                title={t('accounts.client_acp_tooltip', 'Active account in Antigravity IDE Plugin (ACP)')}
                            >
                                {t('accounts.client_acp', 'Plugin')}
                            </span>
                        )}
                        {!isClassic && !isIde && !isAgy && !isAcp && isCurrent && (
                            <span className="px-2 py-0.5 rounded-md bg-blue-100 dark:bg-blue-900/50 text-blue-700 dark:text-blue-300 text-[10px] font-bold shadow-sm border border-blue-200/50 dark:border-blue-800/50">
                                {t('accounts.current').toUpperCase()}
                            </span>
                        )}
                        {isDisabled && (
                            <span
                                className="px-2 py-0.5 rounded-md bg-rose-100 dark:bg-rose-900/50 text-rose-700 dark:text-rose-300 text-[10px] font-bold flex items-center gap-1 shadow-sm border border-rose-200/50"
                            >
                                <Ban className="w-2.5 h-2.5" />
                                <span>{t('accounts.disabled')}</span>
                            </span>
                        )}

                        {isBanned && (
                            <span
                                className="px-2 py-0.5 rounded-md bg-red-100 dark:bg-red-900/50 text-red-700 dark:text-red-300 text-[10px] font-bold flex items-center gap-1 shadow-sm border border-red-200/50"
                                title={account.banned_reason || t('accounts.banned_tooltip', '账号已被上游封禁（违反服务条款），请提交申诉')}
                            >
                                <Ban className="w-2.5 h-2.5" />
                                <span>{t('accounts.banned', '已封禁')}</span>
                            </span>
                        )}

                        {account.proxy_disabled && !isDisabled && !isBanned && (
                            <span
                                className="px-2 py-0.5 rounded-md bg-orange-100 dark:bg-orange-900/50 text-orange-700 dark:text-orange-300 text-[10px] font-bold flex items-center gap-1 shadow-sm border border-orange-200/50"
                            >
                                <Ban className="w-2.5 h-2.5" />
                                <span>{t('accounts.proxy_disabled')}</span>
                            </span>
                        )}

                        {account.quota?.is_forbidden && !isBanned && (
                            <span className="px-2 py-0.5 rounded-md bg-red-100 dark:bg-red-900/50 text-red-600 dark:text-red-400 text-[10px] font-bold flex items-center gap-1 shadow-sm border border-red-200/50">
                                <Lock className="w-2.5 h-2.5" />
                                <span>{t('accounts.forbidden')}</span>
                            </span>
                        )}
                        {isValidationBlocked && (
                            <span className="px-2 py-0.5 rounded-md bg-amber-100 dark:bg-amber-900/50 text-amber-700 dark:text-amber-400 text-[10px] font-bold flex items-center gap-1 shadow-sm border border-amber-200/50">
                                <Clock className="w-2.5 h-2.5" />
                                <span>{validationBlockedLabel}</span>
                            </span>
                        )}
                        {account.forbidden_models && account.forbidden_models.length > 0 && (
                            account.forbidden_models.map((fm) => (
                                <span
                                    key={fm}
                                    className="px-2 py-0.5 rounded-md bg-rose-100 dark:bg-rose-950/40 text-rose-600 dark:text-rose-400 text-[10px] font-bold flex items-center gap-1 shadow-xs border border-rose-200/50 dark:border-rose-900/30"
                                    title={t('accounts.forbidden_model_tooltip', '该模型已被 403 冻结')}
                                >
                                    ❄️ {fm}
                                </span>
                            ))
                        )}
                        <QoSStatusBadge
                            status={account.qos_status}
                            resetSeconds={account.qos_reset_seconds}
                            size="md"
                            onRelease={handleReleaseJail}
                        />


                        {/* 订阅类型徽章 */}
                        {(() => {
                            const tier = getAccountTier(account);
                            if (tier === 'ultra') {
                                return (
                                    <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-gradient-to-r from-purple-600 to-pink-600 text-white text-[10px] font-bold shadow-sm hover:scale-105 transition-transform cursor-default">
                                        <Gem className="w-2.5 h-2.5 fill-current" />
                                        {t('accounts.ultra')}
                                    </span>
                                );
                            } else if (tier === 'enterprise') {
                                return (
                                    <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-gradient-to-r from-emerald-600 to-teal-600 text-white text-[10px] font-bold shadow-sm hover:scale-105 transition-transform cursor-default" title={account.token?.project_id ? `GCP Project: ${account.token.project_id}` : 'GCP / Vertex AI Enterprise'}>
                                        <Diamond className="w-2.5 h-2.5 fill-current" />
                                        ENTERPRISE
                                    </span>
                                );
                            } else if (tier === 'pro') {
                                return (
                                    <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-gradient-to-r from-blue-600 to-indigo-600 text-white text-[10px] font-bold shadow-sm hover:scale-105 transition-transform cursor-default">
                                        <Diamond className="w-2.5 h-2.5 fill-current" />
                                        {t('accounts.pro')}
                                    </span>
                                );
                            } else {
                                return (
                                    <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-gray-100 dark:bg-white/10 text-gray-600 dark:text-gray-400 text-[10px] font-bold shadow-sm border border-gray-200 dark:border-white/10 hover:bg-gray-200 transition-colors cursor-default">
                                        <Circle className="w-2.5 h-2.5" />
                                        {t('accounts.free')}
                                    </span>
                                );
                            }
                        })()}
                        {/* 蹬额度中高亮徽章 */}
                        {isPedaling && (
                            <span
                                className="px-2 py-0.5 rounded-md bg-amber-500/15 text-amber-800 dark:text-amber-300 text-[10px] font-bold flex items-center gap-1 shadow-xs border border-amber-500/40 animate-pulse motion-reduce:animate-none"
                                title={t('pedal.account_pedaling_tooltip', '该账号当前处于蹬额度模式，已被设为最高调度优先级 (priority = 1)')}
                            >
                                <Zap className="w-2.5 h-2.5 fill-current text-amber-500" />
                                {t('pedal.account_pedaling_badge', '⚡ 蹬额度中')}
                            </span>
                        )}
                        {/* 优先级 */}
                        <span
                            className={cn(
                                "px-2 py-0.5 rounded-md text-[10px] font-bold",
                                isPedaling
                                    ? "bg-amber-100 dark:bg-amber-900/50 text-amber-800 dark:text-amber-200 border border-amber-300/80 dark:border-amber-700/80 shadow-2xs"
                                    : hasCustomModelPriorities(account)
                                        ? "bg-indigo-50 dark:bg-indigo-900/30 text-indigo-700 dark:text-indigo-300 border border-indigo-200/60 dark:border-indigo-800/50"
                                        : "bg-gray-100 dark:bg-base-300 text-gray-500 dark:text-gray-400"
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
                                    <span key={`${tag}-${idx}`} className="inline-flex items-center gap-1 text-[10px] px-2 py-0.5 rounded-md font-bold bg-orange-100 text-orange-700 dark:bg-orange-900/40 dark:text-orange-300 border border-orange-200/50 dark:border-orange-800/50 shadow-sm">
                                        <Tag className="w-2.5 h-2.5" />
                                        {tag}
                                    </span>
                                ))}
                                {tags.length > 2 && (
                                    <span className="text-[10px] px-1.5 py-0.5 rounded-md text-gray-500 bg-gray-100 dark:bg-base-200 border border-gray-200 dark:border-base-300 font-bold" title={tags.slice(2).join(', ')}>
                                        +{tags.length - 2}
                                    </span>
                                )}
                            </>
                        )}

                        {/* 核心支持模型徽章栏 (直观展现该账号具体可用的旗舰模型代际) */}
                        {modelCapabilities.hasArgon && (
                            <span
                                className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-gradient-to-r from-purple-500/15 to-indigo-500/15 text-purple-700 dark:text-purple-300 text-[10px] font-bold shadow-2xs border border-purple-200/60 dark:border-purple-800/50 cursor-default"
                            >
                                <Sparkles className="w-2.5 h-2.5 text-purple-500" />
                                Argon
                            </span>
                        )}
                        {modelCapabilities.hasClaude55 && (
                            <span
                                className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-amber-500/15 text-amber-700 dark:text-amber-300 text-[10px] font-bold shadow-2xs border border-amber-200/60 dark:border-amber-800/50 cursor-default"
                            >
                                Claude 5.5
                            </span>
                        )}
                        {modelCapabilities.hasClaude46 && !modelCapabilities.hasClaude55 && (
                            <span
                                className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-amber-500/10 text-amber-600/90 dark:text-amber-400/90 text-[10px] font-bold shadow-2xs border border-amber-200/40 dark:border-amber-800/30 cursor-default"
                            >
                                Claude 4.6
                            </span>
                        )}
                        {modelCapabilities.hasGpt && (
                            <span
                                className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-emerald-500/15 text-emerald-700 dark:text-emerald-300 text-[10px] font-bold shadow-2xs border border-emerald-200/60 dark:border-emerald-800/50 cursor-default"
                            >
                                GPT-120B
                            </span>
                        )}
                    </div>

                </div>
            </td>

            {/* 模型配额列 */}
            <td className="px-3 py-2 align-middle">
                {isDisabled || account.banned || account.quota?.is_forbidden || isValidationBlocked ? (
                    <div className={cn(
                        "flex items-center justify-center gap-3 py-1.5 px-4 rounded-xl border group/error",
                        isValidationBlocked && !account.banned ? "bg-amber-50/50 dark:bg-amber-900/10 border-amber-100/50 dark:border-amber-900/20" : "bg-red-50/50 dark:bg-red-900/10 border-red-100/50 dark:border-red-900/20"
                    )}>
                        <div className={cn(
                            "flex items-center gap-1.5",
                            isValidationBlocked && !account.banned ? "text-amber-600 dark:text-amber-400" : "text-red-600 dark:text-red-400"
                        )}>
                            {isValidationBlocked && !account.banned ? <Clock className="w-3.5 h-3.5" /> : (account.banned ? <Ban className="w-3.5 h-3.5" /> : (account.quota?.is_forbidden ? <Lock className="w-3.5 h-3.5" /> : <Ban className="w-3.5 h-3.5" />))}
                            <span className={cn(
                                "text-[11px] font-bold",
                                isValidationBlocked && !account.banned ? "text-amber-700/80 dark:text-amber-400" : "text-red-700/80 dark:text-red-400"
                            )}>
                                {isValidationBlocked && !account.banned ? validationBlockedLabel : (account.banned ? t('accounts.banned_msg', '账号已被上游封禁，已停止自动刷新') : (isDisabled ? t('accounts.status.disabled') : t('accounts.forbidden_msg')))}
                            </span>
                        </div>
                        <div className={cn(
                            "w-px h-3",
                            isValidationBlocked ? "bg-amber-200 dark:bg-amber-800/50" : "bg-red-200 dark:bg-red-800/50"
                        )} />
                        <button
                            onClick={(e) => { e.stopPropagation(); onViewError(); }}
                            className="text-[10px] font-medium text-blue-600 dark:text-blue-400 hover:underline flex items-center gap-0.5"
                        >
                            {t('accounts.view_error')}
                        </button>
                    </div>
                ) : (
                    <div className="flex flex-col gap-1.5 py-0.5">
                        <div className="grid grid-cols-1 sm:grid-cols-2 gap-1.5">
                            {groupSummaries.filter(s => s.key !== 'argon').map((summary) => (
                                <GroupQuotaCard
                                    key={summary.key}
                                    summary={summary}
                                    isProtected={Boolean(config?.quota_protection?.enabled && isGroupProtected(account.protected_models, summary.key))}
                                    liveLimit={summary.key === 'gemini' ? getLiveLimitForModel(account, 'gemini-pro') : getLiveLimitForModel(account, 'claude')}
                                    compact
                                />
                            ))}
                        </div>
                        {(() => {
                            const argon = groupSummaries.find(s => s.key === 'argon');
                            if (!argon) return null;
                            return (
                                <div className="w-full pt-0.5">
                                    <GroupQuotaCard
                                        summary={argon}
                                        isProtected={Boolean(config?.quota_protection?.enabled && isGroupProtected(account.protected_models, 'argon'))}
                                        liveLimit={getLiveLimitForModel(account, 'gemini-4-argon')}
                                        compact
                                        fullWidth
                                    />
                                </div>
                            );
                        })()}
                    </div>
                )}
            </td>

            {/* 最后使用时间列 */}
            <td className="px-2 py-1 align-middle">
                <div className="flex flex-col">
                    <span className="text-xs font-medium text-gray-600 dark:text-gray-400 font-mono whitespace-nowrap">
                        {new Date(account.last_used * 1000).toLocaleDateString()}
                    </span>
                    <span className="text-[10px] text-gray-400 dark:text-gray-500 font-mono whitespace-nowrap leading-tight">
                        {new Date(account.last_used * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                    </span>
                </div>
            </td>

            {/* 操作列 */}
            <td className={cn(
                "px-1 py-1 sticky right-0 w-[136px] min-w-[136px] max-w-[136px] z-10 shadow-[-12px_0_12px_-12px_rgba(0,0,0,0.1)] dark:shadow-[-12px_0_12px_-12px_rgba(255,255,255,0.05)] text-center align-middle",
                // 动态背景色处理
                isCurrent
                    ? "bg-[#f1f6ff] dark:bg-[#1e2330]" // 接近 blue-50/50 的实色
                    : "bg-white dark:bg-base-100",
                !isCurrent && "group-hover:bg-gray-50 dark:group-hover:bg-base-200"
            )}>
                <div className="grid grid-cols-2 gap-1 w-full max-w-[128px] mx-auto">
                    {/* 1. 切换 */}
                    <button
                        className={cn(
                            "flex items-center justify-center gap-0.5 px-1 py-1 rounded-lg text-xs font-semibold transition-all shadow-xs",
                            (isSwitching || isDisabled || isBanned)
                                ? "bg-gray-100 dark:bg-base-200 text-gray-400 dark:text-gray-500 cursor-not-allowed border border-gray-200 dark:border-base-300"
                                : "bg-blue-50 dark:bg-blue-900/20 text-blue-600 dark:text-blue-400 hover:bg-blue-100 dark:hover:bg-blue-900/40 border border-blue-200/60 dark:border-blue-800/40 active:scale-95"
                        )}
                        onClick={(e) => { e.stopPropagation(); onSwitch('ide'); }}
                        disabled={isSwitching || isDisabled || isBanned}
                        title={isDisabled ? t('accounts.disabled_tooltip') : (isSwitching ? t('common.loading') : t('accounts.switch_to_ide', '切换到 Antigravity IDE'))}
                    >
                        <Repeat2 className={cn("w-3 h-3 flex-shrink-0", isSwitching && "animate-spin")} />
                        <span className="truncate">{t('accounts.action_switch', '切换')}</span>
                    </button>

                    {/* 2. 刷新 */}
                    <button
                        className={cn(
                            "flex items-center justify-center gap-0.5 px-1 py-1 rounded-lg text-xs font-semibold transition-all shadow-xs",
                            (isRefreshing || isDisabled)
                                ? "bg-gray-100 dark:bg-base-200 text-gray-400 dark:text-gray-500 cursor-not-allowed border border-gray-200 dark:border-base-300"
                                : "bg-emerald-50 dark:bg-emerald-900/20 text-emerald-600 dark:text-emerald-400 hover:bg-emerald-100 dark:hover:bg-emerald-900/40 border border-emerald-200/60 dark:border-emerald-800/40 active:scale-95"
                        )}
                        onClick={(e) => { e.stopPropagation(); onRefresh(); }}
                        disabled={isRefreshing || isDisabled}
                        title={isDisabled ? t('accounts.disabled_tooltip') : t('accounts.refresh_quota_and_ban', '刷新配额并检测封禁')}
                    >
                        <RefreshCw className={cn("w-3 h-3 flex-shrink-0", isRefreshing && "animate-spin")} />
                        <span className="truncate">{t('accounts.action_refresh', '刷新')}</span>
                    </button>

                    {/* 3. 启停 */}
                    <button
                        className={cn(
                            "flex items-center justify-center gap-0.5 px-1 py-1 rounded-lg text-xs font-semibold transition-all shadow-xs",
                            isBanned
                                ? "bg-gray-100 dark:bg-base-200 text-gray-400 dark:text-gray-500 cursor-not-allowed border border-gray-200 dark:border-base-300"
                                : account.proxy_disabled
                                    ? "bg-amber-50 dark:bg-amber-900/20 text-amber-600 dark:text-amber-400 hover:bg-amber-100 dark:hover:bg-amber-900/40 border border-amber-200/60 dark:border-amber-800/40 active:scale-95"
                                    : "bg-teal-50 dark:bg-teal-900/20 text-teal-600 dark:text-teal-400 hover:bg-teal-100 dark:hover:bg-teal-900/40 border border-teal-200/60 dark:border-teal-800/40 active:scale-95"
                        )}
                        onClick={(e) => { e.stopPropagation(); onToggleProxy(); }}
                        disabled={isBanned}
                        title={isBanned ? t('accounts.banned_tooltip', '账号已被上游封禁（违反服务条款），请提交申诉') : (account.proxy_disabled ? t('accounts.enable_proxy') : t('accounts.disable_proxy'))}
                    >
                        <Power className="w-3 h-3 flex-shrink-0" />
                        <span className="truncate">
                            {account.proxy_disabled ? t('accounts.action_enable', '启用') : t('accounts.action_disable', '停用')}
                        </span>
                    </button>

                    {/* 4. 更多 */}
                    <button
                        className="flex items-center justify-center gap-0.5 px-1 py-1 rounded-lg text-xs font-semibold transition-all shadow-xs bg-gray-50 dark:bg-base-200 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-base-300 border border-gray-200 dark:border-base-300 active:scale-95"
                        onClick={(e) => { e.stopPropagation(); setIsActionsModalOpen(true); }}
                        title={t('accounts.actions_modal.title', '账号操作管理')}
                    >
                        <MoreHorizontal className="w-3 h-3 flex-shrink-0" />
                        <span className="truncate">{t('accounts.action_more', '更多')}</span>
                    </button>
                </div>

                {/* Modals */}
                <AccountActionsModal
                    isOpen={isActionsModalOpen}
                    onClose={() => setIsActionsModalOpen(false)}
                    account={account}
                    onSwitch={(target) => onSwitch(target)}
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
            </td>
        </>
    );
}

// ============================================================================
// 主组件
// ============================================================================

/**
 * 账号表格组件
 * 支持拖拽排序、多选、批量操作等功能
 */
function AccountTable({
    accounts,
    selectedIds,
    refreshingIds,
    onToggleSelect,
    onToggleAll,
    currentAccountId,
    activeClients,
    switchingAccountId,
    onSwitch,
    onRefresh,
    onViewDevice,
    onViewDetails,
    onExport,
    onDelete,
    onToggleProxy,
    onReorder,
    onWarmup,
    onUpdateLabel,
    onViewError,
}: AccountTableProps) {
    const { t } = useTranslation();

    const [activeId, setActiveId] = useState<string | null>(null);
    // 排序状态配置: 支持按配额重置时间 (reset_time) 或最后使用时间 (last_used) 排序
    const [sortConfig, setSortConfig] = useState<{
        key: 'reset_time' | 'last_used' | null;
        direction: 'asc' | 'desc' | null;
    }>({
        key: null,
        direction: null,
    });

    const isSortingActive = sortConfig.key !== null && sortConfig.direction !== null;

    const handleSortToggle = (key: 'reset_time' | 'last_used') => {
        setSortConfig(prev => {
            if (prev.key !== key) {
                return { key, direction: 'asc' };
            }
            if (prev.direction === 'asc') {
                return { key, direction: 'desc' };
            }
            return { key: null, direction: null };
        });
    };

    // 根据排序状态对 accounts 进行拦截排序
    const sortedAccounts = useMemo(() => {
        if (!isSortingActive) return accounts;

        return [...accounts].sort((a, b) => {
            if (sortConfig.key === 'reset_time') {
                const timeA = extractAccountResetTime(a);
                const timeB = extractAccountResetTime(b);

                // 没有 reset_time 的排到后面
                if (timeA === null && timeB === null) return 0;
                if (timeA === null) return 1;
                if (timeB === null) return -1;

                return sortConfig.direction === 'asc' ? timeA - timeB : timeB - timeA;
            }

            if (sortConfig.key === 'last_used') {
                const timeA = a.last_used || 0;
                const timeB = b.last_used || 0;

                return sortConfig.direction === 'asc' ? timeA - timeB : timeB - timeA;
            }

            return 0;
        });
    }, [accounts, sortConfig, isSortingActive]);

    // 配置拖拽传感器
    const sensors = useSensors(
        useSensor(PointerSensor, {
            activationConstraint: { distance: 8 }, // 需要移动 8px 才触发拖拽
        }),
        useSensor(KeyboardSensor, {
            coordinateGetter: sortableKeyboardCoordinates,
        })
    );

    const accountIds = useMemo(() => sortedAccounts.map(a => a.id), [sortedAccounts]);
    const activeAccount = useMemo(() => sortedAccounts.find(a => a.id === activeId), [sortedAccounts, activeId]);

    const handleDragStart = (event: DragStartEvent) => {
        if (isSortingActive) return;
        setActiveId(event.active.id as string);
    };

    const handleDragEnd = (event: DragEndEvent) => {
        const { active, over } = event;
        setActiveId(null);

        if (isSortingActive) return;

        if (over && active.id !== over.id) {
            const oldIndex = accountIds.indexOf(active.id as string);
            const newIndex = accountIds.indexOf(over.id as string);

            if (oldIndex !== -1 && newIndex !== -1 && onReorder) {
                onReorder(arrayMove(accountIds, oldIndex, newIndex));
            }
        }
    };

    if (accounts.length === 0) {
        return (
            <div className="bg-white dark:bg-base-100 rounded-2xl p-12 shadow-sm border border-gray-100 dark:border-base-200 text-center">
                <p className="text-gray-400 mb-2">{t('accounts.empty.title')}</p>
                <p className="text-sm text-gray-400">{t('accounts.empty.desc')}</p>
            </div>
        );
    }

    return (
        <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            onDragStart={handleDragStart}
            onDragEnd={handleDragEnd}
        >
            <div className="overflow-x-auto">
                <table className="w-full">
                    <thead>
                        <tr className="border-b border-gray-100 dark:border-base-200 bg-gray-50 dark:bg-base-200">
                            <th className="pl-2 py-2 text-left w-8">
                                <span className="sr-only">{t('accounts.drag_to_reorder')}</span>
                            </th>
                            <th className="px-2 py-2 text-left w-10">
                                <input
                                    type="checkbox"
                                    className="checkbox checkbox-sm rounded border-2 border-gray-400 dark:border-gray-500 checked:border-blue-600 checked:bg-blue-600 [--chkbg:theme(colors.blue.600)] [--chkfg:white]"
                                    checked={accounts.length > 0 && selectedIds.size === accounts.length}
                                    onChange={onToggleAll}
                                />
                            </th>
                            <th className="px-2 py-1 text-left rtl:text-right text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider w-[300px] whitespace-nowrap">{t('accounts.table.email')}</th>
                            <th className="px-2 py-1 text-left rtl:text-right text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider min-w-[340px] whitespace-nowrap">
                                <button
                                    type="button"
                                    onClick={() => handleSortToggle('reset_time')}
                                    className={cn(
                                        "inline-flex items-center gap-1 hover:text-blue-600 dark:hover:text-blue-400 transition-colors uppercase font-medium",
                                        sortConfig.key === 'reset_time' && "text-blue-600 dark:text-blue-400 font-semibold"
                                    )}
                                    title={t('accounts.table.sort_by_reset_time', '点击按配额重置时间排序')}
                                >
                                    <span>{t('accounts.table.quota', '模型配额')}</span>
                                    {sortConfig.key === 'reset_time' ? (
                                        sortConfig.direction === 'asc' ? <ArrowUp className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" /> : <ArrowDown className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" />
                                    ) : (
                                        <ArrowUpDown className="w-3 h-3 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 opacity-60 hover:opacity-100" />
                                    )}
                                </button>
                            </th>
                            <th className="px-2 py-1 text-left rtl:text-right text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider w-[90px] whitespace-nowrap">
                                <button
                                    type="button"
                                    onClick={() => handleSortToggle('last_used')}
                                    className={cn(
                                        "inline-flex items-center gap-1 hover:text-blue-600 dark:hover:text-blue-400 transition-colors uppercase font-medium",
                                        sortConfig.key === 'last_used' && "text-blue-600 dark:text-blue-400 font-semibold"
                                    )}
                                    title={t('accounts.table.sort_by_last_used', '点击按最后使用时间排序')}
                                >
                                    <span>{t('accounts.table.last_used')}</span>
                                    {sortConfig.key === 'last_used' ? (
                                        sortConfig.direction === 'asc' ? <ArrowUp className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" /> : <ArrowDown className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" />
                                    ) : (
                                        <ArrowUpDown className="w-3 h-3 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 opacity-60 hover:opacity-100" />
                                    )}
                                </button>
                            </th>
                            <th className="px-1 py-1 text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider whitespace-nowrap sticky right-0 w-[136px] min-w-[136px] max-w-[136px] bg-gray-50 dark:bg-base-200 z-20 shadow-[-12px_0_12px_-12px_rgba(0,0,0,0.1)] dark:shadow-[-12px_0_12px_-12px_rgba(255,255,255,0.05)] text-center">{t('accounts.table.actions')}</th>
                        </tr >
                    </thead >
                    <SortableContext items={accountIds} strategy={verticalListSortingStrategy}>
                        <tbody className="divide-y divide-gray-100 dark:divide-base-200">
                            {sortedAccounts.map((account) => (
                                <SortableAccountRow
                                    key={account.id}
                                    account={account}
                                    selected={selectedIds.has(account.id)}
                                    isRefreshing={refreshingIds.has(account.id)}
                                    isCurrent={account.id === currentAccountId}
                                    activeClients={activeClients}
                                    isSwitching={account.id === switchingAccountId}
                                    isDragging={account.id === activeId}
                                    onSelect={() => onToggleSelect(account.id)}
                                    onSwitch={(targetIde?: string) => onSwitch(account.id, targetIde)}
                                    onRefresh={() => onRefresh(account.id)}
                                    onViewDevice={() => onViewDevice(account.id)}
                                    onViewDetails={() => onViewDetails(account.id)}
                                    onExport={() => onExport(account.id)}
                                    onDelete={() => onDelete(account.id)}
                                    onToggleProxy={() => onToggleProxy(account.id)}
                                    onWarmup={onWarmup ? () => onWarmup(account.id) : undefined}
                                    onUpdateLabel={onUpdateLabel ? (label: string) => onUpdateLabel(account.id, label) : undefined}
                                    onViewError={() => onViewError(account.id)}
                                    isDragDisabled={isSortingActive}
                                />
                            ))}
                        </tbody>
                    </SortableContext>
                </table >
            </div >

            {/* 拖拽悬浮预览层 */}
            <DragOverlay>
                {
                    activeAccount ? (
                        <table className="w-full bg-white dark:bg-base-100 shadow-2xl rounded-lg border border-blue-200 dark:border-blue-800">
                            <tbody>
                                <tr className="bg-blue-50 dark:bg-blue-900/30">
                                    <td className="pl-2 py-1 w-8">
                                        <div className="flex items-center justify-center w-6 h-6 text-blue-500">
                                            <GripVertical className="w-4 h-4" />
                                        </div>
                                    </td>
                                    <td className="px-2 py-1 w-10">
                                        <input
                                            type="checkbox"
                                            className="checkbox checkbox-xs rounded border-2"
                                            checked={selectedIds.has(activeAccount.id)}
                                            readOnly
                                        />
                                    </td>
                                    <AccountRowContent
                                        account={activeAccount}
                                        isCurrent={activeAccount.id === currentAccountId}
                                        activeClients={activeClients}
                                        isRefreshing={refreshingIds.has(activeAccount.id)}
                                        isSwitching={activeAccount.id === switchingAccountId}
                                        onSwitch={() => { }}
                                        onRefresh={() => { }}
                                        onViewDevice={() => { }}
                                        onViewDetails={() => { }}
                                        onExport={() => { }}
                                        onDelete={() => { }}
                                        onToggleProxy={() => { }}
                                        isDisabled={Boolean(activeAccount.disabled)}
                                        onViewError={() => { }}
                                    />
                                </tr>
                            </tbody>
                        </table>
                    ) : null
                }
            </DragOverlay>
        </DndContext>
    );
}

export default AccountTable;
