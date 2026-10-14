import { memo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { AlertCircle, Brain, Check, ChevronRight, Copy, RotateCcw } from 'lucide-react';
import { cn } from '../../utils/cn';
import type { ChatMessage } from '../../stores/useChatStore';

async function copyText(text: string) {
    try {
        await navigator.clipboard.writeText(text);
        return true;
    } catch {
        return false;
    }
}

function CopyButton({ text, withLabel, className }: { text: string; withLabel?: boolean; className?: string }) {
    const { t } = useTranslation();
    const [copied, setCopied] = useState(false);
    return (
        <button
            type="button"
            title={t('chat.copy', 'Copy')}
            onClick={async () => {
                if (await copyText(text)) {
                    setCopied(true);
                    setTimeout(() => setCopied(false), 1500);
                }
            }}
            className={cn('inline-flex items-center gap-1 transition-colors', className)}
        >
            {copied ? <Check size={14} /> : <Copy size={14} />}
            {withLabel && <span>{copied ? t('chat.copied', 'Copied') : t('chat.copy', 'Copy')}</span>}
        </button>
    );
}

/** Plain text of a hast node, used to copy fenced code blocks verbatim. */
function nodeText(node: any): string {
    if (!node) return '';
    if (node.type === 'text') return node.value ?? '';
    return (node.children ?? []).map(nodeText).join('');
}

const MARKDOWN_CLASSES = cn(
    'text-[15px] leading-7 text-gray-800 dark:text-gray-100 break-words',
    '[&>*:first-child]:mt-0 [&>*:last-child]:mb-0',
    '[&_p]:my-3 [&_ul]:my-3 [&_ol]:my-3 [&_ul]:list-disc [&_ol]:list-decimal [&_ul]:pl-6 [&_ol]:pl-6 [&_li]:my-1',
    '[&_h1]:text-2xl [&_h2]:text-xl [&_h3]:text-lg [&_h1]:font-bold [&_h2]:font-semibold [&_h3]:font-semibold',
    '[&_h1]:mt-6 [&_h2]:mt-5 [&_h3]:mt-4 [&_h1]:mb-3 [&_h2]:mb-2 [&_h3]:mb-2',
    '[&_a]:text-blue-600 dark:[&_a]:text-blue-400 [&_a]:underline',
    '[&_blockquote]:border-l-4 [&_blockquote]:border-gray-300 dark:[&_blockquote]:border-gray-600 [&_blockquote]:pl-4 [&_blockquote]:text-gray-600 dark:[&_blockquote]:text-gray-300',
    '[&_hr]:my-6 [&_hr]:border-gray-200 dark:[&_hr]:border-base-200',
    '[&_table]:w-full [&_table]:text-sm [&_table]:border-collapse',
    '[&_th]:border [&_td]:border [&_th]:border-gray-200 [&_td]:border-gray-200 dark:[&_th]:border-base-200 dark:[&_td]:border-base-200 [&_th]:px-3 [&_td]:px-3 [&_th]:py-1.5 [&_td]:py-1.5 [&_th]:bg-gray-50 dark:[&_th]:bg-base-200',
    '[&_:not(pre)>code]:px-1.5 [&_:not(pre)>code]:py-0.5 [&_:not(pre)>code]:rounded [&_:not(pre)>code]:bg-gray-100 dark:[&_:not(pre)>code]:bg-base-200 [&_:not(pre)>code]:text-[0.85em] [&_:not(pre)>code]:font-mono'
);

const markdownComponents = {
    pre: ({ node, children }: { node?: any; children?: ReactNode }) => {
        const codeNode = node?.children?.[0];
        const lang = (codeNode?.properties?.className as string[] | undefined)
            ?.find((c) => c.startsWith('language-'))
            ?.slice('language-'.length);
        return (
            <div className="my-4 rounded-xl overflow-hidden border border-gray-200 dark:border-base-200 bg-gray-50 dark:bg-[#0b1220]">
                <div className="flex items-center justify-between px-4 py-1.5 text-xs text-gray-500 dark:text-gray-400 bg-gray-100 dark:bg-base-200/60">
                    <span className="font-mono">{lang || 'text'}</span>
                    <CopyButton text={nodeText(codeNode).replace(/\n$/, '')} withLabel className="hover:text-gray-800 dark:hover:text-gray-100" />
                </div>
                <pre className="overflow-x-auto p-4 text-[13px] leading-6 font-mono text-gray-800 dark:text-gray-100">{children}</pre>
            </div>
        );
    },
    a: ({ children, href }: { children?: ReactNode; href?: string }) => (
        <a href={href} target="_blank" rel="noreferrer noopener">
            {children}
        </a>
    ),
};

export const MarkdownContent = memo(function MarkdownContent({ text }: { text: string }) {
    return (
        <div className={MARKDOWN_CLASSES}>
            <Markdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
                {text}
            </Markdown>
        </div>
    );
});

function ThinkingBlock({ text, active }: { text: string; active: boolean }) {
    const { t } = useTranslation();
    const [open, setOpen] = useState(false);
    return (
        <div className="mb-3">
            <button
                type="button"
                onClick={() => setOpen((o) => !o)}
                className="inline-flex items-center gap-1.5 text-sm text-gray-500 dark:text-gray-400 hover:text-gray-800 dark:hover:text-gray-200"
            >
                <Brain size={14} className={cn(active && 'animate-pulse')} />
                {active ? t('chat.thinking_active', 'Thinking…') : t('chat.thinking', 'Thoughts')}
                <ChevronRight size={14} className={cn('transition-transform', open && 'rotate-90')} />
            </button>
            {open && (
                <div className="mt-2 pl-4 border-l-2 border-gray-200 dark:border-base-200 text-sm leading-6 text-gray-500 dark:text-gray-400 whitespace-pre-wrap">
                    {text}
                </div>
            )}
        </div>
    );
}

interface ChatMessageViewProps {
    message: ChatMessage;
    isLast: boolean;
    onRegenerate?: () => void;
}

export const ChatMessageView = memo(function ChatMessageView({ message, isLast, onRegenerate }: ChatMessageViewProps) {
    const { t } = useTranslation();

    if (message.role === 'user') {
        return (
            <div className="flex justify-end">
                <div className="max-w-[80%] rounded-3xl bg-gray-100 dark:bg-base-200 px-5 py-2.5 text-[15px] leading-7 text-gray-900 dark:text-gray-100 whitespace-pre-wrap break-words">
                    {message.content}
                </div>
            </div>
        );
    }

    const waiting = message.pending && !message.content && !message.thinking;
    const meta = [
        message.model,
        message.protocol,
        message.durationMs !== undefined && `${(message.durationMs / 1000).toFixed(1)}s`,
        message.usage?.input !== undefined && `↑${message.usage.input}`,
        message.usage?.output !== undefined && `↓${message.usage.output}`,
    ].filter(Boolean);

    return (
        <div className="group">
            {message.thinking && <ThinkingBlock text={message.thinking} active={!!message.pending && !message.content} />}

            {waiting ? (
                <div className="flex items-center gap-1 h-7">
                    <span className="w-2 h-2 rounded-full bg-gray-400 animate-bounce [animation-delay:-0.3s]" />
                    <span className="w-2 h-2 rounded-full bg-gray-400 animate-bounce [animation-delay:-0.15s]" />
                    <span className="w-2 h-2 rounded-full bg-gray-400 animate-bounce" />
                </div>
            ) : (
                message.content && <MarkdownContent text={message.content} />
            )}

            {message.error && (
                <div className="mt-3 flex items-start gap-2 rounded-xl border border-red-200 dark:border-red-900/60 bg-red-50 dark:bg-red-950/30 px-4 py-3 text-sm text-red-700 dark:text-red-300">
                    <AlertCircle size={16} className="shrink-0 mt-0.5" />
                    <span className="break-words whitespace-pre-wrap min-w-0">{message.error}</span>
                </div>
            )}

            {!message.pending && (
                <div
                    className={cn(
                        'mt-2 flex items-center gap-1 text-gray-400 transition-opacity',
                        !isLast && 'opacity-0 group-hover:opacity-100'
                    )}
                >
                    {message.content && (
                        <CopyButton text={message.content} className="p-1.5 rounded-md hover:bg-gray-100 dark:hover:bg-base-200 hover:text-gray-700 dark:hover:text-gray-200" />
                    )}
                    {onRegenerate && (
                        <button
                            type="button"
                            onClick={onRegenerate}
                            title={t('chat.regenerate', 'Regenerate')}
                            className="p-1.5 rounded-md hover:bg-gray-100 dark:hover:bg-base-200 hover:text-gray-700 dark:hover:text-gray-200"
                        >
                            <RotateCcw size={14} />
                        </button>
                    )}
                    {meta.length > 0 && <span className="ml-2 text-xs font-mono truncate">{meta.join(' · ')}</span>}
                </div>
            )}
        </div>
    );
});
