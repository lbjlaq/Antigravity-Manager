// File: src/features/accounts/ui/DeviceFingerprintDialog.tsx
// Device fingerprint management dialog - restored from original with FSD adaptation

import { memo, useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { motion, AnimatePresence } from 'framer-motion';
import { Wand2, RotateCcw, FolderOpen, Trash2, X, Fingerprint } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Account } from '@/entities/account';
import { cn } from '@/shared/lib';
import { invoke } from '@/shared/api';

// Types matching backend
interface DeviceProfile {
    machine_id: string;
    mac_machine_id: string;
    dev_device_id: string;
    sqm_id: string;
}

interface DeviceProfileVersion {
    id: string;
    label?: string;
    created_at: number;
    profile: DeviceProfile;
    is_current: boolean;
}

interface DeviceProfilesResponse {
    current_storage?: DeviceProfile;
    history?: DeviceProfileVersion[];
    baseline?: DeviceProfile;
}

interface DeviceFingerprintDialogProps {
    account: Account | null;
    onClose: () => void;
}

// API functions
const getDeviceProfiles = (accountId: string) => 
    invoke<DeviceProfilesResponse>('get_device_profiles', { accountId });

const previewGenerateProfile = () => 
    invoke<DeviceProfile>('preview_generate_profile');

const bindDeviceProfileWithProfile = (accountId: string, profile: DeviceProfile) => 
    invoke<void>('bind_device_profile', { accountId, profile });

const restoreOriginalDevice = () => 
    invoke<string>('restore_original_device');

const restoreDeviceVersion = (accountId: string, versionId: string) => 
    invoke<void>('restore_device_version', { accountId, versionId });

const deleteDeviceVersion = (accountId: string, versionId: string) => 
    invoke<void>('delete_device_version', { accountId, versionId });

const openDeviceFolder = () => 
    invoke<void>('open_device_folder');

