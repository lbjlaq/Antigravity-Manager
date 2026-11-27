import { useState, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Target, Plus, Trash2, Edit2, X, Zap, Lock, Bot } from 'lucide-react';
import { ModelAccountBinding, ModelBindingPolicy } from '../../../types/config';
import { useAccountStore } from '../../../stores/useAccountStore';
import { getAccountTier } from '../../../types/account';
import { showToast } from '../../common/ToastContainer';

interface ModelAccountBindingManagerProps {
    bindings?: ModelAccountBinding[];
    onChange: (bindings: ModelAccountBinding[]) => void;
    onSave?: () => Promise<void>;
}

export default function ModelAccountBindingManager({
    bindings = [],
    onChange,
    onSave,
}: ModelAccountBindingManagerProps) {
    const { t } = useTranslation();
    const { accounts } = useAccountStore();

    const [isEditing, setIsEditing] = useState(false);
    const [editingId, setEditingId] = useState<string | null>(null);
    const [pattern, setPattern] = useState('');
    const [policy, setPolicy] = useState<ModelBindingPolicy>('exclusive');
    const [selectedAccountIds, setSelectedAccountIds] = useState<string[]>([]);
    const [accountSearch, setAccountSearch] = useState('');
    const [saving, setSaving] = useState(false);

    // Compute specificity score of pattern
    const patternSpecificity = useMemo(() => {
        const trimmed = pattern.trim();
        return trimmed.length - (trimmed.match(/\*/g) || []).length - (trimmed.match(/\?/g) || []).length;
    }, [pattern]);

    const activeAccounts = useMemo(() => {
        return accounts.filter(a => !a.disabled && !a.banned);
    }, [accounts]);

    const filteredAccounts = useMemo(() => {
        if (!accountSearch.trim()) return activeAccounts;
        const q = accountSearch.toLowerCase();
        return activeAccounts.filter(a => a.email.toLowerCase().includes(q) || a.id.toLowerCase().includes(q));
    }, [activeAccounts, accountSearch]);

    const openAdd = () => {
        setEditingId(null);
        setPattern('');
        setPolicy('exclusive');
        setSelectedAccountIds(activeAccounts.length > 0 ? [activeAccounts[0].id] : []);
        setAccountSearch('');
        setIsEditing(true);
    };

    const openEdit = (binding: ModelAccountBinding) => {
        setEditingId(binding.id);
        setPattern(binding.pattern);
        setPolicy(binding.policy);
        setSelectedAccountIds([...binding.account_ids]);
        setAccountSearch('');
        setIsEditing(true);
    };

    const cancelEdit = () => {
        setIsEditing(false);
        setEditingId(null);
    };

    const toggleAccountSelection = (accountId: string) => {
        setSelectedAccountIds(prev =>
            prev.includes(accountId) ? prev.filter(id => id !== accountId) : [...prev, accountId]
        );
    };

    const selectAllAccounts = () => {
        setSelectedAccountIds(activeAccounts.map(a => a.id));
    };

    const deselectAllAccounts = () => {
        setSelectedAccountIds([]);
    };

    const applyPreset = (presetPattern: string) => {
        setPattern(presetPattern);
    };

    const handleSaveBinding = async () => {
        const cleanPattern = pattern.trim().toLowerCase();
        if (!cleanPattern) {
            showToast(t('proxy.model_bindings.pattern_placeholder', '请输入模型匹配模式'), 'error');
            return;
        }
        if (selectedAccountIds.length === 0) {
            showToast(t('proxy.model_bindings.accounts_selected', { defaultValue: '请至少选择一个绑定账号', count: 0 }), 'error');
            return;
        }

        const newBinding: ModelAccountBinding = {
            id: editingId || `mb_${Date.now()}_${Math.random().toString(36).substring(2, 7)}`,
            pattern: cleanPattern,
            account_ids: selectedAccountIds,
            policy,
            enabled: true,
        };

        const nextBindings = editingId
            ? bindings.map(b => (b.id === editingId ? newBinding : b))
            : [...bindings, newBinding];

        onChange(nextBindings);
        setIsEditing(false);
        setEditingId(null);
        showToast(t('common.saved', '已保存'), 'success');

        if (onSave) {
            setSaving(true);
            try {
                await onSave();
            } finally {
                setSaving(false);
            }
        }
    };

    const handleDeleteBinding = async (id: string) => {
        const nextBindings = bindings.filter(b => b.id !== id);
        onChange(nextBindings);
        showToast(t('common.saved', '已保存'), 'success');
        if (onSave) {
            await onSave();
        }
    };

    const handleToggleEnabled = async (id: string, enabled: boolean) => {
        const nextBindings = bindings.map(b => (b.id === id ? { ...b, enabled } : b));
        onChange(nextBindings);
        showToast(t('common.saved', '已保存'), 'success');
        if (onSave) {
            await onSave();
        }
    };

    return (
        <div className="bg-gray-50/50 dark:bg-white/5 p-4 rounded-xl border border-gray-100 dark:border-white/5 flex flex-col gap-3">
            {/* Header */}
            <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                <div>
                    <h3 className="text-xs font-bold text-gray-900 dark:text-white flex items-center gap-1.5">
                        <Target size={15} className="text-indigo-500" />
                        <span>{t('proxy.model_bindings.title', '全局模型-账号定向绑定')}</span>
                        {bindings.length > 0 && (
                            <span className="text-[10px] px-2 py-0.5 rounded-full bg-indigo-100 dark:bg-indigo-900/40 text-indigo-700 dark:text-indigo-300 font-mono font-bold">
                                {bindings.length}
                            </span>
                        )}
                    </h3>
                    <p className="text-[11px] text-gray-500 dark:text-gray-400 mt-0.5">
                        {t('proxy.model_bindings.subtitle', '针对特定模型或通配符规则，将请求锁定或优先分配给指定账号集合。')}
                    </p>
                </div>
                {!isEditing && (
                    <button
                        type="button"
                        onClick={openAdd}
                        className="btn btn-xs btn-primary gap-1 self-start sm:self-auto text-xs"
                    >
                        <Plus size={13} />
                        {t('proxy.model_bindings.add_rule', '添加定向绑定')}
                    </button>
                )}
            </div>

            {/* Editing / Adding Form */}
            {isEditing && (
                <div className="p-3.5 rounded-xl border border-indigo-200/80 dark:border-indigo-800/60 bg-white dark:bg-base-100 shadow-sm flex flex-col gap-3 animate-in fade-in duration-150">
                    <div className="flex items-center justify-between pb-2 border-b border-gray-100 dark:border-base-200">
                        <span className="text-xs font-bold text-indigo-700 dark:text-indigo-300 flex items-center gap-1.5">
                            <Target size={13} />
                            {editingId ? t('common.edit', '编辑绑定规则') : t('proxy.model_bindings.add_rule', '添加定向绑定')}
                        </span>
                        <button
                            type="button"
                            onClick={cancelEdit}
                            className="p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 rounded"
                        >
                            <X size={14} />
                        </button>
                    </div>

                    {/* Pattern Input & Presets */}
                    <div className="flex flex-col gap-1.5">
                        <div className="flex items-center justify-between">
                            <label className="text-xs font-medium text-gray-700 dark:text-gray-300">
                                {t('proxy.model_bindings.pattern_label', '模型匹配模式 (通配符)')}
                            </label>
                            {pattern.trim() && (
                                <span className="text-[10px] text-gray-400 font-mono" title={t('proxy.model_bindings.specificity_tip', { spec: patternSpecificity })}>
                                    {t('proxy.model_bindings.specificity_tip', { spec: patternSpecificity, defaultValue: `特异度评分: ${patternSpecificity}` })}
                                </span>
                            )}
                        </div>
                        <input
                            type="text"
                            placeholder={t('proxy.model_bindings.pattern_placeholder', '如 *claude*, *gemini*pro*, *opus*')}
                            className="input input-bordered input-sm w-full font-mono text-xs"
                            value={pattern}
                            onChange={e => setPattern(e.target.value)}
                        />
                        <div className="flex items-center gap-1.5 flex-wrap pt-0.5">
                            <span className="text-[10px] text-gray-400 dark:text-gray-500">快捷预设:</span>
                            {['*claude*', '*gemini*pro*', '*gemini*flash*', '*opus*', '*sonnet*'].map(p => (
                                <button
                                    key={p}
                                    type="button"
                                    onClick={() => applyPreset(p)}
                                    className="btn btn-2xs btn-outline border-dashed text-[10px] font-mono px-1.5 py-0.5"
                                >
                                    {p}
                                </button>
                            ))}
                        </div>
                    </div>

                    {/* Policy Radio / Pills */}
                    <div className="flex flex-col gap-1.5 pt-1">
                        <label className="text-xs font-medium text-gray-700 dark:text-gray-300">
                            {t('proxy.model_bindings.policy_label', '调度策略')}
                        </label>
                        <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                            <div
                                onClick={() => setPolicy('exclusive')}
                                className={`p-2.5 rounded-lg border cursor-pointer transition-all ${
                                    policy === 'exclusive'
                                        ? 'bg-purple-50/70 border-purple-400 dark:bg-purple-950/30 dark:border-purple-600 ring-1 ring-purple-400/40'
                                        : 'bg-gray-50 dark:bg-base-200 border-gray-200 dark:border-base-300 hover:bg-gray-100 dark:hover:bg-base-300'
                                }`}
                            >
                                <div className="flex items-center gap-1.5 text-xs font-bold text-purple-700 dark:text-purple-300">
                                    <Lock size={12} />
                                    <span>{t('proxy.model_bindings.policy_exclusive', '专属锁定 (Exclusive)')}</span>
                                </div>
                                <p className="text-[10px] text-gray-500 dark:text-gray-400 mt-1 leading-relaxed">
                                    {t('proxy.model_bindings.policy_exclusive_desc', '仅由指定账号处理；全部满额或故障时直接失败，绝不外溢至其他账号')}
                                </p>
                            </div>

                            <div
                                onClick={() => setPolicy('priority')}
                                className={`p-2.5 rounded-lg border cursor-pointer transition-all ${
                                    policy === 'priority'
                                        ? 'bg-indigo-50/70 border-indigo-400 dark:bg-indigo-950/30 dark:border-indigo-600 ring-1 ring-indigo-400/40'
                                        : 'bg-gray-50 dark:bg-base-200 border-gray-200 dark:border-base-300 hover:bg-gray-100 dark:hover:bg-base-300'
                                }`}
                            >
                                <div className="flex items-center gap-1.5 text-xs font-bold text-indigo-700 dark:text-indigo-300">
                                    <Zap size={12} />
                                    <span>{t('proxy.model_bindings.policy_priority', '优先调度 (Priority)')}</span>
                                </div>
                                <p className="text-[10px] text-gray-500 dark:text-gray-400 mt-1 leading-relaxed">
                                    {t('proxy.model_bindings.policy_priority_desc', '优先调度指定账号；当指定账号满额或冷却时，优雅降级回退到常规账号池')}
                                </p>
                            </div>
                        </div>
                    </div>

                    {/* Accounts Selection */}
                    <div className="flex flex-col gap-1.5 pt-1">
                        <div className="flex items-center justify-between">
                            <label className="text-xs font-medium text-gray-700 dark:text-gray-300 flex items-center gap-1.5">
                                <Bot size={13} className="text-blue-500" />
                                <span>{t('proxy.model_bindings.accounts_label', '绑定账号集合')}</span>
                                <span className="text-[11px] font-mono text-indigo-600 dark:text-indigo-400 font-bold">
                                    ({selectedAccountIds.length}/{activeAccounts.length})
                                </span>
                            </label>
                            <div className="flex items-center gap-2">
                                <button
                                    type="button"
                                    onClick={selectAllAccounts}
                                    className="text-[10px] text-blue-600 hover:underline"
                                >
                                    {t('proxy.model_bindings.select_all', '全选')}
                                </button>
                                <span className="text-gray-300 dark:text-gray-600 text-xs">|</span>
                                <button
                                    type="button"
                                    onClick={deselectAllAccounts}
                                    className="text-[10px] text-gray-500 hover:underline"
                                >
                                    {t('proxy.model_bindings.deselect_all', '清空选择')}
                                </button>
                            </div>
                        </div>

                        {/* Search input if accounts > 5 */}
                        {activeAccounts.length > 5 && (
                            <input
                                type="text"
                                placeholder={t('common.search', '搜索账号邮箱...')}
                                className="input input-bordered input-xs w-full text-xs font-mono"
                                value={accountSearch}
                                onChange={e => setAccountSearch(e.target.value)}
                            />
                        )}

                        <div className="max-h-48 overflow-y-auto border border-gray-200 dark:border-base-300 rounded-lg p-2 bg-gray-50/50 dark:bg-base-200/50 flex flex-col gap-1.5 custom-scrollbar">
                            {filteredAccounts.length === 0 ? (
                                <div className="py-4 text-center text-xs text-gray-400">
                                    {t('accounts.no_data', '暂无可用账号')}
                                </div>
                            ) : (
                                filteredAccounts.map(acc => {
                                    const isSelected = selectedAccountIds.includes(acc.id);
                                    const tier = getAccountTier(acc);
                                    return (
                                        <div
                                            key={acc.id}
                                            onClick={() => toggleAccountSelection(acc.id)}
                                            className={`flex items-center justify-between px-2.5 py-1.5 rounded-md cursor-pointer transition-colors text-xs ${
                                                isSelected
                                                    ? 'bg-blue-50 dark:bg-blue-950/40 border border-blue-200 dark:border-blue-800/50'
                                                    : 'hover:bg-gray-100 dark:hover:bg-base-100 border border-transparent'
                                            }`}
                                        >
                                            <div className="flex items-center gap-2 min-w-0">
                                                <input
                                                    type="checkbox"
                                                    checked={isSelected}
                                                    onChange={() => {}}
                                                    className="checkbox checkbox-xs checkbox-primary"
                                                />
                                                <span className="font-mono truncate text-gray-800 dark:text-gray-200" title={acc.email}>
                                                    {acc.email}
                                                </span>
                                            </div>
                                            <div className="flex items-center gap-1.5 shrink-0 ml-2">
                                                <span className={`text-[9px] font-bold px-1.5 py-0.5 rounded uppercase font-mono ${
                                                    tier === 'ultra'
                                                        ? 'bg-purple-100 text-purple-700 dark:bg-purple-950/60 dark:text-purple-300'
                                                        : tier === 'pro'
                                                        ? 'bg-blue-100 text-blue-700 dark:bg-blue-950/60 dark:text-blue-300'
                                                        : 'bg-gray-200 text-gray-600 dark:bg-base-300 dark:text-gray-400'
                                                }`}>
                                                    {tier}
                                                </span>
                                            </div>
                                        </div>
                                    );
                                })
                            )}
                        </div>
                    </div>

                    {/* Actions */}
                    <div className="flex items-center justify-end gap-2 pt-2 border-t border-gray-100 dark:border-base-200">
                        <button
                            type="button"
                            onClick={cancelEdit}
                            className="btn btn-xs btn-ghost"
                            disabled={saving}
                        >
                            {t('common.cancel', '取消')}
                        </button>
                        <button
                            type="button"
                            onClick={handleSaveBinding}
                            className="btn btn-xs btn-primary px-3"
                            disabled={saving || !pattern.trim() || selectedAccountIds.length === 0}
                        >
                            {saving ? t('common.saving', '保存中...') : t('common.save', '保存')}
                        </button>
                    </div>
                </div>
            )}

            {/* Bindings List */}
            {bindings.length === 0 && !isEditing ? (
                <div className="py-6 text-center text-xs text-gray-400 dark:text-gray-500 border border-dashed border-gray-200 dark:border-base-300 rounded-lg flex flex-col items-center justify-center gap-1.5">
                    <Target size={20} className="opacity-40" />
                    <span>{t('proxy.model_bindings.empty', '暂无定向绑定规则，所有模型遵循全局负载均衡与账号准入策略。')}</span>
                </div>
            ) : (
                <div className="flex flex-col gap-2">
                    {bindings.map(binding => {
                        const boundAccounts = accounts.filter(a => binding.account_ids.includes(a.id));
                        const isExclusive = binding.policy === 'exclusive';
                        return (
                            <div
                                key={binding.id}
                                className={`flex flex-col sm:flex-row sm:items-center justify-between p-2.5 rounded-lg border transition-all gap-2 ${
                                    binding.enabled
                                        ? 'bg-white dark:bg-base-100 border-gray-200/80 dark:border-base-300 shadow-2xs'
                                        : 'bg-gray-50/50 dark:bg-base-200/30 border-gray-200/40 dark:border-base-300/40 opacity-60'
                                }`}
                            >
                                <div className="flex items-start sm:items-center gap-2.5 min-w-0 flex-1">
                                    <input
                                        type="checkbox"
                                        checked={binding.enabled}
                                        onChange={e => handleToggleEnabled(binding.id, e.target.checked)}
                                        className="toggle toggle-xs toggle-primary mt-0.5 sm:mt-0 shrink-0"
                                        title={t('proxy.model_bindings.enabled_label', '启用状态')}
                                    />
                                    <div className="flex items-center gap-1.5 flex-wrap min-w-0">
                                        <span className="font-mono text-xs font-bold text-gray-900 dark:text-white px-2 py-0.5 rounded bg-gray-100 dark:bg-base-200 border border-gray-200/60 dark:border-base-300">
                                            {binding.pattern}
                                        </span>
                                        <span
                                            className={`text-[10px] px-2 py-0.5 rounded font-bold flex items-center gap-1 ${
                                                isExclusive
                                                    ? 'bg-purple-100 text-purple-700 dark:bg-purple-950/50 dark:text-purple-300'
                                                    : 'bg-indigo-100 text-indigo-700 dark:bg-indigo-950/50 dark:text-indigo-300'
                                            }`}
                                        >
                                            {isExclusive ? <Lock size={10} /> : <Zap size={10} />}
                                            {isExclusive ? 'Exclusive 专属' : 'Priority 优先'}
                                        </span>
                                        <div className="flex items-center gap-1 flex-wrap">
                                            <span className="text-[11px] text-gray-500 dark:text-gray-400 font-mono">
                                                ➔ {boundAccounts.length > 0 ? (
                                                    boundAccounts.map(a => a.email.split('@')[0]).join(', ') + (boundAccounts.length > 2 ? ` 等 ${boundAccounts.length} 个账号` : '')
                                                ) : (
                                                    `${binding.account_ids.length} 个账号`
                                                )}
                                            </span>
                                        </div>
                                    </div>
                                </div>

                                <div className="flex items-center gap-1 self-end sm:self-auto shrink-0">
                                    <button
                                        type="button"
                                        onClick={() => openEdit(binding)}
                                        className="p-1 text-gray-400 hover:text-blue-600 dark:hover:text-blue-400 rounded transition-colors"
                                        title={t('common.edit', '编辑')}
                                    >
                                        <Edit2 size={13} />
                                    </button>
                                    <button
                                        type="button"
                                        onClick={() => handleDeleteBinding(binding.id)}
                                        className="p-1 text-gray-400 hover:text-rose-600 dark:hover:text-rose-400 rounded transition-colors"
                                        title={t('common.delete', '删除')}
                                    >
                                        <Trash2 size={13} />
                                    </button>
                                </div>
                            </div>
                        );
                    })}
                </div>
            )}
        </div>
    );
}
