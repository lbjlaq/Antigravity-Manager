import { useMemo, useState, memo } from 'react';
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
} from 'lucide-react';
import { Account } from '../../types/account';
import { useTranslation } from 'react-i18next';
import { cn } from '../../utils/cn';
import { getQuotaColor, formatTimeRemaining, getTimeRemainingColor } from '../../utils/format';
import { useConfigStore } from '../../stores/useConfigStore';

// ===================================
// Helper - Model Label
// ===================================
function getModelLabel(id: string): string {
    if (id.includes('gemini-3-pro')) return 'G3 PRO';
    if (id.includes('gemini-3-flash')) return 'G3 FLASH';
    if (id.includes('claude-sonnet')) return 'CLAUDE';
    if (id.includes('gpt-4o')) return 'GPT-4o';
    if (id.includes('o1-mini')) return 'O1 MINI';
    if (id.includes('o1-preview')) return 'O1 PRE';
    if (id.includes('pro-image')) return 'IMG';
    return id.split('-').slice(1).join(' ').toUpperCase().substring(0, 8);
}

// ===================================
// Types
// ===================================
interface AccountTableProps {
    accounts: Account[];
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
// Helper - Quota Colors
// ===================================
function getColorClass(percentage: number): string {
    const color = getQuotaColor(percentage);
    switch (color) {
        case 'success': return 'bg-emerald-500';
        case 'warning': return 'bg-amber-500';
        case 'error': return 'bg-rose-500';
        default: return 'bg-zinc-500';
    }
}
function getTimeColorClass(resetTime: string | undefined): string {
    const color = getTimeRemainingColor(resetTime);
    switch (color) {
        case 'success': return 'text-emerald-400';
        case 'warning': return 'text-amber-400';
        default: return 'text-zinc-500';
    }
}

// ===================================
// Row Component
// ===================================
function SortableAccountRow({
    account, selected, isRefreshing, isCurrent, isSwitching, isSelectedForProxy,
    onSelect, onSwitch, onRefresh, onViewDevice, onViewDetails, onExport, onDelete, onToggleProxy, onWarmup
}: SortableRowProps) {
    const { t } = useTranslation();
    const { attributes, listeners, setNodeRef, transform, transition, isDragging: isSortableDragging } = useSortable({ id: account.id });

    const style = {
        transform: CSS.Transform.toString(transform),
        transition,
        opacity: isSortableDragging ? 0.4 : 1,
        zIndex: isSortableDragging ? 50 : undefined,
    };

    return (
        <div ref={setNodeRef} style={style} id={account.id} className={cn(
            "group relative grid grid-cols-[28px_28px_320px_1fr_100px_auto] gap-2 items-center px-2 py-1 mb-[1px] rounded-md transition-all duration-200",
            "border border-white/5 bg-zinc-900/40 backdrop-blur-sm",
            "hover:border-white/10 hover:bg-zinc-900/60 hover:translate-x-0.5",
            isCurrent && "border-indigo-500/30 bg-indigo-500/5 hover:bg-indigo-500/10",
            selected && !isCurrent && "border-indigo-500/30 bg-indigo-500/10",
        )}>
            {/* Drag Handle */}
            <div className="flex justify-center">
                <div {...attributes} {...listeners} className="p-1 cursor-grab active:cursor-grabbing text-zinc-600 hover:text-zinc-300 transition-colors">
                    <GripVertical className="w-5 h-5" />
                </div>
            </div>

            {/* Checkbox */}
            <div className="flex justify-center">
                 <div 
                    className={cn(
                        "w-5 h-5 rounded border flex items-center justify-center transition-all cursor-pointer",
                        selected 
                            ? "bg-indigo-500 border-indigo-500" 
                            : "border-zinc-600 group-hover:border-zinc-500 bg-transparent"
                    )}
                    onClick={(e) => { e.stopPropagation(); onSelect(); }}
                >
                    {selected && <div className="w-2.5 h-2.5 rounded-[1px] bg-white" />}
                </div>
            </div>

            {/* Email & Account Info */}
            <div className="flex flex-col min-w-0 pr-4">
                <div className="flex items-center gap-2 mb-1">
                    {/* Selected for Proxy */}
                    {isSelectedForProxy && <span className="px-1.5 py-0.5 rounded bg-green-500/20 text-green-300 text-[9px] font-bold border border-green-500/20">SELECTED</span>}

                    {/* Subscription Tier */}
                    {(() => {
                         const tier = (account.quota?.subscription_tier || '').toLowerCase();
                         if (tier.includes('ultra')) return <span className="px-1.5 py-0.5 rounded bg-purple-500/20 text-purple-300 text-[9px] font-bold border border-purple-500/20">ULTRA</span>;
                         if (tier.includes('pro')) return <span className="px-1.5 py-0.5 rounded bg-blue-500/20 text-blue-300 text-[9px] font-bold border border-blue-500/20">PRO</span>;
                         return <span className="px-1.5 py-0.5 rounded bg-zinc-700/50 text-zinc-400 text-[9px] font-bold border border-zinc-600/50">FREE</span>;
                    })()}

                    <span className={cn(
                        "font-bold text-sm tracking-wide font-mono break-all",
                        isCurrent ? "text-indigo-300" : "text-zinc-200"
                    )} title={account.email}>{account.email}</span>
                </div>
                <div className="flex items-center gap-1.5 flex-wrap">
                    {/* Tags */}
                    {isCurrent && <span className="px-1.5 py-0.5 rounded bg-indigo-500/20 text-indigo-300 text-[9px] font-bold border border-indigo-500/20">CURRENT</span>}
                    {account.disabled && <span className="px-1.5 py-0.5 rounded bg-rose-500/20 text-rose-300 text-[9px] font-bold border border-rose-500/20">DISABLED</span>}
                    {account.proxy_disabled && <span className="px-1.5 py-0.5 rounded bg-orange-500/20 text-orange-300 text-[9px] font-bold border border-orange-500/20">NO PROXY</span>}
                </div>
            </div>

            {/* Quota Bars */}
            <div className="grid grid-cols-2 gap-x-4 gap-y-2">
                 {(() => {
                    const config = useConfigStore(s => s.config);
                    const pinned = config?.pinned_quota_models?.models || [];
                    const modelsToShow = pinned.length > 0 ? pinned : [
                        'gemini-3-pro-high', 
                        'gemini-3-flash', 
                        'claude-sonnet-4-5-thinking'
                    ];

                    return modelsToShow.map(modelId => {
                        const m = account.quota?.models.find(m => m.name === modelId);
                        if (!m) return null;
                        
                        return (
                            <div key={modelId} className="flex items-center gap-2 text-[10px]" title={m.reset_time ? `${t('common.reset')}: ${new Date(m.reset_time).toLocaleString()}` : undefined}>
                                <span className="w-12 font-bold text-zinc-500 shrink-0 text-right">{getModelLabel(modelId)}</span>
                                <div className="flex-1 h-1.5 bg-zinc-800 rounded-full overflow-hidden border border-white/5 relative group/bar">
                                    <div className={cn("h-full rounded-full transition-all duration-500", getColorClass(m.percentage))} style={{ width: `${m.percentage}%` }} />
                                </div>
                                <div className="flex flex-col items-end w-20 leading-none">
                                     <span className={cn("font-mono font-bold", getTimeColorClass(m.reset_time))}>{m.percentage}%</span>
                                     {m.reset_time && (
                                         <span className="text-[10px] text-zinc-400 font-mono mt-0.5 whitespace-nowrap">
                                             {formatTimeRemaining(m.reset_time)}
                                         </span>
                                     )}
                                </div>
                            </div>
                        );
                    });
                 })()}
            </div>

            {/* Last Used */}
            <div className="flex flex-col text-right">
                <span className="text-xs font-mono text-zinc-300">
                    {new Date(account.last_used * 1000).toLocaleDateString()}
                </span>
                <span className="text-[10px] font-mono text-zinc-500">
                    {new Date(account.last_used * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                </span>
            </div>

            {/* Actions */}
            <div className="flex items-center justify-end gap-0.5 opacity-60 group-hover:opacity-100 transition-opacity">
                 <button onClick={onViewDetails} className="p-1 rounded text-zinc-400 hover:text-white hover:bg-zinc-800 transition-colors" title={t('common.details')}>
                    <Info className="w-3.5 h-3.5" />
                 </button>
                 <button onClick={onViewDevice} className="p-1 rounded text-zinc-400 hover:text-white hover:bg-zinc-800 transition-colors" title={t('accounts.device_fingerprint')}>
                    <Fingerprint className="w-3.5 h-3.5" />
                 </button>
                 <button 
                    onClick={onSwitch} 
                    disabled={isSwitching}
                    className="p-1 rounded text-zinc-400 hover:text-indigo-400 hover:bg-indigo-500/20 transition-colors disabled:opacity-50" 
                    title={t('common.switch')}
                >
                    <ArrowRightLeft className={cn("w-3.5 h-3.5", isSwitching && "animate-spin")} />
                 </button>
                 {onWarmup && (
                    <button 
                        onClick={onWarmup} 
                        disabled={isRefreshing}
                        className="p-1 rounded text-zinc-400 hover:text-amber-400 hover:bg-amber-500/20 transition-colors disabled:opacity-50"
                        title={t('accounts.warmup_this')}
                    >
                        <Sparkles className={cn("w-3.5 h-3.5", isRefreshing && "animate-pulse")} />
                    </button>
                 )}
                 <button 
                    onClick={onRefresh} 
                    disabled={isRefreshing}
                    className="p-1 rounded text-zinc-400 hover:text-emerald-400 hover:bg-emerald-500/20 transition-colors disabled:opacity-50"
                    title={t('common.refresh')}
                >
                    <RefreshCw className={cn("w-3.5 h-3.5", isRefreshing && "animate-spin")} />
                 </button>
                 <button 
                    onClick={onExport} 
                    className="p-1 rounded text-zinc-400 hover:text-blue-400 hover:bg-blue-500/20 transition-colors"
                    title={t('common.export')}
                >
                    <Download className="w-3.5 h-3.5" />
                 </button>
                 <button onClick={onToggleProxy} className="p-1 rounded text-zinc-400 hover:text-orange-400 hover:bg-orange-500/20 transition-colors" title={t('accounts.toggle_proxy')}>
                    {account.proxy_disabled ? <ToggleRight className="w-3.5 h-3.5" /> : <ToggleLeft className="w-3.5 h-3.5" />}
                 </button>
                 <button onClick={onDelete} className="p-1 rounded text-zinc-400 hover:text-rose-400 hover:bg-rose-500/20 transition-colors" title={t('common.delete')}>
                    <Trash2 className="w-3.5 h-3.5" />
                 </button>
            </div>
        </div>
    );
}

// ===================================
// Main Component
// ===================================
// Custom modifier to restrict drag to vertical axis
const restrictToVerticalAxis = ({ transform }: { transform: any }) => {
    return {
        ...transform,
        x: 0,
    };
};

const AccountTable = memo(function AccountTable({
    accounts, selectedIds, refreshingIds, proxySelectedAccountIds, onToggleSelect, onToggleAll,
    currentAccountId, switchingAccountId, onSwitch, onRefresh, onViewDevice,
    onViewDetails, onExport, onDelete, onToggleProxy, onReorder, onWarmup
}: AccountTableProps) {
    const { t } = useTranslation();
    const [activeId, setActiveId] = useState<string | null>(null);
    const [draggedWidth, setDraggedWidth] = useState<number | undefined>(undefined);

    const sensors = useSensors(
        useSensor(PointerSensor, { 
            activationConstraint: { 
                distance: 5 // Reduced from 8 to make it slightly more responsive but still allow clicks
            } 
        }),
        useSensor(KeyboardSensor, { 
            coordinateGetter: sortableKeyboardCoordinates 
        })
    );

    const accountIds = useMemo(() => accounts.map(a => a.id), [accounts]);
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
            const oldIndex = accountIds.indexOf(active.id as string);
            const newIndex = accountIds.indexOf(over.id as string);
            onReorder(arrayMove(accountIds, oldIndex, newIndex));
        }
    };

    if (accounts.length === 0) {
        return (
            <div className="flex flex-col items-center justify-center py-20 bg-zinc-900/40 backdrop-blur-xl border border-white/5 rounded-2xl">
                <p className="text-zinc-500 mb-2">{t('accounts.empty.title')}</p>
                <p className="text-sm text-zinc-600">{t('accounts.empty.desc')}</p>
            </div>
        );
    }

    return (
        <DndContext 
            sensors={sensors} 
            collisionDetection={closestCenter} 
            onDragStart={handleDragStart} 
            onDragEnd={handleDragEnd}
            modifiers={[restrictToVerticalAxis]} // Restrict horizontal movement
        >
            <div className="w-full">
                {/* Header Row */}
                <div className="grid grid-cols-[28px_28px_320px_1fr_100px_auto] gap-2 px-2 py-1.5 mb-1 text-[9px] font-bold text-zinc-500 uppercase tracking-widest bg-zinc-900/80 backdrop-blur border-b border-white/5 sticky top-0 z-20">
                    <div className="text-center">#</div>
                    <div className="flex justify-center">
                        <div 
                            className={cn(
                                "w-3.5 h-3.5 rounded border flex items-center justify-center transition-all cursor-pointer",
                                accounts.length > 0 && selectedIds.size === accounts.length
                                    ? "bg-indigo-500 border-indigo-500" 
                                    : "border-zinc-600 bg-transparent"
                            )}
                            onClick={onToggleAll}
                        >
                            {accounts.length > 0 && selectedIds.size === accounts.length && <div className="w-2 h-2 rounded-[0.5px] bg-white" />}
                        </div>
                    </div>
                    <div>{t('accounts.table.email')}</div>
                    <div>{t('accounts.table.quota')}</div>
                    <div className="text-right">{t('accounts.table.last_used')}</div>
                    <div className="text-right">{t('accounts.table.actions')}</div>
                </div>

                <SortableContext items={accountIds} strategy={verticalListSortingStrategy}>
                    <div className="space-y-0.5">
                        {accounts.map((account) => (
                            <SortableAccountRow
                                key={account.id}
                                account={account}
                                selected={selectedIds.has(account.id)}
                                isRefreshing={refreshingIds.has(account.id)}
                                isCurrent={account.id === currentAccountId}
                                isSwitching={account.id === switchingAccountId}
                                isSelectedForProxy={proxySelectedAccountIds?.has(account.id) || false}
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

            {/* Drag Overlay - Matching the Row Layout exactly */}
            <DragOverlay dropAnimation={{
                duration: 250,
                easing: 'cubic-bezier(0.18, 0.67, 0.6, 1.22)',
            }}>
                {activeAccount ? (
                     <div 
                        style={{ width: draggedWidth }}
                        className={cn(
                        "grid grid-cols-[28px_28px_320px_1fr_100px_auto] gap-2 items-center px-2 py-1 rounded-md shadow-2xl brightness-110",
                        "border border-indigo-500/50 bg-zinc-900/95 backdrop-blur-xl",
                    )}>
                        {/* Drag Handle */}
                        <div className="flex justify-center">
                            <div className="p-1 text-indigo-400 cursor-grabbing">
                                <GripVertical className="w-5 h-5" />
                            </div>
                        </div>

                        {/* Checkbox Placeholder */}
                        <div className="flex justify-center">
                             <div className="w-5 h-5 rounded border border-zinc-600 bg-transparent opacity-50" />
                        </div>

                        {/* Email & Info */}
                        <div className="flex flex-col min-w-0 pr-4">
                            <div className="flex items-center gap-2 mb-1">
                                <span className="font-bold text-sm tracking-wide font-mono break-all text-indigo-100">
                                    {activeAccount.email}
                                </span>
                            </div>
                        </div>

                        {/* Quota Placeholder (Simulated) */}
                        <div className="grid grid-cols-2 gap-x-4 gap-y-2 opacity-50">
                             <div className="col-span-2 h-1.5 bg-zinc-800 rounded-full w-full" />
                             <div className="col-span-2 h-1.5 bg-zinc-800 rounded-full w-3/4" />
                        </div>

                        {/* Date Placeholder */}
                        <div className="flex flex-col text-right opacity-50">
                            <span className="text-xs font-mono text-zinc-500">...</span>
                        </div>

                        {/* Actions Placeholder */}
                        <div className="w-10" />
                     </div>
                ) : null}
            </DragOverlay>
        </DndContext>
    );
});

export default AccountTable;
