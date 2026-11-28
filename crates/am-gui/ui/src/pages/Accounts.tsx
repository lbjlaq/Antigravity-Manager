import React, { useState, useMemo, useEffect, useCallback, useRef } from 'react';
import {
  Search,
  Plus,
  RefreshCw,
  AlertCircle,
  Download,
  Upload,
  Trash2,
  Tag,
  BookCopy,
  CheckCircle2,
  ArrowUpDown,
  Fingerprint,
  MoreHorizontal,
  Check,
  X,
  ExternalLink,
  GripVertical,
  LayoutGrid,
  List,
} from 'lucide-react';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { api, HostTarget, BackendAccount, ActiveHostStatus } from '../api';
import { tUi } from '../i18n/ui';
import {
  readStoredAccountsSortMode,
  readStoredAccountsViewMode,
  saveAccountsSortMode,
  saveAccountsViewMode,
  hydrateUiStateFromConfig,
} from '../utils/uiState';
import {
  extractAccountPoolQuota,
  formatBucketReset,
  PoolWindowQuota,
} from '../utils/quota';
import {
  QuotaMatrixBar,
  QuotaWindowBadge,
  quotaPctTextClass,
} from '../components/common/QuotaMatrixBar';
import { AddAccountDialog } from '../components/accounts/AddAccountDialog';
import {
  getAccountTier,
  getValidationBlockedLabelKey,
  isAccountAbnormal,
  isLocationUnsupportedReason,
  isValidationBlockActive,
  tierSortWeight,
} from '../utils/accountStatus';

export type SortMode = 'health_default' | 'name_asc' | 'tier_desc' | 'custom';
export type ViewMode = 'table' | 'card';

export interface QuotaPeriod {
  percentage: number;
  resetText: string;
}

export interface ModelGroupQuota {
  fiveHour?: QuotaPeriod;
  sevenDay?: QuotaPeriod;
}

export interface AccountItem {
  id: string;
  email: string;
  alias: string;
  tier: 'PRO' | 'FREE' | 'ULTRA' | 'ENTERPRISE';
  tags: string[];
  activeHosts: HostTarget[];
  geminiQuota?: ModelGroupQuota;
  claudeQuota?: ModelGroupQuota;
  argonQuota?: ModelGroupQuota;
  isAbnormal?: boolean;
  isDisabled?: boolean;
  isBanned?: boolean;
  isValidationBlocked?: boolean;
  validationLabelKey?: 'status_risk_controlled' | 'status_oauth_reauth_required' | 'status_validation_required';
  isForbidden?: boolean;
  isTokenExpired?: boolean;
  statusReason?: string;
  banAppealUrl?: string;
  rawError?: string;
  refreshToken?: string;
  fingerprint?: string;
}

interface AccountCheckboxProps {
  checked: boolean;
  indeterminate?: boolean;
  onChange: () => void;
  title?: string;
}

const AccountCheckbox: React.FC<AccountCheckboxProps> = ({
  checked,
  indeterminate = false,
  onChange,
  title,
}) => {
  const active = checked || indeterminate;
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={indeterminate ? 'mixed' : checked}
      onClick={(e) => {
        e.stopPropagation();
        onChange();
      }}
      title={title}
      className={`w-3.5 h-3.5 rounded-[4px] flex items-center justify-center shrink-0 transition-all cursor-pointer outline-none focus-visible:ring-2 focus-visible:ring-blue-500/40 ${
        active
          ? 'bg-blue-600 dark:bg-blue-500 border border-blue-600 dark:border-blue-500 text-white shadow-2xs'
          : 'bg-white dark:bg-[#1a1b22] border border-slate-300 dark:border-zinc-600 hover:border-blue-400 dark:hover:border-blue-400'
      }`}
    >
      {checked ? (
        <Check className="w-2.5 h-2.5 stroke-[3]" />
      ) : indeterminate ? (
        <span className="w-2 h-[1.6px] rounded-full bg-white" />
      ) : null}
    </button>
  );
};

interface QuotaPoolDisplayProps {
  pool: 'gemini' | 'claude';
  quota?: ModelGroupQuota;
  lang: string;
}

const QuotaPoolDisplay: React.FC<QuotaPoolDisplayProps> = React.memo(({
  pool,
  quota,
  lang,
}) => {
  const isGemini = pool === 'gemini';
  const rows: PoolWindowQuota[] = [];
  if (quota?.fiveHour) {
    rows.push({
      window: '5h',
      percentage: Math.max(0, Math.min(100, quota.fiveHour.percentage)),
      resetText: quota.fiveHour.resetText || '4h 59m',
    });
  }
  if (quota?.sevenDay) {
    rows.push({
      window: '7d',
      percentage: Math.max(0, Math.min(100, quota.sevenDay.percentage)),
      resetText: quota.sevenDay.resetText || '6d 23h',
    });
  }

  return (
    <div className="flex flex-col justify-center gap-2 px-2 py-2 min-h-[48px] h-full min-w-0 overflow-hidden rounded-[10px] bg-white/95 dark:bg-[#181920] border border-slate-200/75 dark:border-white/[0.08] shadow-[0_1px_2px_rgba(15,23,42,0.03)]">
      {rows.length === 0 ? (
        <div className="flex items-center gap-1.5 min-w-0">
          <div className="w-[60px] shrink-0 flex items-center gap-1 leading-none">
            <span className="text-[11px] font-bold text-slate-800 dark:text-zinc-100">
              {isGemini ? 'Gemini' : 'Claude'}
            </span>
            <QuotaWindowBadge window="7d" className="text-[9px] px-1 py-[0.5px] opacity-40" />
          </div>
          <QuotaMatrixBar pool={pool} window="7d" percentage={0} empty className="flex-1 min-w-[36px] h-[12px]" />
          <div className="flex items-center justify-end gap-1 font-mono tabular-nums shrink-0 leading-none">
            <span className="text-[10.5px] font-bold w-[31px] text-right text-slate-400 dark:text-zinc-500">
              --
            </span>
            <span className="text-[9px] text-slate-400 dark:text-zinc-500 w-[42px] text-right">
              --
            </span>
          </div>
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
            <div key={item.window} title={tooltip} className="flex items-center gap-1.5 min-w-0">
              {/* 模型名 + 窗口高对比度胶囊层级 */}
              <div className="w-[60px] shrink-0 flex items-center gap-1 leading-none">
                <span className="text-[11px] font-bold text-slate-800 dark:text-zinc-100">
                  {isGemini ? 'Gemini' : 'Claude'}
                </span>
                <QuotaWindowBadge window={item.window} className="text-[9px] px-1 py-[0.5px]" />
              </div>

              {/* 精密 2×20 晶格微方块矩阵进度条 + 1px 官方时间节奏标尺 */}
              <QuotaMatrixBar
                pool={pool}
                window={item.window}
                percentage={item.percentage}
                className="flex-1 min-w-[36px] h-[12px]"
              />

              {/* 纯净右对齐数值与倒计时 */}
              <div className="flex items-center justify-end gap-1 font-mono tabular-nums shrink-0 leading-none">
                <span className={`text-[10.5px] font-bold w-[31px] text-right ${quotaPctTextClass(item.percentage)}`}>
                  {item.percentage}%
                </span>
                <span className="text-[9px] w-[42px] text-right whitespace-nowrap truncate text-slate-400 dark:text-zinc-500">
                  {item.resetText}
                </span>
              </div>
            </div>
          );
        })
      )}
    </div>
  );
});

interface AccountsProps {
  lang: string;
}

