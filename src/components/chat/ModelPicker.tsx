import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Bot, Check, ChevronDown, Search } from 'lucide-react';
import { Gemini, Claude, OpenAI } from '@lobehub/icons';
import { cn } from '../../utils/cn';
import { compareModelsDesc, getModelConfig, getGroupPriority, inferModelGroup } from '../../config/modelConfig';

export interface PickerModel {
    id: string;
    group: string;
}

export function toPickerModels(ids: string[]): PickerModel[] {
    return Array.from(new Set(ids))
        .map((id) => ({ id, name: id, group: getModelConfig(id)?.group ?? inferModelGroup(id) }))
        .sort(compareModelsDesc);
}

export function ModelIcon({ id, size = 16 }: { id: string; size?: number }) {
    const cfg = getModelConfig(id);
    if (cfg) return <cfg.Icon size={size} />;
    const group = inferModelGroup(id);
    if (group.startsWith('Gemini')) return <Gemini.Color size={size} />;
    if (group === 'Claude') return <Claude.Color size={size} />;
    if (group === 'OpenAI') return <OpenAI size={size} />;
    return <Bot size={size} className="text-blue-400" />;
}

interface ModelPickerProps {
    models: PickerModel[];
    value: string;
    onChange: (id: string) => void;
    loading?: boolean;
}

export function ModelPicker({ models, value, onChange, loading }: ModelPickerProps) {
    const { t } = useTranslation();
    const [open, setOpen] = useState(false);
    const [query, setQuery] = useState('');
    const rootRef = useRef<HTMLDivElement>(null);
    const searchRef = useRef<HTMLInputElement>(null);

    useEffect(() => {
        if (!open) return;
        searchRef.current?.focus();
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

    const groups = useMemo(() => {
        const q = query.trim().toLowerCase();
        const map = new Map<string, PickerModel[]>();
        for (const m of models) {
            if (q && !m.id.toLowerCase().includes(q)) continue;
            const list = map.get(m.group) ?? [];
            list.push(m);
            map.set(m.group, list);
        }
        return Array.from(map.entries()).sort(([a], [b]) => getGroupPriority(a) - getGroupPriority(b));
    }, [models, query]);

    const select = (id: string) => {
        onChange(id);
        setOpen(false);
        setQuery('');
    };

    return (
        <div ref={rootRef} className="relative min-w-0">
            <button
                type="button"
                onClick={() => setOpen((o) => !o)}
                className="flex items-center gap-2 max-w-full px-3 py-1.5 rounded-xl text-sm font-semibold text-gray-800 dark:text-gray-100 hover:bg-gray-100 dark:hover:bg-base-200 transition-colors"
            >
                {value ? <ModelIcon id={value} /> : <Bot size={16} className="text-gray-400" />}
                <span className="truncate">{value || t('chat.select_model', 'Select model')}</span>
                {loading ? (
                    <span className="loading loading-spinner loading-xs text-gray-400" />
                ) : (
                    <ChevronDown size={14} className={cn('text-gray-400 transition-transform', open && 'rotate-180')} />
                )}
            </button>

            {open && (
                <div className="absolute left-0 top-full mt-2 z-50 w-80 max-w-[calc(100vw-2rem)] bg-white dark:bg-base-100 rounded-xl shadow-xl border border-gray-100 dark:border-base-200 overflow-hidden">
                    <div className="flex items-center gap-2 px-3 py-2 border-b border-gray-100 dark:border-base-200">
                        <Search size={14} className="text-gray-400 shrink-0" />
                        <input
                            ref={searchRef}
                            value={query}
                            onChange={(e) => setQuery(e.target.value)}
                            onKeyDown={(e) => {
                                if (e.key === 'Enter') {
                                    // Allow testing any model id, even one not listed by the proxy.
                                    const first = groups[0]?.[1][0]?.id;
                                    const typed = query.trim();
                                    if (first || typed) select(first ?? typed);
                                }
                            }}
                            placeholder={t('chat.search_model', 'Search or type a model ID')}
                            className="flex-1 bg-transparent text-sm outline-none text-gray-800 dark:text-gray-100 placeholder:text-gray-400"
                        />
                    </div>
                    <div className="max-h-80 overflow-y-auto py-1">
                        {groups.length === 0 ? (
                            <div className="px-3 py-4 text-xs text-gray-400 text-center">
                                {query.trim() ? (
                                    <button type="button" className="btn btn-ghost btn-xs" onClick={() => select(query.trim())}>
                                        {t('chat.use_custom_model', 'Use "{{model}}"', { model: query.trim() })}
                                    </button>
                                ) : (
                                    t('chat.no_models', 'No models available')
                                )}
                            </div>
                        ) : (
                            groups.map(([group, items]) => (
                                <div key={group}>
                                    <div className="px-3 pt-2 pb-1 text-[10px] font-bold uppercase tracking-wider text-gray-400">{group}</div>
                                    {items.map((m) => (
                                        <button
                                            key={m.id}
                                            type="button"
                                            onClick={() => select(m.id)}
                                            className={cn(
                                                'w-full flex items-center gap-2 px-3 py-1.5 text-left text-sm hover:bg-gray-100 dark:hover:bg-base-200',
                                                m.id === value ? 'text-blue-600 dark:text-blue-400' : 'text-gray-700 dark:text-gray-200'
                                            )}
                                        >
                                            <ModelIcon id={m.id} />
                                            <span className="flex-1 truncate font-mono text-xs">{m.id}</span>
                                            {m.id === value && <Check size={14} />}
                                        </button>
                                    ))}
                                </div>
                            ))
                        )}
                    </div>
                </div>
            )}
        </div>
    );
}
