import { createPortal } from 'react-dom';
import { useEffect, useState } from 'react';
import { Wand2, RotateCcw, FolderOpen, Trash2, X } from 'lucide-react';
import { Account, DeviceProfile, DeviceProfileVersion } from '../../types/account';
import * as accountService from '../../services/accountService';
import { useTranslation } from 'react-i18next';

interface DeviceFingerprintDialogProps {
    account: Account | null;
    onClose: () => void;
}

export default function DeviceFingerprintDialog({ account, onClose }: DeviceFingerprintDialogProps) {
    const { t } = useTranslation();
    const [deviceProfiles, setDeviceProfiles] = useState<{ current_storage?: DeviceProfile; history?: DeviceProfileVersion[]; baseline?: DeviceProfile } | null>(null);
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
            const res = await accountService.getDeviceProfiles(target.id);
            setDeviceProfiles(res);
        } catch (e: any) {
            setActionMessage(typeof e === 'string' ? e : '加载设备信息失败');
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
            const profile = await accountService.previewGenerateProfile();
            setConfirmProfile(profile);
            setConfirmType('generate');
        } catch (e: any) {
            setActionMessage(typeof e === 'string' ? e : '生成失败');
        } finally {
            setActionLoading(null);
        }
    };

    const handleConfirmGenerate = async () => {
        if (!account || !confirmProfile) return;
        setActionLoading('generate');
        try {
            await accountService.bindDeviceProfileWithProfile(account.id, confirmProfile);
            setActionMessage('已生成并绑定');
            setConfirmProfile(null);
            setConfirmType(null);
            await fetchDevice(account); // 刷新历史
        } catch (e: any) {
            setActionMessage(typeof e === 'string' ? e : '绑定失败');
        } finally {
            setActionLoading(null);
        }
    };

    const handleRestoreOriginalConfirm = () => {
        if (!deviceProfiles?.baseline) {
            setActionMessage('未找到原始指纹');
            return;
        }
        setConfirmProfile(deviceProfiles.baseline);
        setConfirmType('restoreOriginal');
    };

    const handleRestoreOriginal = async () => {
        if (!account) return;
        setActionLoading('restore');
        try {
            const msg = await accountService.restoreOriginalDevice();
            setActionMessage(msg);
            setConfirmProfile(null);
            setConfirmType(null);
            await fetchDevice(account);
        } catch (e: any) {
            setActionMessage(typeof e === 'string' ? e : '恢复失败');
        } finally {
            setActionLoading(null);
        }
    };

    const handleRestoreVersion = async (versionId: string) => {
        if (!account) return;
        setActionLoading(`restore-${versionId}`);
        try {
            await accountService.restoreDeviceVersion(account.id, versionId);
            setActionMessage('已恢复指定指纹');
            await fetchDevice(account);
        } catch (e: any) {
            setActionMessage(typeof e === 'string' ? e : '恢复失败');
        } finally {
            setActionLoading(null);
        }
    };

    const handleDeleteVersion = async (versionId: string, isCurrent?: boolean) => {
        if (!account || isCurrent) return;
        setActionLoading(`delete-${versionId}`);
        try {
            await accountService.deleteDeviceVersion(account.id, versionId);
            setActionMessage('已删除该历史指纹');
            await fetchDevice(account);
        } catch (e: any) {
            setActionMessage(typeof e === 'string' ? e : '删除失败');
        } finally {
            setActionLoading(null);
        }
    };

    const handleOpenFolder = async () => {
        setActionLoading('open-folder');
        try {
            await accountService.openDeviceFolder();
            setActionMessage('已打开设备存储目录');
        } catch (e: any) {
            setActionMessage(typeof e === 'string' ? e : '无法打开目录');
        } finally {
            setActionLoading(null);
        }
    };

    const renderProfile = (profile?: DeviceProfile) => {
        if (!profile) return <span className="text-xs text-gray-400">{t('common.empty') || '空'}</span>;
        return (
            <div className="grid grid-cols-1 gap-2 text-xs font-mono text-gray-600 dark:text-gray-300">
                <div><span className="font-semibold">machineId:</span> {profile.machine_id}</div>
                <div><span className="font-semibold">macMachineId:</span> {profile.mac_machine_id}</div>
                <div><span className="font-semibold">devDeviceId:</span> {profile.dev_device_id}</div>
                <div><span className="font-semibold">sqmId:</span> {profile.sqm_id}</div>
            </div>
        );
    };

    if (!account) return null;

    return createPortal(
        <div className="modal modal-open z-[120]">
            <div data-tauri-drag-region className="fixed top-0 left-0 right-0 h-8 z-[130]" />
            <div className="modal-box relative max-w-3xl bg-white dark:bg-base-100 shadow-2xl rounded-2xl p-0 overflow-hidden">
                <div className="px-6 py-5 border-b border-gray-100 dark:border-base-200 bg-gray-50/50 dark:bg-base-200/50 flex justify-between items-center">
                    <div className="flex items-center gap-3">
                        <h3 className="font-bold text-lg text-gray-900 dark:text-base-content">设备指纹</h3>
                        <div className="px-2.5 py-0.5 rounded-full bg-gray-100 dark:bg-base-200 border border-gray-200 dark:border-base-300 text-xs font-mono text-gray-500 dark:text-gray-400">
                            {account.email}
                        </div>
                    </div>
                    <button
                        onClick={onClose}
                        className="btn btn-sm btn-circle btn-ghost text-gray-400 hover:bg-gray-100 dark:hover:bg-base-200 hover:text-gray-600 dark:hover:text-base-content transition-colors"
                    >
                        <X size={18} />
                    </button>
                </div>

                <div className="p-6 space-y-3 max-h-[70vh] overflow-y-auto">
                    <div className="flex items-center justify-between mb-2">
                        <div className="text-sm font-semibold text-gray-800 dark:text-gray-200">设备指纹操作</div>
                        <div className="flex gap-2 flex-wrap">
                            <button className="btn btn-xs btn-outline" disabled={loadingDevice || actionLoading === 'preview'} onClick={handleGeneratePreview}>
                                <Wand2 size={14} className="mr-1" />生成并绑定
                            </button>
                            <button className="btn btn-xs btn-outline btn-error" disabled={loadingDevice || actionLoading === 'restore'} onClick={handleRestoreOriginalConfirm}>
                                <RotateCcw size={14} className="mr-1" />恢复原始
                            </button>
                            <button className="btn btn-xs btn-outline" disabled={actionLoading === 'open-folder'} onClick={handleOpenFolder}>
                                <FolderOpen size={14} className="mr-1" />打开存储目录
                            </button>
                        </div>
                    </div>
                    {actionMessage && <div className="text-xs text-blue-600 dark:text-blue-300">{actionMessage}</div>}
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                        <div className="p-4 rounded-xl border border-gray-100 dark:border-base-200 bg-white dark:bg-base-100 shadow-sm">
                            <div className="flex items-center justify-between mb-1">
                                <div className="text-xs font-semibold text-gray-600 dark:text-gray-300">当前存储</div>
                                <span className="text-[10px] px-2 py-0.5 rounded-full bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-300 border border-blue-100 dark:border-blue-400/40">已生效</span>
                            </div>
                            <p className="text-[10px] text-gray-400 dark:text-gray-500 mb-2">读取自 storage.json（切换账号时应用绑定后更新）</p>
                            {loadingDevice ? <div className="text-xs text-gray-400">加载中...</div> : renderProfile(deviceProfiles?.current_storage)}
                        </div>
                        <div className="p-4 rounded-xl border border-gray-100 dark:border-base-200 bg-white dark:bg-base-100 shadow-sm">
                            <div className="flex items-center justify-between mb-1">
                                <div className="text-xs font-semibold text-gray-600 dark:text-gray-300">账号绑定</div>
                                <span className="text-[10px] px-2 py-0.5 rounded-full bg-amber-50 text-amber-600 dark:bg-amber-500/10 dark:text-amber-300 border border-amber-100 dark:border-amber-400/40">待应用</span>
                            </div>
                            <p className="text-[10px] text-gray-400 dark:text-gray-500 mb-2">生成/恢复后保存为绑定，切换账号时写入 storage.json</p>
                            {/* 绑定指纹 = 当前历史中 is_current 的那条 */}
                            {loadingDevice ? (
                                <div className="text-xs text-gray-400">加载中...</div>
                            ) : (
                                renderProfile(deviceProfiles?.history?.find(h => h.is_current)?.profile)
                            )}
                        </div>
                    </div>
                    <div className="p-3 rounded-xl border border-gray-100 dark:border-base-200 bg-white dark:bg-base-100">
                        <div className="text-xs font-semibold text-gray-700 dark:text-gray-200 mb-2">历史指纹（可选恢复/删除）</div>
                        {loadingDevice ? (
                            <div className="text-xs text-gray-400">加载中...</div>
                        ) : (
                            <div className="space-y-2">
                                {deviceProfiles?.history && deviceProfiles.history.map(v => (
                                    <HistoryRow
                                        id={v.id}
                                        key={v.id}
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
                                    <div className="text-xs text-gray-400">暂无历史</div>
                                )}
                            </div>
                        )}
                    </div>
                </div>
            </div>
            <div className="modal-backdrop bg-black/40 backdrop-blur-sm" onClick={onClose}></div>
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
        </div>,
        document.body
    );
}

