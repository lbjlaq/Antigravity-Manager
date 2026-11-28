import { api, HostTarget, UiStateConfig } from '../api';
import type { TabId } from '../components/navbar/NavMenu';

export type DashboardPickStrategy = 'max-quota' | 'expiring-soon' | 'tier-first' | 'least-used';
export type DashboardPoolPreference = 'balanced' | 'claude' | 'gemini';
export type AccountsSortMode = 'health_default' | 'name_asc' | 'tier_desc' | 'custom';
export type AccountsViewMode = 'table' | 'card';

export const VALID_TABS: readonly TabId[] = ['dashboard', 'accounts', 'stats', 'toolbox', 'settings'];
const VALID_HOSTS: readonly HostTarget[] = ['app', 'ide', 'plugin', 'cli'];
const VALID_PICK_STRATEGIES: readonly DashboardPickStrategy[] = [
  'max-quota',
  'expiring-soon',
  'tier-first',
  'least-used',
];
const VALID_POOL_PREFS: readonly DashboardPoolPreference[] = ['balanced', 'claude', 'gemini'];
const VALID_SORT_MODES: readonly AccountsSortMode[] = [
  'health_default',
  'name_asc',
  'tier_desc',
  'custom',
];
const VALID_VIEW_MODES: readonly AccountsViewMode[] = ['table', 'card'];

const STORAGE_KEYS = {
  pendingTab: 'am_pending_tab',
  lastTab: 'am_last_tab',
  dashboardHost: 'am_dashboard_host',
  dashboardPickStrategy: 'am_dashboard_pick_strategy',
  dashboardPoolPreference: 'am_dashboard_pool_preference',
  accountsSortMode: 'am_accounts_sort_mode',
  accountsViewMode: 'am_accounts_view_mode',
  uiZoom: 'app-ui-zoom',
} as const;

const PENDING_TAB_TTL_MS = 5_000;

