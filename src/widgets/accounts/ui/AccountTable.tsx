// File: src/widgets/accounts/ui/AccountTable.tsx
// Account list table with drag-and-drop reordering

import { useMemo, useState, memo, useCallback, useRef, useEffect } from 'react';
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
    ToggleLeft,
    ToggleRight,
    Sparkles,
    MoreVertical,
    Clock,
    ExternalLink, // [NEW]
} from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import { Account } from '@/entities/account';
import { useTranslation } from 'react-i18next';
import { cn, formatTimeRemaining } from '@/shared/lib';
import { useConfigStore } from '@/entities/config';
import { openUrl } from '@tauri-apps/plugin-opener';

// ===================================
// Constants - Fixed Column Widths
// ===================================
const COLUMN_WIDTHS = {
    drag: 28,
    checkbox: 28,
    email: 280,
    quota: 'flex', // takes remaining space (includes last used)
    actions: 64,
} as const;

// ===================================
// Helper - Model Label (readable)
// ===================================
function getModelLabel(id: string): string {
    const lower = id.toLowerCase();
    // Claude Opus
    if (lower.includes('claude-opus-4-6') || lower.includes('claude-opus-4.6')) return 'Opus 4.6';
    if (lower.includes('claude-opus-4-5')) return 'Opus 4.5';
    if (lower.includes('claude-opus-4')) return 'Opus 4';
    if (lower.includes('claude-opus')) return 'Opus';
    // Claude Sonnet
    if (lower.includes('claude-sonnet-4-5')) return 'Sonnet 4.5';
    if (lower.includes('claude-sonnet-4')) return 'Sonnet 4';
    if (lower.includes('claude-3-7')) return 'Claude 3.7';
    if (lower.includes('claude-3-5')) return 'Claude 3.5';
    if (lower.includes('claude')) return 'Claude';
    // Gemini 3
    if (lower.includes('gemini-3-pro-image')) return 'G3 Image';
    if (lower.includes('gemini-3-pro-high')) return 'G3 Pro';
    if (lower.includes('gemini-3-pro-low')) return 'G3 Low';
    if (lower.includes('gemini-3-pro')) return 'G3 Pro';
    if (lower.includes('gemini-3-flash')) return 'G3 Flash';
    // Gemini 2.5
    if (lower.includes('gemini-2.5-flash-lite')) return 'G2.5 Lite';
    if (lower.includes('gemini-2.5-flash-thinking')) return 'G2.5 Think';
    if (lower.includes('gemini-2.5-flash')) return 'G2.5 Flash';
    if (lower.includes('gemini-2.5-pro')) return 'G2.5 Pro';
    // Gemini 2.0
    if (lower.includes('gemini-2.0-flash')) return 'G2 Flash';
    if (lower.includes('gemini-2.0-pro')) return 'G2 Pro';
    // GPT/O1
    if (lower.includes('gpt-4o')) return 'GPT-4o';
    if (lower.includes('o1-mini')) return 'O1 Mini';
    if (lower.includes('o1-preview')) return 'O1 Preview';
    // Fallback - shorten if too long
    if (id.length > 12) return id.slice(0, 10) + '…';
    return id;
}

// ===================================
// Types
// ===================================
interface AccountTableProps {
    accounts: Account[];
    allAccounts?: Account[];
    selectedIds: Set<string>;
    refreshingIds: Set<string>;
    proxySelectedAccountIds?: Set<string>;
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
    onWarmup?: (accountId: string) => void;
    onReorder?: (accountIds: string[]) => void;
}

interface SortableRowProps {
    account: Account;
    selected: boolean;
    isRefreshing: boolean;
    isCurrent: boolean;
    isSwitching: boolean;
    isSelectedForProxy?: boolean;
    index: number;
    totalCount: number;
    onSelect: () => void;
    onSwitch: () => void;
    onRefresh: () => void;
    onViewDevice: () => void;
    onViewDetails: () => void;
    onExport: () => void;
    onDelete: () => void;
    onToggleProxy: () => void;
    onWarmup?: () => void;
}

