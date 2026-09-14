import { save } from '@tauri-apps/plugin-dialog';
import { AlertTriangle, ArrowRight, Bot, Clock, Download, RefreshCw, Sparkles, Users } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import AddAccountDialog from '../components/accounts/AddAccountDialog';
import { showToast } from '../components/common/ToastContainer';
import BestAccounts from '../components/dashboard/BestAccounts';
import CurrentAccount from '../components/dashboard/CurrentAccount';
import { exportAccounts } from '../services/accountService';
import { useAccountStore } from '../stores/useAccountStore';
import { Account } from '../types/account';
import { isTauri } from '../utils/env';
import { request as invoke } from '../utils/request';

function formatResetDuration(diffMs: number): string {
    if (diffMs <= 0) return '即将重置';
    const totalMins = Math.floor(diffMs / (1000 * 60));
    const totalHrs = Math.floor(totalMins / 60);
    const days = Math.floor(totalHrs / 24);
    const remainingHrs = totalHrs % 24;

    if (days > 0) {
        return `${days}天${remainingHrs > 0 ? `${remainingHrs}h` : ''}`;
    }
    const mins = totalMins % 60;
    return `${totalHrs}h${mins > 0 ? `${mins}m` : ''}`;
}

function formatTokens(tokens: number): string {
    if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}M`;
    if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}K`;
    return `${tokens}`;
}

export interface TodayUsageSummary {
    gemini_tokens: number;
    gemini_requests: number;
    gemini_used_pct: number;
    claude_tokens: number;
    claude_requests: number;
    claude_used_pct: number;
    total_tokens: number;
    total_requests: number;
}

export interface PacingBenchmark {
    dailyPace: number;
    dailyPaceText: string;
    poolDailyPaceText: string;
    hourlyPaceText: string;
    todayUsedText: string;
    todayUsedPct: number;
    todayTokens: number;
    todayRequests: number;
    tier: 'empty' | 'sprint' | 'over_pace' | 'abundant' | 'balanced' | 'tight';
    tierLabelKey: string;
    tierDefaultLabel: string;
    tierBadgeClass: string;
    tierBgClass: string;
    tooltipText: string;
}

