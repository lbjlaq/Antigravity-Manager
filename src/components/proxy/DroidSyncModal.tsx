import { useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { RefreshCw, X, Bot } from 'lucide-react';
import {
    DndContext, closestCenter, KeyboardSensor, PointerSensor,
    useSensor, useSensors, type DragEndEvent,
} from '@dnd-kit/core';
import {
    arrayMove, SortableContext, sortableKeyboardCoordinates, verticalListSortingStrategy,
} from '@dnd-kit/sortable';
import { cn } from '../../utils/cn';
import { request as invoke } from '../../utils/request';
import { showToast } from '../common/ToastContainer';
import { useProxyModels } from '../../hooks/useProxyModels';
import { SortableModelItem, type PreviewModelEntry } from './SortableModelItem';

interface DroidSyncModalProps {
    proxyUrl: string;
    apiKey: string;
    getFormattedProxyUrl: (app: 'Claude' | 'Codex' | 'Gemini' | 'OpenCode' | 'Droid') => string;
    onClose: () => void;
    onSyncDone: () => void;
}

function buildDroidModel(modelId: string, modelName: string) {
    const isClaude = modelId.startsWith('claude-');
    const isGemini = modelId.startsWith('gemini-');
    const isThinking = modelId.includes('thinking');
    if (isClaude) {
        return {
            model: modelId,
            displayName: `AG-${modelName}`,
            provider: 'anthropic',
            noImageSupport: false,
            maxOutputTokens: 64000,
            ...(isThinking ? { extraArgs: { thinking: { type: 'enabled', budget_tokens: 32000 } } } : {}),
        };
    }
    // Gemini 与其他视觉模型全面开启图像支持 (noImageSupport: false)
    const supportsImage = isGemini || modelId.includes('gpt-4') || modelId.includes('image');
    return {
        model: modelId,
        displayName: `AG-${modelName}`,
        provider: 'generic-chat-completion-api',
        noImageSupport: !supportsImage,
    };
}

export function DroidSyncModal({ apiKey, getFormattedProxyUrl, onClose, onSyncDone }: DroidSyncModalProps) {
    const { t } = useTranslation();
    const { models: antigravityModels } = useProxyModels();
    const [selectedModels, setSelectedModels] = useState<Set<string>>(new Set());
    const [previewModels, setPreviewModels] = useState<PreviewModelEntry[]>([]);
    const [expanded, setExpanded] = useState<Set<string>>(new Set());
    const [syncing, setSyncing] = useState(false);
    const [configLoaded, setConfigLoaded] = useState(false);
    const [currentConfig, setCurrentConfig] = useState<Record<string, unknown> | null>(null);

    const sensors = useSensors(
        useSensor(PointerSensor, { activationConstraint: { distance: 3 } }),
        useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
    );

    const rebuildPreview = useCallback((selectedIds: Set<string>, existingConfig: Record<string, unknown> | null) => {
        const base = getFormattedProxyUrl('Droid').replace(/\/+$/, '');
        const existing = existingConfig ?? {};
        const existingModels = Array.isArray((existing as Record<string, unknown>).customModels)
            ? [...(existing as Record<string, unknown>).customModels as Record<string, unknown>[]]
            : [];

        const existingEntries: PreviewModelEntry[] = existingModels.map((m, i) => ({
            ...(m as PreviewModelEntry),
            _uid: `existing-${i}`,
            isAg: ((m as Record<string, unknown>).id as string || '').startsWith('custom:AG-'),
            index: i,
        }));

        const existingAgModels = new Set(existingEntries.filter(e => e.isAg).map(e => e.model));
        const selected = antigravityModels.filter(m => selectedIds.has(m.id));
        const newEntries: PreviewModelEntry[] = selected
            .filter(m => {
                const cfg = buildDroidModel(m.id, m.name);
                return !existingAgModels.has(cfg.model);
            })
            .map((m, i) => {
                const cfg = buildDroidModel(m.id, m.name);
                const actualBase = cfg.provider === 'generic-chat-completion-api'
                    ? (base.endsWith('/v1') ? base : `${base}/v1`) : base;
                const entry: PreviewModelEntry = {
                    _uid: `new-${i}`,
                    model: cfg.model,
                    id: `custom:${cfg.displayName.replace(/\s/g, '-')}`,
                    index: 0,
                    baseUrl: actualBase,
                    apiKey: apiKey,
                    displayName: cfg.displayName,
                    noImageSupport: cfg.noImageSupport ?? false,
                    provider: cfg.provider,
                    isAg: true,
                };
                if ('maxOutputTokens' in cfg) entry.maxOutputTokens = cfg.maxOutputTokens;
                if ('extraArgs' in cfg) entry.extraArgs = cfg.extraArgs;
                return entry;
            });

        const merged = [...existingEntries, ...newEntries];
        merged.forEach((m, i) => {
            m.index = i;
            if (m.isAg) m.id = `custom:${m.displayName.replace(/\s/g, '-')}-${i}`;
        });
        setPreviewModels(merged);
    }, [antigravityModels, apiKey, getFormattedProxyUrl]);

    // 初始加载 settings.json
    if (!configLoaded) {
        setConfigLoaded(true);
        invoke<string>('get_droid_config_content', {})
            .then(content => {
                const parsed = JSON.parse(content);
                setCurrentConfig(parsed);
                rebuildPreview(new Set(), parsed);
            })
            .catch(() => rebuildPreview(new Set(), null));
    }

    const reindexId = (id: string, newIdx: number) => id.replace(/-\d+$/, `-${newIdx}`);

    const allSelected = antigravityModels.length > 0 && antigravityModels.every(m => selectedModels.has(m.id));
    const toggleAll = () => {
        const next = allSelected ? new Set<string>() : new Set(antigravityModels.map(m => m.id));
        setSelectedModels(next);
        rebuildPreview(next, currentConfig);
    };

    const toggleModel = (modelListId: string) => {
        const next = new Set(selectedModels);
        const adding = !next.has(modelListId);
        if (adding) next.add(modelListId); else next.delete(modelListId);
        setSelectedModels(next);

        if (adding) {
            const m = antigravityModels.find(x => x.id === modelListId);
            if (!m) return;
            const base = getFormattedProxyUrl('Droid').replace(/\/+$/, '');
            const cfg = buildDroidModel(m.id, m.name);
            const actualBase = cfg.provider === 'generic-chat-completion-api'
                ? (base.endsWith('/v1') ? base : `${base}/v1`) : base;
            const newIdx = previewModels.length;
            const entry: PreviewModelEntry = {
                _uid: `new-${Date.now()}-${m.id}`,
                model: cfg.model,
                id: `custom:${cfg.displayName.replace(/\s/g, '-')}-${newIdx}`,
                index: newIdx,
                baseUrl: actualBase,
                apiKey: apiKey,
                displayName: cfg.displayName,
                noImageSupport: cfg.noImageSupport ?? false,
                provider: cfg.provider,
                isAg: true,
            };
            if ('maxOutputTokens' in cfg) entry.maxOutputTokens = cfg.maxOutputTokens;
            if ('extraArgs' in cfg) entry.extraArgs = cfg.extraArgs;
            setPreviewModels([...previewModels, entry]);
        } else {
            const m = antigravityModels.find(x => x.id === modelListId);
            if (!m) return;
            const cfg = buildDroidModel(m.id, m.name);
            setPreviewModels(
                previewModels.filter(e => !(e.isAg && e.model === cfg.model)).map((m, i) => ({
                    ...m, index: i, id: reindexId(m.id, i),
                }))
            );
        }
    };

    const handleDragEnd = (event: DragEndEvent) => {
        const { active, over } = event;
        if (!over || active.id === over.id) return;
        const oldIdx = previewModels.findIndex(m => m._uid === active.id);
        const newIdx = previewModels.findIndex(m => m._uid === over.id);
        if (oldIdx < 0 || newIdx < 0) return;
        setPreviewModels(arrayMove([...previewModels], oldIdx, newIdx).map((m, i) => ({
            ...m, index: i, id: reindexId(m.id, i),
        })));
    };

    const handleRemoveModel = (uid: string) => {
        setPreviewModels(
            previewModels.filter(m => m._uid !== uid).map((m, i) => ({
                ...m, index: i, id: reindexId(m.id, i),
            }))
        );
    };

    const executeDroidSync = async () => {
        if (!previewModels.some(m => m.isAg)) {
            showToast(t('proxy.droid_sync.toast.no_models_selected', { defaultValue: '请至少选择一个模型' }), 'error');
            return;
        }
        setSyncing(true);
        try {
            const customModels = previewModels.map(m => {
                const { _uid, isAg, ...rest } = m;
                return rest;
            });
            const added = await invoke<number>('execute_droid_sync', { customModels });
            showToast(t('proxy.droid_sync.toast.sync_success_count', { count: added, defaultValue: `已添加 ${added} 个模型到 Droid` }), 'success');
            onSyncDone();
            onClose();
        } catch (error: any) {
            showToast(t('proxy.droid_sync.toast.sync_error', { error: error.toString(), defaultValue: `同步失败: ${error.toString()}` }), 'error');
        } finally {
            setSyncing(false);
        }
    };

    const groups = [...new Set(antigravityModels.map(m => m.group))];
    const existingCount = previewModels.filter(m => !m.isAg).length;
    const agCount = previewModels.filter(m => m.isAg).length;

    return (
        <div className="fixed inset-0 z-[300] flex items-center justify-center p-4 bg-black/50 backdrop-blur-sm animate-in fade-in duration-200">
            <div className="bg-white dark:bg-base-100 rounded-2xl shadow-2xl border border-gray-200 dark:border-base-300 w-full max-w-2xl max-h-[85vh] overflow-hidden animate-in zoom-in-95 duration-200 flex flex-col">
                {/* Header */}
                <div className="px-5 pt-4 pb-3 shrink-0">
                    <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2.5">
                            <div className="p-2 bg-orange-50 dark:bg-orange-900/20 rounded-lg">
                                <Bot size={18} className="text-orange-500" />
                            </div>
                            <div>
                                <h3 className="text-sm font-bold text-gray-900 dark:text-base-content">
                                    {t('proxy.droid_sync.modal_title', { defaultValue: '添加模型到 Droid' })}
                                </h3>
                                <p className="text-[10px] text-gray-400 mt-0.5">~/.factory/settings.json</p>
                            </div>
                        </div>
                        <button onClick={onClose} className="p-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-base-300 transition-colors">
                            <X size={16} className="text-gray-400" />
                        </button>
                    </div>
                </div>

                {/* 模型选择区 */}
                <div className="px-5 pb-3 shrink-0 border-b border-gray-100 dark:border-base-200">
                    <div className="flex items-center justify-between mb-2">
                        <span className="text-[10px] font-bold text-gray-400 uppercase tracking-wider">
                            {t('proxy.droid_sync.select_models', { defaultValue: '选择要添加的模型' })}
                            <span className="ml-2 text-gray-300">{selectedModels.size}/{antigravityModels.length}</span>
                        </span>
                        <button onClick={toggleAll} className="text-[10px] text-blue-500 hover:text-blue-600 font-medium transition-colors">
                            {allSelected ? t('common.deselect_all', { defaultValue: '取消全选' }) : t('common.select_all', { defaultValue: '全选' })}
                        </button>
                    </div>
                    <div className="space-y-2 max-h-[25vh] overflow-auto">
                        {groups.map(group => {
                            const groupModels = antigravityModels.filter(m => m.group === group);
                            return (
                                <div key={group}>
                                    <div className="text-[9px] font-bold text-gray-400 uppercase tracking-widest mb-1">{group}</div>
                                    <div className="flex flex-wrap gap-1.5">
                                        {groupModels.map(m => {
                                            const selected = selectedModels.has(m.id);
                                            return (
                                                <button
                                                    key={m.id}
                                                    onClick={() => toggleModel(m.id)}
                                                    className={cn(
                                                        "px-2.5 py-1 rounded-md text-[11px] font-medium transition-all duration-150 border",
                                                        selected
                                                            ? "bg-orange-500 text-white border-orange-500"
                                                            : "bg-gray-50 dark:bg-base-200 text-gray-500 dark:text-gray-400 border-gray-200 dark:border-base-300 hover:border-orange-300"
                                                    )}
                                                >
                                                    {m.name}
                                                </button>
                                            );
                                        })}
                                    </div>
                                </div>
                            );
                        })}
                    </div>
                </div>

                {/* Preview 主体区 */}
                <div className="flex-1 min-h-0 flex flex-col">
                    <div className="px-5 py-2 flex items-center justify-between shrink-0">
                        <span className="text-[10px] font-bold text-gray-400 uppercase tracking-wider">
                            customModels Preview
                            <span className="ml-2 font-normal">
                                {existingCount > 0 && <span className="text-gray-300">{existingCount} existing</span>}
                                {existingCount > 0 && agCount > 0 && <span className="text-gray-200 mx-1">+</span>}
                                {agCount > 0 && <span className="text-orange-400">{agCount} new</span>}
                            </span>
                        </span>
                        <span className="text-[9px] font-mono text-gray-300">{previewModels.length} total</span>
                    </div>
                    <div className="px-4 pb-3 overflow-auto flex-1">
                        <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
                            <SortableContext items={previewModels.map(m => m._uid)} strategy={verticalListSortingStrategy}>
                                <div className="space-y-1.5">
                                    {previewModels.map(entry => (
                                        <SortableModelItem
                                            key={entry._uid}
                                            entry={entry}
                                            collapsed={!expanded.has(entry._uid)}
                                            onToggle={() => {
                                                const next = new Set(expanded);
                                                if (next.has(entry._uid)) next.delete(entry._uid); else next.add(entry._uid);
                                                setExpanded(next);
                                            }}
                                            onRemove={() => handleRemoveModel(entry._uid)}
                                        />
                                    ))}
                                </div>
                            </SortableContext>
                        </DndContext>
                        {previewModels.length === 0 && (
                            <div className="text-center text-xs text-gray-400 py-8">
                                {t('proxy.droid_sync.no_models', { defaultValue: '请在上方选择要添加的模型' })}
                            </div>
                        )}
                    </div>
                </div>

                {/* Footer */}
                <div className="px-5 py-3 border-t border-gray-100 dark:border-base-200 flex items-center justify-end gap-2 shrink-0">
                    <button className="px-3 py-1.5 text-xs text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 rounded-lg hover:bg-gray-100 dark:hover:bg-base-300 transition-colors" onClick={onClose}>
                        {t('common.cancel', { defaultValue: '取消' })}
                    </button>
                    <button
                        className={cn(
                            "px-4 py-1.5 text-xs font-bold rounded-lg transition-all flex items-center gap-1.5",
                            previewModels.some(m => m.isAg)
                                ? "bg-orange-500 hover:bg-orange-600 active:bg-orange-700 text-white shadow-sm"
                                : "bg-gray-200 dark:bg-gray-700 text-gray-400 cursor-not-allowed"
                        )}
                        disabled={!previewModels.some(m => m.isAg) || syncing}
                        onClick={executeDroidSync}
                    >
                        <RefreshCw size={12} className={syncing ? 'animate-spin' : ''} />
                        {t('proxy.droid_sync.btn_confirm_sync', { defaultValue: '写入配置' })}
                    </button>
                </div>
            </div>
        </div>
    );
}