interface HistoryRowProps {
    id?: string;
    label: string;
    createdAt: number;
    profile: DeviceProfile;
    onRestore: () => void;
    onDelete?: () => void;
    isCurrent?: boolean;
    loadingKey?: string | null;
}

function HistoryRow({ id, label, createdAt, profile, onRestore, onDelete, isCurrent, loadingKey }: HistoryRowProps) {
    const key = id || label;
    return (
        <div className="flex items-start justify-between p-2 rounded-lg border border-gray-100 dark:border-base-200 hover:border-indigo-200 dark:hover:border-indigo-500/40 transition-colors">
            <div className="text-[11px] text-gray-600 dark:text-gray-300 flex-1">
                <div className="font-semibold">{label}{isCurrent && <span className="ml-2 text-[10px] text-blue-500">当前</span>}</div>
                {createdAt > 0 && <div className="text-[10px] text-gray-400">{new Date(createdAt * 1000).toLocaleString()}</div>}
                <div className="mt-1 text-[10px] font-mono text-gray-500">
                    <div>machineId: {profile.machine_id}</div>
                    <div>macMachineId: {profile.mac_machine_id}</div>
                    <div>devDeviceId: {profile.dev_device_id}</div>
                    <div>sqmId: {profile.sqm_id}</div>
                </div>
            </div>
            <div className="flex gap-2 ml-2">
                <button className="btn btn-xs btn-outline" disabled={loadingKey === `restore-${key}` || isCurrent} onClick={onRestore} title="恢复此版本">恢复</button>
                {!isCurrent && onDelete && (
                    <button className="btn btn-xs btn-outline btn-error" disabled={loadingKey === `delete-${key}`} onClick={onDelete} title="删除此版本">
                        <Trash2 size={14} />
                    </button>
                )}
            </div>
        </div>
    );
}

