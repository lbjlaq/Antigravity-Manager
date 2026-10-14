import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { FileText, ImageOff, X } from 'lucide-react';
import { cn } from '../../utils/cn';
import { formatBytes, loadAttachmentData, toDataUrl, type ChatAttachment } from '../../services/chatAttachments';

/** Payload of a stored attachment; `null` once it is known to be unavailable. */
export function useAttachmentData(id: string, initial?: string): string | null | undefined {
    const [data, setData] = useState<string | null | undefined>(initial);
    useEffect(() => {
        if (initial !== undefined) return;
        let cancelled = false;
        loadAttachmentData(id).then((d) => !cancelled && setData(d ?? null));
        return () => {
            cancelled = true;
        };
    }, [id, initial]);
    return data;
}

function Lightbox({ src, alt, onClose }: { src: string; alt: string; onClose: () => void }) {
    useEffect(() => {
        const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
        document.addEventListener('keydown', onKey);
        return () => document.removeEventListener('keydown', onKey);
    }, [onClose]);
    return createPortal(
        <div className="fixed inset-0 z-[10000] bg-black/80 flex items-center justify-center p-6" onClick={onClose}>
            <img src={src} alt={alt} className="max-w-full max-h-full object-contain rounded-lg shadow-2xl" />
        </div>,
        document.body
    );
}

interface AttachmentPreviewProps {
    attachment: ChatAttachment;
    /** Payload already in hand (composer); otherwise it is loaded from storage. */
    data?: string;
    onRemove?: () => void;
    size?: 'sm' | 'lg';
}

export function AttachmentPreview({ attachment, data, onRemove, size = 'sm' }: AttachmentPreviewProps) {
    const { t } = useTranslation();
    const payload = useAttachmentData(attachment.id, attachment.kind === 'image' ? data : '');
    const [zoomed, setZoomed] = useState(false);

    const removeButton = onRemove && (
        <button
            type="button"
            onClick={onRemove}
            title={t('chat.remove_attachment', 'Remove')}
            className="absolute -top-1.5 -right-1.5 w-5 h-5 rounded-full bg-gray-900 text-white dark:bg-white dark:text-gray-900 flex items-center justify-center shadow opacity-90 hover:opacity-100"
        >
            <X size={12} />
        </button>
    );

    if (attachment.kind === 'image') {
        const box = size === 'lg' ? 'h-40 max-w-[16rem]' : 'h-14 w-14';
        if (payload === null) {
            return (
                <div
                    title={t('chat.attach_unavailable', 'Attachment is no longer available')}
                    className={cn('relative rounded-xl border border-dashed border-gray-300 dark:border-base-200 flex items-center justify-center text-gray-400', size === 'lg' ? 'h-24 w-24' : box)}
                >
                    <ImageOff size={18} />
                    {removeButton}
                </div>
            );
        }
        const src = payload ? toDataUrl({ mime: attachment.mime, data: payload }) : undefined;
        return (
            <div className="relative">
                {src ? (
                    <img
                        src={src}
                        alt={attachment.name}
                        title={attachment.name}
                        onClick={() => setZoomed(true)}
                        className={cn('rounded-xl object-cover cursor-zoom-in border border-gray-200 dark:border-base-200', box)}
                    />
                ) : (
                    <div className={cn('rounded-xl bg-gray-100 dark:bg-base-200 animate-pulse', box)} />
                )}
                {removeButton}
                {zoomed && src && <Lightbox src={src} alt={attachment.name} onClose={() => setZoomed(false)} />}
            </div>
        );
    }

    return (
        <div
            title={attachment.name}
            className="relative flex items-center gap-2.5 max-w-[16rem] h-14 pl-2.5 pr-3 rounded-xl border border-gray-200 dark:border-base-200 bg-white dark:bg-base-100"
        >
            <div
                className={cn(
                    'shrink-0 w-9 h-9 rounded-lg flex items-center justify-center text-white',
                    attachment.kind === 'pdf' ? 'bg-red-500' : 'bg-blue-500'
                )}
            >
                <FileText size={18} />
            </div>
            <div className="min-w-0">
                <div className="text-sm font-medium text-gray-800 dark:text-gray-100 truncate">{attachment.name}</div>
                <div className="text-xs text-gray-400 truncate">
                    {attachment.kind === 'pdf' ? 'PDF' : (/\.([^./]+)$/.exec(attachment.name)?.[1] ?? 'txt').toUpperCase()} · {formatBytes(attachment.size)}
                </div>
            </div>
            {removeButton}
        </div>
    );
}