// ===================================
// Row Component
// ===================================
function SortableAccountRow({
    account, selected, isRefreshing, isCurrent, isSwitching, isSelectedForProxy, index, totalCount,
    onSelect, onSwitch, onRefresh, onViewDevice, onViewDetails, onExport, onDelete, onToggleProxy, onWarmup
}: SortableRowProps) {
    const { t } = useTranslation();
    const { attributes, listeners, setNodeRef, transform, transition, isDragging: isSortableDragging } = useSortable({ id: account.id });
    const config = useConfigStore(s => s.config);
    const [menuOpen, setMenuOpen] = useState(false);
    const menuRef = useRef<HTMLDivElement>(null);

    // Close menu on outside click
    const handleClickOutside = useCallback((e: MouseEvent) => {
        if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
            setMenuOpen(false);
        }
    }, []);

    useEffect(() => {
        if (menuOpen) {
            document.addEventListener('mousedown', handleClickOutside);
            return () => document.removeEventListener('mousedown', handleClickOutside);
        }
    }, [menuOpen, handleClickOutside]);

    const style = {
        transform: CSS.Transform.toString(transform),
        transition,
        opacity: isSortableDragging ? 0.4 : 1,
        zIndex: isSortableDragging ? 50 : undefined,
    };

    const pinned = config?.pinned_quota_models?.models || [];
    const modelsToShow = pinned.length > 0 ? pinned : [
        'gemini-3-pro-high',
        'gemini-3-flash',
        'claude-sonnet-4-5-thinking'
    ];

    const lastUsedDate = new Date(account.last_used * 1000);
    const lastUsedStr = `${lastUsedDate.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })} ${lastUsedDate.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })}`;

    const menuItems = [
        { icon: Info, label: t('common.details'), onClick: onViewDetails, color: 'text-zinc-600 dark:text-zinc-400' },
        { icon: Fingerprint, label: t('accounts.device_fingerprint'), onClick: onViewDevice, color: 'text-zinc-600 dark:text-zinc-400' },
        { icon: ArrowRightLeft, label: t('common.switch'), onClick: onSwitch, color: 'text-indigo-600 dark:text-indigo-400', disabled: isSwitching },
        ...(onWarmup ? [{ icon: Sparkles, label: t('accounts.warmup_this'), onClick: onWarmup, color: 'text-amber-600 dark:text-amber-400', disabled: isRefreshing }] : []),
        { icon: RefreshCw, label: t('common.refresh'), onClick: onRefresh, color: 'text-emerald-600 dark:text-emerald-400', disabled: isRefreshing },
        { icon: Download, label: t('common.export'), onClick: onExport, color: 'text-blue-600 dark:text-blue-400' },
        { icon: account.proxy_disabled ? ToggleRight : ToggleLeft, label: t('accounts.toggle_proxy'), onClick: onToggleProxy, color: 'text-orange-600 dark:text-orange-400' },
        { icon: Trash2, label: t('common.delete'), onClick: onDelete, color: 'text-rose-600 dark:text-rose-400', danger: true },
    ];

    return (
        <div 
            ref={setNodeRef} 
            style={style} 
            id={account.id} 
            className={cn(
                "group flex items-center h-[52px] px-1 rounded-md transition-all duration-150 w-full",
                "border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900",
                "hover:border-zinc-300 dark:hover:border-zinc-700 hover:bg-zinc-50 dark:hover:bg-zinc-800/80",
                isCurrent && "border-indigo-300 dark:border-indigo-500/30 bg-indigo-50 dark:bg-indigo-950/30",
                selected && !isCurrent && "border-indigo-300 dark:border-indigo-500/30 bg-indigo-50/50 dark:bg-indigo-950/20",
            )}
        >
            {/* Drag Handle */}
            <div style={{ width: COLUMN_WIDTHS.drag }} className="flex justify-center shrink-0">
                <div {...attributes} {...listeners} className="p-1 cursor-grab active:cursor-grabbing text-zinc-400 dark:text-zinc-600 hover:text-zinc-600 dark:hover:text-zinc-400 transition-colors">
                    <GripVertical className="w-4 h-4" />
                </div>
            </div>

            {/* Checkbox */}
            <div style={{ width: COLUMN_WIDTHS.checkbox }} className="flex justify-center shrink-0">
                <div 
                    className={cn(
                        "w-4 h-4 rounded border flex items-center justify-center transition-all cursor-pointer",
                        selected 
                            ? "bg-indigo-500 border-indigo-500" 
                            : "border-zinc-300 dark:border-zinc-600 group-hover:border-zinc-400 dark:group-hover:border-zinc-500 bg-transparent"
                    )}
                    onClick={(e) => { e.stopPropagation(); onSelect(); }}
                >
                    {selected && <div className="w-2 h-2 rounded-sm bg-white" />}
                </div>
            </div>

            {/* Email & Account Info */}
            <div style={{ width: COLUMN_WIDTHS.email }} className="flex flex-col min-w-0 px-2 shrink-0">
                <div className="flex items-center gap-1 mb-0.5">
                    {isSelectedForProxy && <span className="px-1 py-0.5 rounded bg-green-100 dark:bg-green-500/20 text-green-700 dark:text-green-400 text-[8px] font-bold">SEL</span>}
                    {(() => {
                        const tier = (account.quota?.subscription_tier || '').toLowerCase();
                        if (tier.includes('ultra')) return <span className="px-1 py-0.5 rounded bg-purple-100 dark:bg-purple-500/20 text-purple-700 dark:text-purple-400 text-[8px] font-bold">ULTRA</span>;
                        if (tier.includes('pro')) return <span className="px-1 py-0.5 rounded bg-blue-100 dark:bg-blue-500/20 text-blue-700 dark:text-blue-400 text-[8px] font-bold">PRO</span>;
                        return <span className="px-1 py-0.5 rounded bg-zinc-100 dark:bg-zinc-800 text-zinc-500 dark:text-zinc-500 text-[8px] font-bold">FREE</span>;
                    })()}
                    {isCurrent && <span className="px-1 py-0.5 rounded bg-indigo-100 dark:bg-indigo-500/20 text-indigo-700 dark:text-indigo-400 text-[8px] font-bold">NOW</span>}
                </div>
                <span className={cn(
                    "text-xs font-medium truncate",
                    isCurrent ? "text-indigo-600 dark:text-indigo-300" : "text-zinc-800 dark:text-zinc-200"
                )} title={account.email}>
                    {account.email}
                </span>
                {(account.disabled || account.proxy_disabled) && (
                    <div className="flex items-center gap-1 mt-0.5">
                        {account.disabled && <span className="px-1 py-0.5 rounded bg-rose-100 dark:bg-rose-500/20 text-rose-600 dark:text-rose-400 text-[7px] font-bold">DISABLED</span>}
                        {account.proxy_disabled && <span className="px-1 py-0.5 rounded bg-orange-100 dark:bg-orange-500/20 text-orange-600 dark:text-orange-400 text-[7px] font-bold">NO PROXY</span>}
                        {account.verification_needed && (
                             <button 
                                onClick={(e) => {
                                    e.stopPropagation();
                                    if (account.verification_url) {
                                        openUrl(account.verification_url).catch(() => window.open(account.verification_url, '_blank'));
                                    }
                                }}
                                className="px-1.5 py-0.5 rounded bg-red-100 dark:bg-red-500/20 text-red-700 dark:text-red-400 text-[8px] font-bold hover:bg-red-200 dark:hover:bg-red-500/30 transition-colors flex items-center gap-1 animate-pulse"
                            >
                                <ExternalLink className="w-2 h-2" /> VERIFY NEEDED
                            </button>
                        )}
                    </div>
                )}
            </div>

            {/* Quota Models + Last Used */}
            <div className="flex-1 min-w-0 px-2 flex items-center gap-3">
                {/* Quota pills */}
                <div className="flex items-center gap-1.5 flex-1 min-w-0">
                    {modelsToShow.slice(0, 4).map(modelId => {
                        const m = account.quota?.models.find(m => m.name === modelId);
                        if (!m) return (
                            <div key={modelId} className="flex items-center gap-1 px-1.5 py-0.5 rounded bg-zinc-100 dark:bg-zinc-800/50 text-[8px] text-zinc-400 shrink-0">
                                <span className="font-medium">{getModelLabel(modelId)}</span>
                                <span>—</span>
                            </div>
                        );
                        
                        const timeRemaining = formatTimeRemaining(m.reset_time);
                        const isLow = m.percentage < 20;
                        const isMedium = m.percentage >= 20 && m.percentage <= 60;
                        
                        return (
                            <div 
                                key={modelId} 
                                className={cn(
                                    "flex items-center gap-1 px-1.5 py-0.5 rounded text-[9px] font-medium shrink-0",
                                    "border",
                                    isLow && "bg-rose-50 dark:bg-rose-950/40 border-rose-200/60 dark:border-rose-800/40",
                                    isMedium && "bg-amber-50 dark:bg-amber-950/40 border-amber-200/60 dark:border-amber-800/40",
                                    !isLow && !isMedium && "bg-emerald-50 dark:bg-emerald-950/40 border-emerald-200/60 dark:border-emerald-800/40"
                                )}
                                title={`${m.percentage}% • Reset: ${m.reset_time ? new Date(m.reset_time).toLocaleString() : '—'}`}
                            >
                                {/* Model name */}
                                <span className={cn(
                                    "font-semibold",
                                    isLow && "text-rose-600 dark:text-rose-400",
                                    isMedium && "text-amber-600 dark:text-amber-400",
                                    !isLow && !isMedium && "text-emerald-600 dark:text-emerald-400"
                                )}>
                                    {getModelLabel(modelId)}
                                </span>
                                
                                {/* Percentage */}
                                <span className={cn(
                                    "font-mono font-bold tabular-nums",
                                    isLow && "text-rose-700 dark:text-rose-300",
                                    isMedium && "text-amber-700 dark:text-amber-300",
                                    !isLow && !isMedium && "text-emerald-700 dark:text-emerald-300"
                                )}>
                                    {m.percentage}%
                                </span>
                                
                                {/* Reset time - compact */}
                                <span className={cn(
                                    "flex items-center gap-0.5 text-[8px] opacity-70",
                                    isLow && "text-rose-500 dark:text-rose-400",
                                    isMedium && "text-amber-500 dark:text-amber-400",
                                    !isLow && !isMedium && "text-emerald-500 dark:text-emerald-400"
                                )}>
                                    <Clock className="w-2 h-2" />
                                    {timeRemaining}
                                </span>
                            </div>
                        );
                    })}
                    
                    {/* Show "+N more" if more than 4 models - with hover tooltip */}
                    {modelsToShow.length > 4 && (() => {
                        // Smart positioning: show tooltip UP if in bottom half, DOWN if in top half
                        const showAbove = index >= totalCount / 2;
                        
                        return (
                            <div className="relative group/more shrink-0">
                                <div className="px-1.5 py-0.5 rounded bg-zinc-100 dark:bg-zinc-800 text-[8px] text-zinc-500 font-medium cursor-pointer hover:bg-zinc-200 dark:hover:bg-zinc-700 transition-colors">
                                    +{modelsToShow.length - 4}
                                </div>
                                
                                {/* Hover tooltip - smart positioning based on row position */}
                                <div className={cn(
                                    "absolute left-0 z-[100] opacity-0 invisible group-hover/more:opacity-100 group-hover/more:visible transition-all duration-150 pointer-events-none group-hover/more:pointer-events-auto",
                                    showAbove ? "bottom-full mb-1" : "top-full mt-1"
                                )}>
                                    <div className="bg-white dark:bg-zinc-800 rounded-lg shadow-xl border border-zinc-200 dark:border-zinc-700 p-2 min-w-[280px]">
                                        <div className="text-[9px] font-semibold text-zinc-500 dark:text-zinc-400 uppercase tracking-wide mb-1.5 px-1">
                                            More Models
                                        </div>
                                        <div className="space-y-1">
                                            {modelsToShow.slice(4).map(modelId => {
                                                const m = account.quota?.models.find(m => m.name === modelId);
                                                // Format full model name for display
                                                const fullName = modelId
                                                    .replace(/-/g, ' ')
                                                    .replace(/\b\w/g, c => c.toUpperCase());
                                                
                                                if (!m) return (
                                                    <div key={modelId} className="flex items-center justify-between gap-2 px-1.5 py-1 rounded bg-zinc-50 dark:bg-zinc-900/50 text-[9px]">
                                                        <span className="font-medium text-zinc-500">{fullName}</span>
                                                        <span className="text-zinc-400">—</span>
                                                    </div>
                                                );
                                                
                                                const timeRemaining = formatTimeRemaining(m.reset_time);
                                                const isLow = m.percentage < 20;
                                                const isMedium = m.percentage >= 20 && m.percentage < 50;
                                                
                                                return (
                                                    <div 
                                                        key={modelId}
                                                        className={cn(
                                                            "flex items-center justify-between gap-3 px-2 py-1.5 rounded text-[9px]",
                                                            isLow && "bg-rose-50 dark:bg-rose-950/40",
                                                            isMedium && "bg-amber-50 dark:bg-amber-950/40",
                                                            !isLow && !isMedium && "bg-emerald-50 dark:bg-emerald-950/40"
                                                        )}
                                                    >
                                                        {/* Full model name */}
                                                        <span className={cn(
                                                            "font-medium",
                                                            isLow && "text-rose-600 dark:text-rose-400",
                                                            isMedium && "text-amber-600 dark:text-amber-400",
                                                            !isLow && !isMedium && "text-emerald-600 dark:text-emerald-400"
                                                        )}>
                                                            {fullName}
                                                        </span>
                                                        
                                                        {/* Right side: percentage + time */}
                                                        <div className="flex items-center gap-2 shrink-0">
                                                            <span className={cn(
                                                                "font-mono font-bold tabular-nums",
                                                                isLow && "text-rose-700 dark:text-rose-300",
                                                                isMedium && "text-amber-700 dark:text-amber-300",
                                                                !isLow && !isMedium && "text-emerald-700 dark:text-emerald-300"
                                                            )}>
                                                                {m.percentage}%
                                                            </span>
                                                            
                                                            <span className={cn(
                                                                "flex items-center gap-0.5 text-[8px]",
                                                                isLow && "text-rose-500 dark:text-rose-400",
                                                                isMedium && "text-amber-500 dark:text-amber-400",
                                                                !isLow && !isMedium && "text-emerald-500 dark:text-emerald-400"
                                                            )}>
                                                                <Clock className="w-2 h-2" />
                                                                {timeRemaining}
                                                            </span>
                                                        </div>
                                                    </div>
                                                );
                                            })}
                                        </div>
                                    </div>
                                </div>
                            </div>
                        );
                    })()}
                </div>
                
                {/* Last Used - right side */}
                <div className="shrink-0 text-[9px] text-zinc-400 dark:text-zinc-500 font-mono whitespace-nowrap" title={lastUsedDate.toLocaleString()}>
                    {lastUsedStr}
                </div>
            </div>

            {/* Actions - Dropdown Menu */}
            <div style={{ width: COLUMN_WIDTHS.actions }} className="flex items-center justify-center shrink-0 relative" ref={menuRef}>
                <button 
                    onClick={(e) => { e.stopPropagation(); setMenuOpen(!menuOpen); }}
                    className={cn(
                        "p-1.5 rounded-md transition-colors",
                        menuOpen 
                            ? "bg-zinc-200 dark:bg-zinc-700 text-zinc-900 dark:text-white" 
                            : "text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800"
                    )}
                >
                    <MoreVertical className="w-4 h-4" />
                </button>

                <AnimatePresence>
                    {menuOpen && (() => {
                        // Smart positioning: show menu UP if in bottom half, DOWN if in top half
                        const showAbove = index >= totalCount / 2;
                        
                        return (
                            <motion.div
                                initial={{ opacity: 0, scale: 0.95, y: showAbove ? 5 : -5 }}
                                animate={{ opacity: 1, scale: 1, y: 0 }}
                                exit={{ opacity: 0, scale: 0.95, y: showAbove ? 5 : -5 }}
                                transition={{ duration: 0.12 }}
                                className={cn(
                                    "absolute right-0 z-50 min-w-[140px] py-0.5 bg-white dark:bg-zinc-800 rounded-md shadow-lg border border-zinc-200 dark:border-zinc-700",
                                    showAbove ? "bottom-full mb-1" : "top-full mt-1"
                                )}
                                onClick={(e) => e.stopPropagation()}
                            >
                                {menuItems.map((item, idx) => (
                                    <button
                                        key={idx}
                                        onClick={() => { item.onClick(); setMenuOpen(false); }}
                                        disabled={item.disabled}
                                        className={cn(
                                            "w-full flex items-center gap-2 px-2.5 py-1.5 text-left text-xs transition-colors disabled:opacity-50",
                                            item.danger 
                                                ? "hover:bg-rose-50 dark:hover:bg-rose-500/10" 
                                                : "hover:bg-zinc-100 dark:hover:bg-zinc-700/50",
                                            item.color
                                        )}
                                    >
                                        <item.icon className="w-3.5 h-3.5" />
                                        <span className="text-zinc-700 dark:text-zinc-200">{item.label}</span>
                                    </button>
                                ))}
                            </motion.div>
                        );
                    })()}
                </AnimatePresence>
            </div>
        </div>
    );
}