export const Accounts: React.FC<AccountsProps> = ({ lang }) => {
  // 1. 检索与分类状态
  const [searchQuery, setSearchQuery] = useState('');
  const [filterTab, setFilterTab] = useState<'all' | 'pro' | 'ultra' | 'enterprise' | 'free' | 'warning'>('all');
  const [selectedTag, setSelectedTag] = useState<string>('all');
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [copiedId, setCopiedId] = useState<string | null>(null);

  // 视图模式切换：列表风格 (table) vs 卡片风格 (card)，localStorage + config.json 双写记忆
  const initialStoredViewRef = useRef<ViewMode | null>(readStoredAccountsViewMode());
  const initialStoredSortRef = useRef<SortMode | null>(readStoredAccountsSortMode());
  const uiTouchedRef = useRef({ view: false, sort: false });

  const [viewMode, setViewMode] = useState<ViewMode>(
    () => initialStoredViewRef.current ?? 'table'
  );

  const handleToggleViewMode = useCallback((mode: ViewMode) => {
    uiTouchedRef.current.view = true;
    setViewMode(mode);
    saveAccountsViewMode(mode);
  }, []);

  // 排序与自由拖拽状态（双写持久化 sortMode，卡片/列表双视图通用）
  const [sortMode, setSortMode] = useState<SortMode>(
    () => initialStoredSortRef.current ?? 'health_default'
  );
  const handleChangeSortMode = useCallback((mode: SortMode) => {
    uiTouchedRef.current.sort = true;
    setSortMode(mode);
    saveAccountsSortMode(mode);
  }, []);

  useEffect(() => {
    void hydrateUiStateFromConfig().then((hydrated) => {
      if (!hydrated) return;
      if (!initialStoredViewRef.current && !uiTouchedRef.current.view && hydrated.accountsViewMode) {
        setViewMode(hydrated.accountsViewMode);
      }
      if (!initialStoredSortRef.current && !uiTouchedRef.current.sort && hydrated.accountsSortMode) {
        setSortMode(hydrated.accountsSortMode);
      }
    });
  }, []);

  const [draggedId, setDraggedId] = useState<string | null>(null);
  const [dragOverId, setDragOverId] = useState<string | null>(null);
  const [dragGhostPos, setDragGhostPos] = useState<{ x: number; y: number } | null>(null);
  const pointerDragCleanupRef = useRef<(() => void) | null>(null);
  useEffect(() => {
    return () => {
      pointerDragCleanupRef.current?.();
    };
  }, []);
  const pointerDragRef = useRef<{
    sourceId: string;
    startX: number;
    startY: number;
    active: boolean;
    currentOverId: string | null;
  } | null>(null);

  // 弹窗状态 (指纹、标签管理、JSON导入导出、添加账号/重新授权、异常详情/申诉、删除确认)
  const [activeModal, setActiveModal] = useState<{
    type: 'fingerprint' | 'tags' | 'export' | 'import' | 'oauth' | 'error' | 'deleteConfirm';
    account?: AccountItem;
  } | null>(null);
  const [tempTagInput, setTempTagInput] = useState('');
  const [importJsonInput, setImportJsonInput] = useState('');

  useEffect(() => {
    if (!activeModal || activeModal.type === 'oauth') return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setActiveModal(null);
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [activeModal]);

  // 2. 账号状态 (真实对接磁盘/Tauri后端数据)
  const [accounts, setAccounts] = useState<AccountItem[]>([]);
  const [activeHostsStatus, setActiveHostsStatus] = useState<ActiveHostStatus | null>(null);

  // 从后端加载真实账号与多宿主激活状态
  const loadAccountsData = useCallback(async () => {
    try {
      const [backendAccounts, hosts] = await Promise.all([
        api.getAccounts(),
        api.getActiveHosts(),
      ]);
      setActiveHostsStatus(hosts || null);

      if (backendAccounts && backendAccounts.length > 0) {
        const mapped: AccountItem[] = backendAccounts.map((acc: BackendAccount) => {
          // 判定当前绑定的宿主
          const activeHostsList: HostTarget[] = [];
          const matches = (targetAccount?: string | null) =>
            Boolean(targetAccount && (targetAccount === acc.id || targetAccount.toLowerCase() === acc.email.toLowerCase()));

          if (matches(hosts?.app_account)) activeHostsList.push('app');
          if (matches(hosts?.ide_account)) activeHostsList.push('ide');
          if (matches(hosts?.plugin_account)) activeHostsList.push('plugin');
          if (matches(hosts?.agy_account)) activeHostsList.push('cli');

          // 真实订阅等级判定：直接复用 Antigravity Tools getAccountTier
          const tier = getAccountTier(acc);

          // 真实异常/禁用状态判定：直接复用 Antigravity Tools isAccountAbnormal / isValidationBlockActive
          const isBanned = Boolean(acc.banned);
          const isDisabled = Boolean(acc.disabled);
          const isValidationBlocked = isValidationBlockActive(acc);
          const isAbnormal = isAccountAbnormal(acc);
          const proxyReasonLower = (acc.proxy_disabled_reason ?? '').toLowerCase();
          const isProxyForbidden = Boolean(
            acc.proxy_disabled &&
              (proxyReasonLower.startsWith('forbidden (403)') ||
                proxyReasonLower.includes('403 forbidden') ||
                proxyReasonLower.startsWith('banned:'))
          );
          const isQuotaForbidden = Boolean(acc.quota?.is_forbidden || isProxyForbidden);
          const rawReason =
            acc.banned_reason ||
            acc.validation_blocked_reason ||
            acc.disabled_reason ||
            acc.quota?.forbidden_reason ||
            (isProxyForbidden ? acc.proxy_disabled_reason : '') ||
            '';
          const rawLower = rawReason.toLowerCase();
          const isOAuthInvalid =
            rawLower.includes('invalid_grant') ||
            rawLower.includes('token expired') ||
            rawLower.includes('revoked') ||
            rawLower.includes('unauthorized_client') ||
            rawLower.includes('invalid_client');

          const isForbidden = Boolean(isBanned || isQuotaForbidden);
          const isTokenExpired = Boolean(isDisabled && isOAuthInvalid);
          const validationLabelKey = isValidationBlocked
            ? getValidationBlockedLabelKey(rawReason)
            : undefined;
          let statusReason: string | undefined = undefined;
          let banAppealUrl: string | undefined = acc.ban_appeal_url || acc.validation_url || undefined;

          if (rawReason) {
            const urlMatch = rawReason.match(/https?:\/\/[^\s"',}]+/i);
            if (urlMatch && !banAppealUrl) {
              banAppealUrl = urlMatch[0];
            }
          }

          if (isBanned) {
            statusReason = tUi(lang, 'status_banned_banner');
            if (!banAppealUrl) {
              banAppealUrl = 'https://forms.gle/hGzM9MEUv2azZsrb9';
            }
          } else if (isQuotaForbidden) {
            statusReason = isLocationUnsupportedReason(rawReason)
              ? tUi(lang, 'status_location_unsupported')
              : tUi(lang, 'status_forbidden_403');
            if (!banAppealUrl) {
              banAppealUrl = 'https://forms.gle/hGzM9MEUv2azZsrb9';
            }
          } else if (isValidationBlocked && validationLabelKey) {
            statusReason = tUi(lang, validationLabelKey);
          } else if (isDisabled) {
            statusReason = isOAuthInvalid
              ? tUi(lang, 'status_token_expired')
              : tUi(lang, 'status_disabled_banner');
          }

          // 配额解析：复用单一数据源 extractAccountPoolQuota (与托盘小窗 100% 同步)
          const parsedGemini = extractAccountPoolQuota(acc, 'gemini');
          const parsedClaude = extractAccountPoolQuota(acc, 'claude');

          const geminiQuota: ModelGroupQuota | undefined = parsedGemini.hasData
            ? {
                fiveHour: parsedGemini.fiveHour
                  ? {
                      percentage: parsedGemini.fiveHour.percentage,
                      resetText: parsedGemini.fiveHour.resetText,
                    }
                  : undefined,
                sevenDay: parsedGemini.sevenDay
                  ? {
                      percentage: parsedGemini.sevenDay.percentage,
                      resetText: parsedGemini.sevenDay.resetText,
                    }
                  : undefined,
              }
            : undefined;

          const claudeQuota: ModelGroupQuota | undefined = parsedClaude.hasData
            ? {
                fiveHour: parsedClaude.fiveHour
                  ? {
                      percentage: parsedClaude.fiveHour.percentage,
                      resetText: parsedClaude.fiveHour.resetText,
                    }
                  : undefined,
                sevenDay: parsedClaude.sevenDay
                  ? {
                      percentage: parsedClaude.sevenDay.percentage,
                      resetText: parsedClaude.sevenDay.resetText,
                    }
                  : undefined,
              }
            : undefined;

          let argonQuota: ModelGroupQuota | undefined = undefined;
          const ag = acc.quota?.quota_groups?.find((g) =>
            (g.display_name || '').toLowerCase().includes('argon')
          );
          if (ag) {
            const bw = ag.buckets?.find((b) => b.window === 'weekly');
            if (bw) {
              const pw = Math.round((bw.remaining_fraction ?? 1) * 100);
              argonQuota = {
                sevenDay: {
                  percentage: pw,
                  resetText: formatBucketReset(bw.reset_time, bw.description, pw, false),
                },
              };
            }
          }

          return {
            id: acc.id,
            email: acc.email,
            alias: acc.name && acc.name.trim() ? acc.name : acc.email.split('@')[0],
            tier,
            tags: acc.tags || [],
            activeHosts: activeHostsList,
            fingerprint:
              acc.device_profile?.dev_device_id ||
              acc.device_profile?.machine_id ||
              'fp_isolated_default',
            isAbnormal,
            isDisabled,
            isBanned,
            isValidationBlocked,
            validationLabelKey,
            isForbidden,
            isTokenExpired,
            statusReason,
            banAppealUrl,
            rawError: rawReason || undefined,
            refreshToken: acc.token?.refresh_token || undefined,
            geminiQuota,
            claudeQuota,
            argonQuota,
          };
        });

        setAccounts(mapped);
      } else if (backendAccounts) {
        setAccounts([]);
      }
    } catch (e) {
      console.warn('Accounts load fallback:', e);
    }
  }, [lang]);

  useEffect(() => {
    loadAccountsData();
    let disposed = false;
    let unlisten: (() => void) | undefined;
    if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
      import('@tauri-apps/api/event').then(async ({ listen }) => {
        const off = await listen('tray://accounts-changed', () => {
          loadAccountsData();
        });
        if (disposed) {
          off();
          return;
        }
        unlisten = off;
      });
    }
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [loadAccountsData]);

  // 所有 Tag 列表
  const allTags = useMemo(() => {
    const set = new Set<string>();
    accounts.forEach((a) => a.tags.forEach((t) => set.add(t)));
    return Array.from(set);
  }, [accounts]);

  // 统计各 Tab 数量：直接对齐 Antigravity Tools（PRO / ULTRA / ENTERPRISE / FREE / 异常；disabled 归入已禁用，不计入异常）
  const tabCounts = useMemo(() => {
    return {
      all: accounts.length,
      pro: accounts.filter((a) => a.tier === 'PRO').length,
      ultra: accounts.filter((a) => a.tier === 'ULTRA').length,
      enterprise: accounts.filter((a) => a.tier === 'ENTERPRISE').length,
      free: accounts.filter((a) => a.tier === 'FREE').length,
      warning: accounts.filter((a) => a.isAbnormal).length,
    };
  }, [accounts]);

  // 筛选与多维排序计算 (无论何种排序，不可用账号默认沉底)
  const filteredAccounts = useMemo(() => {
    const list = accounts.filter((acc) => {
      const matchText =
        acc.email.toLowerCase().includes(searchQuery.toLowerCase()) ||
        acc.tags.some((t) => t.toLowerCase().includes(searchQuery.toLowerCase()));

      if (!matchText) return false;

      // Tab 过滤
      if (filterTab === 'pro' && acc.tier !== 'PRO') return false;
      if (filterTab === 'ultra' && acc.tier !== 'ULTRA') return false;
      if (filterTab === 'enterprise' && acc.tier !== 'ENTERPRISE') return false;
      if (filterTab === 'free' && acc.tier !== 'FREE') return false;
      if (filterTab === 'warning' && !acc.isAbnormal) return false;

      // Tag 下拉过滤
      if (selectedTag === 'all') return true;
      return acc.tags.includes(selectedTag);
    });

    // 复制数组用于排序
    const sorted = [...list];

    // custom 模式：完全忠实保留用户自由拖拽后的自定义顺序，不强行重排
    if (sortMode === 'custom') {
      return sorted;
    }

    sorted.sort((a, b) => {
      // 非自定义模式下，异常/已禁用账号默认沉底排在健康账号后面
      const aUnavailable = Boolean(a.isAbnormal || a.isDisabled);
      const bUnavailable = Boolean(b.isAbnormal || b.isDisabled);
      if (aUnavailable !== bUnavailable) {
        return aUnavailable ? 1 : -1;
      }

      // 如果健康状态相同，则根据当前选中的排序模式深入比较：
      if (sortMode === 'name_asc') {
        return a.email.localeCompare(b.email);
      }
      if (sortMode === 'tier_desc') {
        const diff = tierSortWeight(b.tier) - tierSortWeight(a.tier);
        if (diff !== 0) return diff;
        return a.email.localeCompare(b.email);
      }

      // health_default: 保持既定相对自然序列
      return 0;
    });

    return sorted;
  }, [accounts, searchQuery, filterTab, selectedTag, sortMode]);

  // 持有最新列表引用，确保全局 pointerup 回调内读取到最新状态（避免闭包陈旧）
  const filteredAccountsRef = useRef(filteredAccounts);
  filteredAccountsRef.current = filteredAccounts;
  const accountsRef = useRef(accounts);
  accountsRef.current = accounts;

  // 提交重排并持久化到后端 accounts.json
  const commitReorder = useCallback(
    async (sourceId: string, targetId: string) => {
      if (!sourceId || !targetId || sourceId === targetId) return;

      const currentList = [...filteredAccountsRef.current];
      const fromIndex = currentList.findIndex((a) => a.id === sourceId);
      const toIndex = currentList.findIndex((a) => a.id === targetId);
      if (fromIndex === -1 || toIndex === -1) return;

      const [moved] = currentList.splice(fromIndex, 1);
      currentList.splice(toIndex, 0, moved);

      // 同步合并至全局 accounts 列表（保持未筛选出来的项在末尾）
      const movedIds = new Set(currentList.map((a) => a.id));
      const restAccounts = accountsRef.current.filter((a) => !movedIds.has(a.id));
      const fullNewOrder = [...currentList, ...restAccounts];

      // 拖拽后自动切换为自定义自由排序模式，以便忠实反应用户调整
      handleChangeSortMode('custom');
      setAccounts(fullNewOrder);

      try {
        const orderedIds = fullNewOrder.map((a) => a.id);
        await api.reorderAccounts(orderedIds);
        setSwitchToast({
          message: tUi(lang, 'toast_order_saved'),
          type: 'success',
        });
        setTimeout(() => setSwitchToast(null), 2200);
      } catch (err: any) {
        console.error('Failed to save account order:', err);
      }
    },
    [handleChangeSortMode, lang]
  );

  // Pointer-Events 跨平台拖拽排序引擎（绕过 macOS WKWebView 原生 HTML5 DnD 失效问题，卡片/列表双视图统一丝滑支持）
  const handlePointerDragStart = useCallback(
    (e: React.PointerEvent, id: string, fromHandle = false) => {
      if (e.button !== 0) return;
      if (!fromHandle) {
        const target = e.target as HTMLElement | null;
        if (target?.closest('button, input, a, [role="checkbox"], [role="menuitem"]')) {
          return;
        }
      } else {
        e.preventDefault();
        e.stopPropagation();
      }

      pointerDragCleanupRef.current?.();

      pointerDragRef.current = {
        sourceId: id,
        startX: e.clientX,
        startY: e.clientY,
        active: false,
        currentOverId: null,
      };

      const cleanup = () => {
        window.removeEventListener('pointermove', onPointerMove);
        window.removeEventListener('pointerup', onPointerUp);
        window.removeEventListener('pointercancel', onPointerCancel);
        window.removeEventListener('keydown', onKeyDown);
        document.body.style.userSelect = '';
        document.body.style.cursor = '';
        pointerDragRef.current = null;
        pointerDragCleanupRef.current = null;
        setDraggedId(null);
        setDragOverId(null);
        setDragGhostPos(null);
      };
      pointerDragCleanupRef.current = cleanup;

      const onPointerMove = (ev: PointerEvent) => {
        const st = pointerDragRef.current;
        if (!st) return;

        if (!st.active) {
          if (Math.hypot(ev.clientX - st.startX, ev.clientY - st.startY) < 5) {
            return;
          }
          st.active = true;
          setDraggedId(st.sourceId);
          document.body.style.userSelect = 'none';
          document.body.style.cursor = 'grabbing';
        }

        setDragGhostPos({ x: ev.clientX, y: ev.clientY });

        // 靠近视口上下边缘时自动平滑滚动
        const edgeZone = 68;
        if (ev.clientY < edgeZone) {
          window.scrollBy(0, -10);
        } else if (ev.clientY > window.innerHeight - edgeZone) {
          window.scrollBy(0, 10);
        }

        const els = document.elementsFromPoint(ev.clientX, ev.clientY);
        let hitId: string | null = null;
        for (const el of els) {
          const itemEl = (el as HTMLElement).closest?.('[data-account-item-id]') as HTMLElement | null;
          if (itemEl?.dataset.accountItemId) {
            hitId = itemEl.dataset.accountItemId;
            break;
          }
        }

        if (hitId !== st.currentOverId) {
          st.currentOverId = hitId;
          setDragOverId(hitId && hitId !== st.sourceId ? hitId : null);
        }
      };

      const onPointerUp = () => {
        const st = pointerDragRef.current;
        const shouldCommit = Boolean(st?.active && st.currentOverId && st.currentOverId !== st.sourceId);
        const src = st?.sourceId;
        const dst = st?.currentOverId;
        cleanup();
        if (shouldCommit && src && dst) {
          void commitReorder(src, dst);
        }
      };

      const onPointerCancel = () => {
        cleanup();
      };

      const onKeyDown = (ev: KeyboardEvent) => {
        if (ev.key === 'Escape') {
          cleanup();
        }
      };

      window.addEventListener('pointermove', onPointerMove);
      window.addEventListener('pointerup', onPointerUp);
      window.addEventListener('pointercancel', onPointerCancel);
      window.addEventListener('keydown', onKeyDown);
    },
    [commitReorder]
  );

  const handleToggleSelect = (id: string) => {
    setSelectedIds((prev) => (prev.includes(id) ? prev.filter((i) => i !== id) : [...prev, id]));
  };

  const handleSelectAll = () => {
    if (selectedIds.length === filteredAccounts.length) {
      setSelectedIds([]);
    } else {
      setSelectedIds(filteredAccounts.map((a) => a.id));
    }
  };

  // 切换宿主加载与通知状态
  const [switchingHost, setSwitchingHost] = useState<{ id: string; host: HostTarget } | null>(null);
  const [switchToast, setSwitchToast] = useState<{ message: string; type: 'success' | 'error' } | null>(null);

  // 确定性切换宿主 (修复反转 Bug + 级联同步真实状态 + Toast 反馈)
  const handleToggleHost = async (accountId: string, host: HostTarget) => {
    if (switchingHost) return;
    setSwitchingHost({ id: accountId, host });

    try {
      // 1. 确定性乐观更新：当前账号必然拥有此 host，其他账号必然移除此 host
      setAccounts((prev) =>
        prev.map((acc) => ({
          ...acc,
          activeHosts:
            acc.id === accountId
              ? acc.activeHosts.includes(host)
                ? acc.activeHosts
                : [...acc.activeHosts, host]
              : acc.activeHosts.filter((h) => h !== host),
        }))
      );

      // 2. 调用底层切换接口
      await api.switchAccount(accountId, host, false);

      // 3. 重新向后端拉取最新的真实宿主状态 (对齐 active_account.json 与 accounts.json)
      await loadAccountsData();

      const targetAcc = accounts.find((a) => a.id === accountId);
      const emailName = targetAcc?.email.split('@')[0] || accountId.slice(0, 8);
      const hostLabel = host.toUpperCase();

      const verStr =
        host === 'app' && activeHostsStatus?.app_version
          ? ` (v${activeHostsStatus.app_version})`
          : host === 'ide' && activeHostsStatus?.ide_version
          ? ` (v${activeHostsStatus.ide_version})`
          : '';

      const successMsg = tUi(lang, 'toast_switched_host', { host: hostLabel, version: verStr, account: emailName });

      setSwitchToast({
        message: successMsg,
        type: 'success',
      });
    } catch (e: any) {
      console.error('Failed to switch host account:', e);
      // 发生真实错误时重新同步回当前真实状态
      await loadAccountsData();
      setSwitchToast({
        message: tUi(lang, 'toast_switch_failed') + (e?.message || String(e)),
        type: 'error',
      });
    } finally {
      setSwitchingHost(null);
      setTimeout(() => setSwitchToast(null), 2500);
    }
  };

  // 复制账号 Refresh Token (RT)
  const handleCopyRefreshToken = (id: string) => {
    const target = accounts.find((a) => a.id === id);
    if (target?.refreshToken) {
      navigator.clipboard?.writeText(target.refreshToken);
      setCopiedId(id);
      setSwitchToast({
        message: tUi(lang, 'toast_copied_rt'),
        type: 'success',
      });
      setTimeout(() => setCopiedId(null), 2000);
      setTimeout(() => setSwitchToast(null), 2000);
    } else {
      setSwitchToast({
        message: tUi(lang, 'toast_no_rt'),
        type: 'error',
      });
      setTimeout(() => setSwitchToast(null), 2000);
    }
  };

  // 下载单个账号凭据 JSON 文件 (对齐主分支格式 {"accounts": [{"email": "...", "refresh_token": "..."}]})
  const handleDownloadCredentialsJson = (id: string) => {
    const target = accounts.find((a) => a.id === id);
    if (!target) return;
    try {
      const fileName = `${target.email.replace(/[@.]/g, '_')}_account.json`;
      const exportPayload = {
        accounts: [
          {
            email: target.email,
            refresh_token: target.refreshToken || '',
          },
        ],
      };
      const jsonContent = JSON.stringify(exportPayload, null, 2);
      const blob = new Blob([jsonContent], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = fileName;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
      setSwitchToast({
        message: tUi(lang, 'toast_exported_cred'),
        type: 'success',
      });
      setTimeout(() => setSwitchToast(null), 2000);
    } catch (e: any) {
      setSwitchToast({
        message: tUi(lang, 'toast_export_failed') + (e?.message || String(e)),
        type: 'error',
      });
      setTimeout(() => setSwitchToast(null), 2500);
    }
  };

  // 确认并执行删除账号 (通过内嵌自定义模态窗口，杜绝系统 window.confirm 在 Tauri 中的卡滞问题)
  const handleExecuteDelete = async (id: string) => {
    const target = accounts.find((a) => a.id === id);
    const email = target?.email || id;
    try {
      await api.deleteAccount(id);
      setAccounts((prev) => prev.filter((a) => a.id !== id));
      setSelectedIds((prev) => prev.filter((i) => i !== id));
      setActiveModal(null);
      await loadAccountsData();
      setSwitchToast({
        message: tUi(lang, 'toast_deleted_acc', { email }),
        type: 'success',
      });
    } catch (e: any) {
      console.error('Failed to delete account:', e);
      setAccounts((prev) => prev.filter((a) => a.id !== id));
      setActiveModal(null);
      await loadAccountsData();
      setSwitchToast({
        message: tUi(lang, 'toast_delete_failed') + (e?.message || String(e)),
        type: 'error',
      });
    } finally {
      setTimeout(() => setSwitchToast(null), 2500);
    }
  };

  // 手动解除 403 锁定与风控校验
  const handleClearForbidden = async (accountId: string) => {
    try {
      await api.clearAccountForbidden(accountId);
      await loadAccountsData();
      setSwitchToast({
        message: tUi(lang, 'toast_cleared_403'),
        type: 'success',
      });
      if (activeModal?.type === 'error') {
        setActiveModal(null);
      }
    } catch (e: any) {
      setSwitchToast({
        message: tUi(lang, 'toast_clear_403_failed') + (e?.message || String(e)),
        type: 'error',
      });
    } finally {
      setTimeout(() => setSwitchToast(null), 2500);
    }
  };

  // 刷新全量账号配额
  const handleRefreshAll = async () => {
    setIsRefreshing(true);
    setSwitchToast({
      message: tUi(lang, 'toast_refreshing_all'),
      type: 'success',
    });
    try {
      await api.refreshAllQuotas();
      await loadAccountsData();
      setSwitchToast({
        message: tUi(lang, 'toast_all_refreshed'),
        type: 'success',
      });
    } catch (e) {
      console.warn('Batch refresh error:', e);
    } finally {
      setTimeout(() => setIsRefreshing(false), 800);
      setTimeout(() => setSwitchToast(null), 2000);
    }
  };

  // 刷新单账号配额
  const handleRefreshOne = async (id: string) => {
    const target = accounts.find((a) => a.id === id);
    const emailName = target?.email.split('@')[0] || id.slice(0, 8);
    try {
      setSwitchToast({
        message: tUi(lang, 'toast_refreshing_acc', { account: emailName }),
        type: 'success',
      });
      await api.refreshAccountQuota(id);
      await loadAccountsData();
      setSwitchToast({
        message: tUi(lang, 'toast_acc_refreshed', { account: emailName }),
        type: 'success',
      });
    } catch (e: any) {
      console.warn('Single refresh error:', e);
      setSwitchToast({
        message: tUi(lang, 'toast_refresh_failed') + (e?.message || String(e)),
        type: 'error',
      });
    } finally {
      setTimeout(() => setSwitchToast(null), 2000);
    }
  };

  // 批量删除
  const handleBatchDelete = async () => {
    if (!confirm(tUi(lang, 'prompt_batch_delete', { count: selectedIds.length }))) {
      return;
    }
    try {
      await api.deleteAccounts(selectedIds);
      setAccounts((prev) => prev.filter((a) => !selectedIds.includes(a.id)));
      setSelectedIds([]);
      await loadAccountsData();
    } catch (e) {
      console.error('Failed to batch delete:', e);
      setAccounts((prev) => prev.filter((a) => !selectedIds.includes(a.id)));
      setSelectedIds([]);
      await loadAccountsData();
    }
  };

  // 添加标签
  const handleAddTagToAccount = async (accountId: string, tag: string) => {
    const clean = tag.trim();
    if (!clean) return;
    const target = accounts.find((a) => a.id === accountId);
    if (!target || target.tags.includes(clean)) return;

    const newTags = [...target.tags, clean];
    try {
      await api.setAccountTags(accountId, newTags);
      setAccounts((prev) =>
        prev.map((acc) => (acc.id === accountId ? { ...acc, tags: newTags } : acc))
      );
    } catch (e) {
      console.error('Failed to set tags:', e);
      setAccounts((prev) =>
        prev.map((acc) => (acc.id === accountId ? { ...acc, tags: newTags } : acc))
      );
    }
    setTempTagInput('');
  };

  // 移除标签
  const handleRemoveTagFromAccount = async (accountId: string, tag: string) => {
    const target = accounts.find((a) => a.id === accountId);
    if (!target) return;
    const newTags = target.tags.filter((t) => t !== tag);
    try {
      await api.setAccountTags(accountId, newTags);
      setAccounts((prev) =>
        prev.map((acc) => (acc.id === accountId ? { ...acc, tags: newTags } : acc))
      );
    } catch (e) {
      console.error('Failed to remove tag:', e);
      setAccounts((prev) =>
        prev.map((acc) => (acc.id === accountId ? { ...acc, tags: newTags } : acc))
      );
    }
  };

  // 批量打标签
  const handleBatchTag = async () => {
    const tag = prompt(tUi(lang, 'prompt_batch_tag'));
    if (!tag || !tag.trim()) return;
    const clean = tag.trim();

    for (const id of selectedIds) {
      const acc = accounts.find((a) => a.id === id);
      if (acc && !acc.tags.includes(clean)) {
        const nextTags = [...acc.tags, clean];
        try {
          await api.setAccountTags(id, nextTags);
        } catch {
          // ignore
        }
      }
    }
    await loadAccountsData();
  };

  // 重新生成独立设备指纹
  const handleRegenerateFingerprint = async (accountId: string) => {
    try {
      const updated = await api.regenerateDeviceProfile(accountId);
      const newFp =
        updated.device_profile?.dev_device_id ||
        updated.device_profile?.machine_id ||
        `fp_regen_${Math.random().toString(36).substring(2, 10)}`;

      setAccounts((prev) =>
        prev.map((a) => (a.id === accountId ? { ...a, fingerprint: newFp } : a))
      );
    } catch {
      const newFp = `fp_regen_${Math.random().toString(36).substring(2, 10)}`;
      setAccounts((prev) =>
        prev.map((a) => (a.id === accountId ? { ...a, fingerprint: newFp } : a))
      );
    }
    setActiveModal(null);
  };

  // 导入备份包
  const handleImportBackup = async () => {
    if (!importJsonInput.trim()) return;
    try {
      const report = await api.importAccountsBackup(importJsonInput, true);
      alert(tUi(lang, 'toast_import_report', { imported: report.imported, overwritten: report.overwritten, skipped: report.skipped }));
      setActiveModal(null);
      setImportJsonInput('');
      await loadAccountsData();
    } catch (e: any) {
      alert(tUi(lang, 'toast_import_failed') + (e.message || String(e)));
    }
  };

  // 导出全量账号备份包 (对齐主分支 antigravity_accounts_YYYY-MM-DD.json 命名与 {"accounts": [...]} 格式)
  const handleExportBackup = async () => {
    try {
      const json = await api.exportAccountsBackup();
      const fileName = `antigravity_accounts_${new Date().toISOString().split('T')[0]}.json`;
      const blob = new Blob([json], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = fileName;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
      setSwitchToast({
        message: tUi(lang, 'toast_exported_backup'),
        type: 'success',
      });
      setActiveModal(null);
    } catch (e: any) {
      alert(tUi(lang, 'toast_export_failed') + (e?.message || String(e)));
      setActiveModal(null);
    }
  };

  return (
    <div className="w-full max-w-[1400px] mx-auto px-4 py-2 flex flex-col gap-3 animate-in fade-in-50 duration-300 relative">
      {/* 悬浮状态轻量通知胶囊 (Apple Dynamic Island 风格：置于底部中央，杜绝遮挡顶部导航与操作栏) */}
      {switchToast && (
        <div
          className={`fixed bottom-6 left-1/2 -translate-x-1/2 z-50 px-4 py-2 rounded-full text-xs font-medium shadow-2xl backdrop-blur-xl flex items-center gap-2.5 animate-in fade-in slide-in-from-bottom-3 duration-250 pointer-events-none transition-all ${
            switchToast.type === 'success'
              ? 'bg-zinc-900/90 text-white dark:bg-white/95 dark:text-zinc-900 border border-white/10 dark:border-zinc-200/50 shadow-black/20'
              : 'bg-rose-600/95 text-white border border-rose-400/30 shadow-rose-950/30'
          }`}
        >
          <div
            className={`w-2 h-2 rounded-full shrink-0 ${
              switchToast.type === 'success' ? 'bg-emerald-400 dark:bg-emerald-600 animate-pulse' : 'bg-white'
            }`}
          />
          <span className="tracking-wide">{switchToast.message}</span>
        </div>
      )}

      {/* 顶部 Apple HIG 极简单行工具栏（统一 28px 控件基线，12 种语言恒定不折行） */}
      <div className="bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] rounded-xl px-3 py-2 shadow-2xs flex items-center justify-between gap-2 min-w-0">
        {/* 左侧：聚焦微扩搜索框 + 订阅分类分段胶囊 */}
        <div className="flex items-center gap-2 min-w-0 flex-1">
          <div className="relative h-7 w-36 focus-within:w-48 xl:w-44 xl:focus-within:w-56 shrink-0 transition-[width] duration-200 ease-out">
            <Search className="w-3.5 h-3.5 absolute left-2.5 top-1/2 -translate-y-1/2 text-gray-400 pointer-events-none" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder={tUi(lang, 'search_placeholder')}
              className="w-full h-7 pl-8 pr-6 text-[11px] rounded-lg bg-gray-100/85 dark:bg-[#1a1b1f] border border-transparent focus:border-blue-500/40 dark:focus:border-blue-400/40 focus:bg-white dark:focus:bg-[#14151a] text-gray-900 dark:text-zinc-100 placeholder-gray-400 dark:placeholder-zinc-500 outline-none transition-all duration-150"
            />
            {searchQuery && (
              <button
                type="button"
                onClick={() => setSearchQuery('')}
                className="absolute right-1.5 top-1/2 -translate-y-1/2 w-4 h-4 rounded-full flex items-center justify-center text-gray-400 hover:text-gray-600 dark:hover:text-zinc-200 hover:bg-gray-200/70 dark:hover:bg-zinc-700/70 transition-colors cursor-pointer"
                aria-label="Clear search"
              >
                <X className="w-2.5 h-2.5" />
              </button>
            )}
          </div>

          {/* 分类快捷分段胶囊（直接对齐 Antigravity Tools：全部 / PRO / ULTRA / ENTERPRISE / FREE / 异常，均常驻显示） */}
          <div className="h-7 flex items-center p-0.5 bg-gray-100/85 dark:bg-[#1a1b1f] rounded-lg border border-gray-200/50 dark:border-white/5 text-[11px] min-w-0 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
            <button
              onClick={() => setFilterTab('all')}
              className={`h-6 px-2 rounded-md font-medium transition-all cursor-pointer whitespace-nowrap flex items-center gap-1 ${
                filterTab === 'all'
                  ? 'bg-white dark:bg-zinc-800 text-gray-900 dark:text-zinc-100 shadow-2xs font-semibold'
                  : 'text-gray-500 hover:text-gray-900 dark:text-zinc-400 dark:hover:text-zinc-100'
              }`}
            >
              <span>{tUi(lang, 'all')}</span>
              <span className="tabular-nums opacity-55 text-[10px]">{tabCounts.all}</span>
            </button>
            <button
              onClick={() => setFilterTab('pro')}
              className={`h-6 px-2 rounded-md font-medium transition-all cursor-pointer whitespace-nowrap flex items-center gap-1 ${
                filterTab === 'pro'
                  ? 'bg-blue-600 text-white shadow-2xs font-semibold'
                  : 'text-gray-500 hover:text-blue-600 dark:text-zinc-400 dark:hover:text-blue-400'
              }`}
            >
              <span>PRO</span>
              <span className="tabular-nums opacity-75 text-[10px]">{tabCounts.pro}</span>
            </button>
            <button
              onClick={() => setFilterTab('ultra')}
              className={`h-6 px-2 rounded-md font-medium transition-all cursor-pointer whitespace-nowrap flex items-center gap-1 ${
                filterTab === 'ultra'
                  ? 'bg-gradient-to-r from-amber-500 to-orange-500 text-white shadow-2xs font-semibold'
                  : 'text-amber-600/85 hover:text-amber-700 dark:text-amber-400/85 dark:hover:text-amber-300'
              }`}
            >
              <span>ULTRA</span>
              <span className="tabular-nums opacity-75 text-[10px]">{tabCounts.ultra}</span>
            </button>
            <button
              onClick={() => setFilterTab('enterprise')}
              className={`h-6 px-2 rounded-md font-medium transition-all cursor-pointer whitespace-nowrap flex items-center gap-1 ${
                filterTab === 'enterprise'
                  ? 'bg-gradient-to-r from-emerald-600 to-teal-500 text-white shadow-2xs font-semibold'
                  : 'text-emerald-600/85 hover:text-emerald-700 dark:text-emerald-400/85 dark:hover:text-emerald-300'
              }`}
            >
              <span>ENTERPRISE</span>
              <span className="tabular-nums opacity-75 text-[10px]">{tabCounts.enterprise}</span>
            </button>
            <button
              onClick={() => setFilterTab('free')}
              className={`h-6 px-2 rounded-md font-medium transition-all cursor-pointer whitespace-nowrap flex items-center gap-1 ${
                filterTab === 'free'
                  ? 'bg-white dark:bg-zinc-800 text-gray-900 dark:text-zinc-100 shadow-2xs font-semibold'
                  : 'text-gray-500 hover:text-gray-900 dark:text-zinc-400 dark:hover:text-zinc-100'
              }`}
            >
              <span>FREE</span>
              <span className="tabular-nums opacity-55 text-[10px]">{tabCounts.free}</span>
            </button>
            <button
              onClick={() => setFilterTab('warning')}
              className={`h-6 px-2 rounded-md font-medium transition-all cursor-pointer whitespace-nowrap flex items-center gap-1 ${
                filterTab === 'warning'
                  ? 'bg-rose-500 text-white shadow-2xs font-semibold'
                  : 'text-rose-500/85 hover:text-rose-600 dark:text-rose-400/85 dark:hover:text-rose-300'
              }`}
            >
              <span>{tUi(lang, 'errors')}</span>
              <span className="tabular-nums opacity-75 text-[10px]">{tabCounts.warning}</span>
            </button>
          </div>
        </div>

        {/* 右侧：视图与筛选无边框图标组 | 刷新与更多操作 | 唯一蓝底主 CTA */}
        <div className="flex items-center gap-1 shrink-0">
          {/* 标签过滤（未筛选时纯图标，激活时淡紫胶囊显示选中标签） */}
          {allTags.length > 0 && (
            <DropdownMenu.Root>
              <DropdownMenu.Trigger asChild>
                <button
                  className={
                    selectedTag === 'all'
                      ? 'h-7 w-7 rounded-lg flex items-center justify-center text-gray-500 dark:text-zinc-400 hover:text-purple-600 dark:hover:text-purple-400 hover:bg-gray-100/90 dark:hover:bg-white/[0.06] transition-colors outline-none cursor-pointer'
                      : 'h-7 px-2 rounded-lg bg-purple-50 dark:bg-purple-500/15 text-purple-600 dark:text-purple-300 border border-purple-200/70 dark:border-purple-500/25 text-[11px] font-medium flex items-center gap-1 max-w-[96px] transition-colors outline-none cursor-pointer'
                  }
                  title={selectedTag === 'all' ? tUi(lang, 'tag') : `${tUi(lang, 'tag')}: ${selectedTag}`}
                >
                  <Tag className="w-3.5 h-3.5 shrink-0" />
                  {selectedTag !== 'all' && <span className="truncate">{selectedTag}</span>}
                </button>
              </DropdownMenu.Trigger>
              <DropdownMenu.Portal>
                <DropdownMenu.Content
                  align="end"
                  sideOffset={6}
                  className="w-40 p-1 bg-white dark:bg-[#1a1b20] border border-gray-200 dark:border-zinc-800 rounded-xl shadow-xl z-50 text-[11px] animate-in fade-in-0 zoom-in-95 duration-100"
                >
                  <DropdownMenu.Item
                    onClick={() => setSelectedTag('all')}
                    className={`px-2 py-1.5 rounded-lg flex items-center justify-between cursor-pointer outline-none ${
                      selectedTag === 'all'
                        ? 'bg-purple-50 text-purple-600 dark:bg-purple-500/10 dark:text-purple-400 font-semibold'
                        : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
                    }`}
                  >
                    <span>{tUi(lang, 'all_tags')}</span>
                    {selectedTag === 'all' && <Check className="w-3 h-3 text-purple-600" />}
                  </DropdownMenu.Item>
                  <DropdownMenu.Separator className="h-px bg-gray-100 dark:bg-zinc-800 my-1" />
                  {allTags.map((t) => (
                    <DropdownMenu.Item
                      key={t}
                      onClick={() => setSelectedTag(t)}
                      className={`px-2 py-1.5 rounded-lg flex items-center justify-between cursor-pointer outline-none ${
                        selectedTag === t
                          ? 'bg-purple-50 text-purple-600 dark:bg-purple-500/10 dark:text-purple-400 font-semibold'
                          : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
                      }`}
                    >
                      <span className="truncate">{t}</span>
                      {selectedTag === t && <Check className="w-3 h-3 text-purple-600" />}
                    </DropdownMenu.Item>
                  ))}
                </DropdownMenu.Content>
              </DropdownMenu.Portal>
            </DropdownMenu.Root>
          )}

          {/* 全局排序图标触发器（非默认排序时右上角蓝点提示，悬停显示当前排序方式） */}
          <DropdownMenu.Root>
            <DropdownMenu.Trigger asChild>
              <button
                className={`relative h-7 w-7 rounded-lg flex items-center justify-center transition-colors outline-none cursor-pointer ${
                  sortMode !== 'health_default'
                    ? 'text-blue-600 dark:text-blue-400 bg-blue-50/80 dark:bg-blue-500/10 hover:bg-blue-100/70 dark:hover:bg-blue-500/20'
                    : 'text-gray-500 dark:text-zinc-400 hover:text-gray-900 dark:hover:text-zinc-100 hover:bg-gray-100/90 dark:hover:bg-white/[0.06]'
                }`}
                title={`${
                  sortMode === 'health_default'
                    ? tUi(lang, 'sort_health')
                    : sortMode === 'name_asc'
                    ? 'A-Z'
                    : sortMode === 'tier_desc'
                    ? tUi(lang, 'sort_tier')
                    : tUi(lang, 'sort_custom')
                } · ${tUi(lang, 'acc_drag_reorder')}`}
              >
                <ArrowUpDown className="w-3.5 h-3.5" />
                {sortMode !== 'health_default' && (
                  <span className="absolute top-1 right-1 w-1.5 h-1.5 rounded-full bg-blue-500" />
                )}
              </button>
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content
                align="end"
                sideOffset={6}
                className="w-48 p-1 bg-white dark:bg-[#1a1b20] border border-gray-200 dark:border-zinc-800 rounded-xl shadow-xl z-50 text-[11px] animate-in fade-in-0 zoom-in-95 duration-100"
              >
                <DropdownMenu.Item
                  onClick={() => handleChangeSortMode('health_default')}
                  className={`px-2.5 py-1.5 rounded-lg flex items-center justify-between cursor-pointer outline-none ${
                    sortMode === 'health_default'
                      ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 font-semibold'
                      : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
                  }`}
                >
                  <div className="flex flex-col">
                    <span>{tUi(lang, 'sort_health_full')}</span>
                    <span className="text-[9.5px] opacity-60 font-normal">{tUi(lang, 'sort_health')}</span>
                  </div>
                  {sortMode === 'health_default' && <Check className="w-3.5 h-3.5 text-blue-600" />}
                </DropdownMenu.Item>

                <DropdownMenu.Item
                  onClick={() => handleChangeSortMode('tier_desc')}
                  className={`px-2.5 py-1.5 rounded-lg flex items-center justify-between cursor-pointer outline-none ${
                    sortMode === 'tier_desc'
                      ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 font-semibold'
                      : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
                  }`}
                >
                  <div className="flex flex-col">
                    <span>{tUi(lang, 'sort_tier')}</span>
                    <span className="text-[9.5px] opacity-60 font-normal">ENTERPRISE / ULTRA / PRO &gt; FREE</span>
                  </div>
                  {sortMode === 'tier_desc' && <Check className="w-3.5 h-3.5 text-blue-600" />}
                </DropdownMenu.Item>

                <DropdownMenu.Item
                  onClick={() => handleChangeSortMode('name_asc')}
                  className={`px-2.5 py-1.5 rounded-lg flex items-center justify-between cursor-pointer outline-none ${
                    sortMode === 'name_asc'
                      ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 font-semibold'
                      : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
                  }`}
                >
                  <div className="flex flex-col">
                    <span>{tUi(lang, 'sort_alpha')}</span>
                    <span className="text-[9.5px] opacity-60 font-normal">A → Z</span>
                  </div>
                  {sortMode === 'name_asc' && <Check className="w-3.5 h-3.5 text-blue-600" />}
                </DropdownMenu.Item>

                <DropdownMenu.Separator className="h-px bg-gray-100 dark:bg-zinc-800 my-1" />

                <DropdownMenu.Item
                  onClick={() => handleChangeSortMode('custom')}
                  className={`px-2.5 py-1.5 rounded-lg flex items-center justify-between cursor-pointer outline-none ${
                    sortMode === 'custom'
                      ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 font-semibold'
                      : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
                  }`}
                >
                  <div className="flex flex-col">
                    <span>{tUi(lang, 'sort_custom')}</span>
                    <span className="text-[9.5px] opacity-60 font-normal">{tUi(lang, 'acc_drag_reorder')}</span>
                  </div>
                  {sortMode === 'custom' && <Check className="w-3.5 h-3.5 text-blue-600" />}
                </DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>

          {/* 列表 / 卡片 视图切换胶囊 */}
          <div className="h-7 flex items-center p-0.5 bg-gray-100/85 dark:bg-[#1a1b1f] rounded-lg border border-gray-200/50 dark:border-white/5">
            <button
              onClick={() => handleToggleViewMode('table')}
              className={`h-6 w-6 rounded-md flex items-center justify-center transition-all cursor-pointer ${
                viewMode === 'table'
                  ? 'bg-white dark:bg-zinc-800 text-gray-900 dark:text-zinc-100 shadow-2xs'
                  : 'text-gray-400 hover:text-gray-700 dark:hover:text-zinc-200'
              }`}
              title={tUi(lang, 'table_view')}
            >
              <List className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={() => handleToggleViewMode('card')}
              className={`h-6 w-6 rounded-md flex items-center justify-center transition-all cursor-pointer ${
                viewMode === 'card'
                  ? 'bg-white dark:bg-zinc-800 text-gray-900 dark:text-zinc-100 shadow-2xs'
                  : 'text-gray-400 hover:text-gray-700 dark:hover:text-zinc-200'
              }`}
              title={tUi(lang, 'card_view')}
            >
              <LayoutGrid className="w-3.5 h-3.5" />
            </button>
          </div>

          <div className="w-px h-4 bg-slate-200/90 dark:bg-white/10 mx-0.5 shrink-0" />

          {/* 刷新全量配额（无边框纯图标按钮） */}
          <button
            onClick={handleRefreshAll}
            disabled={isRefreshing}
            className="h-7 w-7 rounded-lg flex items-center justify-center text-gray-500 dark:text-zinc-400 hover:text-gray-900 dark:hover:text-zinc-100 hover:bg-gray-100/90 dark:hover:bg-white/[0.06] disabled:opacity-50 transition-colors cursor-pointer"
            title={tUi(lang, 'refresh_quotas')}
            aria-label={tUi(lang, 'refresh_quotas')}
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin text-blue-500' : ''}`} />
          </button>

          {/* 更多管理操作（收纳导入 / 导出低频动作） */}
          <DropdownMenu.Root>
            <DropdownMenu.Trigger asChild>
              <button
                className="h-7 w-7 rounded-lg flex items-center justify-center text-gray-500 dark:text-zinc-400 hover:text-gray-900 dark:hover:text-zinc-100 hover:bg-gray-100/90 dark:hover:bg-white/[0.06] transition-colors outline-none cursor-pointer"
                title={`${tUi(lang, 'export')} / ${tUi(lang, 'import')}`}
                aria-label={`${tUi(lang, 'export')} / ${tUi(lang, 'import')}`}
              >
                <MoreHorizontal className="w-3.5 h-3.5" />
              </button>
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content
                align="end"
                sideOffset={6}
                className="w-44 p-1 bg-white dark:bg-[#1a1b20] border border-gray-200 dark:border-zinc-800 rounded-xl shadow-xl z-50 text-[11px] animate-in fade-in-0 zoom-in-95 duration-100"
              >
                <DropdownMenu.Item
                  onClick={() => setActiveModal({ type: 'import' })}
                  className="px-2.5 py-1.5 rounded-lg flex items-center gap-2 text-gray-700 dark:text-zinc-200 hover:bg-gray-100 dark:hover:bg-zinc-800 cursor-pointer outline-none"
                >
                  <Upload className="w-3.5 h-3.5 text-gray-500 dark:text-zinc-400 shrink-0" />
                  <span className="truncate">{tUi(lang, 'import')}</span>
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  onClick={() => setActiveModal({ type: 'export' })}
                  className="px-2.5 py-1.5 rounded-lg flex items-center gap-2 text-gray-700 dark:text-zinc-200 hover:bg-gray-100 dark:hover:bg-zinc-800 cursor-pointer outline-none"
                >
                  <Download className="w-3.5 h-3.5 text-gray-500 dark:text-zinc-400 shrink-0" />
                  <span className="truncate">{tUi(lang, 'export')}</span>
                </DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>

          <div className="w-px h-4 bg-slate-200/90 dark:bg-white/10 mx-0.5 shrink-0" />

          {/* 唯一蓝底主 CTA */}
          <button
            onClick={() => setActiveModal({ type: 'oauth' })}
            className="h-7 px-3 rounded-lg bg-blue-600 hover:bg-blue-700 active:scale-[0.98] text-white text-[11px] font-semibold flex items-center gap-1.5 shadow-xs shadow-blue-500/20 transition-all cursor-pointer whitespace-nowrap shrink-0"
          >
            <Plus className="w-3.5 h-3.5 stroke-[2.25]" />
            <span>{tUi(lang, 'add_account')}</span>
          </button>
        </div>
      </div>

      {/* 视图内容区：支持列表风格 (table) 与 4 轨胶囊卡片风格 (card) */}
      {viewMode === 'table' ? (
        <div className="bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] rounded-xl overflow-hidden shadow-2xs relative w-full">
          <table className="w-full text-left border-collapse table-fixed">
            <colgroup>
              <col className="w-[50px]" />
              <col className="w-[206px]" />
              <col className="w-auto" />
              <col className="w-[176px]" />
            </colgroup>

            <thead className="bg-gray-50/80 dark:bg-[#16171b]/80 border-b border-gray-200/60 dark:border-white/5 text-gray-500 dark:text-zinc-400">
              <tr>
                <th className="pl-[30px] pr-1.5 py-2.5 align-middle">
                  <div className="flex items-center">
                    <AccountCheckbox
                      checked={selectedIds.length === filteredAccounts.length && filteredAccounts.length > 0}
                      indeterminate={selectedIds.length > 0 && selectedIds.length < filteredAccounts.length}
                      onChange={handleSelectAll}
                    />
                  </div>
                </th>
                <th className="px-2.5 py-2 font-medium text-gray-600 dark:text-zinc-300 text-[10.5px]">
                  <DropdownMenu.Root>
                    <DropdownMenu.Trigger asChild>
                      <button className="flex items-center gap-1 hover:text-blue-600 dark:hover:text-blue-400 transition-colors cursor-pointer group outline-none">
                        <span>{tUi(lang, 'account_tags')}</span>
                        <ArrowUpDown className="w-2.5 h-2.5 opacity-60 group-hover:opacity-100" />
                        <span className="text-[9px] text-gray-400 dark:text-zinc-500 font-normal">
                          ({sortMode === 'health_default' ? tUi(lang, 'sort_health') : sortMode === 'name_asc' ? 'A-Z' : sortMode === 'tier_desc' ? 'Tier' : tUi(lang, 'sort_custom')})
                        </span>
                      </button>
                    </DropdownMenu.Trigger>
                    <DropdownMenu.Portal>
                      <DropdownMenu.Content
                        align="start"
                        sideOffset={6}
                        className="w-44 p-1 bg-white dark:bg-[#1a1b20] border border-gray-200 dark:border-zinc-800 rounded-xl shadow-xl z-50 text-[11px] animate-in fade-in-0 zoom-in-95 duration-100"
                      >
                        <DropdownMenu.Item
                          onClick={() => handleChangeSortMode('health_default')}
                          className={`px-2.5 py-1.5 rounded-lg flex items-center justify-between cursor-pointer outline-none ${
                            sortMode === 'health_default'
                              ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 font-semibold'
                              : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
                          }`}
                        >
                          <div className="flex flex-col">
                            <span>{tUi(lang, 'sort_health_full')}</span>
                            <span className="text-[9.5px] opacity-60 font-normal">{tUi(lang, 'sort_health_full')}</span>
                          </div>
                          {sortMode === 'health_default' && <Check className="w-3.5 h-3.5 text-blue-600" />}
                        </DropdownMenu.Item>

                        <DropdownMenu.Item
                          onClick={() => handleChangeSortMode('tier_desc')}
                          className={`px-2.5 py-1.5 rounded-lg flex items-center justify-between cursor-pointer outline-none ${
                            sortMode === 'tier_desc'
                              ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 font-semibold'
                              : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
                          }`}
                        >
                          <div className="flex flex-col">
                            <span>{tUi(lang, 'sort_tier')}</span>
                            <span className="text-[9.5px] opacity-60 font-normal">ENTERPRISE / ULTRA / PRO &gt; FREE</span>
                          </div>
                          {sortMode === 'tier_desc' && <Check className="w-3.5 h-3.5 text-blue-600" />}
                        </DropdownMenu.Item>

                        <DropdownMenu.Item
                          onClick={() => handleChangeSortMode('name_asc')}
                          className={`px-2.5 py-1.5 rounded-lg flex items-center justify-between cursor-pointer outline-none ${
                            sortMode === 'name_asc'
                              ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 font-semibold'
                              : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
                          }`}
                        >
                          <div className="flex flex-col">
                            <span>{tUi(lang, 'sort_alpha')}</span>
                            <span className="text-[9.5px] opacity-60 font-normal">{tUi(lang, 'sort_alpha')}</span>
                          </div>
                          {sortMode === 'name_asc' && <Check className="w-3.5 h-3.5 text-blue-600" />}
                        </DropdownMenu.Item>

                        <DropdownMenu.Separator className="h-px bg-gray-100 dark:bg-zinc-800 my-1" />

                        <DropdownMenu.Item
                          onClick={() => handleChangeSortMode('custom')}
                          className={`px-2.5 py-1.5 rounded-lg flex items-center justify-between cursor-pointer outline-none ${
                            sortMode === 'custom'
                              ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 font-semibold'
                              : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
                          }`}
                        >
                          <div className="flex flex-col">
                            <span>{tUi(lang, 'sort_custom')}</span>
                            <span className="text-[9.5px] opacity-60 font-normal">{tUi(lang, 'sort_custom')}</span>
                          </div>
                          {sortMode === 'custom' && <Check className="w-3.5 h-3.5 text-blue-600" />}
                        </DropdownMenu.Item>
                      </DropdownMenu.Content>
                    </DropdownMenu.Portal>
                  </DropdownMenu.Root>
                </th>
                <th className="px-2.5 py-2 font-medium text-gray-600 dark:text-zinc-300 text-[10.5px]">
                  <div className="flex items-center justify-between">
                    <span>{tUi(lang, 'quota_pools')}</span>
                    <span className="text-[9px] text-gray-400 font-normal">{tUi(lang, 'quota_hint_stream_reservoir')}</span>
                  </div>
                </th>
                <th className="px-2 py-2 font-medium text-gray-600 dark:text-zinc-300 text-[10.5px] text-center w-[164px]">
                  <span>{tUi(lang, 'control_actions')}</span>
                </th>
              </tr>
            </thead>

            <tbody className="divide-y divide-gray-100 dark:divide-white/5 font-sans">
              {filteredAccounts.map((acc) => {
                const isSelected = selectedIds.includes(acc.id);
                const isDragged = draggedId === acc.id;
                const isDragOver = dragOverId === acc.id;
                const isUltra = acc.tier === 'ULTRA' || acc.tier === 'ENTERPRISE';
                const isPro = acc.tier === 'PRO';

                const tierRowClass = isSelected
                  ? 'bg-blue-50/75 dark:bg-blue-500/15'
                  : isUltra
                  ? 'bg-amber-50/65 hover:bg-amber-100/55 dark:bg-amber-500/[0.06] dark:hover:bg-amber-500/[0.10]'
                  : isPro
                  ? 'bg-sky-50/65 hover:bg-sky-100/55 dark:bg-sky-500/[0.06] dark:hover:bg-sky-500/[0.10]'
                  : 'hover:bg-gray-50/90 dark:hover:bg-white/[0.03]';

                return (
                  <tr
                    key={acc.id}
                    data-account-item-id={acc.id}
                    onPointerDown={(e) => handlePointerDragStart(e, acc.id, false)}
                    className={`transition-colors group select-none ${tierRowClass} ${
                      isDragged ? 'opacity-40 bg-blue-50 dark:bg-blue-900/20' : ''
                    } ${isDragOver ? 'border-t-2 border-t-blue-500 dark:border-t-blue-400 bg-blue-50/40 dark:bg-blue-500/10' : ''}`}
                    style={{
                      userSelect: 'none',
                      WebkitUserSelect: 'none',
                    } as React.CSSProperties}
                  >
                    {/* 自由拖动手柄 + 垂直对齐表头的圆角复选框 */}
                    <td className="pl-2.5 pr-1.5 py-2.5 align-middle">
                      <div className="flex items-center gap-1.5">
                        <div
                          onPointerDown={(e) => handlePointerDragStart(e, acc.id, true)}
                          className="cursor-grab active:cursor-grabbing text-gray-300 opacity-50 group-hover:opacity-100 hover:text-gray-600 dark:text-zinc-600 dark:hover:text-zinc-300 transition-all shrink-0 touch-none"
                          title={tUi(lang, 'acc_drag_reorder')}
                        >
                          <GripVertical className="w-3.5 h-3.5" />
                        </div>
                        <AccountCheckbox
                          checked={isSelected}
                          onChange={() => handleToggleSelect(acc.id)}
                        />
                      </div>
                    </td>

                    {/* 栏 1：账号与标签 */}
                    <td className="px-2.5 py-2 align-middle">
                      <div className="flex flex-col gap-1 min-w-0 pr-1">
                        <div className="flex items-center gap-1.5 min-w-0">
                          <span
                            className="font-semibold truncate text-[11px] text-gray-900 dark:text-zinc-100"
                            title={acc.email}
                          >
                            {acc.email}
                          </span>
                          <span
                            className={`px-1.5 py-0 text-[8.5px] font-extrabold tracking-wide rounded shrink-0 ${
                              acc.tier === 'PRO'
                                ? 'bg-gradient-to-r from-blue-600 to-indigo-500 text-white shadow-xs ring-1 ring-blue-400/25'
                                : acc.tier === 'ULTRA'
                                ? 'bg-gradient-to-r from-amber-500 to-orange-500 text-white shadow-xs ring-1 ring-amber-400/30'
                                : acc.tier === 'ENTERPRISE'
                                ? 'bg-gradient-to-r from-emerald-600 to-teal-500 text-white shadow-xs ring-1 ring-emerald-400/30'
                                : 'bg-gray-200/80 text-gray-600 dark:bg-zinc-800 dark:text-zinc-400'
                            }`}
                          >
                            {acc.tier}
                          </span>

                          {/* 复制账号 Refresh Token (RT) */}
                          <button
                            onClick={() => handleCopyRefreshToken(acc.id)}
                            className="p-0.5 rounded hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-400 hover:text-blue-600 dark:hover:text-blue-400 transition-colors shrink-0 cursor-pointer ml-auto"
                            title={tUi(lang, 'acc_copy_rt')}
                          >
                            {copiedId === acc.id ? (
                              <CheckCircle2 className="w-3 h-3 text-emerald-500" />
                            ) : (
                              <BookCopy className="w-3 h-3" />
                            )}
                          </button>
                        </div>

                        {/* 第二行：应用激活状态标签 (放于此使第一行邮箱纯粹整齐) + 用户自定义标签 */}
                        <div className="flex flex-wrap items-center gap-1 min-w-0">
                          {/* 状态徽章：已禁用 / 已封禁 / 403 受限 / 需验证（与 Antigravity Tools 严格分离） */}
                          {acc.isDisabled && (
                            <span className="px-1.5 py-0 text-[8.5px] font-bold rounded bg-rose-100 dark:bg-rose-900/50 text-rose-700 dark:text-rose-300 border border-rose-200/60 dark:border-rose-800/50 shrink-0">
                              {tUi(lang, 'status_disabled_badge')}
                            </span>
                          )}
                          {acc.isBanned && (
                            <span className="px-1.5 py-0 text-[8.5px] font-bold rounded bg-red-100 dark:bg-red-900/50 text-red-700 dark:text-red-300 border border-red-200/60 dark:border-red-800/50 shrink-0">
                              {tUi(lang, 'status_banned_badge')}
                            </span>
                          )}
                          {acc.isForbidden && !acc.isBanned && (
                            <span className="px-1.5 py-0 text-[8.5px] font-bold rounded bg-red-100 dark:bg-red-900/50 text-red-600 dark:text-red-400 border border-red-200/60 dark:border-red-800/50 shrink-0">
                              {tUi(lang, 'status_forbidden_badge')}
                            </span>
                          )}
                          {acc.isValidationBlocked && !acc.isBanned && (
                            <span className="px-1.5 py-0 text-[8.5px] font-bold rounded bg-amber-100 dark:bg-amber-900/50 text-amber-700 dark:text-amber-300 border border-amber-200/60 dark:border-amber-800/50 shrink-0">
                              {tUi(lang, acc.validationLabelKey || 'status_validation_required')}
                            </span>
                          )}

                          {/* 活跃宿主指示胶囊标签 */}
                          {acc.activeHosts.map((h) => (
                            <span
                              key={h}
                              className="px-1.5 py-0 text-[8.5px] font-extrabold rounded bg-emerald-500 text-white flex items-center gap-0.5 shadow-2xs shrink-0 animate-in fade-in duration-150"
                              title={tUi(lang, 'tag_active_host_tooltip', { host: h === 'app' ? 'Antigravity' : h === 'ide' ? 'Antigravity IDE' : h === 'plugin' ? 'ACP' : 'CLI' })}
                            >
                              <span className="w-1 h-1 rounded-full bg-white animate-ping" />
                              <span>
                                {h === 'app' ? tUi(lang, 'tag_host_app') : h === 'ide' ? tUi(lang, 'tag_host_ide') : h === 'plugin' ? tUi(lang, 'tag_host_plugin') : tUi(lang, 'tag_host_cli')}
                              </span>
                            </span>
                          ))}

                          {/* 自定义标签列表 */}
                          {acc.tags.map((t) => (
                            <button
                              key={t}
                              onClick={() => setSelectedTag(t)}
                              className="px-1 py-0 text-[8.5px] font-medium rounded bg-gray-100 dark:bg-zinc-800 text-gray-600 dark:text-zinc-300 border border-gray-200/50 dark:border-white/5 hover:border-purple-400 transition-colors cursor-pointer"
                              title={tUi(lang, 'filter_by_tag', { tag: t })}
                            >
                              {t}
                            </button>
                          ))}
                          <button
                            onClick={() => setActiveModal({ type: 'tags', account: acc })}
                            className="px-0.5 py-0 text-[8.5px] rounded text-gray-400 hover:text-blue-600 hover:bg-blue-50 dark:hover:bg-blue-500/10 transition-colors cursor-pointer"
                            title={tUi(lang, 'acc_manage_tags')}
                          >
                            + {tUi(lang, 'tag')}
                          </button>
                        </div>

                        {/* 异常状态微型胶囊指示器 (仅对封禁/403/验证受限账号展示，已禁用账号已在配额栏直接呈现) */}
                        {(acc.isForbidden || acc.isValidationBlocked) && (
                          <button
                            onClick={() => setActiveModal({ type: 'error', account: acc })}
                            className={`inline-flex items-center gap-1 text-[9.5px] font-medium hover:underline max-w-[220px] truncate text-left cursor-pointer ${
                              acc.isValidationBlocked && !acc.isBanned
                                ? 'text-amber-500 hover:text-amber-600 dark:text-amber-400'
                                : 'text-rose-500 hover:text-rose-600 dark:text-rose-400'
                            }`}
                            title={tUi(lang, 'acc_view_detail_hint')}
                          >
                            <AlertCircle className="w-2.5 h-2.5 shrink-0" />
                            <span className="truncate">{acc.statusReason || tUi(lang, 'acc_status_error_fallback')}</span>
                            {acc.banAppealUrl && (
                              <ExternalLink className="w-2 h-2 shrink-0 opacity-70" />
                            )}
                          </button>
                        )}
                      </div>
                    </td>

                    {/* 栏 2：Gemini / Claude / Argon 额度条 (异常与已禁用账号独立呈现对应状态条) */}
                    <td className="px-2 py-2 align-middle overflow-hidden">
                      {acc.isAbnormal && acc.isForbidden ? (
                        <div className="flex flex-col gap-1 pr-2">
                          <div className="p-2 px-3 rounded-xl bg-rose-500/10 border border-rose-500/20 flex items-center justify-between gap-2.5 text-[11px] shadow-2xs">
                            <div className="flex items-center gap-2 text-rose-600 dark:text-rose-400 min-w-0">
                              <AlertCircle className="w-3.5 h-3.5 shrink-0" />
                              <span className="font-semibold truncate">
                                {acc.statusReason || tUi(lang, 'status_forbidden_403')}
                              </span>
                            </div>
                            <div className="flex items-center gap-1.5 shrink-0">
                              {!acc.isBanned && (
                                <button
                                  onClick={() => handleClearForbidden(acc.id)}
                                  className="px-2 py-1 rounded-lg text-[10px] font-semibold bg-white dark:bg-zinc-800 text-rose-600 dark:text-rose-400 border border-rose-300 dark:border-rose-900/40 hover:bg-rose-50 dark:hover:bg-rose-950/30 transition-colors flex items-center gap-1 cursor-pointer shadow-2xs"
                                  title={tUi(lang, 'unlock_403')}
                                >
                                  <RefreshCw className="w-2.5 h-2.5" />
                                  <span>{tUi(lang, 'unlock_403')}</span>
                                </button>
                              )}

                              {acc.banAppealUrl && (
                                <button
                                  onClick={() => api.openExternalUrl(acc.banAppealUrl!)}
                                  className="px-2.5 py-1 rounded-lg text-[10px] font-semibold bg-rose-600 text-white hover:bg-rose-700 transition-colors flex items-center gap-1 cursor-pointer shadow-2xs"
                                >
                                  <span>{tUi(lang, 'appeal')}</span>
                                  <ExternalLink className="w-2.5 h-2.5" />
                                </button>
                              )}

                              <button
                                onClick={() => setActiveModal({ type: 'error', account: acc })}
                                className="px-1.5 py-1 rounded-lg text-[10px] font-medium text-blue-600 dark:text-blue-400 hover:underline transition-colors cursor-pointer"
                              >
                                {tUi(lang, 'view_reason')}
                              </button>
                            </div>
                          </div>
                        </div>
                      ) : acc.isAbnormal && acc.isValidationBlocked ? (
                        <div className="flex flex-col gap-1 pr-2">
                          <div className="p-2 px-3 rounded-xl bg-amber-500/10 border border-amber-500/20 flex items-center justify-between gap-2.5 text-[11px] shadow-2xs">
                            <div className="flex items-center gap-2 text-amber-600 dark:text-amber-400 min-w-0">
                              <AlertCircle className="w-3.5 h-3.5 shrink-0" />
                              <span className="font-semibold truncate">
                                {acc.statusReason || tUi(lang, 'status_validation_required')}
                              </span>
                            </div>
                            <div className="flex items-center gap-1.5 shrink-0">
                              {acc.validationLabelKey === 'status_oauth_reauth_required' && (
                                <button
                                  onClick={() => setActiveModal({ type: 'oauth', account: acc })}
                                  className="px-2 py-1 rounded-lg text-[10px] font-semibold bg-white dark:bg-zinc-800 text-amber-600 dark:text-amber-400 border border-amber-300 dark:border-amber-900/40 hover:bg-amber-50 dark:hover:bg-amber-950/30 transition-colors flex items-center gap-1 cursor-pointer shadow-2xs"
                                  title={tUi(lang, 're_login')}
                                >
                                  <ExternalLink className="w-2.5 h-2.5" />
                                  <span>{tUi(lang, 're_login')}</span>
                                </button>
                              )}
                              {acc.banAppealUrl && (
                                <button
                                  onClick={() => api.openExternalUrl(acc.banAppealUrl!)}
                                  className="px-2.5 py-1 rounded-lg text-[10px] font-semibold bg-amber-600 text-white hover:bg-amber-700 transition-colors flex items-center gap-1 cursor-pointer shadow-2xs"
                                >
                                  <span>{tUi(lang, 'appeal')}</span>
                                  <ExternalLink className="w-2.5 h-2.5" />
                                </button>
                              )}
                              <button
                                onClick={() => setActiveModal({ type: 'error', account: acc })}
                                className="px-1.5 py-1 rounded-lg text-[10px] font-medium text-blue-600 dark:text-blue-400 hover:underline transition-colors cursor-pointer"
                              >
                                {tUi(lang, 'view_reason')}
                              </button>
                            </div>
                          </div>
                        </div>
                      ) : acc.isDisabled ? (
                        <div className="flex flex-col gap-1 pr-2">
                          <div className="p-2 px-3 rounded-xl bg-rose-500/8 border border-rose-500/20 flex items-center justify-center gap-3 text-[11px] shadow-2xs">
                            <div className="flex items-center gap-1.5 text-rose-600 dark:text-rose-400 min-w-0">
                              <AlertCircle className="w-3.5 h-3.5 shrink-0" />
                              <span className="font-bold truncate">
                                {tUi(lang, 'status_disabled_banner')}
                              </span>
                            </div>
                            <span className="w-px h-3 bg-rose-200 dark:bg-rose-800/50 shrink-0" />
                            <button
                              onClick={() => setActiveModal({ type: 'error', account: acc })}
                              className="text-[10.5px] font-medium text-blue-600 dark:text-blue-400 hover:underline transition-colors cursor-pointer shrink-0"
                            >
                              {tUi(lang, 'view_reason')}
                            </button>
                          </div>
                        </div>
                      ) : (
                        <div className="grid grid-cols-2 gap-2 w-full min-w-0 pr-1 items-stretch">
                          <QuotaPoolDisplay pool="gemini" quota={acc.geminiQuota} lang={lang} />
                          <QuotaPoolDisplay pool="claude" quota={acc.claudeQuota} lang={lang} />
                        </div>
                      )}
                    </td>

                    {/* 栏 3：快捷控制台 (首行纯文字多端切号 + 次行操作工具) */}
                    <td className="px-2 py-2 align-middle text-center whitespace-nowrap">
                      <div className="p-1 rounded-[10px] bg-white/95 dark:bg-[#181920] border border-slate-200/75 dark:border-white/[0.06] w-[164px] mx-auto shadow-[0_1px_2px_rgba(15,23,42,0.03)] flex flex-col gap-1 items-center justify-center">
                        {/* 第一行：4 个端纯文本切换按钮 (Apple HIG 分段选择器风格，去除突兀蓝底，高雅内敛) */}
                        <div className="grid grid-cols-4 gap-0.5 w-full bg-gray-200/60 dark:bg-black/40 p-0.5 rounded-lg border border-black/5 dark:border-white/5">
                          {[
                            { id: 'app' as HostTarget, label: tUi(lang, 'host_app') },
                            { id: 'ide' as HostTarget, label: tUi(lang, 'host_ide') },
                            { id: 'plugin' as HostTarget, label: tUi(lang, 'host_plugin') },
                            { id: 'cli' as HostTarget, label: tUi(lang, 'host_cli') },
                          ].map((host) => {
                            const isActive = acc.activeHosts.includes(host.id);
                            const isSwitching = switchingHost?.id === acc.id && switchingHost?.host === host.id;

                            return (
                              <button
                                key={host.id}
                                disabled={Boolean(switchingHost)}
                                onClick={() => handleToggleHost(acc.id, host.id)}
                                className={`relative h-[20px] rounded-[5px] text-[9.5px] flex items-center justify-center transition-all cursor-pointer select-none ${
                                  isSwitching
                                    ? 'bg-blue-500 text-white opacity-85 cursor-wait'
                                    : isActive
                                    ? 'bg-white dark:bg-[#2c2d33] text-gray-900 dark:text-zinc-50 font-bold shadow-xs shadow-black/5 ring-1 ring-black/5 dark:ring-white/10'
                                    : 'text-gray-500 dark:text-zinc-400 font-medium hover:text-gray-900 dark:hover:text-zinc-100 hover:bg-white/40 dark:hover:bg-white/5'
                                }`}
                                title={
                                  isSwitching
                                    ? tUi(lang, 'host_switching_to', { host: host.label })
                                    : isActive
                                    ? tUi(lang, 'host_bound_to', { host: host.label })
                                    : tUi(lang, 'host_switch_to', { host: host.label })
                                }
                              >
                                {isSwitching ? (
                                  <RefreshCw className="w-2.5 h-2.5 animate-spin text-white" />
                                ) : (
                                  <span className="flex items-center gap-0.5">
                                    {isActive && (
                                      <span className="w-1 h-1 rounded-full bg-emerald-500 dark:bg-emerald-400 shrink-0" />
                                    )}
                                    <span>{host.label}</span>
                                  </span>
                                )}
                              </button>
                            );
                          })}
                        </div>

                        {/* 第二行：操作工具条 (横向 5 个图标按钮并排平铺) */}
                        <div className="flex items-center justify-between w-full px-0.5 pt-0.5 border-t border-gray-200/40 dark:border-white/5">
                          <button
                            onClick={() => handleRefreshOne(acc.id)}
                            className="w-5 h-5 rounded hover:bg-white dark:hover:bg-[#202126] text-gray-500 hover:text-blue-600 dark:text-zinc-400 dark:hover:text-blue-400 transition-colors flex items-center justify-center cursor-pointer"
                            title={tUi(lang, 'refresh_quotas')}
                          >
                            <RefreshCw className="w-2.5 h-2.5" />
                          </button>

                          <button
                            onClick={() => setActiveModal({ type: 'fingerprint', account: acc })}
                            className="w-5 h-5 rounded hover:bg-white dark:hover:bg-[#202126] text-gray-500 hover:text-indigo-600 dark:text-zinc-400 dark:hover:text-indigo-400 transition-colors flex items-center justify-center cursor-pointer"
                            title={tUi(lang, 'acc_device_profile')}
                          >
                            <Fingerprint className="w-2.5 h-2.5" />
                          </button>

                          <button
                            onClick={() => setActiveModal({ type: 'tags', account: acc })}
                            className="w-5 h-5 rounded hover:bg-white dark:hover:bg-[#202126] text-gray-500 hover:text-purple-600 dark:text-zinc-400 dark:hover:text-purple-400 transition-colors flex items-center justify-center cursor-pointer"
                            title={tUi(lang, 'acc_manage_tags')}
                          >
                            <Tag className="w-2.5 h-2.5" />
                          </button>

                          {/* 下载单账号凭据 JSON 文件 */}
                          <button
                            onClick={() => handleDownloadCredentialsJson(acc.id)}
                            className="w-5 h-5 rounded hover:bg-white dark:hover:bg-[#202126] text-gray-500 hover:text-emerald-600 dark:text-zinc-400 dark:hover:text-emerald-400 transition-colors flex items-center justify-center cursor-pointer"
                            title={tUi(lang, 'acc_download_json')}
                          >
                            <Download className="w-2.5 h-2.5" />
                          </button>

                          {/* 删除账号按钮：唤起无损内嵌模态确认框 */}
                          <button
                            onClick={() => setActiveModal({ type: 'deleteConfirm', account: acc })}
                            className="w-5 h-5 rounded hover:bg-rose-50 dark:hover:bg-rose-950/30 text-rose-500 hover:text-rose-600 dark:text-rose-400 transition-colors flex items-center justify-center cursor-pointer"
                            title={tUi(lang, 'acc_delete')}
                          >
                            <Trash2 className="w-2.5 h-2.5" />
                          </button>
                        </div>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      ) : (
        /* 4 轨胶囊卡片视图 (紧凑精炼，每行固定 3 列，对齐列表快捷控制台设计) */
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3 w-full">
          {filteredAccounts.map((acc) => {
            const isSelected = selectedIds.includes(acc.id);
            const isDragged = draggedId === acc.id;
            const isDragOver = dragOverId === acc.id;
            const isUltra = acc.tier === 'ULTRA' || acc.tier === 'ENTERPRISE';
            const isPro = acc.tier === 'PRO';

            const cardTierBg = isUltra
              ? 'bg-amber-50/75 dark:bg-[#1e1a13]'
              : isPro
              ? 'bg-sky-50/75 dark:bg-[#141a24]'
              : 'bg-white/85 dark:bg-[#16171b]';

            const cardBorderClass = isSelected
              ? 'border-blue-500 ring-2 ring-blue-500/20 dark:border-blue-400'
              : isUltra
              ? 'border-amber-200/80 dark:border-amber-500/20 hover:border-amber-300 dark:hover:border-amber-500/35'
              : isPro
              ? 'border-sky-200/80 dark:border-sky-500/20 hover:border-sky-300 dark:hover:border-sky-500/35'
              : 'border-gray-200/80 dark:border-white/5 hover:border-gray-300 dark:hover:border-zinc-700';

            return (
              <div
                key={acc.id}
                data-account-item-id={acc.id}
                onPointerDown={(e) => handlePointerDragStart(e, acc.id, false)}
                className={`${cardTierBg} border rounded-xl p-3 shadow-2xs hover:shadow-md transition-all flex flex-col justify-between gap-2.5 relative overflow-hidden group select-none ${cardBorderClass} ${
                  isDragged ? 'opacity-40 scale-[0.98]' : ''
                } ${isDragOver ? 'ring-2 ring-blue-500 border-blue-500 scale-[1.01]' : ''}`}
                style={{
                  userSelect: 'none',
                  WebkitUserSelect: 'none',
                } as React.CSSProperties}
              >
                {/* 卡片头部：拖拽手柄 + 复选框 + 邮箱 + 活跃宿主指示 + 订阅等级胶囊 + 复制 RT */}
                <div className="flex flex-col gap-1.5 min-w-0">
                  <div className="flex items-center justify-between gap-1.5 min-w-0">
                    <div className="flex items-center gap-1.5 min-w-0">
                      <div
                        onPointerDown={(e) => handlePointerDragStart(e, acc.id, true)}
                        className="cursor-grab active:cursor-grabbing text-gray-300 opacity-50 group-hover:opacity-100 hover:text-gray-600 dark:text-zinc-600 dark:hover:text-zinc-300 transition-all shrink-0 touch-none"
                        title={tUi(lang, 'acc_drag_reorder')}
                      >
                        <GripVertical className="w-3.5 h-3.5" />
                      </div>
                      <AccountCheckbox
                        checked={isSelected}
                        onChange={() => handleToggleSelect(acc.id)}
                      />
                      <span
                        className="font-bold text-[11.5px] truncate text-gray-900 dark:text-zinc-100"
                        title={acc.email}
                      >
                        {acc.email}
                      </span>
                    </div>

                    <div className="flex items-center gap-1 shrink-0">
                      {/* 活跃宿主指示胶囊标签 */}
                      {acc.activeHosts.map((h) => (
                        <span
                          key={h}
                          className="px-1 py-0 text-[8px] font-extrabold rounded bg-emerald-500 text-white flex items-center gap-0.5 shadow-2xs shrink-0"
                          title={tUi(lang, 'tag_active_host_tooltip', { host: h === 'app' ? 'App' : h === 'ide' ? 'IDE' : h === 'plugin' ? 'Plug' : 'CLI' })}
                        >
                          <span className="w-1 h-1 rounded-full bg-white animate-ping" />
                          <span>{h === 'app' ? tUi(lang, 'tag_host_app_short') : h === 'ide' ? tUi(lang, 'tag_host_ide') : h === 'plugin' ? tUi(lang, 'tag_host_plugin') : tUi(lang, 'tag_host_cli')}</span>
                        </span>
                      ))}

                      <span
                        className={`px-1.5 py-0 text-[9px] font-extrabold tracking-wide rounded shadow-xs ${
                          acc.tier === 'PRO'
                            ? 'bg-gradient-to-r from-blue-600 to-indigo-500 text-white ring-1 ring-blue-400/25'
                            : acc.tier === 'ULTRA'
                            ? 'bg-gradient-to-r from-amber-500 to-orange-500 text-white ring-1 ring-amber-400/30'
                            : acc.tier === 'ENTERPRISE'
                            ? 'bg-gradient-to-r from-emerald-600 to-teal-500 text-white ring-1 ring-emerald-400/30'
                            : 'bg-gray-200/80 text-gray-700 dark:bg-zinc-800 dark:text-zinc-300'
                        }`}
                      >
                        {acc.tier}
                      </span>

                      {/* 快捷复制 RT */}
                      <button
                        onClick={() => handleCopyRefreshToken(acc.id)}
                        className="p-0.5 rounded hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-400 hover:text-blue-600 dark:hover:text-blue-400 transition-colors cursor-pointer"
                        title={tUi(lang, 'acc_copy_rt')}
                      >
                        {copiedId === acc.id ? (
                          <CheckCircle2 className="w-3 h-3 text-emerald-500" />
                        ) : (
                          <BookCopy className="w-3 h-3" />
                        )}
                      </button>
                    </div>
                  </div>

                  {/* 卡片第二行：状态徽章与自定义 Tag 标签展示区 */}
                  <div className="flex flex-wrap items-center gap-1 pl-6 min-w-0">
                    {acc.isDisabled && (
                      <span className="px-1.5 py-0 text-[8px] font-bold rounded bg-rose-100 dark:bg-rose-900/50 text-rose-700 dark:text-rose-300 border border-rose-200/60 dark:border-rose-800/50 shrink-0">
                        {tUi(lang, 'status_disabled_badge')}
                      </span>
                    )}
                    {acc.isBanned && (
                      <span className="px-1.5 py-0 text-[8px] font-bold rounded bg-red-100 dark:bg-red-900/50 text-red-700 dark:text-red-300 border border-red-200/60 dark:border-red-800/50 shrink-0">
                        {tUi(lang, 'status_banned_badge')}
                      </span>
                    )}
                    {acc.isForbidden && !acc.isBanned && (
                      <span className="px-1.5 py-0 text-[8px] font-bold rounded bg-red-100 dark:bg-red-900/50 text-red-600 dark:text-red-400 border border-red-200/60 dark:border-red-800/50 shrink-0">
                        {tUi(lang, 'status_forbidden_badge')}
                      </span>
                    )}
                    {acc.isValidationBlocked && !acc.isBanned && (
                      <span className="px-1.5 py-0 text-[8px] font-bold rounded bg-amber-100 dark:bg-amber-900/50 text-amber-700 dark:text-amber-300 border border-amber-200/60 dark:border-amber-800/50 shrink-0">
                        {tUi(lang, acc.validationLabelKey || 'status_validation_required')}
                      </span>
                    )}
                    {acc.tags.map((t) => (
                      <button
                        key={t}
                        onClick={() => setSelectedTag(t)}
                        className="px-1 py-0 text-[8px] font-medium rounded bg-gray-100 dark:bg-zinc-800 text-gray-600 dark:text-zinc-300 border border-gray-200/50 dark:border-white/5 hover:border-purple-400 transition-colors cursor-pointer"
                        title={tUi(lang, 'filter_by_tag', { tag: t })}
                      >
                        {t}
                      </button>
                    ))}
                    <button
                      onClick={() => setActiveModal({ type: 'tags', account: acc })}
                      className="px-0.5 py-0 text-[8px] rounded text-gray-400 hover:text-blue-600 hover:bg-blue-50 dark:hover:bg-blue-500/10 transition-colors cursor-pointer"
                      title={tUi(lang, 'acc_manage_tags')}
                    >
                      + {tUi(lang, 'tag')}
                    </button>
                  </div>
                </div>

                {/* 卡片中部：数码像素晶格胶囊配额池 或 异常/已禁用提示（flex-1 + grid-rows-2 确保同行单/双窗口卡片等高对称） */}
                <div className="flex-1 flex flex-col justify-center gap-1 py-0.5">
                  {acc.isAbnormal && acc.isForbidden ? (
                    <div className="p-2 rounded-xl bg-rose-500/10 border border-rose-500/20 flex flex-col gap-1.5">
                      <div className="flex items-center gap-1.5 text-rose-600 dark:text-rose-400 text-[11px] font-semibold">
                        <AlertCircle className="w-3.5 h-3.5 shrink-0" />
                        <span className="truncate">
                          {acc.statusReason || tUi(lang, 'status_forbidden_403')}
                        </span>
                      </div>
                      <div className="flex items-center justify-end gap-1.5 pt-1 border-t border-rose-500/10">
                        {!acc.isBanned && (
                          <button
                            onClick={() => handleClearForbidden(acc.id)}
                            className="px-2 py-0.5 rounded-md text-[10px] font-semibold bg-white dark:bg-zinc-800 text-rose-600 dark:text-rose-400 border border-rose-300 dark:border-rose-900/40 hover:bg-rose-50 dark:hover:bg-rose-950/30 transition-colors flex items-center gap-1 cursor-pointer shadow-2xs"
                          >
                            <RefreshCw className="w-2.5 h-2.5" />
                            <span>{tUi(lang, 'unlock_403')}</span>
                          </button>
                        )}
                        {acc.banAppealUrl && (
                          <button
                            onClick={() => api.openExternalUrl(acc.banAppealUrl!)}
                            className="px-2 py-0.5 rounded-md text-[10px] font-semibold bg-rose-600 text-white hover:bg-rose-700 transition-colors flex items-center gap-1 cursor-pointer shadow-2xs"
                          >
                            <span>{tUi(lang, 'appeal')}</span>
                            <ExternalLink className="w-2.5 h-2.5" />
                          </button>
                        )}
                        <button
                          onClick={() => setActiveModal({ type: 'error', account: acc })}
                          className="px-2 py-0.5 rounded-md text-[10px] font-medium text-blue-600 dark:text-blue-400 hover:underline cursor-pointer"
                        >
                          {tUi(lang, 'view_reason')}
                        </button>
                      </div>
                    </div>
                  ) : acc.isAbnormal && acc.isValidationBlocked ? (
                    <div className="p-2 rounded-xl bg-amber-500/10 border border-amber-500/20 flex flex-col gap-1.5">
                      <div className="flex items-center gap-1.5 text-amber-600 dark:text-amber-400 text-[11px] font-semibold">
                        <AlertCircle className="w-3.5 h-3.5 shrink-0" />
                        <span className="truncate">
                          {acc.statusReason || tUi(lang, 'status_validation_required')}
                        </span>
                      </div>
                      <div className="flex items-center justify-end gap-1.5 pt-1 border-t border-amber-500/10">
                        {acc.validationLabelKey === 'status_oauth_reauth_required' && (
                          <button
                            onClick={() => setActiveModal({ type: 'oauth', account: acc })}
                            className="px-2 py-0.5 rounded-md text-[10px] font-semibold bg-amber-600 text-white hover:bg-amber-700 transition-colors flex items-center gap-1 cursor-pointer shadow-2xs"
                          >
                            <ExternalLink className="w-2.5 h-2.5" />
                            <span>{tUi(lang, 're_login')}</span>
                          </button>
                        )}
                        <button
                          onClick={() => setActiveModal({ type: 'error', account: acc })}
                          className="px-2 py-0.5 rounded-md text-[10px] font-medium text-blue-600 dark:text-blue-400 hover:underline cursor-pointer"
                        >
                          {tUi(lang, 'view_reason')}
                        </button>
                      </div>
                    </div>
                  ) : acc.isDisabled ? (
                    <div className="p-2 rounded-xl bg-rose-500/8 border border-rose-500/20 flex items-center justify-center gap-2.5">
                      <div className="flex items-center gap-1.5 text-rose-600 dark:text-rose-400 text-[11px] font-bold">
                        <AlertCircle className="w-3.5 h-3.5 shrink-0" />
                        <span className="truncate">
                          {tUi(lang, 'status_disabled_banner')}
                        </span>
                      </div>
                      <span className="w-px h-3 bg-rose-200 dark:bg-rose-800/50 shrink-0" />
                      <button
                        onClick={() => setActiveModal({ type: 'error', account: acc })}
                        className="text-[10.5px] font-medium text-blue-600 dark:text-blue-400 hover:underline cursor-pointer shrink-0"
                      >
                        {tUi(lang, 'view_reason')}
                      </button>
                    </div>
                  ) : (
                    <div className="flex-1 grid grid-rows-2 gap-1.5">
                      <QuotaPoolDisplay pool="gemini" quota={acc.geminiQuota} lang={lang} />
                      <QuotaPoolDisplay pool="claude" quota={acc.claudeQuota} lang={lang} />
                    </div>
                  )}
                </div>

                {/* 卡片底栏：100% 结构对齐列表控制台 (首行纯文字 4 端切号分段器 + 次行操作工具条) */}
                <div className="p-1 rounded-[10px] bg-white/95 dark:bg-[#181920] border border-slate-200/75 dark:border-white/[0.06] w-full shadow-[0_1px_2px_rgba(15,23,42,0.03)] flex flex-col gap-1 items-center justify-center">
                  {/* 第一行：4 端切号分段器 (完全对齐列表样式) */}
                  <div className="grid grid-cols-4 gap-0.5 w-full bg-gray-200/60 dark:bg-black/40 p-0.5 rounded-lg border border-black/5 dark:border-white/5">
                    {[
                      { id: 'app' as HostTarget, label: tUi(lang, 'host_app') },
                      { id: 'ide' as HostTarget, label: tUi(lang, 'host_ide') },
                      { id: 'plugin' as HostTarget, label: tUi(lang, 'host_plugin') },
                      { id: 'cli' as HostTarget, label: tUi(lang, 'host_cli') },
                    ].map((host) => {
                      const isActive = acc.activeHosts.includes(host.id);
                      const isSwitching = switchingHost?.id === acc.id && switchingHost?.host === host.id;

                      return (
                        <button
                          key={host.id}
                          disabled={Boolean(switchingHost)}
                          onClick={() => handleToggleHost(acc.id, host.id)}
                          className={`relative h-[20px] rounded-[5px] text-[9.5px] flex items-center justify-center transition-all cursor-pointer select-none ${
                            isSwitching
                              ? 'bg-blue-500 text-white opacity-85 cursor-wait'
                              : isActive
                              ? 'bg-white dark:bg-[#2c2d33] text-gray-900 dark:text-zinc-50 font-bold shadow-xs shadow-black/5 ring-1 ring-black/5 dark:ring-white/10'
                              : 'text-gray-500 dark:text-zinc-400 font-medium hover:text-gray-900 dark:hover:text-zinc-100 hover:bg-white/40 dark:hover:bg-white/5'
                          }`}
                          title={
                            isSwitching
                              ? tUi(lang, 'host_switching_to', { host: host.label })
                              : isActive
                              ? tUi(lang, 'host_bound_to', { host: host.label })
                              : tUi(lang, 'host_switch_to', { host: host.label })
                          }
                        >
                          {isSwitching ? (
                            <RefreshCw className="w-2.5 h-2.5 animate-spin text-white" />
                          ) : (
                            <span className="flex items-center gap-0.5">
                              {isActive && (
                                <span className="w-1 h-1 rounded-full bg-emerald-500 dark:bg-emerald-400 shrink-0" />
                              )}
                              <span>{host.label}</span>
                            </span>
                          )}
                        </button>
                      );
                    })}
                  </div>

                  {/* 第二行：操作工具条 (横向 5 个图标按钮并排平铺，100% 对齐列表快捷控制台) */}
                  <div className="flex items-center justify-between w-full px-1 pt-0.5 border-t border-gray-200/40 dark:border-white/5">
                    <button
                      onClick={() => handleRefreshOne(acc.id)}
                      className="w-5 h-5 rounded hover:bg-white dark:hover:bg-[#202126] text-gray-500 hover:text-blue-600 dark:text-zinc-400 dark:hover:text-blue-400 transition-colors flex items-center justify-center cursor-pointer"
                      title={tUi(lang, 'refresh_quotas')}
                    >
                      <RefreshCw className="w-2.5 h-2.5" />
                    </button>

                    <button
                      onClick={() => setActiveModal({ type: 'fingerprint', account: acc })}
                      className="w-5 h-5 rounded hover:bg-white dark:hover:bg-[#202126] text-gray-500 hover:text-indigo-600 dark:text-zinc-400 dark:hover:text-indigo-400 transition-colors flex items-center justify-center cursor-pointer"
                      title={tUi(lang, 'acc_device_profile')}
                    >
                      <Fingerprint className="w-2.5 h-2.5" />
                    </button>

                    <button
                      onClick={() => setActiveModal({ type: 'tags', account: acc })}
                      className="w-5 h-5 rounded hover:bg-white dark:hover:bg-[#202126] text-gray-500 hover:text-purple-600 dark:text-zinc-400 dark:hover:text-purple-400 transition-colors flex items-center justify-center cursor-pointer"
                      title={tUi(lang, 'acc_manage_tags')}
                    >
                      <Tag className="w-2.5 h-2.5" />
                    </button>

                    <button
                      onClick={() => handleDownloadCredentialsJson(acc.id)}
                      className="w-5 h-5 rounded hover:bg-white dark:hover:bg-[#202126] text-gray-500 hover:text-emerald-600 dark:text-zinc-400 dark:hover:text-emerald-400 transition-colors flex items-center justify-center cursor-pointer"
                      title={tUi(lang, 'acc_download_json')}
                    >
                      <Download className="w-2.5 h-2.5" />
                    </button>

                    <button
                      onClick={() => setActiveModal({ type: 'deleteConfirm', account: acc })}
                      className="w-5 h-5 rounded hover:bg-rose-50 dark:hover:bg-rose-950/30 text-rose-500 hover:text-rose-600 dark:text-rose-400 transition-colors flex items-center justify-center cursor-pointer"
                      title={tUi(lang, 'acc_delete')}
                    >
                      <Trash2 className="w-2.5 h-2.5" />
                    </button>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* 底部批量操作悬浮岛 */}
      {selectedIds.length > 0 && (
        <div className="sticky bottom-4 z-30 bg-gray-900/95 dark:bg-zinc-100/95 text-white dark:text-zinc-900 backdrop-blur-2xl rounded-2xl p-3 px-6 shadow-2xl flex items-center justify-between gap-4 animate-in slide-in-from-bottom-2 duration-200">
          <div className="flex items-center gap-3 text-xs font-semibold">
            <span>{tUi(lang, 'batch_selected_count', { count: selectedIds.length })}</span>
            <button
              onClick={() => setSelectedIds([])}
              className="text-xs opacity-60 hover:opacity-100 underline ml-2 cursor-pointer"
            >
              {tUi(lang, 'batch_deselect')}
            </button>
          </div>

          <div className="flex items-center gap-2">
            <button
              onClick={handleRefreshAll}
              className="px-3 py-1.5 rounded-lg bg-white/10 hover:bg-white/20 dark:bg-black/10 dark:hover:bg-black/20 text-xs font-medium transition-colors flex items-center gap-1.5 cursor-pointer"
            >
              <RefreshCw className="w-3.5 h-3.5" />
              <span>{tUi(lang, 'batch_refresh')}</span>
            </button>
            <button
              onClick={handleBatchTag}
              className="px-3 py-1.5 rounded-lg bg-white/10 hover:bg-white/20 dark:bg-black/10 dark:hover:bg-black/20 text-xs font-medium transition-colors flex items-center gap-1.5 cursor-pointer"
            >
              <Tag className="w-3.5 h-3.5" />
              <span>{tUi(lang, 'batch_tag')}</span>
            </button>
            <button
              onClick={handleBatchDelete}
              className="px-3 py-1.5 rounded-lg bg-rose-500 hover:bg-rose-600 text-white text-xs font-medium transition-colors flex items-center gap-1.5 cursor-pointer"
            >
              <Trash2 className="w-3.5 h-3.5" />
              <span>{tUi(lang, 'batch_delete')}</span>
            </button>
          </div>
        </div>
      )}

      <AddAccountDialog
        isOpen={activeModal?.type === 'oauth'}
        onClose={() => setActiveModal(null)}
        onSuccess={loadAccountsData}
        lang={lang}
        reauthEmail={activeModal?.type === 'oauth' ? activeModal.account?.email : null}
      />

      {/* 模态浮层：设备指纹 / 标签管理 / 导出备份 / 导入备份 / 异常诊断 / 删除确认 */}
      {activeModal && activeModal.type !== 'oauth' && (
        <div
          className="fixed inset-0 z-50 bg-black/40 backdrop-blur-sm flex items-center justify-center p-4 animate-in fade-in-0 duration-150"
          onClick={() => {
            setActiveModal(null);
          }}
        >
          <div
            className="w-full max-w-md bg-white dark:bg-[#16171b] border border-gray-200 dark:border-zinc-800 rounded-2xl p-6 shadow-2xl relative"
            onClick={(e) => e.stopPropagation()}
          >
            <button
              onClick={() => {
                setActiveModal(null);
              }}
              className="absolute top-4 right-4 p-1.5 rounded-lg text-gray-400 hover:text-gray-700 dark:hover:text-zinc-200 hover:bg-gray-100 dark:hover:bg-zinc-800 transition-colors cursor-pointer"
            >
              <X className="w-4 h-4" />
            </button>

            {/* 1. 独立设备指纹模态 */}
            {activeModal.type === 'fingerprint' && activeModal.account && (
              <div className="space-y-4">
                <div className="flex items-center gap-2.5">
                  <div className="w-8 h-8 rounded-xl bg-blue-50 dark:bg-blue-500/10 text-blue-600 dark:text-blue-400 flex items-center justify-center font-bold">
                    <Fingerprint className="w-4 h-4" />
                  </div>
                  <div>
                    <h3 className="text-sm font-bold text-gray-900 dark:text-zinc-100">
                      {tUi(lang, 'acc_device_title')}
                    </h3>
                    <p className="text-xs text-gray-400">{activeModal.account.email}</p>
                  </div>
                </div>

                <div className="p-3.5 rounded-xl bg-gray-50 dark:bg-zinc-900/60 border border-gray-200/60 dark:border-zinc-800 text-xs space-y-2">
                  <div className="flex justify-between items-center text-gray-500">
                    <span>{tUi(lang, 'acc_device_hash')}</span>
                    <span className="font-mono text-gray-900 dark:text-zinc-100 font-semibold truncate max-w-[200px]" title={activeModal.account.fingerprint}>
                      {activeModal.account.fingerprint || 'DEFAULT_ISOLATED_MAC_SONOMA'}
                    </span>
                  </div>
                  <div className="flex justify-between items-center text-gray-500">
                    <span>{tUi(lang, 'acc_device_status')}</span>
                    <span className="text-emerald-600 dark:text-emerald-400 font-semibold flex items-center gap-1">
                      <span className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
                      {tUi(lang, 'acc_device_isolated_ok')}
                    </span>
                  </div>
                </div>

                <div className="flex justify-end gap-2 pt-2">
                  <button
                    onClick={() => handleRegenerateFingerprint(activeModal.account!.id)}
                    className="px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold transition-colors cursor-pointer"
                  >
                    {tUi(lang, 'acc_device_regen')}
                  </button>
                </div>
              </div>
            )}

            {/* 2. 标签管理模态 */}
            {activeModal.type === 'tags' && activeModal.account && (
              <div className="space-y-4">
                <div className="flex items-center gap-2.5">
                  <div className="w-8 h-8 rounded-xl bg-purple-50 dark:bg-purple-500/10 text-purple-600 dark:text-purple-400 flex items-center justify-center font-bold">
                    <Tag className="w-4 h-4" />
                  </div>
                  <div>
                    <h3 className="text-sm font-bold text-gray-900 dark:text-zinc-100">
                      {tUi(lang, 'acc_manage_tags')}
                    </h3>
                    <p className="text-xs text-gray-400">{activeModal.account.email}</p>
                  </div>
                </div>

                <div className="flex flex-wrap gap-1.5 min-h-[40px] p-2.5 rounded-xl border border-gray-200/60 dark:border-zinc-800 bg-gray-50/50 dark:bg-zinc-900/40">
                  {activeModal.account.tags.map((t) => (
                    <span
                      key={t}
                      className="px-2 py-1 rounded-md text-xs font-medium bg-white dark:bg-zinc-800 border border-gray-200 dark:border-zinc-700 text-gray-700 dark:text-zinc-200 flex items-center gap-1.5 shadow-xs"
                    >
                      <span>{t}</span>
                      <button
                        onClick={() => handleRemoveTagFromAccount(activeModal.account!.id, t)}
                        className="text-gray-400 hover:text-rose-500 cursor-pointer"
                      >
                        <X className="w-3 h-3" />
                      </button>
                    </span>
                  ))}
                  {activeModal.account.tags.length === 0 && (
                    <span className="text-xs text-gray-400 self-center">{tUi(lang, 'acc_tags_empty')}</span>
                  )}
                </div>

                <div className="flex items-center gap-2">
                  <input
                    type="text"
                    value={tempTagInput}
                    onChange={(e) => setTempTagInput(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') handleAddTagToAccount(activeModal.account!.id, tempTagInput);
                    }}
                    placeholder={tUi(lang, 'acc_tags_placeholder')}
                    className="flex-1 px-3 py-2 text-xs rounded-xl bg-gray-100/80 dark:bg-[#1a1b1f] border border-transparent focus:border-purple-500 text-gray-900 dark:text-zinc-100 outline-none"
                  />
                  <button
                    onClick={() => handleAddTagToAccount(activeModal.account!.id, tempTagInput)}
                    className="px-3.5 py-2 rounded-xl bg-purple-600 hover:bg-purple-700 text-white text-xs font-semibold transition-colors cursor-pointer"
                  >
                    {tUi(lang, 'acc_add_btn')}
                  </button>
                </div>
              </div>
            )}

            {/* 3. 导出备份模态 */}
            {activeModal.type === 'export' && (
              <div className="space-y-4">
                <div className="flex items-center gap-2.5">
                  <div className="w-8 h-8 rounded-xl bg-emerald-50 dark:bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 flex items-center justify-center font-bold">
                    <Download className="w-4 h-4" />
                  </div>
                  <div>
                    <h3 className="text-sm font-bold text-gray-900 dark:text-zinc-100">
                      {tUi(lang, 'acc_export_title')}
                    </h3>
                    <p className="text-xs text-gray-400">{tUi(lang, 'acc_export_ready_count', { count: accounts.length })}</p>
                  </div>
                </div>

                <div className="mini-scroll p-3 bg-gray-50 dark:bg-zinc-900 rounded-xl text-xs font-mono text-gray-600 dark:text-zinc-400 max-h-40 overflow-y-auto">
                  {JSON.stringify(accounts.map((a) => ({ id: a.id, email: a.email, alias: a.alias, tier: a.tier, tags: a.tags })), null, 2)}
                </div>

                <div className="flex justify-end gap-2 pt-2">
                  <button
                    onClick={handleExportBackup}
                    className="px-4 py-2 rounded-xl bg-emerald-600 hover:bg-emerald-700 text-white text-xs font-semibold transition-colors cursor-pointer"
                  >
                    {tUi(lang, 'acc_export_copy_json')}
                  </button>
                </div>
              </div>
            )}

            {/* 4. 导入备份模态 */}
            {activeModal.type === 'import' && (
              <div className="space-y-4">
                <div className="flex items-center gap-2.5">
                  <div className="w-8 h-8 rounded-xl bg-blue-50 dark:bg-blue-500/10 text-blue-600 dark:text-blue-400 flex items-center justify-center font-bold">
                    <Upload className="w-4 h-4" />
                  </div>
                  <div>
                    <h3 className="text-sm font-bold text-gray-900 dark:text-zinc-100">
                      {tUi(lang, 'acc_import_title')}
                    </h3>
                    <p className="text-xs text-gray-400">{tUi(lang, 'acc_import_desc')}</p>
                  </div>
                </div>

                <textarea
                  rows={6}
                  value={importJsonInput}
                  onChange={(e) => setImportJsonInput(e.target.value)}
                  placeholder={tUi(lang, 'acc_import_desc')}
                  className="w-full p-2.5 text-xs font-mono rounded-xl bg-gray-50 dark:bg-zinc-900/80 border border-gray-200 dark:border-zinc-800 text-gray-900 dark:text-zinc-100 outline-none focus:border-blue-500"
                />

                <div className="flex justify-end gap-2 pt-2">
                  <button
                    onClick={handleImportBackup}
                    className="px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold transition-colors cursor-pointer"
                  >
                    {tUi(lang, 'acc_import_start')}
                  </button>
                </div>
              </div>
            )}

            {/* 6. 异常诊断与官方申诉模态 */}
            {activeModal.type === 'error' && activeModal.account && (
              <div className="space-y-4">
                <div className="flex items-center gap-2.5">
                  <div className="w-8 h-8 rounded-xl bg-rose-50 dark:bg-rose-500/10 text-rose-600 dark:text-rose-400 flex items-center justify-center font-bold">
                    <AlertCircle className="w-4 h-4" />
                  </div>
                  <div>
                    <h3 className="text-sm font-bold text-gray-900 dark:text-zinc-100">
                      {tUi(lang, 'acc_diag_title')}
                    </h3>
                    <p className="text-xs text-gray-400">{activeModal.account.email}</p>
                  </div>
                </div>

                <div className="p-3.5 rounded-xl bg-rose-500/10 border border-rose-500/20 text-xs space-y-2.5">
                  <div className="flex items-center justify-between">
                    <span className="text-gray-500 dark:text-zinc-400">{tUi(lang, 'acc_diag_category')}</span>
                    <span className="font-semibold text-rose-600 dark:text-rose-400">
                      {activeModal.account.statusReason || tUi(lang, 'acc_status_unknown_error')}
                    </span>
                  </div>

                  {activeModal.account.banAppealUrl && (
                    <div className="p-2.5 rounded-lg bg-white/60 dark:bg-black/30 border border-rose-500/30 flex items-center justify-between gap-2">
                      <div className="min-w-0">
                        <p className="text-[11px] font-semibold text-rose-700 dark:text-rose-300">
                          {tUi(lang, 'acc_diag_appeal_title')}
                        </p>
                        <p className="text-[9.5px] text-gray-500 truncate">
                          {activeModal.account.banAppealUrl}
                        </p>
                      </div>
                      <button
                        onClick={() => api.openExternalUrl(activeModal.account!.banAppealUrl!)}
                        className="px-3 py-1 rounded-lg bg-rose-600 hover:bg-rose-700 text-white text-xs font-semibold shrink-0 transition-colors flex items-center gap-1 cursor-pointer"
                      >
                        <span>{tUi(lang, 'appeal')}</span>
                        <ExternalLink className="w-3 h-3" />
                      </button>
                    </div>
                  )}

                  {activeModal.account.rawError && (
                    <div className="space-y-1">
                      <span className="text-[10px] text-gray-400 font-mono">{tUi(lang, 'acc_diag_raw')}</span>
                      <pre className="mini-scroll p-2.5 rounded-lg bg-gray-900 text-gray-200 text-[10px] font-mono whitespace-pre-wrap break-all max-h-40 overflow-y-auto">
                        {activeModal.account.rawError}
                      </pre>
                    </div>
                  )}
                </div>

                <div className="flex justify-end gap-2 pt-1">
                  {activeModal.account.isForbidden && (
                    <button
                      onClick={() => handleClearForbidden(activeModal.account!.id)}
                      className="px-3.5 py-2 rounded-xl bg-white dark:bg-zinc-800 border border-rose-300 dark:border-rose-900/40 text-rose-600 dark:text-rose-400 hover:bg-rose-50 dark:hover:bg-rose-950/30 text-xs font-semibold transition-colors flex items-center gap-1.5 cursor-pointer shadow-2xs"
                    >
                      <RefreshCw className="w-3 h-3" />
                      <span>{tUi(lang, 'unlock_403')}</span>
                    </button>
                  )}
                  {(activeModal.account.isTokenExpired || activeModal.account.isDisabled) && (
                    <button
                      onClick={() => {
                        const target = activeModal.account;
                        setActiveModal(null);
                        if (target) {
                          setTimeout(() => setActiveModal({ type: 'oauth', account: target }), 100);
                        }
                      }}
                      className="px-3.5 py-2 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold transition-colors flex items-center gap-1.5 cursor-pointer shadow-2xs"
                    >
                      <ExternalLink className="w-3 h-3" />
                      <span>{tUi(lang, 're_login')}</span>
                    </button>
                  )}
                  <button
                    onClick={() => setActiveModal(null)}
                    className="px-4 py-2 rounded-xl bg-gray-100 hover:bg-gray-200 dark:bg-zinc-800 dark:hover:bg-zinc-700 text-gray-700 dark:text-zinc-200 text-xs font-semibold transition-colors cursor-pointer"
                  >
                    {tUi(lang, 'settings_close')}
                  </button>
                </div>
              </div>
            )}

            {/* 7. 删除确认模态 (取代系统原生 window.confirm，丝滑无阻塞) */}
            {activeModal.type === 'deleteConfirm' && activeModal.account && (
              <div className="space-y-4">
                <div className="flex items-center gap-2.5">
                  <div className="w-8 h-8 rounded-xl bg-rose-50 dark:bg-rose-500/10 text-rose-600 dark:text-rose-400 flex items-center justify-center font-bold">
                    <Trash2 className="w-4 h-4" />
                  </div>
                  <div>
                    <h3 className="text-sm font-bold text-gray-900 dark:text-zinc-100">
                      {tUi(lang, 'acc_delete_confirm_title')}
                    </h3>
                    <p className="text-xs text-rose-500 font-medium">{activeModal.account.email}</p>
                  </div>
                </div>

                <div className="p-3.5 rounded-xl bg-rose-500/10 border border-rose-500/20 text-xs text-gray-600 dark:text-zinc-300 space-y-1.5">
                  <p>
                    {tUi(lang, 'acc_delete_warning')}
                  </p>
                  <p className="text-[11px] text-gray-400">
                    {tUi(lang, 'acc_delete_confirm_tip')}
                  </p>
                </div>

                <div className="flex justify-end gap-2 pt-1">
                  <button
                    onClick={() => setActiveModal(null)}
                    className="px-4 py-2 rounded-xl bg-gray-100 hover:bg-gray-200 dark:bg-zinc-800 dark:hover:bg-zinc-700 text-gray-700 dark:text-zinc-200 text-xs font-semibold transition-colors cursor-pointer"
                  >
                    {tUi(lang, 'acc_cancel')}
                  </button>
                  <button
                    onClick={() => handleExecuteDelete(activeModal.account!.id)}
                    className="px-4 py-2 rounded-xl bg-rose-600 hover:bg-rose-700 text-white text-xs font-semibold transition-colors flex items-center gap-1.5 cursor-pointer shadow-2xs"
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                    <span>{tUi(lang, 'acc_confirm_delete')}</span>
                  </button>
                </div>
              </div>
            )}
          </div>
        </div>
      )}
      {/* 自由拖拽排序时的实时跟随浮标（跨平台 WKWebView 丝滑触觉反馈） */}
      {draggedId && dragGhostPos && (() => {
        const draggedAcc = accounts.find((a) => a.id === draggedId);
        if (!draggedAcc) return null;
        return (
          <div
            style={{
              left: dragGhostPos.x + 14,
              top: dragGhostPos.y + 14,
            }}
            className="fixed z-[9999] pointer-events-none flex items-center gap-2 px-3 py-1.5 rounded-xl bg-white/95 dark:bg-[#1a1b22]/95 backdrop-blur-md border border-blue-500/60 shadow-xl shadow-blue-500/15 text-[11.5px] font-semibold text-gray-900 dark:text-zinc-100"
          >
            <GripVertical className="w-3.5 h-3.5 text-blue-500 shrink-0" />
            <span className="max-w-[200px] truncate">{draggedAcc.email}</span>
            <span
              className={`px-1.5 py-0 text-[8.5px] font-extrabold rounded shrink-0 ${
                draggedAcc.tier === 'PRO'
                  ? 'bg-gradient-to-r from-blue-600 to-indigo-500 text-white'
                  : draggedAcc.tier === 'ULTRA'
                  ? 'bg-gradient-to-r from-amber-500 to-orange-500 text-white'
                  : draggedAcc.tier === 'ENTERPRISE'
                  ? 'bg-gradient-to-r from-emerald-600 to-teal-500 text-white'
                  : 'bg-gray-200 text-gray-700 dark:bg-zinc-800 dark:text-zinc-300'
              }`}
            >
              {draggedAcc.tier}
            </span>
          </div>
        );
      })()}
    </div>
  );
};
