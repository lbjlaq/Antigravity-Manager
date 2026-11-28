import React, { useState, useEffect, useCallback, useRef } from 'react';
import {
  AppWindow,
  Code2,
  Puzzle,
  Terminal,
  Sparkles,
  RefreshCw,
  CheckCircle2,
  ChevronDown,
  Zap,
  ShieldCheck,
  TrendingUp,
  Clock,
  Gauge,
  AlertTriangle,
  Users,
} from 'lucide-react';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { api, HostTarget, BackendAccount } from '../api';
import { tUi } from '../i18n/ui';
import {
  readStoredDashboardHost,
  readStoredDashboardPickStrategy,
  readStoredDashboardPoolPreference,
  saveDashboardHost,
  saveDashboardPickStrategy,
  saveDashboardPoolPreference,
  hydrateUiStateFromConfig,
} from '../utils/uiState';
import {
  getAccountTier,
  isAccountAbnormal,
  isAccountRestricted,
  tierSortWeight,
} from '../utils/accountStatus';

interface DashboardProps {
  lang: string;
}

interface HostAccountItem {
  id: string;
  email: string;
  name: string;
  tag: string;
  claude: number;
  gemini: number;
  active: boolean;
}

type PickStrategy = 'max-quota' | 'expiring-soon' | 'tier-first' | 'least-used';
type PoolPreference = 'balanced' | 'claude' | 'gemini';

interface CandidateAccount {
  id: string;
  email: string;
  name: string;
  tier: string;
  claude: number;
  gemini: number;
  reset: string;
  resetTs: number | null;
  lastUsed: number;
  restricted: boolean;
  status: 'healthy' | 'warning';
}

function tierRank(tier: string): number {
  return tierSortWeight(tier);
}

function resolveSubscriptionTier(acc: BackendAccount): string {
  return getAccountTier(acc);
}