export const DeviceFingerprintDialog = memo(function DeviceFingerprintDialog({ account, onClose }: DeviceFingerprintDialogProps) {
    const { t } = useTranslation();
    const [deviceProfiles, setDeviceProfiles] = useState<DeviceProfilesResponse | null>(null);
    const [loadingDevice, setLoadingDevice] = useState(false);
    const [actionLoading, setActionLoading] = useState<string | null>(null);
    const [actionMessage, setActionMessage] = useState<string | null>(null);
    const [confirmProfile, setConfirmProfile] = useState<DeviceProfile | null>(null);
    const [confirmType, setConfirmType] = useState<'generate' | 'restoreOriginal' | null>(null);

    const fetchDevice = async (target?: Account | null) => {
        if (!target) {
            setDeviceProfiles(null);
            return;
        }
        setLoadingDevice(true);
        try {
            const res = await getDeviceProfiles(target.id);
            setDeviceProfiles(res);
        } catch (e: unknown) {
            const errorMsg = typeof e === 'string' ? e : (e as Error).message || '';
            const translated = errorMsg === 'storage_json_not_found'
                ? t('accounts.device_fingerprint_dialog.storage_json_not_found')
                : (typeof e === 'string' ? e : t('accounts.device_fingerprint_dialog.failed_to_load_device_info'));
            setActionMessage(translated);
        } finally {
            setLoadingDevice(false);
        }
    };

    useEffect(() => {
        fetchDevice(account);
    }, [account]);

    const handleGeneratePreview = async () => {
        setActionLoading('preview');
        try {
            const profile = await previewGenerateProfile();
            setConfirmProfile(profile);
            setConfirmType('generate');
        } catch (e: unknown) {
            setActionMessage(typeof e === 'string' ? e : t('accounts.device_fingerprint_dialog.generation_failed'));
        } finally {
            setActionLoading(null);
        }
    };

    const handleConfirmGenerate = async () => {
        if (!account || !confirmProfile) return;
        setActionLoading('generate');
        try {
            await bindDeviceProfileWithProfile(account.id, confirmProfile);
            setActionMessage(t('accounts.device_fingerprint_dialog.generated_and_bound'));
            setConfirmProfile(null);
            setConfirmType(null);
            await fetchDevice(account);
        } catch (e: unknown) {
            setActionMessage(typeof e === 'string' ? e : t('accounts.device_fingerprint_dialog.binding_failed'));
        } finally {
            setActionLoading(null);
        }
    };

    const handleRestoreOriginalConfirm = () => {
        if (!deviceProfiles?.baseline) {
            setActionMessage(t('accounts.device_fingerprint_dialog.original_fingerprint_not_found'));
            return;
        }
        setConfirmProfile(deviceProfiles.baseline);
        setConfirmType('restoreOriginal');
    };

    const handleRestoreOriginal = async () => {
        if (!account) return;
        setActionLoading('restore');
        try {
            const msg = await restoreOriginalDevice();
            setActionMessage(msg || t('accounts.device_fingerprint_dialog.restored'));
            setConfirmProfile(null);
            setConfirmType(null);
            await fetchDevice(account);
        } catch (e: unknown) {
            setActionMessage(typeof e === 'string' ? e : t('accounts.device_fingerprint_dialog.restoration_failed'));
        } finally {
            setActionLoading(null);
        }
    };

    const handleRestoreVersion = async (versionId: string) => {
        if (!account) return;
        setActionLoading(`restore-${versionId}`);
        try {
            await restoreDeviceVersion(account.id, versionId);
            setActionMessage(t('accounts.device_fingerprint_dialog.restored'));
            await fetchDevice(account);
        } catch (e: unknown) {
            setActionMessage(typeof e === 'string' ? e : t('accounts.device_fingerprint_dialog.restoration_failed'));
        } finally {
            setActionLoading(null);
        }
    };

    const handleDeleteVersion = async (versionId: string, isCurrent?: boolean) => {
        if (!account || isCurrent) return;
        setActionLoading(`delete-${versionId}`);
        try {
            await deleteDeviceVersion(account.id, versionId);
            setActionMessage(t('accounts.device_fingerprint_dialog.deleted'));
            await fetchDevice(account);
        } catch (e: unknown) {
            setActionMessage(typeof e === 'string' ? e : t('accounts.device_fingerprint_dialog.deletion_failed'));
        } finally {
            setActionLoading(null);
        }
    };

    const handleOpenFolder = async () => {
        setActionLoading('open-folder');
        try {
            await openDeviceFolder();
            setActionMessage(t('accounts.device_fingerprint_dialog.directory_opened'));
        } catch (e: unknown) {
            setActionMessage(typeof e === 'string' ? e : t('accounts.device_fingerprint_dialog.directory_open_failed'));
        } finally {
            setActionLoading(null);
        }
    };

    const renderProfile = (profile?: DeviceProfile) => {
        if (!profile) return <span className="text-xs text-zinc-400">{t('common.empty', '—')}</span>;
        return (
            <div className="grid grid-cols-1 gap-1.5 text-xs font-mono text-zinc-600 dark:text-zinc-300">
                <div><span className="font-semibold text-zinc-500">machineId:</span> {profile.machine_id}</div>
                <div><span className="font-semibold text-zinc-500">macMachineId:</span> {profile.mac_machine_id}</div>
                <div><span className="font-semibold text-zinc-500">devDeviceId:</span> {profile.dev_device_id}</div>
                <div><span className="font-semibold text-zinc-500">sqmId:</span> {profile.sqm_id}</div>
            </div>
        );
    };

    if (!account) return null;

    return createPortal(
        <AnimatePresence>
            <div className="fixed inset-0 z-[120] flex items-center justify-center p-4">
                {/* Backdrop */}
                <motion.div 
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0 }}
                    className="absolute inset-0 bg-black/50 backdrop-blur-sm"
                    onClick={onClose}
                />

                {/* Tauri drag region */}
                <div data-tauri-drag-region className="fixed top-0 left-0 right-0 h-8 z-[130]" />

                {/* Modal */}
                <motion.div
                    initial={{ opacity: 0, scale: 0.95, y: 10 }}
                    animate={{ opacity: 1, scale: 1, y: 0 }}
                    exit={{ opacity: 0, scale: 0.95, y: 10 }}
                    transition={{ duration: 0.2, ease: 'easeOut' }}
                    className="relative w-full max-w-3xl bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 shadow-2xl overflow-hidden"
                >
                    {/* Header */}
                    <div className="px-6 py-4 border-b border-zinc-200 dark:border-zinc-800 bg-zinc-50 dark:bg-zinc-900/50 flex justify-between items-center">
                        <div className="flex items-center gap-3">
                            <div className="p-2 rounded-lg bg-indigo-500/10">
                                <Fingerprint className="w-5 h-5 text-indigo-500" />
                            </div>
                            <div>
                                <h3 className="font-bold text-lg text-zinc-900 dark:text-white">{t('accounts.device_fingerprint_dialog.title')}</h3>
                                <p className="text-xs text-zinc-500 font-mono">{account.email}</p>
                            </div>
                        </div>
                        <button
                            onClick={onClose}
                            className="p-2 rounded-lg text-zinc-400 hover:text-zinc-600 dark:hover:text-white hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors"
                        >
                            <X className="w-5 h-5" />
                        </button>
                    </div>

                    {/* Content */}
                    <div className="p-6 space-y-4 max-h-[70vh] overflow-y-auto scrollbar-thin scrollbar-thumb-zinc-300 dark:scrollbar-thumb-zinc-700">
                        {/* Operations */}
                        <div className="flex items-center justify-between">
                            <div className="text-sm font-semibold text-zinc-800 dark:text-zinc-200">
                                {t('accounts.device_fingerprint_dialog.operations')}
                            </div>
                            <div className="flex gap-2 flex-wrap">
                                <button 
                                    className={cn(
                                        "px-3 py-1.5 text-xs font-medium rounded-lg border transition-colors flex items-center gap-1.5",
                                        "bg-white dark:bg-zinc-800 border-zinc-200 dark:border-zinc-700",
                                        "hover:bg-zinc-50 dark:hover:bg-zinc-700 text-zinc-700 dark:text-zinc-300",
                                        "disabled:opacity-50"
                                    )}
                                    disabled={loadingDevice || actionLoading === 'preview'} 
                                    onClick={handleGeneratePreview}
                                >
                                    <Wand2 className="w-3.5 h-3.5" />
                                    {t('accounts.device_fingerprint_dialog.generate_and_bind')}
                                </button>
                                <button 
                                    className={cn(
                                        "px-3 py-1.5 text-xs font-medium rounded-lg border transition-colors flex items-center gap-1.5",
                                        "bg-white dark:bg-zinc-800 border-rose-200 dark:border-rose-800/50",
                                        "hover:bg-rose-50 dark:hover:bg-rose-900/20 text-rose-600 dark:text-rose-400",
                                        "disabled:opacity-50"
                                    )}
                                    disabled={loadingDevice || actionLoading === 'restore'} 
                                    onClick={handleRestoreOriginalConfirm}
                                >
                                    <RotateCcw className="w-3.5 h-3.5" />
                                    {t('accounts.device_fingerprint_dialog.restore_original')}
                                </button>
                                <button 
                                    className={cn(
                                        "px-3 py-1.5 text-xs font-medium rounded-lg border transition-colors flex items-center gap-1.5",
                                        "bg-white dark:bg-zinc-800 border-zinc-200 dark:border-zinc-700",
                                        "hover:bg-zinc-50 dark:hover:bg-zinc-700 text-zinc-700 dark:text-zinc-300",
                                        "disabled:opacity-50"
                                    )}
                                    disabled={actionLoading === 'open-folder'} 
                                    onClick={handleOpenFolder}
                                >
                                    <FolderOpen className="w-3.5 h-3.5" />
                                    {t('accounts.device_fingerprint_dialog.open_storage_directory')}
                                </button>
                            </div>
                        </div>

                        {/* Action Message */}
                        {actionMessage && (
                            <div className="text-xs text-indigo-600 dark:text-indigo-400 bg-indigo-50 dark:bg-indigo-900/20 px-3 py-2 rounded-lg">
                                {actionMessage}
                            </div>
                        )}

                        {/* Storage Cards */}
                        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                            {/* Current Storage */}
                            <div className="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900">
                                <div className="flex items-center justify-between mb-2">
                                    <div className="text-xs font-semibold text-zinc-600 dark:text-zinc-300">
                                        {t('accounts.device_fingerprint_dialog.current_storage')}
                                    </div>
                                    <span className="text-[10px] px-2 py-0.5 rounded-full bg-blue-50 dark:bg-blue-500/10 text-blue-600 dark:text-blue-400 border border-blue-100 dark:border-blue-400/30">
                                        {t('accounts.device_fingerprint_dialog.effective')}
                                    </span>
                                </div>
                                <p className="text-[10px] text-zinc-400 dark:text-zinc-500 mb-3">
                                    {t('accounts.device_fingerprint_dialog.current_storage_desc')}
                                </p>
                                {loadingDevice ? (
                                    <div className="text-xs text-zinc-400">{t('accounts.device_fingerprint_dialog.loading', 'Loading...')}</div>
                                ) : (
                                    renderProfile(deviceProfiles?.current_storage)
                                )}
                            </div>

                            {/* Account Binding */}
                            <div className="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900">
                                <div className="flex items-center justify-between mb-2">
                                    <div className="text-xs font-semibold text-zinc-600 dark:text-zinc-300">
                                        {t('accounts.device_fingerprint_dialog.account_binding')}
                                    </div>
                                    <span className="text-[10px] px-2 py-0.5 rounded-full bg-amber-50 dark:bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-100 dark:border-amber-400/30">
                                        {t('accounts.device_fingerprint_dialog.pending_application')}
                                    </span>
                                </div>
                                <p className="text-[10px] text-zinc-400 dark:text-zinc-500 mb-3">
                                    {t('accounts.device_fingerprint_dialog.account_binding_desc')}
                                </p>
                                {loadingDevice ? (
                                    <div className="text-xs text-zinc-400">{t('accounts.device_fingerprint_dialog.loading', 'Loading...')}</div>
                                ) : (
                                    renderProfile(deviceProfiles?.history?.find(h => h.is_current)?.profile)
                                )}
                            </div>
                        </div>

                        {/* History */}
                        <div className="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900">
                            <div className="text-xs font-semibold text-zinc-700 dark:text-zinc-200 mb-3">
                                {t('accounts.device_fingerprint_dialog.historical_fingerprints')}
                            </div>
                            {loadingDevice ? (
                                <div className="text-xs text-zinc-400">{t('accounts.device_fingerprint_dialog.loading', 'Loading...')}</div>
                            ) : (
                                <div className="space-y-2">
                                    {deviceProfiles?.history?.map(v => (
                                        <HistoryRow
                                            key={v.id}
                                            id={v.id}
                                            label={v.label || v.id}
                                            createdAt={v.created_at}
                                            profile={v.profile}
                                            isCurrent={v.is_current}
                                            onRestore={() => handleRestoreVersion(v.id)}
                                            onDelete={() => handleDeleteVersion(v.id, v.is_current)}
                                            loadingKey={actionLoading}
                                        />
                                    ))}
                                    {(!deviceProfiles?.history || deviceProfiles.history.length === 0) && !deviceProfiles?.baseline && (
                                        <div className="text-xs text-zinc-400">{t('accounts.device_fingerprint_dialog.no_history')}</div>
                                    )}
                                </div>
                            )}
                        </div>
                    </div>
                </motion.div>

                {/* Confirm Dialog */}
                {confirmProfile && confirmType && (
                    <ConfirmDialog
                        profile={confirmProfile}
                        type={confirmType}
                        onCancel={() => {
                            if (actionLoading) return;
                            setConfirmProfile(null);
                            setConfirmType(null);
                        }}
                        onConfirm={confirmType === 'generate' ? handleConfirmGenerate : handleRestoreOriginal}
                        loading={!!actionLoading}
                    />
                )}
            </div>
        </AnimatePresence>,
        document.body
    );
});

