import { useEffect, useState, useRef, useMemo } from 'react';
import { Maximize2, RefreshCw, Clock, ShieldAlert, Tag, Activity, Users } from 'lucide-react';
import { useViewStore } from '../../stores/useViewStore';
import { useAccountStore } from '../../stores/useAccountStore';
import { isTauri } from '../../utils/env';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { motion, AnimatePresence } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import clsx from 'clsx';
import { formatTimeRemaining, formatCompactNumber } from '../../utils/format';
import { enterMiniMode, exitMiniMode } from '../../utils/windowManager';
import { getModelDisplayName, findQuotaModel, findImageQuotaModel } from '../../config/modelConfig';
import { getModelConstrainedQuota, DashboardQuotaView } from '../../utils/quotaDisplay';
import { getVersion } from '@tauri-apps/api/app';
import { listen } from '@tauri-apps/api/event';
import { useConfigStore } from '../../stores/useConfigStore';

interface ProxyRequestLog {
    id: string;
    model?: string;
    input_tokens?: number;
    output_tokens?: number;
    timestamp: number;
    status: number;
    duration: number;
    mapped_model?: string;
}

type MiniScopeMode = 'single' | 'pool';

interface MiniQuotaRowProps {
    displayName: string;
    percentage: number;
    colorClass?: 'emerald' | 'cyan' | 'purple';
    tag?: React.ReactNode;
    resetText?: string;
    resetTooltip?: string;
    isWeeklyExhausted?: boolean;
}

function MiniQuotaRow({
    displayName,
    percentage,
    colorClass = 'emerald',
    tag,
    resetText,
    resetTooltip,
    isWeeklyExhausted = false,
}: MiniQuotaRowProps) {
    const p = percentage;

    const getStatusColor = (pct: number) => {
        if (isWeeklyExhausted) return 'text-rose-500';
        if (pct >= 50) return 'text-emerald-500';
        if (pct >= 20) return 'text-amber-500';
        return 'text-rose-500';
    };

    const getBarColor = (pct: number) => {
        if (isWeeklyExhausted) return 'bg-gradient-to-r from-rose-400 to-rose-500';
        if (pct >= 50) {
            if (colorClass === 'cyan') return 'bg-gradient-to-r from-cyan-400 to-cyan-500';
            if (colorClass === 'purple') return 'bg-gradient-to-r from-purple-400 to-purple-500';
            return 'bg-gradient-to-r from-emerald-400 to-emerald-500';
        }
        if (pct >= 20) {
            if (colorClass === 'cyan') return 'bg-gradient-to-r from-orange-400 to-orange-500';
            return 'bg-gradient-to-r from-amber-400 to-amber-500';
        }
        return 'bg-gradient-to-r from-rose-400 to-rose-500';
    };

    return (
        <motion.div
            layout
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            className="space-y-1.5"
        >
            <div className="flex justify-between items-baseline">
                <span className="text-xs font-medium text-gray-600 dark:text-gray-400 truncate max-w-[145px]" title={displayName}>
                    {displayName}
                </span>
                <div className="flex items-center gap-1.5 shrink-0">
                    {tag}
                    {resetText && (
                        <span className="text-[10px] text-blue-600 dark:text-blue-400 font-mono" title={resetTooltip}>
                            {resetText}
                        </span>
                    )}
                    <span className={clsx("text-xs font-bold font-mono", getStatusColor(p))}>
                        {p}%
                    </span>
                </div>
            </div>
            <div className="w-full bg-gray-100 dark:bg-white/10 rounded-full h-1.5 overflow-hidden">
                <motion.div
                    initial={{ width: 0 }}
                    animate={{ width: `${p}%` }}
                    transition={{ duration: 0.8, ease: "easeOut" }}
                    className={clsx("h-full rounded-full shadow-[0_0_8px_currentColor]", getBarColor(p))}
                />
            </div>
        </motion.div>
    );
}

