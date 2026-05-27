import React, { useState, useEffect, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus, Trash2, RefreshCw, Copy, Activity, User, Settings, Shield, Clock, Users, Check, Sparkles, X, Zap, ArrowUp, ArrowDown } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import { request as invoke } from '../utils/request';
import { showToast } from '../components/common/ToastContainer';
import { copyToClipboard } from '../utils/clipboard';
import { MODEL_CONFIG } from '../config/modelConfig';

interface UserToken {
    id: string;
    token: string;
    username: string;
    description?: string;
    enabled: boolean;
    expires_type: string;
    expires_at?: number;
    max_ips: number;
    curfew_start?: string;
    curfew_end?: string;
    created_at: number;
    updated_at: number;
    last_used_at?: number;
    total_requests: number;
    total_tokens_used: number;
    allowed_models: string[];
}

interface UserTokenStats {
    total_tokens: number;
    active_tokens: number;
    total_users: number;
    today_requests: number;
    today_tokens: number;
}

// interface CreateTokenRequest omitted as it's not explicitly used for typing variables

const UserToken: React.FC = () => {
    const { t } = useTranslation();
    const [tokens, setTokens] = useState<UserToken[]>([]);
    const [stats, setStats] = useState<UserTokenStats | null>(null);
    const [loading, setLoading] = useState(false);
    const [showCreateModal, setShowCreateModal] = useState(false);
    const [creating, setCreating] = useState(false);

    // Edit State
    const [showEditModal, setShowEditModal] = useState(false);
    const [editingToken, setEditingToken] = useState<UserToken | null>(null);
    const [editUsername, setEditUsername] = useState('');
    const [editDesc, setEditDesc] = useState('');
    const [editMaxIps, setEditMaxIps] = useState(0);
    const [editCurfewStart, setEditCurfewStart] = useState('');
    const [editCurfewEnd, setEditCurfewEnd] = useState('');
    const [updating, setUpdating] = useState(false);
    const [editAllowedModels, setEditAllowedModels] = useState<string[]>([]);

    // 排序状态
    const [sortField, setSortField] = useState<'usage' | null>(null);
    const [sortOrder, setSortOrder] = useState<'asc' | 'desc'>('desc');

    // Create Form State
    const [newUsername, setNewUsername] = useState('');
    const [newDesc, setNewDesc] = useState('');
    const [newExpiresType, setNewExpiresType] = useState('month'); // day, week, month, never, custom
    const [newMaxIps, setNewMaxIps] = useState(0);
    const [newCurfewStart, setNewCurfewStart] = useState('');
    const [newCurfewEnd, setNewCurfewEnd] = useState('');
    const [newCustomExpires, setNewCustomExpires] = useState(''); // datetime-local value
    const [newAllowedModels, setNewAllowedModels] = useState<string[]>([]);

    const loadData = async () => {
        setLoading(true);
        try {
            const [tokensData, statsData] = await Promise.all([
                invoke<UserToken[]>('list_user_tokens'),
                invoke<UserTokenStats>('get_user_token_summary')
            ]);
            setTokens(tokensData);
            setStats(statsData);
        } catch (e) {
            console.error('Failed to load user tokens', e);
            showToast(t('user_token.load_failed') || '加载数据失败', 'error');
        } finally {
            setLoading(false);
        }
    };

    // 按使用量排序切换
    const toggleUsageSort = () => {
        if (sortField !== 'usage') {
            setSortField('usage');
            setSortOrder('desc');
        } else {
            setSortOrder(sortOrder === 'desc' ? 'asc' : 'desc');
        }
    };

    // 根据排序状态处理 tokens
    const sortedTokens = useMemo(() => {
        if (sortField === 'usage') {
            return [...tokens].sort((a, b) => {
                return sortOrder === 'desc'
                    ? b.total_requests - a.total_requests
                    : a.total_requests - b.total_requests;
            });
        }
        return tokens;
    }, [tokens, sortField, sortOrder]);

    useEffect(() => {
        loadData();
    }, []);

    const handleCreate = async () => {
        if (!newUsername) {
            showToast(t('user_token.username_required') || '用户名不能为空', 'error');
            return;
        }

        // 验证自定义时间
        if (newExpiresType === 'custom' && !newCustomExpires) {
            showToast(t('user_token.custom_expires_required') || '请选择自定义过期时间', 'error');
            return;
        }

        setCreating(true);
        try {
            // 计算自定义过期时间戳
            const customExpiresAt = newExpiresType === 'custom' && newCustomExpires
                ? Math.floor(new Date(newCustomExpires).getTime() / 1000)
                : undefined;

            await invoke('create_user_token', {
                request: {
                    username: newUsername,
                    expires_type: newExpiresType,
                    description: newDesc || null,
                    max_ips: newMaxIps,
                    curfew_start: newCurfewStart || null,
                    curfew_end: newCurfewEnd || null,
                    custom_expires_at: customExpiresAt || null,
                    allowed_models: newAllowedModels
                }
            });
            showToast(t('common.create_success') || 'Created successfully', 'success');
            setShowCreateModal(false);
            setNewUsername('');
            setNewDesc('');
            setNewExpiresType('month');
            setNewMaxIps(0);
            setNewCurfewStart('');
            setNewCurfewEnd('');
            setNewCustomExpires('');
            setNewAllowedModels([]);
            loadData();
        } catch (e) {
            console.error('Failed to create token', e);
            showToast(String(e), 'error');
        } finally {
            setCreating(false);
        }
    };

    const handleDelete = async (id: string) => {
        try {
            await invoke('delete_user_token', { id });
            showToast(t('common.delete_success') || 'Deleted successfully', 'success');
            loadData();
        } catch (e) {
            showToast(String(e), 'error');
        }
    };

    const handleEdit = (token: UserToken) => {
        console.log('Editing token:', token); // 调试日志
        setEditingToken(token);
        setEditUsername(token.username);
        setEditDesc(token.description || '');
        setEditMaxIps(token.max_ips ?? 0);
        setEditCurfewStart(token.curfew_start ?? '');
        setEditCurfewEnd(token.curfew_end ?? '');
        setEditAllowedModels(token.allowed_models || []);
        setShowEditModal(true);
    };

    const handleUpdate = async () => {
        if (!editingToken) return;
        if (!editUsername) {
            showToast(t('user_token.username_required') || 'Username is required', 'error');
            return;
        }

        setUpdating(true);
        try {
            await invoke('update_user_token', {
                id: editingToken.id,
                request: {
                    username: editUsername,
                    description: editDesc || undefined,
                    max_ips: editMaxIps,
                    curfew_start: editCurfewStart === '' ? null : editCurfewStart,
                    curfew_end: editCurfewEnd === '' ? null : editCurfewEnd,
                    allowed_models: editAllowedModels
                }
            });
            showToast(t('common.update_success') || 'Updated successfully', 'success');
            setShowEditModal(false);
            setEditingToken(null);
            loadData();
        } catch (e) {
            console.error('Failed to update token', e);
            showToast(String(e), 'error');
        } finally {
            setUpdating(false);
        }
    };

    const handleRenew = async (id: string, type: string) => {
        try {
            await invoke('renew_user_token', { id, expiresType: type });
            showToast(t('user_token.renew_success') || 'Renewed successfully', 'success');
            loadData();
        } catch (e) {
            showToast(String(e), 'error');
        }
    };

    const handleCopyToken = async (text: string) => {
        const success = await copyToClipboard(text);
        if (success) {
            showToast(t('common.copied') || '已复制到剪贴板', 'success');
        } else {
            showToast(t('user_token.copy_failed') || '复制失败', 'error');
        }
    };

    const formatTime = (ts?: number) => {
        if (!ts) return '-';
        return new Date(ts * 1000).toLocaleString();
    };

    const getExpiresLabel = (type: string) => {
        switch (type) {
            case 'day': return t('user_token.expires_day', { defaultValue: '1 Day' });
            case 'week': return t('user_token.expires_week', { defaultValue: '1 Week' });
            case 'month': return t('user_token.expires_month', { defaultValue: '1 Month' });
            case 'never': return t('user_token.expires_never', { defaultValue: 'Never' });
            case 'custom': return t('user_token.expires_custom', { defaultValue: 'Custom' });
            default: return type;
        }
    };

    // Calculate expiration status style
    const getExpiresStatus = (expiresAt?: number) => {
        if (!expiresAt) return 'text-green-500';
        const now = Date.now() / 1000;
        if (expiresAt < now) return 'text-red-500 font-bold';
        if (expiresAt - now < 86400 * 3) return 'text-orange-500'; // Less than 3 days
        return 'text-green-500';
    };

    return (
        <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            className="h-full flex flex-col p-5 gap-5 max-w-7xl mx-auto w-full"
        >
            {/* Header */}
            <div className="flex justify-between items-center">
                <h1 className="text-2xl font-bold text-gray-900 dark:text-white flex items-center gap-2">
                    <div className="p-2 bg-purple-50 dark:bg-purple-900/20 rounded-lg">
                        <User className="text-purple-500 w-5 h-5" />
                    </div>
                    {t('user_token.title', { defaultValue: 'User Tokens' })}
                </h1>

                <div className="flex items-center gap-2">
                    <button
                        onClick={() => loadData()}
                        className={`p-2 hover:bg-gray-100 dark:hover:bg-base-200 rounded-lg transition-colors ${loading ? 'text-blue-500' : 'text-gray-500'}`}
                        title={t('common.refresh') || 'Refresh'}
                    >
                        <RefreshCw size={18} className={loading ? 'animate-spin' : ''} />
                    </button>
                    <button
                        onClick={() => setShowCreateModal(true)}
                        className="px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white text-sm font-medium rounded-lg transition-all flex items-center gap-2 shadow-sm shadow-blue-500/20"
                    >
                        <Plus size={16} />
                        <span>{t('user_token.create', { defaultValue: '创建 Token' })}</span>
                    </button>
                </div>
            </div>

            {/* Stats Cards Row */}
            <div className="grid grid-cols-2 md:grid-cols-5 gap-4">
                <motion.div
                    whileHover={{ y: -2 }}
                    className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200"
                >
                    <div className="flex items-center justify-between mb-2">
                        <div className="p-1.5 bg-blue-50 dark:bg-blue-900/20 rounded-md">
                            <Users className="w-4 h-4 text-blue-500" />
                        </div>
                    </div>
                    <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{stats?.total_users || 0}</div>
                    <div className="text-xs text-gray-500 dark:text-gray-400">{t('user_token.total_users', { defaultValue: '用户总数' })}</div>
                </motion.div>

                <motion.div
                    whileHover={{ y: -2 }}
                    className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200"
                >
                    <div className="flex items-center justify-between mb-2">
                        <div className="p-1.5 bg-green-50 dark:bg-green-900/20 rounded-md">
                            <Activity className="w-4 h-4 text-green-500" />
                        </div>
                    </div>
                    <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{stats?.active_tokens || 0}</div>
                    <div className="text-xs text-gray-500 dark:text-gray-400">{t('user_token.active_tokens', { defaultValue: '活跃 Token' })}</div>
                </motion.div>

                <motion.div
                    whileHover={{ y: -2 }}
                    className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200"
                >
                    <div className="flex items-center justify-between mb-2">
                        <div className="p-1.5 bg-purple-50 dark:bg-purple-900/20 rounded-md">
                            <Clock className="w-4 h-4 text-purple-500" />
                        </div>
                    </div>
                    <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{stats?.total_tokens || 0}</div>
                    <div className="text-xs text-gray-500 dark:text-gray-400">{t('user_token.total_created', { defaultValue: '累计创建' })}</div>
                </motion.div>

                <motion.div
                    whileHover={{ y: -2 }}
                    className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200"
                >
                    <div className="flex items-center justify-between mb-2">
                        <div className="p-1.5 bg-orange-50 dark:bg-orange-900/20 rounded-md">
                            <Shield className="w-4 h-4 text-orange-500" />
                        </div>
                    </div>
                    <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{stats?.today_requests || 0}</div>
                    <div className="text-xs text-gray-500 dark:text-gray-400">{t('user_token.today_requests', { defaultValue: '今日请求数' })}</div>
                </motion.div>

                <motion.div
                    whileHover={{ y: -2 }}
                    className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200"
                >
                    <div className="flex items-center justify-between mb-2">
                        <div className="p-1.5 bg-yellow-50 dark:bg-yellow-900/20 rounded-md">
                            <Zap className="w-4 h-4 text-yellow-500" />
                        </div>
                    </div>
                    <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">
                        {stats?.today_tokens ? (stats.today_tokens / 1000).toFixed(1) + 'k' : '0'}
                    </div>
                    <div className="text-xs text-gray-500 dark:text-gray-400">{t('user_token.today_tokens', { defaultValue: '今日 Tokens' })}</div>
                </motion.div>
            </div>

            {/* Token List */}
            <div className="flex-1 overflow-auto bg-white dark:bg-base-100 rounded-2xl shadow-sm border border-gray-100 dark:border-base-200">
                <table className="table table-pin-rows">
                    <thead>
                        <tr className="bg-gray-50/50 dark:bg-base-200/50">
                            <th className="bg-transparent text-gray-500 font-medium py-4">{t('user_token.username', { defaultValue: '用户名' })}</th>
                            <th className="bg-transparent text-gray-500 font-medium py-4">{t('user_token.token', { defaultValue: 'Token' })}</th>
                            <th className="bg-transparent text-gray-500 font-medium py-4">{t('user_token.expires', { defaultValue: '过期时间' })}</th>
                            <th
                                className="bg-transparent text-gray-500 font-medium py-4 cursor-pointer hover:text-gray-700 dark:hover:text-gray-300 select-none"
                                onClick={toggleUsageSort}
                            >
                                <div className="flex items-center gap-1">
                                    {t('user_token.usage', { defaultValue: '使用量' })}
                                    {sortField === 'usage' && (
                                        sortOrder === 'desc' ? <ArrowDown size={14} /> : <ArrowUp size={14} />
                                    )}
                                </div>
                            </th>
                            <th className="bg-transparent text-gray-500 font-medium py-4">{t('user_token.ip_limit', { defaultValue: 'IP 限制' })}</th>
                            <th className="bg-transparent text-gray-500 font-medium py-4">{t('user_token.created', { defaultValue: '创建时间' })}</th>
                            <th className="bg-transparent text-gray-500 font-medium py-4 text-right">{t('common.actions', { defaultValue: '操作' })}</th>
                        </tr>
                    </thead>
                    <tbody className="divide-y divide-gray-50 dark:divide-base-200">
                        <AnimatePresence mode="popLayout">
                            {sortedTokens.map((token, index) => (
                                <motion.tr
                                    key={token.id}
                                    initial={{ opacity: 0, x: -10 }}
                                    animate={{ opacity: 1, x: 0 }}
                                    exit={{ opacity: 0, scale: 0.95 }}
                                    transition={{ delay: index * 0.03 }}
                                    className="hover:bg-gray-50/80 dark:hover:bg-base-200/50 transition-colors group"
                                >
                                    <td className="py-4">
                                        <div className="flex items-center gap-3">
                                            <div className="w-8 h-8 rounded-full bg-purple-50 dark:bg-purple-900/20 flex items-center justify-center text-purple-600 font-bold text-xs">
                                                {token.username.substring(0, 2).toUpperCase()}
                                            </div>
                                            <div>
                                                <div className="font-semibold text-gray-900 dark:text-white uppercase tracking-wider text-xs">{token.username}</div>
                                                <div className="text-[10px] text-gray-500">{token.description || '-'}</div>
                                            </div>
                                        </div>
                                    </td>
                                    <td>
                                        <div className="flex items-center gap-2 group/token">
                                            <code className="bg-gray-50 dark:bg-base-200 px-2 py-1 rounded border border-gray-100 dark:border-base-300 text-[11px] font-mono text-gray-600 dark:text-gray-400">
                                                {token.token.substring(0, 8)}••••••••
                                            </code>
                                            <button
                                                onClick={() => handleCopyToken(token.token)}
                                                className="p-1.5 hover:bg-gray-200 dark:hover:bg-base-300 rounded-md transition-all text-gray-400 hover:text-gray-600 dark:hover:text-white"
                                            >
                                                <Copy size={13} />
                                            </button>
                                        </div>
                                    </td>
                                    <td>
                                        <div className={`text-xs font-medium mb-1 ${getExpiresStatus(token.expires_at)}`}>
                                            {token.expires_at ? formatTime(token.expires_at) : t('user_token.never', { defaultValue: '永不过期' })}
                                        </div>
                                        <div className="flex items-center gap-2">
                                            <span className="text-[10px] px-1.5 py-0.5 bg-gray-100 dark:bg-base-200 text-gray-500 rounded lowercase">
                                                {getExpiresLabel(token.expires_type)}
                                            </span>
                                            {token.expires_at && token.expires_at < Date.now() / 1000 && (
                                                <button
                                                    onClick={() => handleRenew(token.id, token.expires_type)}
                                                    className="text-[10px] text-blue-500 hover:underline font-medium"
                                                >
                                                    {t('user_token.renew_button', { defaultValue: '续费' })}
                                                </button>
                                            )}
                                        </div>
                                    </td>
                                    <td>
                                        <div className="text-xs font-semibold text-gray-700 dark:text-gray-300">{token.total_requests} <span className="text-[10px] font-normal text-gray-400">次请求</span></div>
                                        <div className="text-[10px] text-gray-400 mt-0.5">
                                            {(token.total_tokens_used / 1000).toFixed(1)}k Token
                                        </div>
                                    </td>
                                    <td>
                                        {token.max_ips === 0
                                            ? <span className="px-2 py-0.5 bg-gray-100 dark:bg-base-200 text-gray-500 text-[10px] rounded-full">{t('user_token.unlimited', { defaultValue: '不限' })}</span>
                                            : <span className="px-2 py-0.5 bg-orange-50 dark:bg-orange-900/20 text-orange-600 dark:text-orange-400 text-[10px] font-medium rounded-full border border-orange-100 dark:border-orange-900/30">{token.max_ips} IP</span>
                                        }
                                        {token.curfew_start && token.curfew_end && (
                                            <div className="text-[10px] text-gray-400 mt-1.5 flex items-center gap-1 bg-gray-50 dark:bg-base-200 w-fit px-1.5 py-0.5 rounded">
                                                <Clock size={10} className="text-orange-500" />
                                                <span>{token.curfew_start} - {token.curfew_end}</span>
                                            </div>
                                        )}
                                        {token.allowed_models && token.allowed_models.length > 0 && (
                                            <div className="mt-1.5 flex flex-wrap gap-1 max-w-[150px]">
                                                {token.allowed_models.slice(0, 3).map((modelId) => (
                                                    <span
                                                        key={modelId}
                                                        className="px-1 py-0.5 bg-blue-50 dark:bg-blue-900/20 text-blue-600 dark:text-blue-400 text-[9px] rounded border border-blue-100 dark:border-blue-900/30"
                                                        title={MODEL_CONFIG[modelId]?.label || modelId}
                                                    >
                                                        {MODEL_CONFIG[modelId]?.shortLabel || modelId.substring(0, 8)}
                                                    </span>
                                                ))}
                                                {token.allowed_models.length > 3 && (
                                                    <span className="px-1 py-0.5 bg-gray-100 dark:bg-base-200 text-gray-500 text-[9px] rounded">
                                                        +{token.allowed_models.length - 3}
                                                    </span>
                                                )}
                                            </div>
                                        )}
                                    </td>
                                    <td className="text-[10px] text-gray-400 italic">
                                        {formatTime(token.created_at)}
                                    </td>
                                    <td className="text-right">
                                        <div className="flex justify-end gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                                            <button
                                                onClick={() => handleEdit(token)}
                                                className="p-1.5 hover:bg-gray-100 dark:hover:bg-base-200 rounded-lg text-gray-500 hover:text-blue-500 transition-colors"
                                                title={t('common.edit', { defaultValue: 'Edit' })}
                                            >
                                                <Settings size={14} />
                                            </button>
                                            <div className="dropdown dropdown-end">
                                                <label tabIndex={0} className="p-1.5 hover:bg-gray-100 dark:hover:bg-base-200 rounded-lg text-gray-500 hover:text-green-500 transition-colors inline-block cursor-pointer">
                                                    <RefreshCw size={14} />
                                                </label>
                                                <ul tabIndex={0} className="dropdown-content z-[10] menu p-2 shadow-xl bg-white dark:bg-base-100 rounded-xl w-32 border border-gray-100 dark:border-base-200 mt-1">
                                                    <div className="px-3 py-1.5 text-[10px] font-bold text-gray-400 uppercase tracking-widest">{t('user_token.renew')}</div>
                                                    <li><a className="text-xs py-2" onClick={() => handleRenew(token.id, 'day')}>{t('user_token.expires_day', { defaultValue: '1 天' })}</a></li>
                                                    <li><a className="text-xs py-2" onClick={() => handleRenew(token.id, 'week')}>{t('user_token.expires_week', { defaultValue: '1 周' })}</a></li>
                                                    <li><a className="text-xs py-2" onClick={() => handleRenew(token.id, 'month')}>{t('user_token.expires_month', { defaultValue: '1 个月' })}</a></li>
                                                </ul>
                                            </div>
                                            <button
                                                onClick={() => handleDelete(token.id)}
                                                className="p-1.5 hover:bg-red-50 dark:hover:bg-red-900/20 rounded-lg text-gray-400 hover:text-red-500 transition-colors"
                                            >
                                                <Trash2 size={14} />
                                            </button>
                                        </div>
                                    </td>
                                </motion.tr>
                            ))}
                        </AnimatePresence>
                        {tokens.length === 0 && !loading && (
                            <tr>
                                <td colSpan={7} className="py-20">
                                    <div className="flex flex-col items-center justify-center text-gray-400 gap-3">
                                        <div className="p-4 bg-gray-50 dark:bg-base-200 rounded-full">
                                            <Users size={40} className="opacity-20" />
                                        </div>
                                        <p className="text-sm">{t('user_token.no_data', { defaultValue: '暂无数据' })}</p>
                                        <button
                                            onClick={() => setShowCreateModal(true)}
                                            className="text-xs text-blue-500 hover:underline"
                                        >
                                            {t('user_token.create', { defaultValue: '创建你的第一个 Token' })}
                                        </button>
                                    </div>
                                </td>
                            </tr>
                        )}
                    </tbody>
                </table>
            </div>

            {/* Create Modal */}
            {showCreateModal && (
                <div className="modal modal-open">
                    <div className="modal-box">
                        <h3 className="font-bold text-lg mb-4">{t('user_token.create_title', { defaultValue: '创建新 Token' })}</h3>

                        <div className="form-control w-full mb-3">
                            <label className="label">
                                <span className="label-text">{t('user_token.username', { defaultValue: '用户名' })} *</span>
                            </label>
                            <input
                                type="text"
                                className="input input-bordered w-full"
                                value={newUsername}
                                onChange={e => setNewUsername(e.target.value)}
                                placeholder={t('user_token.placeholder_username', { defaultValue: 'e.g. user1' })}
                            />
                        </div>

                        <div className="form-control w-full mb-3">
                            <label className="label">
                                <span className="label-text">{t('user_token.description', { defaultValue: '描述' })}</span>
                            </label>
                            <input
                                type="text"
                                className="input input-bordered w-full"
                                value={newDesc}
                                onChange={e => setNewDesc(e.target.value)}
                                placeholder={t('user_token.placeholder_desc', { defaultValue: '选填备注' })}
                            />
                        </div>

                        <div className="grid grid-cols-2 gap-4 mb-3">
                            <div className="form-control w-full">
                                <label className="label">
                                    <span className="label-text">{t('user_token.expires_in', { defaultValue: '有效期' })}</span>
                                </label>
                                <select
                                    className="select select-bordered w-full"
                                    value={newExpiresType}
                                    onChange={e => setNewExpiresType(e.target.value)}
                                >
                                    <option value="day">{t('user_token.expires_day', { defaultValue: '1 天' })}</option>
                                    <option value="week">{t('user_token.expires_week', { defaultValue: '1 周' })}</option>
                                    <option value="month">{t('user_token.expires_month', { defaultValue: '1 个月' })}</option>
                                    <option value="custom">{t('user_token.expires_custom', { defaultValue: '自定义' })}</option>
                                    <option value="never">{t('user_token.expires_never', { defaultValue: '永不过期' })}</option>
                                </select>
                            </div>

                            <div className="form-control w-full">
                                <label className="label">
                                    <span className="label-text">{t('user_token.ip_limit', { defaultValue: '最大 IP 数' })}</span>
                                </label>
                                <input
                                    type="number"
                                    className="input input-bordered w-full"
                                    value={newMaxIps}
                                    onChange={e => setNewMaxIps(parseInt(e.target.value) || 0)}
                                    min="0"
                                    placeholder={t('user_token.placeholder_max_ips', { defaultValue: '0 = 不限制' })}
                                />
                                <label className="label">
                                    <span className="label-text-alt text-gray-500">{t('user_token.hint_max_ips', { defaultValue: '0 表示不限制' })}</span>
                                </label>
                            </div>
                        </div>

                        {/* Custom Expiration Time Picker */}
                        {newExpiresType === 'custom' && (
                            <div className="form-control w-full mb-3">
                                <label className="label">
                                    <span className="label-text">{t('user_token.custom_expires_at', { defaultValue: '自定义过期时间' })} *</span>
                                </label>
                                <input
                                    type="datetime-local"
                                    className="input input-bordered w-full"
                                    value={newCustomExpires}
                                    onChange={e => setNewCustomExpires(e.target.value)}
                                    min={new Date().toISOString().slice(0, 16)}
                                />
                                <label className="label">
                                    <span className="label-text-alt text-gray-500">{t('user_token.hint_custom_expires', { defaultValue: '选择此 Token 过期的确切日期和时间' })}</span>
                                </label>
                            </div>
                        )}

                        <div className="form-control w-full mb-3">
                            <label className="label">
                                <span className="label-text">{t('user_token.curfew', { defaultValue: '宵禁时间 (服务不可用时段)' })}</span>
                            </label>
                            <div className="flex gap-2 items-center">
                                <input
                                    type="time"
                                    className="input input-bordered w-full"
                                    value={newCurfewStart}
                                    onChange={e => setNewCurfewStart(e.target.value)}
                                />
                                <span className="text-gray-400">{t('user_token.curfew_to', { defaultValue: '至' })}</span>
                                <input
                                    type="time"
                                    className="input input-bordered w-full"
                                    value={newCurfewEnd}
                                    onChange={e => setNewCurfewEnd(e.target.value)}
                                />
                            </div>
                            <label className="label">
                                <span className="label-text-alt text-gray-500">{t('user_token.curfew_hint', { defaultValue: '留空则禁用宵禁。基于服务器时间。' })}</span>
                            </label>
                        </div>

                        {/* 模型限制选择 */}
                        <div className="form-control w-full mb-3">
                            <label className="label">
                                <span className="label-text font-medium flex items-center gap-2">
                                    <Sparkles className="w-4 h-4 text-purple-500" />
                                    {t('user_token.allowed_models', { defaultValue: '允许使用的模型' })}
                                </span>
                            </label>
                            <div className="space-y-3">
                                <div className="flex gap-2">
                                    <button
                                        type="button"
                                        className="btn btn-xs btn-outline btn-primary flex-1"
                                        onClick={() => setNewAllowedModels(Object.keys(MODEL_CONFIG))}
                                    >
                                        <Check className="w-3 h-3" />
                                        {t('user_token.select_all', { defaultValue: '全选' })}
                                    </button>
                                    <button
                                        type="button"
                                        className="btn btn-xs btn-outline btn-error flex-1"
                                        onClick={() => setNewAllowedModels([])}
                                    >
                                        <X className="w-3 h-3" />
                                        {t('user_token.clear_all', { defaultValue: '清空' })}
                                    </button>
                                </div>
                                <div className="max-h-48 overflow-y-auto border border-base-300 rounded-xl p-3 space-y-3 bg-gradient-to-br from-base-100 to-base-200/30">
                                    {Object.entries(
                                        Object.entries(MODEL_CONFIG).reduce((acc, [id, config]) => {
                                            const group = config.group;
                                            if (!acc[group]) acc[group] = [];
                                            acc[group].push({ id, ...config });
                                            return acc;
                                        }, {} as Record<string, Array<{ id: string } & typeof MODEL_CONFIG[string]>>)
                                    ).map(([group, models]) => (
                                        <div key={group}>
                                            <div className="text-xs font-bold text-primary/70 mb-2 flex items-center gap-1">
                                                <Sparkles className="w-3 h-3" />
                                                {group}
                                            </div>
                                            <div className="grid grid-cols-2 gap-2">
                                                {models.map((model) => {
                                                    const isSelected = newAllowedModels.includes(model.id);
                                                    return (
                                                        <motion.label
                                                            key={model.id}
                                                            whileHover={{ scale: 1.02 }}
                                                            whileTap={{ scale: 0.98 }}
                                                            className={`cursor-pointer rounded-lg p-2 transition-all border ${
                                                                isSelected
                                                                    ? 'bg-gradient-to-br from-purple-500/10 to-blue-500/10 border-primary/30 shadow-sm'
                                                                    : 'bg-base-100/50 border-base-300 hover:border-primary/20'
                                                            }`}
                                                        >
                                                            <div className="flex items-center gap-2">
                                                                <motion.div
                                                                    initial={false}
                                                                    animate={{
                                                                        scale: isSelected ? 1 : 0,
                                                                        rotate: isSelected ? 180 : 0
                                                                    }}
                                                                    className={`w-5 h-5 rounded-md flex items-center justify-center ${
                                                                        isSelected
                                                                            ? 'bg-gradient-to-br from-purple-500 to-blue-500 text-white'
                                                                            : 'bg-base-300'
                                                                    }`}
                                                                >
                                                                    {isSelected && <Check className="w-3 h-3" />}
                                                                </motion.div>
                                                                <div className="flex-1 min-w-0">
                                                                    <div className="text-xs font-medium truncate">{model.shortLabel}</div>
                                                                    {model.label && (
                                                                        <div className="text-[10px] text-gray-500 truncate">{model.label}</div>
                                                                    )}
                                                                </div>
                                                            </div>
                                                            <input
                                                                type="checkbox"
                                                                className="hidden"
                                                                checked={isSelected}
                                                                onChange={(e) => {
                                                                    if (e.target.checked) {
                                                                        setNewAllowedModels([...newAllowedModels, model.id]);
                                                                    } else {
                                                                        setNewAllowedModels(newAllowedModels.filter(id => id !== model.id));
                                                                    }
                                                                }}
                                                            />
                                                        </motion.label>
                                                    );
                                                })}
                                            </div>
                                        </div>
                                    ))}
                                </div>
                            </div>
                            <label className="label">
                                <span className="label-text-alt text-gray-500 flex items-center gap-1">
                                    {newAllowedModels.length === 0 ? (
                                        <>
                                            <Check className="w-3 h-3 text-green-500" />
                                            {t('user_token.models_unlimited', { defaultValue: 'Empty = All models allowed' })}
                                        </>
                                    ) : (
                                        <>
                                            <Sparkles className="w-3 h-3 text-purple-500" />
                                            {newAllowedModels.length} {t('user_token.models_selected', { defaultValue: 'models selected' })}
                                        </>
                                    )}
                                </span>
                            </label>
                        </div>

                        <div className="modal-action">
                            <button className="px-4 py-2 hover:bg-gray-100 dark:hover:bg-base-200 rounded-lg text-sm transition-colors" onClick={() => setShowCreateModal(false)}>
                                {t('common.cancel', { defaultValue: 'Cancel' })}
                            </button>
                            <button
                                className={`px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white text-sm font-medium rounded-lg transition-all shadow-sm shadow-blue-500/20 flex items-center gap-2 ${creating ? 'opacity-50 cursor-not-allowed' : ''}`}
                                onClick={handleCreate}
                                disabled={creating}
                            >
                                {creating && <RefreshCw size={14} className="animate-spin" />}
                                {t('common.create', { defaultValue: '创建' })}
                            </button>
                        </div>
                    </div>
                </div>
            )}

            {/* Edit Modal */}
            {showEditModal && editingToken && (
                <div className="modal modal-open">
                    <div className="modal-box">
                        <h3 className="font-bold text-lg mb-4">{t('user_token.edit_title', { defaultValue: '编辑 Token' })}</h3>

                        <div className="form-control w-full mb-3">
                            <label className="label">
                                <span className="label-text">{t('user_token.username', { defaultValue: '用户名' })} *</span>
                            </label>
                            <input
                                type="text"
                                className="input input-bordered w-full"
                                value={editUsername}
                                onChange={e => setEditUsername(e.target.value)}
                                placeholder={t('user_token.placeholder_username', { defaultValue: 'e.g. user1' })}
                            />
                        </div>

                        <div className="form-control w-full mb-3">
                            <label className="label">
                                <span className="label-text">{t('user_token.description', { defaultValue: '描述' })}</span>
                            </label>
                            <input
                                type="text"
                                className="input input-bordered w-full"
                                value={editDesc}
                                onChange={e => setEditDesc(e.target.value)}
                                placeholder={t('user_token.placeholder_desc', { defaultValue: '选填备注' })}
                            />
                        </div>

                        <div className="form-control w-full mb-3">
                            <label className="label">
                                <span className="label-text">{t('user_token.ip_limit', { defaultValue: '最大 IP 数' })}</span>
                            </label>
                            <input
                                type="number"
                                className="input input-bordered w-full"
                                value={editMaxIps}
                                onChange={e => setEditMaxIps(parseInt(e.target.value) || 0)}
                                min="0"
                                placeholder={t('user_token.placeholder_max_ips', { defaultValue: '0 = Unlimited' })}
                            />
                            <label className="label">
                                <span className="label-text-alt text-gray-500">{t('user_token.hint_max_ips', { defaultValue: '0 = Unlimited' })}</span>
                            </label>
                        </div>

                        <div className="form-control w-full mb-3">
                            <label className="label">
                                <span className="label-text">{t('user_token.curfew', { defaultValue: '宵禁时间 (服务不可用时段)' })}</span>
                            </label>
                            <div className="flex gap-2 items-center">
                                <input
                                    type="time"
                                    className="input input-bordered w-full"
                                    value={editCurfewStart}
                                    onChange={e => setEditCurfewStart(e.target.value)}
                                />
                                <span className="text-gray-400">{t('user_token.curfew_to', { defaultValue: '至' })}</span>
                                <input
                                    type="time"
                                    className="input input-bordered w-full"
                                    value={editCurfewEnd}
                                    onChange={e => setEditCurfewEnd(e.target.value)}
                                />
                            </div>
                            <label className="label">
                                <span className="label-text-alt text-gray-500">{t('user_token.curfew_hint', { defaultValue: '留空则禁用宵禁。基于服务器时间。' })}</span>
                            </label>
                        </div>

                        {/* 模型限制选择 */}
                        <div className="form-control w-full mb-3">
                            <label className="label">
                                <span className="label-text font-medium flex items-center gap-2">
                                    <Sparkles className="w-4 h-4 text-purple-500" />
                                    {t('user_token.allowed_models', { defaultValue: '允许使用的模型' })}
                                </span>
                            </label>
                            <div className="space-y-3">
                                <div className="flex gap-2">
                                    <button
                                        type="button"
                                        className="btn btn-xs btn-outline btn-primary flex-1"
                                        onClick={() => setEditAllowedModels(Object.keys(MODEL_CONFIG))}
                                    >
                                        <Check className="w-3 h-3" />
                                        {t('user_token.select_all', { defaultValue: '全选' })}
                                    </button>
                                    <button
                                        type="button"
                                        className="btn btn-xs btn-outline btn-error flex-1"
                                        onClick={() => setEditAllowedModels([])}
                                    >
                                        <X className="w-3 h-3" />
                                        {t('user_token.clear_all', { defaultValue: '清空' })}
                                    </button>
                                </div>
                                <div className="max-h-48 overflow-y-auto border border-base-300 rounded-xl p-3 space-y-3 bg-gradient-to-br from-base-100 to-base-200/30">
                                    {Object.entries(
                                        Object.entries(MODEL_CONFIG).reduce((acc, [id, config]) => {
                                            const group = config.group;
                                            if (!acc[group]) acc[group] = [];
                                            acc[group].push({ id, ...config });
                                            return acc;
                                        }, {} as Record<string, Array<{ id: string } & typeof MODEL_CONFIG[string]>>)
                                    ).map(([group, models]) => (
                                        <div key={group}>
                                            <div className="text-xs font-bold text-primary/70 mb-2 flex items-center gap-1">
                                                <Sparkles className="w-3 h-3" />
                                                {group}
                                            </div>
                                            <div className="grid grid-cols-2 gap-2">
                                                {models.map((model) => {
                                                    const isSelected = editAllowedModels.includes(model.id);
                                                    return (
                                                        <motion.label
                                                            key={model.id}
                                                            whileHover={{ scale: 1.02 }}
                                                            whileTap={{ scale: 0.98 }}
                                                            className={`cursor-pointer rounded-lg p-2 transition-all border ${
                                                                isSelected
                                                                    ? 'bg-gradient-to-br from-purple-500/10 to-blue-500/10 border-primary/30 shadow-sm'
                                                                    : 'bg-base-100/50 border-base-300 hover:border-primary/20'
                                                            }`}
                                                        >
                                                            <div className="flex items-center gap-2">
                                                                <motion.div
                                                                    initial={false}
                                                                    animate={{
                                                                        scale: isSelected ? 1 : 0,
                                                                        rotate: isSelected ? 180 : 0
                                                                    }}
                                                                    className={`w-5 h-5 rounded-md flex items-center justify-center ${
                                                                        isSelected
                                                                            ? 'bg-gradient-to-br from-purple-500 to-blue-500 text-white'
                                                                            : 'bg-base-300'
                                                                    }`}
                                                                >
                                                                    {isSelected && <Check className="w-3 h-3" />}
                                                                </motion.div>
                                                                <div className="flex-1 min-w-0">
                                                                    <div className="text-xs font-medium truncate">{model.shortLabel}</div>
                                                                    {model.label && (
                                                                        <div className="text-[10px] text-gray-500 truncate">{model.label}</div>
                                                                    )}
                                                                </div>
                                                            </div>
                                                            <input
                                                                type="checkbox"
                                                                className="hidden"
                                                                checked={isSelected}
                                                                onChange={(e) => {
                                                                    if (e.target.checked) {
                                                                        setEditAllowedModels([...editAllowedModels, model.id]);
                                                                    } else {
                                                                        setEditAllowedModels(editAllowedModels.filter(id => id !== model.id));
                                                                    }
                                                                }}
                                                            />
                                                        </motion.label>
                                                    );
                                                })}
                                            </div>
                                        </div>
                                    ))}
                                </div>
                            </div>
                            <label className="label">
                                <span className="label-text-alt text-gray-500 flex items-center gap-1">
                                    {editAllowedModels.length === 0 ? (
                                        <>
                                            <Check className="w-3 h-3 text-green-500" />
                                            {t('user_token.models_unlimited', { defaultValue: 'Empty = All models allowed' })}
                                        </>
                                    ) : (
                                        <>
                                            <Sparkles className="w-3 h-3 text-purple-500" />
                                            {editAllowedModels.length} {t('user_token.models_selected', { defaultValue: 'models selected' })}
                                        </>
                                    )}
                                </span>
                            </label>
                        </div>

                        <div className="modal-action">
                            <button className="px-4 py-2 hover:bg-gray-100 dark:hover:bg-base-200 rounded-lg text-sm transition-colors" onClick={() => setShowEditModal(false)}>
                                {t('common.cancel', { defaultValue: '取消' })}
                            </button>
                            <button
                                className={`px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white text-sm font-medium rounded-lg transition-all shadow-sm shadow-blue-500/20 flex items-center gap-2 ${updating ? 'opacity-50 cursor-not-allowed' : ''}`}
                                onClick={handleUpdate}
                                disabled={updating}
                            >
                                {updating && <RefreshCw size={14} className="animate-spin" />}
                                {t('common.update', { defaultValue: '更新' })}
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </motion.div>
    );
};
export default UserToken;
