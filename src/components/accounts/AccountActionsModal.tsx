import { useEffect } from 'react';
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
    Loader2
} from 'lucide-react';
import { Account } from '../../types/account';
import { useTranslation } from 'react-i18next';
import { parseTags } from '../../utils/tagUtils';

export interface AccountActionsModalProps {
    isOpen: boolean;
    onClose: () => void;
    account: Account;
    onSwitch: (targetIde: 'classic' | 'ide' | 'agy' | 'acp') => void;
    onWarmup?: () => void;
    onViewDevice: () => void;
    onViewDetails: () => void;
    onExport: () => void;
    onOpenTagEditor: () => void;
    onDelete: () => void;
    isSwitching?: boolean;
    isRefreshing?: boolean;
}

export default function AccountActionsModal({
    isOpen,
    onClose,
    account,
    onSwitch,
    onWarmup,
    onViewDevice,
    onViewDetails,
    onExport,
    onOpenTagEditor,
    onDelete,
    isSwitching = false,
    isRefreshing = false,
}: AccountActionsModalProps) {
    const { t } = useTranslation();

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

    const tags = parseTags(account.custom_label);
    const isSwitchDisabled = isSwitching || Boolean(account.disabled) || Boolean(account.banned);

    return createPortal(
        <div className="modal modal-open z-[130] bg-black/50 backdrop-blur-sm transition-all">
            <div className="modal-box w-11/12 max-w-xl bg-white dark:bg-base-100 rounded-2xl shadow-2xl p-0 overflow-hidden border border-gray-100 dark:border-base-200">
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
                                <div className="p-2 rounded-lg bg-blue-50 text-blue-600 dark:bg-blue-500/15 dark:text-blue-400 group-hover:scale-105 transition-transform">
                                    <Repeat2 size={18} />
                                </div>
                                <div className="flex-1 min-w-0">
                                    <div className="text-xs font-semibold text-gray-900 dark:text-base-content truncate">
                                        {t('accounts.actions_modal.clients.ide')}
                                    </div>
                                    <div className="text-[11px] text-gray-500 dark:text-gray-400 truncate">
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
                                <div className="p-2 rounded-lg bg-purple-50 text-purple-600 dark:bg-purple-500/15 dark:text-purple-400 group-hover:scale-105 transition-transform">
                                    <Blocks size={18} />
                                </div>
                                <div className="flex-1 min-w-0">
                                    <div className="text-xs font-semibold text-gray-900 dark:text-base-content truncate">
                                        {t('accounts.actions_modal.clients.acp')}
                                    </div>
                                    <div className="text-[11px] text-gray-500 dark:text-gray-400 truncate">
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
                                <div className="p-2 rounded-lg bg-emerald-50 text-emerald-600 dark:bg-emerald-500/15 dark:text-emerald-400 group-hover:scale-105 transition-transform">
                                    <Terminal size={18} />
                                </div>
                                <div className="flex-1 min-w-0">
                                    <div className="text-xs font-semibold text-gray-900 dark:text-base-content truncate">
                                        {t('accounts.actions_modal.clients.agy')}
                                    </div>
                                    <div className="text-[11px] text-gray-500 dark:text-gray-400 truncate">
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
                                <div className="p-2 rounded-lg bg-amber-50 text-amber-600 dark:bg-amber-500/15 dark:text-amber-400 group-hover:scale-105 transition-transform">
                                    <ArrowRightLeft size={18} />
                                </div>
                                <div className="flex-1 min-w-0">
                                    <div className="text-xs font-semibold text-gray-900 dark:text-base-content truncate">
                                        {t('accounts.actions_modal.clients.classic')}
                                    </div>
                                    <div className="text-[11px] text-gray-500 dark:text-gray-400 truncate">
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
                                onClick={onOpenTagEditor}
                                className="w-full flex items-center justify-between p-3 rounded-xl border border-gray-200 dark:border-base-200 hover:border-indigo-500 dark:hover:border-indigo-500 hover:bg-indigo-50/40 dark:hover:bg-indigo-500/10 transition-all text-left group"
                            >
                                <div className="flex items-center gap-3 min-w-0">
                                    <div className="p-2 rounded-lg bg-indigo-50 text-indigo-600 dark:bg-indigo-500/15 dark:text-indigo-400">
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
                                <div className="flex items-center gap-1.5 flex-wrap justify-end max-w-[180px]">
                                    {tags.length > 0 ? (
                                        tags.map((tag, idx) => (
                                            <span
                                                key={idx}
                                                className="px-1.5 py-0.5 text-[10px] bg-indigo-100 dark:bg-indigo-900/40 text-indigo-700 dark:text-indigo-300 rounded font-medium truncate"
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
                                        <div className="p-2 rounded-lg bg-cyan-50 text-cyan-600 dark:bg-cyan-500/15 dark:text-cyan-400">
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
                                    <div className="p-2 rounded-lg bg-teal-50 text-teal-600 dark:bg-teal-500/15 dark:text-teal-400">
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
                                    <div className="p-2 rounded-lg bg-sky-50 text-sky-600 dark:bg-sky-500/15 dark:text-sky-400">
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
                                    <div className="p-2 rounded-lg bg-blue-50 text-blue-600 dark:bg-blue-500/15 dark:text-blue-400">
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
            </div>
            <div className="modal-backdrop" onClick={onClose} />
        </div>,
        document.body
    );
}