// ===================================
// Main Component
// ===================================
const restrictToVerticalAxis = ({ transform }: { transform: { x: number; y: number; scaleX: number; scaleY: number } }) => ({
    ...transform,
    x: 0,
});

export const AccountTable = memo(function AccountTable({
    accounts, allAccounts, selectedIds, refreshingIds, proxySelectedAccountIds, onToggleSelect, onToggleAll,
    currentAccountId, switchingAccountId, onSwitch, onRefresh, onViewDevice,
    onViewDetails, onExport, onDelete, onToggleProxy, onReorder, onWarmup
}: AccountTableProps) {
    const { t } = useTranslation();
    const [activeId, setActiveId] = useState<string | null>(null);
    const [draggedWidth, setDraggedWidth] = useState<number | undefined>(undefined);

    const sensors = useSensors(
        useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
        useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates })
    );

    const accountIds = useMemo(() => accounts.map(a => a.id), [accounts]);
    const fullAccountIds = useMemo(() => (allAccounts || accounts).map(a => a.id), [allAccounts, accounts]);
    const activeAccount = useMemo(() => accounts.find(a => a.id === activeId), [accounts, activeId]);

    const handleDragStart = (event: DragStartEvent) => {
        const id = event.active.id as string;
        setActiveId(id);
        const node = document.getElementById(id);
        if (node) {
            setDraggedWidth(node.offsetWidth);
        }
    };

    const handleDragEnd = (event: DragEndEvent) => {
        const { active, over } = event;
        setActiveId(null);
        setDraggedWidth(undefined);

        if (over && active.id !== over.id && onReorder) {
            const activeIdStr = active.id as string;
            const overIdStr = over.id as string;
            const oldIndex = fullAccountIds.indexOf(activeIdStr);
            const newIndex = fullAccountIds.indexOf(overIdStr);
            
            if (oldIndex !== -1 && newIndex !== -1) {
                onReorder(arrayMove(fullAccountIds, oldIndex, newIndex));
            }
        }
    };

    if (accounts.length === 0) {
        return (
            <div className="flex flex-col items-center justify-center py-16 bg-zinc-50 dark:bg-zinc-900/50 border border-zinc-200 dark:border-zinc-800 rounded-lg">
                <p className="text-zinc-500 mb-1">{t('accounts.empty.title')}</p>
                <p className="text-sm text-zinc-400">{t('accounts.empty.desc')}</p>
            </div>
        );
    }

    return (
        <DndContext 
            sensors={sensors} 
            collisionDetection={closestCenter} 
            onDragStart={handleDragStart} 
            onDragEnd={handleDragEnd}
            modifiers={[restrictToVerticalAxis]}
        >
            <div className="w-full overflow-hidden">
                {/* Header Row */}
                <div className="flex items-center h-8 px-1 mb-1 text-[10px] font-semibold text-zinc-500 dark:text-zinc-400 uppercase tracking-wide bg-zinc-50 dark:bg-zinc-800/50 border-b border-zinc-200 dark:border-zinc-700 sticky top-0 z-20">
                    <div style={{ width: COLUMN_WIDTHS.drag }} className="text-center shrink-0">#</div>
                    <div style={{ width: COLUMN_WIDTHS.checkbox }} className="flex justify-center shrink-0">
                        <div 
                            className={cn(
                                "w-3.5 h-3.5 rounded border flex items-center justify-center transition-all cursor-pointer",
                                accounts.length > 0 && selectedIds.size === accounts.length
                                    ? "bg-indigo-500 border-indigo-500" 
                                    : "border-zinc-300 dark:border-zinc-600 bg-transparent hover:border-zinc-400"
                            )}
                            onClick={onToggleAll}
                        >
                            {accounts.length > 0 && selectedIds.size === accounts.length && <div className="w-2 h-2 rounded-sm bg-white" />}
                        </div>
                    </div>
                    <div style={{ width: COLUMN_WIDTHS.email }} className="px-2 shrink-0">
                        {t('accounts.table.email')}
                    </div>
                    <div className="flex-1 px-2">
                        {t('accounts.table.quota')}
                    </div>
                    <div style={{ width: COLUMN_WIDTHS.actions }} className="text-center shrink-0">
                        {t('accounts.table.actions')}
                    </div>
                </div>

                <SortableContext items={accountIds} strategy={verticalListSortingStrategy}>
                    <div className="space-y-0.5">
                        {accounts.map((account, idx) => (
                            <SortableAccountRow
                                key={account.id}
                                account={account}
                                selected={selectedIds.has(account.id)}
                                isRefreshing={refreshingIds.has(account.id)}
                                isCurrent={account.id === currentAccountId}
                                isSwitching={account.id === switchingAccountId}
                                isSelectedForProxy={proxySelectedAccountIds?.has(account.id) || false}
                                index={idx}
                                totalCount={accounts.length}
                                onSelect={() => onToggleSelect(account.id)}
                                onSwitch={() => onSwitch(account.id)}
                                onRefresh={() => onRefresh(account.id)}
                                onViewDevice={() => onViewDevice(account.id)}
                                onViewDetails={() => onViewDetails(account.id)}
                                onExport={() => onExport(account.id)}
                                onDelete={() => onDelete(account.id)}
                                onToggleProxy={() => onToggleProxy(account.id)}
                                onWarmup={onWarmup ? () => onWarmup(account.id) : undefined}
                            />
                        ))}
                    </div>
                </SortableContext>
            </div>

            {/* Drag Overlay */}
            <DragOverlay dropAnimation={{ duration: 200, easing: 'ease-out' }}>
                {activeAccount ? (
                    <div 
                        style={{ width: draggedWidth }}
                        className="flex items-center h-[52px] px-1 rounded-md shadow-lg border border-indigo-400 dark:border-indigo-500 bg-white dark:bg-zinc-900"
                    >
                        <div style={{ width: COLUMN_WIDTHS.drag }} className="flex justify-center shrink-0">
                            <div className="p-1 text-indigo-500 cursor-grabbing">
                                <GripVertical className="w-4 h-4" />
                            </div>
                        </div>
                        <div style={{ width: COLUMN_WIDTHS.checkbox }} className="flex justify-center shrink-0">
                            <div className="w-4 h-4 rounded border border-zinc-300 dark:border-zinc-600 opacity-50" />
                        </div>
                        <div style={{ width: COLUMN_WIDTHS.email }} className="px-2 shrink-0">
                            <span className="text-xs font-medium text-indigo-600 dark:text-indigo-300 truncate block">
                                {activeAccount.email}
                            </span>
                        </div>
                        <div className="flex-1 opacity-30">
                            <div className="h-1 bg-zinc-200 dark:bg-zinc-700 rounded-full w-3/4" />
                        </div>
                    </div>
                ) : null}
            </DragOverlay>
        </DndContext>
    );
});
