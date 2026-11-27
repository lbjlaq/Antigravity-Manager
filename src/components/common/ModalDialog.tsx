import { useEffect, useId, useRef } from 'react';
import { AlertTriangle, CheckCircle, XCircle, Info } from 'lucide-react';
import { useTranslation } from 'react-i18next';

export type ModalType = 'confirm' | 'success' | 'error' | 'info';

interface ModalDialogProps {
    isOpen: boolean;
    title: string;
    message?: string;
    children?: React.ReactNode;
    type?: ModalType;
    onConfirm: () => void;
    onCancel?: () => void;
    confirmText?: string;
    cancelText?: string;
    isDestructive?: boolean;
    isLoading?: boolean;
}

export default function ModalDialog({
    isOpen,
    title,
    message,
    children,
    type = 'confirm',
    onConfirm,
    onCancel,
    confirmText,
    cancelText,
    isDestructive = false,
    isLoading = false
}: ModalDialogProps) {
    const { t } = useTranslation();
    const dialogRef = useRef<HTMLDialogElement>(null);
    const titleId = useId();
    const messageId = useId();
    const finalConfirmText = confirmText || t('common.confirm');
    const finalCancelText = cancelText || t('common.cancel');
    const showCancel = type === 'confirm' && Boolean(onCancel);

    useEffect(() => {
        const dialog = dialogRef.current;
        if (!dialog) return;

        if (isOpen && !dialog.open) {
            dialog.showModal();
        } else if (!isOpen && dialog.open) {
            dialog.close();
        }
    }, [isOpen]);

    if (!isOpen) return null;

    const getIcon = () => {
        switch (type) {
            case 'success':
                return <CheckCircle className="w-7 h-7 text-green-500" aria-hidden="true" />;
            case 'error':
                return <XCircle className="w-7 h-7 text-red-500" aria-hidden="true" />;
            case 'info':
                return <Info className="w-7 h-7 text-blue-500" aria-hidden="true" />;
            case 'confirm':
            default:
                return isDestructive
                    ? <AlertTriangle className="w-7 h-7 text-red-500" aria-hidden="true" />
                    : <AlertTriangle className="w-7 h-7 text-blue-500" aria-hidden="true" />;
        }
    };

    const getIconBg = () => {
        switch (type) {
            case 'success': return 'bg-green-50 dark:bg-green-900/20';
            case 'error': return 'bg-red-50 dark:bg-red-900/20';
            case 'info': return 'bg-blue-50 dark:bg-blue-900/20';
            case 'confirm': default: return isDestructive ? 'bg-red-50 dark:bg-red-900/20' : 'bg-blue-50 dark:bg-blue-900/20';
        }
    };

    return (
        <dialog
            ref={dialogRef}
            aria-labelledby={titleId}
            aria-describedby={!children && message ? messageId : undefined}
            onCancel={(event) => {
                event.preventDefault();
                if (!isLoading && showCancel) {
                    onCancel?.();
                }
            }}
            onClick={(event) => {
                if (event.target === dialogRef.current && showCancel && !isLoading) {
                    onCancel?.();
                }
            }}
            className="modal modal-open z-[100] bg-transparent p-0 m-0 max-w-none max-h-none w-screen h-screen backdrop:bg-black/40 backdrop:backdrop-blur-sm"
        >
            {/* Draggable Top Region */}
            <div data-tauri-drag-region className="fixed top-0 inset-x-0 h-8 z-[110]" />

            <div className="modal-box relative max-w-sm bg-white dark:bg-base-100 shadow-2xl rounded-2xl p-0 overflow-hidden transform transition-all animate-in fade-in zoom-in-95 duration-200">
                <div className="flex flex-col items-center text-center p-6 pt-8">
                    <div className={`w-14 h-14 rounded-full flex items-center justify-center mb-4 shadow-sm ${getIconBg()}`}>
                        {getIcon()}
                    </div>

                    <h3 id={titleId} className="text-xl font-bold text-gray-900 dark:text-base-content mb-2">{title}</h3>

                    {children ? (
                        <div className="w-full text-start mb-8 px-1">
                            {children}
                        </div>
                    ) : (
                        <p id={messageId} className="text-gray-600 dark:text-gray-300 text-sm mb-8 leading-relaxed px-4">{message}</p>
                    )}

                    <div className="flex gap-3 w-full">
                        {showCancel && (
                            <button
                                type="button"
                                autoFocus={isDestructive}
                                disabled={isLoading}
                                className="flex-1 px-4 py-2.5 bg-gray-100 dark:bg-base-200 text-gray-700 dark:text-gray-300 font-medium rounded-xl hover:bg-gray-200 dark:hover:bg-base-300 transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-gray-400 dark:focus-visible:ring-base-300 disabled:opacity-50 disabled:cursor-not-allowed"
                                onClick={onCancel}
                            >
                                {finalCancelText}
                            </button>
                        )}
                        <button
                            type="button"
                            autoFocus={!isDestructive || !showCancel}
                            disabled={isLoading}
                            className={`flex-1 px-4 py-2.5 text-white font-medium rounded-xl shadow-md transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 flex items-center justify-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed ${isDestructive && type === 'confirm'
                                ? 'bg-red-600 hover:bg-red-700 focus-visible:ring-red-500 shadow-red-100'
                                : 'bg-blue-600 hover:bg-blue-700 focus-visible:ring-blue-500 shadow-blue-100'
                                }`}
                            onClick={onConfirm}
                        >
                            {isLoading && <span className="loading loading-spinner loading-xs" aria-hidden="true"></span>}
                            {finalConfirmText}
                        </button>
                    </div>
                </div>
            </div>
        </dialog>
    );
}
