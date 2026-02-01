// File: src/shared/ui/toast/Toast.tsx
// Toast notification component - redesigned with modern style

import { useState, useEffect, useCallback } from 'react';
import { motion } from 'framer-motion';
import { CheckCircle, XCircle, Info, AlertTriangle, X } from 'lucide-react';
import { cn } from '@/shared/lib';

export type ToastType = 'success' | 'error' | 'info' | 'warning';

export interface ToastProps {
    id: string;
    message: string;
    type: ToastType;
    duration?: number;
    onClose: (id: string) => void;
}

const TOAST_CONFIG = {
    success: {
        icon: CheckCircle,
        barColor: 'bg-emerald-500',
        iconBg: 'bg-emerald-100 dark:bg-emerald-900/30',
        iconColor: 'text-emerald-600 dark:text-emerald-400',
        progressColor: 'bg-emerald-500',
    },
    error: {
        icon: XCircle,
        barColor: 'bg-rose-500',
        iconBg: 'bg-rose-100 dark:bg-rose-900/30',
        iconColor: 'text-rose-600 dark:text-rose-400',
        progressColor: 'bg-rose-500',
    },
    warning: {
        icon: AlertTriangle,
        barColor: 'bg-amber-500',
        iconBg: 'bg-amber-100 dark:bg-amber-900/30',
        iconColor: 'text-amber-600 dark:text-amber-400',
        progressColor: 'bg-amber-500',
    },
    info: {
        icon: Info,
        barColor: 'bg-indigo-500',
        iconBg: 'bg-indigo-100 dark:bg-indigo-900/30',
        iconColor: 'text-indigo-600 dark:text-indigo-400',
        progressColor: 'bg-indigo-500',
    },
};

export const Toast = ({ id, message, type, duration = 3000, onClose }: ToastProps) => {
    const [isPaused, setIsPaused] = useState(false);
    const [progress, setProgress] = useState(100);
    const [startTime, setStartTime] = useState(Date.now());
    const [remainingTime, setRemainingTime] = useState(duration);

    const config = TOAST_CONFIG[type];
    const Icon = config.icon;

    const handleClose = useCallback(() => {
        onClose(id);
    }, [id, onClose]);

    // Progress bar logic with pause support
    useEffect(() => {
        if (duration <= 0) return;

        if (isPaused) {
            // Save remaining time when paused
            const elapsed = Date.now() - startTime;
            setRemainingTime(prev => Math.max(0, prev - elapsed));
            return;
        }

        // Reset start time when resuming
        setStartTime(Date.now());

        const interval = setInterval(() => {
            const elapsed = Date.now() - startTime;
            const newProgress = Math.max(0, ((remainingTime - elapsed) / duration) * 100);
            setProgress(newProgress);

            if (newProgress <= 0) {
                clearInterval(interval);
                handleClose();
            }
        }, 16); // ~60fps

        return () => clearInterval(interval);
    }, [duration, isPaused, startTime, remainingTime, handleClose]);

    return (
        <motion.div
            layout
            initial={{ opacity: 0, x: 50, scale: 0.95 }}
            animate={{ opacity: 1, x: 0, scale: 1 }}
            exit={{ opacity: 0, x: 50, scale: 0.95 }}
            transition={{ duration: 0.2, ease: 'easeOut' }}
            onMouseEnter={() => setIsPaused(true)}
            onMouseLeave={() => {
                setIsPaused(false);
                setStartTime(Date.now());
            }}
            className={cn(
                "relative flex overflow-hidden rounded-lg shadow-lg border",
                "bg-white dark:bg-zinc-900",
                "border-zinc-200 dark:border-zinc-800",
                "min-w-[280px] max-w-[380px]"
            )}
        >
            {/* Left color bar */}
            <div className={cn("w-1 shrink-0", config.barColor)} />

            {/* Content */}
            <div className="flex-1 flex items-center gap-3 px-3 py-3">
                {/* Icon with background */}
                <div className={cn("p-1.5 rounded-full shrink-0", config.iconBg)}>
                    <Icon className={cn("w-4 h-4", config.iconColor)} />
                </div>

                {/* Message */}
                <p className="flex-1 text-sm font-medium text-zinc-700 dark:text-zinc-200 leading-tight">
                    {message}
                </p>

                {/* Close button */}
                <button
                    onClick={handleClose}
                    className="p-1 rounded-md text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors shrink-0"
                >
                    <X className="w-3.5 h-3.5" />
                </button>
            </div>

            {/* Progress bar */}
            {duration > 0 && (
                <div className="absolute bottom-0 left-0 right-0 h-0.5 bg-zinc-100 dark:bg-zinc-800">
                    <motion.div
                        className={cn("h-full", config.progressColor)}
                        style={{ width: `${progress}%` }}
                        transition={{ duration: 0 }}
                    />
                </div>
            )}
        </motion.div>
    );
};

export default Toast;