export default function MiniView() {
    const { setMiniView } = useViewStore();
    const { accounts, currentAccount, refreshQuota, refreshAllQuotas, fetchAccounts, fetchCurrentAccount } = useAccountStore();
    const { config } = useConfigStore();
    const { t } = useTranslation();
    const [isRefreshing, setIsRefreshing] = useState(false);
    const containerRef = useRef<HTMLDivElement>(null);
    const [appVersion, setAppVersion] = useState('0.0.0');
    const [latestLog, setLatestLog] = useState<ProxyRequestLog | null>(null);

    const [scopeMode, setScopeMode] = useState<MiniScopeMode>(() => {
        const saved = localStorage.getItem('mini_view_scope');
        return (saved === 'single' || saved === 'pool') ? saved : 'single';
    });

    const [quotaView, setQuotaView] = useState<DashboardQuotaView>(() => {
        const saved = localStorage.getItem('dashboard_quota_view');
        return (saved === '5h' || saved === 'weekly' || saved === 'weighted') ? saved : 'weighted';
    });

    useEffect(() => {
        localStorage.setItem('mini_view_scope', scopeMode);
    }, [scopeMode]);

    useEffect(() => {
        localStorage.setItem('dashboard_quota_view', quotaView);
    }, [quotaView]);

    useEffect(() => {
        let unlistenFn: (() => void) | null = null;

        const setupListener = async () => {
            if (!isTauri()) return;
            try {
                unlistenFn = await listen<ProxyRequestLog>('proxy://request', (event) => {
                    setLatestLog(event.payload);
                });
            } catch (e) {
                console.error('Failed to setup log listener:', e);
            }
        };

        setupListener();

        return () => {
            if (unlistenFn) unlistenFn();
        };
    }, []);

    useEffect(() => {
        const fetchVersion = async () => {
            if (isTauri()) {
                try {
                    const version = await getVersion();
                    setAppVersion(version);
                } catch (error) {
                    console.error('Failed to get app version:', error);
                }
            } else {
                // Fallback for web mode if needed, or import from package.json
                setAppVersion('4.9.6');
            }
        };
        fetchVersion();
    }, []);

    useEffect(() => {
        fetchAccounts();
        fetchCurrentAccount();
    }, [fetchAccounts, fetchCurrentAccount]);

    const handleRefresh = async () => {
        if (isRefreshing) return;
        if (scopeMode === 'single' && !currentAccount) return;

        setIsRefreshing(true);
        try {
            if (scopeMode === 'pool') {
                await refreshAllQuotas();
                await fetchCurrentAccount();
            } else {
                if (currentAccount) {
                    await refreshQuota(currentAccount.id);
                    await fetchCurrentAccount();
                }
            }
        } finally {
            setTimeout(() => setIsRefreshing(false), 800);
        }
    };

    const handleRefreshRef = useRef(handleRefresh);
    handleRefreshRef.current = handleRefresh;

    useEffect(() => {
        if (!config?.auto_refresh || !config?.refresh_interval || config.refresh_interval <= 0) return;

        const intervalId = setInterval(() => {
            handleRefreshRef.current();
        }, config.refresh_interval * 60 * 1000);

        return () => clearInterval(intervalId);
    }, [config?.auto_refresh, config?.refresh_interval]);

    useEffect(() => {
        const adjustSize = async () => {
            if (isTauri() && containerRef.current) {
                const height = containerRef.current.scrollHeight;
                await enterMiniMode(height);
            }
        };

        const timer = setTimeout(adjustSize, 50);
        return () => clearTimeout(timer);
    }, [currentAccount, scopeMode, accounts, quotaView]);

    const handleMaximize = async () => {
        await exitMiniMode();
        setMiniView(false);
    };

    const handleMouseDown = () => {
        if (isTauri()) {
            getCurrentWindow().startDragging();
        }
    };

    const availableAccounts = useMemo(() => {
        return accounts.filter(
            a => !a.disabled && !a.proxy_disabled && !a.validation_blocked && !a.quota?.is_forbidden
        );
    }, [accounts]);

    const activePool = useMemo(() => {
        return availableAccounts.length > 0
            ? availableAccounts
            : accounts.filter(a => !a.validation_blocked && !a.quota?.is_forbidden);
    }, [availableAccounts, accounts]);

    const poolMetrics = useMemo(() => {
        const calculateModelMetrics = (modelKey: 'gemini-pro' | 'gemini-image' | 'claude') => {
            if (activePool.length === 0) {
                return { avg: 0, count: 0, zeroCount: 0, constrainedCount: 0, coolingCount: 0, nearestReset: null as string | null };
            }

            let sumEffective = 0;
            let count = 0;
            let zeroCount = 0;
            let constrainedCount = 0;
            let coolingCount = 0;
            let nearestResetMs: number | null = null;

            for (const acc of activePool) {
                const model = modelKey === 'gemini-image'
                    ? findImageQuotaModel(acc.quota?.models)
                    : findQuotaModel(acc.quota?.models, modelKey);
                if (!model) continue;

                const q = getModelConstrainedQuota(model.name, model, acc.quota?.quota_groups, quotaView);
                sumEffective += q.effectivePercentage;
                count++;
                if (q.effectivePercentage <= 0) {
                    zeroCount++;
                }
                if (quotaView === '5h' && q.isWeeklyConstrained && q.effectivePercentage > 0) {
                    constrainedCount++;
                }
                if (quotaView === 'weekly' && q.is5hCooling) {
                    coolingCount++;
                }

                if (q.resetTime) {
                    const resetMs = new Date(q.resetTime).getTime();
                    if (!isNaN(resetMs) && resetMs > Date.now()) {
                        if (nearestResetMs === null || resetMs < nearestResetMs) {
                            nearestResetMs = resetMs;
                        }
                    }
                }
            }

            const avg = count > 0 ? Math.round(sumEffective / count) : 0;
            const nearestReset = nearestResetMs ? new Date(nearestResetMs).toISOString() : null;

            return { avg, count, zeroCount, constrainedCount, coolingCount, nearestReset };
        };

        return {
            gemini: calculateModelMetrics('gemini-pro'),
            geminiImage: calculateModelMetrics('gemini-image'),
            claude: calculateModelMetrics('claude'),
        };
    }, [activePool, quotaView]);

    const geminiProModel = findQuotaModel(currentAccount?.quota?.models, 'gemini-pro');
    const geminiFlashModel = findQuotaModel(currentAccount?.quota?.models, 'gemini-flash');
    const claudeModel = findQuotaModel(currentAccount?.quota?.models, 'claude');

    const renderAccountModelRow = (model: any, displayName: string, colorClass: 'emerald' | 'cyan' | 'purple') => {
        if (!model) return null;

        const q = getModelConstrainedQuota(model.name, model, currentAccount?.quota?.quota_groups, quotaView);
        const resetTooltip = [
            q.fiveHourResetTime ? `5H: ${new Date(q.fiveHourResetTime).toLocaleTimeString()}` : null,
            q.weeklyResetTime ? `周: ${new Date(q.weeklyResetTime).toLocaleDateString()}` : null,
        ].filter(Boolean).join(' | ');

        const tag = q.isWeeklyExhausted ? (
            <span className="text-[9px] font-bold text-rose-500 bg-rose-50 dark:bg-rose-900/30 px-1 py-0.5 rounded" title={t('dashboard.weekly_exhausted_badge', '周额度熔断')}>
                {t('dashboard.mini_tag_exhausted', '熔断')}
            </span>
        ) : quotaView === '5h' && q.isWeeklyConstrained && q.raw5h !== null && q.rawWeekly !== null ? (
            <span className="text-[9px] font-bold text-amber-600 dark:text-amber-400" title={t('dashboard.constrained_by_weekly_desc', { raw5h: q.raw5h, rawWeekly: q.rawWeekly, effective: q.effectivePercentage })}>
                [{t('dashboard.mini_tag_constrained', '周限')}: {q.rawWeekly}%]
            </span>
        ) : quotaView === 'weekly' && q.is5hCooling ? (
            <span className="text-[9px] font-bold text-blue-600 dark:text-blue-400" title={t('dashboard.cooling_5h_desc', { rawWeekly: q.rawWeekly })}>
                [{t('dashboard.mini_tag_cooling', '冷却')}]
            </span>
        ) : null;

        return (
            <MiniQuotaRow
                key={model.name}
                displayName={displayName}
                percentage={q.effectivePercentage}
                colorClass={colorClass}
                tag={tag}
                resetText={q.resetTime ? `R: ${formatTimeRemaining(q.resetTime)}` : t('common.unknown')}
                resetTooltip={resetTooltip || `${t('accounts.reset_time')}: ${model.reset_time}`}
                isWeeklyExhausted={q.isWeeklyExhausted}
            />
        );
    };

    const renderPoolMetricRow = (
        displayName: string,
        metric: { avg: number; count: number; zeroCount: number; constrainedCount: number; coolingCount: number; nearestReset: string | null },
        colorClass: 'emerald' | 'cyan' | 'purple'
    ) => {
        const tag = (
            <div className="flex items-center gap-1 shrink-0">
                {metric.zeroCount > 0 && (
                    <span className="text-[9px] font-bold text-rose-500 bg-rose-50 dark:bg-rose-900/30 px-1 py-0.5 rounded" title={t('dashboard.mini_pool_zero_tooltip', '{{count}} 个账号额度见底/熔断', { count: metric.zeroCount })}>
                        {t('dashboard.mini_pool_zero_tag', '{{count}}枯竭', { count: metric.zeroCount })}
                    </span>
                )}
                {quotaView === '5h' && metric.constrainedCount > 0 && (
                    <span className="text-[9px] font-bold text-amber-600 dark:text-amber-400" title={t('dashboard.mini_pool_constrained_tooltip', '{{count}} 个账号 5H 额度受 7 天周配额短板压制', { count: metric.constrainedCount })}>
                        [{t('dashboard.mini_tag_constrained', '周限')}: {t('dashboard.account_count_unit', '{{count}}个', { count: metric.constrainedCount })}]
                    </span>
                )}
                {quotaView === 'weekly' && metric.coolingCount > 0 && (
                    <span className="text-[9px] font-bold text-blue-600 dark:text-blue-400" title={t('dashboard.mini_pool_cooling_tooltip', '{{count}} 个账号处于 5H 冷却态', { count: metric.coolingCount })}>
                        [{t('dashboard.mini_tag_cooling', '冷却')}: {t('dashboard.account_count_unit', '{{count}}个', { count: metric.coolingCount })}]
                    </span>
                )}
            </div>
        );

        return (
            <MiniQuotaRow
                key={displayName}
                displayName={displayName}
                percentage={metric.avg}
                colorClass={colorClass}
                tag={tag}
                resetText={metric.nearestReset ? `R: ${formatTimeRemaining(metric.nearestReset)}` : t('common.unknown')}
                resetTooltip={metric.nearestReset ? `${t('dashboard.nearest_reset', '最近重置')}: ${new Date(metric.nearestReset).toLocaleTimeString()}` : t('common.unknown')}
            />
        );
    };

    return (
        <div className="h-screen w-full flex items-center justify-center bg-transparent">
            <motion.div
                ref={containerRef}
                initial={{ opacity: 0, scale: 0.95 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.95 }}
                className="w-[328px] flex flex-col bg-white/80 dark:bg-[#121212]/80 backdrop-blur-md shadow-2xl overflow-hidden border-x border-y border-gray-200/50 dark:border-white/10 sm:rounded-2xl"
            >
                <div
                    className="flex-none flex items-center justify-between px-3.5 py-1.5 bg-gray-50/50 dark:bg-white/5 border-b border-gray-100 dark:border-white/5 select-none"
                    onMouseDown={handleMouseDown}
                    data-tauri-drag-region
                >
                    <div className="flex items-center gap-1.5 text-xs font-semibold text-gray-900 dark:text-white overflow-hidden max-w-[136px]">
                        <div className={clsx("w-2 h-2 rounded-full shrink-0 animate-pulse", scopeMode === 'pool' ? "bg-indigo-500 shadow-[0_0_8px_rgba(99,102,241,0.5)]" : "bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.4)]")} />
                        {scopeMode === 'pool' ? (
                            <span className="truncate" title={t('dashboard.account_pool_tooltip', '账号池 ({{available}}/{{total}} 可用)', { available: availableAccounts.length, total: accounts.length })}>
                                {t('dashboard.account_pool', '账号池')} ({availableAccounts.length}/{accounts.length})
                            </span>
                        ) : (
                            <span className="truncate" title={currentAccount?.email}>
                                {currentAccount?.email?.split('@')[0] || t('dashboard.no_active_account', '未选账号')}
                            </span>
                        )}
                    </div>

                    <div
                        className="flex items-center gap-1 no-drag shrink-0"
                        onMouseDown={(e) => e.stopPropagation()}
                    >
                        <div className="flex items-center bg-gray-200/70 dark:bg-white/10 rounded-md p-0.5 text-[9px] font-bold">
                            <button
                                onClick={() => setScopeMode('single')}
                                className={clsx(
                                    "px-1.5 py-0.5 rounded transition-all",
                                    scopeMode === 'single' ? "bg-emerald-600 text-white shadow-xs" : "text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white"
                                )}
                                title={t('dashboard.single_account_view', '单账号监测')}
                            >
                                {t('dashboard.mini_scope_single_short', '单')}
                            </button>
                            <button
                                onClick={() => setScopeMode('pool')}
                                className={clsx(
                                    "px-1.5 py-0.5 rounded transition-all",
                                    scopeMode === 'pool' ? "bg-indigo-600 text-white shadow-xs" : "text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white"
                                )}
                                title={t('dashboard.pool_matrix_view', '全账号池矩阵')}
                            >
                                {t('dashboard.mini_scope_pool_short', '池')}
                            </button>
                        </div>

                        <div className="w-px h-3 bg-gray-300 dark:bg-white/20 mx-0.5" />

                        <div className="flex items-center bg-gray-200/70 dark:bg-white/10 rounded-md p-0.5 text-[9px] font-bold">
                            <button
                                onClick={() => setQuotaView('weighted')}
                                className={clsx(
                                    "px-1.5 py-0.5 rounded transition-all",
                                    quotaView === 'weighted' ? "bg-indigo-600 text-white shadow-xs" : "text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white"
                                )}
                                title={t('dashboard.view_mode_title_weighted', '综合加权')}
                            >
                                {t('dashboard.mini_view_weighted_short', '综')}
                            </button>
                            <button
                                onClick={() => setQuotaView('5h')}
                                className={clsx(
                                    "px-1.5 py-0.5 rounded transition-all",
                                    quotaView === '5h' ? "bg-emerald-600 text-white shadow-xs" : "text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white"
                                )}
                                title={t('dashboard.view_mode_title_5h', '5H 滚动')}
                            >
                                {t('dashboard.mini_view_5h_short', '5H')}
                            </button>
                            <button
                                onClick={() => setQuotaView('weekly')}
                                className={clsx(
                                    "px-1.5 py-0.5 rounded transition-all",
                                    quotaView === 'weekly' ? "bg-purple-600 text-white shadow-xs" : "text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white"
                                )}
                                title={t('dashboard.view_mode_title_weekly', '7天周配额')}
                            >
                                {t('dashboard.mini_view_weekly_short', '周')}
                            </button>
                        </div>

                        <div className="w-px h-3 bg-gray-300 dark:bg-white/20 mx-0.5" />

                        <button
                            onClick={handleRefresh}
                            className={clsx(
                                "p-1.5 rounded-lg hover:bg-gray-200/50 dark:hover:bg-white/10 transition-colors"
                            )}
                            title={t('common.refresh', 'Refresh')}
                        >
                            <RefreshCw size={13} className={clsx(isRefreshing && "animate-spin text-blue-500")} />
                        </button>
                        <div className="w-px h-3 bg-gray-300 dark:bg-white/20 mx-0.5" />
                        <button
                            onClick={handleMaximize}
                            className="p-1.5 rounded-lg hover:bg-gray-200/50 dark:hover:bg-white/10 transition-colors text-gray-500 hover:text-gray-900 dark:text-gray-400 dark:hover:text-white"
                            title={t('common.maximize', 'Full View')}
                        >
                            <Maximize2 size={13} />
                        </button>
                    </div>
                </div>

                <div className="flex-1 overflow-y-auto overflow-x-hidden p-4 space-y-4 scrollbar-thin scrollbar-track-transparent scrollbar-thumb-gray-200 dark:scrollbar-thumb-white/10">
                    {scopeMode === 'pool' ? (
                        accounts.length === 0 ? (
                            <div className="h-full flex flex-col items-center justify-center text-center opacity-50 space-y-2 py-4">
                                <ShieldAlert size={32} />
                                <p className="text-sm">{t('dashboard.no_accounts', '账号池为空')}</p>
                            </div>
                        ) : (
                            <AnimatePresence mode='popLayout'>
                                <div className="space-y-4">
                                    {renderPoolMetricRow(t('dashboard.gemini_available_quota', 'Gemini 可用配额'), poolMetrics.gemini, 'emerald')}
                                    {renderPoolMetricRow(t('dashboard.gemini_image_quota', 'Gemini 绘图配额'), poolMetrics.geminiImage, 'purple')}
                                    {renderPoolMetricRow(t('dashboard.claude_available_quota', 'Claude 可用配额'), poolMetrics.claude, 'cyan')}
                                </div>
                            </AnimatePresence>
                        )
                    ) : !currentAccount ? (
                        <div className="h-full flex flex-col items-center justify-center text-center opacity-50 space-y-2 py-4">
                            <ShieldAlert size={32} />
                            <p className="text-sm">{t('dashboard.no_active_account', '未选当前账号')}</p>
                            {accounts.length > 0 && (
                                <button
                                    onClick={() => setScopeMode('pool')}
                                    className="text-xs text-indigo-600 dark:text-indigo-400 hover:underline flex items-center gap-1 mt-1"
                                >
                                    <Users className="w-3 h-3" />
                                    {t('dashboard.switch_to_pool_view', '切换到全池矩阵视图')}
                                </button>
                            )}
                        </div>
                    ) : (
                        <div className="space-y-4">
                            {currentAccount.custom_label && (
                                <div className="flex flex-wrap gap-2">
                                    <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-orange-100 dark:bg-orange-900/30 text-orange-600 dark:text-orange-400 text-[10px] font-bold shadow-sm shrink-0">
                                        <Tag className="w-2.5 h-2.5" />
                                        {currentAccount.custom_label}
                                    </span>
                                </div>
                            )}

                            <AnimatePresence mode='popLayout'>
                                <div className="space-y-4 !mt-0">
                                    {renderAccountModelRow(geminiProModel, getModelDisplayName(geminiProModel), 'emerald')}
                                    {renderAccountModelRow(geminiFlashModel, getModelDisplayName(geminiFlashModel), 'emerald')}
                                    {renderAccountModelRow(claudeModel, getModelDisplayName(claudeModel, t('common.claude_series', 'Claude 系列')), 'cyan')}

                                    {!geminiProModel && !geminiFlashModel && !claudeModel && (
                                        <div className="text-center py-4 text-xs text-gray-400">
                                            {t('dashboard.no_quota_data', 'No quota data available')}
                                        </div>
                                    )}
                                </div>
                            </AnimatePresence>
                        </div>
                    )}
                </div>

                <div className="flex-none h-8 bg-gray-50 dark:bg-black/20 flex items-center justify-between px-3 text-[10px] text-gray-500 dark:text-gray-400 border-t border-gray-100 dark:border-white/5 overflow-hidden">
                    {latestLog ? (
                        <motion.div
                            key={latestLog.id}
                            initial={{ opacity: 0, y: 5 }}
                            animate={{ opacity: 1, y: 0 }}
                            className="flex items-center w-full gap-2"
                        >
                            <span title={latestLog.status.toString()} className={`w-1.5 h-1.5 rounded-full ${latestLog.status >= 200 && latestLog.status < 400 ? 'bg-emerald-500' : 'bg-red-500'}`}></span>
                            <span className="font-bold truncate max-w-[100px]" title={latestLog.model}>
                                {latestLog.mapped_model || latestLog.model}
                            </span>

                            <div className="flex-1 flex items-center justify-end gap-2">
                                <div className="flex items-center gap-1.5 text-[9px]" title="Input/Output Tokens">
                                    <Activity size={10} className="text-blue-500" />
                                    <span className="flex items-center gap-0.5 text-gray-500 dark:text-gray-400">
                                        I:<span className="font-mono text-gray-900 dark:text-gray-200">{formatCompactNumber(latestLog.input_tokens || 0)}</span>
                                    </span>
                                    <span className="text-gray-300 dark:text-gray-600">/</span>
                                    <span className="flex items-center gap-0.5 text-gray-500 dark:text-gray-400">
                                        O:<span className="font-mono text-gray-900 dark:text-gray-200">{formatCompactNumber(latestLog.output_tokens || 0)}</span>
                                    </span>
                                </div>

                                <div className="w-px h-2.5 bg-gray-300 dark:bg-white/10" />

                                <div className="flex items-center gap-0.5" title="Duration">
                                    <Clock size={10} className="text-gray-400" />
                                    <span className="font-mono">{(latestLog.duration / 1000).toFixed(2)}s</span>
                                </div>
                            </div>
                        </motion.div>
                    ) : (
                        <>
                            <div className="flex items-center gap-1.5">
                                <div className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
                                <span>Connected</span>
                            </div>
                            <span className="font-mono opacity-50">v{appVersion}</span>
                        </>
                    )}
                </div>
            </motion.div>
        </div>
    );
}
