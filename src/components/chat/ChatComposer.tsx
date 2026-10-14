import { forwardRef, useImperativeHandle, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowUp, Square } from 'lucide-react';
import { cn } from '../../utils/cn';

interface ChatComposerProps {
    onSend: (text: string) => void;
    onStop: () => void;
    streaming: boolean;
    disabled?: boolean;
}

export interface ChatComposerHandle {
    focus: () => void;
}

const MAX_HEIGHT = 200;

export const ChatComposer = forwardRef<ChatComposerHandle, ChatComposerProps>(function ChatComposer(
    { onSend, onStop, streaming, disabled },
    ref
) {
    const { t } = useTranslation();
    const [text, setText] = useState('');
    const textareaRef = useRef<HTMLTextAreaElement>(null);

    useImperativeHandle(ref, () => ({ focus: () => textareaRef.current?.focus() }), []);

    useLayoutEffect(() => {
        const el = textareaRef.current;
        if (!el) return;
        el.style.height = 'auto';
        el.style.height = `${Math.min(el.scrollHeight, MAX_HEIGHT)}px`;
    }, [text]);

    const canSend = !streaming && !disabled && text.trim().length > 0;

    const submit = () => {
        if (!canSend) return;
        onSend(text.trim());
        setText('');
    };

    return (
        <div className="flex items-end gap-2 w-full rounded-[28px] bg-white dark:bg-base-200/60 border border-gray-200 dark:border-base-200 shadow-sm px-4 py-2.5 focus-within:border-gray-300 dark:focus-within:border-gray-600 transition-colors">
            <textarea
                ref={textareaRef}
                rows={1}
                value={text}
                autoFocus
                onChange={(e) => setText(e.target.value)}
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
    );
});
