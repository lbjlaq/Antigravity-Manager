import { useEffect, useMemo, useState } from 'react';
import { X, Clock, AlertCircle, Bot, Ban, ExternalLink, Copy } from 'lucide-react';
import { createPortal } from 'react-dom';
import { Account, getAccountTier, getTierLabel, ModelPriorities } from '../../types/account';
import { formatDate, formatTimeRemaining } from '../../utils/format';
import { useTranslation } from 'react-i18next';
import { MODEL_CONFIG, sortModels } from '../../config/modelConfig';
import { isSupportedDisplayModel } from '../../utils/quotaDisplay';
import { showToast } from '../common/ToastContainer';
import { copyToClipboard } from '../../utils/clipboard';

interface AccountDetailsDialogProps {
    account: Account | null;
    onClose: () => void;
    onUpdatePriority: (accountId: string, priority: number, modelPriorities?: ModelPriorities) => Promise<void>;
    onUpdateProjectId?: (accountId: string, projectId: string) => Promise<void>;
    onAutoFetchProjectId?: (accountId: string) => Promise<string>;
}

export default function AccountDetailsDialog({
    account,
    onClose,
    onUpdatePriority,
    onUpdateProjectId,
    onAutoFetchProjectId,
}: AccountDetailsDialogProps) {
    const { t } = useTranslation();
    const [activeTab, setActiveTab] = useState<'basic' | 'detailed'>('basic');
    const [priorityInput, setPriorityInput] = useState('50');
    const [geminiPriorityInput, setGeminiPriorityInput] = useState('');
    const [claudePriorityInput, setClaudePriorityInput] = useState('');
    const [savingPriority, setSavingPriority] = useState(false);
    const [projectIdInput, setProjectIdInput] = useState('');
    const [savingProjectId, setSavingProjectId] = useState(false);
    const [fetchingProjectId, setFetchingProjectId] = useState(false);

    const displayGroups = useMemo(() => {
        return account?.quota?.quota_groups || [];
    }, [account?.quota?.quota_groups]);

    const displayModels = useMemo(() => {
        const models = account?.quota?.models || [];
        return models.filter(m => isSupportedDisplayModel(m.name || ''));
    }, [account?.quota?.models]);

    useEffect(() => {
        setPriorityInput(String(account?.priority ?? 50));
        setGeminiPriorityInput(
            account?.model_priorities?.gemini !== undefined
                ? String(account.model_priorities.gemini)
                : ''
        );
        setClaudePriorityInput(
            account?.model_priorities?.claude !== undefined
                ? String(account.model_priorities.claude)
                : ''
        );
        setProjectIdInput(account?.token?.project_id || '');
    }, [
        account?.id,
        account?.priority,
        account?.model_priorities?.gemini,
        account?.model_priorities?.claude,
        account?.token?.project_id,
    ]);

    if (!account) return null;

    const currentGeminiStr =
        account.model_priorities?.gemini !== undefined ? String(account.model_priorities.gemini) : '';
    const currentClaudeStr =
        account.model_priorities?.claude !== undefined ? String(account.model_priorities.claude) : '';
    const isPriorityDirty =
        priorityInput.trim() !== String(account.priority ?? 50) ||
        geminiPriorityInput.trim() !== currentGeminiStr ||
        claudePriorityInput.trim() !== currentClaudeStr;

    const savePriority = async () => {
        const priority = Number(priorityInput);
        if (!Number.isInteger(priority) || priority < 1 || priority > 100) {
            showToast(t('accounts.priority_invalid'), 'error');
            return;
        }
        const nextModelPriorities: ModelPriorities = {};
        const gTrim = geminiPriorityInput.trim();
        if (gTrim !== '') {
            const gVal = Number(gTrim);
            if (!Number.isInteger(gVal) || gVal < 1 || gVal > 100) {
                showToast(t('accounts.priority_invalid'), 'error');
                return;
            }
            nextModelPriorities.gemini = gVal;
        }
        const cTrim = claudePriorityInput.trim();
        if (cTrim !== '') {
            const cVal = Number(cTrim);
            if (!Number.isInteger(cVal) || cVal < 1 || cVal > 100) {
                showToast(t('accounts.priority_invalid'), 'error');
                return;
            }
            nextModelPriorities.claude = cVal;
        }

        setSavingPriority(true);
        try {
            await onUpdatePriority(account.id, priority, nextModelPriorities);
            showToast(t('accounts.priority_updated'), 'success');
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        } finally {
            setSavingPriority(false);
        }
    };

    const saveProjectId = async () => {
        if (!onUpdateProjectId) return;
        setSavingProjectId(true);
        try {
            await onUpdateProjectId(account.id, projectIdInput.trim());
            showToast(t('accounts.project_id_updated', 'GCP Project ID 已更新'), 'success');
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        } finally {
            setSavingProjectId(false);
        }
    };

    const autoFetchProjectId = async () => {
        if (!onAutoFetchProjectId) return;
        setFetchingProjectId(true);
        try {
            const fetched = await onAutoFetchProjectId(account.id);
            setProjectIdInput(fetched);
            showToast(
                t('accounts.auto_fetch_success', {
                    defaultValue: '成功自动获取 Project ID: {{projectId}}',
                    projectId: fetched,
                }),
                'success'
            );
        } catch (error) {
            showToast(
                `${t('accounts.auto_fetch_failed', '未能在上游找到关联的 Project ID')}: ${error}`,
                'error'
            );
        } finally {
            setFetchingProjectId(false);
        }
    };

    return createPortal(
        <div className="modal modal-open z-[100]">
            {/* Draggable Top Region */}
            <div data-tauri-drag-region className="fixed top-0 left-0 right-0 h-8 z-[110]" />

            <div className="modal-box relative max-w-3xl bg-white dark:bg-base-100 shadow-2xl rounded-2xl p-0 overflow-hidden">
                {/* Header */}
                <div className="px-6 py-5 border-b border-gray-100 dark:border-base-200 bg-gray-50/50 dark:bg-base-200/50 flex justify-between items-center">
                    <div className="flex items-center gap-3">
                        <h3 className="font-bold text-lg text-gray-900 dark:text-base-content">{t('accounts.details.title')}</h3>
                        <div className="px-2.5 py-0.5 rounded-full bg-gray-100 dark:bg-base-200 border border-gray-200 dark:border-base-300 text-xs font-mono text-gray-500 dark:text-gray-400">
                            {account.email}
                        </div>
                        {(() => {
                            const tier = getAccountTier(account);
                            const label = getTierLabel(tier);
                            return (
                                <div className={`px-2 py-0.5 rounded text-[10px] font-bold uppercase tracking-wider ${
                                    tier === 'ultra' ? 'bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-400' :
                                    tier === 'pro' ? 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400' :
                                    'bg-gray-100 text-gray-600 dark:bg-base-300 dark:text-gray-400'
                                }`}>
                                    {label}
                                </div>
                            );
                        })()}
                    </div>
                    <button
                        onClick={onClose}
                        className="btn btn-sm btn-circle btn-ghost text-gray-400 hover:bg-gray-100 dark:hover:bg-base-200 hover:text-gray-600 dark:hover:text-base-content transition-colors"
                    >
                        <X size={18} />
                    </button>
                </div>

                {/* Status Alerts */}
                {(account.disabled || account.proxy_disabled || account.banned) && (
                    <div className="px-6 py-3 bg-red-50 dark:bg-red-950/20 border-b border-red-100 dark:border-red-900/30 flex flex-col gap-1">
                        {account.banned && (
                            <div className="flex flex-wrap items-center gap-2 text-xs text-red-700 dark:text-red-400">
                                <Ban size={14} />
                                <span className="font-semibold">{t('accounts.status.banned', '账号已封禁')}:</span>
                                <span className="break-all">{account.banned_reason || t('common.unknown')}</span>
                                {account.ban_appeal_url && (
                                    <>
                                        <a
                                            href={account.ban_appeal_url}
                                            target="_blank"
                                            rel="noopener noreferrer"
                                            className="inline-flex items-center gap-1 text-blue-600 dark:text-blue-400 hover:underline font-semibold"
                                        >
                                            <ExternalLink size={12} />
                                            {t('accounts.go_to_appeal', '前往申诉')}
                                        </a>
                                        <button
                                            onClick={async () => {
                                                const ok = await copyToClipboard(account.ban_appeal_url || '');
                                                showToast(
                                                    ok
                                                        ? t('accounts.appeal_url_copied', '申诉链接已复制到剪贴板')
                                                        : t('accounts.copy_failed', '复制失败，请手动选择链接复制'),
                                                    ok ? 'success' : 'error'
                                                );
                                            }}
                                            className="inline-flex items-center gap-1 text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
                                            title={account.ban_appeal_url}
                                        >
                                            <Copy size={12} />
                                            {t('accounts.copy_appeal_url', '复制申诉链接')}
                                        </button>
                                        <span className="w-full mt-1 text-[10px] font-mono break-all select-all text-gray-500 dark:text-gray-400">
                                            {account.ban_appeal_url}
                                        </span>
                                    </>
                                )}
                            </div>
                        )}
                        {account.disabled && (
                            <div className="flex items-center gap-2 text-xs text-red-700 dark:text-red-400">
                                <AlertCircle size={14} />
                                <span className="font-semibold">{t('accounts.status.disabled')}:</span>
                                <span>{account.disabled_reason || t('common.unknown')}</span>
                            </div>
                        )}
                        {account.proxy_disabled && !account.banned && (
                            <div className="flex items-center gap-2 text-xs text-orange-700 dark:text-orange-400">
                                <AlertCircle size={14} />
                                <span className="font-semibold">{t('accounts.status.proxy_disabled')}:</span>
                                <span>{account.proxy_disabled_reason || t('common.unknown')}</span>
                            </div>
                        )}
                    </div>
                )}

                {/* Content */}
                <div className="p-6 max-h-[68vh] overflow-y-auto space-y-6">
                    {/* 优先级设置 */}
                    <div className="p-4 rounded-xl border border-gray-100 dark:border-base-200 bg-gray-50/50 dark:bg-base-200/40 flex flex-col gap-3">
                        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                            <div>
                                <div className="text-sm font-semibold text-gray-800 dark:text-gray-200 flex flex-wrap items-center gap-1.5">
                                    <span>{t('accounts.priority')}</span>
                                    <span className="text-xs px-2 py-0.5 rounded-full bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300 font-mono font-bold">
                                        {account.priority ?? 50}
                                    </span>
                                    {account.model_priorities?.gemini !== undefined && (
                                        <span className="text-xs px-2 py-0.5 rounded-full bg-indigo-100 dark:bg-indigo-900/40 text-indigo-700 dark:text-indigo-300 font-mono font-bold">
                                            G:{account.model_priorities.gemini}
                                        </span>
                                    )}
                                    {account.model_priorities?.claude !== undefined && (
                                        <span className="text-xs px-2 py-0.5 rounded-full bg-amber-100 dark:bg-amber-900/40 text-amber-700 dark:text-amber-300 font-mono font-bold">
                                            C:{account.model_priorities.claude}
                                        </span>
                                    )}
                                </div>
                                <p className="mt-1 text-xs text-gray-400 dark:text-gray-500 leading-relaxed max-w-xl">
                                    {t('accounts.priority_hint')}
                                </p>
                            </div>
                            <div className="flex items-center gap-2 self-start sm:self-auto shrink-0">
                                {(geminiPriorityInput.trim() !== '' || claudePriorityInput.trim() !== '') && (
                                    <button
                                        type="button"
                                        className="btn btn-ghost btn-xs text-gray-500 hover:text-gray-700 dark:text-gray-400"
                                        disabled={savingPriority}
                                        onClick={() => {
                                            setGeminiPriorityInput('');
                                            setClaudePriorityInput('');
                                        }}
                                        title={t('accounts.model_priority_reset_hint', '清空独立模型优先级，全部跟随账号默认优先级')}
                                    >
                                        {t('accounts.model_priority_reset', '恢复统一')}
                                    </button>
                                )}
                                <button
                                    className="btn btn-primary btn-sm px-4"
                                    onClick={savePriority}
                                    disabled={savingPriority || !isPriorityDirty}
                                >
                                    {savingPriority ? t('common.saving') : t('common.save')}
                                </button>
                            </div>
                        </div>

                        <div className="grid grid-cols-1 sm:grid-cols-3 gap-2.5 pt-2 border-t border-gray-200/60 dark:border-base-300/60">
                            <label className="flex items-center justify-between gap-2 px-3 py-2 rounded-lg bg-white dark:bg-base-100 border border-gray-200/80 dark:border-base-300">
                                <div className="min-w-0">
                                    <div className="text-xs font-semibold text-gray-700 dark:text-gray-200">
                                        {t('accounts.priority_default_label', '默认优先级')}
                                    </div>
                                    <div className="text-[10px] text-gray-400 dark:text-gray-500 truncate">
                                        {t('accounts.priority_default_sub', '全模型兜底基准')}
                                    </div>
                                </div>
                                <input
                                    id="account-priority"
                                    type="number"
                                    min={1}
                                    max={100}
                                    step={1}
                                    className="input input-bordered input-xs w-16 text-center font-mono font-bold"
                                    value={priorityInput}
                                    disabled={savingPriority}
                                    onChange={event => setPriorityInput(event.target.value)}
                                />
                            </label>

                            <label className="flex items-center justify-between gap-2 px-3 py-2 rounded-lg bg-white dark:bg-base-100 border border-gray-200/80 dark:border-base-300">
                                <div className="min-w-0">
                                    <div className="text-xs font-semibold text-indigo-700 dark:text-indigo-300">
                                        {t('accounts.priority_gemini_label', 'Gemini 优先级')}
                                    </div>
                                    <div className="text-[10px] text-gray-400 dark:text-gray-500 truncate">
                                        {t('accounts.priority_model_sub', '留空则跟随默认')}
                                    </div>
                                </div>
                                <input
                                    id="account-priority-gemini"
                                    type="number"
                                    min={1}
                                    max={100}
                                    step={1}
                                    placeholder={priorityInput || '50'}
                                    className="input input-bordered input-xs w-16 text-center font-mono font-bold"
                                    value={geminiPriorityInput}
                                    disabled={savingPriority}
                                    onChange={event => setGeminiPriorityInput(event.target.value)}
                                />
                            </label>

                            <label className="flex items-center justify-between gap-2 px-3 py-2 rounded-lg bg-white dark:bg-base-100 border border-gray-200/80 dark:border-base-300">
                                <div className="min-w-0">
                                    <div className="text-xs font-semibold text-amber-700 dark:text-amber-300">
                                        {t('accounts.priority_claude_label', 'Claude 优先级')}
                                    </div>
                                    <div className="text-[10px] text-gray-400 dark:text-gray-500 truncate">
                                        {t('accounts.priority_model_sub', '留空则跟随默认')}
                                    </div>
                                </div>
                                <input
                                    id="account-priority-claude"
                                    type="number"
                                    min={1}
                                    max={100}
                                    step={1}
                                    placeholder={priorityInput || '50'}
                                    className="input input-bordered input-xs w-16 text-center font-mono font-bold"
                                    value={claudePriorityInput}
                                    disabled={savingPriority}
                                    onChange={event => setClaudePriorityInput(event.target.value)}
                                />
                            </label>
                        </div>
                    </div>

                    {/* GCP Project ID 设置 */}
                    <div className="p-4 rounded-xl border border-gray-100 dark:border-base-200 bg-gray-50/50 dark:bg-base-200/40 flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                        <div>
                            <div className="text-sm font-semibold text-gray-800 dark:text-gray-200 flex items-center gap-1.5">
                                <span>{t('accounts.project_id', 'GCP Project ID')}</span>
                                {account.token?.project_id ? (
                                    <span className="text-xs px-2 py-0.5 rounded-md bg-emerald-100 dark:bg-emerald-900/40 text-emerald-700 dark:text-emerald-300 font-mono font-semibold">
                                        {account.token.project_id}
                                    </span>
                                ) : (
                                    <span className="text-xs px-2 py-0.5 rounded-md bg-gray-200 dark:bg-base-300 text-gray-600 dark:text-gray-400 font-mono">
                                        {t('common.none', 'None')}
                                    </span>
                                )}
                            </div>
                            <p className="mt-1 text-xs text-gray-400 dark:text-gray-500 leading-relaxed max-w-md">
                                {t('accounts.project_id_hint', 'Google Cloud Project ID associated with the enterprise/GCP account. Native Antigravity associates or defaults this upon sign-in.')}
                            </p>
                        </div>
                        <div className="flex items-center gap-2 self-start sm:self-auto shrink-0 flex-wrap">
                            <input
                                id="account-project-id"
                                type="text"
                                placeholder={t('accounts.project_id_placeholder', 'e.g. my-project-id')}
                                className="input input-bordered input-sm w-36 sm:w-44 font-mono text-xs"
                                value={projectIdInput}
                                disabled={savingProjectId || fetchingProjectId}
                                onChange={event => setProjectIdInput(event.target.value)}
                            />
                            {onAutoFetchProjectId && (
                                <button
                                    className="btn btn-outline btn-sm px-2.5 text-xs"
                                    onClick={autoFetchProjectId}
                                    disabled={savingProjectId || fetchingProjectId}
                                    title={t('accounts.auto_fetch_project_id', '自动获取')}
                                >
                                    {fetchingProjectId ? (
                                        <span className="loading loading-spinner loading-xs" />
                                    ) : (
                                        t('accounts.auto_fetch_project_id', '自动获取')
                                    )}
                                </button>
                            )}
                            <button
                                className="btn btn-primary btn-sm px-3 text-xs"
                                onClick={saveProjectId}
                                disabled={savingProjectId || fetchingProjectId || projectIdInput.trim() === (account.token?.project_id || '')}
                            >
                                {savingProjectId ? t('common.saving') : t('common.save')}
                            </button>
                        </div>
                    </div>

                    {/* Protected Models Section */}
                    {account.protected_models && account.protected_models.length > 0 && (
                        <div className="p-4 rounded-xl border border-amber-100 dark:border-amber-900/30 bg-amber-50/40 dark:bg-amber-950/20">
                            <h4 className="text-xs font-bold text-amber-800 dark:text-amber-400 uppercase tracking-widest mb-2.5 flex items-center gap-1.5">
                                <AlertCircle size={13} className="text-amber-500" />
                                {t('accounts.details.protected_models')}
                            </h4>
                            <div className="flex flex-wrap gap-1.5">
                                {account.protected_models.map(model => (
                                    <span key={model} className="px-2 py-0.5 bg-amber-100/70 dark:bg-amber-900/40 text-amber-800 dark:text-amber-300 text-[11px] font-mono border border-amber-200/60 dark:border-amber-800/50 rounded-md">
                                        {model}
                                    </span>
                                ))}
                            </div>
                        </div>
                    )}

                    <div>
                        <div className="flex gap-6 border-b border-gray-100 dark:border-base-200 mb-4">
                            <button
                                onClick={() => setActiveTab('basic')}
                                className={`pb-2.5 text-xs font-bold uppercase tracking-wider transition-colors border-b-2 ${activeTab === 'basic' ? 'border-blue-500 text-blue-600 dark:text-blue-400' : 'border-transparent text-gray-400 hover:text-gray-600 dark:hover:text-gray-300'}`}
                            >
                                {t('accounts.details.model_quota')}
                            </button>
                            {displayGroups.length > 0 && (
                                <button
                                    onClick={() => setActiveTab('detailed')}
                                    className={`pb-2.5 text-xs font-bold uppercase tracking-wider transition-colors border-b-2 flex items-center gap-1.5 ${activeTab === 'detailed' ? 'border-blue-500 text-blue-600 dark:text-blue-400' : 'border-transparent text-gray-400 hover:text-gray-600 dark:hover:text-gray-300'}`}
                                >
                                    <Bot size={13} className={activeTab === 'detailed' ? 'text-blue-500' : ''} />
                                    {t('accounts.details.quota_groups', '配额分组窗口')}
                                </button>
                            )}
                        </div>

                        {activeTab === 'basic' && (
                            <div className="grid grid-cols-1 md:grid-cols-2 gap-3.5">
                                {(() => {
                                    const uniqueLabels = new Set<string>();
                                    return sortModels(
                                        displayModels.map(model => {
                                            const config = MODEL_CONFIG[model.name.toLowerCase()];
                                            const label = model.display_name || (config?.i18nKey ? t(config.i18nKey) : (config?.label || model.name));
                                            return {
                                                id: model.name.toLowerCase(),
                                                label: label,
                                                model
                                            };
                                        })
                                    ).filter(m => {
                                        if (uniqueLabels.has(m.label)) return false;
                                        uniqueLabels.add(m.label);
                                        return true;
                                    }).map(({ model, label }) => {
                                        const p = model.percentage;
                                        const progressColor = p >= 50 ? 'bg-emerald-500' : p >= 20 ? 'bg-amber-500' : 'bg-rose-500';
                                        const badgeColor = p >= 50
                                            ? 'bg-emerald-50 text-emerald-700 dark:bg-emerald-950/40 dark:text-emerald-400 border border-emerald-200/50 dark:border-emerald-800/40'
                                            : p >= 20
                                                ? 'bg-amber-50 text-amber-700 dark:bg-amber-950/40 dark:text-amber-400 border border-amber-200/50 dark:border-amber-800/40'
                                                : 'bg-rose-50 text-rose-700 dark:bg-rose-950/40 dark:text-rose-400 border border-rose-200/50 dark:border-rose-800/40';

                                        return (
                                            <div key={model.name} className="p-3.5 rounded-xl border border-gray-100 dark:border-base-200 bg-white dark:bg-base-100/70 hover:border-blue-200 dark:hover:border-blue-900/60 hover:shadow-sm transition-all group">
                                                <div className="flex justify-between items-start gap-2 mb-2.5">
                                                    <div className="flex-1 min-w-0">
                                                        <div className="flex items-center gap-1.5">
                                                            {(() => {
                                                                const Icon = MODEL_CONFIG[model.name.toLowerCase()]?.Icon || Bot;
                                                                return <Icon size={15} className="shrink-0 text-gray-500 group-hover:text-blue-500 transition-colors" />;
                                                            })()}
                                                            <span className="text-xs font-semibold font-mono text-gray-800 dark:text-gray-200 truncate group-hover:text-blue-600 dark:group-hover:text-blue-400 transition-colors">
                                                                {label}
                                                            </span>
                                                        </div>
                                                        <div className="mt-1 flex flex-wrap items-center gap-1">
                                                            <span className="text-[10px] text-blue-600 dark:text-blue-400 font-mono bg-blue-50 dark:bg-blue-950/40 px-1.5 py-0.5 rounded border border-blue-200/50 dark:border-blue-800/40">
                                                                {model.name}
                                                            </span>
                                                            {model.thinking_budget !== undefined && (
                                                                <span className="text-[10px] text-gray-500 dark:text-gray-400 font-mono bg-gray-100 dark:bg-base-200 px-1.5 py-0.5 rounded border border-gray-200/50 dark:border-base-300">
                                                                    {t('accounts.details.thinking_budget', '思考预算')}: <span className="font-bold text-gray-700 dark:text-gray-300">{model.thinking_budget}</span>
                                                                </span>
                                                            )}
                                                        </div>
                                                    </div>
                                                    <span className={`text-[11px] font-mono font-bold px-2 py-0.5 rounded-md shrink-0 ${badgeColor}`}>
                                                        {model.percentage}%
                                                    </span>
                                                </div>

                                                {/* Progress Bar */}
                                                <div className="h-1.5 w-full bg-gray-100 dark:bg-base-200 rounded-full overflow-hidden mb-2">
                                                    <div
                                                        className={`h-full rounded-full transition-all duration-500 ${progressColor}`}
                                                        style={{ width: `${Math.min(100, Math.max(0, model.percentage))}%` }}
                                                    />
                                                </div>

                                                <div className="flex items-center justify-between text-[10px] text-gray-400 dark:text-gray-500 font-mono">
                                                    <div className="flex items-center gap-1 truncate">
                                                        <Clock size={11} className="shrink-0" />
                                                        <span className="truncate">{formatDate(model.reset_time) || t('common.unknown')}</span>
                                                    </div>
                                                    {model.reset_time && (
                                                        <span className="shrink-0 font-medium text-gray-500 dark:text-gray-400 ml-1">
                                                            {formatTimeRemaining(model.reset_time)}
                                                        </span>
                                                    )}
                                                </div>
                                            </div>
                                        );
                                    });
                                })() || (
                                    <div className="col-span-2 py-10 text-center text-gray-400 flex flex-col items-center">
                                        <AlertCircle className="w-8 h-8 mb-2 opacity-20" />
                                        <span>{t('accounts.no_data')}</span>
                                    </div>
                                )}
                            </div>
                        )}

                        {/* Quota Groups Section */}
                        {activeTab === 'detailed' && displayGroups.length > 0 && (
                            <div className="flex flex-col gap-4">
                                {displayGroups.map((group, idx) => (
                                    <div key={idx} className="p-4 rounded-xl border border-blue-100/70 dark:border-blue-900/30 bg-blue-50/20 dark:bg-blue-900/10">
                                        <div className="font-semibold text-xs text-blue-900 dark:text-blue-300 mb-3 flex flex-wrap justify-between items-center gap-1.5">
                                            <span className="font-bold text-sm">
                                                {(() => {
                                                    const raw = group.display_name || '';
                                                    if (raw.includes('Gemini Models')) return t('accounts.quota_group_gemini', 'Gemini 模型');
                                                    if (raw.includes('Claude and GPT')) return t('accounts.quota_group_claude_gpt', 'Claude 与 GPT 模型');
                                                    if (raw.includes('Argon')) return t('accounts.quota_group_argon', 'Gemini 4 Argon');
                                                    return raw;
                                                })()}
                                            </span>
                                            {group.description && (
                                                <span className="text-[11px] font-normal text-blue-800/80 dark:text-blue-300/80 truncate max-w-md font-mono" title={group.description}>
                                                    {group.description.startsWith('Models within this group:')
                                                        ? t('accounts.details.group_models_desc', {
                                                            defaultValue: '组内包含模型: {{models}}',
                                                            models: group.description.replace(/^Models within this group:\s*/i, ''),
                                                        })
                                                        : group.description}
                                                </span>
                                            )}
                                        </div>
                                        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                            {(group.buckets || []).map((bucket, bIdx) => {
                                                const percentage = Math.round(bucket.remaining_fraction * 100);
                                                const bColor = percentage >= 50
                                                    ? 'bg-emerald-50 text-emerald-700 dark:bg-emerald-950/40 dark:text-emerald-400 border border-emerald-200/50'
                                                    : percentage >= 20
                                                        ? 'bg-amber-50 text-amber-700 dark:bg-amber-950/40 dark:text-amber-400 border border-amber-200/50'
                                                        : 'bg-rose-50 text-rose-700 dark:bg-rose-950/40 dark:text-rose-400 border border-rose-200/50';

                                                const windowLabel = (() => {
                                                    const w = (bucket.window || '').toUpperCase();
                                                    if (w === 'WEEKLY') return t('accounts.quota_window_weekly_desc', '7天周配额 (WEEKLY)');
                                                    if (w === '5H') return t('accounts.quota_window_5h_desc', '5小时配额 (5H)');
                                                    if (w === '1H') return t('accounts.quota_window_1h_desc', '1小时配额 (1H)');
                                                    return bucket.window;
                                                })();

                                                return (
                                                    <div key={bIdx} className="bg-white dark:bg-base-200 p-3 rounded-lg border border-gray-100 dark:border-white/5 shadow-sm">
                                                        <div className="flex justify-between items-center mb-2">
                                                            <span className="text-xs font-bold text-gray-700 dark:text-gray-300 tracking-wider">
                                                                {windowLabel}
                                                            </span>
                                                            <span className={`text-[10px] font-bold px-1.5 py-0.5 rounded font-mono ${bColor}`}>
                                                                {percentage}%
                                                            </span>
                                                        </div>
                                                        {/* Progress bar */}
                                                        <div className="h-1.5 w-full bg-gray-100 dark:bg-base-300 rounded-full overflow-hidden mb-2">
                                                            <div
                                                                className={`h-full rounded-full transition-all duration-500 ${percentage >= 50 ? 'bg-emerald-500' : percentage >= 20 ? 'bg-amber-400' : 'bg-rose-500'}`}
                                                                style={{ width: `${percentage}%` }}
                                                            />
                                                        </div>
                                                        <div className="flex justify-between items-center text-[10px] text-gray-500 font-mono">
                                                            <div className="flex items-center gap-1 truncate">
                                                                <Clock size={10} className="shrink-0" />
                                                                <span className="truncate">{formatDate(bucket.reset_time) || t('common.unknown')}</span>
                                                            </div>
                                                            {bucket.reset_time && (
                                                                <span className="shrink-0 font-medium text-gray-600 dark:text-gray-400 ml-1">
                                                                    {formatTimeRemaining(bucket.reset_time)}
                                                                </span>
                                                            )}
                                                        </div>
                                                    </div>
                                                );
                                            })}
                                        </div>
                                    </div>
                                ))}
                            </div>
                        )}
                    </div>
                </div>
            </div>
            <div className="modal-backdrop bg-black/40 backdrop-blur-sm" onClick={onClose}></div>
        </div>,
        document.body
    );
}
