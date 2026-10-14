import { forwardRef, useImperativeHandle, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowUp, Plus, Square } from 'lucide-react';
import { cn } from '../../utils/cn';
import { showToast } from '../common/ToastContainer';
import { newId } from '../../stores/useChatStore';
import {
    AttachmentError,
    MAX_ATTACHMENTS,
    MAX_ATTACHMENT_BYTES,
    formatBytes,
    readAttachment,
    type LoadedAttachment,
} from '../../services/chatAttachments';
import { AttachmentPreview } from './AttachmentPreview';

interface ChatComposerProps {
    onSend: (text: string, attachments: LoadedAttachment[]) => void;
    onStop: () => void;
    streaming: boolean;
    disabled?: boolean;
}

export interface ChatComposerHandle {
    focus: () => void;
    addFiles: (files: Iterable<File>) => void;
}

const MAX_HEIGHT = 200;

export const ChatComposer = forwardRef<ChatComposerHandle, ChatComposerProps>(function ChatComposer(
    { onSend, onStop, streaming, disabled },
    ref
) {
    const { t } = useTranslation();
    const [text, setText] = useState('');
    const [attachments, setAttachments] = useState<LoadedAttachment[]>([]);
    const [reading, setReading] = useState(0);
    const textareaRef = useRef<HTMLTextAreaElement>(null);
    const fileInputRef = useRef<HTMLInputElement>(null);
    const countRef = useRef(0);
    countRef.current = attachments.length + reading;

    const addFiles = (files: Iterable<File>) => {
        for (const file of files) {
            if (countRef.current >= MAX_ATTACHMENTS) {
                showToast(t('chat.attach_too_many', 'You can attach up to {{max}} files', { max: MAX_ATTACHMENTS }), 'error');
                break;
            }
            countRef.current += 1;
            setReading((n) => n + 1);
            readAttachment(file, newId())
                .then((a) => setAttachments((list) => [...list, a]))
                .catch((err) => {
                    const name = err instanceof AttachmentError ? err.fileName : file.name;
                    if (err instanceof AttachmentError && err.code === 'too_large') {
                        showToast(t('chat.attach_too_large', '{{name}} is larger than {{max}}', { name, max: formatBytes(MAX_ATTACHMENT_BYTES) }), 'error');
                    } else {
                        showToast(t('chat.attach_unsupported', '{{name}}: only images, PDF and text files are supported', { name }), 'error');
                    }
                })
                .finally(() => setReading((n) => n - 1));
        }
        textareaRef.current?.focus();
    };

    useImperativeHandle(ref, () => ({ focus: () => textareaRef.current?.focus(), addFiles }));

    useLayoutEffect(() => {
        const el = textareaRef.current;
        if (!el) return;
        el.style.height = 'auto';
        el.style.height = `${Math.min(el.scrollHeight, MAX_HEIGHT)}px`;
    }, [text]);

    const canSend = !streaming && !disabled && reading === 0 && (text.trim().length > 0 || attachments.length > 0);

    const submit = () => {
        if (!canSend) return;
        onSend(text.trim(), attachments);
        setText('');
        setAttachments([]);
    };

    return (
        <div
            className={cn(
                'w-full bg-white dark:bg-base-200/60 border border-gray-200 dark:border-base-200 shadow-sm px-2.5 py-2.5 focus-within:border-gray-300 dark:focus-within:border-gray-600 transition-colors',
                attachments.length + reading > 0 ? 'rounded-3xl' : 'rounded-[28px]'
            )}
        >
            {attachments.length + reading > 0 && (
                <div className="flex flex-wrap gap-2 px-1.5 pt-1 pb-2">
                    {attachments.map((a) => (
                        <AttachmentPreview
                            key={a.id}
                            attachment={a}
                            data={a.data}
                            onRemove={() => setAttachments((list) => list.filter((x) => x.id !== a.id))}
                        />
                    ))}
                    {Array.from({ length: reading }, (_, i) => (
                        <div key={`reading-${i}`} className="h-14 w-14 rounded-xl bg-gray-100 dark:bg-base-200 flex items-center justify-center">
                            <span className="loading loading-spinner loading-sm text-gray-400" />
                        </div>
                    ))}
                </div>
            )}

            <div className="flex items-end gap-2">
                <input
                    ref={fileInputRef}
                    type="file"
                    multiple
                    hidden
                    onChange={(e) => {
                        if (e.target.files) addFiles(Array.from(e.target.files));
                        e.target.value = '';
                    }}
                />
                <button
                    type="button"
                    onClick={() => fileInputRef.current?.click()}
                    title={t('chat.attach', 'Add photos and files')}
                    className="shrink-0 w-9 h-9 rounded-full flex items-center justify-center text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-base-200 transition-colors"
                >
                    <Plus size={20} />
                </button>
                <textarea
                    ref={textareaRef}
                    rows={1}
                    value={text}
                    autoFocus
                    onChange={(e) => setText(e.target.value)}
                    onPaste={(e) => {
                        const files = Array.from(e.clipboardData.files);
                        if (files.length > 0) {
                            e.preventDefault();
                            addFiles(files);
                        }
                    }}
                    onKeyDown={(e) => {
                        if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
                            e.preventDefault();
                            submit();
                        }
                    }}
                    placeholder={t('chat.placeholder', 'Ask anything')}
                    className="flex-1 resize-none bg-transparent outline-none text-[15px] leading-6 py-1.5 text-gray-900 dark:text-gray-100 placeholder:text-gray-400 max-h-[200px]"
                />
                {streaming ? (
                    <button
                        type="button"
                        onClick={onStop}
                        title={t('chat.stop', 'Stop')}
                        className="shrink-0 w-9 h-9 rounded-full flex items-center justify-center bg-gray-900 text-white dark:bg-white dark:text-gray-900 hover:opacity-80 transition-opacity"
                    >
                        <Square size={14} fill="currentColor" />
                    </button>
                ) : (
                    <button
                        type="button"
                        onClick={submit}
                        disabled={!canSend}
                        title={t('chat.send', 'Send')}
                        className={cn(
                            'shrink-0 w-9 h-9 rounded-full flex items-center justify-center transition-colors',
                            canSend
                                ? 'bg-blue-600 text-white hover:bg-blue-700'
                                : 'bg-gray-200 text-gray-400 dark:bg-base-200 dark:text-gray-500 cursor-not-allowed'
                        )}
                    >
                        <ArrowUp size={18} />
                    </button>
                )}
            </div>
        </div>
    );
});
