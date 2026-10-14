import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { cn } from '../../utils/cn';
import type { ChatSettings } from '../../stores/useChatStore';

interface ChatSettingsPopoverProps {
    systemPrompt: string;
    temperature?: number;
    onChange: (patch: Partial<ChatSettings>) => void;
    trigger: ReactNode;
}

const DEFAULT_TEMPERATURE = 1;

export function ChatSettingsPopover({ systemPrompt, temperature, onChange, trigger }: ChatSettingsPopoverProps) {
    const { t } = useTranslation();
    const [open, setOpen] = useState(false);
    const rootRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        if (!open) return;
        const onClick = (e: MouseEvent) => {
            if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
        };
        const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false);
        document.addEventListener('mousedown', onClick);
        document.addEventListener('keydown', onKey);
        return () => {
            document.removeEventListener('mousedown', onClick);
            document.removeEventListener('keydown', onKey);
        };
    }, [open]);

    const customized = !!systemPrompt.trim() || temperature !== undefined;

    return (
        <div ref={rootRef} className="relative">
            <button
                type="button"
                onClick={() => setOpen((o) => !o)}
                title={t('chat.settings', 'Chat settings')}
                className={cn(
                    'relative p-2 rounded-lg hover:bg-gray-100 dark:hover:bg-base-200',
                    open ? 'text-gray-900 dark:text-white' : 'text-gray-500'
                )}
            >
                {trigger}
                {customized && <span className="absolute top-1.5 right-1.5 w-1.5 h-1.5 rounded-full bg-blue-500" />}
            </button>

            {open && (
                <div className="absolute right-0 top-full mt-2 z-50 w-80 max-w-[calc(100vw-2rem)] bg-white dark:bg-base-100 rounded-xl shadow-xl border border-gray-100 dark:border-base-200 p-4 flex flex-col gap-4">
                    <label className="flex flex-col gap-1.5">
                        <span className="text-xs font-semibold text-gray-600 dark:text-gray-300">{t('chat.system_prompt', 'System prompt')}</span>
                        <textarea
                            rows={4}
                            value={systemPrompt}
                            onChange={(e) => onChange({ systemPrompt: e.target.value })}
                            placeholder={t('chat.system_prompt_placeholder', 'Optional instructions sent with every request')}
                            className="textarea textarea-bordered textarea-sm w-full resize-y text-sm"
                        />
                    </label>

                    <div className="flex flex-col gap-1.5">
                        <div className="flex items-center justify-between">
                            <span className="text-xs font-semibold text-gray-600 dark:text-gray-300">{t('chat.temperature', 'Temperature')}</span>
                            <label className="flex items-center gap-2 text-xs text-gray-500">
                                {t('chat.temperature_default', 'Model default')}
                                <input
                                    type="checkbox"
                                    className="toggle toggle-xs toggle-primary"
                                    checked={temperature === undefined}
                                    onChange={(e) => onChange({ temperature: e.target.checked ? undefined : DEFAULT_TEMPERATURE })}
                                />
                            </label>
                        </div>
                        {temperature !== undefined && (
                            <div className="flex items-center gap-3">
                                <input
                                    type="range"
                                    min={0}
                                    max={2}
                                    step={0.1}
                                    value={temperature}
                                    onChange={(e) => onChange({ temperature: Number(e.target.value) })}
                                    className="range range-xs range-primary flex-1"
                                />
                                <span className="w-8 text-right text-xs font-mono text-gray-600 dark:text-gray-300">{temperature.toFixed(1)}</span>
                            </div>
                        )}
                    </div>
                </div>
            )}
        </div>
    );
}
