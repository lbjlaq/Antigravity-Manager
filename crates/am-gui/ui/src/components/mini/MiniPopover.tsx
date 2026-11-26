import React, { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import {
  AppWindow,
  Code2,
  Puzzle,
  Terminal,
  Sparkles,
  RefreshCw,
  Pin,
  Maximize2,
  Power,
  Check,
  Zap,
  Sun,
  Moon,
  ArrowLeftRight,
  BarChart3,
  ChevronRight,
  ArrowUpRight,
  Activity,
} from 'lucide-react';
import { api, HostTarget, BackendAccount, ActiveHostStatus, isTauri } from '../../api';
import { tUi } from '../../i18n/ui';
import {
  extractAccountPoolQuota,
  isAccountQuotaStale,
  ParsedPoolQuota,
  PoolWindowQuota,
} from '../../utils/quota';
import {
  QuotaMatrixBar,
  QuotaWindowBadge,
  quotaPctTextClass,
} from '../common/QuotaMatrixBar';
import {
  AccountsSortMode,
  DashboardPickStrategy,
  DashboardPoolPreference,
  readStoredAccountsSortMode,
  readStoredDashboardPickStrategy,
  readStoredDashboardPoolPreference,
} from '../../utils/uiState';

function tierSortWeight(tier: 'ULTRA' | 'PRO' | 'ENTERPRISE' | 'FREE'): number {
  if (tier === 'ENTERPRISE') return 4;
  if (tier === 'ULTRA') return 3;
  if (tier === 'PRO') return 2;
  return 1;
}

function normalizeSortMode(raw?: string | null): AccountsSortMode {
  if (raw === 'tier_desc' || raw === 'name_asc' || raw === 'custom' || raw === 'health_default') {
    return raw;
  }
  return 'health_default';
}

function normalizePickStrategy(raw?: string | null): DashboardPickStrategy {
  const s = (raw || '').toLowerCase();
  if (s === 'expiring-soon' || s === 'expiring_soon') return 'expiring-soon';
  if (s === 'tier-first' || s === 'tier_first') return 'tier-first';
  if (s === 'least-used' || s === 'least_used' || s === 'balanced') return 'least-used';
  return 'max-quota';
}

function normalizePoolPreference(raw?: string | null): DashboardPoolPreference {
  const p = (raw || '').toLowerCase();
  if (p === 'gemini' || p === 'claude') return p;
  return 'balanced';
}

interface MiniPopoverProps {
  lang: string;
  theme: 'light' | 'dark';
  onToggleTheme: () => void;
}

type MiniViewMode = 'switch' | 'stats';

interface TodayHourlyBucket {
  hour: number;
  tokens: number;
  requests: number;
}

interface TodayModelItem {
  name: string;
  tokens: number;
  sharePct: number;
  avgTps: number;
  requests: number;
}

interface TodayStatsState {
  loaded: boolean;
  totalTokens: number;
  promptTokens: number;
  cachedTokens: number;
  outputTokens: number;
  totalRequests: number;
  cacheHitRate: number;
  avgTps: number;
  peakTps: number;
  avgTtftMs: number;
  avgDurationMs: number;
  savedUsd: number;
  runningCount: number;
  hourly: TodayHourlyBucket[];
  topModels: TodayModelItem[];
}

function createEmptyHourly(): TodayHourlyBucket[] {
  return Array.from({ length: 24 }, (_, hour) => ({ hour, tokens: 0, requests: 0 }));
}

const INITIAL_TODAY_STATS: TodayStatsState = {
  loaded: false,
  totalTokens: 0,
  promptTokens: 0,
  cachedTokens: 0,
  outputTokens: 0,
  totalRequests: 0,
  cacheHitRate: 0,
  avgTps: 0,
  peakTps: 0,
  avgTtftMs: 0,
  avgDurationMs: 0,
  savedUsd: 0,
  runningCount: 0,
  hourly: createEmptyHourly(),
  topModels: [],
};

function fmtCompactTokens(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '0';
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(2)}B`;
  if (n >= 10_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 100_000) return `${(n / 1_000).toFixed(0)}K`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return `${Math.round(n)}`;
}

function fmtTtft(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return '--';
  if (ms < 1000) return `${Math.round(ms)}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

function fmtSavedUsd(usd: number): string {
  if (!Number.isFinite(usd) || usd < 0.01) return '';
  if (usd >= 10) return `$${usd.toFixed(1)}`;
  return `$${usd.toFixed(2)}`;
}

function modelDotColor(name: string): string {
  const lower = name.toLowerCase();
  if (lower.includes('claude')) return 'bg-violet-500';
  if (lower.includes('gemini')) return 'bg-blue-500';
  if (lower.includes('gpt') || lower.includes('o1') || lower.includes('o3')) return 'bg-amber-500';
  return 'bg-emerald-500';
}

function resolveTier(acc: BackendAccount): 'ULTRA' | 'PRO' | 'ENTERPRISE' | 'FREE' {
  const sub = (acc.quota?.subscription_tier || '').toUpperCase();
  if (sub.includes('ULTRA') || sub.includes('HELIUM')) return 'ULTRA';
  if (sub.includes('PRO') || sub.includes('PREMIUM')) return 'PRO';
  if (sub.includes('ENTERPRISE')) return 'ENTERPRISE';
  if (acc.tags?.some((t) => t.toUpperCase().includes('ULTRA'))) return 'ULTRA';
  if (acc.tags?.some((t) => t.toUpperCase().includes('PRO'))) return 'PRO';
  return 'FREE';
}

function formatCandidatePoolTooltip(label: string, q: ParsedPoolQuota): string {
  const parts: string[] = [];
  if (q.fiveHour) parts.push(`5h ${q.fiveHour.percentage}%`);
  if (q.sevenDay) parts.push(`7d ${q.sevenDay.percentage}%`);
  return parts.length > 0 ? `${label}: ${parts.join(' · ')}` : `${label}: --`;
}

interface MiniPoolRowsProps {
  pool: 'claude' | 'gemini';
  quota: ParsedPoolQuota;
  lang: string;
}

const MiniPoolRows: React.FC<MiniPoolRowsProps> = React.memo(({ pool, quota, lang }) => {
  const isClaude = pool === 'claude';
  const rows: PoolWindowQuota[] = [];
  if (quota.fiveHour) rows.push(quota.fiveHour);
  if (quota.sevenDay) rows.push(quota.sevenDay);

  const label = isClaude ? 'Claude' : 'Gemini';
  const railClass = isClaude
    ? 'bg-gradient-to-b from-violet-400 to-purple-600'
    : 'bg-gradient-to-b from-sky-400 to-blue-600';

  return (
    <div className="flex items-stretch gap-1.5">
      {/* 左侧池色标 + 池名（与 5h/7d 行垂直居中对齐） */}
      <div className="w-[46px] shrink-0 flex items-center gap-1.5">
        <span className={`w-[2.5px] self-stretch my-[1px] rounded-full shrink-0 ${railClass}`} />
        <span className="text-[10.5px] font-bold tracking-tight text-slate-700 dark:text-zinc-200 leading-none">
          {label}
        </span>
      </div>

      {/* 右侧真实窗口行：5h / 7d 各自独立晶格矩阵条 + 百分比 + 实时倒计时 */}
      <div className="flex-1 min-w-0 flex flex-col gap-1 justify-center">
        {rows.length === 0 ? (
          <div className="flex items-center gap-1.5 leading-none">
            <QuotaWindowBadge window="5h" className="w-[22px] text-[8.5px] leading-[13px] opacity-40" />
            <QuotaMatrixBar pool={pool} window="5h" percentage={0} empty className="h-[11px]" />
            <span className="w-[32px] shrink-0 text-right font-mono tabular-nums text-[10px] font-semibold text-gray-400 dark:text-zinc-500">
              --
            </span>
            <span className="w-[40px] shrink-0 text-right font-mono tabular-nums text-[9px] text-gray-400 dark:text-zinc-500">
              --
            </span>
          </div>
        ) : (
          rows.map((item) => {
            const baseTip =
              item.window === '5h'
                ? tUi(lang, 'quota_5h_tooltip', { pct: item.percentage })
                : tUi(lang, 'quota_7d_tooltip', { pct: item.percentage });
            const paceTip =
              item.timeRemainingPct !== undefined && item.paceStatus
                ? ` · ${tUi(lang, 'quota_time_left', { pct: item.timeRemainingPct })} · ${tUi(
                    lang,
                    item.paceStatus === 'safe'
                      ? 'quota_pace_safe'
                      : item.paceStatus === 'warn'
                      ? 'quota_pace_warn'
                      : 'quota_pace_fast'
                  )}`
                : '';
            const tooltip = `${baseTip}${paceTip}`;

            return (
              <div key={item.window} title={tooltip} className="flex items-center gap-1.5 leading-none">
                <QuotaWindowBadge window={item.window} className="w-[22px] text-[8.5px] leading-[13px]" />
                <QuotaMatrixBar
                  pool={pool}
                  window={item.window}
                  percentage={item.percentage}
                  className="h-[11px]"
                />
                <span
                  className={`w-[32px] shrink-0 text-right font-mono tabular-nums text-[10.5px] font-bold ${quotaPctTextClass(
                    item.percentage
                  )}`}
                >
                  {item.percentage}%
                </span>
                <span className="w-[40px] shrink-0 text-right font-mono tabular-nums text-[9px] whitespace-nowrap truncate text-slate-400 dark:text-zinc-500">
                  {item.resetText}
                </span>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
});

function parseTodayReport(report: any): TodayStatsState {
  const hourly = createEmptyHourly();
  let peakTps = 0;

  const traffic = Array.isArray(report?.recentTraffic) ? report.recentTraffic : [];
  for (const r of traffic) {
    let hour = -1;
    const ts = Number(r?.timestamp || 0);
    if (ts > 0 && !Number.isNaN(ts)) {
      hour = new Date(ts).getHours();
    } else if (typeof r?.timeStr === 'string') {
      const m = r.timeStr.match(/(\d{2}):\d{2}/);
      if (m) hour = Math.min(23, Math.max(0, parseInt(m[1], 10)));
    }
    const toks =
      Number(r?.promptTokens || 0) +
      Number(r?.cachedTokens || 0) +
      Number(r?.completionTokens || 0);
    if (hour >= 0 && hour < 24) {
      hourly[hour].tokens += toks;
      hourly[hour].requests += 1;
    }
    const tps = Number(r?.tokensPerSecond || 0);
    if (tps > peakTps) peakTps = tps;
  }

  const rawModels = Array.isArray(report?.byModel) ? report.byModel : [];
  const topModels: TodayModelItem[] = rawModels.slice(0, 6).map((m: any) => ({
    name: String(m?.requestedModel || m?.physicalModel || 'unknown'),
    tokens: Number(m?.totalTokens || 0),
    sharePct: Number(m?.sharePercentage || 0),
    avgTps: Number(m?.avgTps || 0),
    requests: Number(m?.totalRequests || 0),
  }));

  const totalTokens = Number(report?.totalTokens || 0);
  const promptTokens = Number(report?.totalPromptTokens || 0);
  const cachedTokens = Number(report?.totalCachedTokens || 0);
  const outputTokens = Number(report?.totalOutputTokens || 0);
  const totalRequests = Number(report?.totalRequests || 0);
  const cacheHitRate = Number(report?.globalCacheHitRate || 0);
  const avgTps = Number(report?.avgSpeedTps || 0);
  const peakSpeedTps = Math.max(Number(report?.peakSpeedTps || 0), peakTps);
  const avgTtftMs = Number(report?.avgTtftMs || 0);
  const avgDurationMs = Number(report?.avgDurationMs || 0);
  const savedUsd = Number(report?.totalSavedCostUsd || 0);
  const runningCount = Number(report?.agentHierarchy?.runningCount || 0);

  return {
    loaded: true,
    totalTokens,
    promptTokens,
    cachedTokens,
    outputTokens,
    totalRequests,
    cacheHitRate,
    avgTps,
    peakTps: peakSpeedTps,
    avgTtftMs,
    avgDurationMs,
    savedUsd,
    runningCount,
    hourly,
    topModels,
  };
}

export const MiniPopover: React.FC<MiniPopoverProps> = ({ lang, theme, onToggleTheme }) => {
  const [viewMode, setViewMode] = useState<MiniViewMode>('switch');
  const viewModeRef = useRef<MiniViewMode>(viewMode);
  viewModeRef.current = viewMode;
  const [selectedHost, setSelectedHost] = useState<HostTarget>('ide');
  const [accounts, setAccounts] = useState<BackendAccount[]>([]);
  const [hosts, setHosts] = useState<ActiveHostStatus | null>(null);
  const [pinned, setPinned] = useState(false);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [switchingId, setSwitchingId] = useState<string | null>(null);
  const [justSwitched, setJustSwitched] = useState(false);
  const [todayStats, setTodayStats] = useState<TodayStatsState>(INITIAL_TODAY_STATS);
  const [hoveredHour, setHoveredHour] = useState<number | null>(null);
  const [accountsSortMode, setAccountsSortMode] = useState<AccountsSortMode>(
    () => readStoredAccountsSortMode() ?? 'health_default'
  );
  const [pickStrategy, setPickStrategy] = useState<DashboardPickStrategy>(
    () => readStoredDashboardPickStrategy() ?? 'max-quota'
  );
  const [poolPreference, setPoolPreference] = useState<DashboardPoolPreference>(
    () => readStoredDashboardPoolPreference() ?? 'balanced'
  );

  // Zero-Border Mini View 透明底色注入
  useEffect(() => {
    const html = document.documentElement;
    const body = document.body;
    const prevHtmlBg = html.style.background;
    const prevBodyBg = body.style.background;
    html.style.background = 'transparent';
    body.style.background = 'transparent';
    return () => {
      html.style.background = prevHtmlBg;
      body.style.background = prevBodyBg;
    };
  }, []);

  const loadTodayStats = useCallback(async () => {
    try {
      const now = new Date();
      const startOfTodayMs = new Date(
        now.getFullYear(),
        now.getMonth(),
        now.getDate(),
        0,
        0,
        0,
        0
      ).getTime();
      const report = await api
        .getStatsReport({ startTsMs: startOfTodayMs, endTsMs: null })
        .catch(() => null);
      if (report) {
        setTodayStats(parseTodayReport(report));
      } else {
        setTodayStats((prev) => ({ ...prev, loaded: true }));
      }
    } catch (e) {
      console.warn('[MiniPopover] today stats failed:', e);
    }
  }, []);

  const loadData = useCallback(async () => {
    try {
      const [accs, hostStatus, isPin, appCfg] = await Promise.all([
        api.getAccounts(),
        api.getActiveHosts(),
        api.getMiniPinned(),
        api.getAppConfig().catch(() => null),
      ]);
      if (accs) setAccounts(accs);
      if (hostStatus) setHosts(hostStatus);
      setPinned(Boolean(isPin));
      if (appCfg) {
        const ui = appCfg.ui_state || {};
        setAccountsSortMode(
          normalizeSortMode(ui.accounts_sort_mode || readStoredAccountsSortMode())
        );
        setPickStrategy(
          normalizePickStrategy(
            ui.dashboard_pick_strategy || appCfg.auto_switch?.strategy || readStoredDashboardPickStrategy()
          )
        );
        setPoolPreference(
          normalizePoolPreference(
            ui.dashboard_pool_preference || appCfg.auto_switch?.pool || readStoredDashboardPoolPreference()
          )
        );
      }
      // 仅当用户当前处于「今日数据」视图时再查询本地统计，避免在默认切号视图白跑统计扫描
      if (viewModeRef.current === 'stats') {
        void loadTodayStats();
      }
    } catch (e) {
      console.warn('[MiniPopover] load failed:', e);
    }
  }, [loadTodayStats]);

  useEffect(() => {
    loadData();
    const onStorage = (e: StorageEvent) => {
      if (
        e.key === 'am_accounts_sort_mode' ||
        e.key === 'am_dashboard_pick_strategy' ||
        e.key === 'am_dashboard_pool_preference'
      ) {
        void loadData();
      }
    };
    window.addEventListener('storage', onStorage);

    if (!isTauri()) {
      return () => {
        window.removeEventListener('storage', onStorage);
      };
    }
    let disposed = false;
    let unlistenOpen: (() => void) | undefined;
    let unlistenChanged: (() => void) | undefined;
    let unlistenUiState: (() => void) | undefined;

    import('@tauri-apps/api/event').then(async ({ listen }) => {
      const offOpen = await listen('tray://mini-opened', () => {
        loadData();
      });
      if (disposed) {
        offOpen();
        return;
      }
      unlistenOpen = offOpen;

      const offChanged = await listen('tray://accounts-changed', () => {
        loadData();
      });
      if (disposed) {
        offChanged();
        return;
      }
      unlistenChanged = offChanged;

      const offUiState = await listen('tray://ui-state-changed', () => {
        loadData();
      });
      if (disposed) {
        offUiState();
        return;
      }
      unlistenUiState = offUiState;
    });

    return () => {
      disposed = true;
      window.removeEventListener('storage', onStorage);
      unlistenOpen?.();
      unlistenChanged?.();
      unlistenUiState?.();
    };
  }, [loadData]);

  // 当停留在「今日数据」视图且窗口可见时，轻量 15s 自动保鲜（特别适合置顶常驻观察）
  useEffect(() => {
    if (viewMode !== 'stats') return;
    const timer = window.setInterval(() => {
      if (typeof document !== 'undefined' && document.hidden) return;
      void loadTodayStats();
    }, 15_000);
    return () => window.clearInterval(timer);
  }, [viewMode, loadTodayStats]);

  const handleHeaderDrag = async (e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest('button')) return;
    if (!isTauri()) return;
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      await getCurrentWindow().startDragging();
    } catch {}
  };

  const activeAccountForHost = useMemo(() => {
    const targetKey =
      selectedHost === 'app'
        ? hosts?.app_account
        : selectedHost === 'ide'
        ? hosts?.ide_account
        : selectedHost === 'plugin'
        ? hosts?.plugin_account
        : hosts?.agy_account;

    if (targetKey) {
      const found = accounts.find(
        (a) => a.id === targetKey || a.email.toLowerCase() === targetKey.toLowerCase()
      );
      if (found) return found;
      return {
        id: targetKey,
        email: targetKey,
        token: { access_token: '', refresh_token: '' },
        tags: [],
        disabled: false,
        banned: false,
      } as BackendAccount;
    }
    return null;
  }, [accounts, hosts, selectedHost]);

  const claudeQuota = useMemo(
    () => extractAccountPoolQuota(activeAccountForHost, 'claude'),
    [activeAccountForHost]
  );
  const geminiQuota = useMemo(
    () => extractAccountPoolQuota(activeAccountForHost, 'gemini'),
    [activeAccountForHost]
  );

  // 当前活跃账号若缓存已过期（例如 5h 重置点已过或超 5 分钟未同步），静默触发一次真实接口保鲜（单账号 60s 节流防抖）
  const autoRefreshedAtRef = useRef<Map<string, number>>(new Map());
  useEffect(() => {
    if (viewMode !== 'switch') return;
    if (!activeAccountForHost || !isAccountQuotaStale(activeAccountForHost)) return;
    const accId = activeAccountForHost.id;
    const now = Date.now();
    const lastAttempt = autoRefreshedAtRef.current.get(accId) || 0;
    if (now - lastAttempt < 60_000) return;

    autoRefreshedAtRef.current.set(accId, now);
    let cancelled = false;
    setIsRefreshing(true);
    void api
      .refreshAccountQuota(accId)
      .then(() => {
        if (!cancelled) return loadData();
      })
      .catch(() => {})
      .finally(() => {
        if (!cancelled) setIsRefreshing(false);
      });

    return () => {
      cancelled = true;
    };
  }, [activeAccountForHost, viewMode, loadData]);

  // 候选账号列表严格遵循主窗口「账号管理」页的排序模式（tier_desc / name_asc / custom / health_default）
  const healthyCandidates = useMemo(() => {
    const list = accounts
      .filter((a) => !a.disabled && !a.banned && !a.validation_blocked && !a.quota?.is_forbidden)
      .map((a) => {
        const gPool = extractAccountPoolQuota(a, 'gemini');
        const cPool = extractAccountPoolQuota(a, 'claude');
        const g = gPool.hasData ? gPool.effectivePct : 100;
        const c = cPool.hasData ? cPool.effectivePct : 100;
        const tier = resolveTier(a);
        const tooltip = `${formatCandidatePoolTooltip('Gemini', gPool)} | ${formatCandidatePoolTooltip('Claude', cPool)}`;
        return { acc: a, g, c, tier, tooltip };
      });

    if (accountsSortMode === 'custom' || accountsSortMode === 'health_default') {
      return list;
    }

    return [...list].sort((a, b) => {
      if (accountsSortMode === 'name_asc') {
        return a.acc.email.localeCompare(b.acc.email);
      }
      if (accountsSortMode === 'tier_desc') {
        const diff = tierSortWeight(b.tier) - tierSortWeight(a.tier);
        if (diff !== 0) return diff;
        return a.acc.email.localeCompare(b.acc.email);
      }
      return 0;
    });
  }, [accounts, accountsSortMode]);

  const handleTogglePin = async () => {
    const next = await api.setMiniPinned(!pinned);
    setPinned(next);
  };

  const handleRefresh = async () => {
    if (isRefreshing) return;
    setIsRefreshing(true);
    try {
      if (viewMode === 'stats') {
        await loadTodayStats();
      } else {
        if (activeAccountForHost) {
          await api.refreshAccountQuota(activeAccountForHost.id);
        } else {
          await api.refreshAllQuotas();
        }
        await loadData();
      }
    } catch {}
    setIsRefreshing(false);
  };

  const handleSmartSwitch = async () => {
    setSwitchingId('__smart__');
    try {
      await api.smartPickAccount(selectedHost, pickStrategy, poolPreference);
      await loadData();
      setJustSwitched(true);
      setTimeout(() => setJustSwitched(false), 1400);
    } catch {}
    setSwitchingId(null);
  };

  const handleDirectSwitch = async (accId: string) => {
    if (switchingId) return;
    setSwitchingId(accId);
    try {
      await api.switchAccount(accId, selectedHost, false);
      await loadData();
      setJustSwitched(true);
      setTimeout(() => setJustSwitched(false), 1200);
    } catch {}
    setSwitchingId(null);
  };

  const hostTabs: { id: HostTarget; label: string; live: boolean; icon: React.FC<{ className?: string }> }[] = [
    { id: 'app', label: 'App', live: Boolean(hosts?.app_running), icon: AppWindow },
    { id: 'ide', label: 'IDE', live: Boolean(hosts?.ide_running), icon: Code2 },
    { id: 'plugin', label: tUi(lang, 'mini_host_plugin'), live: Boolean(hosts?.plugin_running), icon: Puzzle },
    { id: 'cli', label: 'CLI', live: Boolean(hosts?.agy_running), icon: Terminal },
  ];

  const activeTier = activeAccountForHost ? resolveTier(activeAccountForHost) : 'FREE';
  const currentHostLabel = hostTabs.find((h) => h.id === selectedHost)?.label || selectedHost.toUpperCase();

  const activeStrategyLabel =
    pickStrategy === 'expiring-soon'
      ? tUi(lang, 'strategy_expiring_soon')
      : pickStrategy === 'tier-first'
      ? tUi(lang, 'strategy_tier_first')
      : pickStrategy === 'least-used'
      ? tUi(lang, 'strategy_least_used')
      : tUi(lang, 'strategy_max_quota');

  const activePoolLabel =
    poolPreference === 'gemini'
      ? 'Gemini'
      : poolPreference === 'claude'
      ? 'Claude'
      : tUi(lang, 'pool_balanced');

  const activeSortLabel =
    accountsSortMode === 'tier_desc'
      ? tUi(lang, 'sort_tier')
      : accountsSortMode === 'name_asc'
      ? tUi(lang, 'sort_alpha')
      : accountsSortMode === 'custom'
      ? tUi(lang, 'sort_custom')
      : tUi(lang, 'sort_health');

  // 今日统计衍生展示指标
  const hasTodayData = todayStats.totalRequests > 0 || todayStats.totalTokens > 0;
  const savedCostLabel = fmtSavedUsd(todayStats.savedUsd);
  const nowDate = new Date();
  const nowHour = nowDate.getHours();
  const todayDateBadge = `${String(nowDate.getMonth() + 1).padStart(2, '0')}/${String(nowDate.getDate()).padStart(2, '0')}`;

  const tokenComposition = useMemo(() => {
    const total = todayStats.cachedTokens + todayStats.promptTokens + todayStats.outputTokens;
    if (total <= 0) {
      return { cachedPct: 0, promptPct: 0, outputPct: 0 };
    }
    const c = todayStats.cachedTokens > 0 ? Math.max(2, (todayStats.cachedTokens / total) * 100) : 0;
    const p = todayStats.promptTokens > 0 ? Math.max(2, (todayStats.promptTokens / total) * 100) : 0;
    const o = todayStats.outputTokens > 0 ? Math.max(2, (todayStats.outputTokens / total) * 100) : 0;
    const sum = c + p + o || 1;
    return {
      cachedPct: (c / sum) * 100,
      promptPct: (p / sum) * 100,
      outputPct: (o / sum) * 100,
    };
  }, [todayStats.cachedTokens, todayStats.promptTokens, todayStats.outputTokens]);

  const hourlyPeak = useMemo(() => {
    let maxTokens = 0;
    let peakHour = -1;
    for (const b of todayStats.hourly) {
      if (b.tokens > maxTokens) {
        maxTokens = b.tokens;
        peakHour = b.hour;
      }
    }
    return { maxTokens, peakHour };
  }, [todayStats.hourly]);

  const cacheColorClass = !hasTodayData
    ? 'text-gray-400 dark:text-zinc-500'
    : todayStats.cacheHitRate >= 75
    ? 'text-emerald-600 dark:text-emerald-400'
    : todayStats.cacheHitRate >= 45
    ? 'text-amber-600 dark:text-amber-400'
    : 'text-rose-600 dark:text-rose-400';

  return (
    <div className="w-full h-full rounded-2xl bg-[#FAFBFC]/95 dark:bg-[#141519]/95 backdrop-blur-2xl border border-gray-200/90 dark:border-white/[0.12] flex flex-col overflow-hidden select-none text-slate-900 dark:text-zinc-100">
      {/* 1. 顶部拖拽与控制栏（含 Apple 风格视图切换分段胶囊） */}
      <div
        onMouseDown={handleHeaderDrag}
        className="h-10 px-3 bg-white/70 dark:bg-white/[0.03] flex items-center justify-between border-b border-gray-200/70 dark:border-white/[0.07] shrink-0 cursor-default"
      >
        <div className="flex items-center gap-1.5 min-w-0">
          <img src="/logo.png" alt="Logo" className="w-4 h-4 rounded-sm shrink-0" />
          <span className="text-xs font-bold tracking-tight text-gray-900 dark:text-zinc-100 truncate">
            Antigravity
          </span>
          <span className="px-1.5 py-0.5 rounded-full bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 text-[9.5px] font-mono font-semibold shrink-0 tabular-nums">
            {healthyCandidates.length}/{accounts.length}
          </span>
        </div>

        <div className="flex items-center shrink-0">
          {/* 视图切换胶囊：账号切号 / 今日数据 */}
          <div className="inline-flex items-center p-0.5 rounded-lg bg-gray-200/75 dark:bg-white/[0.07] border border-gray-200/60 dark:border-white/[0.06] gap-0.5">
            <button
              tabIndex={-1}
              onClick={() => setViewMode('switch')}
              title={tUi(lang, 'mini_view_switch')}
              aria-label={tUi(lang, 'mini_view_switch')}
              className={`px-1.5 py-1 rounded-[6px] transition-all cursor-pointer focus:outline-none ${
                viewMode === 'switch'
                  ? 'bg-white dark:bg-[#23252f] text-blue-600 dark:text-blue-400 shadow-xs'
                  : 'text-gray-500 dark:text-zinc-400 hover:text-gray-900 dark:hover:text-zinc-200'
              }`}
            >
              <ArrowLeftRight className="w-3 h-3" />
            </button>
            <button
              tabIndex={-1}
              onClick={() => {
                setViewMode('stats');
                void loadTodayStats();
              }}
              title={tUi(lang, 'mini_view_stats')}
              aria-label={tUi(lang, 'mini_view_stats')}
              className={`px-1.5 py-1 rounded-[6px] transition-all cursor-pointer focus:outline-none ${
                viewMode === 'stats'
                  ? 'bg-white dark:bg-[#23252f] text-blue-600 dark:text-blue-400 shadow-xs'
                  : 'text-gray-500 dark:text-zinc-400 hover:text-gray-900 dark:hover:text-zinc-200'
              }`}
            >
              <BarChart3 className="w-3 h-3" />
            </button>
          </div>

          <span className="w-px h-3.5 bg-gray-200/90 dark:bg-white/10 mx-1.5" />

          <div className="flex items-center gap-0.5">
            <button
              tabIndex={-1}
              onClick={handleRefresh}
              title={viewMode === 'stats' ? tUi(lang, 'mini_refresh_stats') : tUi(lang, 'mini_refresh_quota')}
              className="p-1 rounded-lg text-gray-500 hover:text-gray-900 dark:text-zinc-400 dark:hover:text-zinc-100 hover:bg-gray-200/60 dark:hover:bg-white/[0.08] transition-colors cursor-pointer focus:outline-none"
            >
              <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin text-blue-500' : ''}`} />
            </button>
            <button
              tabIndex={-1}
              onClick={onToggleTheme}
              title={tUi(lang, 'mini_toggle_theme')}
              className="p-1 rounded-lg text-gray-500 hover:text-gray-900 dark:text-zinc-400 dark:hover:text-zinc-100 hover:bg-gray-200/60 dark:hover:bg-white/[0.08] transition-colors cursor-pointer focus:outline-none"
            >
              {theme === 'dark' ? <Sun className="w-3.5 h-3.5" /> : <Moon className="w-3.5 h-3.5" />}
            </button>
            <button
              tabIndex={-1}
              onClick={handleTogglePin}
              title={pinned ? tUi(lang, 'mini_unpin') : tUi(lang, 'mini_pin')}
              className={`p-1 rounded-lg transition-colors cursor-pointer focus:outline-none ${
                pinned
                  ? 'bg-blue-500/15 text-blue-600 dark:text-blue-400'
                  : 'text-gray-500 hover:text-gray-900 dark:text-zinc-400 dark:hover:text-zinc-100 hover:bg-gray-200/60 dark:hover:bg-white/[0.08]'
              }`}
            >
              <Pin className="w-3.5 h-3.5" />
            </button>
            <button
              tabIndex={-1}
              onClick={() => api.showMainWindow(viewMode === 'stats' ? 'stats' : undefined)}
              title={tUi(lang, 'mini_open_studio')}
              className="p-1 rounded-lg text-gray-500 hover:text-gray-900 dark:text-zinc-400 dark:hover:text-zinc-100 hover:bg-gray-200/60 dark:hover:bg-white/[0.08] transition-colors cursor-pointer focus:outline-none"
            >
              <Maximize2 className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
      </div>

      {/* ==================== 视图 A：多宿主账号快切视图 ==================== */}
      {viewMode === 'switch' ? (
        <>
          {/* 2. 四宿主运行态切换胶囊 */}
          <div className="px-3 pt-2.5 pb-1.5 shrink-0">
            <div className="grid grid-cols-4 gap-1 p-1 rounded-xl bg-gray-200/70 dark:bg-white/[0.05] border border-gray-200/80 dark:border-white/[0.06]">
              {hostTabs.map((h) => {
                const Icon = h.icon;
                const active = selectedHost === h.id;
                return (
                  <button
                    key={h.id}
                    onClick={() => setSelectedHost(h.id)}
                    className={`py-1 px-1.5 rounded-lg text-[10.5px] font-semibold flex items-center justify-center gap-1 transition-all cursor-pointer ${
                      active
                        ? 'bg-white dark:bg-[#22242d] text-blue-600 dark:text-blue-400 shadow-sm'
                        : 'text-gray-600 dark:text-zinc-400 hover:text-gray-900 dark:hover:text-zinc-200'
                    }`}
                  >
                    <span
                      className={`w-1.5 h-1.5 rounded-full shrink-0 ${
                        h.live ? 'bg-emerald-500 shadow-[0_0_6px_rgba(16,185,129,0.7)]' : 'bg-gray-400/60 dark:bg-zinc-600'
                      }`}
                    />
                    <Icon className="w-3 h-3 shrink-0" />
                    <span className="truncate">{h.label}</span>
                  </button>
                );
              })}
            </div>
          </div>

          {/* 3. 当前宿主绑定账号与双池实时水位 */}
          <div className="px-3 py-1 shrink-0">
            <div className="px-2.5 py-2 rounded-xl bg-white dark:bg-white/[0.04] border border-gray-200/80 dark:border-white/[0.08] shadow-sm flex flex-col gap-1.5">
              <div className="flex items-center justify-between gap-2">
                <div className="min-w-0">
                  <div className="text-xs font-bold text-gray-900 dark:text-zinc-100 truncate">
                    {activeAccountForHost ? activeAccountForHost.email : tUi(lang, 'mini_no_account')}
                  </div>
                </div>
                {activeAccountForHost && (
                  <span
                    className={`px-1.5 py-0.5 rounded-md text-[9.5px] font-bold tracking-wide shrink-0 ${
                      activeTier === 'ULTRA'
                        ? 'bg-amber-500/15 text-amber-600 dark:text-amber-400'
                        : activeTier === 'PRO'
                        ? 'bg-blue-500/15 text-blue-600 dark:text-blue-400'
                        : activeTier === 'ENTERPRISE'
                        ? 'bg-indigo-500/15 text-indigo-600 dark:text-indigo-400'
                        : 'bg-gray-200/70 dark:bg-white/10 text-gray-600 dark:text-zinc-400'
                    }`}
                  >
                    {activeTier}
                  </span>
                )}
              </div>

              {/* Gemini & Claude 双池双轨晶格水位面板 (Gemini 优先在上，与主工作台统一视觉语言) */}
              <div className="rounded-lg bg-slate-50/90 dark:bg-black/25 border border-slate-200/70 dark:border-white/[0.06] px-2 py-1.5 flex flex-col gap-1.5">
                <MiniPoolRows pool="gemini" quota={geminiQuota} lang={lang} />
                <div className="h-px bg-slate-200/75 dark:bg-white/[0.06]" />
                <MiniPoolRows pool="claude" quota={claudeQuota} lang={lang} />
              </div>

              {/* 一键智能换号按钮（实时遵循主窗口仪表盘 / 设置选号策略与模型池偏好） */}
              <button
                onClick={handleSmartSwitch}
                disabled={switchingId !== null || accounts.length === 0}
                title={`${activeStrategyLabel} · ${activePoolLabel}`}
                className="w-full py-1.5 px-3 rounded-lg bg-blue-600 hover:bg-blue-500 active:bg-blue-700 disabled:opacity-50 text-white text-[11px] font-semibold flex items-center justify-center gap-1.5 shadow-sm transition-all cursor-pointer"
              >
                {justSwitched ? (
                  <>
                    <Check className="w-3.5 h-3.5" />
                    <span>{tUi(lang, 'mini_switched')}</span>
                  </>
                ) : (
                  <>
                    <Sparkles className="w-3.5 h-3.5" />
                    <span>{tUi(lang, 'mini_smart_switch', { host: currentHostLabel })}</span>
                  </>
                )}
              </button>
            </div>
          </div>

          {/* 4. 备用候选账号一键直切列表（严格遵循主窗口账号列表排序） */}
          <div className="px-3 pt-1 pb-1.5 flex-1 min-h-0 flex flex-col">
            <div
              className="flex items-center justify-between text-[10px] font-medium text-gray-500 dark:text-zinc-400 px-1 mb-1"
              title={activeSortLabel}
            >
              <span className="truncate">{tUi(lang, 'mini_quick_pool')}</span>
              <span className="font-mono shrink-0">G / C</span>
            </div>
            <div className="mini-scroll flex-1 overflow-y-auto space-y-1 pr-0.5">
              {healthyCandidates.length === 0 ? (
                <div className="h-full flex items-center justify-center text-[11px] text-gray-400 text-center px-2">
                  {tUi(lang, 'mini_empty_pool')}
                </div>
              ) : (
                healthyCandidates.map(({ acc, c, g, tier, tooltip }) => {
                  const isCurrent = activeAccountForHost?.id === acc.id;
                  const isSwitching = switchingId === acc.id;
                  const cCls =
                    c <= 20
                      ? 'text-rose-500 dark:text-rose-400'
                      : c < 60
                      ? 'text-amber-500 dark:text-amber-400'
                      : 'text-violet-600 dark:text-violet-400';
                  const gCls =
                    g <= 20
                      ? 'text-rose-500 dark:text-rose-400'
                      : g < 60
                      ? 'text-amber-500 dark:text-amber-400'
                      : 'text-blue-600 dark:text-blue-400';
                  return (
                    <button
                      key={acc.id}
                      onClick={() => handleDirectSwitch(acc.id)}
                      title={tooltip}
                      className={`w-full px-2.5 py-1.5 rounded-xl text-left flex items-center justify-between gap-2 transition-colors cursor-pointer ${
                        isCurrent
                          ? 'bg-blue-50 dark:bg-blue-500/15 border border-blue-200/80 dark:border-blue-500/30'
                          : 'bg-white/60 dark:bg-white/[0.02] hover:bg-white dark:hover:bg-white/[0.06] border border-gray-200/50 dark:border-transparent'
                      }`}
                    >
                      <div className="flex items-center gap-1.5 min-w-0">
                        <span
                          className={`w-1.5 h-1.5 rounded-full shrink-0 ${
                            isCurrent ? 'bg-blue-500' : 'bg-emerald-500'
                          }`}
                        />
                        <span className="text-[11px] font-medium text-gray-800 dark:text-zinc-200 truncate">
                          {isSwitching ? tUi(lang, 'mini_switching') : acc.email}
                        </span>
                        <span
                          className={`px-1 py-[1px] rounded text-[8.5px] font-bold tracking-tight shrink-0 ${
                            tier === 'ULTRA'
                              ? 'bg-amber-500/15 text-amber-600 dark:text-amber-400'
                              : tier === 'PRO'
                              ? 'bg-blue-500/15 text-blue-600 dark:text-blue-400'
                              : tier === 'ENTERPRISE'
                              ? 'bg-indigo-500/15 text-indigo-600 dark:text-indigo-400'
                              : 'bg-gray-200/80 dark:bg-white/10 text-gray-500 dark:text-zinc-400'
                          }`}
                        >
                          {tier}
                        </span>
                      </div>
                      <div className="flex items-center gap-1.5 shrink-0 font-mono text-[10px] tabular-nums">
                        <span className={`${gCls} font-semibold`}>{g}%</span>
                        <span className="text-gray-300 dark:text-zinc-600">/</span>
                        <span className={`${cCls} font-semibold`}>{c}%</span>
                      </div>
                    </button>
                  );
                })
              )}
            </div>
          </div>
        </>
      ) : (
        /* ==================== 视图 B：Apple HIG 今日数据遥测视图 ==================== */
        <div className="px-3 pt-2.5 pb-2 flex-1 min-h-0 flex flex-col gap-2">
          {/* B1. 统一今日遥测主卡（用量构成 + 三列核心效率 + 24h 活跃节奏一体化） */}
          <div className="px-3 py-2.5 rounded-xl bg-white dark:bg-white/[0.04] border border-gray-200/80 dark:border-white/[0.08] shadow-sm shrink-0">
            {/* 1) Hero 今日 Token 吞吐与 macOS 存储式三色构成条 */}
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-1.5 min-w-0">
                <span className="text-[10.5px] font-semibold text-gray-600 dark:text-zinc-300 truncate">
                  {tUi(lang, 'mini_today_usage')}
                </span>
                <span className="text-[9.5px] font-mono text-gray-400 dark:text-zinc-500 tabular-nums">
                  {todayDateBadge}
                </span>
              </div>
              <div className="flex items-center gap-1 shrink-0">
                {todayStats.runningCount > 0 && (
                  <span className="inline-flex items-center gap-1 px-1.5 py-[1px] rounded-full bg-emerald-500/12 text-emerald-600 dark:text-emerald-400 text-[9px] font-semibold tabular-nums">
                    <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse" />
                    {tUi(lang, 'mini_running_live', { count: todayStats.runningCount })}
                  </span>
                )}
                <span className="px-1.5 py-[1px] rounded-md bg-gray-100 dark:bg-white/[0.07] text-gray-600 dark:text-zinc-300 text-[9.5px] font-mono font-medium tabular-nums">
                  {tUi(lang, 'mini_reqs_count', { count: todayStats.totalRequests })}
                </span>
              </div>
            </div>

            <div className="mt-1 flex items-baseline justify-between gap-2">
              <div className="flex items-baseline gap-1 min-w-0">
                <span className="text-[21px] leading-none font-bold font-mono tracking-tight text-gray-900 dark:text-zinc-50 tabular-nums">
                  {fmtCompactTokens(todayStats.totalTokens)}
                </span>
                <span className="text-[9.5px] font-medium text-gray-400 dark:text-zinc-500">
                  tokens
                </span>
              </div>
              {savedCostLabel && (
                <span className="px-1.5 py-0.5 rounded-md bg-emerald-500/12 text-emerald-600 dark:text-emerald-400 text-[9.5px] font-mono font-semibold shrink-0 tabular-nums">
                  {tUi(lang, 'mini_saved_cost', { amount: savedCostLabel })}
                </span>
              )}
            </div>

            <div className="mt-2 h-1.5 w-full rounded-full bg-gray-100 dark:bg-white/[0.08] overflow-hidden flex gap-[1.5px]">
              {tokenComposition.cachedPct > 0 && (
                <div
                  className="h-full bg-emerald-500 transition-all duration-300"
                  style={{ width: `${tokenComposition.cachedPct}%` }}
                />
              )}
              {tokenComposition.promptPct > 0 && (
                <div
                  className="h-full bg-blue-500 transition-all duration-300"
                  style={{ width: `${tokenComposition.promptPct}%` }}
                />
              )}
              {tokenComposition.outputPct > 0 && (
                <div
                  className="h-full bg-violet-500 transition-all duration-300"
                  style={{ width: `${tokenComposition.outputPct}%` }}
                />
              )}
            </div>

            <div className="mt-1.5 flex items-center justify-between text-[9.5px] font-mono text-gray-500 dark:text-zinc-400 tabular-nums">
              <span className="inline-flex items-center gap-1 truncate">
                <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 shrink-0" />
                {tUi(lang, 'mini_cache')} {fmtCompactTokens(todayStats.cachedTokens)}
              </span>
              <span className="inline-flex items-center gap-1 truncate">
                <span className="w-1.5 h-1.5 rounded-full bg-blue-500 shrink-0" />
                {tUi(lang, 'mini_input')} {fmtCompactTokens(todayStats.promptTokens)}
              </span>
              <span className="inline-flex items-center gap-1 truncate">
                <span className="w-1.5 h-1.5 rounded-full bg-violet-500 shrink-0" />
                {tUi(lang, 'mini_output')} {fmtCompactTokens(todayStats.outputTokens)}
              </span>
            </div>

            {/* 2) 内嵌三列核心效率与性能条（发丝分隔，零冗余边框） */}
            <div className="mt-2 pt-2 border-t border-gray-100 dark:border-white/[0.06] grid grid-cols-3 divide-x divide-gray-100 dark:divide-white/[0.06]">
              <div className="pr-2 min-w-0">
                <div className="text-[9.5px] font-medium text-gray-400 dark:text-zinc-500 truncate">
                  {tUi(lang, 'mini_cache_hit_rate')}
                </div>
                <div className={`mt-0.5 text-[14.5px] leading-tight font-bold font-mono tabular-nums ${cacheColorClass}`}>
                  {hasTodayData ? `${todayStats.cacheHitRate.toFixed(1)}%` : '--'}
                </div>
              </div>

              <div className="px-2 min-w-0">
                <div className="text-[9.5px] font-medium text-gray-400 dark:text-zinc-500 truncate">
                  {tUi(lang, 'mini_avg_speed')}
                </div>
                <div className="mt-0.5 flex items-baseline gap-0.5 min-w-0">
                  <span className="text-[14.5px] leading-tight font-bold font-mono text-gray-900 dark:text-zinc-100 tabular-nums">
                    {hasTodayData && todayStats.avgTps > 0 ? todayStats.avgTps.toFixed(0) : '--'}
                  </span>
                  <span className="text-[9px] font-mono text-gray-400 dark:text-zinc-500">
                    t/s
                  </span>
                </div>
              </div>

              <div
                className="pl-2 min-w-0"
                title={
                  todayStats.avgTtftMs > 0 && todayStats.avgDurationMs > 0
                    ? `TTFT ${fmtTtft(todayStats.avgTtftMs)} · ${tUi(lang, 'mini_avg_duration')} ${fmtTtft(todayStats.avgDurationMs)}`
                    : undefined
                }
              >
                <div className="text-[9.5px] font-medium text-gray-400 dark:text-zinc-500 truncate">
                  {todayStats.avgTtftMs > 0 ? tUi(lang, 'mini_ttft') : tUi(lang, 'mini_avg_duration')}
                </div>
                <div className="mt-0.5 flex items-baseline gap-1 min-w-0">
                  <span className="text-[14.5px] leading-tight font-bold font-mono text-gray-900 dark:text-zinc-100 tabular-nums">
                    {hasTodayData
                      ? fmtTtft(todayStats.avgTtftMs > 0 ? todayStats.avgTtftMs : todayStats.avgDurationMs)
                      : '--'}
                  </span>
                  {todayStats.peakTps > 0 && (
                    <span
                      title={`${tUi(lang, 'mini_peak_speed', { val: Math.round(todayStats.peakTps) })} tok/s`}
                      className="text-[9px] font-mono text-blue-500/85 dark:text-blue-400/85 truncate tabular-nums"
                    >
                      ↑{Math.round(todayStats.peakTps)}
                    </span>
                  )}
                </div>
              </div>
            </div>

            {/* 3) 今日 24h 活跃节奏柱（融入主卡底部，视觉一体化） */}
            {hasTodayData && (
              <div
                onMouseLeave={() => setHoveredHour(null)}
                className="mt-2 pt-2 border-t border-gray-100 dark:border-white/[0.06]"
              >
                <div className="flex items-center justify-between text-[9.5px] gap-2">
                  <span className="font-semibold text-gray-600 dark:text-zinc-300 truncate">
                    {tUi(lang, 'mini_today_rhythm')}
                  </span>
                  <span className="font-mono tabular-nums shrink-0">
                    {hoveredHour !== null ? (
                      <span className="text-blue-600 dark:text-blue-400 font-semibold">
                        {String(hoveredHour).padStart(2, '0')}:00 · {fmtCompactTokens(todayStats.hourly[hoveredHour]?.tokens || 0)} · {tUi(lang, 'mini_reqs_count', { count: todayStats.hourly[hoveredHour]?.requests || 0 })}
                      </span>
                    ) : hourlyPeak.peakHour >= 0 ? (
                      <span className="text-gray-400 dark:text-zinc-500">
                        {tUi(lang, 'mini_peak_hour', { hour: `${String(hourlyPeak.peakHour).padStart(2, '0')}:00` })} · {fmtCompactTokens(hourlyPeak.maxTokens)}
                      </span>
                    ) : (
                      <span className="text-gray-400 dark:text-zinc-500">24h</span>
                    )}
                  </span>
                </div>

                <div className="mt-1.5 h-6 flex items-end gap-[2.5px]">
                  {todayStats.hourly.map((bucket) => {
                    const isHovered = hoveredHour === bucket.hour;
                    const isCurrent = bucket.hour === nowHour;
                    const isFuture = bucket.hour > nowHour;
                    const hasTok = bucket.tokens > 0 && hourlyPeak.maxTokens > 0;
                    const pct = hasTok
                      ? Math.max(18, Math.round((bucket.tokens / hourlyPeak.maxTokens) * 100))
                      : 0;

                    return (
                      <div
                        key={bucket.hour}
                        onMouseEnter={() => setHoveredHour(bucket.hour)}
                        className="flex-1 h-full flex items-end cursor-pointer"
                      >
                        <div
                          className={`w-full rounded-full transition-all duration-150 ${
                            hasTok
                              ? isHovered
                                ? 'bg-blue-600 dark:bg-blue-400'
                                : isCurrent
                                ? 'bg-gradient-to-t from-blue-500 to-cyan-400'
                                : 'bg-blue-500/70 dark:bg-blue-400/65'
                              : isFuture
                              ? 'bg-gray-200/45 dark:bg-white/[0.04]'
                              : isHovered
                              ? 'bg-gray-400/70 dark:bg-white/25'
                              : 'bg-gray-200/85 dark:bg-white/[0.09]'
                          }`}
                          style={{ height: hasTok ? `${pct}%` : '3px' }}
                        />
                      </div>
                    );
                  })}
                </div>

                <div className="mt-1 flex items-center justify-between text-[8.5px] font-mono text-gray-400 dark:text-zinc-500 tabular-nums">
                  <span>00</span>
                  <span>06</span>
                  <span>12</span>
                  <span>18</span>
                  <span>24</span>
                </div>
              </div>
            )}
          </div>

          {/* B2. 今日主力模型列表 / 空态 */}
          {hasTodayData ? (
            <div className="flex-1 min-h-0 flex flex-col">
              <div className="flex items-center justify-between text-[9.5px] font-medium text-gray-500 dark:text-zinc-400 px-1 mb-1">
                <span className="truncate">{tUi(lang, 'mini_top_models')}</span>
                <span className="font-mono shrink-0">{tUi(lang, 'mini_model_metric_col')}</span>
              </div>
              <div className="mini-scroll flex-1 overflow-y-auto space-y-1 pr-0.5">
                {todayStats.topModels.map((m) => (
                  <div
                    key={m.name}
                    title={m.name}
                    className="px-2.5 py-1.5 rounded-xl bg-white/75 dark:bg-white/[0.03] border border-gray-200/60 dark:border-white/[0.06] flex items-center justify-between gap-2"
                  >
                    <div className="flex items-center gap-1.5 min-w-0">
                      <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${modelDotColor(m.name)}`} />
                      <span className="text-[10.5px] font-medium text-gray-800 dark:text-zinc-200 truncate">
                        {m.name}
                      </span>
                      <span className="text-[9px] font-mono text-gray-400 dark:text-zinc-500 shrink-0 tabular-nums">
                        {Math.round(m.sharePct)}%
                      </span>
                    </div>
                    <div className="flex items-center gap-1.5 shrink-0 font-mono text-[10px] tabular-nums">
                      <span className="font-semibold text-gray-700 dark:text-zinc-200">
                        {fmtCompactTokens(m.tokens)}
                      </span>
                      <span className="text-gray-300 dark:text-zinc-600">/</span>
                      <span className="text-blue-600 dark:text-blue-400 font-semibold">
                        {m.avgTps > 0 ? `${m.avgTps.toFixed(0)} t/s` : '--'}
                      </span>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ) : (
            /* 今日尚无调用记录时的克制空状态 */
            <div className="flex-1 min-h-0 rounded-xl bg-white/60 dark:bg-white/[0.02] border border-dashed border-gray-200/90 dark:border-white/[0.08] flex flex-col items-center justify-center text-center px-5 gap-1.5">
              <div className="w-7 h-7 rounded-full bg-blue-500/10 text-blue-500 flex items-center justify-center">
                <Activity className="w-3.5 h-3.5" />
              </div>
              <div className="text-[11px] font-semibold text-gray-700 dark:text-zinc-200">
                {tUi(lang, 'mini_stats_empty_title')}
              </div>
              <p className="text-[9.5px] leading-relaxed text-gray-400 dark:text-zinc-500">
                {tUi(lang, 'mini_stats_empty_desc')}
              </p>
            </div>
          )}
        </div>
      )}

      {/* 5. 底部实时遥测摘要 / 统计直达与退出控制栏 */}
      <div className="h-8 px-3.5 border-t border-gray-200/70 dark:border-white/[0.07] bg-white/80 dark:bg-black/30 flex items-center justify-between gap-2 shrink-0 text-[10px] text-gray-500 dark:text-zinc-400">
        {viewMode === 'switch' ? (
          <button
            onClick={() => {
              setViewMode('stats');
              void loadTodayStats();
            }}
            title={tUi(lang, 'mini_view_today_stats')}
            className="flex items-center gap-1.5 min-w-0 truncate rounded-md px-1 -ml-1 py-0.5 hover:bg-gray-200/60 dark:hover:bg-white/[0.07] hover:text-gray-900 dark:hover:text-zinc-100 transition-colors cursor-pointer group"
          >
            <Zap className="w-3 h-3 text-emerald-500 shrink-0" />
            {hasTodayData ? (
              <>
                <span className="font-mono truncate tabular-nums">
                  {tUi(lang, 'mini_today')} {fmtCompactTokens(todayStats.totalTokens)} · {todayStats.cacheHitRate.toFixed(0)}% · {todayStats.avgTps.toFixed(0)} t/s
                </span>
                <ChevronRight className="w-3 h-3 opacity-40 group-hover:opacity-100 group-hover:translate-x-0.5 transition-all shrink-0" />
              </>
            ) : (
              <span className="truncate">{tUi(lang, 'mini_guard_ready')}</span>
            )}
          </button>
        ) : (
          <button
            onClick={() => api.showMainWindow('stats')}
            className="flex items-center gap-1 min-w-0 truncate rounded-md px-1 -ml-1 py-0.5 text-blue-600 dark:text-blue-400 hover:bg-blue-500/10 transition-colors cursor-pointer font-medium"
          >
            <BarChart3 className="w-3 h-3 shrink-0" />
            <span className="truncate">{tUi(lang, 'mini_open_full_stats')}</span>
            <ArrowUpRight className="w-3 h-3 shrink-0" />
          </button>
        )}

        <button
          onClick={() => api.quitApp()}
          title={tUi(lang, 'mini_quit')}
          className="flex items-center gap-1 px-1.5 py-0.5 rounded-md text-gray-500 dark:text-zinc-400 hover:text-rose-600 dark:hover:text-rose-400 hover:bg-rose-500/10 transition-colors cursor-pointer shrink-0"
        >
          <Power className="w-3 h-3" />
          <span>{tUi(lang, 'mini_quit')}</span>
        </button>
      </div>
    </div>
  );
};
