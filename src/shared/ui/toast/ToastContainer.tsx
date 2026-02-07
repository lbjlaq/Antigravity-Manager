// File: src/shared/ui/toast/ToastContainer.tsx
// Toast container with global showToast function - bottom-right position

import { useState, useCallback, useEffect } from 'react';
import { createPortal } from 'react-dom';
import { AnimatePresence } from 'framer-motion';
import { Toast, ToastType } from './Toast';

export interface ToastItem {
    id: string;
    message: string;
    type: ToastType;
    duration?: number;
}

let toastCounter = 0;
let addToastExternal: ((message: string, type: ToastType, duration?: number) => void) | null = null;

export const showToast = (message: string, type: ToastType = 'info', duration: number = 3000) => {
    if (addToastExternal) {
        addToastExternal(message, type, duration);
    } else {
        console.warn('ToastContainer not mounted');
    }
};

export const ToastContainer = () => {
    const [toasts, setToasts] = useState<ToastItem[]>([]);

    const addToast = useCallback((message: string, type: ToastType, duration?: number) => {
        const id = `toast-${Date.now()}-${toastCounter++}`;
        setToasts(prev => [...prev, { id, message, type, duration }]);
    }, []);

    const removeToast = useCallback((id: string) => {
        setToasts(prev => prev.filter(t => t.id !== id));
    }, []);

    useEffect(() => {
        addToastExternal = addToast;
        return () => {
            addToastExternal = null;
        };
    }, [addToast]);

    return createPortal(
        <div className="fixed bottom-6 right-6 z-[200] flex flex-col-reverse gap-2 pointer-events-none">
            <AnimatePresence mode="popLayout">
                {toasts.map(toast => (
                    <div key={toast.id} className="pointer-events-auto">
                        <Toast
                            {...toast}
                            onClose={removeToast}
                        />
                    </div>
                ))}
            </AnimatePresence>
        </div>,
        document.body
    );
};

export default ToastContainer;
