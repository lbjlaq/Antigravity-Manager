// File: src/features/accounts/ui/AccountDetailsDialog.tsx
// Account quota details dialog - redesigned to match project style

import { memo } from 'react';
import { createPortal } from 'react-dom';
import { motion, AnimatePresence } from 'framer-motion';
import { X, Clock, AlertCircle, Info, Shield } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Account, ModelQuota } from '@/entities/account';
import { formatDate, cn } from '@/shared/lib';

interface AccountDetailsDialogProps {
    account: Account | null;
    onClose: () => void;
}

export const AccountDetailsDialog = memo(function AccountDetailsDialog({ account, onClose }: AccountDetailsDialogProps) {
    const { t } = useTranslation();
    
    if (!account) return null;

    const getTierBadge = () => {
        const tier = (account.quota?.subscription_tier || '').toLowerCase();
        if (tier.includes('ultra')) {
            return <span className="px-2 py-0.5 rounded-md bg-purple-100 dark:bg-purple-500/20 text-purple-700 dark:text-purple-400 text-[10px] font-bold uppercase">ULTRA</span>;
        }
        if (tier.includes('pro')) {
            return <span className="px-2 py-0.5 rounded-md bg-blue-100 dark:bg-blue-500/20 text-blue-700 dark:text-blue-400 text-[10px] font-bold uppercase">PRO</span>;
        }
        return <span className="px-2 py-0.5 rounded-md bg-zinc-100 dark:bg-zinc-800 text-zinc-500 text-[10px] font-bold uppercase">FREE</span>;
    };

    const getQuotaColor = (percentage: number) => {
        if (percentage >= 50) return 'success';
        if (percentage >= 20) return 'warning';
        return 'error';
    };

    return createPortal(
        <AnimatePresence>
            <div className="fixed inset-0 z-[100] flex items-center justify-center p-4">
                {/* Backdrop */}
                <motion.div 
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0 }}
                    className="absolute inset-0 bg-black/50 backdrop-blur-sm"
                    onClick={onClose}
                />

                {/* Draggable region for Tauri */}
                <div data-tauri-drag-region className="fixed top-0 left-0 right-0 h-8 z-[110]" />

                {/* Modal */}
                <motion.div
                    initial={{ opacity: 0, scale: 0.95, y: 10 }}
                    animate={{ opacity: 1, scale: 1, y: 0 }}
                    exit={{ opacity: 0, scale: 0.95, y: 10 }}
                    transition={{ duration: 0.2, ease: 'easeOut' }}
                    className="relative w-full max-w-3xl bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 shadow-2xl overflow-hidden flex flex-col max-h-[85vh]"
                >
                    {/* Header */}
                    <div className="flex items-center justify-between px-6 py-4 border-b border-zinc-200 dark:border-zinc-800 bg-zinc-50 dark:bg-zinc-900/50">
                        <div className="flex items-center gap-3">
                            <div className="p-2 rounded-lg bg-indigo-500/10">
                                <Info className="w-5 h-5 text-indigo-500" />
                            </div>
                            <div>
                                <h3 className="font-bold text-zinc-900 dark:text-white text-lg">
                                    {t('accounts.details.title')}
                                </h3>
                                <p className="text-xs text-zinc-500 font-mono">{account.email}</p>
                            </div>
                            {getTierBadge()}
                        </div>
                        <button 
                            onClick={onClose}
                            className="p-2 rounded-lg text-zinc-400 hover:text-zinc-600 dark:hover:text-white hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors"
                        >
                            <X className="w-5 h-5" />
                        </button>
                    </div>

                    {/* Status Alerts */}
                    {(account.disabled || account.proxy_disabled) && (
                        <div className="px-6 py-3 bg-rose-50 dark:bg-rose-950/20 border-b border-rose-100 dark:border-rose-900/30 space-y-1.5">
                            {account.disabled && (
                                <div className="flex items-center gap-2 text-xs text-rose-700 dark:text-rose-400">
                                    <AlertCircle className="w-3.5 h-3.5" />
                                    <span className="font-semibold">{t('accounts.disabled')}:</span>
                                    <span>{account.disabled_reason || t('common.unknown')}</span>
                                </div>
                            )}
                            {account.proxy_disabled && (
                                <div className="flex items-center gap-2 text-xs text-orange-700 dark:text-orange-400">
                                    <AlertCircle className="w-3.5 h-3.5" />
                                    <span className="font-semibold">{t('accounts.proxy_disabled')}:</span>
                                    <span>{account.proxy_disabled_reason || t('common.unknown')}</span>
                                </div>
                            )}
                        </div>
                    )}

                    {/* Content */}
                    <div className="flex-1 overflow-y-auto p-6 scrollbar-thin scrollbar-thumb-zinc-300 dark:scrollbar-thumb-zinc-700 scrollbar-track-transparent">
                        {/* Protected Models Section */}
                        {account.protected_models && account.protected_models.length > 0 && (
                            <div className="mb-6">
                                <h4 className="text-xs font-bold text-zinc-500 dark:text-zinc-400 uppercase tracking-wider mb-3 flex items-center gap-2">
                                    <Shield className="w-3.5 h-3.5 text-amber-500" />
                                    {t('accounts.details.protected_models', 'Protected Models')}
                                </h4>
                                <div className="flex flex-wrap gap-2">
                                    {account.protected_models.map(model => (
                                        <span 
                                            key={model} 
                                            className="px-2.5 py-1 bg-amber-50 dark:bg-amber-900/20 text-amber-700 dark:text-amber-400 text-[11px] font-mono border border-amber-200 dark:border-amber-800/40 rounded-lg"
                                        >
                                            {model}
                                        </span>
                                    ))}
                                </div>
                            </div>
                        )}

                        {/* Model Quota Section */}
                        <h4 className="text-xs font-bold text-zinc-500 dark:text-zinc-400 uppercase tracking-wider mb-4">
                            {t('accounts.details.model_quota', 'Model Quota')}
                        </h4>
                        
                        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                            {account.quota?.models?.map((model: ModelQuota) => {
                                const color = getQuotaColor(model.percentage);
                                return (
                                    <div 
                                        key={model.name} 
                                        className={cn(
                                            "p-4 rounded-xl border transition-all group",
                                            "bg-white dark:bg-zinc-900",
                                            "border-zinc-200 dark:border-zinc-800",
                                            "hover:border-zinc-300 dark:hover:border-zinc-700",
                                            "hover:shadow-sm"
                                        )}
                                    >
                                        {/* Model name + percentage */}
                                        <div className="flex justify-between items-start mb-3">
                                            <span className="text-sm font-medium text-zinc-700 dark:text-zinc-300 group-hover:text-indigo-600 dark:group-hover:text-indigo-400 transition-colors">
                                                {model.name}
                                            </span>
                                            <span className={cn(
                                                "text-xs font-bold px-2 py-0.5 rounded-md",
                                                color === 'success' && "bg-emerald-50 dark:bg-emerald-900/30 text-emerald-700 dark:text-emerald-400",
                                                color === 'warning' && "bg-amber-50 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400",
                                                color === 'error' && "bg-rose-50 dark:bg-rose-900/30 text-rose-700 dark:text-rose-400"
                                            )}>
                                                {model.percentage}%
                                            </span>
                                        </div>

                                        {/* Progress Bar */}
                                        <div className="h-1.5 w-full bg-zinc-100 dark:bg-zinc-800 rounded-full overflow-hidden mb-3">
                                            <motion.div
                                                initial={{ width: 0 }}
                                                animate={{ width: `${model.percentage}%` }}
                                                transition={{ duration: 0.5, ease: 'easeOut' }}
                                                className={cn(
                                                    "h-full rounded-full",
                                                    color === 'success' && "bg-emerald-500",
                                                    color === 'warning' && "bg-amber-500",
                                                    color === 'error' && "bg-rose-500"
                                                )}
                                            />
                                        </div>

                                        {/* Reset time */}
                                        <div className="flex items-center gap-1.5 text-[10px] text-zinc-400 dark:text-zinc-500 font-mono">
                                            <Clock className="w-3 h-3" />
                                            <span>{t('accounts.reset_time')}: {formatDate(model.reset_time) || t('common.unknown')}</span>
                                        </div>
                                    </div>
                                );
                            }) || (
                                <div className="col-span-2 py-12 text-center text-zinc-400 flex flex-col items-center">
                                    <AlertCircle className="w-10 h-10 mb-3 opacity-30" />
                                    <span>{t('accounts.no_data')}</span>
                                </div>
                            )}
                        </div>
                    </div>
                </motion.div>
            </div>
        </AnimatePresence>,
        document.body
    );
});

export default AccountDetailsDialog;
