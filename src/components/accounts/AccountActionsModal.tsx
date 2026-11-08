import React, { useState, useEffect } from 'react';
import { createPortal } from 'react-dom';
import {
    X,
    Repeat2,
    Blocks,
    Terminal,
    ArrowRightLeft,
    Tag,
    Sparkles,
    Smartphone,
    Info,
    Download,
    Trash2,
    Loader2,
    ChevronLeft,
    Plus,
    ArrowUp,
    ArrowDown,
    Check
} from 'lucide-react';
import { Account } from '../../types/account';
import { useTranslation } from 'react-i18next';
import { parseTags, formatTags, reorderTags } from '../../utils/tagUtils';

export interface AccountActionsModalProps {
    isOpen: boolean;
    onClose: () => void;
    account: Account;
    onSwitch: (targetIde: 'classic' | 'ide' | 'agy' | 'acp') => void;
    onWarmup?: () => void;
    onViewDevice: () => void;
    onViewDetails: () => void;
    onExport: () => void;
    onUpdateLabel?: (newLabel: string) => Promise<void> | void;
    onOpenTagEditor?: () => void;
    onDelete: () => void;
    isSwitching?: boolean;
    isRefreshing?: boolean;
}

const PRESET_TAGS = ['主力', '备用', '高配', '测试', 'Claude', 'Gemini'];