function readLs(key: string): string | null {
  if (typeof window === 'undefined') return null;
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeLs(key: string, value: string): void {
  if (typeof window === 'undefined') return;
  try {
    localStorage.setItem(key, value);
  } catch {}
}

function removeLs(key: string): void {
  if (typeof window === 'undefined') return;
  try {
    localStorage.removeItem(key);
  } catch {}
}

let pendingPatch: UiStateConfig = {};
let flushTimer: number | null = null;

function flushPendingUiState(): void {
  if (flushTimer !== null) {
    window.clearTimeout(flushTimer);
    flushTimer = null;
  }
  if (Object.keys(pendingPatch).length === 0) return;
  const toSend = pendingPatch;
  pendingPatch = {};
  void api.patchUiState(toSend).catch((err) => {
    console.warn('[uiState] patchUiState failed:', err);
  });
}

if (typeof window !== 'undefined') {
  window.addEventListener('beforeunload', flushPendingUiState);
}

function schedulePatchUiState(patch: UiStateConfig): void {
  pendingPatch = { ...pendingPatch, ...patch };
  if (typeof window === 'undefined') return;
  if (flushTimer !== null) {
    window.clearTimeout(flushTimer);
  }
  flushTimer = window.setTimeout(flushPendingUiState, 100);
}

/**
 * 消费来自 Mini 小窗的一次性跳转目标（仅 5 秒内有效，防止旧跳转残留污染冷启动）
 */
export function consumeFreshPendingTab(): TabId | null {
  const raw = readLs(STORAGE_KEYS.pendingTab);
  if (!raw) return null;
  removeLs(STORAGE_KEYS.pendingTab);
  try {
    const parsed = JSON.parse(raw) as { tab?: string; ts?: number };
    if (
      parsed &&
      typeof parsed.tab === 'string' &&
      typeof parsed.ts === 'number' &&
      Date.now() - parsed.ts <= PENDING_TAB_TTL_MS &&
      VALID_TABS.includes(parsed.tab as TabId)
    ) {
      return parsed.tab as TabId;
    }
  } catch {
    // 兼容旧版裸字符串格式：直接丢弃，防止上次残留挟持冷启动
  }
  return null;
}

export function readStoredLastTab(): TabId | null {
  const saved = readLs(STORAGE_KEYS.lastTab);
  return saved && VALID_TABS.includes(saved as TabId) ? (saved as TabId) : null;
}

export function saveLastTab(tab: TabId): void {
  writeLs(STORAGE_KEYS.lastTab, tab);
  schedulePatchUiState({ last_tab: tab });
}

export function readStoredDashboardHost(): HostTarget | null {
  const saved = readLs(STORAGE_KEYS.dashboardHost);
  return saved && VALID_HOSTS.includes(saved as HostTarget) ? (saved as HostTarget) : null;
}

export function saveDashboardHost(host: HostTarget): void {
  writeLs(STORAGE_KEYS.dashboardHost, host);
  schedulePatchUiState({ dashboard_host: host });
}

export function readStoredDashboardPickStrategy(): DashboardPickStrategy | null {
  const saved = readLs(STORAGE_KEYS.dashboardPickStrategy);
  return saved && VALID_PICK_STRATEGIES.includes(saved as DashboardPickStrategy)
    ? (saved as DashboardPickStrategy)
    : null;
}

export function saveDashboardPickStrategy(strategy: DashboardPickStrategy): void {
  writeLs(STORAGE_KEYS.dashboardPickStrategy, strategy);
  schedulePatchUiState({ dashboard_pick_strategy: strategy });
}

export function readStoredDashboardPoolPreference(): DashboardPoolPreference | null {
  const saved = readLs(STORAGE_KEYS.dashboardPoolPreference);
  return saved && VALID_POOL_PREFS.includes(saved as DashboardPoolPreference)
    ? (saved as DashboardPoolPreference)
    : null;
}

export function saveDashboardPoolPreference(pool: DashboardPoolPreference): void {
  writeLs(STORAGE_KEYS.dashboardPoolPreference, pool);
  schedulePatchUiState({ dashboard_pool_preference: pool });
}

export function readStoredAccountsSortMode(): AccountsSortMode | null {
  const saved = readLs(STORAGE_KEYS.accountsSortMode);
  return saved && VALID_SORT_MODES.includes(saved as AccountsSortMode)
    ? (saved as AccountsSortMode)
    : null;
}

export function saveAccountsSortMode(mode: AccountsSortMode): void {
  writeLs(STORAGE_KEYS.accountsSortMode, mode);
  schedulePatchUiState({ accounts_sort_mode: mode });
}

export function readStoredAccountsViewMode(): AccountsViewMode | null {
  const saved = readLs(STORAGE_KEYS.accountsViewMode);
  return saved && VALID_VIEW_MODES.includes(saved as AccountsViewMode)
    ? (saved as AccountsViewMode)
    : null;
}

export function saveAccountsViewMode(mode: AccountsViewMode): void {
  writeLs(STORAGE_KEYS.accountsViewMode, mode);
  schedulePatchUiState({ accounts_view_mode: mode });
}

export function saveUiZoom(zoom: number): void {
  writeLs(STORAGE_KEYS.uiZoom, String(zoom));
  schedulePatchUiState({ ui_zoom: zoom });
}

export interface HydratedUiState {
  lastTab: TabId | null;
  dashboardHost: HostTarget | null;
  dashboardPickStrategy: DashboardPickStrategy | null;
  dashboardPoolPreference: DashboardPoolPreference | null;
  accountsSortMode: AccountsSortMode | null;
  accountsViewMode: AccountsViewMode | null;
  uiZoom: number | null;
}

let hydratePromise: Promise<HydratedUiState | null> | null = null;

/**
 * 从 ~/.antigravity_tools/config.json 水合持久化的 UI 记忆状态，并回填缺失的 localStorage 项
 */
export function hydrateUiStateFromConfig(): Promise<HydratedUiState | null> {
  if (hydratePromise) return hydratePromise;
  hydratePromise = api
    .getAppConfig()
    .then((cfg) => {
      if (!cfg) return null;
      const ui = cfg.ui_state || {};

      const lastTab =
        ui.last_tab && VALID_TABS.includes(ui.last_tab as TabId)
          ? (ui.last_tab as TabId)
          : null;
      const dashboardHost =
        ui.dashboard_host && VALID_HOSTS.includes(ui.dashboard_host as HostTarget)
          ? (ui.dashboard_host as HostTarget)
          : null;

      const rawStrategy = ui.dashboard_pick_strategy || cfg.auto_switch?.strategy;
      const dashboardPickStrategy =
        rawStrategy && VALID_PICK_STRATEGIES.includes(rawStrategy as DashboardPickStrategy)
          ? (rawStrategy as DashboardPickStrategy)
          : null;

      const rawPool = ui.dashboard_pool_preference || cfg.auto_switch?.pool;
      const dashboardPoolPreference =
        rawPool && VALID_POOL_PREFS.includes(rawPool as DashboardPoolPreference)
          ? (rawPool as DashboardPoolPreference)
          : null;

      const accountsSortMode =
        ui.accounts_sort_mode && VALID_SORT_MODES.includes(ui.accounts_sort_mode as AccountsSortMode)
          ? (ui.accounts_sort_mode as AccountsSortMode)
          : null;
      const accountsViewMode =
        ui.accounts_view_mode && VALID_VIEW_MODES.includes(ui.accounts_view_mode as AccountsViewMode)
          ? (ui.accounts_view_mode as AccountsViewMode)
          : null;
      const uiZoom =
        typeof ui.ui_zoom === 'number' && Number.isFinite(ui.ui_zoom) ? ui.ui_zoom : null;

      // 仅在 localStorage 尚未有本地记录时回填，保证磁盘持久化兜底生效
      if (!readStoredLastTab() && lastTab) writeLs(STORAGE_KEYS.lastTab, lastTab);
      if (!readStoredDashboardHost() && dashboardHost) writeLs(STORAGE_KEYS.dashboardHost, dashboardHost);
      if (!readStoredDashboardPickStrategy() && dashboardPickStrategy) {
        writeLs(STORAGE_KEYS.dashboardPickStrategy, dashboardPickStrategy);
      }
      if (!readStoredDashboardPoolPreference() && dashboardPoolPreference) {
        writeLs(STORAGE_KEYS.dashboardPoolPreference, dashboardPoolPreference);
      }
      if (!readStoredAccountsSortMode() && accountsSortMode) {
        writeLs(STORAGE_KEYS.accountsSortMode, accountsSortMode);
      }
      if (!readStoredAccountsViewMode() && accountsViewMode) {
        writeLs(STORAGE_KEYS.accountsViewMode, accountsViewMode);
      }
      if (!readLs(STORAGE_KEYS.uiZoom) && uiZoom !== null) {
        writeLs(STORAGE_KEYS.uiZoom, String(uiZoom));
      }

      return {
        lastTab,
        dashboardHost,
        dashboardPickStrategy,
        dashboardPoolPreference,
        accountsSortMode,
        accountsViewMode,
        uiZoom,
      };
    })
    .catch(() => null);
  return hydratePromise;
}
