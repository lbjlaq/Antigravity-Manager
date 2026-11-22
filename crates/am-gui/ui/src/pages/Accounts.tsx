import React, { useState, useMemo } from 'react';
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
  Lock,
  ArrowUpDown,
  AppWindow,
  Code2,
  Puzzle,
  Terminal,
  Fingerprint,
  ChevronDown,
  Check,
  X,
  Filter
} from 'lucide-react';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';

export type HostTarget = 'app' | 'ide' | 'plugin' | 'cli';

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
  tier: 'PRO' | 'FREE';
  tags: string[];
  activeHosts: HostTarget[];
  geminiQuota?: ModelGroupQuota;
  claudeQuota?: ModelGroupQuota;
  argonQuota?: ModelGroupQuota;
  isForbidden?: boolean;
  fingerprint?: string;
}

interface AccountsProps {
  lang: 'zh' | 'en';
}

export const Accounts: React.FC<AccountsProps> = ({ lang }) => {
  const isZh = lang === 'zh';

  // 1. 检索与分类状态
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedTag, setSelectedTag] = useState<string>('all');
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [copiedId, setCopiedId] = useState<string | null>(null);

  // 弹窗状态 (指纹、标签管理、JSON导入导出)
  const [activeModal, setActiveModal] = useState<{
    type: 'fingerprint' | 'tags' | 'export';
    account?: AccountItem;
  } | null>(null);
  const [tempTagInput, setTempTagInput] = useState('');

  // 2. 账号 Mock 数据
  const [accounts, setAccounts] = useState<AccountItem[]>([
    {
      id: '1',
      email: 'ctrler@868666.best',
      alias: '开发主力机 (Master)',
      tier: 'PRO',
      tags: ['🌟 主力', 'VIP', 'Claude池'],
      activeHosts: ['app', 'ide'],
      fingerprint: 'fp_mac_a8f93e2b_sonoma',
      geminiQuota: {
        fiveHour: { percentage: 100, resetText: '4h 54m' },
        sevenDay: { percentage: 100, resetText: '6d 23h' },
      },
      claudeQuota: {
        // 场景 A：5h 为 0% 耗尽，周额度还剩 50%
        fiveHour: { percentage: 0, resetText: '3h 15m' },
        sevenDay: { percentage: 50, resetText: '5d 13h' },
      },
      argonQuota: {
        sevenDay: { percentage: 100, resetText: '6d 18h' },
      },
    },
    {
      id: '2',
      email: 'developer.pro@gmail.com',
      alias: 'Cloud Copilot',
      tier: 'PRO',
      tags: ['🌟 主力', 'Team Pro'],
      activeHosts: ['cli'],
      fingerprint: 'fp_win_e3c109df_rtx4090',
      geminiQuota: {
        fiveHour: { percentage: 100, resetText: '3h 12m' },
        sevenDay: { percentage: 95, resetText: '4d 08h' },
      },
      claudeQuota: {
        // 场景 B：5h 为 85%，周额度为 40% (5h 比 7d 长，40% 短轨在上方)
        fiveHour: { percentage: 85, resetText: '2h 10m' },
        sevenDay: { percentage: 40, resetText: '4d 20h' },
      },
    },
    {
      id: '3',
      email: 'research.pool1@gmail.com',
      alias: '科研备用池 01',
      tier: 'FREE',
      tags: ['Claude池', '备用'],
      activeHosts: ['plugin'],
      fingerprint: 'fp_linux_89ab32ff_debian',
      geminiQuota: {
        fiveHour: { percentage: 100, resetText: '5h 00m' },
        sevenDay: { percentage: 100, resetText: '7d 00h' },
      },
      claudeQuota: {
        fiveHour: { percentage: 40, resetText: '1h 15m' },
        sevenDay: { percentage: 30, resetText: '2d 04h' },
      },
    },
    {
      id: '4',
      email: 'tester.heavy@gmail.com',
      alias: '压力测试用号',
      tier: 'FREE',
      tags: ['压力测试'],
      activeHosts: [],
      fingerprint: 'fp_sandbox_0012ea77',
      isForbidden: true,
      geminiQuota: {
        fiveHour: { percentage: 0, resetText: '已受限' },
        sevenDay: { percentage: 0, resetText: '需解封' },
      },
    },
    {
      id: '5',
      email: 'sprint.dev@corp.internal',
      alias: '敏捷研发 05',
      tier: 'PRO',
      tags: ['VIP', 'Argon池'],
      activeHosts: [],
      fingerprint: 'fp_mac_7721cc89_m3max',
      geminiQuota: {
        fiveHour: { percentage: 100, resetText: '4h 30m' },
        sevenDay: { percentage: 100, resetText: '6d 12h' },
      },
      argonQuota: {
        sevenDay: { percentage: 90, resetText: '5d 08h' },
      },
    },
  ]);

  // 所有 Tag 列表
  const allTags = useMemo(() => {
    const set = new Set<string>();
    accounts.forEach((a) => a.tags.forEach((t) => set.add(t)));
    return Array.from(set);
  }, [accounts]);

  // 筛选计算
  const filteredAccounts = accounts.filter((acc) => {
    const matchText =
      acc.email.toLowerCase().includes(searchQuery.toLowerCase()) ||
      acc.alias.toLowerCase().includes(searchQuery.toLowerCase()) ||
      acc.tags.some((t) => t.toLowerCase().includes(searchQuery.toLowerCase()));

    if (!matchText) return false;

    if (selectedTag === 'all') return true;
    if (selectedTag === '__pro') return acc.tier === 'PRO';
    if (selectedTag === '__warning') return acc.isForbidden || acc.claudeQuota?.fiveHour?.percentage === 0;
    return acc.tags.includes(selectedTag);
  });

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

  const handleToggleHost = (accountId: string, host: HostTarget) => {
    setAccounts((prev) =>
      prev.map((acc) => {
        if (acc.id === accountId) {
          const exists = acc.activeHosts.includes(host);
          return {
            ...acc,
            activeHosts: exists ? acc.activeHosts.filter((h) => h !== host) : [...acc.activeHosts, host],
          };
        }
        return {
          ...acc,
          activeHosts: acc.activeHosts.filter((h) => h !== host),
        };
      })
    );
  };

  const handleCopyCredentials = (id: string) => {
    setCopiedId(id);
    setTimeout(() => setCopiedId(null), 1500);
  };

  const handleRefreshAll = () => {
    setIsRefreshing(true);
    setTimeout(() => setIsRefreshing(false), 1200);
  };

  const handleDelete = (id: string) => {
    setAccounts((prev) => prev.filter((a) => a.id !== id));
  };

  const handleAddTagToAccount = (accountId: string, tag: string) => {
    if (!tag.trim()) return;
    setAccounts((prev) =>
      prev.map((acc) => (acc.id === accountId && !acc.tags.includes(tag.trim()) ? { ...acc, tags: [...acc.tags, tag.trim()] } : acc))
    );
    setTempTagInput('');
  };

  const handleRemoveTagFromAccount = (accountId: string, tag: string) => {
    setAccounts((prev) =>
      prev.map((acc) => (acc.id === accountId ? { ...acc, tags: acc.tags.filter((t) => t !== tag) } : acc))
    );
  };

  /**
   * Apple 级高精度“双时间智能对齐”进度条算法：
   * 1. 5h（实心圆点 ●）与 7d（空心圆环 ⊚）指标各配置明确标定。
   * 2. 当 5h 为 0%（耗尽）时：
   *    - 顶部文字呈现精致的微闪红光胶囊，标定恢复时间。
   *    - 进度槽呈现精致的红色斜向虚线斑马纹（Hatched Pattern），极富科技感且警示性强。
   * 3. 当 5h 比 7d 额度长（如 5h=85%、7d=40%）或 7d 比 5h 长时：
   *    - 核心法则【短轨浮于上层】：较短的轨放置在顶层 (z-10)，带纯色饱和度与边界微阴影；
   *    - 较长的轨作为基底铺设 (z-0)，带柔和半透对比；
   *    - 如此无论谁长谁短，0% -> 短端点 -> 长端点 -> 100% 的阶梯边界全部一目了然！
   */
  const renderQuotaBar = (name: string, key: 'gemini' | 'claude' | 'argon', quota?: ModelGroupQuota) => {
    if (!quota) return null;
    const { fiveHour, sevenDay } = quota;
    const isClaude = key === 'claude';
    const isArgon = key === 'argon';

    const p5h = fiveHour?.percentage;
    const p7d = sevenDay?.percentage;

    const has5h = p5h !== undefined;
    const has7d = p7d !== undefined;

    const is5hZero = has5h && p5h === 0;
    const is7dZero = has7d && p7d === 0;

    // 基础色彩阶 (5h 饱和色)
    const color5h = is5hZero
      ? 'bg-rose-500'
      : (p5h ?? 100) < 30
      ? 'bg-rose-500'
      : (p5h ?? 100) < 60
      ? 'bg-amber-500'
      : isClaude
      ? 'bg-purple-600 dark:bg-purple-500'
      : isArgon
      ? 'bg-amber-500'
      : 'bg-blue-600 dark:bg-blue-500';

    // 基础色彩阶 (7d 宏观色 - 略浅但清晰)
    const color7d = is7dZero
      ? 'bg-rose-400'
      : (p7d ?? 100) < 30
      ? 'bg-rose-400'
      : (p7d ?? 100) < 60
      ? 'bg-amber-400'
      : isClaude
      ? 'bg-indigo-400 dark:bg-indigo-400'
      : isArgon
      ? 'bg-orange-400 dark:bg-orange-400'
      : 'bg-cyan-500 dark:bg-cyan-400';

    // 智能层叠决策：短的在顶层（确保两者边界均清晰可见）
    const len5h = p5h ?? 0;
    const len7d = p7d ?? 0;
    const is5hShorter = len5h <= len7d;

    return (
      <div className="flex flex-col gap-1 w-full group/bar">
        {/* 顶部指标文字行与色点标定 */}
        <div className="flex items-center justify-between text-[11px] leading-tight">
          <span className="font-semibold text-gray-700 dark:text-zinc-300 flex items-center gap-1.5">
            <span
              className={`w-1.5 h-1.5 rounded-full ${
                is5hZero ? 'bg-rose-500 animate-pulse' : isClaude ? 'bg-purple-500' : isArgon ? 'bg-amber-500' : 'bg-blue-500'
              }`}
            />
            <span className="truncate">{name}</span>
          </span>

          <div className="flex items-center gap-2 text-[10px] text-gray-500 dark:text-zinc-400 font-mono">
            {/* 5h 指标 + 实心圆点 (●) */}
            {fiveHour && (
              <span
                className={`inline-flex items-center gap-1 px-1.5 py-0.5 rounded transition-all ${
                  is5hZero
                    ? 'text-rose-600 dark:text-rose-400 font-bold bg-rose-50 dark:bg-rose-950/40 border border-rose-200/60 dark:border-rose-900/40'
                    : 'bg-gray-100/70 dark:bg-white/[0.04]'
                }`}
                title={`5小时短时窗口: ${p5h}% (重置倒计时: ${fiveHour.resetText})`}
              >
                <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${is5hZero ? 'bg-rose-500 animate-ping' : color5h}`} />
                <span className="opacity-75">5h:</span>
                <span className="font-bold">{p5h}%</span>
                <span className="opacity-60 text-[9px]">({fiveHour.resetText})</span>
              </span>
            )}

            {/* 7d 指标 + 空心圆环 (⊚) */}
            {sevenDay && (
              <span
                className={`inline-flex items-center gap-1 px-1.5 py-0.5 rounded transition-all ${
                  is7dZero
                    ? 'text-rose-600 dark:text-rose-400 font-bold bg-rose-50 dark:bg-rose-950/40 border border-rose-200/60 dark:border-rose-900/40'
                    : 'bg-gray-100/70 dark:bg-white/[0.04]'
                }`}
                title={`7天宏观周期额度: ${p7d}% (重置倒计时: ${sevenDay.resetText})`}
              >
                <span className={`w-1.5 h-1.5 rounded-full shrink-0 border border-current ${color7d}`} />
                <span className="opacity-75">7d:</span>
                <span className="font-bold">{p7d}%</span>
                <span className="opacity-60 text-[9px]">({sevenDay.resetText})</span>
              </span>
            )}
          </div>
        </div>

        {/* 智能双层条带轨道 (Apple HIG 级平滑堆叠) */}
        <div className="relative w-full h-[7px] rounded-full overflow-hidden bg-gray-100 dark:bg-white/[0.08] shadow-inner">
          {/* 1. 若 5h 为 0%，展示细腻警示虚线斑马纹 */}
          {is5hZero && (
            <div
              className="absolute inset-y-0 left-0 w-full opacity-60 z-20 pointer-events-none"
              style={{
                backgroundImage:
                  'repeating-linear-gradient(-45deg, rgba(239,68,68,0.35) 0, rgba(239,68,68,0.35) 4px, transparent 4px, transparent 8px)',
              }}
              title="5小时窗口已彻底耗尽 (0%)"
            />
          )}

          {/* 2. 较长者先铺设为底层 (基底槽，宽度较大，柔和半透对比) */}
          {!is5hShorter && has5h && !is5hZero && (
            <div
              style={{ width: `${len5h}%` }}
              className={`absolute inset-y-0 left-0 rounded-full opacity-45 transition-all duration-300 ${color5h}`}
              title={`5h 额度: ${len5h}%`}
            />
          )}
          {is5hShorter && has7d && !is7dZero && (
            <div
              style={{ width: `${len7d}%` }}
              className={`absolute inset-y-0 left-0 rounded-full opacity-45 transition-all duration-300 ${color7d}`}
              title={`7d 额度: ${len7d}%`}
            />
          )}

          {/* 3. 较短者覆盖在顶层 (纯色饱和 + 锐利右边框，确保两层切点绝对清晰) */}
          {is5hShorter && has5h && !is5hZero && (
            <div
              style={{ width: `${len5h}%` }}
              className={`absolute inset-y-0 left-0 rounded-full shadow-xs transition-all duration-300 z-10 border-r border-white dark:border-[#121316] ${color5h}`}
              title={`5h 额度: ${len5h}%`}
            />
          )}
          {!is5hShorter && has7d && !is7dZero && (
            <div
              style={{ width: `${len7d}%` }}
              className={`absolute inset-y-0 left-0 rounded-full shadow-xs transition-all duration-300 z-10 border-r border-white dark:border-[#121316] ${color7d}`}
              title={`7d 额度: ${len7d}%`}
            />
          )}
        </div>
      </div>
    );
  };

  return (
    <div className="w-full max-w-7xl mx-auto px-6 py-4 flex flex-col gap-4 select-none animate-in fade-in-50 duration-300">
      
      {/* ============================================================== */}
      {/* 优化后的顶部自适应工具栏 (支持自动换行，消灭多余空白)             */}
      {/* ============================================================== */}
      <div className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-2xl p-3 px-4 shadow-xs flex flex-wrap items-center justify-between gap-3">
        {/* 左侧：搜索框 + 简约 Tag 筛选下拉框 */}
        <div className="flex flex-wrap items-center gap-2.5">
          {/* 即时搜索框 */}
          <div className="relative w-56">
            <Search className="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-gray-400" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder={isZh ? '搜索邮箱、备注或标签...' : 'Search accounts...'}
              className="w-full pl-9 pr-3 py-1.5 text-xs rounded-xl bg-gray-100/80 dark:bg-[#1a1b1f] border border-transparent focus:border-blue-500/50 dark:focus:border-blue-400/50 text-gray-900 dark:text-zinc-100 placeholder-gray-400 dark:placeholder-zinc-500 outline-none transition-all duration-150"
            />
          </div>

          {/* 优雅紧凑的 Tag 筛选下拉选择器 (Radix Dropdown) */}
          <DropdownMenu.Root>
            <DropdownMenu.Trigger asChild>
              <button className="px-3 py-1.5 rounded-xl border border-gray-200/80 dark:border-white/10 bg-white dark:bg-[#1a1b20] hover:bg-gray-50 dark:hover:bg-zinc-800 text-xs font-medium text-gray-700 dark:text-zinc-300 flex items-center gap-1.5 transition-colors shadow-xs outline-none">
                <Filter className="w-3.5 h-3.5 text-blue-500" />
                <span>
                  {selectedTag === 'all'
                    ? isZh
                      ? `全部标签 (${accounts.length})`
                      : 'All Tags'
                    : selectedTag === '__pro'
                    ? 'PRO 订阅'
                    : selectedTag === '__warning'
                    ? '⚠️ 额度告警'
                    : `🏷️ ${selectedTag}`}
                </span>
                <ChevronDown className="w-3.5 h-3.5 opacity-60 ml-0.5" />
              </button>
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content
                align="start"
                sideOffset={6}
                className="w-44 p-1 bg-white dark:bg-[#1a1b20] border border-gray-200 dark:border-zinc-800 rounded-xl shadow-xl z-50 text-xs animate-in fade-in-0 zoom-in-95 duration-100"
              >
                <DropdownMenu.Item
                  onClick={() => setSelectedTag('all')}
                  className={`px-2.5 py-1.5 rounded-lg flex items-center justify-between cursor-pointer outline-none ${
                    selectedTag === 'all'
                      ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 font-semibold'
                      : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
                  }`}
                >
                  <span>{isZh ? '全部账号' : 'All Accounts'}</span>
                  {selectedTag === 'all' && <Check className="w-3.5 h-3.5 text-blue-600" />}
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  onClick={() => setSelectedTag('__pro')}
                  className={`px-2.5 py-1.5 rounded-lg flex items-center justify-between cursor-pointer outline-none ${
                    selectedTag === '__pro'
                      ? 'bg-amber-50 text-amber-600 dark:bg-amber-500/10 dark:text-amber-400 font-semibold'
                      : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
                  }`}
                >
                  <span>PRO 订阅账号</span>
                  {selectedTag === '__pro' && <Check className="w-3.5 h-3.5 text-amber-600" />}
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  onClick={() => setSelectedTag('__warning')}
                  className={`px-2.5 py-1.5 rounded-lg flex items-center justify-between cursor-pointer outline-none ${
                    selectedTag === '__warning'
                      ? 'bg-rose-50 text-rose-600 dark:bg-rose-500/10 dark:text-rose-400 font-semibold'
                      : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
                  }`}
                >
                  <span>⚠️ 额度告警/受限</span>
                  {selectedTag === '__warning' && <Check className="w-3.5 h-3.5 text-rose-600" />}
                </DropdownMenu.Item>
                <DropdownMenu.Separator className="h-px bg-gray-100 dark:bg-zinc-800 my-1" />
                {allTags.map((t) => (
                  <DropdownMenu.Item
                    key={t}
                    onClick={() => setSelectedTag(t)}
                    className={`px-2.5 py-1.5 rounded-lg flex items-center justify-between cursor-pointer outline-none ${
                      selectedTag === t
                        ? 'bg-purple-50 text-purple-600 dark:bg-purple-500/10 dark:text-purple-400 font-semibold'
                        : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
                    }`}
                  >
                    <span>🏷️ {t}</span>
                    {selectedTag === t && <Check className="w-3.5 h-3.5 text-purple-600" />}
                  </DropdownMenu.Item>
                ))}
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>
        </div>

        {/* 右侧：紧凑工具按钮组（支持小屏自动换行，呼吸感充足） */}
        <div className="flex flex-wrap items-center gap-2">
          <button
            onClick={handleRefreshAll}
            disabled={isRefreshing}
            className="px-2.5 py-1.5 rounded-xl border border-gray-200/80 dark:border-white/10 bg-white dark:bg-[#1a1b20] hover:bg-gray-50 dark:hover:bg-zinc-800 text-xs font-medium text-gray-700 dark:text-zinc-300 flex items-center gap-1.5 transition-colors shadow-xs"
            title={isZh ? '刷新全量账号配额' : 'Refresh All'}
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin text-blue-500' : ''}`} />
            <span>{isZh ? '刷新' : 'Refresh'}</span>
          </button>

          <button
            onClick={() => setActiveModal({ type: 'export' })}
            className="px-2.5 py-1.5 rounded-xl border border-gray-200/80 dark:border-white/10 bg-white dark:bg-[#1a1b20] hover:bg-gray-50 dark:hover:bg-zinc-800 text-xs font-medium text-gray-700 dark:text-zinc-300 flex items-center gap-1.5 transition-colors shadow-xs"
            title={isZh ? '导出账号数据备份 (JSON)' : 'Export Backup'}
          >
            <Download className="w-3.5 h-3.5" />
            <span>{isZh ? '导出' : 'Export'}</span>
          </button>

          <button
            onClick={() => alert(isZh ? '支持批量导入 JSON 文件或粘贴 Token' : 'Import JSON')}
            className="px-2.5 py-1.5 rounded-xl border border-gray-200/80 dark:border-white/10 bg-white dark:bg-[#1a1b20] hover:bg-gray-50 dark:hover:bg-zinc-800 text-xs font-medium text-gray-700 dark:text-zinc-300 flex items-center gap-1.5 transition-colors shadow-xs"
            title={isZh ? '从备份导入账号' : 'Import Backup'}
          >
            <Upload className="w-3.5 h-3.5" />
            <span>{isZh ? '导入' : 'Import'}</span>
          </button>

          <button
            onClick={() => alert(isZh ? '呼起浏览器进行 Google 官方 OAuth 授权' : 'OAuth Login')}
            className="px-3.5 py-1.5 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold flex items-center gap-1.5 shadow-md shadow-blue-500/20 transition-all"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>{isZh ? '授权登录' : 'Add Account'}</span>
          </button>
        </div>
      </div>

      {/* ============================================================== */}
      {/* 核心 4 栏式黄金表格：                                            */}
      {/* [账号与标签] | [模型额度池] | [四宫格多端切号] | [操作工具]      */}
      {/* ============================================================== */}
      <div className="bg-white/80 dark:bg-[#121316]/80 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-2xl overflow-hidden shadow-xs relative">
        <div className="overflow-x-auto">
          <table className="w-full text-left text-xs border-collapse">
            <thead className="bg-gray-50/80 dark:bg-[#16171b]/80 border-b border-gray-200/60 dark:border-white/5 text-gray-500 dark:text-zinc-400 select-none">
              <tr>
                <th className="pl-3.5 py-3 w-9">
                  <input
                    type="checkbox"
                    checked={selectedIds.length === filteredAccounts.length && filteredAccounts.length > 0}
                    onChange={handleSelectAll}
                    className="rounded border-gray-300 dark:border-zinc-700 text-blue-600 focus:ring-blue-500/30 cursor-pointer"
                  />
                </th>
                {/* 栏 1：账号与标签 */}
                <th className="px-4 py-3 font-semibold text-gray-600 dark:text-zinc-300 w-[230px] whitespace-nowrap">
                  <div className="flex items-center gap-1.5">
                    <span>{isZh ? '账号 / 标签' : 'Account & Tags'}</span>
                    <ArrowUpDown className="w-3 h-3 opacity-40" />
                  </div>
                </th>
                {/* 栏 2：Gemini / Claude / Argon 统一双轨进度 */}
                <th className="px-4 py-3 font-semibold text-gray-600 dark:text-zinc-300 min-w-[340px] whitespace-nowrap">
                  <div className="flex items-center justify-between">
                    <span>{isZh ? '模型额度池 (● 5h 实时流速 / ⊚ 7d 周期水库)' : 'Model Quota Pools (5h / 7d)'}</span>
                    <span className="text-[10px] text-gray-400 font-normal">短轨浮于上层 · 耗尽显虚线</span>
                  </div>
                </th>
                {/* 栏 3：四宫格快捷切换区 (精致内嵌卡片质感) */}
                <th className="px-4 py-3 font-semibold text-gray-600 dark:text-zinc-300 w-[160px] whitespace-nowrap text-center">
                  <span>{isZh ? '四宫格多端切号' : 'Host Matrix (4-Grid)'}</span>
                </th>
                {/* 栏 4：操作工具区 (自然内嵌，无缝衔接) */}
                <th className="px-4 py-3 font-semibold text-gray-600 dark:text-zinc-300 w-[150px] text-center whitespace-nowrap">
                  {isZh ? '快捷工具' : 'Actions'}
                </th>
              </tr>
            </thead>

            <tbody className="divide-y divide-gray-100 dark:divide-white/5 font-sans">
              {filteredAccounts.map((acc) => {
                const isSelected = selectedIds.includes(acc.id);

                return (
                  <tr
                    key={acc.id}
                    className={`hover:bg-gray-50/60 dark:hover:bg-zinc-800/30 transition-colors group ${
                      isSelected
                        ? 'bg-blue-50/50 dark:bg-blue-500/5'
                        : acc.isForbidden
                        ? 'bg-rose-50/30 dark:bg-rose-950/20'
                        : ''
                    }`}
                  >
                    {/* 复选框 */}
                    <td className="pl-3.5 py-3 align-middle">
                      <input
                        type="checkbox"
                        checked={isSelected}
                        onChange={() => handleToggleSelect(acc.id)}
                        className="rounded border-gray-300 dark:border-zinc-700 text-blue-600 focus:ring-blue-500/30 cursor-pointer"
                      />
                    </td>

                    {/* 栏 1：账号与标签 */}
                    <td className="px-4 py-3 align-middle">
                      <div className="flex flex-col gap-1.5 min-w-0 pr-2">
                        {/* 邮箱 + PRO 徽章 + 复制 */}
                        <div className="flex items-center gap-1.5 min-w-0">
                          <span
                            className="font-semibold text-gray-900 dark:text-zinc-100 truncate text-xs"
                            title={acc.email}
                          >
                            {acc.email}
                          </span>
                          <span
                            className={`px-1.5 py-0.2 text-[9px] font-bold rounded shrink-0 ${
                              acc.tier === 'PRO'
                                ? 'bg-gradient-to-r from-amber-500 to-orange-500 text-white shadow-xs'
                                : 'bg-gray-100 text-gray-600 dark:bg-zinc-800 dark:text-zinc-400'
                            }`}
                          >
                            {acc.tier}
                          </span>
                          <button
                            onClick={() => handleCopyCredentials(acc.id)}
                            className="p-1 rounded hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-400 hover:text-blue-600 dark:hover:text-blue-400 transition-colors shrink-0"
                            title={isZh ? '复制账号凭据 (JSON)' : 'Copy Credentials'}
                          >
                            {copiedId === acc.id ? (
                              <CheckCircle2 className="w-3.5 h-3.5 text-emerald-500" />
                            ) : (
                              <BookCopy className="w-3.5 h-3.5" />
                            )}
                          </button>
                        </div>

                        {/* 备注名称 */}
                        <span className="text-xs text-gray-500 dark:text-zinc-400 truncate">
                          {acc.alias}
                        </span>

                        {/* 标签栏 */}
                        <div className="flex flex-wrap items-center gap-1">
                          {acc.tags.map((t) => (
                            <button
                              key={t}
                              onClick={() => setSelectedTag(t)}
                              className="px-1.5 py-0.2 text-[9px] font-medium rounded bg-gray-100 dark:bg-zinc-800 text-gray-600 dark:text-zinc-300 border border-gray-200/50 dark:border-white/5 hover:border-purple-400 transition-colors"
                              title={`过滤标签 [${t}]`}
                            >
                              {t}
                            </button>
                          ))}
                          <button
                            onClick={() => setActiveModal({ type: 'tags', account: acc })}
                            className="px-1 py-0.2 text-[9px] rounded text-gray-400 hover:text-blue-600 hover:bg-blue-50 dark:hover:bg-blue-500/10 transition-colors"
                            title={isZh ? '管理标签' : 'Manage Tags'}
                          >
                            + {isZh ? '标签' : 'Tag'}
                          </button>
                        </div>

                        {/* 403 受限提示 */}
                        {acc.isForbidden && (
                          <div className="inline-flex items-center gap-1 text-[10px] font-semibold text-rose-600 dark:text-rose-400">
                            <Lock className="w-3 h-3" />
                            <span>{isZh ? '账号受限 (403)' : 'Forbidden'}</span>
                          </div>
                        )}
                      </div>
                    </td>

                    {/* 栏 2：Gemini / Claude / Argon 额度条 (5h 为 0 与 5h>7d 均优雅呈现) */}
                    <td className="px-4 py-3 align-middle">
                      {acc.isForbidden ? (
                        <div className="p-2.5 rounded-xl bg-rose-50 dark:bg-rose-950/20 border border-rose-200/60 dark:border-rose-900/30 flex items-center gap-2 text-xs text-rose-700 dark:text-rose-400">
                          <AlertCircle className="w-3.5 h-3.5 shrink-0" />
                          <span>{isZh ? '账号存在风控拦截或订阅失效，请重登或设置网络' : 'Account rate limited or token invalid'}</span>
                        </div>
                      ) : (
                        <div className="flex flex-col gap-2 max-w-[480px]">
                          {/* 1. Gemini 额度 */}
                          {renderQuotaBar('Gemini', 'gemini', acc.geminiQuota)}

                          {/* 2. Claude 额度 */}
                          {renderQuotaBar('Claude / GPT', 'claude', acc.claudeQuota)}

                          {/* 3. Argon 额度 (如有) */}
                          {acc.argonQuota && renderQuotaBar('Gemini 4 Argon', 'argon', acc.argonQuota)}
                        </div>
                      )}
                    </td>

                    {/* 栏 3：四宫格快捷切换区 (精致胶囊圆角，永不截断) */}
                    <td className="px-4 py-3 align-middle">
                      <div className="p-1.5 rounded-xl bg-gray-50/70 dark:bg-white/[0.03] border border-gray-200/60 dark:border-white/5 w-[148px] mx-auto shadow-2xs">
                        <div className="grid grid-cols-2 gap-1.5">
                          {[
                            { id: 'app' as HostTarget, label: 'App', icon: AppWindow },
                            { id: 'ide' as HostTarget, label: 'IDE', icon: Code2 },
                            { id: 'plugin' as HostTarget, label: isZh ? '插件' : 'Plugin', icon: Puzzle },
                            { id: 'cli' as HostTarget, label: 'CLI', icon: Terminal },
                          ].map((host) => {
                            const Icon = host.icon;
                            const isActive = acc.activeHosts.includes(host.id);

                            return (
                              <button
                                key={host.id}
                                onClick={() => handleToggleHost(acc.id, host.id)}
                                className={`px-2 py-1.5 rounded-lg text-[10px] font-semibold flex items-center justify-center gap-1.5 transition-all ${
                                  isActive
                                    ? 'bg-blue-600 text-white shadow-xs ring-1 ring-blue-500'
                                    : 'bg-white hover:bg-gray-100 dark:bg-[#202126] dark:hover:bg-[#282930] text-gray-600 dark:text-zinc-400 hover:text-gray-900 dark:hover:text-zinc-100 border border-gray-200/60 dark:border-white/5'
                                }`}
                                title={
                                  isActive
                                    ? isZh
                                      ? `已在此【${host.label}】运行 (点击可解除)`
                                      : `Active in ${host.label} (Click to unbind)`
                                    : isZh
                                    ? `点击一键推送到【${host.label}】`
                                    : `Click to switch to ${host.label}`
                                }
                              >
                                <Icon className="w-3 h-3 shrink-0" />
                                <span className="truncate">{host.label}</span>
                                {isActive && <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 shrink-0" />}
                              </button>
                            );
                          })}
                        </div>
                      </div>
                    </td>

                    {/* 栏 4：操作工具区 (自然内嵌悬浮 Dock，消除突兀割裂感) */}
                    <td className="px-4 py-3 align-middle text-center whitespace-nowrap">
                      <div className="inline-flex items-center justify-center gap-1 p-1 rounded-xl bg-gray-50/70 dark:bg-white/[0.03] border border-gray-200/60 dark:border-white/5 shadow-2xs">
                        {/* 1. 单账号刷新 */}
                        <button
                          onClick={handleRefreshAll}
                          className="w-7 h-7 rounded-lg hover:bg-white dark:hover:bg-[#202126] text-gray-500 hover:text-blue-600 dark:text-zinc-400 dark:hover:text-blue-400 transition-colors flex items-center justify-center shadow-2xs"
                          title={isZh ? '刷新此账号配额' : 'Refresh'}
                        >
                          <RefreshCw className="w-3.5 h-3.5" />
                        </button>

                        {/* 2. 独立设备指纹查看/重置 */}
                        <button
                          onClick={() => setActiveModal({ type: 'fingerprint', account: acc })}
                          className="w-7 h-7 rounded-lg hover:bg-white dark:hover:bg-[#202126] text-gray-500 hover:text-indigo-600 dark:text-zinc-400 dark:hover:text-indigo-400 transition-colors flex items-center justify-center shadow-2xs"
                          title={isZh ? '独立机器码与设备指纹' : 'Device Fingerprint'}
                        >
                          <Fingerprint className="w-3.5 h-3.5" />
                        </button>

                        {/* 3. 标签管理 */}
                        <button
                          onClick={() => setActiveModal({ type: 'tags', account: acc })}
                          className="w-7 h-7 rounded-lg hover:bg-white dark:hover:bg-[#202126] text-gray-500 hover:text-purple-600 dark:text-zinc-400 dark:hover:text-purple-400 transition-colors flex items-center justify-center shadow-2xs"
                          title={isZh ? '标签管理' : 'Manage Tags'}
                        >
                          <Tag className="w-3.5 h-3.5" />
                        </button>

                        {/* 4. 单账号导出 */}
                        <button
                          onClick={() => handleCopyCredentials(acc.id)}
                          className="w-7 h-7 rounded-lg hover:bg-white dark:hover:bg-[#202126] text-gray-500 hover:text-emerald-600 dark:text-zinc-400 dark:hover:text-emerald-400 transition-colors flex items-center justify-center shadow-2xs"
                          title={isZh ? '导出账号凭据 (JSON)' : 'Export JSON'}
                        >
                          <Download className="w-3.5 h-3.5" />
                        </button>

                        {/* 5. 删除账号 */}
                        <button
                          onClick={() => handleDelete(acc.id)}
                          className="w-7 h-7 rounded-lg hover:bg-rose-50 dark:hover:bg-rose-950/30 text-rose-500 hover:text-rose-600 dark:text-rose-400 transition-colors flex items-center justify-center shadow-2xs"
                          title={isZh ? '删除账号' : 'Delete'}
                        >
                          <Trash2 className="w-3.5 h-3.5" />
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>

      {/* 底部批量操作悬浮岛 */}
      {selectedIds.length > 0 && (
        <div className="sticky bottom-4 z-30 bg-gray-900/95 dark:bg-zinc-100/95 text-white dark:text-zinc-900 backdrop-blur-2xl rounded-2xl p-3 px-6 shadow-2xl flex items-center justify-between gap-4 animate-in slide-in-from-bottom-2 duration-200">
          <div className="flex items-center gap-3 text-xs font-semibold">
            <span>{isZh ? `已选中 ${selectedIds.length} 个账号` : `Selected ${selectedIds.length}`}</span>
            <button
              onClick={() => setSelectedIds([])}
              className="text-xs opacity-60 hover:opacity-100 underline ml-2"
            >
              {isZh ? '取消选择' : 'Deselect'}
            </button>
          </div>

          <div className="flex items-center gap-2">
            <button
              onClick={handleRefreshAll}
              className="px-3 py-1.5 rounded-lg bg-white/10 hover:bg-white/20 dark:bg-black/10 dark:hover:bg-black/20 text-xs font-medium transition-colors flex items-center gap-1.5"
            >
              <RefreshCw className="w-3.5 h-3.5" />
              <span>{isZh ? '批量刷新配额' : 'Batch Refresh'}</span>
            </button>
            <button
              onClick={() => {
                const tag = prompt(isZh ? '请输入为选中账号批量添加的标签名称:' : 'Tag name:');
                if (tag && tag.trim()) {
                  setAccounts((prev) =>
                    prev.map((acc) => (selectedIds.includes(acc.id) && !acc.tags.includes(tag.trim()) ? { ...acc, tags: [...acc.tags, tag.trim()] } : acc))
                  );
                }
              }}
              className="px-3 py-1.5 rounded-lg bg-white/10 hover:bg-white/20 dark:bg-black/10 dark:hover:bg-black/20 text-xs font-medium transition-colors flex items-center gap-1.5"
            >
              <Tag className="w-3.5 h-3.5" />
              <span>{isZh ? '批量打标签' : 'Batch Tag'}</span>
            </button>
            <button
              onClick={() => {
                if (confirm(isZh ? `确定删除选中的 ${selectedIds.length} 个账号吗？` : 'Delete selected?')) {
                  setAccounts((prev) => prev.filter((a) => !selectedIds.includes(a.id)));
                  setSelectedIds([]);
                }
              }}
              className="px-3 py-1.5 rounded-lg bg-rose-500 hover:bg-rose-600 text-white text-xs font-medium transition-colors flex items-center gap-1.5"
            >
              <Trash2 className="w-3.5 h-3.5" />
              <span>{isZh ? '批量删除' : 'Batch Delete'}</span>
            </button>
          </div>
        </div>
      )}

      {/* 模态浮层：设备指纹 / 标签管理 / 导出备份 */}
      {activeModal && (
        <div className="fixed inset-0 z-50 bg-black/40 backdrop-blur-sm flex items-center justify-center p-4 animate-in fade-in-0 duration-150">
          <div className="w-full max-w-md bg-white dark:bg-[#16171b] border border-gray-200 dark:border-zinc-800 rounded-2xl p-6 shadow-2xl relative">
            <button
              onClick={() => setActiveModal(null)}
              className="absolute top-4 right-4 p-1.5 rounded-lg text-gray-400 hover:text-gray-700 dark:hover:text-zinc-200 hover:bg-gray-100 dark:hover:bg-zinc-800 transition-colors"
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
                      {isZh ? '独立硬件指纹隔离' : 'Device Fingerprint Isolation'}
                    </h3>
                    <p className="text-xs text-gray-400">{activeModal.account.email}</p>
                  </div>
                </div>

                <div className="p-3.5 rounded-xl bg-gray-50 dark:bg-zinc-900/60 border border-gray-200/60 dark:border-zinc-800 text-xs space-y-2">
                  <div className="flex justify-between items-center text-gray-500">
                    <span>{isZh ? '机器指纹散列:' : 'Fingerprint Hash:'}</span>
                    <span className="font-mono text-gray-900 dark:text-zinc-100 font-semibold">
                      {activeModal.account.fingerprint || 'DEFAULT_ISOLATED_MAC_SONOMA'}
                    </span>
                  </div>
                  <div className="flex justify-between items-center text-gray-500">
                    <span>{isZh ? '风控隔离状态:' : 'Status:'}</span>
                    <span className="text-emerald-600 dark:text-emerald-400 font-semibold flex items-center gap-1">
                      <span className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
                      {isZh ? '已完全物理隔离 · 防关联' : 'Isolated'}
                    </span>
                  </div>
                </div>

                <div className="flex justify-end gap-2 pt-2">
                  <button
                    onClick={() => {
                      const newFp = `fp_regen_${Math.random().toString(36).substring(2, 10)}`;
                      setAccounts((prev) =>
                        prev.map((a) => (a.id === activeModal.account?.id ? { ...a, fingerprint: newFp } : a))
                      );
                      setActiveModal(null);
                    }}
                    className="px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold transition-colors"
                  >
                    {isZh ? '重新生成随机设备指纹' : 'Regenerate Fingerprint'}
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
                      {isZh ? '标签管理' : 'Manage Tags'}
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
                        className="text-gray-400 hover:text-rose-500"
                      >
                        <X className="w-3 h-3" />
                      </button>
                    </span>
                  ))}
                  {activeModal.account.tags.length === 0 && (
                    <span className="text-xs text-gray-400 self-center">{isZh ? '暂无标签，在下方添加' : 'No tags'}</span>
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
                    placeholder={isZh ? '输入新标签按回车添加...' : 'Add new tag...'}
                    className="flex-1 px-3 py-2 text-xs rounded-xl bg-gray-100/80 dark:bg-[#1a1b1f] border border-transparent focus:border-purple-500 text-gray-900 dark:text-zinc-100 outline-none"
                  />
                  <button
                    onClick={() => handleAddTagToAccount(activeModal.account!.id, tempTagInput)}
                    className="px-3.5 py-2 rounded-xl bg-purple-600 hover:bg-purple-700 text-white text-xs font-semibold transition-colors"
                  >
                    {isZh ? '添加' : 'Add'}
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
                      {isZh ? '全量账号数据安全导出' : 'Export Accounts Backup'}
                    </h3>
                    <p className="text-xs text-gray-400">{isZh ? `已收录共 ${accounts.length} 个账号` : `${accounts.length} accounts ready`}</p>
                  </div>
                </div>

                <div className="p-3 bg-gray-50 dark:bg-zinc-900 rounded-xl text-xs font-mono text-gray-600 dark:text-zinc-400 max-h-40 overflow-y-auto">
                  {JSON.stringify(accounts.map((a) => ({ email: a.email, alias: a.alias, tier: a.tier, tags: a.tags })), null, 2)}
                </div>

                <div className="flex justify-end gap-2 pt-2">
                  <button
                    onClick={() => {
                      navigator.clipboard.writeText(JSON.stringify(accounts, null, 2));
                      alert(isZh ? '已复制全量备份 JSON 到剪贴板！' : 'Copied to clipboard!');
                      setActiveModal(null);
                    }}
                    className="px-4 py-2 rounded-xl bg-emerald-600 hover:bg-emerald-700 text-white text-xs font-semibold transition-colors"
                  >
                    {isZh ? '复制备份 JSON' : 'Copy JSON'}
                  </button>
                </div>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
};