function calculatePacing(
    avgQuota: number,
    avgResetMs: number | null,
    accountCount: number,
    todayUsedPct: number = 0,
    todayTokens: number = 0,
    todayRequests: number = 0
): PacingBenchmark | null {
    if (avgQuota === undefined || avgQuota === null) return null;

    const count = Math.max(1, accountCount);

    // 今日已蹬文字
    const todayUsedText = (todayRequests > 0 || todayTokens > 0 || todayUsedPct > 0)
        ? `${todayUsedPct.toFixed(1)}% · ${formatTokens(todayTokens)} (${todayRequests}次)`
        : '0.0% · 0 Tokens';

    // 1. 配额基本耗尽
    if (avgQuota <= 2) {
        return {
            dailyPace: 0,
            dailyPaceText: '0%/天',
            poolDailyPaceText: '0%/天',
            hourlyPaceText: '0%/h',
            todayUsedText,
            todayUsedPct,
            todayTokens,
            todayRequests,
            tier: 'empty',
            tierLabelKey: 'dashboard.tier_empty',
            tierDefaultLabel: '💤 配额已空，等刷新',
            tierBadgeClass: 'text-gray-500 dark:text-gray-400 font-medium',
            tierBgClass: 'bg-gray-50 dark:bg-gray-800/40 border-gray-200/70 dark:border-gray-700/60',
            tooltipText: `当前平均配额仅剩 ${avgQuota}%，已基本耗尽。今日已用 ${todayUsedText}。`
        };
    }

    const hours = avgResetMs && avgResetMs > 0 ? avgResetMs / (1000 * 60 * 60) : 0.5;
    const days = Math.max(0.05, hours / 24);

    const dailyPace = avgQuota / days;
    const poolDailyPace = dailyPace * count;
    const hourlyPace = avgQuota / Math.max(0.1, hours);

    // 2. 临期冲刺 (不足 6 小时且还有余量 >= 10%)
    if (hours <= 6 && avgQuota >= 10) {
        return {
            dailyPace,
            dailyPaceText: `${dailyPace.toFixed(1)}%/天`,
            poolDailyPaceText: `${poolDailyPace.toFixed(1)}%/天`,
            hourlyPaceText: `${hourlyPace.toFixed(1)}%/h`,
            todayUsedText,
            todayUsedPct,
            todayTokens,
            todayRequests,
            tier: 'sprint',
            tierLabelKey: 'dashboard.tier_sprint',
            tierDefaultLabel: '🔥 临期冲刺，使劲蹬！',
            tierBadgeClass: 'text-emerald-600 dark:text-emerald-400 font-semibold',
            tierBgClass: 'bg-emerald-100/80 dark:bg-emerald-950/50 border-emerald-300 dark:border-emerald-800/60',
            tooltipText: `距离重置仅剩 ${hours.toFixed(1)} 小时，平均还剩 ${avgQuota}%，抓紧加速用完别浪费！今日已蹬: ${todayUsedText}`
        };
    }

    // 3. 动态实时计算今日消耗与全天日均预算比对 (若今日已超速透支)
    if ((todayUsedPct >= Math.max(dailyPace * 1.15, 8) && todayUsedPct >= 5) || (todayUsedPct > dailyPace && todayUsedPct >= 10)) {
        return {
            dailyPace,
            dailyPaceText: `${dailyPace.toFixed(1)}%/天`,
            poolDailyPaceText: `${poolDailyPace.toFixed(1)}%/天`,
            hourlyPaceText: `${hourlyPace.toFixed(2)}%/h`,
            todayUsedText,
            todayUsedPct,
            todayTokens,
            todayRequests,
            tier: 'over_pace',
            tierLabelKey: 'dashboard.tier_over_pace',
            tierDefaultLabel: `⚠️ 今日超速 (已蹬 ${todayUsedPct.toFixed(1)}%)`,
            tierBadgeClass: 'text-amber-700 dark:text-amber-300 font-semibold',
            tierBgClass: 'bg-amber-100/90 dark:bg-amber-950/50 border-amber-300 dark:border-amber-700/60',
            tooltipText: `⚠️ 今日已消耗 ${todayUsedPct.toFixed(1)}%，已超过建议日用预算 (~${dailyPace.toFixed(1)}%/天)；若继续高速消耗可能会提前用尽，建议适当放缓。`
        };
    }

    // 4. 吃紧 / 偏低 (健康基线为 14.3%/天，低于 13.5%/天 说明配额已被透支或余量吃紧)
    if (dailyPace < 13.5 || (avgQuota < 25 && days >= 2)) {
        return {
            dailyPace,
            dailyPaceText: `${dailyPace.toFixed(1)}%/天`,
            poolDailyPaceText: `${poolDailyPace.toFixed(1)}%/天`,
            hourlyPaceText: `${hourlyPace.toFixed(2)}%/h`,
            todayUsedText,
            todayUsedPct,
            todayTokens,
            todayRequests,
            tier: 'tight',
            tierLabelKey: 'dashboard.tier_tight',
            tierDefaultLabel: `🟡 余量吃紧 (~${dailyPace.toFixed(1)}%/天)，悠着点`,
            tierBadgeClass: 'text-amber-600 dark:text-amber-400 font-medium',
            tierBgClass: 'bg-amber-50/80 dark:bg-amber-950/30 border-amber-200/80 dark:border-amber-900/40',
            tooltipText: `剩余配额已落后于健康基线（标准健康日均 ~14.3%/天），距重置还有 ${days.toFixed(1)} 天，建议日均消耗控制在 ~${dailyPace.toFixed(1)}%/天 以内。今日已用: ${todayUsedText}。`
        };
    }

    // 5. 充裕 / 宽松
    if (dailyPace >= 18 || (avgQuota >= 75 && days <= 3.5)) {
        return {
            dailyPace,
            dailyPaceText: `${dailyPace.toFixed(1)}%/天`,
            poolDailyPaceText: `${poolDailyPace.toFixed(1)}%/天`,
            hourlyPaceText: `${hourlyPace.toFixed(2)}%/h`,
            todayUsedText,
            todayUsedPct,
            todayTokens,
            todayRequests,
            tier: 'abundant',
            tierLabelKey: 'dashboard.tier_abundant',
            tierDefaultLabel: '🔥 预算充裕，放心蹬',
            tierBadgeClass: 'text-emerald-600 dark:text-emerald-400 font-medium',
            tierBgClass: 'bg-emerald-50/80 dark:bg-emerald-950/30 border-emerald-200/80 dark:border-emerald-900/40',
            tooltipText: `当前配额充裕，建议日用预算高达 ~${dailyPace.toFixed(1)}%/天。今日已用: ${todayUsedText}。`
        };
    }

    // 6. 健康平衡 (只有在 13.5% ~ 18%/天 且今日未超标的健康状态下才显示)
    return {
        dailyPace,
        dailyPaceText: `${dailyPace.toFixed(1)}%/天`,
        poolDailyPaceText: `${poolDailyPace.toFixed(1)}%/天`,
        hourlyPaceText: `${hourlyPace.toFixed(2)}%/h`,
        todayUsedText,
        todayUsedPct,
        todayTokens,
        todayRequests,
        tier: 'balanced',
        tierLabelKey: 'dashboard.tier_balanced',
        tierDefaultLabel: '⚡ 节奏平稳，照常蹬',
        tierBadgeClass: 'text-blue-600 dark:text-blue-400 font-medium',
        tierBgClass: 'bg-blue-50/80 dark:bg-blue-950/30 border-blue-200/80 dark:border-blue-900/40',
        tooltipText: `建议日用: ~${dailyPace.toFixed(1)}%/天，今日已用: ${todayUsedText}，节奏健康平稳。`
    };
}

