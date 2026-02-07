// File: src/shared/ui/modal-dialog/ModalDialog.tsx
// Modal dialog component - redesigned with Framer Motion

import { useEffect, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { motion, AnimatePresence } from 'framer-motion';
import { AlertTriangle, CheckCircle, XCircle, Info } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { cn } from '@/shared/lib';

export type ModalType = 'confirm' | 'success' | 'error' | 'info';

interface ModalDialogProps {
    isOpen: boolean;
    title: string;
    message: string;
    type?: ModalType;
    onConfirm: () => void;
    onCancel?: () => void;
    confirmText?: string;
    cancelText?: string;
    isDestructive?: boolean;
}

const MODAL_CONFIG = {
    confirm: {
        icon: AlertTriangle,
        iconBg: 'bg-indigo-100 dark:bg-indigo-900/30',
        iconColor: 'text-indigo-600 dark:text-indigo-400',
        buttonBg: 'bg-indigo-600 hover:bg-indigo-500',
    },
    confirmDestructive: {
        icon: AlertTriangle,
        iconBg: 'bg-rose-100 dark:bg-rose-900/30',
        iconColor: 'text-rose-600 dark:text-rose-400',
        buttonBg: 'bg-rose-600 hover:bg-rose-500',
    },
    success: {
        icon: CheckCircle,
        iconBg: 'bg-emerald-100 dark:bg-emerald-900/30',
        iconColor: 'text-emerald-600 dark:text-emerald-400',
        buttonBg: 'bg-emerald-600 hover:bg-emerald-500',
    },
    error: {
        icon: XCircle,
        iconBg: 'bg-rose-100 dark:bg-rose-900/30',
        iconColor: 'text-rose-600 dark:text-rose-400',
        buttonBg: 'bg-rose-600 hover:bg-rose-500',
    },
    info: {
        icon: Info,
        iconBg: 'bg-indigo-100 dark:bg-indigo-900/30',
        iconColor: 'text-indigo-600 dark:text-indigo-400',
        buttonBg: 'bg-indigo-600 hover:bg-indigo-500',
    },
};

export function ModalDialog({
    isOpen,
    title,
    message,
    type = 'confirm',
    onConfirm,
    onCancel,
    confirmText,
    cancelText,
    isDestructive = false
}: ModalDialogProps) {
    const { t } = useTranslation();
    const finalConfirmText = confirmText || t('common.confirm');
    const finalCancelText = cancelText || t('common.cancel');

    // Get config based on type and destructive flag
    const configKey = type === 'confirm' && isDestructive ? 'confirmDestructive' : type;
    const config = MODAL_CONFIG[configKey];
    const Icon = config.icon;

    const showCancel = type === 'confirm' && onCancel;

    // Handle Escape key
    const handleKeyDown = useCallback((e: KeyboardEvent) => {
        if (e.key === 'Escape' && showCancel && onCancel) {
            onCancel();
        }
    }, [showCancel, onCancel]);

    useEffect(() => {
        if (isOpen) {
            document.addEventListener('keydown', handleKeyDown);
            return () => document.removeEventListener('keydown', handleKeyDown);
        }
    }, [isOpen, handleKeyDown]);

    return createPortal(
        <AnimatePresence>
            {isOpen && (
                <div className="fixed inset-0 z-[100] flex items-center justify-center p-4">
                    {/* Backdrop */}
                    <motion.div
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 1 }}
                        exit={{ opacity: 0 }}
                        transition={{ duration: 0.15 }}
                        className="absolute inset-0 bg-black/50"
                        onClick={showCancel ? onCancel : undefined}
                    />

                    {/* Tauri drag region */}
                    <div data-tauri-drag-region className="fixed top-0 left-0 right-0 h-8 z-[110]" />

                    {/* Modal */}
                    <motion.div
                        initial={{ opacity: 0, scale: 0.95, y: 10 }}
                        animate={{ opacity: 1, scale: 1, y: 0 }}
                        exit={{ opacity: 0, scale: 0.95, y: 10 }}
                        transition={{ duration: 0.15, ease: 'easeOut' }}
                        className="relative w-full max-w-sm bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 shadow-lg overflow-hidden"
                    >
                        <div className="flex flex-col items-center text-center p-6 pt-8">
                            {/* Icon */}
                            <div className={cn(
                                "w-12 h-12 rounded-full flex items-center justify-center mb-4",
                                config.iconBg
                            )}>
                                <Icon className={cn("w-6 h-6", config.iconColor)} />
                            </div>

                            {/* Title */}
                            <h3 className="text-lg font-bold text-zinc-900 dark:text-white mb-2">
                                {title}
                            </h3>

                            {/* Message */}
                            <p className="text-sm text-zinc-500 dark:text-zinc-400 leading-relaxed mb-6 px-2">
                                {message}
                            </p>

                            {/* Buttons */}
                            <div className="flex gap-3 w-full">
                                {showCancel && (
                                    <button
                                        onClick={onCancel}
                                        className={cn(
                                            "flex-1 px-4 py-2.5 text-sm font-medium rounded-lg transition-colors",
                                            "bg-zinc-100 dark:bg-zinc-800",
                                            "text-zinc-700 dark:text-zinc-300",
                                            "hover:bg-zinc-200 dark:hover:bg-zinc-700",
                                            "focus:outline-none focus:ring-2 focus:ring-zinc-300 dark:focus:ring-zinc-600"
                                        )}
                                    >
                                        {finalCancelText}
                                    </button>
                                )}
                                <button
                                    onClick={onConfirm}
                                    className={cn(
                                        "flex-1 px-4 py-2.5 text-sm font-medium text-white rounded-lg transition-colors",
                                        "focus:outline-none focus:ring-2 focus:ring-offset-2",
                                        config.buttonBg,
                                        isDestructive && type === 'confirm' 
                                            ? "focus:ring-rose-500" 
                                            : "focus:ring-indigo-500"
                                    )}
                                >
                                    {finalConfirmText}
                                </button>
                            </div>
                        </div>
                    </motion.div>
                </div>
            )}
        </AnimatePresence>,
        document.body
    );
}

export default ModalDialog;
