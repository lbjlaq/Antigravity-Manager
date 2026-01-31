import { ArrowRightLeft, RefreshCw, Trash2, Download, Info, Lock, Ban, Diamond, Gem, Circle, Clock, ToggleLeft, ToggleRight, Fingerprint, Sparkles, CheckCircle2 } from 'lucide-react';
import { Account } from '../../types/account';
import { getQuotaColor, formatTimeRemaining, getTimeRemainingColor } from '../../utils/format';
import { cn } from '../../utils/cn';
import { useTranslation } from 'react-i18next';
import { memo } from 'react';

interface AccountCardProps {
    account: Account;
    selected: boolean;
    onSelect: () => void;
    isCurrent: boolean;
    isRefreshing: boolean;
    isSwitching?: boolean;
    isSelectedForProxy?: boolean;
    onSwitch: () => void;
    onRefresh: () => void;
    onViewDevice: () => void;
    onViewDetails: () => void;
    onExport: () => void;
    onDelete: () => void;
    onToggleProxy: () => void;
    onWarmup?: () => void;
}

const AccountCard = memo(function AccountCard({ 
    account, 
    selected, 
    onSelect, 
    isCurrent, 
    isRefreshing, 
    isSwitching = false,
    isSelectedForProxy = false,
    onSwitch, 
    onRefresh, 
    onViewDetails, 
    onExport, 
    onDelete, 
    onToggleProxy, 
    onViewDevice, 
    onWarmup 
}: AccountCardProps) {
    const { t } = useTranslation();
    
    // Model Data Extraction
    const geminiProModel = account.quota?.models.find(m => m.name === 'gemini-3-pro-high');
    const geminiFlashModel = account.quota?.models.find(m => m.name === 'gemini-3-flash');
    const claudeModel = account.quota?.models.find(m => m.name === 'claude-sonnet-4-5-thinking');
    
    const isDisabled = Boolean(account.disabled);

    // Dynamic Logic
    const getColorClass = (percentage: number) => {
        const color = getQuotaColor(percentage);
        switch (color) {
            case 'success': return 'bg-emerald-500';
            case 'warning': return 'bg-amber-500';
            case 'error': return 'bg-rose-500';
            default: return 'bg-zinc-500';
        }
    };

    const getTimeColorClass = (resetTime: string | undefined) => {
        const color = getTimeRemainingColor(resetTime);
        switch (color) {
            case 'success': return 'text-emerald-400';
            case 'warning': return 'text-amber-400';
            default: return 'text-zinc-500';
        }
    };

    return (
        <div 
            onClick={onSelect}
            className={cn(
                "group relative flex flex-col p-4 rounded-2xl transition-all duration-300 cursor-pointer overflow-hidden backdrop-blur-xl",
                // Base Styles
                "bg-zinc-900/40 border border-white/5",
                // Hover Styles
                "hover:border-white/10 hover:shadow-2xl hover:bg-zinc-900/60 hover:-translate-y-1",
                // Current Account Highlight
                isCurrent && "border-indigo-500/30 bg-indigo-500/5 shadow-indigo-500/10",
                // Selection Highlight
                selected && "ring-1 ring-indigo-500/50 border-indigo-500/50 bg-indigo-500/10",
                // Disabled/Loading States
                (isRefreshing || isDisabled) && "opacity-70 grayscale-[0.5]"
            )}
        >
            {/* Background Glow Effect */}
            <div className={cn(
                "absolute inset-0 bg-gradient-to-br from-indigo-500/5 via-transparent to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500 pointer-events-none",
                isCurrent && "opacity-100"
            )} />

            {/* Header Section */}
            <div className="relative z-10 flex items-start gap-4 mb-4">
                {/* Checkbox (Custom styled) */}
                <div 
                    className={cn(
                        "mt-1 w-4 h-4 rounded border flex items-center justify-center transition-all",
                        selected 
                            ? "bg-indigo-500 border-indigo-500" 
                            : "border-zinc-600 group-hover:border-zinc-500 bg-transparent"
                    )}
                    onClick={(e) => { e.stopPropagation(); onSelect(); }}
                >
                    {selected && <div className="w-2 h-2 rounded-[1px] bg-white" />}
                </div>

                <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 mb-1">
                        <h3 className={cn(
                            "font-bold text-sm truncate tracking-wide font-mono",
                            isCurrent ? "text-indigo-300" : "text-zinc-200"
                        )} title={account.email}>
                            {account.email}
                        </h3>
                        
                        {/* Status Icons */}
                        {isCurrent && <span className="w-1.5 h-1.5 rounded-full bg-indigo-500 shadow-[0_0_8px_rgba(99,102,241,0.6)]" />}
                        {isDisabled && <Ban className="w-3.5 h-3.5 text-rose-500" />}
                        {account.proxy_disabled && <ToggleLeft className="w-3.5 h-3.5 text-orange-500" />}
                    </div>

                    {/* Badges Row */}
                    <div className="flex items-center gap-1.5 flex-wrap">
                        {/* Selected for Proxy Badge */}
                        {isSelectedForProxy && (
                            <span className="flex items-center gap-1 px-1.5 py-0.5 rounded bg-gradient-to-r from-green-500/20 to-emerald-500/20 border border-green-500/30 text-[9px] font-bold text-green-300 uppercase tracking-wider">
                                <CheckCircle2 className="w-2.5 h-2.5" /> SELECTED
                            </span>
                        )}

                        {/* Subscription Tier */}
                        {(() => {
                            const tier = (account.quota?.subscription_tier || '').toLowerCase();
                            if (tier.includes('ultra')) {
                                return (
                                    <span className="flex items-center gap-1 px-1.5 py-0.5 rounded bg-gradient-to-r from-purple-500/20 to-pink-500/20 border border-purple-500/30 text-[9px] font-bold text-purple-300 uppercase tracking-wider">
                                        <Gem className="w-2.5 h-2.5" /> ULTRA
                                    </span>
                                );
                            } else if (tier.includes('pro')) {
                                return (
                                    <span className="flex items-center gap-1 px-1.5 py-0.5 rounded bg-blue-500/10 border border-blue-500/20 text-[9px] font-bold text-blue-300 uppercase tracking-wider">
                                        <Diamond className="w-2.5 h-2.5" /> PRO
                                    </span>
                                );
                            }
                            return (
                                <span className="flex items-center gap-1 px-1.5 py-0.5 rounded bg-zinc-800/50 border border-zinc-600/50 text-[9px] font-bold text-zinc-400 uppercase tracking-wider">
                                    <Circle className="w-2.5 h-2.5" /> FREE
                                </span>
                            );
                        })()}

                        {/* Forbidden Badge */}
                        {account.quota?.is_forbidden && (
                            <span className="px-1.5 py-0.5 rounded bg-rose-500/10 border border-rose-500/20 text-[9px] font-bold text-rose-400 uppercase tracking-wider flex items-center gap-1">
                                <Lock className="w-2.5 h-2.5" /> BANNED
                            </span>
                        )}

                        {/* Validation Blocked Badge (VALIDATION_REQUIRED 403) */}
                        {account.validation_blocked && account.validation_blocked_until && (
                            <span 
                                className="px-1.5 py-0.5 rounded bg-yellow-500/10 border border-yellow-500/20 text-[9px] font-bold text-yellow-400 uppercase tracking-wider flex items-center gap-1 animate-pulse"
                                title={account.validation_blocked_reason || 'Verification required'}
                            >
                                <Clock className="w-2.5 h-2.5" /> {formatTimeRemaining(new Date(account.validation_blocked_until * 1000).toISOString())}
                            </span>
                        )}
                    </div>
                </div>
            </div>

            {/* Quota Bars Section */}
            <div className="relative z-10 flex-1 space-y-3 mb-4">
                {[
                    { model: geminiProModel, label: 'G3 PRO', color: 'indigo' },
                    { model: geminiFlashModel, label: 'G3 FLASH', color: 'cyan' },
                    // { model: geminiImageModel, label: 'IMAGE', color: 'pink' }, // Hidden to save space? Or show if present?
                    { model: claudeModel, label: 'CLAUDE', color: 'orange' }
                ].map(({ model, label }) => {
                    if (!model) return null;
                    return (
                        <div key={label} className="group/bar">
                            <div className="flex items-end justify-between mb-1 text-[10px] font-bold tracking-wider text-zinc-500">
                                <span>{label}</span>
                                <div className="flex items-center gap-2">
                                     {model.reset_time && (
                                        <span className={cn("flex items-center gap-1 font-mono transition-colors", getTimeColorClass(model.reset_time))}>
                                            <Clock className="w-2.5 h-2.5" />
                                            {formatTimeRemaining(model.reset_time)}
                                        </span>
                                    )}
                                    <span className={cn(
                                        "font-mono transition-colors",
                                        getQuotaColor(model.percentage) === 'success' ? 'text-emerald-400' : 
                                        getQuotaColor(model.percentage) === 'warning' ? 'text-amber-400' : 'text-rose-400'
                                    )}>{model.percentage}%</span>
                                </div>
                            </div>
                            <div className="h-1.5 w-full bg-zinc-800 rounded-full overflow-hidden border border-white/5">
                                <div 
                                    className={cn("h-full transition-all duration-700 ease-out rounded-full", getColorClass(model.percentage))}
                                    style={{ width: `${model.percentage}%` }}
                                />
                            </div>
                        </div>
                    );
                })}
                 {/* Special handling for Image model if needed, or just include in map above */}
            </div>

            {/* Footer Actions */}
            <div className="relative z-10 mt-auto pt-3 border-t border-white/5 flex items-center justify-between">
                <span className="text-[10px] text-zinc-600 font-mono group-hover:text-zinc-500 transition-colors">
                     {new Date(account.last_used * 1000).toLocaleDateString(undefined, {month: '2-digit', day: '2-digit'})} 
                     <span className="ml-1 opacity-50">{new Date(account.last_used * 1000).toLocaleTimeString(undefined, {hour: '2-digit', minute:'2-digit'})}</span>
                </span>

                <div className="flex items-center gap-1 opacity-40 group-hover:opacity-100 transition-opacity duration-300">
                    <ActionButton 
                        icon={Info} 
                        onClick={onViewDetails} 
                        tooltip={t('common.details')}
                        variant="default"
                    />
                     <ActionButton 
                        icon={Fingerprint} 
                        onClick={onViewDevice} 
                        tooltip={t('accounts.device_fingerprint')} 
                        variant="default"
                    />
                    <ActionButton 
                        icon={ArrowRightLeft} 
                        onClick={onSwitch}
                        tooltip={isSwitching ? t('common.loading') : t('common.switch')}
                        loading={isSwitching}
                        disabled={isSwitching || isDisabled}
                        variant="primary"
                    />
                    {onWarmup && (
                        <ActionButton 
                            icon={Sparkles}
                            onClick={onWarmup}
                            tooltip={isRefreshing ? t('common.loading') : t('accounts.warmup_this')}
                            loading={isRefreshing}
                            disabled={isRefreshing || isDisabled}
                            variant="warning"
                        />
                    )}
                    <ActionButton 
                        icon={RefreshCw}
                        onClick={onRefresh}
                        tooltip={t('common.refresh')}
                        loading={isRefreshing}
                        disabled={isRefreshing || isDisabled}
                        variant="success"
                    />
                    <ActionButton 
                        icon={Download} 
                        onClick={onExport}
                        tooltip={t('common.export')}
                        variant="default"
                    />
                     <ActionButton 
                        icon={account.proxy_disabled ? ToggleRight : ToggleLeft}
                        onClick={onToggleProxy}
                        tooltip={account.proxy_disabled ? t('accounts.enable_proxy') : t('accounts.disable_proxy')}
                        variant={account.proxy_disabled ? "default" : "warning"}
                    />
                    <ActionButton 
                        icon={Trash2} 
                        onClick={onDelete}
                        tooltip={t('common.delete')}
                        variant="danger"
                    />
                </div>
            </div>
        </div>
    );
});

// Helper component for uniform buttons
function ActionButton({ icon: Icon, onClick, tooltip, loading, disabled, variant = 'default' }: any) {
    const variants: Record<string, string> = {
        default: "hover:bg-zinc-800 text-zinc-400 hover:text-white",
        primary: "hover:bg-indigo-500/20 text-indigo-400 hover:text-indigo-300",
        success: "hover:bg-emerald-500/20 text-emerald-400 hover:text-emerald-300",
        warning: "hover:bg-amber-500/20 text-amber-400 hover:text-amber-300", 
        danger: "hover:bg-rose-500/20 text-rose-400 hover:text-rose-300"
    };

    return (
        <button
            onClick={(e) => { e.stopPropagation(); onClick && onClick(e); }}
            disabled={disabled}
            className={cn(
                "p-1.5 rounded-lg transition-all duration-200",
                variants[variant],
                disabled && "opacity-50 cursor-not-allowed"
            )}
            title={tooltip}
        >
            <Icon className={cn("w-3.5 h-3.5", loading && "animate-spin")} />
        </button>
    );
}

export default AccountCard;