export const Dashboard: React.FC<DashboardProps> = ({ lang }) => {
  // 1. 快捷多端切号状态（自动记忆宿主、选号策略与模型池偏好）
  const initialStoredHostRef = useRef<HostTarget | null>(readStoredDashboardHost());
  const initialStoredStrategyRef = useRef<PickStrategy | null>(readStoredDashboardPickStrategy());
  const initialStoredPoolRef = useRef<PoolPreference | null>(readStoredDashboardPoolPreference());
  const touchedRef = useRef({ host: false, strategy: false, pool: false });

  const [selectedHost, setSelectedHost] = useState<HostTarget>(
    () => initialStoredHostRef.current ?? 'app'
  );
  const [pickStrategy, setPickStrategy] = useState<PickStrategy>(
    () => initialStoredStrategyRef.current ?? 'max-quota'
  );
  const [recommendStrategy, setRecommendStrategy] = useState<PoolPreference>(
    () => initialStoredPoolRef.current ?? 'balanced'
  );

  const handleSelectHost = useCallback((host: HostTarget) => {
    touchedRef.current.host = true;
    setSelectedHost(host);
    saveDashboardHost(host);
  }, []);

  const handleSelectPickStrategy = useCallback((strategy: PickStrategy) => {
    touchedRef.current.strategy = true;
    setPickStrategy(strategy);
    saveDashboardPickStrategy(strategy);
  }, []);

  const handleSelectPoolPreference = useCallback((pool: PoolPreference) => {
    touchedRef.current.pool = true;
    setRecommendStrategy(pool);
    saveDashboardPoolPreference(pool);
  }, []);

  useEffect(() => {
    void hydrateUiStateFromConfig().then((hydrated) => {
      if (!hydrated) return;
      if (!initialStoredHostRef.current && !touchedRef.current.host && hydrated.dashboardHost) {
        setSelectedHost(hydrated.dashboardHost);
      }
      if (
        !initialStoredStrategyRef.current &&
        !touchedRef.current.strategy &&
        hydrated.dashboardPickStrategy
      ) {
        setPickStrategy(hydrated.dashboardPickStrategy);
      }
      if (
        !initialStoredPoolRef.current &&
        !touchedRef.current.pool &&
        hydrated.dashboardPoolPreference
      ) {
        setRecommendStrategy(hydrated.dashboardPoolPreference);
      }
    });
  }, []);

  const [isRefreshing, setIsRefreshing] = useState(false);
  const [justSwitched, setJustSwitched] = useState(false);

  // 2. 真实统计摘要（对接 get_stats_report）
  const [kpi, setKpi] = useState({
    totalTokens: 0,
    totalRequests: 0,
    savedUsd: 0,
    cacheHitRate: 0,
    avgTps: 0,
    avgTtftMs: 0,
  });

  const [, setRawAccounts] = useState<BackendAccount[]>([]);

  const [hostAccounts, setHostAccounts] = useState<Record<HostTarget, HostAccountItem>>(() => ({
    app: { id: '', email: '-', name: tUi(lang, 'dash_syncing'), tag: tUi(lang, 'dash_not_linked'), claude: 0, gemini: 0, active: false },
    ide: { id: '', email: '-', name: tUi(lang, 'dash_syncing'), tag: tUi(lang, 'dash_not_linked'), claude: 0, gemini: 0, active: false },
    plugin: { id: '', email: '-', name: tUi(lang, 'dash_syncing'), tag: tUi(lang, 'dash_not_linked'), claude: 0, gemini: 0, active: false },
    cli: { id: '', email: '-', name: tUi(lang, 'dash_syncing'), tag: tUi(lang, 'dash_not_linked'), claude: 0, gemini: 0, active: false },
  }));

  const [availableAccounts, setAvailableAccounts] = useState<CandidateAccount[]>([]);

  // 全局配额池与账号阵列计数
  const [poolStats, setPoolStats] = useState({
    claudePct: 0,
    geminiPct: 0,
    claudeReadyCount: 0,
    claudeLowCount: 0,
    geminiReadyCount: 0,
    geminiLowCount: 0,
    availableCount: 0,
    healthyCount: 0,
    totalCount: 0,
    warningCount: 0,
    bannedCount: 0,
    disabledCount: 0,
  });

  // 实时等化器脉冲数据（来自真实 recentTraffic）
  const [pulseBars, setPulseBars] = useState<Array<{ time: string; tokens: number; tps: number }>>([]);
  const [hoveredBarIdx, setHoveredBarIdx] = useState<number | null>(null);

  const formatResetCountdown = (raw: string): string => {
    if (!raw || !raw.includes('T')) return raw;
    const ts = Date.parse(raw);
    if (Number.isNaN(ts)) return raw;
    const diffMs = ts - Date.now();
    if (diffMs <= 0) return tUi(lang, 'dash_ready');
    const totalMins = Math.floor(diffMs / 60000);
    const days = Math.floor(totalMins / 1440);
    const hrs = Math.floor((totalMins % 1440) / 60);
    const mins = totalMins % 60;
    if (days > 0) return tUi(lang, 'dash_reset_days_hrs', { days, hrs });
    if (hrs > 0) return tUi(lang, 'dash_reset_hrs_mins', { hrs, mins });
    return tUi(lang, 'dash_reset_mins', { mins });
  };

  // 加载全量数据
  const loadDashboardData = useCallback(async () => {
    try {
      // 1. KPI 报告
      const report = await api.getStatsReport();
      if (report) {
        setKpi((prev) => ({
          totalTokens: report.totalTokens ?? prev.totalTokens,
          totalRequests: report.totalRequests ?? prev.totalRequests,
          savedUsd: report.totalSavedCostUsd ?? prev.savedUsd,
          cacheHitRate: report.globalCacheHitRate ?? prev.cacheHitRate,
          avgTps: report.avgSpeedTps ?? prev.avgTps,
          avgTtftMs: report.avgTtftMs ?? prev.avgTtftMs,
        }));
        const traffic = (report as any).recentTraffic;
        if (Array.isArray(traffic) && traffic.length > 0) {
          const mapped = [...traffic]
            .slice(0, 16)
            .reverse()
            .map((r: any) => {
              const tStr = String(r.timeStr || r.time || '-');
              return {
                time: tStr.includes(' ') ? tStr.split(' ')[1] : tStr,
                tokens: Number(r.promptTokens || 0) + Number(r.cachedTokens || 0) + Number(r.completionTokens || 0),
                tps: Number(r.tokensPerSecond || 0),
              };
            });
          setPulseBars(mapped);
        }
      }

      // 2. 宿主与账号绑定关联 (免活跃状态检测)
      const hosts = await api.getActiveHosts();

      // 3. 账号列表
      const accounts = await api.getAccounts();
      if (accounts && accounts.length > 0) {
        setRawAccounts(accounts);

        // 映射候选账号列表
        const candidates: CandidateAccount[] = accounts.map((acc) => {
          const claude = acc.quota?.claude_percentage ?? 80;
          const gemini = acc.quota?.gemini_percentage ?? 100;
          let reset = '4h 30m';
          let resetTs: number | null = null;
          const nowMs = Date.now();
          if (acc.quota?.quota_groups) {
            for (const g of acc.quota.quota_groups) {
              for (const b of g.buckets || []) {
                if (b.reset_time) {
                  const ts = Date.parse(b.reset_time);
                  if (!Number.isNaN(ts) && ts > nowMs && (resetTs === null || ts < resetTs)) {
                    resetTs = ts;
                    reset = b.reset_time;
                  }
                }
              }
            }
          }
          if (resetTs === null && acc.quota?.models) {
            const m = acc.quota.models.find((item) => item.reset_time);
            if (m?.reset_time) {
              reset = m.reset_time;
              const ts = Date.parse(m.reset_time);
              if (!Number.isNaN(ts) && ts > nowMs) resetTs = ts;
            }
          }
          const restricted = Boolean(acc.disabled || isAccountAbnormal(acc));
          const isWarn = restricted || claude < 30 || gemini < 30;
          return {
            id: acc.id,
            email: acc.email,
            name: acc.name || acc.email.split('@')[0],
            tier: resolveSubscriptionTier(acc),
            claude: Math.round(claude),
            gemini: Math.round(gemini),
            reset,
            resetTs,
            lastUsed: acc.last_used || 0,
            restricted,
            status: isWarn ? 'warning' : 'healthy',
          };
        });

        // 排序：可用账号优先，同态按双池综合配额降序
        candidates.sort((a, b) => {
          if (a.restricted !== b.restricted) return a.restricted ? 1 : -1;
          return (b.claude + b.gemini) - (a.claude + a.gemini);
        });
        setAvailableAccounts(candidates);

        const resolveHostAcc = (targetAccIdOrEmail?: string | null): BackendAccount | undefined => {
          if (!targetAccIdOrEmail) return undefined;
          const found = accounts.find(
            (a) => a.id === targetAccIdOrEmail || a.email.toLowerCase() === targetAccIdOrEmail.toLowerCase()
          );
          if (found) return found;
          return {
            id: targetAccIdOrEmail,
            email: targetAccIdOrEmail,
            token: { access_token: '', refresh_token: '' },
            tags: [],
            disabled: false,
            banned: false,
          };
        };

        const appAcc = resolveHostAcc(hosts?.app_account);
        const ideAcc = resolveHostAcc(hosts?.ide_account);
        const pluginAcc = resolveHostAcc(hosts?.plugin_account);
        const cliAcc = resolveHostAcc(hosts?.agy_account);

        const mapHostAcc = (acc?: BackendAccount): HostAccountItem => {
          if (!acc) {
            return {
              id: '',
              email: tUi(lang, 'mini_no_account'),
              name: tUi(lang, 'dash_not_linked'),
              tag: tUi(lang, 'dash_pending'),
              claude: 0,
              gemini: 0,
              active: false,
            };
          }
          const claude = Math.round(acc.quota?.claude_percentage ?? 80);
          const gemini = Math.round(acc.quota?.gemini_percentage ?? 100);
          // 真实订阅等级与标签：优先使用上游权威 subscription_tier (PRO / ULTRA / FREE)
          const tierBadge = resolveSubscriptionTier(acc);

          const tag = acc.tags.length > 0 ? acc.tags.slice(0, 2).join(' · ') : tierBadge;
          return {
            id: acc.id,
            email: acc.email,
            name: acc.name || acc.email.split('@')[0],
            tag,
            claude,
            gemini,
            active: true,
          };
        };

        setHostAccounts({
          app: mapHostAcc(appAcc),
          ide: mapHostAcc(ideAcc),
          plugin: mapHostAcc(pluginAcc),
          cli: mapHostAcc(cliAcc),
        });

        // 聚合宏观配额池（与账号管理页 / Mini 悬浮窗 / am-core 口径统一：忽略 proxy_disabled）
        let sumClaude = 0;
        let sumGemini = 0;
        let claudeReady = 0;
        let claudeLow = 0;
        let geminiReady = 0;
        let geminiLow = 0;
        let healthy = 0;
        let warning = 0;
        let banned = 0;
        let disabled = 0;

        accounts.forEach((acc) => {
          if (isAccountRestricted(acc)) {
            banned++;
          } else if (acc.disabled) {
            disabled++;
          } else {
            const c = acc.quota?.claude_percentage ?? 100;
            const g = acc.quota?.gemini_percentage ?? 100;
            if (c < 30) claudeLow++;
            else claudeReady++;
            if (g < 30) geminiLow++;
            else geminiReady++;
            if (c < 30 || g < 30) warning++;
            else healthy++;
            sumClaude += c;
            sumGemini += g;
          }
        });

        const availableCount = healthy + warning;
        const activeTotal = availableCount > 0 ? availableCount : 1;
        setPoolStats({
          claudePct: Number((sumClaude / activeTotal).toFixed(1)),
          geminiPct: Number((sumGemini / activeTotal).toFixed(1)),
          claudeReadyCount: claudeReady,
          claudeLowCount: claudeLow,
          geminiReadyCount: geminiReady,
          geminiLowCount: geminiLow,
          availableCount,
          healthyCount: healthy,
          totalCount: accounts.length,
          warningCount: warning,
          bannedCount: banned,
          disabledCount: disabled,
        });
      } else if (accounts) {
        setRawAccounts([]);
        setAvailableAccounts([]);
        setPoolStats({
          claudePct: 0,
          geminiPct: 0,
          claudeReadyCount: 0,
          claudeLowCount: 0,
          geminiReadyCount: 0,
          geminiLowCount: 0,
          availableCount: 0,
          healthyCount: 0,
          totalCount: 0,
          warningCount: 0,
          bannedCount: 0,
          disabledCount: 0,
        });
      }
    } catch (e) {
      console.warn('Dashboard load fallback:', e);
    }
  }, [lang]);

  useEffect(() => {
    loadDashboardData();
    let disposed = false;
    let unlisten: (() => void) | undefined;
    if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
      import('@tauri-apps/api/event').then(async ({ listen }) => {
        const off = await listen('tray://accounts-changed', () => {
          loadDashboardData();
        });
        if (disposed) {
          off();
          return;
        }
        unlisten = off;
      });
    }
    api.getAppConfig().then((cfg) => {
      if (disposed || !cfg) return;
      const rawS = (cfg.ui_state?.dashboard_pick_strategy || cfg.auto_switch?.strategy || 'max-quota').toLowerCase();
      if (!touchedRef.current.strategy) {
        if (rawS === 'expiring-soon' || rawS === 'expiring_soon') setPickStrategy('expiring-soon');
        else if (rawS === 'tier-first' || rawS === 'tier_first') setPickStrategy('tier-first');
        else if (rawS === 'least-used' || rawS === 'least_used' || rawS === 'balanced') setPickStrategy('least-used');
        else setPickStrategy('max-quota');
      }

      const rawP = (cfg.ui_state?.dashboard_pool_preference || cfg.auto_switch?.pool || 'balanced').toLowerCase();
      if (!touchedRef.current.pool) {
        if (rawP === 'claude' || rawP === 'gemini') setRecommendStrategy(rawP);
        else setRecommendStrategy('balanced');
      }
    }).catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [loadDashboardData]);

  const currentHostAcc = hostAccounts[selectedHost];

  // 统一的 4 策略 + 模型池推荐排序（与后端 am-core pick_best_account 完全对齐）
  const getRecommendedAccounts = useCallback((): CandidateAccount[] => {
    const healthy = availableAccounts.filter((a) => !a.restricted);
    const pool = healthy.length > 0 ? healthy : availableAccounts;
    const effQuota = (a: CandidateAccount) =>
      recommendStrategy === 'claude'
        ? a.claude
        : recommendStrategy === 'gemini'
        ? a.gemini
        : (a.claude + a.gemini) / 2;

    return [...pool].sort((a, b) => {
      const qa = effQuota(a);
      const qb = effQuota(b);
      if (pickStrategy === 'expiring-soon') {
        const aElig = qa >= 20 ? 1 : 0;
        const bElig = qb >= 20 ? 1 : 0;
        if (bElig !== aElig) return bElig - aElig;
        const ra = a.resetTs ?? Number.MAX_SAFE_INTEGER;
        const rb = b.resetTs ?? Number.MAX_SAFE_INTEGER;
        if (ra !== rb) return ra - rb;
        return qb - qa;
      }
      if (pickStrategy === 'tier-first') {
        const aAvail = qa > 0 ? 1 : 0;
        const bAvail = qb > 0 ? 1 : 0;
        if (bAvail !== aAvail) return bAvail - aAvail;
        const tr = tierRank(b.tier) - tierRank(a.tier);
        if (tr !== 0) return tr;
        if (qb !== qa) return qb - qa;
        return a.lastUsed - b.lastUsed;
      }
      if (pickStrategy === 'least-used') {
        const aAvail = qa > 0 ? 1 : 0;
        const bAvail = qb > 0 ? 1 : 0;
        if (bAvail !== aAvail) return bAvail - aAvail;
        if (a.lastUsed !== b.lastUsed) return a.lastUsed - b.lastUsed;
        return qb - qa;
      }
      // max-quota
      if (qb !== qa) return qb - qa;
      const tr = tierRank(b.tier) - tierRank(a.tier);
      if (tr !== 0) return tr;
      return a.lastUsed - b.lastUsed;
    });
  }, [availableAccounts, pickStrategy, recommendStrategy]);

  // 快捷切号
  const handleSwitchAccount = async (email: string) => {
    const acc = availableAccounts.find((a) => a.email === email);
    if (!acc) return;

    try {
      await api.switchAccount(acc.id || acc.email, selectedHost, false);
      setHostAccounts((prev) => ({
        ...prev,
        [selectedHost]: {
          ...prev[selectedHost],
          id: acc.id,
          email: acc.email,
          name: acc.name,
          claude: acc.claude,
          gemini: acc.gemini,
        },
      }));
      setJustSwitched(true);
      setTimeout(() => setJustSwitched(false), 2000);
      loadDashboardData();
    } catch {
      // 降级本地平滑响应
      setHostAccounts((prev) => ({
        ...prev,
        [selectedHost]: {
          ...prev[selectedHost],
          email: acc.email,
          name: acc.name,
          claude: acc.claude,
          gemini: acc.gemini,
        },
      }));
      setJustSwitched(true);
      setTimeout(() => setJustSwitched(false), 2000);
    }
  };

  // 智能选号（与当前策略和右下角推荐 Top 1 完全一致）
  const handleSmartSelect = async () => {
    const ranked = getRecommendedAccounts();
    const topPick = ranked.find((a) => a.email !== currentHostAcc.email) || ranked[0];
    if (topPick) {
      await handleSwitchAccount(topPick.email);
      return;
    }
    try {
      const best = await api.smartPickAccount(selectedHost, pickStrategy, recommendStrategy);
      if (best) {
        handleSwitchAccount(best.email);
      }
    } catch {
      // ignore
    }
  };

  // 刷新当前宿主绑定账号的配额
  const handleRefresh = async () => {
    setIsRefreshing(true);
    try {
      if (currentHostAcc.id) {
        await api.refreshAccountQuota(currentHostAcc.id);
      } else {
        await api.refreshAllQuotas();
      }
      await loadDashboardData();
    } catch {
      // 降级
    } finally {
      setTimeout(() => setIsRefreshing(false), 800);
    }
  };

  // 双同心环参数 (r1=46 Claude外环, r2=34 Gemini内环)
  const claudePoolPct = poolStats.claudePct;
  const geminiPoolPct = poolStats.geminiPct;


  return (
    <div className="w-full max-w-[1400px] mx-auto px-4 py-3 flex flex-col gap-3 select-none animate-in fade-in-30 duration-200">
      {/* ============================================================== */}
      {/* 【第一层：4 列首页专属运维 KPI（账号资产 / 双池余量 / 实时遥测）】 */}
      {/* ============================================================== */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-2.5">
        {/* 1. 账号资产总览 */}
        <div className="p-3 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between">
          <div className="flex items-center justify-between text-[10px] text-slate-500 dark:text-zinc-400 font-medium">
            <span>{tUi(lang, 'dash_account_assets')}</span>
            <Users className="w-3.5 h-3.5 text-slate-400/80 dark:text-zinc-500" />
          </div>
          <div className="my-1 flex items-baseline gap-1.5">
            <span className="text-xl font-bold font-mono tabular-nums tracking-tight text-slate-900 dark:text-zinc-50">
              {poolStats.totalCount}
              <span className="text-xs text-slate-400 dark:text-zinc-400 font-normal"> {tUi(lang, 'dash_total_unit')}</span>
            </span>
            <span className="text-[9.5px] font-mono text-emerald-600/85 dark:text-emerald-400/85 font-medium">
              {poolStats.availableCount} {tUi(lang, 'dash_avail_short')}
            </span>
          </div>
          <span className="text-[10px] text-slate-500 dark:text-zinc-400 tabular-nums truncate">
            {`${tUi(lang, 'dash_status_ok')} ${poolStats.healthyCount} · ${tUi(lang, 'dash_status_warn')} ${poolStats.warningCount} · ${tUi(lang, 'dash_status_ban')} ${poolStats.bannedCount} · ${tUi(lang, 'dash_status_off')} ${poolStats.disabledCount}`}
          </span>
        </div>

        <div className="p-3 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between">
          <div className="flex items-center justify-between text-[10px] text-slate-500 dark:text-zinc-400 font-medium">
            <span className="flex items-center gap-1.5">
              <span className="w-1.5 h-1.5 rounded-full bg-violet-400/90" />
              {tUi(lang, 'dash_claude_pool')}
            </span>
            <Sparkles className="w-3.5 h-3.5 text-slate-400/80 dark:text-zinc-500" />
          </div>
          <div className="my-1 flex items-baseline gap-1.5">
            <span className="text-xl font-bold font-mono tabular-nums tracking-tight text-slate-900 dark:text-zinc-50">
              {claudePoolPct}%
            </span>
            <span className="text-[9.5px] font-mono text-slate-500 dark:text-zinc-400 font-medium">
              {poolStats.claudeReadyCount} {tUi(lang, 'dash_ready_pool')}
            </span>
          </div>
          <span className="text-[10px] text-slate-500 dark:text-zinc-400 tabular-nums">
            {poolStats.claudeLowCount > 0
              ? `${poolStats.claudeLowCount} ${tUi(lang, 'dash_warning_pool')}`
              : tUi(lang, 'dash_full_quota')}
          </span>
        </div>

        <div className="p-3 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between">
          <div className="flex items-center justify-between text-[10px] text-slate-500 dark:text-zinc-400 font-medium">
            <span className="flex items-center gap-1.5">
              <span className="w-1.5 h-1.5 rounded-full bg-sky-400/90" />
              {tUi(lang, 'dash_gemini_pool')}
            </span>
            <ShieldCheck className="w-3.5 h-3.5 text-slate-400/80 dark:text-zinc-500" />
          </div>
          <div className="my-1 flex items-baseline gap-1.5">
            <span className="text-xl font-bold font-mono tabular-nums tracking-tight text-slate-900 dark:text-zinc-50">
              {geminiPoolPct}%
            </span>
            <span className="text-[9.5px] font-mono text-slate-500 dark:text-zinc-400 font-medium">
              {poolStats.geminiReadyCount} {tUi(lang, 'dash_ready_pool')}
            </span>
          </div>
          <span className="text-[10px] text-slate-500 dark:text-zinc-400 tabular-nums">
            {poolStats.geminiLowCount > 0
              ? `${poolStats.geminiLowCount} ${tUi(lang, 'dash_warning_pool')}`
              : tUi(lang, 'dash_full_quota')}
          </span>
        </div>

        <div className="p-3 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between">
          <div className="flex items-center justify-between text-[10px] text-slate-500 dark:text-zinc-400 font-medium">
            <span>{tUi(lang, 'dash_speed')}</span>
            <Zap className="w-3.5 h-3.5 text-slate-400/80 dark:text-zinc-500" />
          </div>
          <div className="my-1 flex items-baseline gap-1.5">
            <span className="text-xl font-bold font-mono tabular-nums tracking-tight text-slate-900 dark:text-zinc-50">
              {(kpi.totalTokens / 1000000).toFixed(1)}M
            </span>
            <span className="text-[9.5px] font-mono text-slate-500 dark:text-zinc-400 font-medium">
              {kpi.avgTps.toFixed(0)} t/s
            </span>
          </div>
          <span className="text-[10px] text-slate-500 dark:text-zinc-400 tabular-nums">
            {`${tUi(lang, 'stats_cache')} ${kpi.cacheHitRate.toFixed(1)}% · ${kpi.totalRequests} ${tUi(lang, 'stats_unit_reqs')}`}
          </span>
        </div>
      </div>

      {/* ============================================================== */}
      {/* 【第二层：7 + 5 非对称核心指挥区】                               */}
      {/* 左 7 列：深空黑多端切号中心 + Electrys 粒子能量电池               */}
      {/* 右 5 列：08 FinPoint 风格 180° 半圆弧形分段仪表盘                */}
      {/* ============================================================== */}
      <div className="grid grid-cols-1 md:grid-cols-12 gap-3.5 items-stretch">
        {/* 左卡 (7 cols)：缎面石墨/深邃 OLED 黑曜石快捷多端切号中心 Hero 卡 */}
        <section className="md:col-span-7 bg-gradient-to-br from-[#232734] via-[#1a1d27] to-[#141720] dark:from-[#07090e] dark:via-[#05070b] dark:to-[#030407] border border-slate-900/15 dark:border-white/[0.14] rounded-2xl p-4 flex flex-col justify-between shadow-[inset_0_1px_0_rgba(255,255,255,0.10),0_14px_34px_-14px_rgba(15,23,42,0.28)] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.12),0_18px_40px_-14px_rgba(0,0,0,0.85)] relative overflow-hidden text-white">
          <div
            style={{
              background:
                'radial-gradient(circle at 100% 0%, rgba(16,185,129,0.13) 0%, rgba(16,185,129,0) 58%)',
            }}
            className="pointer-events-none absolute inset-0"
          />

          <div>
            {/* 头部 */}
            <div className="flex items-center justify-between mb-3">
              <div className="flex items-center gap-2">
                <div className="w-7 h-7 rounded-lg bg-emerald-500/15 border border-emerald-400/25 text-emerald-300 flex items-center justify-center">
                  <Zap className="w-3.5 h-3.5" />
                </div>
                <div>
                  <h3 className="text-xs font-extrabold tracking-tight text-white">
                    {tUi(lang, 'dash_switch_center')}
                  </h3>
                  <p className="text-[9.5px] text-zinc-400">
                    {tUi(lang, 'dash_zero_proxy_desc')}
                  </p>
                </div>
              </div>

              <div className="flex items-center gap-1.5">
                <button
                  onClick={handleSmartSelect}
                  className="px-2.5 py-1 text-[10px] font-semibold rounded-lg bg-emerald-500/15 hover:bg-emerald-500/25 border border-emerald-400/30 text-emerald-300 transition-colors flex items-center gap-1 cursor-pointer"
                >
                  <Sparkles className="w-3 h-3" />
                  <span>{tUi(lang, 'dash_smart_pick')}</span>
                </button>
                <button
                  onClick={handleRefresh}
                  disabled={isRefreshing}
                  className="p-1.5 rounded-lg text-zinc-400 hover:text-white hover:bg-white/10 transition-colors cursor-pointer"
                  title={tUi(lang, 'refresh_quotas')}
                >
                  <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin text-emerald-400' : ''}`} />
                </button>
              </div>
            </div>

            {/* 4 宿主目标选择器 (纯粹宿主形态切换，免运行状态检测干扰) */}
            <div className="grid grid-cols-4 gap-1.5 p-1 bg-white/[0.05] border border-white/[0.06] rounded-xl mb-3">
              {[
                { id: 'app' as HostTarget, label: 'Antigravity', sub: tUi(lang, 'settings_desktop_desc'), icon: AppWindow },
                { id: 'ide' as HostTarget, label: 'Antigravity IDE', sub: 'IDE', icon: Code2 },
                { id: 'plugin' as HostTarget, label: 'Antigravity ACP', sub: 'Plugin', icon: Puzzle },
                { id: 'cli' as HostTarget, label: 'Antigravity CLI', sub: 'CLI', icon: Terminal },
              ].map((host) => {
                const Icon = host.icon;
                const isSelected = selectedHost === host.id;
                return (
                  <button
                    key={host.id}
                    onClick={() => handleSelectHost(host.id)}
                    className={`py-1.5 px-2 rounded-lg flex flex-col items-center justify-center gap-0.5 transition-all cursor-pointer ${
                      isSelected
                        ? 'bg-white text-gray-950 shadow-xs font-bold'
                        : 'text-zinc-400 hover:text-white hover:bg-white/[0.04]'
                    }`}
                  >
                    <div className="flex items-center gap-1.5">
                      <Icon className="w-3.5 h-3.5 shrink-0" />
                      <span className="text-[11px] font-semibold truncate">{host.label}</span>
                    </div>
                    <span className={`text-[8.5px] truncate ${isSelected ? 'text-gray-500 font-medium' : 'text-zinc-500'}`}>
                      {host.sub}
                    </span>
                  </button>
                );
              })}
            </div>

            {/* 当前绑定账号 + Electrys 发光粒子能量电池槽 */}
            <div className="p-3 rounded-xl border border-white/10 bg-white/[0.04] flex items-center justify-between gap-3">
              <div className="min-w-0 flex-1 flex flex-col gap-2">
                <div className="flex items-center gap-1.5">
                  <span className="text-xs font-bold text-white truncate font-mono">
                    {currentHostAcc.email}
                  </span>
                  <span
                    className={`px-1.5 py-0.5 text-[8.5px] font-semibold rounded border shrink-0 ${
                      currentHostAcc.tag === 'PRO' || currentHostAcc.tag.includes('PRO')
                        ? 'bg-blue-500/20 text-blue-300 border-blue-400/25'
                        : currentHostAcc.tag === 'ULTRA' || currentHostAcc.tag.includes('ULTRA')
                        ? 'bg-purple-500/20 text-purple-300 border-purple-400/25'
                        : currentHostAcc.tag === 'ENTERPRISE'
                        ? 'bg-amber-500/20 text-amber-300 border-amber-400/25'
                        : 'bg-zinc-700/30 text-zinc-300 border-zinc-600/30'
                    }`}
                  >
                    {currentHostAcc.tag}
                  </span>
                </div>

                {/* Electrys 发光粒子能量电池槽 (Claude 紫绿粒子 + Gemini 青蓝粒子) */}
                <div className="grid grid-cols-2 gap-3 text-[9px] font-mono">
                  {[
                    { name: 'Claude', pct: currentHostAcc.claude, dotColor: '#a855f7', glow: 'rgba(168,85,247,0.65)', textCls: 'text-purple-300' },
                    { name: 'Gemini', pct: currentHostAcc.gemini, dotColor: '#34d399', glow: 'rgba(52,211,153,0.65)', textCls: 'text-emerald-300' },
                  ].map((b) => {
                    const activeCols = Math.round((b.pct / 100) * 12);
                    return (
                      <div key={b.name} className="flex flex-col gap-1">
                        <div className="flex items-center justify-between">
                          <span className={`font-semibold ${b.textCls}`}>{b.name}</span>
                          <span className="font-bold text-white">{b.pct}%</span>
                        </div>
                        <div className="h-5 rounded-md bg-black/50 border border-white/10 px-1.5 py-1 grid grid-cols-12 gap-0.5 items-center">
                          {Array.from({ length: 12 }, (_, colIdx) => {
                            const isLit = colIdx < activeCols;
                            return (
                              <div key={colIdx} className="flex flex-col gap-0.5 items-center justify-center">
                                <span
                                  className="w-1 h-1 rounded-full transition-all"
                                  style={{
                                    backgroundColor: isLit ? b.dotColor : 'rgba(255,255,255,0.08)',
                                    boxShadow: isLit ? `0 0 4px ${b.glow}` : 'none',
                                  }}
                                />
                                <span
                                  className="w-1 h-1 rounded-full transition-all"
                                  style={{
                                    backgroundColor: isLit ? b.dotColor : 'rgba(255,255,255,0.08)',
                                    opacity: isLit ? 0.85 : 1,
                                  }}
                                />
                              </div>
                            );
                          })}
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>

              {/* 快捷切换下拉 */}
              <DropdownMenu.Root>
                <DropdownMenu.Trigger asChild>
                  <button className="px-3 py-2 rounded-xl bg-white text-gray-950 hover:bg-zinc-200 text-[11px] font-bold inline-flex items-center gap-1.5 shadow-sm transition-all shrink-0 cursor-pointer">
                    <span>{justSwitched ? tUi(lang, 'dash_switched_btn') : tUi(lang, 'dash_switch_btn')}</span>
                    <ChevronDown className="w-3.5 h-3.5 opacity-70" />
                  </button>
                </DropdownMenu.Trigger>
                <DropdownMenu.Portal>
                  <DropdownMenu.Content
                    align="end"
                    sideOffset={6}
                    className="mini-scroll w-64 p-1.5 bg-white dark:bg-[#1a1b20] border border-gray-200 dark:border-zinc-800 rounded-xl shadow-xl z-50 animate-in fade-in-0 zoom-in-95 duration-100 max-h-72 overflow-y-auto"
                  >
                    <div className="px-2.5 py-1 text-[10px] font-semibold text-gray-400">
                      {tUi(lang, 'dash_avail_accounts')}
                    </div>
                    {(availableAccounts.some((a) => !a.restricted)
                      ? availableAccounts.filter((a) => !a.restricted)
                      : availableAccounts
                    ).map((acc) => (
                      <DropdownMenu.Item
                        key={acc.email}
                        onClick={() => handleSwitchAccount(acc.email)}
                        className={`p-2 rounded-lg text-[11px] flex items-center justify-between cursor-pointer outline-none transition-colors ${
                          acc.email === currentHostAcc.email
                            ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 font-semibold'
                            : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800/80'
                        }`}
                      >
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-1.5 min-w-0">
                            <p className="truncate font-medium">{acc.email}</p>
                            <span
                              className={`px-1 py-[1px] text-[8.5px] font-bold rounded border shrink-0 leading-tight ${
                                acc.tier === 'ULTRA'
                                  ? 'bg-purple-500/15 text-purple-600 dark:text-purple-300 border-purple-400/30'
                                  : acc.tier === 'PRO'
                                  ? 'bg-blue-500/15 text-blue-600 dark:text-blue-300 border-blue-400/30'
                                  : acc.tier === 'ENTERPRISE'
                                  ? 'bg-amber-500/15 text-amber-600 dark:text-amber-300 border-amber-400/30'
                                  : 'bg-gray-500/10 text-gray-500 dark:text-zinc-400 border-gray-400/25'
                              }`}
                            >
                              {acc.tier}
                            </span>
                          </div>
                          <div className="flex items-center gap-2 text-[9.5px] opacity-75 mt-0.5 font-mono">
                            <span className="text-purple-500 font-semibold">C:{acc.claude}%</span>
                            <span className="text-blue-500 font-semibold">G:{acc.gemini}%</span>
                            <span>{formatResetCountdown(acc.reset)}</span>
                          </div>
                        </div>
                        {acc.email === currentHostAcc.email && <CheckCircle2 className="w-3.5 h-3.5 text-blue-600 shrink-0 ml-2" />}
                      </DropdownMenu.Item>
                    ))}
                  </DropdownMenu.Content>
                </DropdownMenu.Portal>
              </DropdownMenu.Root>
            </div>
          </div>

          {/* 底部保障条 */}
          <div className="mt-2.5 pt-2 border-t border-white/[0.08] flex items-center justify-between text-[9.5px] text-zinc-500">
            <span className="inline-flex items-center gap-1">
              <ShieldCheck className="w-3 h-3 text-emerald-400" />
              {tUi(lang, 'dash_device_isolated_desc')}
            </span>
            <span className="font-mono text-zinc-400">{tUi(lang, 'dash_sqlite_direct')}</span>
          </div>
        </section>

        {/* 右卡 (5 cols)：08 FinPoint 风格 180° 半圆弧形分段仪表盘 */}
        <section className="md:col-span-5 bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] rounded-2xl p-4 flex flex-col justify-between shadow-xs relative overflow-hidden">
          <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
            <span className="text-xs font-bold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
              <Gauge className="w-3.5 h-3.5 text-purple-500" />
              <span>{tUi(lang, 'dash_global_pools')}</span>
            </span>
            <span className="px-2 py-0.5 rounded-full bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 text-[9.5px] font-semibold font-mono">
              {poolStats.availableCount} / {poolStats.totalCount} {tUi(lang, 'dash_avail_short')}
            </span>
          </div>

          {/* 180° 半圆弧形分段仪表盘 */}
          {(() => {
            const total = Math.max(poolStats.totalCount, 1);
            const r = 56;
            const halfC = Math.PI * r;
            const okLen = (poolStats.healthyCount / total) * halfC;
            const warnLen = (poolStats.warningCount / total) * halfC;
            const banLen = (poolStats.bannedCount / total) * halfC;
            return (
              <div className="flex flex-col items-center justify-center my-1">
                <div className="relative w-[176px] h-[94px] flex items-end justify-center">
                  <svg viewBox="0 0 140 78" className="w-full h-full overflow-visible">
                    <path
                      d="M 14 70 A 56 56 0 0 1 126 70"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="10"
                      strokeLinecap="round"
                      className="text-gray-100 dark:text-white/[0.06]"
                    />
                    {okLen > 0 && (
                      <path
                        d="M 14 70 A 56 56 0 0 1 126 70"
                        fill="none"
                        stroke="#10b981"
                        strokeWidth="10"
                        strokeLinecap="round"
                        strokeDasharray={`${okLen} ${halfC}`}
                      />
                    )}
                    {warnLen > 0 && (
                      <path
                        d="M 14 70 A 56 56 0 0 1 126 70"
                        fill="none"
                        stroke="#f59e0b"
                        strokeWidth="10"
                        strokeDasharray={`${warnLen} ${halfC}`}
                        strokeDashoffset={-okLen}
                      />
                    )}
                    {banLen > 0 && (
                      <path
                        d="M 14 70 A 56 56 0 0 1 126 70"
                        fill="none"
                        stroke="#f43f5e"
                        strokeWidth="10"
                        strokeDasharray={`${banLen} ${halfC}`}
                        strokeDashoffset={-(okLen + warnLen)}
                      />
                    )}
                  </svg>
                  <div className="absolute bottom-1 flex flex-col items-center">
                    <span className="text-2xl font-black font-mono text-gray-900 dark:text-zinc-100 leading-none">
                      {poolStats.availableCount}
                      <span className="text-xs text-gray-400 font-semibold">/{poolStats.totalCount}</span>
                    </span>
                    <span className="text-[9px] text-gray-400 font-medium mt-1">
                      {tUi(lang, 'dash_avail_accounts')}
                    </span>
                  </div>
                </div>
                <div className="flex items-center gap-2 mt-1.5 text-[9.5px] font-mono">
                  <span className="px-2 py-0.5 rounded-full bg-purple-500/10 text-purple-600 dark:text-purple-400 font-semibold">
                    Claude {claudePoolPct}%
                  </span>
                  <span className="px-2 py-0.5 rounded-full bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 font-semibold">
                    Gemini {geminiPoolPct}%
                  </span>
                </div>
              </div>
            );
          })()}

          {/* 4 态健康微卡 */}
          <div className="grid grid-cols-4 gap-1.5 text-center pt-2 border-t border-gray-100 dark:border-white/5">
            <div className="py-1 rounded-lg bg-emerald-500/[0.08]">
              <span className="text-[8.5px] text-gray-500 dark:text-zinc-400 block">{tUi(lang, 'dash_status_ok')}</span>
              <span className="text-xs font-bold font-mono text-emerald-600 dark:text-emerald-400">{poolStats.healthyCount}</span>
            </div>
            <div className="py-1 rounded-lg bg-amber-500/[0.08]">
              <span className="text-[8.5px] text-gray-500 dark:text-zinc-400 block">{tUi(lang, 'dash_status_warn')}</span>
              <span className="text-xs font-bold font-mono text-amber-500">{poolStats.warningCount}</span>
            </div>
            <div className="py-1 rounded-lg bg-rose-500/[0.08]">
              <span className="text-[8.5px] text-gray-500 dark:text-zinc-400 block">{tUi(lang, 'dash_status_ban')}</span>
              <span className="text-xs font-bold font-mono text-rose-500">{poolStats.bannedCount}</span>
            </div>
            <div className="py-1 rounded-lg bg-gray-100/80 dark:bg-white/[0.04]">
              <span className="text-[8.5px] text-gray-500 dark:text-zinc-400 block">{tUi(lang, 'dash_status_off')}</span>
              <span className="text-xs font-bold font-mono text-gray-400">{poolStats.disabledCount}</span>
            </div>
          </div>
        </section>
      </div>

      {/* ============================================================== */}
      {/* 【第三层：6 + 6 底部态势感知区】                                 */}
      {/* 左 6 列：16 柱真实等化器脉冲频谱 | 右 6 列：倒计时格式化与切入    */}
      {/* ============================================================== */}
      <div className="grid grid-cols-1 md:grid-cols-12 gap-3.5 items-stretch">
        {/* 左卡 (6 cols)：16 根真实等化器脉冲频谱柱 (支持悬停看单笔详情) */}
        <section className="md:col-span-6 bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] rounded-2xl p-3.5 shadow-xs flex flex-col justify-between gap-2">
          <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
            <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
              <TrendingUp className="w-3.5 h-3.5 text-indigo-500" />
              <span>{tUi(lang, 'dash_equalizer')}</span>
            </span>
            <span className="text-[9.5px] font-mono text-indigo-600 dark:text-indigo-400 font-semibold">
              {hoveredBarIdx !== null && pulseBars[hoveredBarIdx]
                ? `${pulseBars[hoveredBarIdx].time} · ${(pulseBars[hoveredBarIdx].tokens / 1000).toFixed(1)}K · ${pulseBars[hoveredBarIdx].tps.toFixed(0)} t/s`
                : `${tUi(lang, 'dash_recent_reqs')} ${pulseBars.length || 16} ${tUi(lang, 'stats_unit_reqs')}`}
            </span>
          </div>

          <div
            className="h-24 w-full flex items-end gap-1.5 px-1 pt-2"
            onMouseLeave={() => setHoveredBarIdx(null)}
          >
            {(() => {
              const bars =
                pulseBars.length > 0
                  ? pulseBars
                  : Array.from({ length: 16 }, (_, i) => ({ time: `${String(i).padStart(2, '0')}:00`, tokens: 0, tps: 0 }));
              const nonZero = bars.filter((b) => b.tokens > 0).map((b) => b.tokens);
              const maxTok = Math.max(...nonZero, 1);
              const minTok = nonZero.length > 1 ? Math.min(...nonZero) : 0;
              const span = Math.max(maxTok - minTok, maxTok * 0.25, 1);
              return bars.map((bar, idx) => {
                const rel = bar.tokens > 0 ? (bar.tokens - minTok * 0.85) / span : 0;
                const hPct = bar.tokens > 0 ? Math.min(100, Math.max(20, rel * 88 + 14)) : 10;
                const isHov = hoveredBarIdx === idx;
                const colorCls =
                  bar.tokens === 0
                    ? 'bg-gray-100 dark:bg-white/[0.05]'
                    : isHov
                    ? 'bg-gradient-to-t from-rose-500 to-amber-400 scale-x-110 shadow-sm'
                    : bar.tps >= 120
                    ? 'bg-gradient-to-t from-emerald-500 to-teal-300'
                    : bar.tps >= 60
                    ? 'bg-gradient-to-t from-sky-500 to-indigo-400'
                    : 'bg-gradient-to-t from-amber-500 to-rose-400';
                return (
                  <div
                    key={idx}
                    onMouseEnter={() => setHoveredBarIdx(idx)}
                    title={
                      bar.tokens > 0
                        ? `${bar.time} · ${(bar.tokens / 1000).toFixed(1)}K tokens · ${bar.tps.toFixed(0)} tok/s`
                        : bar.time
                    }
                    className="flex-1 h-full flex flex-col justify-end items-center cursor-pointer"
                  >
                    <div
                      style={{ height: `${hPct}%` }}
                      className={`w-full rounded-full transition-all duration-200 ${colorCls}`}
                    />
                  </div>
                );
              });
            })()}
          </div>

          <div className="flex items-center justify-between text-[8.5px] font-mono text-gray-400 pt-1 border-t border-gray-100 dark:border-white/5">
            <span>{pulseBars[0]?.time || '00:00'}</span>
            <div className="flex items-center gap-2">
              <span className="flex items-center gap-1" title="≥ 120 tok/s"><span className="w-1.5 h-1.5 rounded-full bg-emerald-400" />{tUi(lang, 'equalizer_fast')}</span>
              <span className="flex items-center gap-1" title="60–119 tok/s"><span className="w-1.5 h-1.5 rounded-full bg-sky-400" />{tUi(lang, 'equalizer_med')}</span>
              <span className="flex items-center gap-1" title="< 60 tok/s"><span className="w-1.5 h-1.5 rounded-full bg-amber-400" />{tUi(lang, 'equalizer_slow')}</span>
            </div>
            <span>{pulseBars[pulseBars.length - 1]?.time || tUi(lang, 'dash_now')}</span>
          </div>
        </section>

        {/* 右卡 (6 cols)：备用账号配额池（客观呈现各账号当前可用配额） */}
        <section className="md:col-span-6 bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] rounded-2xl p-3.5 shadow-xs flex flex-col justify-between gap-2">
          <div className="flex flex-wrap items-center justify-between gap-1.5 pb-1.5 border-b border-gray-100 dark:border-white/5">
            <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5 shrink-0">
              <Clock className="w-3.5 h-3.5 text-emerald-500" />
              <span>{tUi(lang, 'dash_reserve_accounts')}</span>
            </span>
            <div className="flex flex-wrap items-center gap-1.5">
              <div className="inline-flex p-0.5 rounded-lg bg-gray-100/90 dark:bg-white/[0.06] border border-gray-200/50 dark:border-white/[0.04]">
                {([
                  { id: 'max-quota', key: 'strategy_max_quota_short' },
                  { id: 'expiring-soon', key: 'strategy_expiring_soon_short' },
                  { id: 'tier-first', key: 'strategy_tier_first_short' },
                  { id: 'least-used', key: 'strategy_least_used_short' },
                ] as const).map((s) => (
                  <button
                    key={s.id}
                    onClick={() => handleSelectPickStrategy(s.id)}
                    className={`px-1.5 py-0.5 rounded-md text-[9px] font-semibold transition-all cursor-pointer ${
                      pickStrategy === s.id
                        ? 'bg-white dark:bg-zinc-800 text-blue-600 dark:text-blue-400 shadow-2xs dark:ring-1 dark:ring-white/[0.08]'
                        : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                    }`}
                  >
                    {tUi(lang, s.key)}
                  </button>
                ))}
              </div>
              <div className="inline-flex p-0.5 rounded-lg bg-gray-100/90 dark:bg-white/[0.06] border border-gray-200/50 dark:border-white/[0.04]">
                {([
                  { id: 'balanced', label: tUi(lang, 'pool_balanced') },
                  { id: 'claude', label: 'Claude' },
                  { id: 'gemini', label: 'Gemini' },
                ] as const).map((s) => (
                  <button
                    key={s.id}
                    onClick={() => handleSelectPoolPreference(s.id)}
                    className={`px-1.5 py-0.5 rounded-md text-[9px] font-semibold transition-all cursor-pointer ${
                      recommendStrategy === s.id
                        ? 'bg-white dark:bg-zinc-800 text-gray-900 dark:text-zinc-100 shadow-2xs dark:ring-1 dark:ring-white/[0.08]'
                        : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                    }`}
                  >
                    {s.label}
                  </button>
                ))}
              </div>
            </div>
          </div>

          <div className="flex flex-col gap-1.5 flex-1 justify-center">
            {(() => {
              const sorted = getRecommendedAccounts();
              return sorted.slice(0, 3).map((acc) => {
                return (
                  <div
                    key={acc.email}
                    className="px-2.5 py-2 rounded-xl border flex items-center justify-between gap-2 transition-all bg-gray-50/90 dark:bg-white/[0.03] border-gray-100/80 dark:border-white/[0.05]"
                  >
                    <div className="flex items-center gap-2 min-w-0">
                      {acc.status === 'warning' ? (
                        <AlertTriangle className="w-3.5 h-3.5 text-amber-500 shrink-0" />
                      ) : (
                        <CheckCircle2 className="w-3.5 h-3.5 shrink-0 text-emerald-500" />
                      )}
                      <div className="min-w-0">
                        <div className="flex items-center gap-1.5 min-w-0">
                          <span className="text-[10.5px] font-bold text-gray-900 dark:text-zinc-100 truncate">
                            {acc.email}
                          </span>
                          <span
                            className={`px-1 py-[1px] text-[8.5px] font-bold rounded border shrink-0 leading-tight ${
                              acc.tier === 'ULTRA'
                                ? 'bg-purple-500/15 text-purple-600 dark:text-purple-300 border-purple-400/30'
                                : acc.tier === 'PRO'
                                ? 'bg-blue-500/15 text-blue-600 dark:text-blue-300 border-blue-400/30'
                                : acc.tier === 'ENTERPRISE'
                                ? 'bg-amber-500/15 text-amber-600 dark:text-amber-300 border-amber-400/30'
                                : 'bg-gray-500/10 text-gray-500 dark:text-zinc-400 border-gray-400/25'
                            }`}
                          >
                            {acc.tier}
                          </span>
                        </div>
                        <div className="text-[9px] font-mono text-gray-400 flex items-center gap-1.5 mt-0.5">
                          <span className="text-purple-600 dark:text-purple-400 font-semibold">C:{acc.claude}%</span>
                          <span className="text-sky-600 dark:text-sky-400 font-semibold">G:{acc.gemini}%</span>
                          <span className="px-1.5 py-[1px] rounded bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 font-semibold">
                            {formatResetCountdown(acc.reset)}
                          </span>
                        </div>
                      </div>
                    </div>

                    <button
                      onClick={() => handleSwitchAccount(acc.email)}
                      className="px-2.5 py-1 rounded-lg text-[9.5px] font-bold transition-colors shrink-0 cursor-pointer bg-gray-100 hover:bg-gray-900 hover:text-white dark:bg-white/[0.06] dark:hover:bg-white dark:hover:text-gray-950 text-gray-700 dark:text-zinc-300"
                    >
                      {tUi(lang, 'dash_use_account')}
                    </button>
                  </div>
                );
              });
            })()}
          </div>
        </section>
      </div>
    </div>
  );
};
