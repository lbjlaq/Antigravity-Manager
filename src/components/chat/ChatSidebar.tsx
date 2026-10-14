import { useTranslation } from 'react-i18next';
import { MessageSquare, SquarePen, Trash2 } from 'lucide-react';
import { cn } from '../../utils/cn';
import type { ChatSession } from '../../stores/useChatStore';

interface ChatSidebarProps {
    sessions: ChatSession[];
    activeId: string | null;
    onNewChat: () => void;
    onSelect: (id: string) => void;
    onDelete: (id: string) => void;
}

export function ChatSidebar({ sessions, activeId, onNewChat, onSelect, onDelete }: ChatSidebarProps) {
    const { t } = useTranslation();
    const sorted = [...sessions].sort((a, b) => b.updatedAt - a.updatedAt);

    return (
        <div className="h-full flex flex-col">
            <div className="p-2">
                <button
                    type="button"
                    onClick={onNewChat}
                    className={cn(
                        'w-full flex items-center gap-2 px-3 py-2 rounded-lg text-sm font-medium transition-colors',
                        activeId === null
                            ? 'bg-gray-200/70 dark:bg-white/10 text-gray-900 dark:text-white'
                            : 'text-gray-700 dark:text-gray-200 hover:bg-gray-200/50 dark:hover:bg-white/5'
                    )}
                >
                    <SquarePen size={16} />
                    {t('chat.new_chat', 'New chat')}
                </button>
            </div>

            <div className="px-5 pt-3 pb-1 text-xs font-medium text-gray-400">{t('chat.recents', 'Recents')}</div>

            <div className="flex-1 overflow-y-auto px-2 pb-3">
                {sorted.length === 0 ? (
                    <div className="px-3 py-6 text-xs text-gray-400 flex flex-col items-center gap-2">
                        <MessageSquare size={18} className="opacity-60" />
                        {t('chat.no_history', 'No chats yet')}
                    </div>
                ) : (
                    sorted.map((s) => (
                        <div
                            key={s.id}
                            className={cn(
                                'group flex items-center rounded-lg transition-colors',
                                s.id === activeId
                                    ? 'bg-gray-200/70 dark:bg-white/10'
                                    : 'hover:bg-gray-200/50 dark:hover:bg-white/5'
                            )}
                        >
                            <button
                                type="button"
                                onClick={() => onSelect(s.id)}
                                title={s.title}
                                className="flex-1 min-w-0 text-left px-3 py-2 text-sm text-gray-700 dark:text-gray-200 truncate"
                            >
                                {s.title}
                            </button>
                            <button
                                type="button"
                                onClick={() => onDelete(s.id)}
                                title={t('chat.delete', 'Delete')}
                                className="shrink-0 mr-1 p-1.5 rounded-md text-gray-400 hover:text-red-500 opacity-0 group-hover:opacity-100 focus:opacity-100 transition-opacity"
                            >
                                <Trash2 size={14} />
                            </button>
                        </div>
                    ))
                )}
            </div>
        </div>
    );
}