function Dashboard() {
    const { t } = useTranslation();
    const navigate = useNavigate();
    const {
        accounts,
        currentAccount,
        fetchAccounts,
        fetchCurrentAccount,
        switchAccount,
        addAccount,
        refreshQuota,
        loading
    } = useAccountStore();

    const [todayUsage, setTodayUsage] = useState<TodayUsageSummary | null>(null);

    const fetchTodayUsage = async () => {
        try {
            const res = await invoke<TodayUsageSummary>('get_today_usage_summary');
            setTodayUsage(res);
        } catch (err) {
            console.error('[Dashboard] Failed to fetch today usage:', err);
        }
    };

    useEffect(() => {
        fetchAccounts();
        fetchCurrentAccount();
        fetchTodayUsage();

        const interval = setInterval(() => {
            fetchTodayUsage();
            fetchAccounts();
        }, 3000);

        return () => clearInterval(interval);
    }, []);

    // 计算统计数据 (包含 Gemini / Claude 周配额平均及周重置平均倒计时与日均基准、今日已蹬)
    const stats = useMemo(() => {
        const nowMs = Date.now();

        const getGeminiWeeklyInfo = (a: Account): { quota: number; resetMs: number | null } | null => {
            if (!a.quota || a.quota.is_forbidden) return null;

            // 1. 优先查 quota_groups 中的 gemini-weekly 桶
            if (a.quota.quota_groups) {
                for (const group of a.quota.quota_groups) {
                    const bucket = group.buckets.find(b =>
                        b.bucket_id === 'gemini-weekly' ||
                        (b.window === 'weekly' && b.bucket_id.toLowerCase().includes('gemini'))
                    );
                    if (bucket) {
                        const pct = Math.round(bucket.remaining_fraction * 100);
                        const resetMs = bucket.reset_time ? new Date(bucket.reset_time).getTime() - nowMs : null;
                        return { quota: pct, resetMs: resetMs && resetMs > 0 ? resetMs : null };
                    }
                }
            }

            // 2. 回退：计算 Gemini 模型
            if (a.quota.models && a.quota.models.length > 0) {
                const geminiModels = a.quota.models.filter(m => {
                    const n = m.name.toLowerCase();
                    return n.includes('gemini') || n.includes('imagen') || n.includes('image');
                });
                if (geminiModels.length > 0) {
                    const bestModel = [...geminiModels].sort((x, y) => (y.percentage || 0) - (x.percentage || 0))[0];
                    const resetMs = bestModel.reset_time ? new Date(bestModel.reset_time).getTime() - nowMs : null;
                    return { quota: bestModel.percentage || 0, resetMs: resetMs && resetMs > 0 ? resetMs : null };
                }
            }

            return null;
        };

        const getClaudeWeeklyInfo = (a: Account): { quota: number; resetMs: number | null } | null => {
            if (!a.quota || a.quota.is_forbidden) return null;

            // 1. 优先查 quota_groups 中的 3p-weekly 桶
            if (a.quota.quota_groups) {
                for (const group of a.quota.quota_groups) {
                    const bucket = group.buckets.find(b =>
                        b.bucket_id === '3p-weekly' ||
                        (b.window === 'weekly' && (b.bucket_id.toLowerCase().includes('3p') || b.bucket_id.toLowerCase().includes('claude')))
                    );
                    if (bucket) {
                        const pct = Math.round(bucket.remaining_fraction * 100);
                        const resetMs = bucket.reset_time ? new Date(bucket.reset_time).getTime() - nowMs : null;
                        return { quota: pct, resetMs: resetMs && resetMs > 0 ? resetMs : null };
                    }
                }
            }

            // 2. 回退：计算 Claude 模型
            if (a.quota.models && a.quota.models.length > 0) {
                const claudeModels = a.quota.models.filter(m => {
                    const n = m.name.toLowerCase();
                    return n.includes('claude') || n === '3p-5h' || n === '3p-weekly';
                });
                if (claudeModels.length > 0) {
                    const bestModel = [...claudeModels].sort((x, y) => (y.percentage || 0) - (x.percentage || 0))[0];
                    const resetMs = bestModel.reset_time ? new Date(bestModel.reset_time).getTime() - nowMs : null;
                    return { quota: bestModel.percentage || 0, resetMs: resetMs && resetMs > 0 ? resetMs : null };
                }
            }

            return null;
        };

        const geminiInfos = accounts.map(a => getGeminiWeeklyInfo(a)).filter((i): i is NonNullable<typeof i> => i !== null);
        const claudeInfos = accounts.map(a => getClaudeWeeklyInfo(a)).filter((i): i is NonNullable<typeof i> => i !== null);

        const geminiQuotas = geminiInfos.map(i => i.quota);
        const claudeQuotas = claudeInfos.map(i => i.quota);

        const geminiResetMsList = geminiInfos.map(i => i.resetMs).filter((r): r is number => r !== null);
        const avgGeminiResetMs = geminiResetMsList.length > 0
            ? Math.round(geminiResetMsList.reduce((a, b) => a + b, 0) / geminiResetMsList.length)
            : null;

        const claudeResetMsList = claudeInfos.map(i => i.resetMs).filter((r): r is number => r !== null);
        const avgClaudeResetMs = claudeResetMsList.length > 0
            ? Math.round(claudeResetMsList.reduce((a, b) => a + b, 0) / claudeResetMsList.length)
            : null;

        const avgGemini = geminiQuotas.length > 0
            ? Math.round(geminiQuotas.reduce((a, b) => a + b, 0) / geminiQuotas.length)
            : 0;

        const avgClaude = claudeQuotas.length > 0
            ? Math.round(claudeQuotas.reduce((a, b) => a + b, 0) / claudeQuotas.length)
            : 0;

        const lowQuotaCount = accounts.filter(a => {
            if (a.quota?.is_forbidden) return false;
            const g = getGeminiWeeklyInfo(a);
            const c = getClaudeWeeklyInfo(a);
            if (!g && !c) return false;
            const gemini = g?.quota ?? 100;
            const claude = c?.quota ?? 100;
            return gemini < 20 && claude < 20;
        }).length;

        return {
            total: accounts.length,
            avgGemini,
            avgGeminiResetText: avgGeminiResetMs ? formatResetDuration(avgGeminiResetMs) : null,
            geminiPacing: calculatePacing(
                avgGemini,
                avgGeminiResetMs,
                accounts.length,
                todayUsage?.gemini_used_pct || 0,
                todayUsage?.gemini_tokens || 0,
                todayUsage?.gemini_requests || 0
            ),
            avgClaude,
            avgClaudeResetText: avgClaudeResetMs ? formatResetDuration(avgClaudeResetMs) : null,
            claudePacing: calculatePacing(
                avgClaude,
                avgClaudeResetMs,
                accounts.length,
                todayUsage?.claude_used_pct || 0,
                todayUsage?.claude_tokens || 0,
                todayUsage?.claude_requests || 0
            ),
            lowQuota: lowQuotaCount,
        };
    }, [accounts, todayUsage]);

    const isSwitchingRef = useRef(false);

    const handleSwitch = async (accountId: string) => {
        if (loading || isSwitchingRef.current) return;

        isSwitchingRef.current = true;
        console.log('[Dashboard] handleSwitch called for', accountId);
        try {
            await switchAccount(accountId);
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
            await fetchTodayUsage();
            showToast(t('dashboard.toast.refresh_success'), 'success');
        } catch (error) {
            console.error('[Dashboard] Refresh failed:', error);
            showToast(`${t('dashboard.toast.refresh_error')}: ${error}`, 'error');
        } finally {
            setIsRefreshing(false);
        }
    };

    const exportAccountsToJson = async (accountsToExport: Account[]) => {
        try {
            if (accountsToExport.length === 0) {
                showToast(t('dashboard.toast.export_no_accounts'), 'warning');
                return;
            }

            // Get export data from API (contains refresh_token)
            const accountIds = accountsToExport.map(acc => acc.id);
            const response = await exportAccounts(accountIds);

            if (!response.accounts || response.accounts.length === 0) {
                showToast(t('dashboard.toast.export_no_accounts'), 'warning');
                return;
            }

            const exportData = response.accounts;
            const content = JSON.stringify(exportData, null, 2);
            const fileName = `antigravity_accounts_${new Date().toISOString().split('T')[0]}.json`;

            if (isTauri()) {
                const path = await save({
                    filters: [{
                        name: 'JSON',
                        extensions: ['json']
                    }],
                    defaultPath: fileName
                });

                if (!path) return;

                await invoke('save_text_file', { path, content });
                showToast(t('dashboard.toast.export_success', { path }), 'success');
            } else {
                // Web 模式：使用浏览器下载
                const blob = new Blob([content], { type: 'application/json' });
                const url = URL.createObjectURL(blob);
                const a = document.createElement('a');
                a.href = url;
                a.download = fileName;
                document.body.appendChild(a);
                a.click();
                document.body.removeChild(a);
                URL.revokeObjectURL(url);
                showToast(t('dashboard.toast.export_success', { path: fileName }), 'success');
            }
        } catch (error: any) {
            console.error('Export failed:', error);
            showToast(`${t('dashboard.toast.export_error')}: ${error.toString()}`, 'error');
        }
    };

    const handleExport = () => {
        exportAccountsToJson(accounts);
    };

    return (
        <div className="h-full w-full overflow-y-auto">
            <div
                className="p-5 space-y-4 max-w-7xl mx-auto"
                onMouseMove={() => console.log('Mouse moving over Dashboard')}
                style={{ position: 'relative', zIndex: 1 }}
            >
                {/* 问候语和操作按钮 */}
                <div
                    className="flex justify-between items-center"
                >
                    <div>
                        <h1 className="text-2xl font-bold text-gray-900 dark:text-base-content">
                            {currentAccount
                                ? t('dashboard.hello').replace('用户', currentAccount.name || currentAccount.email.split('@')[0])
                                : t('dashboard.hello')
                            }
                        </h1>
                    </div>
                    <div className="flex gap-2">
                        <AddAccountDialog onAdd={handleAddAccount} />
                        <button
                            className={`px-3 py-1.5 bg-blue-500 text-white text-xs font-medium rounded-lg hover:bg-blue-600 transition-colors flex items-center gap-1.5 shadow-sm ${isRefreshing || !currentAccount ? 'opacity-70 cursor-not-allowed' : ''}`}
                            onClick={handleRefreshCurrent}
                            disabled={isRefreshing || !currentAccount}
                            title={isRefreshing ? t('dashboard.refreshing') : t('dashboard.refresh_quota')}
                        >
                            <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin' : ''}`} />
                            <span className="hidden sm:inline">{isRefreshing ? t('dashboard.refreshing') : t('dashboard.refresh_quota')}</span>
                        </button>
                    </div>
                </div>

                {/* 统计卡片 - 4 栏布局（包含 Gemini 周配额及 Claude 周配额及平均刷新倒计时与今日消耗/使用基准） */}
                <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
                    <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200 flex flex-col justify-between">
                        <div>
                            <div className="flex items-center justify-between mb-2">
                                <div className="p-1.5 bg-blue-50 dark:bg-blue-900/20 rounded-md">
                                    <Users className="w-4 h-4 text-blue-500 dark:text-blue-400" />
                                </div>
                            </div>
                            <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{stats.total}</div>
                            <div className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.total_accounts')}</div>
                        </div>
                    </div>

                    <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200 flex flex-col justify-between">
                        <div>
                            <div className="flex items-center justify-between mb-2">
                                <div className="p-1.5 bg-green-50 dark:bg-green-900/20 rounded-md">
                                    <Sparkles className="w-4 h-4 text-green-500 dark:text-green-400" />
                                </div>
                                {stats.avgGeminiResetText && (
                                    <span className="text-[10px] text-gray-400 dark:text-gray-500 flex items-center gap-0.5 bg-gray-50 dark:bg-base-200/50 px-1.5 py-0.5 rounded">
                                        <Clock className="w-2.5 h-2.5 inline" />
                                        {stats.avgGeminiResetText}
                                    </span>
                                )}
                            </div>
                            <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{stats.avgGemini}%</div>
                            <div className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.avg_gemini', 'Gemini 平均配额')}</div>
                        </div>

                        {stats.geminiPacing && (
                            <div
                                className="mt-3 pt-2 border-t border-gray-100 dark:border-base-200/80 space-y-1 cursor-default"
                                title={stats.geminiPacing.tooltipText}
                            >
                                <div className="flex items-center justify-between text-[11px] gap-1">
                                    <span className="text-gray-500 dark:text-gray-400 font-medium shrink-0">{t('dashboard.today_used', '今日已蹬')}:</span>
                                    <span className="font-semibold text-emerald-600 dark:text-emerald-400 text-right" title={stats.geminiPacing.todayUsedText}>
                                        {stats.geminiPacing.todayUsedText}
                                    </span>
                                </div>
                                <div className="flex items-center justify-between text-[11px]">
                                    <span className="text-gray-500 dark:text-gray-400 font-medium">{t('dashboard.daily_pace_label', '建议日用')}:</span>
                                    <span className="font-semibold text-gray-800 dark:text-gray-200">
                                        ~{stats.geminiPacing.dailyPaceText}
                                    </span>
                                </div>
                                <div className={`text-[11px] py-0.5 px-2 rounded border text-center font-medium transition-all ${stats.geminiPacing.tierBgClass} ${stats.geminiPacing.tierBadgeClass}`}>
                                    {stats.geminiPacing.tierDefaultLabel}
                                </div>
                            </div>
                        )}
                    </div>

                    <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200 flex flex-col justify-between">
                        <div>
                            <div className="flex items-center justify-between mb-2">
                                <div className="p-1.5 bg-cyan-50 dark:bg-cyan-900/20 rounded-md">
                                    <Bot className="w-4 h-4 text-cyan-500 dark:text-cyan-400" />
                                </div>
                                {stats.avgClaudeResetText && (
                                    <span className="text-[10px] text-gray-400 dark:text-gray-500 flex items-center gap-0.5 bg-gray-50 dark:bg-base-200/50 px-1.5 py-0.5 rounded">
                                        <Clock className="w-2.5 h-2.5 inline" />
                                        {stats.avgClaudeResetText}
                                    </span>
                                )}
                            </div>
                            <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{stats.avgClaude}%</div>
                            <div className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.avg_claude', 'Claude 平均配额')}</div>
                        </div>

                        {stats.claudePacing && (
                            <div
                                className="mt-3 pt-2 border-t border-gray-100 dark:border-base-200/80 space-y-1 cursor-default"
                                title={stats.claudePacing.tooltipText}
                            >
                                <div className="flex items-center justify-between text-[11px] gap-1">
                                    <span className="text-gray-500 dark:text-gray-400 font-medium shrink-0">{t('dashboard.today_used', '今日已蹬')}:</span>
                                    <span className="font-semibold text-cyan-600 dark:text-cyan-400 text-right" title={stats.claudePacing.todayUsedText}>
                                        {stats.claudePacing.todayUsedText}
                                    </span>
                                </div>
                                <div className="flex items-center justify-between text-[11px]">
                                    <span className="text-gray-500 dark:text-gray-400 font-medium">{t('dashboard.daily_pace_label', '建议日用')}:</span>
                                    <span className="font-semibold text-gray-800 dark:text-gray-200">
                                        ~{stats.claudePacing.dailyPaceText}
                                    </span>
                                </div>
                                <div className={`text-[11px] py-0.5 px-2 rounded border text-center font-medium transition-all ${stats.claudePacing.tierBgClass} ${stats.claudePacing.tierBadgeClass}`}>
                                    {stats.claudePacing.tierDefaultLabel}
                                </div>
                            </div>
                        )}
                    </div>

                    <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200 flex flex-col justify-between">
                        <div>
                            <div className="flex items-center justify-between mb-2">
                                <div className="p-1.5 bg-orange-50 dark:bg-orange-900/20 rounded-md">
                                    <AlertTriangle className="w-4 h-4 text-orange-500 dark:text-orange-400" />
                                </div>
                            </div>
                            <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{stats.lowQuota}</div>
                            <div className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.low_quota_accounts')}</div>
                            <div className="text-[10px] text-gray-400 dark:text-gray-500 mt-1">{t('dashboard.quota_desc')}</div>
                        </div>
                    </div>
                </div>

                {/* 双栏布局 */}
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <CurrentAccount
                        account={currentAccount}
                        onSwitch={() => navigate('/accounts')}
                    />
                    <BestAccounts
                        accounts={accounts}
                        currentAccountId={currentAccount?.id}
                        onSwitch={handleSwitch}
                    />
                </div>

                {/* 快速链接 */}
                <div className="grid grid-cols-2 gap-3">
                    <button
                        className="bg-indigo-50 dark:bg-indigo-900/20 rounded-lg p-3 shadow-sm border border-indigo-100 dark:border-indigo-900/30 hover:border-indigo-300 dark:hover:border-indigo-700 hover:shadow-md transition-all flex items-center justify-between group"
                        onClick={() => navigate('/accounts')}
                    >
                        <span className="text-indigo-700 dark:text-indigo-300 font-medium text-sm">{t('dashboard.view_all_accounts')}</span>
                        <ArrowRight className="w-4 h-4 text-indigo-400 dark:text-indigo-500 group-hover:text-indigo-600 dark:group-hover:text-indigo-300 group-hover:translate-x-1 transition-all" />
                    </button>
                    <button
                        className="bg-purple-50 dark:bg-purple-900/20 rounded-lg p-3 shadow-sm border border-purple-100 dark:border-purple-900/30 hover:border-purple-300 dark:hover:border-purple-700 hover:shadow-md transition-all flex items-center justify-between group"
                        onClick={handleExport}
                    >
                        <span className="text-purple-700 dark:text-purple-300 font-medium text-sm">{t('dashboard.export_data')}</span>
                        <Download className="w-4 h-4 text-purple-400 dark:text-purple-500 group-hover:text-purple-600 dark:group-hover:text-purple-300 transition-all" />
                    </button>
                </div>
            </div>
        </div>
    );
}

export default Dashboard;
