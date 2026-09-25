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
    ArrowRightLeft,
    RefreshCw,
    Trash2,
    Download,
    Fingerprint,
    Info,
    Lock,
    Ban,
    Diamond,
    Gem,
    Circle,
    ToggleLeft,
    ToggleRight,
    Sparkles,
    Tag,
    X,
    Check,
    Clock,
    Bot,
    Repeat2,
    Terminal,
    ArrowUpDown,
    ArrowUp,
    ArrowDown,
} from 'lucide-react';
import { type Account, type ModelQuota, getAccountTier } from '../../types/account';
import { useTranslation } from 'react-i18next';
import { cn } from '../../utils/cn';

import { useConfigStore } from '../../stores/useConfigStore';
import { QuotaItem } from './QuotaItem';
import { getModelQuotaDisplay } from '../../utils/quotaDisplay';
import { MODEL_CONFIG, sortModels, resolveQuotaModels, ensurePinnedImageSelector } from '../../config/modelConfig';
import { categorizeModel, getModelProtectionKey } from '../../utils/modelCategory';
import { getValidationBlockedStatusLabel } from './accountValidationStatus';
import { getLiveLimitForModel } from '../../utils/liveLimit';

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
    quotaWindow?: '5h' | 'weekly';
}

interface SortableRowProps {
    account: Account;
    selected: boolean;
    isRefreshing: boolean;
    isCurrent: boolean;
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
    quotaWindow?: '5h' | 'weekly';
    isDragDisabled?: boolean;
}

interface AccountRowContentProps {
    account: Account;
    isCurrent: boolean;
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
    quotaWindow?: '5h' | 'weekly';
}

// ============================================================================
// 辅助函数
// ============================================================================



function isModelProtected(protectedModels: string[] | undefined, modelName: string): boolean {
    if (!protectedModels || protectedModels.length === 0) return false;
    const lowerName = modelName.toLowerCase();

    if (lowerName === 'gemini-pro') {
        return protectedModels.some((model) =>
            categorizeModel(model) === 'gemini-pro' && getModelProtectionKey(model) === 'gemini-3-pro-high',
        );
    }
    if (lowerName === 'gemini-flash') {
        return protectedModels.some((model) =>
            categorizeModel(model) === 'gemini-flash' && getModelProtectionKey(model) === 'gemini-3-flash',
        );
    }
    if (lowerName === 'claude-sonnet') {
        return protectedModels.some((model) =>
            categorizeModel(model) === 'claude' && getModelProtectionKey(model) === 'claude',
        );
    }

    const protectionKey = getModelProtectionKey(lowerName);
    return protectionKey ? protectedModels.includes(protectionKey) : false;
}

/**
 * 提取账号的最快配额重置时间（毫秒时间戳）
 * 用于表格排序
 */