// History Row Component
interface HistoryRowProps {
    id: string;
    label: string;
    createdAt: number;
    profile: DeviceProfile;
    onRestore: () => void;
    onDelete?: () => void;
    isCurrent?: boolean;
    loadingKey?: string | null;
}

function HistoryRow({ id, label, createdAt, profile, onRestore, onDelete, isCurrent, loadingKey }: HistoryRowProps) {
    const { t } = useTranslation();
    return (
        <div className="flex items-start justify-between p-3 rounded-lg border border-zinc-100 dark:border-zinc-800 hover:border-indigo-200 dark:hover:border-indigo-500/40 transition-colors bg-zinc-50 dark:bg-zinc-800/30">
            <div className="text-[11px] text-zinc-600 dark:text-zinc-300 flex-1">
                <div className="font-semibold">
                    {label}
                    {isCurrent && (
                        <span className="ml-2 text-[10px] text-indigo-500 dark:text-indigo-400">
                            {t('accounts.device_fingerprint_dialog.current')}
                        </span>
                    )}
                </div>
                {createdAt > 0 && (
                    <div className="text-[10px] text-zinc-400 mt-0.5">
                        {new Date(createdAt * 1000).toLocaleString()}
                    </div>
                )}
                <div className="mt-2 text-[10px] font-mono text-zinc-500 space-y-0.5">
                    <div>machineId: {profile.machine_id}</div>
                    <div>macMachineId: {profile.mac_machine_id}</div>
                    <div>devDeviceId: {profile.dev_device_id}</div>
                    <div>sqmId: {profile.sqm_id}</div>
                </div>
            </div>
            <div className="flex gap-2 ml-3">
                <button 
                    className={cn(
                        "px-2 py-1 text-[10px] font-medium rounded border transition-colors",
                        "bg-white dark:bg-zinc-800 border-zinc-200 dark:border-zinc-700",
                        "hover:bg-zinc-50 dark:hover:bg-zinc-700 text-zinc-600 dark:text-zinc-300",
                        "disabled:opacity-50"
                    )}
                    disabled={loadingKey === `restore-${id}` || isCurrent} 
                    onClick={onRestore}
                >
                    {t('accounts.device_fingerprint_dialog.restore')}
                </button>
                {!isCurrent && onDelete && (
                    <button 
                        className={cn(
                            "p-1 rounded border transition-colors",
                            "bg-white dark:bg-zinc-800 border-rose-200 dark:border-rose-800/50",
                            "hover:bg-rose-50 dark:hover:bg-rose-900/20 text-rose-500",
                            "disabled:opacity-50"
                        )}
                        disabled={loadingKey === `delete-${id}`} 
                        onClick={onDelete}
                    >
                        <Trash2 className="w-3.5 h-3.5" />
                    </button>
                )}
            </div>
        </div>
    );
}

