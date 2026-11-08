import React, { useState, useEffect } from 'react';
import { createPortal } from 'react-dom';
import { Tag, Plus, X, ArrowUp, ArrowDown, Check, Loader2 } from 'lucide-react';
import { parseTags, formatTags, reorderTags } from '../../utils/tagUtils';

export interface AccountTagEditorModalProps {
    isOpen: boolean;
    onClose: () => void;
    accountName: string;
    initialLabel?: string;
    onSave: (formattedLabel: string) => Promise<void>;
}

const PRESET_TAGS = ['主力', '备用', '高配', '测试', 'Claude', 'Gemini'];

export default function AccountTagEditorModal({
    isOpen,
    onClose,
    accountName,
    initialLabel,
    onSave,
}: AccountTagEditorModalProps) {
    const [tags, setTags] = useState<string[]>([]);
    const [inputValue, setInputValue] = useState('');
    const [errorMsg, setErrorMsg] = useState<string | null>(null);
    const [isSaving, setIsSaving] = useState(false);

    useEffect(() => {
        if (isOpen) {
            setTags(parseTags(initialLabel));
            setInputValue('');
            setErrorMsg(null);
            setIsSaving(false);
        }
    }, [isOpen, initialLabel]);

    // Handle escape key
    useEffect(() => {
        if (!isOpen) return;
        const handleKeyDown = (e: KeyboardEvent) => {
            if (e.key === 'Escape') {
                onClose();
            }
        };
        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [isOpen, onClose]);

    if (!isOpen) return null;

    const handleAddTag = (rawTag: string) => {
        setErrorMsg(null);
        const trimmed = rawTag.trim();
        if (!trimmed) return;

        if (trimmed.length > 15) {
            setErrorMsg('标签长度不能超过 15 个字符');
            return;
        }

        if (tags.length >= 8) {
            setErrorMsg('最多只能添加 8 个标签');
            return;
        }

        if (tags.includes(trimmed)) {
            setErrorMsg('该标签已存在');
            return;
        }

        setTags([...tags, trimmed]);
        setInputValue('');
    };

    const handleKeyDownInput = (e: React.KeyboardEvent<HTMLInputElement>) => {
        if (e.key === 'Enter') {
            e.preventDefault();
            handleAddTag(inputValue);
        }
    };

    const handleRemoveTag = (index: number) => {
        setErrorMsg(null);
        setTags(tags.filter((_, i) => i !== index));
    };

    const handleMoveUp = (index: number) => {
        if (index <= 0) return;
        setErrorMsg(null);
        setTags(reorderTags(tags, index, index - 1));
    };

    const handleMoveDown = (index: number) => {
        if (index >= tags.length - 1) return;
        setErrorMsg(null);
        setTags(reorderTags(tags, index, index + 1));
    };

    const handleAddPreset = (preset: string) => {
        if (tags.includes(preset)) return;
        if (tags.length >= 8) {
            setErrorMsg('最多只能添加 8 个标签');
            return;
        }
        setErrorMsg(null);
        setTags([...tags, preset]);
    };

    const handleSave = async () => {
        try {
            setIsSaving(true);
            setErrorMsg(null);
            const formatted = formatTags(tags);
            await onSave(formatted);
            onClose();
        } catch (err: any) {
            setErrorMsg(err?.message || '保存标签失败');
        } finally {
            setIsSaving(false);
        }
    };

    return createPortal(
        <div className="modal modal-open z-[100]">
            <div data-tauri-drag-region className="fixed top-0 left-0 right-0 h-8 z-[110]" />

            <div className="modal-box relative max-w-md bg-white dark:bg-base-100 shadow-2xl rounded-2xl p-0 overflow-hidden transform transition-all animate-in fade-in zoom-in-95 duration-200">
                {/* Header */}
                <div className="flex items-center justify-between px-6 pt-6 pb-4 border-b border-gray-100 dark:border-base-200">
                    <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-blue-50 dark:bg-blue-900/20 text-blue-500 flex items-center justify-center">
                            <Tag className="w-5 h-5" />
                        </div>
                        <div>
                            <h3 className="text-lg font-bold text-gray-900 dark:text-base-content">管理账号标签</h3>
                            <p className="text-xs text-gray-500 dark:text-gray-400 truncate max-w-[240px]">
                                {accountName}
                            </p>
                        </div>
                    </div>
                    <button
                        onClick={onClose}
                        className="btn btn-ghost btn-sm btn-circle text-gray-400 hover:text-gray-600 dark:hover:text-gray-200"
                    >
                        <X className="w-5 h-5" />
                    </button>
                </div>

                {/* Body */}
                <div className="p-6 space-y-5">
                    {/* Input section */}
                    <div className="space-y-2">
                        <label className="text-xs font-semibold text-gray-600 dark:text-gray-300 uppercase tracking-wider">
                            添加新标签 ({tags.length}/8)
                        </label>
                        <div className="flex gap-2">
                            <input
                                type="text"
                                placeholder="输入标签名（最多 15 字符）..."
                                value={inputValue}
                                onChange={(e) => setInputValue(e.target.value)}
                                onKeyDown={handleKeyDownInput}
                                maxLength={15}
                                className="input input-bordered flex-1 bg-gray-50 dark:bg-base-200 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                            />
                            <button
                                type="button"
                                onClick={() => handleAddTag(inputValue)}
                                className="btn btn-primary bg-blue-500 hover:bg-blue-600 border-none text-white px-4 gap-1.5"
                            >
                                <Plus className="w-4 h-4" />
                                添加
                            </button>
                        </div>
                        {errorMsg && (
                            <p className="text-xs text-red-500 mt-1 font-medium">{errorMsg}</p>
                        )}
                    </div>

                    {/* Presets */}
                    <div className="space-y-2">
                        <span className="text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                            常用快捷标签
                        </span>
                        <div className="flex flex-wrap gap-1.5">
                            {PRESET_TAGS.map((preset) => {
                                const exists = tags.includes(preset);
                                return (
                                    <button
                                        key={preset}
                                        type="button"
                                        onClick={() => handleAddPreset(preset)}
                                        disabled={exists}
                                        className={`px-2.5 py-1 text-xs font-medium rounded-lg transition-all ${
                                            exists
                                                ? 'bg-gray-100 dark:bg-base-200 text-gray-400 dark:text-gray-500 cursor-not-allowed'
                                                : 'bg-blue-50 dark:bg-blue-900/20 text-blue-600 dark:text-blue-400 hover:bg-blue-100 dark:hover:bg-blue-900/40 border border-blue-200 dark:border-blue-800/50'
                                        }`}
                                    >
                                        {preset} {exists && '✓'}
                                    </button>
                                );
                            })}
                        </div>
                    </div>

                    {/* Tag list */}
                    <div className="space-y-2">
                        <span className="text-xs font-semibold text-gray-600 dark:text-gray-300 uppercase tracking-wider">
                            当前标签列表
                        </span>
                        {tags.length === 0 ? (
                            <div className="text-center py-8 border-2 border-dashed border-gray-200 dark:border-base-300 rounded-xl text-gray-400 dark:text-gray-500 text-sm">
                                暂无标签，请在上方输入或点击快捷标签添加
                            </div>
                        ) : (
                            <div className="max-h-48 overflow-y-auto space-y-2 pr-1">
                                {tags.map((tag, index) => (
                                    <div
                                        key={`${tag}-${index}`}
                                        className="flex items-center justify-between px-3 py-2 bg-gray-50 dark:bg-base-200 rounded-xl border border-gray-100 dark:border-base-300 group"
                                    >
                                        <div className="flex items-center gap-2">
                                            <span className="w-5 h-5 rounded-full bg-blue-100 dark:bg-blue-900/40 text-blue-600 dark:text-blue-400 text-xs font-bold flex items-center justify-center">
                                                {index + 1}
                                            </span>
                                            <span className="text-sm font-medium text-gray-800 dark:text-base-content">
                                                {tag}
                                            </span>
                                        </div>
                                        <div className="flex items-center gap-1">
                                            <button
                                                type="button"
                                                onClick={() => handleMoveUp(index)}
                                                disabled={index === 0}
                                                title="上移"
                                                className="p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 disabled:opacity-30 disabled:cursor-not-allowed rounded"
                                            >
                                                <ArrowUp className="w-4 h-4" />
                                            </button>
                                            <button
                                                type="button"
                                                onClick={() => handleMoveDown(index)}
                                                disabled={index === tags.length - 1}
                                                title="下移"
                                                className="p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 disabled:opacity-30 disabled:cursor-not-allowed rounded"
                                            >
                                                <ArrowDown className="w-4 h-4" />
                                            </button>
                                            <button
                                                type="button"
                                                onClick={() => handleRemoveTag(index)}
                                                title="删除"
                                                className="p-1 text-red-400 hover:text-red-600 rounded ml-1"
                                            >
                                                <X className="w-4 h-4" />
                                            </button>
                                        </div>
                                    </div>
                                ))}
                            </div>
                        )}
                    </div>
                </div>

                {/* Footer */}
                <div className="flex items-center justify-end gap-3 px-6 py-4 bg-gray-50 dark:bg-base-200/50 border-t border-gray-100 dark:border-base-200">
                    <button
                        type="button"
                        disabled={isSaving}
                        onClick={onClose}
                        className="px-4 py-2 bg-gray-100 dark:bg-base-200 text-gray-700 dark:text-gray-300 font-medium rounded-xl hover:bg-gray-200 dark:hover:bg-base-300 transition-colors text-sm disabled:opacity-50"
                    >
                        取消
                    </button>
                    <button
                        type="button"
                        disabled={isSaving}
                        onClick={handleSave}
                        className="px-5 py-2 bg-blue-500 hover:bg-blue-600 text-white font-medium rounded-xl shadow-md transition-all text-sm flex items-center gap-2 disabled:opacity-50"
                    >
                        {isSaving && <Loader2 className="w-4 h-4 animate-spin" />}
                        <Check className="w-4 h-4" />
                        保存
                    </button>
                </div>
            </div>

            <div className="modal-backdrop bg-black/40 backdrop-blur-sm fixed inset-0 z-[-1]" onClick={onClose} />
        </div>,
        document.body
    );
}