function extractAccountResetTime(account: Account, quotaWindow?: '5h' | 'weekly'): number | null {
    let earliestTime: number | null = null;

    if (quotaWindow === 'weekly') {
        const groups = account.quota?.quota_groups || [];
        for (const group of groups) {
            for (const bucket of group.buckets || []) {
                const isWeekly = bucket.window.toLowerCase().includes('week') || bucket.bucket_id.toLowerCase().includes('week');
                if (isWeekly && bucket.reset_time) {
                    const t = new Date(bucket.reset_time).getTime();
                    if (!isNaN(t) && (earliestTime === null || t < earliestTime)) {
                        earliestTime = t;
                    }
                }
            }
        }
    } else {
        // 5h 或常规视图下，从 models 中获取最近的 reset_time
        const models = account.quota?.models || [];
        for (const model of models) {
            if (model.reset_time) {
                const t = new Date(model.reset_time).getTime();
                if (!isNaN(t) && (earliestTime === null || t < earliestTime)) {
                    earliestTime = t;
                }
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
    quotaWindow,
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

    return (
        <tr
            ref={setNodeRef}
            style={style as React.CSSProperties}
            className={cn(
                "group transition-colors border-b border-gray-100 dark:border-base-200",
                isCurrent && "bg-blue-50/50 dark:bg-blue-900/10",
                isDragging && "bg-blue-100 dark:bg-blue-900/30 shadow-lg",
                !isDragging && "hover:bg-gray-50 dark:hover:bg-base-200"
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
                quotaWindow={quotaWindow}
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
    quotaWindow,
}: AccountRowContentProps) {
    const { t } = useTranslation();
    const { config, showAllQuotas } = useConfigStore();
    const validationBlockedLabel = getValidationBlockedStatusLabel(account.validation_blocked_reason, t);

    // 自定义标签编辑状态
    const [isEditingLabel, setIsEditingLabel] = useState(false);
    const [labelInput, setLabelInput] = useState(account.custom_label || '');

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

    // 解析周配额项 (当处于 weekly 视图时)
    const weeklyItems = useMemo(() => {
        if (quotaWindow !== 'weekly') return [];
        return (account.quota?.quota_groups || []).flatMap(group => {
            return (group.buckets || [])
                .filter(b => b.window.toLowerCase().includes('week') || b.bucket_id.toLowerCase().includes('week'))
                .map(b => {
                    const shortGroupName = (group.display_name || '')
                        .replace(/ models?$/i, '')
                        .replace(/Claude and GPT/i, 'Claude/GPT');
                    return {
                        id: `${group.display_name}-${b.bucket_id}`,
                        label: b.display_name ? `${shortGroupName} (${b.display_name})` : `${shortGroupName} (周)`,
                        percentage: Math.round((b.remaining_fraction || 0) * 100),
                        resetTime: b.reset_time,
                        cycleTokens: b.cycle_tokens,
                        Icon: shortGroupName.toLowerCase().includes('claude') ? Sparkles : Bot,
                    };
                });
        });
    }, [quotaWindow, account.quota?.quota_groups]);

    // 获取要显示的模型列表
    const pinnedModels = ensurePinnedImageSelector(
        config?.pinned_quota_models?.models || Object.keys(MODEL_CONFIG),
    );

    // 根据 show_all 状态决定显示哪些模型
    const uniqueLabels = new Set<string>();
    const displayModels = sortModels(
        (showAllQuotas
            ? (account.quota?.models || []).map(m => {
                const config = MODEL_CONFIG[m.name.toLowerCase()];
                const label = m.display_name || (config?.i18nKey ? t(config.i18nKey) : (config?.shortLabel || config?.label || m.name));
                return {
                    id: m.name.toLowerCase(),
                    label: label,
                    protectedKey: config?.protectedKey || m.name.toLowerCase(),
                    data: m
                };
            })
            : resolveQuotaModels(account.quota?.models, pinnedModels).map(sel => {
                const selectorConfig = MODEL_CONFIG[sel.selectorId.toLowerCase()];
                const resolvedConfig = sel.model ? MODEL_CONFIG[sel.model.name.toLowerCase()] : undefined;
                if (!selectorConfig && !sel.model) return null;
                const label = sel.model?.display_name
                    || (resolvedConfig?.shortLabel || resolvedConfig?.label)
                    || (selectorConfig?.shortLabel || selectorConfig?.label)
                    || (resolvedConfig?.i18nKey ? t(resolvedConfig.i18nKey) : undefined)
                    || (selectorConfig?.i18nKey ? t(selectorConfig.i18nKey) : undefined)
                    || sel.selectorId;
                return {
                    id: sel.model?.name.toLowerCase() ?? sel.selectorId.toLowerCase(),
                    label,
                    protectedKey: getModelProtectionKey(sel.model?.name ?? sel.selectorId) ?? resolvedConfig?.protectedKey ?? selectorConfig?.protectedKey ?? sel.selectorId,
                    data: sel.model,
                };
            }).filter((item): item is { id: string; label: string; protectedKey: string; data: ModelQuota | undefined } => item !== null)
    ).filter(m => {
            // 过滤特定的 Claude/Gemini 思考变体 (在列表页隐藏)
            const isHiddenThinking = m.id.includes('thinking');

            if (isHiddenThinking) return false;

            // 基于标签去重 (例如 G3.1 Pro 只显示一次)
            // 优先显示有配额数据的 ID
            const labelKey = `${m.label}-${m.protectedKey}`;
            if (uniqueLabels.has(labelKey)) {
                return false;
            }
            if (m.data) {
                uniqueLabels.add(labelKey);
                return true;
            }
            return true;
        })
    ).filter((m, index, self) => {
        // 第二次过滤：确保即使没有数据的重复 Label 也只保留一个
        const labelKey = `${m.label}-${m.protectedKey}`;
        return self.findIndex(t => `${t.label}-${t.protectedKey}` === labelKey) === index;
    });


    return (
        <>
            {/* 邮箱列 */}
            <td className="px-2 py-1 align-middle">
                <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                    <span className={cn(
                        "font-medium text-sm break-all transition-colors",
                        isCurrent ? "text-blue-700 dark:text-blue-400" : "text-gray-900 dark:text-base-content"
                    )} title={account.email}>
                        {account.email}
                    </span>

                    <div className="flex items-center gap-1.5 shrink-0">
                        {isCurrent && (
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

                        {account.proxy_disabled && (
                            <span
                                className="px-2 py-0.5 rounded-md bg-orange-100 dark:bg-orange-900/50 text-orange-700 dark:text-orange-300 text-[10px] font-bold flex items-center gap-1 shadow-sm border border-orange-200/50"
                            >
                                <Ban className="w-2.5 h-2.5" />
                                <span>{t('accounts.proxy_disabled')}</span>
                            </span>
                        )}

                        {account.quota?.is_forbidden && (
                            <span className="px-2 py-0.5 rounded-md bg-red-100 dark:bg-red-900/50 text-red-600 dark:text-red-400 text-[10px] font-bold flex items-center gap-1 shadow-sm border border-red-200/50">
                                <Lock className="w-2.5 h-2.5" />
                                <span>{t('accounts.forbidden')}</span>
                            </span>
                        )}
                        {account.validation_blocked && (
                            <span className="px-2 py-0.5 rounded-md bg-amber-100 dark:bg-amber-900/50 text-amber-700 dark:text-amber-400 text-[10px] font-bold flex items-center gap-1 shadow-sm border border-amber-200/50">
                                <Clock className="w-2.5 h-2.5" />
                                <span>{validationBlockedLabel}</span>
                            </span>
                        )}


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
                        {/* 自定义标签 */}
                        <span className="px-2 py-0.5 rounded-md bg-gray-100 dark:bg-base-300 text-gray-500 dark:text-gray-400 text-[10px] font-bold" title={t('accounts.priority_hint')}>
                            {t('accounts.priority')}: {account.priority ?? 50}
                        </span>
                        {account.custom_label && !isEditingLabel && (
                            <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-orange-100 dark:bg-orange-900/40 text-orange-700 dark:text-orange-300 text-[10px] font-bold shadow-sm border border-orange-200/50 dark:border-orange-800/50">
                                <Tag className="w-2.5 h-2.5" />
                                {account.custom_label}
                            </span>
                        )}
                        {/* 标签编辑输入框 */}
                        {isEditingLabel && (
                            <div className="flex items-center gap-1">
                                <input
                                    type="text"
                                    className="px-1.5 py-0.5 text-[10px] w-20 border border-orange-300 dark:border-orange-700 rounded focus:outline-none focus:ring-1 focus:ring-orange-500 bg-white dark:bg-base-200"
                                    placeholder={t('accounts.custom_label_placeholder', 'Label')}
                                    value={labelInput}
                                    onChange={(e) => setLabelInput(e.target.value)}
                                    onKeyDown={handleKeyDown}
                                    autoFocus
                                    maxLength={15}
                                    onClick={(e) => e.stopPropagation()}
                                />
                                <button
                                    className="p-0.5 text-green-600 hover:bg-green-50 dark:hover:bg-green-900/30 rounded transition-all"
                                    onClick={(e) => { e.stopPropagation(); handleSaveLabel(); }}
                                >
                                    <Check className="w-3 h-3" />
                                </button>
                                <button
                                    className="p-0.5 text-gray-400 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-900/30 rounded transition-all"
                                    onClick={(e) => { e.stopPropagation(); handleCancelLabel(); }}
                                >
                                    <X className="w-3 h-3" />
                                </button>
                            </div>
                        )}
                    </div>

                </div>
            </td>

            {/* 模型配额列 */}
            <td className="px-2 py-1 align-middle">
                {isDisabled || account.quota?.is_forbidden || account.validation_blocked ? (
                    <div className={cn(
                        "flex items-center justify-center gap-3 py-1.5 px-4 rounded-xl border group/error",
                        account.validation_blocked ? "bg-amber-50/50 dark:bg-amber-900/10 border-amber-100/50 dark:border-amber-900/20" : "bg-red-50/50 dark:bg-red-900/10 border-red-100/50 dark:border-red-900/20"
                    )}>
                        <div className={cn(
                            "flex items-center gap-1.5",
                            account.validation_blocked ? "text-amber-600 dark:text-amber-400" : "text-red-600 dark:text-red-400"
                        )}>
                            {account.validation_blocked ? <Clock className="w-3.5 h-3.5" /> : (account.quota?.is_forbidden ? <Lock className="w-3.5 h-3.5" /> : <Ban className="w-3.5 h-3.5" />)}
                            <span className={cn(
                                "text-[11px] font-bold",
                                account.validation_blocked ? "text-amber-700/80 dark:text-amber-400" : "text-red-700/80 dark:text-red-400"
                            )}>
                                {account.validation_blocked ? validationBlockedLabel : (isDisabled ? t('accounts.status.disabled') : t('accounts.forbidden_msg'))}
                            </span>
                        </div>
                        <div className={cn(
                            "w-px h-3",
                            account.validation_blocked ? "bg-amber-200 dark:bg-amber-800/50" : "bg-red-200 dark:bg-red-800/50"
                        )} />
                        <button
                            onClick={(e) => { e.stopPropagation(); onViewError(); }}
                            className="text-[10px] font-medium text-blue-600 dark:text-blue-400 hover:underline flex items-center gap-0.5"
                        >
                            {t('accounts.view_error')}
                        </button>
                    </div>
                ) : (
                    <div className={cn(
                        "grid gap-x-2 gap-y-1 py-0",
                        (quotaWindow === 'weekly' && weeklyItems.length > 0)
                            ? (weeklyItems.length === 1 ? "grid-cols-1" : "grid-cols-2")
                            : (displayModels.length === 1 ? "grid-cols-1" : "grid-cols-2")
                    )}>
                        {quotaWindow === 'weekly' && weeklyItems.length > 0 ? (
                            weeklyItems.map((item) => (
                                <QuotaItem
                                    key={item.id}
                                    label={item.label}
                                    percentage={item.percentage}
                                    resetTime={item.resetTime}
                                    weeklyTokens={item.cycleTokens ?? null}
                                    Icon={item.Icon}
                                />
                            ))
                        ) : (
                            displayModels.map((model) => {
                                const modelData = model.data;
                                const display = getModelQuotaDisplay(model.id, modelData, account.quota?.quota_groups);

                                return (
                                    <QuotaItem
                                        key={model.id}
                                        label={model.label}
                                        {...display}
                                        isProtected={Boolean(config?.quota_protection?.enabled && isModelProtected(account.protected_models, model.protectedKey))}
                                        liveLimit={getLiveLimitForModel(account, model.id, model.protectedKey)}
                                        Icon={MODEL_CONFIG[model.id]?.Icon || Bot}
                                    />
                                );
                            })
                        )}
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
                "px-1 py-1 sticky right-0 z-10 shadow-[-12px_0_12px_-12px_rgba(0,0,0,0.1)] dark:shadow-[-12px_0_12px_-12px_rgba(255,255,255,0.05)] text-center align-middle",
                // 动态背景色处理
                isCurrent
                    ? "bg-[#f1f6ff] dark:bg-[#1e2330]" // 接近 blue-50/50 的实色
                    : "bg-white dark:bg-base-100",
                !isCurrent && "group-hover:bg-gray-50 dark:group-hover:bg-base-200"
            )}>
                <div className="flex flex-wrap items-center justify-center gap-1 opacity-60 group-hover:opacity-100 transition-opacity max-w-[220px] mx-auto">
                    <button
                        className="p-1.5 text-gray-500 dark:text-gray-400 hover:text-sky-600 dark:hover:text-sky-400 hover:bg-sky-50 dark:hover:bg-sky-900/30 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); onViewDetails(); }}
                        title={t('common.details')}
                    >
                        <Info className="w-3.5 h-3.5" />
                    </button>
                    <button
                        className="p-1.5 text-gray-500 dark:text-gray-400 hover:text-indigo-600 dark:hover:text-indigo-400 hover:bg-indigo-50 dark:hover:bg-indigo-900/30 rounded-lg transition-all"
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
                                    : "text-gray-500 dark:text-gray-400 hover:text-orange-500 hover:bg-orange-50 dark:hover:bg-orange-900/30"
                            )}
                            onClick={(e) => { e.stopPropagation(); setIsEditingLabel(true); }}
                            title={t('accounts.edit_label', 'Edit Label')}
                        >
                            <Tag className="w-3.5 h-3.5" />
                        </button>
                    )}
                    <button
                        className={`p-1.5 text-gray-500 dark:text-gray-400 rounded-lg transition-all ${(isSwitching || isDisabled) ? 'bg-blue-50 dark:bg-blue-900/10 text-blue-600 dark:text-blue-400 cursor-not-allowed' : 'hover:text-blue-600 dark:hover:text-blue-400 hover:bg-blue-50 dark:hover:bg-blue-900/30'}`}
                        onClick={(e) => { e.stopPropagation(); onSwitch('classic'); }}
                        title={isDisabled ? t('accounts.disabled_tooltip') : (isSwitching ? t('common.loading') : t('accounts.switch_to_classic', '切换到 Antigravity (经典版)'))}
                        disabled={isSwitching || isDisabled}
                    >
                        <ArrowRightLeft className={`w-3.5 h-3.5 ${isSwitching ? 'animate-spin' : ''}`} />
                    </button>
                    <button
                        className={`p-1.5 text-gray-500 dark:text-gray-400 rounded-lg transition-all ${(isSwitching || isDisabled) ? 'bg-blue-50 dark:bg-blue-900/10 text-blue-600 dark:text-blue-400 cursor-not-allowed' : 'hover:text-sky-600 dark:hover:text-sky-400 hover:bg-sky-50 dark:hover:bg-sky-900/30'}`}
                        onClick={(e) => { e.stopPropagation(); onSwitch('ide'); }}
                        title={isDisabled ? t('accounts.disabled_tooltip') : (isSwitching ? t('common.loading') : t('accounts.switch_to_ide', '切换到 Antigravity IDE'))}
                        disabled={isSwitching || isDisabled}
                    >
                        <Repeat2 className={`w-3.5 h-3.5 ${isSwitching ? 'animate-spin' : ''}`} />
                    </button>
                    <button
                        className={`p-1.5 text-gray-500 dark:text-gray-400 rounded-lg transition-all ${(isSwitching || isDisabled) ? 'bg-blue-50 dark:bg-blue-900/10 text-blue-600 dark:text-blue-400 cursor-not-allowed' : 'hover:text-emerald-600 dark:hover:text-emerald-400 hover:bg-emerald-50 dark:hover:bg-emerald-900/30'}`}
                        onClick={(e) => { e.stopPropagation(); onSwitch('agy'); }}
                        title={isDisabled ? t('accounts.disabled_tooltip') : (isSwitching ? t('common.loading') : t('accounts.switch_to_agy', '切换到 Antigravity CLI (agy)'))}
                        disabled={isSwitching || isDisabled}
                    >
                        <Terminal className={`w-3.5 h-3.5 ${isSwitching ? 'animate-spin' : ''}`} />
                    </button>
                    {onWarmup && (
                        <button
                            className={`p-1.5 text-gray-500 dark:text-gray-400 rounded-lg transition-all ${(isRefreshing || isDisabled) ? 'bg-orange-50 dark:bg-orange-900/10 text-orange-600 dark:text-orange-400 cursor-not-allowed' : 'hover:text-orange-500 dark:hover:text-orange-400 hover:bg-orange-50 dark:hover:bg-orange-900/30'}`}
                            onClick={(e) => { e.stopPropagation(); onWarmup(); }}
                            title={isDisabled ? t('accounts.disabled_tooltip') : (isRefreshing ? t('common.loading') : t('accounts.warmup_this', '预热该账号'))}
                            disabled={isRefreshing || isDisabled}
                        >
                            <Sparkles className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-pulse' : ''}`} />
                        </button>
                    )}
                    <button
                        className={`p-1.5 text-gray-500 dark:text-gray-400 rounded-lg transition-all ${(isRefreshing || isDisabled) ? 'bg-green-50 dark:bg-green-900/10 text-green-600 dark:text-green-400 cursor-not-allowed' : 'hover:text-green-600 dark:hover:text-green-400 hover:bg-green-50 dark:hover:bg-green-900/30'}`}
                        onClick={(e) => { e.stopPropagation(); onRefresh(); }}
                        title={isDisabled ? t('accounts.disabled_tooltip') : (isRefreshing ? t('common.refreshing') : t('common.refresh'))}
                        disabled={isRefreshing || isDisabled}
                    >
                        <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin' : ''}`} />
                    </button>
                    <button
                        className="p-1.5 text-gray-500 dark:text-gray-400 hover:text-indigo-600 dark:hover:text-indigo-400 hover:bg-indigo-50 dark:hover:bg-indigo-900/30 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); onExport(); }}
                        title={t('common.export')}
                    >
                        <Download className="w-3.5 h-3.5" />
                    </button>
                    <button
                        className={cn(
                            "p-1.5 rounded-lg transition-all",
                            account.proxy_disabled
                                ? "text-gray-500 dark:text-gray-400 hover:text-green-600 dark:hover:text-green-400 hover:bg-green-50 dark:hover:bg-green-900/30"
                                : "text-gray-500 dark:text-gray-400 hover:text-orange-600 dark:hover:text-orange-400 hover:bg-orange-50 dark:hover:bg-orange-900/30"
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
                        className="p-1.5 text-gray-500 dark:text-gray-400 hover:text-red-600 dark:hover:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); onDelete(); }}
                        title={t('common.delete')}
                    >
                        <Trash2 className="w-3.5 h-3.5" />
                    </button>
                </div>
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
    quotaWindow,
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
                const timeA = extractAccountResetTime(a, quotaWindow);
                const timeB = extractAccountResetTime(b, quotaWindow);

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
    }, [accounts, sortConfig, quotaWindow, isSortingActive]);

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
                                    <span>{quotaWindow === 'weekly' ? t('accounts.table.weekly_quota', '周配额') : t('accounts.table.quota')}</span>
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
                            <th className="px-2 py-1 text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider whitespace-nowrap sticky right-0 w-[220px] bg-gray-50 dark:bg-base-200 z-20 shadow-[-12px_0_12px_-12px_rgba(0,0,0,0.1)] dark:shadow-[-12px_0_12px_-12px_rgba(255,255,255,0.05)] text-center">{t('accounts.table.actions')}</th>
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
                                    quotaWindow={quotaWindow}
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
                                        quotaWindow={quotaWindow}
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
