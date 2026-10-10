import { save } from '@tauri-apps/plugin-dialog';
import {
    AlertTriangle,
    ArrowRight,
    Ban,
    Bot,
    Calendar,
    Clock,
    Download,
    RefreshCw,
    ShieldAlert,
    ShieldCheck,
    Sparkles,
    Users,
} from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import AddAccountDialog from '../components/accounts/AddAccountDialog';
import { showToast } from '../components/common/ToastContainer';
import BestAccounts from '../components/dashboard/BestAccounts';
import { findImageQuotaModel, findQuotaModel } from '../config/modelConfig';
import CurrentAccount from '../components/dashboard/CurrentAccount';
import { exportAccounts } from '../services/accountService';
import { useAccountStore } from '../stores/useAccountStore';
import { Account } from '../types/account';
import { isTauri } from '../utils/env';
import { request as invoke } from '../utils/request';
import { DashboardQuotaView } from '../utils/quotaDisplay';

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

    const [onlyAvailable, setOnlyAvailable] = useState<boolean>(() => {
        const saved = localStorage.getItem('dashboard_filter_only_available');
        return saved !== null ? saved === 'true' : true;
    });

    const [quotaView, setQuotaView] = useState<DashboardQuotaView>(() => {
        const saved = localStorage.getItem('dashboard_quota_view');
        return (saved === '5h' || saved === 'weekly' || saved === 'weighted') ? saved : 'weighted';
    });

    useEffect(() => {
        localStorage.setItem('dashboard_filter_only_available', String(onlyAvailable));
    }, [onlyAvailable]);

    useEffect(() => {
        localStorage.setItem('dashboard_quota_view', quotaView);
    }, [quotaView]);

    // 全维度账号健康与配额计算矩阵 (状态由风控定生死，底座池支持仅可用账号与全部正常账号无缝切换)
    const stats = useMemo(() => {
        const nowMs = Date.now();

        // 1. 账号生态健康状态分类（风控与启用状态定生死，不以额度定生死）
        // 异常账号：触发 Google 验证码/风控阻断，或配额接口返回 403 Forbidden 封禁
        const abnormalAccounts = accounts.filter(
            a => a.validation_blocked || a.quota?.is_forbidden
        );
        // 禁用账号：非异常，但用户手动禁用反代或停用账号
        const disabledAccounts = accounts.filter(
            a => !a.validation_blocked && !a.quota?.is_forbidden && (a.disabled || a.proxy_disabled)
        );
        // 可用账号：开启且状态正常、风控正常的生产力账号
        const availableAccounts = accounts.filter(
            a => !a.disabled && !a.proxy_disabled && !a.validation_blocked && !a.quota?.is_forbidden
        );
        // 全部正常状态账号（包含禁用与非禁用，彻底剔除风控异常账号）
        const normalAccounts = accounts.filter(
            a => !a.validation_blocked && !a.quota?.is_forbidden
        );

        // 依据按钮状态选择底座统计池：默认仅看可用账号；关闭则看全部正常状态账号
        const basePool = onlyAvailable
            ? (availableAccounts.length > 0 ? availableAccounts : normalAccounts)
            : normalAccounts;

        // 2. 单账号配额提取辅助函数
        const get5hQuota = (a: Account, modelKey: 'gemini-pro' | 'gemini-image' | 'claude'): number | null => {
            const isClaude = modelKey === 'claude';
            // 优先从 quota_groups 提取原生 5H 滚动配额桶
            if (a.quota?.quota_groups) {
                for (const group of a.quota.quota_groups) {
                    const gname = group.display_name.toLowerCase();
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
            if (a.quota?.quota_groups) {
                for (const group of a.quota.quota_groups) {
                    const gname = group.display_name.toLowerCase();
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
                zeroWeeklyCount,
                coolingCount,
                cappedCount,
            };
        };

        const gemini = computeMetrics('gemini-pro');
        const geminiImage = computeMetrics('gemini-image');
        const claude = computeMetrics('claude');

        // 计算 Gemini / Claude 周配额重置倒计时与日均用量基准
        const getGeminiWeeklyInfo = (a: Account): { quota: number; resetMs: number | null } | null => {
            if (!a.quota || a.quota.is_forbidden) return null;
            if (a.quota.quota_groups) {
                for (const group of a.quota.quota_groups) {
                    const bucket = group.buckets?.find(b =>
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
            if (a.quota.quota_groups) {
                for (const group of a.quota.quota_groups) {
                    const bucket = group.buckets?.find(b =>
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

        const geminiInfos = basePool.map(a => getGeminiWeeklyInfo(a)).filter((i): i is NonNullable<typeof i> => i !== null);
        const claudeInfos = basePool.map(a => getClaudeWeeklyInfo(a)).filter((i): i is NonNullable<typeof i> => i !== null);

        const geminiResetMsList = geminiInfos.map(i => i.resetMs).filter((r): r is number => r !== null);
        const avgGeminiResetMs = geminiResetMsList.length > 0
            ? Math.round(geminiResetMsList.reduce((a, b) => a + b, 0) / geminiResetMsList.length)
            : null;

        const claudeResetMsList = claudeInfos.map(i => i.resetMs).filter((r): r is number => r !== null);
        const avgClaudeResetMs = claudeResetMsList.length > 0
            ? Math.round(claudeResetMsList.reduce((a, b) => a + b, 0) / claudeResetMsList.length)
            : null;

        const avgGemini = gemini.avgWeekly;
        const avgClaude = claude.avgWeekly;

        const avgGeminiResetText = avgGeminiResetMs ? formatResetDuration(avgGeminiResetMs) : null;
        const geminiPacing = calculatePacing(
            avgGemini,
            avgGeminiResetMs,
            basePool.length,
            todayUsage?.gemini_used_pct || 0,
            todayUsage?.gemini_tokens || 0,
            todayUsage?.gemini_requests || 0
        );

        const avgClaudeResetText = avgClaudeResetMs ? formatResetDuration(avgClaudeResetMs) : null;
        const claudePacing = calculatePacing(
            avgClaude,
            avgClaudeResetMs,
            basePool.length,
            todayUsage?.claude_used_pct || 0,
            todayUsage?.claude_tokens || 0,
            todayUsage?.claude_requests || 0
        );

        return {
            total: accounts.length,
            available: availableAccounts.length,
            disabled: disabledAccounts.length,
            abnormal: abnormalAccounts.length,
            normalCount: normalAccounts.length,
            basePoolCount: basePool.length,
            gemini,
            geminiImage,
            claude,
            avgGeminiResetText,
            geminiPacing,
            avgClaudeResetText,
            claudePacing,
        };
    }, [accounts, onlyAvailable, todayUsage]);

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

                {/* 1. 账号生态健康状态阵列 (4 核心卡片，以风控与配置定生死) */}
                <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
                    {/* 总账号数 */}
                    <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
                        <div className="flex items-center justify-between mb-2">
                            <div className="p-1.5 bg-blue-50 dark:bg-blue-900/20 rounded-md">
                                <Users className="w-4 h-4 text-blue-500 dark:text-blue-400" />
                            </div>
                            <span className="text-[10px] font-mono font-bold text-gray-400 dark:text-gray-500">TOTAL</span>
                        </div>
                        <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{stats.total}</div>
                        <div className="text-xs text-gray-500 dark:text-gray-400 font-medium">
                            {t('dashboard.total_accounts', '总账号数')}
                        </div>
                        <div className="text-[10px] text-gray-400 dark:text-gray-500 mt-1.5">
                            {t('dashboard.total_accounts_desc', '已录入配置的全部账号')}
                        </div>
                    </div>

                    {/* 可用账号数 (开启且风控正常，不以额度定生死) */}
                    <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-emerald-100 dark:border-emerald-950/40">
                        <div className="flex items-center justify-between mb-2">
                            <div className="p-1.5 bg-emerald-50 dark:bg-emerald-900/20 rounded-md">
                                <ShieldCheck className="w-4 h-4 text-emerald-600 dark:text-emerald-400" />
                            </div>
                            <span className="text-[10px] font-mono font-bold text-emerald-600 dark:text-emerald-400">ACTIVE</span>
                        </div>
                        <div className="text-2xl font-bold text-emerald-600 dark:text-emerald-400 mb-0.5">{stats.available}</div>
                        <div className="text-xs text-gray-700 dark:text-gray-300 font-bold">
                            {t('dashboard.available_accounts', '可用账号数')}
                        </div>
                        <div className="text-[10px] text-emerald-600/80 dark:text-emerald-400/80 mt-1.5">
                            {t('dashboard.available_accounts_desc', '✓ 开启且状态正常 · 反代服务中')}
                        </div>
                    </div>

                    {/* 禁用账号数 (手动停用) */}
                    <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
                        <div className="flex items-center justify-between mb-2">
                            <div className="p-1.5 bg-gray-100 dark:bg-base-300 rounded-md">
                                <Ban className="w-4 h-4 text-gray-500 dark:text-gray-400" />
                            </div>
                            <span className="text-[10px] font-mono font-bold text-gray-400 dark:text-gray-500">DISABLED</span>
                        </div>
                        <div className="text-2xl font-bold text-gray-600 dark:text-gray-300 mb-0.5">{stats.disabled}</div>
                        <div className="text-xs text-gray-500 dark:text-gray-400 font-medium">
                            {t('dashboard.disabled_accounts', '禁用账号数')}
                        </div>
                        <div className="text-[10px] text-gray-400 dark:text-gray-500 mt-1.5">
                            {t('dashboard.disabled_accounts_desc', '手动停用 / 反代已禁用')}
                        </div>
                    </div>

                    {/* 异常账号数 (风控阻断或403封禁) */}
                    <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
                        <div className="flex items-center justify-between mb-2">
                            <div className={`p-1.5 rounded-md ${stats.abnormal > 0 ? 'bg-rose-50 dark:bg-rose-900/20' : 'bg-gray-50 dark:bg-base-300'}`}>
                                <ShieldAlert className={`w-4 h-4 ${stats.abnormal > 0 ? 'text-rose-600 dark:text-rose-400' : 'text-gray-400'}`} />
                            </div>
                            <span className={`text-[10px] font-mono font-bold ${stats.abnormal > 0 ? 'text-rose-500' : 'text-gray-400'}`}>RISK</span>
                        </div>
                        <div className={`text-2xl font-bold mb-0.5 ${stats.abnormal > 0 ? 'text-rose-600 dark:text-rose-400' : 'text-gray-900 dark:text-base-content'}`}>{stats.abnormal}</div>
                        <div className="text-xs text-gray-500 dark:text-gray-400 font-medium">
                            {t('dashboard.abnormal_accounts', '异常账号数')}
                        </div>
                        <div className={`text-[10px] mt-1.5 ${stats.abnormal > 0 ? 'text-rose-600 font-bold' : 'text-gray-400 dark:text-gray-500'}`}>
                            {stats.abnormal > 0
                                ? t('dashboard.abnormal_accounts_has_risk', '⚠ 需处理风控验证 / 封禁')
                                : t('dashboard.abnormal_accounts_no_risk', '✓ 零风控异常账号')
                            }
                        </div>
                    </div>
                </div>

                {/* 2. 配额生产力矩阵与显眼的双选胶囊控制器 (Pill Capsule Control) */}
                <div className="flex flex-wrap items-center justify-between gap-2 pt-1 pb-1">
                    <div className="flex items-center gap-2">
                        <span className="text-xs font-bold text-gray-800 dark:text-gray-200">
                            {onlyAvailable
                                ? t('dashboard.quota_matrix_active', '可用账号配额生产力矩阵')
                                : t('dashboard.quota_matrix_all_normal', '全量正常账号配额矩阵 (包含已禁用)')
                            }
                        </span>
                        <span className="text-[10px] text-gray-400 dark:text-gray-500 font-mono">
                            ({t('dashboard.base_pool_count', { count: stats.basePoolCount })})
                        </span>
                    </div>

                    <div className="flex flex-wrap items-center gap-2">
                        {/* 显眼的配额视窗三态胶囊控制器 (综合加权 | 5H 滚动 | 7天周配额) */}
                        <div className="flex items-center gap-1 bg-gray-200/80 dark:bg-base-300 p-1 rounded-full shadow-inner border border-gray-200/80 dark:border-base-200 select-none">
                            <button
                                type="button"
                                onClick={() => setQuotaView('weighted')}
                                className={`px-3 py-1.5 rounded-full text-xs font-bold transition-all flex items-center gap-1 cursor-pointer ${
                                    quotaView === 'weighted'
                                        ? 'bg-indigo-600 text-white shadow-md scale-100 ring-2 ring-indigo-400/30'
                                        : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 hover:bg-black/5 dark:hover:bg-white/5'
                                }`}
                                title={t('dashboard.view_mode_title_weighted')}
                            >
                                <Sparkles className="w-3.5 h-3.5" />
                                <span>{t('dashboard.view_mode_weighted', '综合加权')}</span>
                            </button>
                            <button
                                type="button"
                                onClick={() => setQuotaView('5h')}
                                className={`px-3 py-1.5 rounded-full text-xs font-bold transition-all flex items-center gap-1 cursor-pointer ${
                                    quotaView === '5h'
                                        ? 'bg-emerald-600 text-white shadow-md scale-100 ring-2 ring-emerald-400/30'
                                        : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 hover:bg-black/5 dark:hover:bg-white/5'
                                }`}
                                title={t('dashboard.view_mode_title_5h')}
                            >
                                <Clock className="w-3.5 h-3.5" />
                                <span>{t('dashboard.view_mode_5h', '5H 滚动')}</span>
                            </button>
                            <button
                                type="button"
                                onClick={() => setQuotaView('weekly')}
                                className={`px-3 py-1.5 rounded-full text-xs font-bold transition-all flex items-center gap-1 cursor-pointer ${
                                    quotaView === 'weekly'
                                        ? 'bg-purple-600 text-white shadow-md scale-100 ring-2 ring-purple-400/30'
                                        : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 hover:bg-black/5 dark:hover:bg-white/5'
                                }`}
                                title={t('dashboard.view_mode_title_weekly')}
                            >
                                <Calendar className="w-3.5 h-3.5" />
                                <span>{t('dashboard.view_mode_weekly', '7天周配额')}</span>
                            </button>
                        </div>

                        {/* 显眼的双选胶囊控制器 (Pill Capsule) */}
                        <div className="flex items-center gap-1 bg-gray-200/80 dark:bg-base-300 p-1 rounded-full shadow-inner border border-gray-200/80 dark:border-base-200 select-none">
                            <button
                                type="button"
                                onClick={() => setOnlyAvailable(true)}
                                className={`px-3.5 py-1.5 rounded-full text-xs font-bold transition-all flex items-center gap-1.5 cursor-pointer ${
                                    onlyAvailable
                                        ? 'bg-emerald-600 text-white shadow-md scale-100 ring-2 ring-emerald-400/30'
                                        : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 hover:bg-black/5 dark:hover:bg-white/5'
                                }`}
                                title={t('dashboard.btn_title_only_available', '当前：仅看开启且正常的可用账号 (点击包含已禁用账号)')}
                            >
                                <ShieldCheck className="w-3.5 h-3.5" />
                                <span>
                                    {t('dashboard.btn_only_available', { count: stats.available })}
                                </span>
                            </button>
                            <button
                                type="button"
                                onClick={() => setOnlyAvailable(false)}
                                className={`px-3.5 py-1.5 rounded-full text-xs font-bold transition-all flex items-center gap-1.5 cursor-pointer ${
                                    !onlyAvailable
                                        ? 'bg-blue-600 text-white shadow-md scale-100 ring-2 ring-blue-400/30'
                                        : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 hover:bg-black/5 dark:hover:bg-white/5'
                                }`}
                                title={t('dashboard.btn_title_include_disabled', '当前：查看全部正常状态账号包含禁用 (点击仅看可用账号)')}
                            >
                                <Users className="w-3.5 h-3.5" />
                                <span>
                                    {t('dashboard.btn_include_disabled', { count: stats.normalCount })}
                                </span>
                            </button>
                        </div>
                    </div>
                </div>

                {/* 3 大模型卡片：展示 5H均值、周配额均值及周额度熔断加权配额 */}
                {(() => {
                    const getCardQuota = (stat: typeof stats.gemini) => {
                        if (quotaView === '5h') {
                            return {
                                val: stat.avg5h,
                                label: t('dashboard.view_mode_5h', '5H 滚动可用'),
                                isTight: stat.avg5h < 50,
                            };
                        }
                        if (quotaView === 'weekly') {
                            return {
                                val: stat.avgWeekly,
                                label: t('dashboard.view_mode_weekly', '7天周配额'),
                                isTight: stat.avgWeekly < 50,
                            };
                        }
                        return {
                            val: stat.weightedEffective,
                            label: t('dashboard.weighted_available', '综合加权可用'),
                            isTight: stat.weightedEffective < 50,
                        };
                    };

                    const geminiCard = getCardQuota(stats.gemini);
                    const geminiImageCard = getCardQuota(stats.geminiImage);
                    const claudeCard = getCardQuota(stats.claude);

                    return (
                        <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
                            {/* Gemini 文本模型配额 */}
                            <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200 flex flex-col justify-between">
                                <div>
                                    <div className="flex items-center justify-between mb-2">
                                        <div className="flex items-center gap-2">
                                            <div className="p-1.5 bg-green-50 dark:bg-green-900/20 rounded-md">
                                                <Sparkles className="w-4 h-4 text-green-500 dark:text-green-400" />
                                            </div>
                                            <span className="text-xs font-bold text-gray-800 dark:text-gray-200">
                                                {t('dashboard.gemini_available_quota', 'Gemini 可用配额')}
                                            </span>
                                        </div>
                                        <div className="flex items-center gap-1.5">
                                            {stats.avgGeminiResetText && (
                                                <span className="text-[10px] text-gray-400 dark:text-gray-500 flex items-center gap-0.5 bg-gray-50 dark:bg-base-200/50 px-1.5 py-0.5 rounded">
                                                    <Clock className="w-2.5 h-2.5 inline" />
                                                    {stats.avgGeminiResetText}
                                                </span>
                                            )}
                                            <span className={`px-2 py-0.5 rounded text-[10px] font-bold ${!geminiCard.isTight ? 'bg-green-50 text-green-700 dark:bg-green-900/30 dark:text-green-300' : 'bg-amber-50 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300'}`}>
                                                {!geminiCard.isTight
                                                    ? t('dashboard.quota_sufficient_short', '充足')
                                                    : t('dashboard.quota_tight_short', '偏紧')
                                                }
                                            </span>
                                        </div>
                                    </div>

                                    <div className="flex items-baseline gap-2 mb-2">
                                        <span className="text-3xl font-extrabold text-gray-900 dark:text-base-content font-mono">
                                            {geminiCard.val}%
                                        </span>
                                        <span className="text-[11px] text-gray-400 dark:text-gray-500 font-medium">
                                            {geminiCard.label}
                                        </span>
                                    </div>
                                </div>

                                <div className="pt-2 border-t border-gray-100 dark:border-base-300/60 flex items-center justify-between text-[11px]">
                                    <div className="flex items-center gap-1.5">
                                        <span className="text-gray-400 dark:text-gray-500">{t('dashboard.rolling_5h', '5小时滚动:')}</span>
                                        <span className="font-mono font-bold text-emerald-600 dark:text-emerald-400">{stats.gemini.avg5h}%</span>
                                    </div>
                                    <div className="flex items-center gap-1.5">
                                        <span className="text-gray-400 dark:text-gray-500">{t('dashboard.weekly_7d', '7天周配额:')}</span>
                                        <span className={`font-mono font-bold ${stats.gemini.avgWeekly <= 10 ? 'text-amber-600 dark:text-amber-400' : 'text-gray-700 dark:text-gray-300'}`}>{stats.gemini.avgWeekly}%</span>
                                    </div>
                                </div>
                                {stats.gemini.zeroWeeklyCount > 0 && (
                                    <div className="text-[10px] text-amber-600 dark:text-amber-400 mt-1.5 font-medium flex items-center gap-1">
                                        <AlertTriangle className="w-3 h-3 shrink-0" />
                                        <span>{t('dashboard.zero_weekly_warning', { count: stats.gemini.zeroWeeklyCount })}</span>
                                    </div>
                                )}
                                {quotaView === '5h' && stats.gemini.cappedCount > 0 && stats.gemini.zeroWeeklyCount === 0 && (
                                    <div className="text-[10px] text-amber-600 dark:text-amber-400 mt-1.5 font-medium flex items-center gap-1">
                                        <AlertTriangle className="w-3 h-3 shrink-0" />
                                        <span>{stats.gemini.cappedCount} 个账号 5H 上限受到周配额压制</span>
                                    </div>
                                )}
                                {quotaView === 'weekly' && stats.gemini.coolingCount > 0 && (
                                    <div className="text-[10px] text-blue-600 dark:text-blue-400 mt-1.5 font-medium flex items-center gap-1">
                                        <Clock className="w-3 h-3 shrink-0" />
                                        <span>{stats.gemini.coolingCount} 个账号当前处于 5H 冷却冻结中</span>
                                    </div>
                                )}
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

                            {/* Gemini 绘图模型配额 */}
                            <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200 flex flex-col justify-between">
                                <div>
                                    <div className="flex items-center justify-between mb-2">
                                        <div className="flex items-center gap-2">
                                            <div className="p-1.5 bg-purple-50 dark:bg-purple-900/20 rounded-md">
                                                <Sparkles className="w-4 h-4 text-purple-500 dark:text-purple-400" />
                                            </div>
                                            <span className="text-xs font-bold text-gray-800 dark:text-gray-200">
                                                {t('dashboard.gemini_image_quota', 'Gemini 绘图配额')}
                                            </span>
                                        </div>
                                        <span className={`px-2 py-0.5 rounded text-[10px] font-bold ${!geminiImageCard.isTight ? 'bg-purple-50 text-purple-700 dark:bg-purple-900/30 dark:text-purple-300' : 'bg-amber-50 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300'}`}>
                                            {!geminiImageCard.isTight
                                                ? t('dashboard.quota_sufficient_short', '充足')
                                                : t('dashboard.quota_tight_short', '偏紧')
                                            }
                                        </span>
                                    </div>

                                    <div className="flex items-baseline gap-2 mb-2">
                                        <span className="text-3xl font-extrabold text-gray-900 dark:text-base-content font-mono">
                                            {geminiImageCard.val}%
                                        </span>
                                        <span className="text-[11px] text-gray-400 dark:text-gray-500 font-medium">
                                            {geminiImageCard.label}
                                        </span>
                                    </div>
                                </div>

                                <div className="pt-2 border-t border-gray-100 dark:border-base-300/60 flex items-center justify-between text-[11px]">
                                    <div className="flex items-center gap-1.5">
                                        <span className="text-gray-400 dark:text-gray-500">{t('dashboard.rolling_5h', '5小时滚动:')}</span>
                                        <span className="font-mono font-bold text-emerald-600 dark:text-emerald-400">{stats.geminiImage.avg5h}%</span>
                                    </div>
                                    <div className="flex items-center gap-1.5">
                                        <span className="text-gray-400 dark:text-gray-500">{t('dashboard.weekly_7d', '7天周配额:')}</span>
                                        <span className={`font-mono font-bold ${stats.geminiImage.avgWeekly <= 10 ? 'text-amber-600 dark:text-amber-400' : 'text-gray-700 dark:text-gray-300'}`}>{stats.geminiImage.avgWeekly}%</span>
                                    </div>
                                </div>
                                {stats.geminiImage.zeroWeeklyCount > 0 && (
                                    <div className="text-[10px] text-amber-600 dark:text-amber-400 mt-1.5 font-medium flex items-center gap-1">
                                        <AlertTriangle className="w-3 h-3 shrink-0" />
                                        <span>{t('dashboard.zero_weekly_warning', { count: stats.geminiImage.zeroWeeklyCount })}</span>
                                    </div>
                                )}
                                {quotaView === '5h' && stats.geminiImage.cappedCount > 0 && stats.geminiImage.zeroWeeklyCount === 0 && (
                                    <div className="text-[10px] text-amber-600 dark:text-amber-400 mt-1.5 font-medium flex items-center gap-1">
                                        <AlertTriangle className="w-3 h-3 shrink-0" />
                                        <span>{stats.geminiImage.cappedCount} 个账号 5H 上限受到周配额压制</span>
                                    </div>
                                )}
                                {quotaView === 'weekly' && stats.geminiImage.coolingCount > 0 && (
                                    <div className="text-[10px] text-blue-600 dark:text-blue-400 mt-1.5 font-medium flex items-center gap-1">
                                        <Clock className="w-3 h-3 shrink-0" />
                                        <span>{stats.geminiImage.coolingCount} 个账号当前处于 5H 冷却冻结中</span>
                                    </div>
                                )}
                            </div>

                            {/* Claude 模型配额 */}
                            <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200 flex flex-col justify-between">
                                <div>
                                    <div className="flex items-center justify-between mb-2">
                                        <div className="flex items-center gap-2">
                                            <div className="p-1.5 bg-cyan-50 dark:bg-cyan-900/20 rounded-md">
                                                <Bot className="w-4 h-4 text-cyan-500 dark:text-cyan-400" />
                                            </div>
                                            <span className="text-xs font-bold text-gray-800 dark:text-gray-200">
                                                {t('dashboard.claude_available_quota', 'Claude 可用配额')}
                                            </span>
                                        </div>
                                        <div className="flex items-center gap-1.5">
                                            {stats.avgClaudeResetText && (
                                                <span className="text-[10px] text-gray-400 dark:text-gray-500 flex items-center gap-0.5 bg-gray-50 dark:bg-base-200/50 px-1.5 py-0.5 rounded">
                                                    <Clock className="w-2.5 h-2.5 inline" />
                                                    {stats.avgClaudeResetText}
                                                </span>
                                            )}
                                            <span className={`px-2 py-0.5 rounded text-[10px] font-bold ${!claudeCard.isTight ? 'bg-cyan-50 text-cyan-700 dark:bg-cyan-900/30 dark:text-cyan-300' : 'bg-amber-50 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300'}`}>
                                                {!claudeCard.isTight
                                                    ? t('dashboard.quota_sufficient_short', '充足')
                                                    : t('dashboard.quota_tight_short', '偏紧')
                                                }
                                            </span>
                                        </div>
                                    </div>

                                    <div className="flex items-baseline gap-2 mb-2">
                                        <span className="text-3xl font-extrabold text-gray-900 dark:text-base-content font-mono">
                                            {claudeCard.val}%
                                        </span>
                                        <span className="text-[11px] text-gray-400 dark:text-gray-500 font-medium">
                                            {claudeCard.label}
                                        </span>
                                    </div>
                                </div>

                                <div className="pt-2 border-t border-gray-100 dark:border-base-300/60 flex items-center justify-between text-[11px]">
                                    <div className="flex items-center gap-1.5">
                                        <span className="text-gray-400 dark:text-gray-500">{t('dashboard.rolling_5h', '5小时滚动:')}</span>
                                        <span className="font-mono font-bold text-emerald-600 dark:text-emerald-400">{stats.claude.avg5h}%</span>
                                    </div>
                                    <div className="flex items-center gap-1.5">
                                        <span className="text-gray-400 dark:text-gray-500">{t('dashboard.weekly_7d', '7天周配额:')}</span>
                                        <span className={`font-mono font-bold ${stats.claude.avgWeekly <= 10 ? 'text-amber-600 dark:text-amber-400' : 'text-gray-700 dark:text-gray-300'}`}>{stats.claude.avgWeekly}%</span>
                                    </div>
                                </div>
                                {stats.claude.zeroWeeklyCount > 0 && (
                                    <div className="text-[10px] text-amber-600 dark:text-amber-400 mt-1.5 font-medium flex items-center gap-1">
                                        <AlertTriangle className="w-3 h-3 shrink-0" />
                                        <span>{t('dashboard.zero_weekly_warning', { count: stats.claude.zeroWeeklyCount })}</span>
                                    </div>
                                )}
                                {quotaView === '5h' && stats.claude.cappedCount > 0 && stats.claude.zeroWeeklyCount === 0 && (
                                    <div className="text-[10px] text-amber-600 dark:text-amber-400 mt-1.5 font-medium flex items-center gap-1">
                                        <AlertTriangle className="w-3 h-3 shrink-0" />
                                        <span>{stats.claude.cappedCount} 个账号 5H 上限受到周配额压制</span>
                                    </div>
                                )}
                                {quotaView === 'weekly' && stats.claude.coolingCount > 0 && (
                                    <div className="text-[10px] text-blue-600 dark:text-blue-400 mt-1.5 font-medium flex items-center gap-1">
                                        <Clock className="w-3 h-3 shrink-0" />
                                        <span>{stats.claude.coolingCount} 个账号当前处于 5H 冷却冻结中</span>
                                    </div>
                                )}
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
                        </div>
                    );
                })()}

                {/* 双栏布局 */}
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <CurrentAccount
                        account={currentAccount}
                        quotaView={quotaView}
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