export default function AccountActionsModal({
    isOpen,
    onClose,
    account,
    onSwitch,
    onWarmup,
    onViewDevice,
    onViewDetails,
    onExport,
    onUpdateLabel,
    onOpenTagEditor,
    onDelete,
    isSwitching = false,
    isRefreshing = false,
}: AccountActionsModalProps) {
    const { t } = useTranslation();
    const [currentView, setCurrentView] = useState<'menu' | 'tags'>('menu');

    // Tags view state
    const [tags, setTags] = useState<string[]>([]);
    const [inputValue, setInputValue] = useState('');
    const [tagErrorMsg, setTagErrorMsg] = useState<string | null>(null);
    const [isSavingTags, setIsSavingTags] = useState(false);

    useEffect(() => {
        if (isOpen) {
            setCurrentView('menu');
            setTags(parseTags(account.custom_label));
            setInputValue('');
            setTagErrorMsg(null);
            setIsSavingTags(false);
        }
    }, [isOpen, account.custom_label]);

    useEffect(() => {
        if (!isOpen) return;
        const handleKeyDown = (e: KeyboardEvent) => {
            if (e.key === 'Escape') {
                if (currentView === 'tags') {
                    setCurrentView('menu');
                } else {
                    onClose();
                }
            }
        };
        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [isOpen, currentView, onClose]);

    if (!isOpen) return null;

    const isSwitchDisabled = isSwitching || Boolean(account.disabled) || Boolean(account.banned);

    const handleAddTag = (rawTag: string) => {
        setTagErrorMsg(null);
        const parsedIncoming = parseTags(rawTag);
        if (parsedIncoming.length === 0) return;

        for (const tag of parsedIncoming) {
            if (tag.length > 15) {
                setTagErrorMsg(t('accounts.tag_editor.error_too_long', '标签长度不能超过 15 个字符'));
                return;
            }
        }

        let updated = [...tags];
        for (const tag of parsedIncoming) {
            if (updated.length >= 8) {
                setTagErrorMsg(t('accounts.tag_editor.error_max_limit', '最多只能添加 8 个标签'));
                break;
            }
            if (updated.some(t => t.toLowerCase() === tag.toLowerCase())) {
                setTagErrorMsg(t('accounts.tag_editor.error_duplicate', '该标签已存在'));
                continue;
            }
            updated.push(tag);
        }

        setTags(updated);
        setInputValue('');
    };

    const handleKeyDownInput = (e: React.KeyboardEvent<HTMLInputElement>) => {
        if (e.key === 'Enter') {
            e.preventDefault();
            handleAddTag(inputValue);
        }
    };

    const handleRemoveTag = (index: number) => {
        setTagErrorMsg(null);
        setTags(tags.filter((_, i) => i !== index));
    };

    const handleMoveUp = (index: number) => {
        if (index <= 0) return;
        setTagErrorMsg(null);
        setTags(reorderTags(tags, index, index - 1));
    };

    const handleMoveDown = (index: number) => {
        if (index >= tags.length - 1) return;
        setTagErrorMsg(null);
        setTags(reorderTags(tags, index, index + 1));
    };

    const handleAddPreset = (preset: string) => {
        if (tags.some(t => t.toLowerCase() === preset.toLowerCase())) return;
        if (tags.length >= 8) {
            setTagErrorMsg(t('accounts.tag_editor.error_max_limit', '最多只能添加 8 个标签'));
            return;
        }
        setTagErrorMsg(null);
        setTags([...tags, preset]);
    };

    const handleSaveTags = async () => {
        try {
            setIsSavingTags(true);
            setTagErrorMsg(null);
            const formatted = formatTags(tags);
            if (onUpdateLabel) {
                await onUpdateLabel(formatted);
            }
            setCurrentView('menu');
        } catch (err: any) {
            setTagErrorMsg(err?.message || t('accounts.tag_editor.save_failed', '保存标签失败'));
        } finally {
            setIsSavingTags(false);
        }
    };

    const accountDisplayTags = parseTags(account.custom_label);

    return createPortal(
        <div className="modal modal-open z-[130] bg-black/50 backdrop-blur-sm transition-all">
            <div className="modal-box w-11/12 max-w-2xl bg-white dark:bg-base-100 rounded-2xl shadow-2xl p-0 overflow-hidden border border-gray-100 dark:border-base-200">
                {currentView === 'menu' ? (
                    <>
                        {/* Header */}
                        <div className="flex items-center justify-between px-6 py-4 border-b border-gray-100 dark:border-base-200 bg-gray-50/50 dark:bg-base-200/40">
                            <div>
                                <h3 className="font-bold text-base text-gray-900 dark:text-base-content flex items-center gap-2">
                                    <span>{t('accounts.actions_modal.title')}</span>
                                </h3>
                                <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5 truncate max-w-md">
                                    {account.email}
                                </p>
                            </div>
                            <button
                                onClick={onClose}
                                className="btn btn-sm btn-circle btn-ghost text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
                            >
                                <X size={18} />
                            </button>
                        </div>

                        {/* Body Content */}
                        <div className="p-6 space-y-6 max-h-[75vh] overflow-y-auto">
                            {/* Section 1: Client Quick Switching */}
                            <div>
                                <h4 className="text-xs font-semibold uppercase tracking-wider text-gray-400 dark:text-gray-500 mb-3">
                                    {t('accounts.actions_modal.clients_section')}
                                </h4>
                                <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
                                    {/* IDE */}
                                    <button
                                        disabled={isSwitchDisabled}
                                        onClick={() => onSwitch('ide')}
                                        className={`flex items-start gap-3 p-3 rounded-xl border border-gray-200 dark:border-base-200 transition-all text-left group ${
                                            isSwitchDisabled
                                                ? 'opacity-50 cursor-not-allowed bg-gray-50/50 dark:bg-base-200/30'
                                                : 'hover:border-blue-500 dark:hover:border-blue-500 hover:bg-blue-50/40 dark:hover:bg-blue-500/10'
                                        }`}
                                    >
                                        <div className="p-2 rounded-lg bg-blue-50 text-blue-600 dark:bg-blue-500/15 dark:text-blue-400 group-hover:scale-105 transition-transform flex-shrink-0">
                                            <Repeat2 size={18} />
                                        </div>
                                        <div className="flex-1 min-w-0">
                                            <div className="text-xs font-semibold text-gray-900 dark:text-base-content whitespace-normal break-words leading-snug">
                                                {t('accounts.actions_modal.clients.ide')}
                                            </div>
                                            <div className="text-[11px] text-gray-500 dark:text-gray-400 mt-0.5 whitespace-normal break-words leading-tight">
                                                {t('accounts.actions_modal.clients.ide_desc')}
                                            </div>
                                        </div>
                                    </button>

                                    {/* ACP / JetBrains */}
                                    <button
                                        disabled={isSwitchDisabled}
                                        onClick={() => onSwitch('acp')}
                                        className={`flex items-start gap-3 p-3 rounded-xl border border-gray-200 dark:border-base-200 transition-all text-left group ${
                                            isSwitchDisabled
                                                ? 'opacity-50 cursor-not-allowed bg-gray-50/50 dark:bg-base-200/30'
                                                : 'hover:border-purple-500 dark:hover:border-purple-500 hover:bg-purple-50/40 dark:hover:bg-purple-500/10'
                                        }`}
                                    >
                                        <div className="p-2 rounded-lg bg-purple-50 text-purple-600 dark:bg-purple-500/15 dark:text-purple-400 group-hover:scale-105 transition-transform flex-shrink-0">
                                            <Blocks size={18} />
                                        </div>
                                        <div className="flex-1 min-w-0">
                                            <div className="text-xs font-semibold text-gray-900 dark:text-base-content whitespace-normal break-words leading-snug">
                                                {t('accounts.actions_modal.clients.acp')}
                                            </div>
                                            <div className="text-[11px] text-gray-500 dark:text-gray-400 mt-0.5 whitespace-normal break-words leading-tight">
                                                {t('accounts.actions_modal.clients.acp_desc')}
                                            </div>
                                        </div>
                                    </button>

                                    {/* CLI (agy) */}
                                    <button
                                        disabled={isSwitchDisabled}
                                        onClick={() => onSwitch('agy')}
                                        className={`flex items-start gap-3 p-3 rounded-xl border border-gray-200 dark:border-base-200 transition-all text-left group ${
                                            isSwitchDisabled
                                                ? 'opacity-50 cursor-not-allowed bg-gray-50/50 dark:bg-base-200/30'
                                                : 'hover:border-emerald-500 dark:hover:border-emerald-500 hover:bg-emerald-50/40 dark:hover:bg-emerald-500/10'
                                        }`}
                                    >
                                        <div className="p-2 rounded-lg bg-emerald-50 text-emerald-600 dark:bg-emerald-500/15 dark:text-emerald-400 group-hover:scale-105 transition-transform flex-shrink-0">
                                            <Terminal size={18} />
                                        </div>
                                        <div className="flex-1 min-w-0">
                                            <div className="text-xs font-semibold text-gray-900 dark:text-base-content whitespace-normal break-words leading-snug">
                                                {t('accounts.actions_modal.clients.agy')}
                                            </div>
                                            <div className="text-[11px] text-gray-500 dark:text-gray-400 mt-0.5 whitespace-normal break-words leading-tight">
                                                {t('accounts.actions_modal.clients.agy_desc')}
                                            </div>
                                        </div>
                                    </button>

                                    {/* Classic */}
                                    <button
                                        disabled={isSwitchDisabled}
                                        onClick={() => onSwitch('classic')}
                                        className={`flex items-start gap-3 p-3 rounded-xl border border-gray-200 dark:border-base-200 transition-all text-left group ${
                                            isSwitchDisabled
                                                ? 'opacity-50 cursor-not-allowed bg-gray-50/50 dark:bg-base-200/30'
                                                : 'hover:border-amber-500 dark:hover:border-amber-500 hover:bg-amber-50/40 dark:hover:bg-amber-500/10'
                                        }`}
                                    >
                                        <div className="p-2 rounded-lg bg-amber-50 text-amber-600 dark:bg-amber-500/15 dark:text-amber-400 group-hover:scale-105 transition-transform flex-shrink-0">
                                            <ArrowRightLeft size={18} />
                                        </div>
                                        <div className="flex-1 min-w-0">
                                            <div className="text-xs font-semibold text-gray-900 dark:text-base-content whitespace-normal break-words leading-snug">
                                                {t('accounts.actions_modal.clients.classic')}
                                            </div>
                                            <div className="text-[11px] text-gray-500 dark:text-gray-400 mt-0.5 whitespace-normal break-words leading-tight">
                                                {t('accounts.actions_modal.clients.classic_desc')}
                                            </div>
                                        </div>
                                    </button>
                                </div>
                            </div>

                            {/* Section 2: Maintenance & Tools */}
                            <div>
                                <h4 className="text-xs font-semibold uppercase tracking-wider text-gray-400 dark:text-gray-500 mb-3">
                                    {t('accounts.actions_modal.maintenance_section')}
                                </h4>
                                <div className="space-y-2">
                                    {/* Tags Management */}
                                    <button
                                        onClick={() => {
                                            if (onOpenTagEditor) {
                                                onOpenTagEditor();
                                            }
                                            setCurrentView('tags');
                                        }}
                                        className="w-full flex items-center justify-between p-3 rounded-xl border border-gray-200 dark:border-base-200 hover:border-indigo-500 dark:hover:border-indigo-500 hover:bg-indigo-50/40 dark:hover:bg-indigo-500/10 transition-all text-left group"
                                    >
                                        <div className="flex items-center gap-3 min-w-0">
                                            <div className="p-2 rounded-lg bg-indigo-50 text-indigo-600 dark:bg-indigo-500/15 dark:text-indigo-400 flex-shrink-0">
                                                <Tag size={18} />
                                            </div>
                                            <div>
                                                <div className="text-xs font-semibold text-gray-900 dark:text-base-content">
                                                    {t('accounts.actions_modal.tools.tags')}
                                                </div>
                                                <div className="text-[11px] text-gray-500 dark:text-gray-400">
                                                    {t('accounts.actions_modal.tools.tags_desc')}
                                                </div>
                                            </div>
                                        </div>
                                        <div className="flex items-center gap-1.5 flex-wrap justify-end max-w-[200px]">
                                            {accountDisplayTags.length > 0 ? (
                                                accountDisplayTags.map((tag, idx) => (
                                                    <span
                                                        key={idx}
                                                        className="px-2 py-0.5 text-[10px] bg-indigo-50 dark:bg-indigo-900/30 text-indigo-600 dark:text-indigo-300 rounded-md font-medium border border-indigo-200/50 dark:border-indigo-800/40"
                                                    >
                                                        {tag}
                                                    </span>
                                                ))
                                            ) : (
                                                <span className="text-[11px] text-gray-400 italic">
                                                    {t('common.none')}
                                                </span>
                                            )}
                                        </div>
                                    </button>

                                    {/* Warmup */}
                                    {onWarmup && (
                                        <button
                                            disabled={isRefreshing}
                                            onClick={onWarmup}
                                            className="w-full flex items-center justify-between p-3 rounded-xl border border-gray-200 dark:border-base-200 hover:border-cyan-500 dark:hover:border-cyan-500 hover:bg-cyan-50/40 dark:hover:bg-cyan-500/10 transition-all text-left group"
                                        >
                                            <div className="flex items-center gap-3">
                                                <div className="p-2 rounded-lg bg-cyan-50 text-cyan-600 dark:bg-cyan-500/15 dark:text-cyan-400 flex-shrink-0">
                                                    {isRefreshing ? <Loader2 size={18} className="animate-spin" /> : <Sparkles size={18} />}
                                                </div>
                                                <div>
                                                    <div className="text-xs font-semibold text-gray-900 dark:text-base-content">
                                                        {t('accounts.actions_modal.tools.warmup')}
                                                    </div>
                                                    <div className="text-[11px] text-gray-500 dark:text-gray-400">
                                                        {t('accounts.actions_modal.tools.warmup_desc')}
                                                    </div>
                                                </div>
                                            </div>
                                        </button>
                                    )}

                                    {/* Device Fingerprint */}
                                    <button
                                        onClick={onViewDevice}
                                        className="w-full flex items-center justify-between p-3 rounded-xl border border-gray-200 dark:border-base-200 hover:border-teal-500 dark:hover:border-teal-500 hover:bg-teal-50/40 dark:hover:bg-teal-500/10 transition-all text-left group"
                                    >
                                        <div className="flex items-center gap-3">
                                            <div className="p-2 rounded-lg bg-teal-50 text-teal-600 dark:bg-teal-500/15 dark:text-teal-400 flex-shrink-0">
                                                <Smartphone size={18} />
                                            </div>
                                            <div>
                                                <div className="text-xs font-semibold text-gray-900 dark:text-base-content">
                                                    {t('accounts.actions_modal.tools.fingerprint')}
                                                </div>
                                                <div className="text-[11px] text-gray-500 dark:text-gray-400">
                                                    {t('accounts.actions_modal.tools.fingerprint_desc')}
                                                </div>
                                            </div>
                                        </div>
                                    </button>

                                    {/* Account Details */}
                                    <button
                                        onClick={onViewDetails}
                                        className="w-full flex items-center justify-between p-3 rounded-xl border border-gray-200 dark:border-base-200 hover:border-sky-500 dark:hover:border-sky-500 hover:bg-sky-50/40 dark:hover:bg-sky-500/10 transition-all text-left group"
                                    >
                                        <div className="flex items-center gap-3">
                                            <div className="p-2 rounded-lg bg-sky-50 text-sky-600 dark:bg-sky-500/15 dark:text-sky-400 flex-shrink-0">
                                                <Info size={18} />
                                            </div>
                                            <div>
                                                <div className="text-xs font-semibold text-gray-900 dark:text-base-content">
                                                    {t('accounts.actions_modal.tools.details')}
                                                </div>
                                                <div className="text-[11px] text-gray-500 dark:text-gray-400">
                                                    {t('accounts.actions_modal.tools.details_desc')}
                                                </div>
                                            </div>
                                        </div>
                                    </button>

                                    {/* Export Credentials */}
                                    <button
                                        onClick={onExport}
                                        className="w-full flex items-center justify-between p-3 rounded-xl border border-gray-200 dark:border-base-200 hover:border-blue-500 dark:hover:border-blue-500 hover:bg-blue-50/40 dark:hover:bg-blue-500/10 transition-all text-left group"
                                    >
                                        <div className="flex items-center gap-3">
                                            <div className="p-2 rounded-lg bg-blue-50 text-blue-600 dark:bg-blue-500/15 dark:text-blue-400 flex-shrink-0">
                                                <Download size={18} />
                                            </div>
                                            <div>
                                                <div className="text-xs font-semibold text-gray-900 dark:text-base-content">
                                                    {t('accounts.actions_modal.tools.export')}
                                                </div>
                                                <div className="text-[11px] text-gray-500 dark:text-gray-400">
                                                    {t('accounts.actions_modal.tools.export_desc')}
                                                </div>
                                            </div>
                                        </div>
                                    </button>
                                </div>
                            </div>

                            {/* Section 3: Danger Zone */}
                            <div>
                                <h4 className="text-xs font-semibold uppercase tracking-wider text-red-500 dark:text-red-400 mb-3">
                                    {t('accounts.actions_modal.danger_section')}
                                </h4>
                                <div className="p-3 rounded-xl border border-red-200 dark:border-red-900/40 bg-red-50/30 dark:bg-red-950/10 flex items-center justify-between">
                                    <div className="flex items-center gap-3 min-w-0 pr-2">
                                        <div className="p-2 rounded-lg bg-red-100 text-red-600 dark:bg-red-900/30 dark:text-red-400 flex-shrink-0">
                                            <Trash2 size={18} />
                                        </div>
                                        <div className="min-w-0">
                                            <div className="text-xs font-semibold text-red-700 dark:text-red-300">
                                                {t('accounts.actions_modal.delete')}
                                            </div>
                                            <div className="text-[11px] text-red-500/80 dark:text-red-400/80 truncate">
                                                {t('accounts.actions_modal.delete_desc')}
                                            </div>
                                        </div>
                                    </div>
                                    <button
                                        onClick={onDelete}
                                        className="btn btn-xs btn-outline btn-error flex-shrink-0"
                                    >
                                        {t('common.delete', '删除')}
                                    </button>
                                </div>
                            </div>
                        </div>

                        {/* Footer */}
                        <div className="flex items-center justify-end px-6 py-3 border-t border-gray-100 dark:border-base-200 bg-gray-50/50 dark:bg-base-200/40">
                            <button
                                onClick={onClose}
                                className="btn btn-sm btn-ghost text-gray-600 dark:text-gray-300"
                            >
                                {t('common.close')}
                            </button>
                        </div>
                    </>
                ) : (
                    <>
                        {/* Sub-view: Tags Management */}
                        <div className="flex items-center justify-between px-6 py-4 border-b border-gray-100 dark:border-base-200 bg-gray-50/50 dark:bg-base-200/40">
                            <div className="flex items-center gap-3">
                                <button
                                    type="button"
                                    onClick={() => setCurrentView('menu')}
                                    className="p-1.5 -ml-1.5 rounded-lg hover:bg-gray-200 dark:hover:bg-base-300 text-gray-600 dark:text-gray-300 transition-colors flex items-center gap-1 text-xs font-semibold"
                                    title={t('common.back', '返回')}
                                >
                                    <ChevronLeft size={16} />
                                    <span>{t('common.back', '返回')}</span>
                                </button>
                                <div className="w-px h-4 bg-gray-200 dark:bg-base-300" />
                                <div className="flex items-center gap-2">
                                    <div className="w-8 h-8 rounded-lg bg-indigo-50 dark:bg-indigo-900/20 text-indigo-500 flex items-center justify-center flex-shrink-0">
                                        <Tag className="w-4 h-4" />
                                    </div>
                                    <div>
                                        <h3 className="font-bold text-sm text-gray-900 dark:text-base-content leading-tight">
                                            {t('accounts.tag_editor.title', '管理账号标签')}
                                        </h3>
                                        <p className="text-[11px] text-gray-500 dark:text-gray-400 truncate max-w-[260px] leading-tight">
                                            {account.name || account.email}
                                        </p>
                                    </div>
                                </div>
                            </div>
                            <button
                                onClick={onClose}
                                className="btn btn-sm btn-circle btn-ghost text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
                            >
                                <X size={18} />
                            </button>
                        </div>

                        {/* Body */}
                        <div className="p-6 space-y-5 max-h-[75vh] overflow-y-auto">
                            {/* Input section */}
                            <div className="space-y-2">
                                <label className="text-xs font-semibold text-gray-600 dark:text-gray-300 uppercase tracking-wider">
                                    {t('accounts.tag_editor.add_tag', '添加新标签')} ({tags.length}/8)
                                </label>
                                <div className="flex gap-2">
                                    <input
                                        type="text"
                                        placeholder={t('accounts.tag_editor.placeholder', '输入标签名（最多 15 字符）...')}
                                        value={inputValue}
                                        onChange={(e) => setInputValue(e.target.value)}
                                        onKeyDown={handleKeyDownInput}
                                        maxLength={15}
                                        className="input input-bordered flex-1 bg-gray-50 dark:bg-base-200 text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500"
                                    />
                                    <button
                                        type="button"
                                        onClick={() => handleAddTag(inputValue)}
                                        className="btn btn-primary bg-indigo-600 hover:bg-indigo-700 border-none text-white px-4 gap-1.5"
                                    >
                                        <Plus className="w-4 h-4" />
                                        {t('accounts.tag_editor.add_btn', '添加')}
                                    </button>
                                </div>
                                {tagErrorMsg && (
                                    <p className="text-xs text-red-500 mt-1 font-medium">{tagErrorMsg}</p>
                                )}
                            </div>

                            {/* Presets */}
                            <div className="space-y-2">
                                <span className="text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                                    {t('accounts.tag_editor.preset_title', '常用快捷标签')}
                                </span>
                                <div className="flex flex-wrap gap-1.5">
                                    {PRESET_TAGS.map((preset) => {
                                        const exists = tags.some(t => t.toLowerCase() === preset.toLowerCase());
                                        return (
                                            <button
                                                key={preset}
                                                type="button"
                                                onClick={() => handleAddPreset(preset)}
                                                disabled={exists}
                                                className={`px-2.5 py-1 text-xs font-medium rounded-lg transition-all ${
                                                    exists
                                                        ? 'bg-gray-100 dark:bg-base-200 text-gray-400 dark:text-gray-500 cursor-not-allowed'
                                                        : 'bg-indigo-50 dark:bg-indigo-900/20 text-indigo-600 dark:text-indigo-400 hover:bg-indigo-100 dark:hover:bg-indigo-900/40 border border-indigo-200 dark:border-indigo-800/50'
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
                                    {t('accounts.tag_editor.current_title', '当前标签列表')}
                                </span>
                                {tags.length === 0 ? (
                                    <div className="text-center py-8 border-2 border-dashed border-gray-200 dark:border-base-300 rounded-xl text-gray-400 dark:text-gray-500 text-sm">
                                        {t('accounts.tag_editor.empty', '暂无标签，请在上方输入或点击快捷标签添加')}
                                    </div>
                                ) : (
                                    <div className="max-h-48 overflow-y-auto space-y-2 pr-1">
                                        {tags.map((tag, index) => (
                                            <div
                                                key={`${tag}-${index}`}
                                                className="flex items-center justify-between px-3 py-2 bg-gray-50 dark:bg-base-200 rounded-xl border border-gray-100 dark:border-base-300 group"
                                            >
                                                <div className="flex items-center gap-2">
                                                    <span className="w-5 h-5 rounded-full bg-indigo-100 dark:bg-indigo-900/40 text-indigo-600 dark:text-indigo-400 text-xs font-bold flex items-center justify-center">
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
                                                        title={t('accounts.tag_editor.move_up', '上移')}
                                                        className="p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 disabled:opacity-30 disabled:cursor-not-allowed rounded"
                                                    >
                                                        <ArrowUp className="w-4 h-4" />
                                                    </button>
                                                    <button
                                                        type="button"
                                                        onClick={() => handleMoveDown(index)}
                                                        disabled={index === tags.length - 1}
                                                        title={t('accounts.tag_editor.move_down', '下移')}
                                                        className="p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 disabled:opacity-30 disabled:cursor-not-allowed rounded"
                                                    >
                                                        <ArrowDown className="w-4 h-4" />
                                                    </button>
                                                    <button
                                                        type="button"
                                                        onClick={() => handleRemoveTag(index)}
                                                        title={t('accounts.tag_editor.delete', '删除')}
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
                        <div className="flex items-center justify-between px-6 py-3 border-t border-gray-100 dark:border-base-200 bg-gray-50/50 dark:bg-base-200/40">
                            <button
                                type="button"
                                disabled={isSavingTags}
                                onClick={() => setCurrentView('menu')}
                                className="btn btn-sm btn-ghost text-gray-600 dark:text-gray-300"
                            >
                                {t('common.cancel', '取消')}
                            </button>
                            <button
                                type="button"
                                disabled={isSavingTags}
                                onClick={handleSaveTags}
                                className="btn btn-sm btn-primary bg-indigo-600 hover:bg-indigo-700 border-none text-white px-5 gap-1.5"
                            >
                                {isSavingTags ? <Loader2 size={14} className="animate-spin" /> : <Check size={14} />}
                                <span>{t('common.save', '保存')}</span>
                            </button>
                        </div>
                    </>
                )}
            </div>
            <div className="modal-backdrop bg-black/40 backdrop-blur-sm fixed inset-0 z-[-1]" onClick={onClose} />
        </div>,
        document.body
    );
}