// Confirm Dialog Component
interface ConfirmDialogProps {
    profile: DeviceProfile;
    type: 'generate' | 'restoreOriginal';
    onConfirm: () => void;
    onCancel: () => void;
    loading?: boolean;
}

function ConfirmDialog({ profile, type, onConfirm, onCancel, loading }: ConfirmDialogProps) {
    const { t } = useTranslation();
    const title = type === 'generate' 
        ? t('accounts.device_fingerprint_dialog.confirm_generate_title') 
        : t('accounts.device_fingerprint_dialog.confirm_restore_title');
    const desc = type === 'generate'
        ? t('accounts.device_fingerprint_dialog.confirm_generate_desc')
        : t('accounts.device_fingerprint_dialog.confirm_restore_desc');

    return createPortal(
        <div className="fixed inset-0 z-[140] flex items-center justify-center p-4">
            <div className="absolute inset-0 bg-black/30" onClick={onCancel} />
            <motion.div
                initial={{ opacity: 0, scale: 0.95 }}
                animate={{ opacity: 1, scale: 1 }}
                className="relative w-full max-w-sm bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 shadow-2xl p-6 text-center"
            >
                <div className="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-full bg-indigo-50 dark:bg-indigo-500/10 text-indigo-500">
                    <Fingerprint className="w-6 h-6" />
                </div>
                <h3 className="font-bold text-lg text-zinc-900 dark:text-white mb-2">{title}</h3>
                <p className="text-sm text-zinc-500 dark:text-zinc-400 mb-4">{desc}</p>
                
                <div className="text-xs font-mono text-zinc-600 dark:text-zinc-300 bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg p-3 text-left space-y-1 mb-5">
                    <div><span className="font-semibold text-zinc-500">machineId:</span> {profile.machine_id}</div>
                    <div><span className="font-semibold text-zinc-500">macMachineId:</span> {profile.mac_machine_id}</div>
                    <div><span className="font-semibold text-zinc-500">devDeviceId:</span> {profile.dev_device_id}</div>
                    <div><span className="font-semibold text-zinc-500">sqmId:</span> {profile.sqm_id}</div>
                </div>
                
                <div className="flex gap-3 justify-center">
                    <button 
                        className="px-4 py-2 text-sm font-medium rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-zinc-700 dark:text-zinc-300 hover:bg-zinc-50 dark:hover:bg-zinc-700 transition-colors disabled:opacity-50"
                        onClick={onCancel} 
                        disabled={loading}
                    >
                        {t('accounts.device_fingerprint_dialog.cancel')}
                    </button>
                    <button 
                        className="px-4 py-2 text-sm font-medium rounded-lg bg-indigo-600 text-white hover:bg-indigo-500 transition-colors disabled:opacity-50"
                        onClick={onConfirm} 
                        disabled={loading}
                    >
                        {loading ? t('accounts.device_fingerprint_dialog.processing') : t('accounts.device_fingerprint_dialog.confirm')}
                    </button>
                </div>
            </motion.div>
        </div>,
        document.body
    );
}

export default DeviceFingerprintDialog;
