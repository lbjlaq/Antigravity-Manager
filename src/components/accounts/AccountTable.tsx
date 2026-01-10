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
    Clock,
    ToggleLeft,
    ToggleRight,
} from 'lucide-react';
import { Account } from '../../types/account';
import { useTranslation } from 'react-i18next';
import { cn } from '../../utils/cn';
import { getQuotaColor, formatTimeRemaining, getTimeRemainingColor } from '../../utils/format';

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
    onSwitch: (accountId: string) => void;
    onRefresh: (accountId: string) => void;
    onViewDevice: (accountId: string) => void;
    onViewDetails: (accountId: string) => void;
    onExport: (accountId: string) => void;
    onDelete: (accountId: string) => void;
    onToggleProxy: (accountId: string) => void;
    /** 拖拽排序回调，当用户完成拖拽时触发 */
    onReorder?: (accountIds: string[]) => void;
}

interface SortableRowProps {
    account: Account;
    selected: boolean;
    isRefreshing: boolean;
    isCurrent: boolean;
    isSwitching: boolean;
    isDragging?: boolean;
    onSelect: () => void;
    onSwitch: () => void;
    onRefresh: () => void;
    onViewDevice: () => void;
    onViewDetails: () => void;
    onExport: () => void;
    onDelete: () => void;
    onToggleProxy: () => void;
}

interface AccountRowContentProps {
    account: Account;
    isCurrent: boolean;
    isRefreshing: boolean;
    isSwitching: boolean;
    onSwitch: () => void;
    onRefresh: () => void;
    onViewDevice: () => void;
    onViewDetails: () => void;
    onExport: () => void;
    onDelete: () => void;
    onToggleProxy: () => void;
}

// ============================================================================
// 辅助函数
// ============================================================================

/**
 * 根据配额百分比获取对应的背景色类名
 */
function getColorClass(percentage: number): string {
    const color = getQuotaColor(percentage);
    switch (color) {
        case 'success': return 'bg-emerald-500';
        case 'warning': return 'bg-amber-500';
        case 'error': return 'bg-rose-500';
        default: return 'bg-gray-500';
    }
}

/**
 * 根据重置时间获取对应的文字色类名
 */
