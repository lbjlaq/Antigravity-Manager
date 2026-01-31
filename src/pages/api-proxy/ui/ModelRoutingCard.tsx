// File: src/pages/api-proxy/ui/ModelRoutingCard.tsx
// Model routing center card

import { useTranslation } from 'react-i18next';
import {
    BrainCircuit,
    Sparkles,
    RefreshCw,
    Target,
    Plus,
    ArrowRight,
    Trash2,
    Edit2,
    Check,
    X
} from 'lucide-react';
import GroupedSelect, { SelectOption } from '@/components/common/GroupedSelect';
import type { AppConfig } from '@/entities/config';

interface ModelRoutingCardProps {
    appConfig: AppConfig;
    customMappingOptions: SelectOption[];
    customMappingValue: string;
    editingKey: string | null;
    editingValue: string;
    onMappingUpdate: (type: 'custom', key: string, value: string) => void;
    onRemoveCustomMapping: (key: string) => void;
    onApplyPresets: () => void;
    onResetMapping: () => void;
    setCustomMappingValue: (value: string) => void;
    setEditingKey: (key: string | null) => void;
    setEditingValue: (value: string) => void;
}

export function ModelRoutingCard({
    appConfig,
    customMappingOptions,
    customMappingValue,
    editingKey,
    editingValue,
    onMappingUpdate,
    onRemoveCustomMapping,
    onApplyPresets,
    onResetMapping,
    setCustomMappingValue,
    setEditingKey,
    setEditingValue,
}: ModelRoutingCardProps) {
    const { t } = useTranslation();

    return (
        <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200 overflow-hidden">
            <div className="px-4 py-2.5 border-b border-gray-100 dark:border-gray-700/50 bg-gray-50/50 dark:bg-gray-800/50">
                <div className="flex items-center justify-between">
                    <div>
                        <h2 className="text-base font-bold flex items-center gap-2 text-gray-900 dark:text-base-content">
                            <BrainCircuit size={18} className="text-blue-500" />
                            {t('proxy.router.title')}
                        </h2>
                        <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                            {t('proxy.router.subtitle_simple')}
                        </p>
                    </div>
                    <div className="flex items-center gap-2">
                        <button
                            onClick={onApplyPresets}
                            className="px-3 py-1 rounded-lg text-xs font-medium transition-colors flex items-center gap-2 bg-blue-500 text-white hover:bg-blue-600 shadow-sm"
                        >
                            <Sparkles size={14} />
                            {t('proxy.router.apply_presets')}
                        </button>
                        <button
                            onClick={onResetMapping}
                            className="px-3 py-1 rounded-lg text-xs font-medium transition-colors flex items-center gap-2 bg-white dark:bg-base-100 border border-gray-200 dark:border-gray-700 text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-base-200 hover:text-blue-600 dark:hover:text-blue-400 hover:border-blue-200 dark:hover:border-blue-800 shadow-sm"
                        >
                            <RefreshCw size={14} />
                            {t('proxy.router.reset_mapping')}
                        </button>
                    </div>
                </div>
            </div>

            <div className="p-3 space-y-3">
                {/* Background task model config */}
                <div className="mb-4 pb-4 border-b border-gray-100 dark:border-base-200">
                    <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                        <div className="flex-1">
                            <h3 className="text-xs font-bold text-gray-700 dark:text-gray-300 flex items-center gap-2">
                                <Sparkles size={14} className="text-blue-500" />
                                {t('proxy.router.background_task_title')}
                            </h3>
                            <p className="text-[10px] text-gray-500 dark:text-gray-400 mt-0.5">
                                {t('proxy.router.background_task_desc')}
                            </p>
                        </div>

                        <div className="flex items-center gap-2 w-full sm:w-auto min-w-[200px] max-w-sm">
                            <div className="relative flex-1">
                                <GroupedSelect
                                    value={appConfig.proxy.custom_mapping?.['internal-background-task'] || ''}
                                    onChange={(val) => onMappingUpdate('custom', 'internal-background-task', val)}
                                    options={[
                                        { value: '', label: 'Default (gemini-2.5-flash)', group: 'System' },
                                        ...customMappingOptions
                                    ]}
                                    placeholder="Default (gemini-2.5-flash)"
                                    className="font-mono text-[11px] h-8 dark:bg-base-200 w-full"
                                />
                            </div>

                            {appConfig.proxy.custom_mapping?.['internal-background-task'] && (
                                <button
                                    onClick={() => onRemoveCustomMapping('internal-background-task')}
                                    className="p-1.5 text-gray-400 hover:text-blue-500 hover:bg-blue-50 dark:hover:bg-blue-900/30 rounded transition-colors"
                                    title={t('proxy.router.use_default')}
                                >
                                    <RefreshCw size={12} />
                                </button>
                            )}
                        </div>
                    </div>
                </div>

                {/* Custom mappings section */}
                <div>
                    <div className="flex items-center justify-between mb-3">
                        <h3 className="text-[10px] font-bold text-gray-400 uppercase tracking-widest flex items-center gap-2">
                            <ArrowRight size={14} /> {t('proxy.router.custom_mappings')}
                        </h3>
                    </div>
                    <div className="flex flex-col gap-4">
                        {/* Current mapping list */}
                        <div className="w-full flex flex-col">
                            <div className="flex items-center justify-between mb-2">
                                <span className="text-[10px] font-bold text-gray-400 dark:text-gray-500 uppercase tracking-wider">
                                    {t('proxy.router.current_list')}
                                </span>
                            </div>
                            <div className="overflow-y-auto max-h-[180px] border border-gray-100 dark:border-white/5 rounded-lg bg-gray-50/10 dark:bg-white/5 p-3" data-custom-mapping-list>
                                <div className="grid grid-cols-1 md:grid-cols-2 gap-x-6 gap-y-2">
                                    {appConfig.proxy.custom_mapping && Object.entries(appConfig.proxy.custom_mapping).length > 0 ? (
                                        Object.entries(appConfig.proxy.custom_mapping).map(([key, val]) => (
                                            <div key={key} className={`flex items-center justify-between p-1.5 rounded-md transition-all border group ${editingKey === key ? 'bg-blue-50/80 dark:bg-blue-900/15 border-blue-300/50 dark:border-blue-500/30 shadow-sm' : 'border-transparent hover:bg-gray-100 dark:hover:bg-white/5 hover:border-gray-200 dark:hover:border-white/10'}`}>
                                                <div className="flex items-center gap-2.5 overflow-hidden flex-1">
                                                    <span className="font-mono text-[10px] font-bold text-blue-600 dark:text-blue-400 truncate max-w-[140px]" title={key}>{key}</span>
                                                    <ArrowRight size={10} className="text-gray-300 dark:text-gray-600 shrink-0" />

                                                    {editingKey === key ? (
                                                        <div className="flex-1 mr-2">
                                                            <GroupedSelect
                                                                value={editingValue}
                                                                onChange={setEditingValue}
                                                                options={customMappingOptions}
                                                                placeholder="Select..."
                                                                className="font-mono text-[10px] h-7 dark:bg-gray-800 border-blue-200 dark:border-blue-800"
                                                            />
                                                        </div>
                                                    ) : (
                                                        <span className="font-mono text-[10px] text-gray-500 dark:text-gray-400 truncate cursor-pointer hover:text-blue-500"
                                                            onClick={() => { setEditingKey(key); setEditingValue(val); }}
                                                            title={val}>{val}</span>
                                                    )}
                                                </div>

                                                <div className="flex items-center gap-1.5 shrink-0">
                                                    {editingKey === key ? (
                                                        <div className="flex items-center gap-1 bg-white dark:bg-gray-800 rounded-md border border-blue-200 dark:border-blue-800 p-0.5 shadow-sm">
                                                            <button
                                                                className="btn btn-ghost btn-xs text-primary hover:bg-blue-50 dark:hover:bg-blue-900/30 p-0 h-6 w-6 min-h-0"
                                                                onClick={() => { onMappingUpdate('custom', key, editingValue); setEditingKey(null); }}
                                                                title={t('common.save') || 'Save'}
                                                            >
                                                                <Check size={14} strokeWidth={3} />
                                                            </button>
                                                            <div className="w-[1px] h-3 bg-gray-200 dark:bg-gray-700" />
                                                            <button
                                                                className="btn btn-ghost btn-xs text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700 p-0 h-6 w-6 min-h-0"
                                                                onClick={() => setEditingKey(null)}
                                                                title={t('common.cancel') || 'Cancel'}
                                                            >
                                                                <X size={14} strokeWidth={3} />
                                                            </button>
                                                        </div>
                                                    ) : (
                                                        <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                                                            <button
                                                                className="btn btn-ghost btn-xs text-gray-400 hover:text-blue-500 hover:bg-blue-50 dark:hover:bg-white/10 p-0 h-6 w-6 min-h-0"
                                                                onClick={() => { setEditingKey(key); setEditingValue(val); }}
                                                                title={t('common.edit') || 'Edit'}
                                                            >
                                                                <Edit2 size={12} />
                                                            </button>
                                                            <button
                                                                className="btn btn-ghost btn-xs text-error hover:bg-red-50 dark:hover:bg-red-900/20 p-0 h-6 w-6 min-h-0"
                                                                onClick={() => onRemoveCustomMapping(key)}
                                                                title={t('common.delete') || 'Delete'}
                                                            >
                                                                <Trash2 size={12} />
                                                            </button>
                                                        </div>
                                                    )}
                                                </div>
                                            </div>
                                        ))
                                    ) : (
                                        <div className="col-span-full text-center py-4 text-gray-400 dark:text-gray-600 italic text-[11px]">{t('proxy.router.no_custom_mapping')}</div>
                                    )}
                                </div>
                            </div>
                        </div>

                        {/* Add mapping form */}
                        <div className="w-full bg-gray-50/50 dark:bg-white/5 p-2.5 rounded-xl border border-gray-100 dark:border-white/5 shadow-inner">
                            <div className="flex flex-col sm:flex-row items-center gap-3">
                                <div className="flex items-center gap-1.5 shrink-0">
                                    <Target size={14} className="text-gray-400 dark:text-gray-500" />
                                    <span className="text-[10px] font-bold text-gray-400 dark:text-gray-500 uppercase tracking-wider">{t('proxy.router.add_mapping')}</span>
                                </div>
                                <div className="flex-1 flex flex-col sm:flex-row gap-2 w-full">
                                    <input
                                        id="custom-key"
                                        type="text"
                                        placeholder={t('proxy.router.original_placeholder') || "Original (e.g. gpt-4 or gpt-4*)"}
                                        className="input input-xs input-bordered flex-1 font-mono text-[11px] bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 shadow-sm focus:border-blue-500 focus:ring-1 focus:ring-blue-500 transition-all placeholder:text-gray-400 dark:placeholder:text-gray-600 h-8"
                                    />
                                    <div className="w-full sm:w-48">
                                        <GroupedSelect
                                            value={customMappingValue}
                                            onChange={setCustomMappingValue}
                                            options={customMappingOptions}
                                            placeholder={t('proxy.router.select_target_model') || 'Select Target Model'}
                                            className="font-mono text-[11px] h-8 dark:bg-gray-800"
                                        />
                                    </div>
                                </div>
                                <button
                                    className="btn btn-xs sm:w-20 gap-1.5 shadow-md hover:shadow-lg transition-all bg-blue-600 hover:bg-blue-700 text-white border-none h-8"
                                    onClick={() => {
                                        const k = (document.getElementById('custom-key') as HTMLInputElement).value;
                                        const v = customMappingValue;
                                        if (k && v) {
                                            onMappingUpdate('custom', k, v);
                                            (document.getElementById('custom-key') as HTMLInputElement).value = '';
                                            setCustomMappingValue('');
                                        }
                                    }}
                                >
                                    <Plus size={14} />
                                    {t('common.add')}
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    );
}