// 确认弹框
function ConfirmDialog({ profile, type, onConfirm, onCancel, loading }: { profile: DeviceProfile; type: 'generate' | 'restoreOriginal'; onConfirm: () => void; onCancel: () => void; loading?: boolean }) {
    const title = type === 'generate' ? '确认生成并绑定？' : '确认恢复原始指纹？';
    const desc =
        type === 'generate'
            ? '将生成一套新的设备指纹并设置为当前指纹。确认继续？'
            : '将恢复为原始指纹并覆盖当前指纹。确认继续？';
    return createPortal(
        <div className="modal modal-open z-[140]">
            <div className="modal-box max-w-sm bg-white dark:bg-base-100 rounded-2xl shadow-2xl p-6 text-center">
                <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-blue-50 text-blue-500 dark:bg-blue-500/10 dark:text-blue-300">
                    <svg xmlns="http://www.w3.org/2000/svg" className="h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                        <path d="M12 9v4" strokeLinecap="round" strokeLinejoin="round" />
                        <path d="M12 17h.01" strokeLinecap="round" strokeLinejoin="round" />
                        <path d="M10 2h4l8 8v4l-8 8h-4l-8-8v-4z" strokeLinecap="round" strokeLinejoin="round" />
                    </svg>
                </div>
                <h3 className="font-bold text-lg text-gray-900 dark:text-base-content mb-1">{title}</h3>
                <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">{desc}</p>
                <div className="text-xs font-mono text-gray-600 dark:text-gray-300 bg-gray-50 dark:bg-base-200/60 border border-gray-100 dark:border-base-200 rounded-lg p-3 text-left space-y-1">
                    <div><span className="font-semibold">machineId:</span> {profile.machine_id}</div>
                    <div><span className="font-semibold">macMachineId:</span> {profile.mac_machine_id}</div>
                    <div><span className="font-semibold">devDeviceId:</span> {profile.dev_device_id}</div>
                    <div><span className="font-semibold">sqmId:</span> {profile.sqm_id}</div>
                </div>
                <div className="mt-5 flex gap-3 justify-center">
                    <button className="btn btn-sm min-w-[100px]" onClick={onCancel} disabled={!!loading}>取消</button>
                    <button className="btn btn-sm btn-primary min-w-[100px]" onClick={onConfirm} disabled={!!loading}>{loading ? '处理中...' : '确认'}</button>
                </div>
            </div>
            <div className="modal-backdrop bg-black/30" onClick={onCancel}></div>
        </div>,
        document.body
    );
}