function getTimeColorClass(resetTime: string | undefined): string {
    const color = getTimeRemainingColor(resetTime);
    switch (color) {
        case 'success': return 'text-emerald-500 dark:text-emerald-400';
        case 'warning': return 'text-amber-500 dark:text-amber-400';
        default: return 'text-gray-400 dark:text-gray-500 opacity-60';
    }
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
}: SortableRowProps) {
    const { t } = useTranslation();
    const {
        attributes,
        listeners,
        setNodeRef,
        transform,
        transition,
        isDragging: isSortableDragging,
    } = useSortable({ id: account.id });

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
            <td className="pl-2 py-1 w-8">
                <div
                    {...attributes}
                    {...listeners}
                    className="flex items-center justify-center w-6 h-6 cursor-grab active:cursor-grabbing text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors rounded hover:bg-gray-100 dark:hover:bg-gray-700"
                    title={t('accounts.drag_to_reorder')}
                >
                    <GripVertical className="w-4 h-4" />
                </div>
            </td>
            {/* 复选框 */}
            <td className="px-2 py-1 w-10">
                <input
                    type="checkbox"
                    className="checkbox checkbox-xs rounded border-2 border-gray-400 dark:border-gray-500 checked:border-blue-600 checked:bg-blue-600 [--chkbg:theme(colors.blue.600)] [--chkfg:white]"
                    checked={selected}
                    onChange={onSelect}
                    onClick={(e) => e.stopPropagation()}
                />
            </td>
            <AccountRowContent
                account={account}
                isCurrent={isCurrent}
                isRefreshing={isRefreshing}
                isSwitching={isSwitching}
                onSwitch={onSwitch}
                onRefresh={onRefresh}
                onViewDevice={onViewDevice}
                onViewDetails={onViewDetails}
                onExport={onExport}
                onDelete={onDelete}
                onToggleProxy={onToggleProxy}
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
    onSwitch,
    onRefresh,
    onViewDevice,
    onViewDetails,
    onExport,
    onDelete,
    onToggleProxy,
}: AccountRowContentProps) {
    const { t } = useTranslation();
    const geminiProModel = account.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-pro-high');
    const geminiFlashModel = account.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-flash');
    const geminiImageModel = account.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-pro-image');
    const claudeModel = account.quota?.models.find(m => m.name.toLowerCase() === 'claude-sonnet-4-5-thinking');
    const isDisabled = Boolean(account.disabled);

    return (
        <>
            {/* 邮箱列 */}
            <td className="px-4 py-1">
                <div className="flex items-center gap-3">
                    <span className={cn(
                        "font-medium text-sm truncate max-w-[180px] xl:max-w-none transition-colors",
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
                                title={account.disabled_reason || t('accounts.disabled_tooltip')}
                            >
                                <Ban className="w-2.5 h-2.5" />
                                <span>{t('accounts.disabled')}</span>
                            </span>
                        )}

                        {account.proxy_disabled && (
                            <span
                                className="px-2 py-0.5 rounded-md bg-orange-100 dark:bg-orange-900/50 text-orange-700 dark:text-orange-300 text-[10px] font-bold flex items-center gap-1 shadow-sm border border-orange-200/50"
                                title={account.proxy_disabled_reason || t('accounts.proxy_disabled_tooltip')}
                            >
                                <Ban className="w-2.5 h-2.5" />
                                <span>{t('accounts.proxy_disabled')}</span>
                            </span>
                        )}

                        {account.quota?.is_forbidden && (
                            <span className="px-2 py-0.5 rounded-md bg-red-100 dark:bg-red-900/50 text-red-600 dark:text-red-400 text-[10px] font-bold flex items-center gap-1 shadow-sm border border-red-200/50" title={t('accounts.forbidden_tooltip')}>
                                <Lock className="w-2.5 h-2.5" />
                                <span>{t('accounts.forbidden')}</span>
                            </span>
                        )}

                        {/* 订阅类型徽章 */}
                        {account.quota?.subscription_tier && (() => {
                            const tier = account.quota.subscription_tier.toLowerCase();
                            if (tier.includes('ultra')) {
                                return (
                                    <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-gradient-to-r from-purple-600 to-pink-600 text-white text-[10px] font-bold shadow-sm hover:scale-105 transition-transform cursor-default">
                                        <Gem className="w-2.5 h-2.5 fill-current" />
                                        ULTRA
                                    </span>
                                );
                            } else if (tier.includes('pro')) {
                                return (
                                    <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-gradient-to-r from-blue-600 to-indigo-600 text-white text-[10px] font-bold shadow-sm hover:scale-105 transition-transform cursor-default">
                                        <Diamond className="w-2.5 h-2.5 fill-current" />
                                        PRO
                                    </span>
                                );
                            } else {
                                return (
                                    <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-gray-100 dark:bg-white/10 text-gray-600 dark:text-gray-400 text-[10px] font-bold shadow-sm border border-gray-200 dark:border-white/10 hover:bg-gray-200 transition-colors cursor-default">
                                        <Circle className="w-2.5 h-2.5" />
                                        FREE
                                    </span>
                                );
                            }
                        })()}
                    </div>
                </div>
            </td>

            {/* 模型配额列 */}
            <td className="px-4 py-1">
                {account.quota?.is_forbidden ? (
                    <div className="flex items-center gap-2 text-xs text-red-500 dark:text-red-400 bg-red-50/50 dark:bg-red-900/10 p-1.5 rounded-lg border border-red-100 dark:border-red-900/30">
                        <Ban className="w-4 h-4 shrink-0" />
                        <span>{t('accounts.forbidden_msg')}</span>
                    </div>
                ) : (
                    <div className="grid grid-cols-2 gap-x-4 gap-y-1 py-0">
                        {/* Gemini Pro */}
                        <div className="relative h-[22px] flex items-center px-1.5 rounded-md overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5 group/quota">
                            {geminiProModel && (
                                <div
                                    className={`absolute inset-y-0 left-0 transition-all duration-700 ease-out opacity-15 dark:opacity-20 ${getColorClass(geminiProModel.percentage)}`}
                                    style={{ width: `${geminiProModel.percentage}%` }}
                                />
                            )}
                            <div className="relative z-10 w-full flex items-center text-[10px] font-mono leading-none">
                                <span className="w-[54px] text-gray-500 dark:text-gray-400 font-bold truncate pr-1" title="Gemini 3 Pro">G3 Pro</span>
                                <div className="flex-1 flex justify-center">
                                    {geminiProModel?.reset_time ? (
                                        <span className={cn("flex items-center gap-0.5 font-medium transition-colors", getTimeColorClass(geminiProModel.reset_time))}>
                                            <Clock className="w-2.5 h-2.5" />
                                            {formatTimeRemaining(geminiProModel.reset_time)}
                                        </span>
                                    ) : (
                                        <span className="text-gray-300 dark:text-gray-600 italic scale-90">N/A</span>
                                    )}
                                </div>
                                <span className={cn("w-[36px] text-right font-bold transition-colors",
                                    getQuotaColor(geminiProModel?.percentage || 0) === 'success' ? 'text-emerald-600 dark:text-emerald-400' :
                                        getQuotaColor(geminiProModel?.percentage || 0) === 'warning' ? 'text-amber-600 dark:text-amber-400' : 'text-rose-600 dark:text-rose-400'
                                )}>
                                    {geminiProModel ? `${geminiProModel.percentage}%` : '-'}
                                </span>
                            </div>
                        </div>

                        {/* Gemini Flash */}
                        <div className="relative h-[22px] flex items-center px-1.5 rounded-md overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5 group/quota">
                            {geminiFlashModel && (
                                <div
                                    className={`absolute inset-y-0 left-0 transition-all duration-700 ease-out opacity-15 dark:opacity-20 ${getColorClass(geminiFlashModel.percentage)}`}
                                    style={{ width: `${geminiFlashModel.percentage}%` }}
                                />
                            )}
                            <div className="relative z-10 w-full flex items-center text-[10px] font-mono leading-none">
                                <span className="w-[54px] text-gray-500 dark:text-gray-400 font-bold truncate pr-1" title="Gemini 3 Flash">G3 Flash</span>
                                <div className="flex-1 flex justify-center">
                                    {geminiFlashModel?.reset_time ? (
                                        <span className={cn("flex items-center gap-0.5 font-medium transition-colors", getTimeColorClass(geminiFlashModel.reset_time))}>
                                            <Clock className="w-2.5 h-2.5" />
                                            {formatTimeRemaining(geminiFlashModel.reset_time)}
                                        </span>
                                    ) : (
                                        <span className="text-gray-300 dark:text-gray-600 italic scale-90">N/A</span>
                                    )}
                                </div>
                                <span className={cn("w-[36px] text-right font-bold transition-colors",
                                    getQuotaColor(geminiFlashModel?.percentage || 0) === 'success' ? 'text-emerald-600 dark:text-emerald-400' :
                                        getQuotaColor(geminiFlashModel?.percentage || 0) === 'warning' ? 'text-amber-600 dark:text-amber-400' : 'text-rose-600 dark:text-rose-400'
                                )}>
                                    {geminiFlashModel ? `${geminiFlashModel.percentage}%` : '-'}
                                </span>
                            </div>
                        </div>

                        {/* Gemini Image */}
                        <div className="relative h-[22px] flex items-center px-1.5 rounded-md overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5 group/quota">
                            {geminiImageModel && (
                                <div
                                    className={`absolute inset-y-0 left-0 transition-all duration-700 ease-out opacity-15 dark:opacity-20 ${getColorClass(geminiImageModel.percentage)}`}
                                    style={{ width: `${geminiImageModel.percentage}%` }}
                                />
                            )}
                            <div className="relative z-10 w-full flex items-center text-[10px] font-mono leading-none">
                                <span className="w-[54px] text-gray-500 dark:text-gray-400 font-bold truncate pr-1" title="Gemini 3 Pro Image">G3 Image</span>
                                <div className="flex-1 flex justify-center">
                                    {geminiImageModel?.reset_time ? (
                                        <span className={cn("flex items-center gap-0.5 font-medium transition-colors", getTimeColorClass(geminiImageModel.reset_time))}>
                                            <Clock className="w-2.5 h-2.5" />
                                            {formatTimeRemaining(geminiImageModel.reset_time)}
                                        </span>
                                    ) : (
                                        <span className="text-gray-300 dark:text-gray-600 italic scale-90">N/A</span>
                                    )}
                                </div>
                                <span className={cn("w-[36px] text-right font-bold transition-colors",
                                    getQuotaColor(geminiImageModel?.percentage || 0) === 'success' ? 'text-emerald-600 dark:text-emerald-400' :
                                        getQuotaColor(geminiImageModel?.percentage || 0) === 'warning' ? 'text-amber-600 dark:text-amber-400' : 'text-rose-600 dark:text-rose-400'
                                )}>
                                    {geminiImageModel ? `${geminiImageModel.percentage}%` : '-'}
                                </span>
                            </div>
                        </div>

                        {/* Claude */}
                        <div className="relative h-[22px] flex items-center px-1.5 rounded-md overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5 group/quota">
                            {claudeModel && (
                                <div
                                    className={`absolute inset-y-0 left-0 transition-all duration-700 ease-out opacity-15 dark:opacity-20 ${getColorClass(claudeModel.percentage)}`}
                                    style={{ width: `${claudeModel.percentage}%` }}
                                />
                            )}
                            <div className="relative z-10 w-full flex items-center text-[10px] font-mono leading-none">
                                <span className="w-[54px] text-gray-500 dark:text-gray-400 font-bold truncate pr-1" title="Claude-sonnet-4.5">Claude 4.5</span>
                                <div className="flex-1 flex justify-center">
                                    {claudeModel?.reset_time ? (
                                        <span className={cn("flex items-center gap-0.5 font-medium transition-colors", getTimeColorClass(claudeModel.reset_time))}>
                                            <Clock className="w-2.5 h-2.5" />
                                            {formatTimeRemaining(claudeModel.reset_time)}
                                        </span>
                                    ) : (
                                        <span className="text-gray-300 dark:text-gray-600 italic scale-90">N/A</span>
                                    )}
                                </div>
                                <span className={cn("w-[36px] text-right font-bold transition-colors",
                                    getQuotaColor(claudeModel?.percentage || 0) === 'success' ? 'text-emerald-600 dark:text-emerald-400' :
                                        getQuotaColor(claudeModel?.percentage || 0) === 'warning' ? 'text-amber-600 dark:text-amber-400' : 'text-rose-600 dark:text-rose-400'
                                )}>
                                    {claudeModel ? `${claudeModel.percentage}%` : '-'}
                                </span>
                            </div>
                        </div>
                    </div>
                )}
            </td>

            {/* 最后使用时间列 */}
            <td className="px-4 py-1">
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
                "px-4 py-1 sticky right-0 z-10 shadow-[-12px_0_12px_-12px_rgba(0,0,0,0.1)] dark:shadow-[-12px_0_12px_-12px_rgba(255,255,255,0.05)] text-center",
                // 动态背景色处理
                isCurrent
                    ? "bg-[#f1f6ff] dark:bg-[#1e2330]" // 接近 blue-50/50 的实色
                    : "bg-white dark:bg-base-100",
                !isCurrent && "group-hover:bg-gray-50 dark:group-hover:bg-base-200"
            )}>
                <div className="flex items-center justify-center gap-0.5 opacity-60 group-hover:opacity-100 transition-opacity">
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
                        title="设备指纹"
                    >
                        <Fingerprint className="w-3.5 h-3.5" />
                    </button>
                    <button
                        className={`p-1.5 text-gray-500 dark:text-gray-400 rounded-lg transition-all ${(isSwitching || isDisabled) ? 'bg-blue-50 dark:bg-blue-900/10 text-blue-600 dark:text-blue-400 cursor-not-allowed' : 'hover:text-blue-600 dark:hover:text-blue-400 hover:bg-blue-50 dark:hover:bg-blue-900/30'}`}
                        onClick={(e) => { e.stopPropagation(); onSwitch(); }}
                        title={isDisabled ? t('accounts.disabled_tooltip') : (isSwitching ? t('common.loading') : t('accounts.switch_to'))}
                        disabled={isSwitching || isDisabled}
                    >
                        <ArrowRightLeft className={`w-3.5 h-3.5 ${isSwitching ? 'animate-spin' : ''}`} />
                    </button>
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
}: AccountTableProps) {
    const { t } = useTranslation();
    const [activeId, setActiveId] = useState<string | null>(null);

    // 配置拖拽传感器
    const sensors = useSensors(
        useSensor(PointerSensor, {
            activationConstraint: { distance: 8 }, // 需要移动 8px 才触发拖拽
        }),
        useSensor(KeyboardSensor, {
            coordinateGetter: sortableKeyboardCoordinates,
        })
    );

    const accountIds = useMemo(() => accounts.map(a => a.id), [accounts]);
    const activeAccount = useMemo(() => accounts.find(a => a.id === activeId), [accounts, activeId]);

    const handleDragStart = (event: DragStartEvent) => {
        setActiveId(event.active.id as string);
    };

    const handleDragEnd = (event: DragEndEvent) => {
        const { active, over } = event;
        setActiveId(null);

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
                            <th className="px-4 py-1 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider whitespace-nowrap">{t('accounts.table.email')}</th>
                            <th className="px-4 py-1 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider w-[440px] min-w-[360px] whitespace-nowrap">{t('accounts.table.quota')}</th>
                            <th className="px-4 py-1 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider whitespace-nowrap">{t('accounts.table.last_used')}</th>
                            <th className="px-4 py-1 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider whitespace-nowrap sticky right-0 bg-gray-50 dark:bg-base-200 z-20 shadow-[-12px_0_12px_-12px_rgba(0,0,0,0.1)] dark:shadow-[-12px_0_12px_-12px_rgba(255,255,255,0.05)] text-center">{t('accounts.table.actions')}</th>
                        </tr>
                    </thead>
                    <SortableContext items={accountIds} strategy={verticalListSortingStrategy}>
                        <tbody className="divide-y divide-gray-100 dark:divide-base-200">
                            {accounts.map((account) => (
                                <SortableAccountRow
                                    key={account.id}
                                    account={account}
                                    selected={selectedIds.has(account.id)}
                                    isRefreshing={refreshingIds.has(account.id)}
                                    isCurrent={account.id === currentAccountId}
                                    isSwitching={account.id === switchingAccountId}
                                    isDragging={account.id === activeId}
                                    onSelect={() => onToggleSelect(account.id)}
                                    onSwitch={() => onSwitch(account.id)}
                                    onRefresh={() => onRefresh(account.id)}
                                    onViewDevice={() => onViewDevice(account.id)}
                                    onViewDetails={() => onViewDetails(account.id)}
                                    onExport={() => onExport(account.id)}
                                    onDelete={() => onDelete(account.id)}
                                    onToggleProxy={() => onToggleProxy(account.id)}
                                />
                            ))}
                        </tbody>
                    </SortableContext>
                </table>
            </div>

            {/* 拖拽悬浮预览层 */}
            <DragOverlay>
                {activeAccount ? (
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
                                />
                            </tr>
                        </tbody>
                    </table>
                ) : null}
            </DragOverlay>
        </DndContext>
    );
}

export default AccountTable;
