import { motion } from 'framer-motion';
import {
    Activity,
    AlertTriangle,
    ArrowRight,
    Bot,
    Calendar,
    Clock,
    RefreshCw,
    ShieldAlert,
    Sparkles,
    Zap,
} from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import AddAccountDialog from '../components/accounts/AddAccountDialog';
import { showToast } from '../components/common/ToastContainer';
import BestAccounts from '../components/dashboard/BestAccounts';
import { findImageQuotaModel, findQuotaModel } from '../config/modelConfig';
import CurrentAccount from '../components/dashboard/CurrentAccount';
import { useAccountStore } from '../stores/useAccountStore';
import { useConfigStore } from '../stores/useConfigStore';
import { Account } from '../types/account';
import { DashboardQuotaView } from '../utils/quotaDisplay';

function Dashboard() {
    const { t } = useTranslation();
    const navigate = useNavigate();
    const {
        accounts,
        currentAccount,
        activeClients,
        fetchAccounts,
        fetchCurrentAccount,
        fetchActiveClients,
        switchAccount,
        addAccount,
        refreshQuota,
        loading
    } = useAccountStore();
    const { config, saveConfig } = useConfigStore();
    const qosAutoJail = !!config?.proxy?.qos_auto_jail;

    const handleToggleQoSAutoJail = async () => {
        if (!config) return;
        const nextVal = !qosAutoJail;
        try {
            await saveConfig({
                ...config,
                proxy: {
                    ...config.proxy,
                    qos_auto_jail: nextVal,
                },
            });
            showToast(
                nextVal
                    ? t('dashboard.qos_auto_jail_on', '小黑屋: 开')
                    : t('dashboard.qos_auto_jail_off', '小黑屋: 关'),
                'info'
            );
        } catch (err) {
            console.error('Failed to update QoS auto jail setting:', err);
        }
    };

    useEffect(() => {
        fetchAccounts();
        fetchCurrentAccount();
        fetchActiveClients();

        // 保持 5 秒轮询三大客户端（经典款 / IDE / CLI）当前生效账号状态
        const intervalId = setInterval(() => {
            fetchActiveClients();
        }, 5000);

        // 窗口聚焦时即刻拉取最新客户端状态
        const handleFocus = () => {
            fetchActiveClients();
        };
        window.addEventListener('focus', handleFocus);

        return () => {
            clearInterval(intervalId);
            window.removeEventListener('focus', handleFocus);
        };
    }, []);

    const [onlyProxyPool, setOnlyProxyPool] = useState<boolean>(() => {
        const saved = localStorage.getItem('dashboard_filter_proxy_pool');
        return saved !== null ? saved === 'true' : true;
    });

    const [quotaView, setQuotaView] = useState<DashboardQuotaView>(() => {
        const saved = localStorage.getItem('dashboard_quota_view');
        return (saved === '5h' || saved === 'weekly' || saved === 'weighted') ? saved : 'weighted';
    });

    useEffect(() => {
        localStorage.setItem('dashboard_filter_proxy_pool', String(onlyProxyPool));
    }, [onlyProxyPool]);

    useEffect(() => {
        localStorage.setItem('dashboard_quota_view', quotaView);
    }, [quotaView]);

    // 全维度账号健康与配额计算矩阵 (状态由风控定生死，底座池支持仅可用账号与全部正常账号无缝切换)
    const stats = useMemo(() => {
        // 1. 账号生态健康状态分类（风控与启用状态定生死，不以额度定生死）
        // 异常账号：触发 Google 验证码/风控阻断，或配额接口返回 403 Forbidden 封禁
        const abnormalAccounts = accounts.filter(
            a => a.validation_blocked || a.banned || a.quota?.is_forbidden
        );
        // 全量正常状态账号（彻底剔除风控异常账号）
        const normalAccounts = accounts.filter(
            a => !a.validation_blocked && !a.banned && !a.quota?.is_forbidden
        );
        // 完全被用户手动停用的账号 (disabled 为 true)
        const fullyDisabledAccounts = normalAccounts.filter(
            a => a.disabled
        );
        // 可用正常账号（非异常且未停用，可供客户端登录或调度）
        const availableAccounts = normalAccounts.filter(
            a => !a.disabled
        );
        // 反代在线账号（可用且未禁用反代，真正加入反代服务池）
        const proxyOnlineAccounts = availableAccounts.filter(
            a => !a.proxy_disabled
        );
        // 仅禁用反代的正常账号 (未停用但禁用了反代)
        const proxyDisabledAccounts = availableAccounts.filter(
            a => a.proxy_disabled
        );

        // 依据按钮状态选择底座统计池：默认看反代在线账号，切换看全部可用账号
        const basePool = onlyProxyPool
            ? (proxyOnlineAccounts.length > 0 ? proxyOnlineAccounts : availableAccounts)
            : availableAccounts;

        // 2. 单账号配额提取辅助函数
        const get5hQuota = (a: Account, modelKey: 'gemini-pro' | 'gemini-image' | 'claude'): number | null => {
            const isClaude = modelKey === 'claude';
            if (isClaude) {
                const hasClaude = a.quota?.models?.some(m => {
                    const n = m.name.toLowerCase();
                    return !n.includes('-disabled') && !n.endsWith('_disabled') && !(m as any).disabled && (n.startsWith('claude') || n.startsWith('gpt'));
                });
                if (!hasClaude) return null;
            }
            // 优先从 quota_groups 提取原生 5H 滚动配额桶
            if (a.quota?.quota_groups) {
                for (const group of a.quota.quota_groups) {
                    const gname = group.display_name.toLowerCase();
                    if (gname.includes('argon')) continue;
                    const matches = isClaude
                        ? (gname.includes('claude') || gname.includes('gpt') || gname.includes('3p'))
                        : (gname.includes('gemini') || (!gname.includes('claude') && !gname.includes('gpt') && !gname.includes('3p')));
                    if (matches) {
                        const bucket5h = group.buckets?.find(b =>
                            b.window?.toLowerCase().includes('5h') ||
                            b.bucket_id?.toLowerCase().includes('5h') ||
                            b.window?.toLowerCase().includes('hour') ||
                            b.bucket_id?.toLowerCase().includes('hour')
                        );
                        if (bucket5h && typeof bucket5h.remaining_fraction === 'number') {
                            return Math.round(bucket5h.remaining_fraction * 100);
                        }
                    }
                }
            }
            if (modelKey === 'gemini-image') {
                return findImageQuotaModel(a.quota?.models)?.percentage ?? null;
            }
            return findQuotaModel(a.quota?.models, modelKey)?.percentage ?? null;
        };

        const getWeeklyQuota = (a: Account, modelKey: 'gemini-pro' | 'gemini-image' | 'claude'): number | null => {
            const isClaude = modelKey === 'claude';
            if (isClaude) {
                const hasClaude = a.quota?.models?.some(m => {
                    const n = m.name.toLowerCase();
                    return !n.includes('-disabled') && !n.endsWith('_disabled') && !(m as any).disabled && (n.startsWith('claude') || n.startsWith('gpt'));
                });
                if (!hasClaude) return null;
            }
            if (a.quota?.quota_groups) {
                for (const group of a.quota.quota_groups) {
                    const gname = group.display_name.toLowerCase();
                    if (gname.includes('argon')) continue;
                    const matches = isClaude
                        ? (gname.includes('claude') || gname.includes('gpt') || gname.includes('3p'))
                        : (gname.includes('gemini') || (!gname.includes('claude') && !gname.includes('gpt') && !gname.includes('3p')));
                    if (matches) {
                        const weekly = group.buckets?.find(b =>
                            b.window?.toLowerCase().includes('week') ||
                            b.bucket_id?.toLowerCase().includes('week') ||
                            b.window?.toLowerCase().includes('7d')
                        );
                        if (weekly && typeof weekly.remaining_fraction === 'number') {
                            return Math.round(weekly.remaining_fraction * 100);
                        }
                    }
                }
            }
            return null;
        };

        // 计算目标底座池在指定模型下的综合指标（5H均值、周配额均值、周配额熔断加权，以及受周额度木桶约束后的 5H 均值）
        const computeMetrics = (modelKey: 'gemini-pro' | 'gemini-image' | 'claude') => {
            const pool = basePool;
            if (pool.length === 0) {
                return {
                    avg5h: 0,
                    constrainedAvg5h: 0,
                    avgWeekly: 0,
                    weightedEffective: 0,
                    zeroWeeklyCount: 0,
                    coolingCount: 0,
                    cappedCount: 0,
                };
            }

            let sum5h = 0, count5h = 0;
            let sumConstrained5h = 0, countConstrained5h = 0;
            let sumWeekly = 0, countWeekly = 0;
            let sumWeighted = 0, totalWeight = 0;
            let zeroWeeklyCount = 0;
            let coolingCount = 0;
            let cappedCount = 0;

            for (const a of pool) {
                const q5h = get5hQuota(a, modelKey);
                const qWeekly = getWeeklyQuota(a, modelKey);

                if (q5h === null && qWeekly === null) {
                    continue;
                }

                if (q5h !== null && q5h >= 0) {
                    sum5h += q5h;
                    count5h++;
                }

                if (qWeekly !== null && qWeekly >= 0) {
                    sumWeekly += qWeekly;
                    countWeekly++;
                    if (qWeekly <= 0) {
                        zeroWeeklyCount++;
                    }
                }

                // 1. 双向木桶约束计算：5H 受周额度硬顶约束
                let constrained5h = q5h ?? (qWeekly ?? 0);
                if (qWeekly !== null && qWeekly <= 0) {
                    constrained5h = 0; // 周见底熔断
                } else if (qWeekly !== null && q5h !== null && qWeekly < q5h) {
                    constrained5h = qWeekly; // 被周额度硬顶压制
                    cappedCount++;
                }
                sumConstrained5h += constrained5h;
                countConstrained5h++;

                // 2. 双向木桶约束计算：周额度受 5H 瞬时冷却约束统计
                if (qWeekly !== null && qWeekly > 0 && q5h !== null && q5h <= 0) {
                    coolingCount++;
                }

                // 3. 核心短板特殊处理：周配额见底(0%)时，上游必然拒绝，实际可用额度熔断归 0！
                let effectiveVal = q5h ?? 0;
                if (qWeekly !== null && qWeekly <= 0) {
                    effectiveVal = 0;
                } else if (qWeekly !== null && q5h !== null) {
                    effectiveVal = Math.min(q5h, qWeekly);
                } else if (qWeekly !== null) {
                    effectiveVal = qWeekly;
                }

                // 套餐权重加权 (ULTRA 2.0, PRO 1.5, FREE 1.0)
                const tier = (a.quota?.subscription_tier || '').toUpperCase();
                const weight = tier.includes('ULTRA') ? 2.0 : tier.includes('PRO') ? 1.5 : 1.0;
                sumWeighted += effectiveVal * weight;
                totalWeight += weight;
            }

            return {
                avg5h: count5h > 0 ? Math.round(sum5h / count5h) : 0,
                constrainedAvg5h: countConstrained5h > 0 ? Math.round(sumConstrained5h / countConstrained5h) : 0,
                avgWeekly: countWeekly > 0 ? Math.round(sumWeekly / countWeekly) : 0,
                weightedEffective: totalWeight > 0 ? Math.round(sumWeighted / totalWeight) : 0,
                hasActivePool: count5h > 0 || countWeekly > 0,
                zeroWeeklyCount,
                coolingCount,
                cappedCount,
            };
        };

        const gemini = computeMetrics('gemini-pro');
        const geminiImage = computeMetrics('gemini-image');
        const claude = computeMetrics('claude');

        return {
            total: accounts.length,
            available: availableAccounts.length,
            proxyOnline: proxyOnlineAccounts.length,
            proxyDisabled: proxyDisabledAccounts.length,
            fullyDisabled: fullyDisabledAccounts.length,
            disabled: fullyDisabledAccounts.length + proxyDisabledAccounts.length,
            abnormal: abnormalAccounts.length,
            normalCount: normalAccounts.length,
            basePoolCount: basePool.length,
            gemini,
            geminiImage,
            claude,
        };
    }, [accounts, onlyProxyPool]);

    const geminiQuotaValue = quotaView === '5h'
        ? stats.gemini.avg5h
        : quotaView === 'weekly'
            ? stats.gemini.avgWeekly
            : stats.gemini.weightedEffective;
    const geminiQuotaLabel = quotaView === '5h'
        ? t('dashboard.view_mode_5h', '5小时滚动可用')
        : quotaView === 'weekly'
            ? t('dashboard.view_mode_weekly', '7天周配额均值')
            : t('dashboard.view_mode_weighted', '综合加权可用');

    const hasClaudePool = stats.claude.hasActivePool;
    const claudeQuotaValue = !hasClaudePool
        ? null
        : quotaView === '5h'
            ? stats.claude.avg5h
            : quotaView === 'weekly'
                ? stats.claude.avgWeekly
                : stats.claude.weightedEffective;
    const claudeQuotaLabel = quotaView === '5h'
        ? t('dashboard.view_mode_5h', '5小时滚动可用')
        : quotaView === 'weekly'
            ? t('dashboard.view_mode_weekly', '7天周配额均值')
            : t('dashboard.view_mode_weighted', '综合加权可用');

    const isSwitchingRef = useRef(false);

    const handleSwitch = async (accountId: string, targetIde?: string) => {
        if (loading || isSwitchingRef.current) return;

        isSwitchingRef.current = true;
        console.log('[Dashboard] handleSwitch called for', accountId, 'targetIde:', targetIde);
        try {
            await switchAccount(accountId, targetIde);
            showToast(t('dashboard.toast.switch_success'), 'success');
        } catch (error) {
            console.error('切换账号失败:', error);
            showToast(`${t('dashboard.toast.switch_error')}: ${error}`, 'error');
        } finally {
            setTimeout(() => {
                isSwitchingRef.current = false;
            }, 1000);
        }
    };

    const handleAddAccount = async (email: string, refreshToken: string) => {
        await addAccount(email, refreshToken);
        await fetchAccounts(); // 刷新列表
    };

    const [isRefreshing, setIsRefreshing] = useState(false);

    const handleRefreshCurrent = async () => {
        if (!currentAccount) return;

        setIsRefreshing(true);
        try {
            await refreshQuota(currentAccount.id);
            // 刷新成功后重新获取最新数据
            await fetchCurrentAccount();
            showToast(t('dashboard.toast.refresh_success'), 'success');
        } catch (error) {
            console.error('[Dashboard] Refresh failed:', error);
            showToast(`${t('dashboard.toast.refresh_error')}: ${error}`, 'error');
        } finally {
            setIsRefreshing(false);
        }
    };

    return (
        <div className="h-full w-full overflow-y-auto">
            <motion.div
                initial={{ opacity: 0, y: 4 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.15, ease: [0.16, 1, 0.3, 1] }}
                style={{ position: 'relative', zIndex: 1, willChange: 'opacity, transform' }}
                className="p-5 space-y-4 max-w-7xl mx-auto"
            >
                {/* 问候语和全局刷新/添加操作栏 */}
                <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                    <div>
                        <h1 className="text-2xl font-bold text-gray-900 dark:text-base-content">
                            {currentAccount
                                ? t('dashboard.hello').replace('用户', currentAccount.name || currentAccount.email.split('@')[0])
                                : t('dashboard.hello')
                            }
                        </h1>
                    </div>

                    {/* 刷新额度与添加账号操作 */}
                    <div className="flex items-center gap-2">
                        <button
                            className={`px-3 py-1.5 bg-blue-500 text-white text-xs font-medium rounded-xl hover:bg-blue-600 transition-colors flex items-center gap-1.5 shadow-sm ${isRefreshing || !currentAccount ? 'opacity-70 cursor-not-allowed' : ''}`}
                            onClick={handleRefreshCurrent}
                            disabled={isRefreshing || !currentAccount}
                            title={isRefreshing ? t('dashboard.refreshing') : t('dashboard.refresh_quota')}
                        >
                            <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin' : ''}`} />
                            <span className="hidden sm:inline">{isRefreshing ? t('dashboard.refreshing') : t('dashboard.refresh_quota')}</span>
                        </button>
                        <AddAccountDialog onAdd={handleAddAccount} />
                    </div>
                </div>

                {/* 顶栏控制条：左侧快捷控制胶囊，右侧纯粹通透的 KPI 指标 */}
                <div className="flex flex-col lg:flex-row lg:items-center justify-between gap-3 px-1 py-0.5">
                    {/* 左上角控制胶囊组 */}
                    <div className="flex flex-wrap items-center gap-2 select-none">
                        {/* 1. 配额视窗模式切换: 综合加权 / 5H滚动 / 7天周配额 */}
                        <div className="flex items-center bg-gray-100 dark:bg-white/[0.04] p-1 rounded-xl text-xs dark:border dark:border-white/[0.08]">
                            <button
                                type="button"
                                onClick={() => setQuotaView('weighted')}
                                className={`px-2.5 py-1 rounded-lg font-medium transition-all cursor-pointer flex items-center gap-1.5 ${
                                    quotaView === 'weighted'
                                        ? 'bg-white dark:bg-zinc-800 text-indigo-600 dark:text-indigo-300 shadow-sm font-bold dark:border dark:border-white/[0.12]'
                                        : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                                }`}
                                title={t('dashboard.view_mode_weighted_title', '综合加权可用 (5H滚动与周配额木桶短板)')}
                            >
                                <Sparkles className="w-3.5 h-3.5" />
                                <span>{t('dashboard.view_mode_weighted', '综合加权')}</span>
                            </button>
                            <button
                                type="button"
                                onClick={() => setQuotaView('5h')}
                                className={`px-2.5 py-1 rounded-lg font-medium transition-all cursor-pointer flex items-center gap-1.5 ${
                                    quotaView === '5h'
                                        ? 'bg-white dark:bg-zinc-800 text-emerald-600 dark:text-emerald-300 shadow-sm font-bold dark:border dark:border-white/[0.12]'
                                        : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                                }`}
                                title={t('dashboard.view_mode_title_5h', '5H 滚动：近 5 小时冲刺可用度 (受周配额硬顶约束)')}
                            >
                                <Clock className="w-3.5 h-3.5" />
                                <span>{t('dashboard.view_mode_5h', '5H 滚动')}</span>
                            </button>
                            <button
                                type="button"
                                onClick={() => setQuotaView('weekly')}
                                className={`px-2.5 py-1 rounded-lg font-medium transition-all cursor-pointer flex items-center gap-1.5 ${
                                    quotaView === 'weekly'
                                        ? 'bg-white dark:bg-zinc-800 text-teal-600 dark:text-teal-300 shadow-sm font-bold dark:border dark:border-white/[0.12]'
                                        : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                                }`}
                                title={t('dashboard.view_mode_title_weekly', '7天周配额：本周宏观总预算 (受 5H 瞬时冷却阀门约束)')}
                            >
                                <Calendar className="w-3.5 h-3.5" />
                                <span>{t('dashboard.view_mode_weekly', '7天周配额')}</span>
                            </button>
                        </div>

                        {/* 3. 账号池范围筛选: 反代账号 vs 可用账号 */}
                        <div className="flex items-center bg-gray-100 dark:bg-white/[0.04] p-1 rounded-xl text-xs dark:border dark:border-white/[0.08]">
                            <button
                                type="button"
                                onClick={() => setOnlyProxyPool(true)}
                                className={`px-2.5 py-1 rounded-lg font-medium transition-all cursor-pointer ${
                                    onlyProxyPool
                                        ? 'bg-white dark:bg-zinc-800 text-emerald-600 dark:text-emerald-300 shadow-sm font-bold dark:border dark:border-white/[0.12]'
                                        : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                                }`}
                                title={t('dashboard.btn_title_proxy_online', '当前：仅统计参与反代分发的在线账号配额')}
                            >
                                {t('dashboard.btn_proxy_online', { count: stats.proxyOnline, defaultValue: `反代账号 (${stats.proxyOnline})` })}
                            </button>
                            <button
                                type="button"
                                onClick={() => setOnlyProxyPool(false)}
                                className={`px-2.5 py-1 rounded-lg font-medium transition-all cursor-pointer ${
                                    !onlyProxyPool
                                        ? 'bg-white dark:bg-zinc-800 text-blue-600 dark:text-blue-300 shadow-sm font-bold dark:border dark:border-white/[0.12]'
                                        : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                                }`}
                                title={t('dashboard.btn_title_all_available', '当前：统计所有正常未停用的可用账号配额')}
                            >
                                {t('dashboard.btn_all_available', { count: stats.available, defaultValue: `可用账号 (${stats.available})` })}
                            </button>
                        </div>

                        {/* 4. 自动限流进小黑屋 (QoS 降速熔断) 开关 */}
                        <div className="flex items-center bg-gray-100 dark:bg-white/[0.04] p-1 rounded-xl text-xs dark:border dark:border-white/[0.08]">
                            <button
                                type="button"
                                onClick={handleToggleQoSAutoJail}
                                className={`px-2.5 py-1 rounded-lg font-medium transition-all cursor-pointer flex items-center gap-1.5 ${
                                    qosAutoJail
                                        ? 'bg-white dark:bg-zinc-800 text-amber-600 dark:text-amber-300 shadow-sm font-bold dark:border dark:border-white/[0.12]'
                                        : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                                }`}
                                title={
                                    qosAutoJail
                                        ? t('dashboard.qos_auto_jail_enabled_tip', '当前：已开启自动限流进小黑屋。遇响应过长的限速账号将自动隔离并解绑会话（点击关闭）')
                                        : t('dashboard.qos_auto_jail_disabled_tip', '当前：已关闭自动小黑屋。直接忽略校验时长，不因首字延迟过长标记疑似限速或解绑会话（点击开启）')
                                }
                            >
                                <ShieldAlert className={`w-3.5 h-3.5 ${qosAutoJail ? 'text-amber-500 dark:text-amber-400' : 'text-gray-400 dark:text-zinc-500'}`} />
                                <span>{t('dashboard.qos_auto_jail', '自动小黑屋')}</span>
                                <span className={`text-[10px] px-1 py-0.5 rounded font-mono ${
                                    qosAutoJail
                                        ? 'bg-amber-500/20 text-amber-700 dark:text-amber-300 font-bold'
                                        : 'bg-gray-200 dark:bg-white/[0.08] text-gray-400 dark:text-zinc-500'
                                }`}>
                                    {qosAutoJail ? t('dashboard.qos_auto_jail_badge_on', '开') : t('dashboard.qos_auto_jail_badge_off', '关')}
                                </span>
                            </button>
                        </div>
                    </div>

                            {/* 右侧：纯粹通透的大字 KPI 矩阵 (Requests · Avg response · Spend 灵感) */}
                            <div className="flex items-center gap-5 sm:gap-6 text-right">
                                <div>
                                    <div className="text-xl sm:text-2xl font-black font-mono tracking-tight text-gray-900 dark:text-white leading-none">
                                        {stats.total}
                                    </div>
                                    <div className="text-[10px] text-gray-400 dark:text-zinc-500 font-medium mt-1">
                                        {t('dashboard.total_accounts', '总账号纳管')}
                                    </div>
                                </div>
                                <div className="h-6 w-px bg-gray-200 dark:bg-white/[0.08]" />
                                <div>
                                    <div className="text-xl sm:text-2xl font-black font-mono tracking-tight text-emerald-600 dark:text-emerald-400 leading-none">
                                        {stats.proxyOnline}
                                    </div>
                                    <div className="text-[10px] text-gray-400 dark:text-zinc-500 font-medium mt-1">
                                        {t('dashboard.proxy_online_unit', '反代服务在线')}
                                    </div>
                                </div>
                                <div className="h-6 w-px bg-gray-200 dark:bg-white/[0.08]" />
                                <div>
                                    <div className="text-xl sm:text-2xl font-black font-mono tracking-tight text-blue-600 dark:text-blue-400 leading-none">
                                        {Math.round((stats.available / (stats.total || 1)) * 100)}%
                                    </div>
                                    <div className="text-[10px] text-gray-400 dark:text-zinc-500 font-medium mt-1">
                                        {t('dashboard.normal_rate', '健康正常率')}
                                    </div>
                                </div>
                            </div>
                        </div>

                <div className="space-y-4">
                        {/* 核心 Bento 4 卡矩阵 (纯黑极简风格) */}
                        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-3.5 items-stretch">
                            {/* 卡片 1: 账号底座资产 */}
                            <motion.div
                                whileHover={{ y: -2 }}
                                transition={{ duration: 0.15 }}
                                className="bg-white dark:bg-[#15161b] rounded-2xl p-5 shadow-sm dark:shadow-xl dark:shadow-black/40 border border-gray-100 dark:border-white/[0.10] flex flex-col justify-between relative overflow-hidden"
                            >
                                <div className="space-y-4">
                                    <div className="flex items-center justify-between">
                                        <div className="flex items-center gap-2">
                                            <div className="p-1.5 rounded-lg bg-emerald-50 dark:bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 border border-transparent dark:border-emerald-500/20">
                                                <Zap className="w-4 h-4" />
                                            </div>
                                            <span className="text-xs font-bold text-gray-800 dark:text-zinc-200">
                                                {t('dashboard.accounts_health', '账号底座资产')}
                                            </span>
                                        </div>
                                        <span className="px-2 py-0.5 rounded-full text-[10px] font-mono font-bold bg-emerald-50 dark:bg-emerald-500/15 text-emerald-700 dark:text-emerald-300 border border-emerald-200/60 dark:border-emerald-500/30">
                                            {stats.available}/{stats.total}
                                        </span>
                                    </div>

                                    {/* 核心大数字 */}
                                    <div>
                                        <div className="text-3xl sm:text-4xl font-black font-mono tracking-tight text-gray-900 dark:text-white">
                                            {stats.available}
                                            <span className="text-sm font-sans font-normal text-gray-400 dark:text-zinc-400 ml-1.5">
                                                / {stats.total} {t('dashboard.available_accounts_unit', '可用')}
                                            </span>
                                        </div>
                                        <div className="text-[11px] text-gray-400 dark:text-zinc-400 mt-1 font-medium">
                                            {stats.proxyOnline > 0
                                                ? t('dashboard.proxy_joined_pool', { count: stats.proxyOnline, defaultValue: `✓ ${stats.proxyOnline} 个账号已加入反代服务池` })
                                                : t('dashboard.disabled_accounts_desc', '手动停用/待开启')
                                            }
                                        </div>
                                    </div>

                                    {/* 彩色进度条 */}
                                    <div className="w-full bg-gray-100 dark:bg-white/10 h-2 rounded-full overflow-hidden">
                                        <div
                                            className="bg-gradient-to-r from-emerald-500 to-teal-400 h-full rounded-full transition-all duration-300 shadow-[0_0_8px_rgba(52,211,153,0.4)]"
                                            style={{ width: `${Math.round((stats.available / (stats.total || 1)) * 100)}%` }}
                                        />
                                    </div>
                                </div>

                                {/* 底部操作栏胶囊 */}
                                <div className="pt-3 mt-3 border-t border-gray-100 dark:border-white/[0.08] flex items-center justify-between text-xs">
                                    <span className="text-[11px] text-gray-400 dark:text-zinc-400 font-mono font-medium">
                                        {Math.round((stats.available / (stats.total || 1)) * 100)}% {t('dashboard.asset_rate', '资产率')}
                                    </span>
                                    <button
                                        type="button"
                                        onClick={() => navigate('/accounts')}
                                        className="px-3 py-1 bg-gray-900 hover:bg-gray-800 dark:bg-white/10 dark:hover:bg-white/15 text-white text-[11px] font-semibold rounded-lg transition-colors flex items-center gap-1 cursor-pointer shadow-xs"
                                    >
                                        <span>{t('dashboard.view_all_accounts', '查看账号')}</span>
                                        <ArrowRight className="w-3 h-3" />
                                    </button>
                                </div>
                            </motion.div>

                            {/* 卡片 2: Gemini 算力池 */}
                            {/* 卡片 2: Gemini 算力池 */}
                            <motion.div
                                whileHover={{ y: -2 }}
                                transition={{ duration: 0.15 }}
                                className="bg-white dark:bg-[#15161b] rounded-2xl p-5 shadow-sm dark:shadow-xl dark:shadow-black/40 border border-gray-100 dark:border-white/[0.10] flex flex-col justify-between"
                            >
                                <div className="space-y-3">
                                    <div className="flex items-center justify-between">
                                        <div className="flex items-center gap-2">
                                            <div className="p-1.5 rounded-lg bg-emerald-50 dark:bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 border border-transparent dark:border-emerald-500/20">
                                                <Sparkles className="w-4 h-4" />
                                            </div>
                                            <span className="text-xs font-bold text-gray-800 dark:text-zinc-200">
                                                {t('accounts.quota_group_gemini', 'Gemini 模型')}
                                            </span>
                                        </div>
                                        <span className={`px-2 py-0.5 rounded text-[10px] font-bold ${
                                            geminiQuotaValue >= 50
                                                ? 'bg-emerald-50 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300 dark:border dark:border-emerald-500/30'
                                                : 'bg-amber-50 text-amber-700 dark:bg-amber-500/15 dark:text-amber-300 dark:border dark:border-amber-500/30'
                                        }`}>
                                            {geminiQuotaValue >= 50 ? t('dashboard.quota_sufficient_short', '充足') : t('dashboard.quota_tight_short', '偏紧')}
                                        </span>
                                    </div>

                                    {/* 核心大字 (随 quotaView 联动) */}
                                    <div className="flex items-baseline justify-between">
                                        <div>
                                            <span className="text-3xl font-black font-mono tracking-tight text-gray-900 dark:text-white">
                                                {geminiQuotaValue}%
                                            </span>
                                            <span className="text-[11px] text-gray-400 dark:text-zinc-400 block mt-0.5">
                                                {geminiQuotaLabel}
                                            </span>
                                        </div>
                                    </div>

                                    {/* 双核配额微度规 */}
                                    <div className="pt-2">
                                        <div className="grid grid-cols-2 gap-2">
                                            {/* 5H 滚动子卡槽 */}
                                            <button
                                                type="button"
                                                onClick={() => setQuotaView('5h')}
                                                className={`p-2 rounded-xl border text-left transition-all duration-200 cursor-pointer ${
                                                    quotaView === '5h'
                                                        ? 'bg-emerald-500/10 border-emerald-500/30 dark:bg-emerald-500/20 dark:border-emerald-500/50 shadow-xs'
                                                        : 'bg-gray-50/70 hover:bg-gray-100/70 dark:bg-white/[0.04] dark:hover:bg-white/[0.08] border-gray-100 dark:border-white/[0.08]'
                                                }`}
                                            >
                                                <div className="flex items-center justify-between text-[11px] mb-1.5">
                                                    <span className={`text-[10px] font-medium ${quotaView === '5h' ? 'text-emerald-700 dark:text-emerald-300 font-bold' : 'text-gray-400 dark:text-zinc-400'}`}>
                                                        {t('dashboard.view_mode_5h', '5H 滚动')}
                                                    </span>
                                                    <span className={`font-mono text-xs font-bold ${quotaView === '5h' ? 'text-emerald-700 dark:text-emerald-300' : 'text-gray-700 dark:text-zinc-200'}`}>
                                                        {stats.gemini.avg5h}%
                                                    </span>
                                                </div>
                                                <div className="h-1.5 w-full bg-gray-200/80 dark:bg-white/10 rounded-full overflow-hidden">
                                                    <div
                                                        className="h-full bg-gradient-to-r from-emerald-500 to-teal-400 rounded-full transition-all duration-300 shadow-[0_0_6px_rgba(16,185,129,0.5)]"
                                                        style={{ width: `${Math.min(stats.gemini.avg5h, 100)}%` }}
                                                    />
                                                </div>
                                            </button>

                                            {/* 7天周配额子卡槽 */}
                                            <button
                                                type="button"
                                                onClick={() => setQuotaView('weekly')}
                                                className={`p-2 rounded-xl border text-left transition-all duration-200 cursor-pointer ${
                                                    quotaView === 'weekly'
                                                        ? 'bg-teal-500/10 border-teal-500/30 dark:bg-teal-500/20 dark:border-teal-500/50 shadow-xs'
                                                        : 'bg-gray-50/70 hover:bg-gray-100/70 dark:bg-white/[0.04] dark:hover:bg-white/[0.08] border-gray-100 dark:border-white/[0.08]'
                                                }`}
                                            >
                                                <div className="flex items-center justify-between text-[11px] mb-1.5">
                                                    <span className={`text-[10px] font-medium ${quotaView === 'weekly' ? 'text-teal-700 dark:text-teal-300 font-bold' : 'text-gray-400 dark:text-zinc-400'}`}>
                                                        {t('dashboard.view_mode_weekly', '7天周配额')}
                                                    </span>
                                                    <span className={`font-mono text-xs font-bold ${quotaView === 'weekly' ? 'text-teal-700 dark:text-teal-300' : 'text-gray-700 dark:text-zinc-200'}`}>
                                                        {stats.gemini.avgWeekly}%
                                                    </span>
                                                </div>
                                                <div className="h-1.5 w-full bg-gray-200/80 dark:bg-white/10 rounded-full overflow-hidden">
                                                    <div
                                                        className="h-full bg-gradient-to-r from-teal-500 to-cyan-400 rounded-full transition-all duration-300 shadow-[0_0_6px_rgba(20,184,166,0.5)]"
                                                        style={{ width: `${Math.min(stats.gemini.avgWeekly, 100)}%` }}
                                                    />
                                                </div>
                                            </button>
                                        </div>
                                    </div>
                                </div>

                                <div className="pt-2.5 mt-2 border-t border-gray-100 dark:border-white/[0.08] text-[10px] text-gray-400 dark:text-zinc-400 flex items-center justify-between">
                                    <span>{t('dashboard.gemini_group_desc', '包含 Pro / Flash / 绘图')}</span>
                                    {stats.gemini.zeroWeeklyCount > 0 && (
                                        <span className="text-rose-600 dark:text-rose-400 font-bold flex items-center gap-0.5">
                                            <AlertTriangle size={10} /> {stats.gemini.zeroWeeklyCount} {t('dashboard.circuit_break', '熔断')}
                                        </span>
                                    )}
                                </div>
                            </motion.div>

                            {/* 卡片 3: Claude & GPT 算力池 */}
                            <motion.div
                                whileHover={{ y: -2 }}
                                transition={{ duration: 0.15 }}
                                className="bg-white dark:bg-[#15161b] rounded-2xl p-5 shadow-sm dark:shadow-xl dark:shadow-black/40 border border-gray-100 dark:border-white/[0.10] flex flex-col justify-between"
                            >
                                <div className="space-y-3">
                                    <div className="flex items-center justify-between">
                                        <div className="flex items-center gap-2">
                                            <div className="p-1.5 rounded-lg bg-cyan-50 dark:bg-cyan-500/15 text-cyan-600 dark:text-cyan-400 border border-transparent dark:border-cyan-500/20">
                                                <Bot className="w-4 h-4" />
                                            </div>
                                            <span className="text-xs font-bold text-gray-800 dark:text-zinc-200">
                                                {t('accounts.quota_group_claude', 'Claude & GPT')}
                                            </span>
                                        </div>
                                        <span className={`px-2 py-0.5 rounded text-[10px] font-bold ${
                                            claudeQuotaValue === null
                                                ? 'bg-gray-100 text-gray-500 dark:bg-white/10 dark:text-zinc-400'
                                                : claudeQuotaValue >= 50
                                                    ? 'bg-emerald-50 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300 dark:border dark:border-emerald-500/30'
                                                    : 'bg-amber-50 text-amber-700 dark:bg-amber-500/15 dark:text-amber-300 dark:border dark:border-amber-500/30'
                                        }`}>
                                            {claudeQuotaValue === null ? '-' : (claudeQuotaValue >= 50 ? t('dashboard.quota_sufficient_short', '充足') : t('dashboard.quota_tight_short', '偏紧'))}
                                        </span>
                                    </div>

                                    {/* 核心大字 (随 quotaView 联动) */}
                                    <div className="flex items-baseline justify-between">
                                        <div>
                                            <span className="text-3xl font-black font-mono tracking-tight text-gray-900 dark:text-white">
                                                {claudeQuotaValue !== null ? `${claudeQuotaValue}%` : '-'}
                                            </span>
                                            <span className="text-[11px] text-gray-400 dark:text-zinc-400 block mt-0.5">
                                                {claudeQuotaLabel}
                                            </span>
                                        </div>
                                    </div>

                                    {/* 双核配额微度规 */}
                                    <div className="pt-2">
                                        <div className="grid grid-cols-2 gap-2">
                                            {/* 5H 滚动子卡槽 */}
                                            <button
                                                type="button"
                                                onClick={() => setQuotaView('5h')}
                                                className={`p-2 rounded-xl border text-left transition-all duration-200 cursor-pointer ${
                                                    quotaView === '5h'
                                                        ? 'bg-cyan-500/10 border-cyan-500/30 dark:bg-cyan-500/20 dark:border-cyan-500/50 shadow-xs'
                                                        : 'bg-gray-50/70 hover:bg-gray-100/70 dark:bg-white/[0.04] dark:hover:bg-white/[0.08] border-gray-100 dark:border-white/[0.08]'
                                                }`}
                                            >
                                                <div className="flex items-center justify-between text-[11px] mb-1.5">
                                                    <span className={`text-[10px] font-medium ${quotaView === '5h' ? 'text-cyan-700 dark:text-cyan-300 font-bold' : 'text-gray-400 dark:text-zinc-400'}`}>
                                                        {t('dashboard.view_mode_5h', '5H 滚动')}
                                                    </span>
                                                    <span className={`font-mono text-xs font-bold ${quotaView === '5h' ? 'text-cyan-700 dark:text-cyan-300' : 'text-gray-700 dark:text-zinc-200'}`}>
                                                        {hasClaudePool ? `${stats.claude.avg5h}%` : '-'}
                                                    </span>
                                                </div>
                                                <div className="h-1.5 w-full bg-gray-200/80 dark:bg-white/10 rounded-full overflow-hidden">
                                                    <div
                                                        className="h-full bg-gradient-to-r from-cyan-500 to-sky-400 rounded-full transition-all duration-300 shadow-[0_0_6px_rgba(6,182,212,0.5)]"
                                                        style={{ width: `${hasClaudePool ? Math.min(stats.claude.avg5h, 100) : 0}%` }}
                                                    />
                                                </div>
                                            </button>

                                            {/* 7天周配额子卡槽 */}
                                            <button
                                                type="button"
                                                onClick={() => setQuotaView('weekly')}
                                                className={`p-2 rounded-xl border text-left transition-all duration-200 cursor-pointer ${
                                                    quotaView === 'weekly'
                                                        ? 'bg-indigo-500/10 border-indigo-500/30 dark:bg-indigo-500/20 dark:border-indigo-500/50 shadow-xs'
                                                        : 'bg-gray-50/70 hover:bg-gray-100/70 dark:bg-white/[0.04] dark:hover:bg-white/[0.08] border-gray-100 dark:border-white/[0.08]'
                                                }`}
                                            >
                                                <div className="flex items-center justify-between text-[11px] mb-1.5">
                                                    <span className={`text-[10px] font-medium ${quotaView === 'weekly' ? 'text-indigo-700 dark:text-indigo-300 font-bold' : 'text-gray-400 dark:text-zinc-400'}`}>
                                                        {t('dashboard.view_mode_weekly', '7天周配额')}
                                                    </span>
                                                    <span className={`font-mono text-xs font-bold ${quotaView === 'weekly' ? 'text-indigo-700 dark:text-indigo-300' : 'text-gray-700 dark:text-zinc-200'}`}>
                                                        {hasClaudePool ? `${stats.claude.avgWeekly}%` : '-'}
                                                    </span>
                                                </div>
                                                <div className="h-1.5 w-full bg-gray-200/80 dark:bg-white/10 rounded-full overflow-hidden">
                                                    <div
                                                        className="h-full bg-gradient-to-r from-indigo-500 to-purple-400 rounded-full transition-all duration-300 shadow-[0_0_6px_rgba(99,102,241,0.5)]"
                                                        style={{ width: `${hasClaudePool ? Math.min(stats.claude.avgWeekly, 100) : 0}%` }}
                                                    />
                                                </div>
                                            </button>
                                        </div>
                                    </div>
                                </div>

                                <div className="pt-2.5 mt-2 border-t border-gray-100 dark:border-white/[0.08] text-[10px] text-gray-400 dark:text-zinc-400 flex items-center justify-between">
                                    <span>{t('dashboard.claude_group_desc', '第三方多模态池')}</span>
                                    {stats.claude.zeroWeeklyCount > 0 && (
                                        <span className="text-rose-600 dark:text-rose-400 font-bold flex items-center gap-0.5">
                                            <AlertTriangle size={10} /> {stats.claude.zeroWeeklyCount} {t('dashboard.circuit_break', '熔断')}
                                        </span>
                                    )}
                                </div>
                            </motion.div>

                            {/* 卡片 4: 调度风控与健康水位 */}
                            <motion.div
                                whileHover={{ y: -2 }}
                                transition={{ duration: 0.15 }}
                                className="bg-white dark:bg-[#15161b] rounded-2xl p-5 shadow-sm dark:shadow-xl dark:shadow-black/40 border border-gray-100 dark:border-white/[0.10] flex flex-col justify-between"
                            >
                                <div className="space-y-3">
                                    <div className="flex items-center justify-between">
                                        <div className="flex items-center gap-2">
                                            <div className="p-1.5 rounded-lg bg-purple-50 dark:bg-purple-500/15 text-purple-600 dark:text-purple-400 border border-transparent dark:border-purple-500/20">
                                                <Activity className="w-4 h-4" />
                                            </div>
                                            <span className="text-xs font-bold text-gray-800 dark:text-zinc-200">
                                                {t('dashboard.health_and_risk', '健康度 & 风控')}
                                            </span>
                                        </div>
                                        <span className={`px-2 py-0.5 rounded text-[10px] font-bold ${
                                            stats.abnormal === 0
                                                ? 'bg-emerald-50 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300 dark:border dark:border-emerald-500/30'
                                                : 'bg-rose-50 text-rose-700 dark:bg-rose-500/20 dark:text-rose-300 dark:border dark:border-rose-500/40'
                                        }`}>
                                            {stats.abnormal === 0
                                                ? t('dashboard.health_all_green', '全绿无风险')
                                                : t('dashboard.abnormal_badge', { count: stats.abnormal, defaultValue: `${stats.abnormal} 异常` })
                                            }
                                        </span>
                                    </div>

                                    {/* 核心大字 */}
                                    <div className="flex items-baseline justify-between">
                                        <div>
                                            <span className="text-3xl font-black font-mono tracking-tight text-gray-900 dark:text-white">
                                                {Math.round((stats.available / (stats.total || 1)) * 100)}%
                                            </span>
                                            <span className="text-[11px] text-gray-400 dark:text-zinc-400 block mt-0.5">
                                                {t('dashboard.health_available_rate', '健康可用率')}
                                            </span>
                                        </div>
                                    </div>

                                    {/* Promptly 风格的连续竖状胶囊柱阵列 */}
                                    <div className="pt-2">
                                        {(() => {
                                            const total = stats.total || 1;
                                            const availableCount = stats.available;
                                            const abnormalCount = stats.abnormal;
                                            const disabledCount = Math.max(0, stats.total - availableCount - abnormalCount);

                                            // 12格最大余额法精确量化
                                            const shares = [
                                                { type: 'available', count: availableCount, raw: (availableCount / total) * 12 },
                                                { type: 'disabled', count: disabledCount, raw: (disabledCount / total) * 12 },
                                                { type: 'abnormal', count: abnormalCount, raw: (abnormalCount / total) * 12 },
                                            ];

                                            const bars = shares.map((s) => ({
                                                type: s.type,
                                                floor: Math.floor(s.raw),
                                                remainder: s.raw - Math.floor(s.raw),
                                                count: s.count > 0 ? Math.max(1, Math.floor(s.raw)) : 0,
                                            }));

                                            // 纠偏保证总格数恒等于 12
                                            let currentTotal = bars.reduce((acc, b) => acc + b.count, 0);
                                            if (currentTotal > 12) {
                                                while (currentTotal > 12) {
                                                    const candidates = bars.filter((b) => b.count > 1).sort((a, b) => a.remainder - b.remainder);
                                                    if (candidates.length > 0) {
                                                        candidates[0].count--;
                                                        currentTotal--;
                                                    } else break;
                                                }
                                            } else if (currentTotal < 12) {
                                                while (currentTotal < 12) {
                                                    const candidates = [...bars].sort((a, b) => b.remainder - a.remainder);
                                                    candidates[0].count++;
                                                    currentTotal++;
                                                }
                                            }

                                            const availableBars = bars.find((b) => b.type === 'available')?.count || 0;
                                            const abnormalBars = bars.find((b) => b.type === 'abnormal')?.count || 0;

                                            return (
                                                <div
                                                    className="flex items-center justify-between gap-1 h-14 px-2 py-2 bg-gray-50 dark:bg-black/40 rounded-xl border border-gray-100 dark:border-white/[0.08]"
                                                    title={`${availableCount} ${t('dashboard.available_accounts_unit', '可用')} · ${disabledCount} ${t('dashboard.disabled_accounts', '停用')} · ${abnormalCount} ${t('dashboard.abnormal_accounts', '异常')}`}
                                                >
                                                    {Array.from({ length: 12 }).map((_, idx) => {
                                                        let colorClass = 'bg-gray-200 dark:bg-white/10';
                                                        if (idx < availableBars) {
                                                            colorClass = 'bg-gray-900 dark:bg-zinc-200';
                                                        } else if (idx >= 12 - abnormalBars) {
                                                            colorClass = 'bg-rose-500 animate-pulse dark:shadow-[0_0_8px_rgba(244,63,94,0.6)]';
                                                        }

                                                        return (
                                                            <div
                                                                key={idx}
                                                                className={`flex-1 h-full rounded-full transition-all duration-200 ${colorClass}`}
                                                            />
                                                        );
                                                    })}
                                                </div>
                                            );
                                        })()}
                                    </div>
                                </div>

                                <div className="pt-2.5 mt-2 border-t border-gray-100 dark:border-white/[0.08] text-[10px] text-gray-400 dark:text-zinc-400 flex items-center justify-between">
                                    <span>
                                        {stats.abnormal > 0
                                            ? t('dashboard.abnormal_accounts_has_risk', '⚠ 需处理风控验证 / 封禁')
                                            : t('dashboard.abnormal_accounts_no_risk', '✓ 零风控异常账号')
                                        }
                                    </span>
                                    <span className="font-mono font-bold text-gray-700 dark:text-zinc-200">
                                        {stats.available}/{stats.total}
                                    </span>
                                </div>
                            </motion.div>
                        </div>
                    </div>

                {/* 双栏布局 */}
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <CurrentAccount
                        account={currentAccount}
                        accounts={accounts}
                        activeClients={activeClients}
                        onSwitch={(targetIde) => navigate('/accounts', { state: { targetIde } })}
                    />
                    <BestAccounts
                        accounts={accounts}
                        currentAccountId={currentAccount?.id}
                        activeClients={activeClients}
                        onSwitch={handleSwitch}
                    />
                </div>

                {/* 快速链接已精简并入顶部/卡片 */}
            </motion.div>
        </div>
    );
}

export default Dashboard;
