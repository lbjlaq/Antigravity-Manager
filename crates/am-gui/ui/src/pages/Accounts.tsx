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
  X,
} from 'lucide-react';

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

  // 2. 账号 Mock 数据 (包含 5h 为 0、5h 长于周额度等真实边缘用例)
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
        // 典型场景：5h 为 0% 耗尽，周额度还剩 50%
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
        // 典型场景：5h 配额为 85%，大于周额度的 40% (即 5h 比周额度长)
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

  // 提取系统中所有出现过的标签
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
    if (selectedTag === '__warning') return acc.isForbidden || (acc.claudeQuota?.fiveHour?.percentage === 0);
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
   * 优雅解决 5h 与 7d 相对长度颠倒及 5h 为 0 的直观双层融合进度条：
   * 采用「外层周配额基底槽 (7d Window Track) + 内嵌/重叠 5h 短期实时流光条」
   * - 当 5h 为 0 时：进度条清零，显示鲜明闪烁红底「5h: 0% 耗尽」
   * - 无论 5h 比 7d 长还是短，两者各自在同一起点独立标定宽度与重置时间，绝对不会发生误导性覆盖
   */
  const renderQuotaBar = (name: string, key: 'gemini' | 'claude' | 'argon', quota?: ModelGroupQuota) => {
    if (!quota) return null;
    const { fiveHour, sevenDay } = quota;
    const isClaude = key === 'claude';
    const isArgon = key === 'argon';

    const fiveHourPct = fiveHour?.percentage;
    const sevenDayPct = sevenDay?.percentage ?? 100;

    const is5hExhausted = fiveHourPct !== undefined && fiveHourPct === 0;
    const is7dExhausted = sevenDayPct === 0;

    const get5hColor = () => {
      if (is5hExhausted) return 'bg-rose-500';
      if ((fiveHourPct ?? 100) < 30) return 'bg-rose-500';
      if ((fiveHourPct ?? 100) < 60) return 'bg-amber-500';
      if (isClaude) return 'bg-purple-500';
      if (isArgon) return 'bg-amber-500';
      return 'bg-blue-500';
    };

    return (
      <div className="flex flex-col gap-1 w-full">
        {/* 指标文本栏 */}
        <div className="flex items-center justify-between text-[11px] leading-tight">
          <span className="font-semibold text-gray-700 dark:text-zinc-300 flex items-center gap-1.5">
            <span
              className={`w-1.5 h-1.5 rounded-full ${
                is5hExhausted ? 'bg-rose-500 animate-ping' : isClaude ? 'bg-purple-500' : isArgon ? 'bg-amber-500' : 'bg-blue-500'
              }`}
            />
            <span className="truncate">{name}</span>
          </span>

          <div className="flex items-center gap-2 text-[10px] text-gray-500 dark:text-zinc-400 font-mono">
            {fiveHour && (
              <span className={`flex items-center gap-1 ${is5hExhausted ? 'text-rose-600 dark:text-rose-400 font-bold bg-rose-50 dark:bg-rose-950/40 px-1 py-0.2 rounded border border-rose-200/50' : ''}`}>
                <span>5h:</span>
                <b className={(fiveHourPct ?? 100) < 25 ? 'text-rose-500 font-bold' : 'text-gray-700 dark:text-zinc-200'}>
                  {fiveHourPct}%
                </b>
                <span className="opacity-70 text-[9px]">({fiveHour.resetText})</span>
              </span>
            )}
            {sevenDay && (
              <span className={`flex items-center gap-1 ${is7dExhausted ? 'text-rose-600 font-bold' : ''}`}>
                <span className="opacity-75">7d:</span>
                <b className={sevenDayPct < 25 ? 'text-rose-500 font-bold' : 'text-gray-700 dark:text-zinc-200'}>
                  {sevenDayPct}%
                </b>
                <span className="opacity-70 text-[9px]">({sevenDay.resetText})</span>
              </span>
            )}
          </div>
        </div>

        {/* 双层同轨融合进度槽 (Dual Track in Same Capsule) */}
        <div
          className={`relative w-full h-[6px] rounded-full overflow-hidden transition-colors ${
            is5hExhausted ? 'bg-rose-100/80 dark:bg-rose-950/40' : 'bg-gray-100 dark:bg-zinc-800'
          }`}
          title={`${name} · 5h: ${fiveHourPct ?? 'N/A'}% | 7d: ${sevenDayPct}%`}
        >
          {/* 1. 底层：周配额 (7d) 宽轨进度 (透明度 35%，标定宏观储备) */}
          {sevenDay && (
            <div
              style={{ width: `${sevenDayPct}%` }}
              className={`absolute top-0 bottom-0 left-0 rounded-full transition-all duration-300 ${
                isClaude ? 'bg-indigo-400/40' : isArgon ? 'bg-orange-400/40' : 'bg-cyan-400/40'
              }`}
            />
          )}

          {/* 2. 顶层：5小时 (5h) 核心流速实时条 (高亮纯色实线) */}
          {fiveHour && (
            <div
              style={{ width: `${fiveHourPct}%` }}
              className={`absolute top-0 bottom-0 left-0 rounded-full shadow-xs transition-all duration-300 ${get5hColor()}`}
            />
          )}
        </div>
      </div>
    );
  };

  return (
    <div className="w-full max-w-7xl mx-auto px-6 py-4 flex flex-col gap-4 select-none animate-in fade-in-50 duration-300">
      
      {/* ============================================================== */}
      {/* 顶部单行集成工具栏 (Integrated Single-Row Universal Toolbar)   */}
      {/* 搜索 + Tag多选分类过滤 + 批量操作 + 备份导入导出与登录，均在一行内高密集成 */}
      {/* ============================================================== */}
      <div className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-2xl p-3 px-4 shadow-xs flex flex-wrap items-center justify-between gap-3">
        {/* 左侧：搜索框与直观 Tag 选中分类 */}
        <div className="flex flex-wrap items-center gap-2.5 flex-1 min-w-[320px]">
          {/* 即时搜索框 */}
          <div className="relative w-52 shrink-0">
            <Search className="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-gray-400" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder={isZh ? '搜索邮箱、备注或标签...' : 'Search accounts...'}
              className="w-full pl-9 pr-3 py-1.5 text-xs rounded-xl bg-gray-100/80 dark:bg-[#1a1b1f] border border-transparent focus:border-blue-500/50 dark:focus:border-blue-400/50 text-gray-900 dark:text-zinc-100 placeholder-gray-400 dark:placeholder-zinc-500 outline-none transition-all duration-150"
            />
          </div>

          {/* 分类与动态 Tag 筛选药丸 */}
          <div className="flex items-center gap-1.5 overflow-x-auto max-w-full py-0.5">
            <button
              onClick={() => setSelectedTag('all')}
              className={`px-2.5 py-1.5 rounded-lg text-xs font-medium transition-colors whitespace-nowrap ${
                selectedTag === 'all'
                  ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/15 dark:text-blue-400 font-semibold'
                  : 'text-gray-600 dark:text-zinc-400 hover:bg-gray-100 dark:hover:bg-zinc-800/60'
              }`}
            >
              {isZh ? `全部 (${accounts.length})` : `All (${accounts.length})`}
            </button>

            <button
              onClick={() => setSelectedTag('__pro')}
              className={`px-2.5 py-1.5 rounded-lg text-xs font-medium transition-colors whitespace-nowrap ${
                selectedTag === '__pro'
                  ? 'bg-amber-50 text-amber-600 dark:bg-amber-500/15 dark:text-amber-400 font-semibold'
                  : 'text-gray-600 dark:text-zinc-400 hover:bg-gray-100 dark:hover:bg-zinc-800/60'
              }`}
            >
              PRO 订阅
            </button>

            <button
              onClick={() => setSelectedTag('__warning')}
              className={`px-2.5 py-1.5 rounded-lg text-xs font-medium transition-colors whitespace-nowrap ${
                selectedTag === '__warning'
                  ? 'bg-rose-50 text-rose-600 dark:bg-rose-500/15 dark:text-rose-400 font-semibold'
                  : 'text-gray-600 dark:text-zinc-400 hover:bg-gray-100 dark:hover:bg-zinc-800/60'
              }`}
            >
              ⚠️ 额度告警/受限
            </button>

            {/* 用户动态标签选择药丸 */}
            {allTags.map((tag) => (
              <button
                key={tag}
                onClick={() => setSelectedTag(tag)}
                className={`px-2.5 py-1.5 rounded-lg text-xs font-medium transition-colors whitespace-nowrap flex items-center gap-1 ${
                  selectedTag === tag
                    ? 'bg-purple-50 text-purple-600 dark:bg-purple-500/15 dark:text-purple-400 font-semibold'
                    : 'text-gray-600 dark:text-zinc-400 hover:bg-gray-100 dark:hover:bg-zinc-800/60'
                }`}
              >
                <span>🏷️ {tag}</span>
              </button>
            ))}
          </div>
        </div>

        {/* 右侧：全功能工具按钮组（刷新、导入导出、指纹全局状态、授权登录） */}
        <div className="flex items-center gap-2 shrink-0">
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
            className="px-3 py-1.5 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold flex items-center gap-1.5 shadow-md shadow-blue-500/20 transition-all"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>{isZh ? '授权登录' : 'Add Account'}</span>
          </button>
        </div>
      </div>

      {/* ============================================================== */}
      {/* 核心 4 栏式黄金表格：                                            */}
      {/* [账号与标签] | [模型配额池 (5h/7d)] | [四宫格多端切号] | [操作区] */}
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
                <th className="px-4 py-3 font-semibold text-gray-600 dark:text-zinc-300 w-[240px] whitespace-nowrap">
                  <div className="flex items-center gap-1.5">
                    <span>{isZh ? '账号 / 标签' : 'Account & Tags'}</span>
                    <ArrowUpDown className="w-3 h-3 opacity-40" />
                  </div>
                </th>
                {/* 栏 2：Gemini / Claude / Argon 统一双轨进度 */}
                <th className="px-4 py-3 font-semibold text-gray-600 dark:text-zinc-300 min-w-[320px] whitespace-nowrap">
                  <div className="flex items-center justify-between">
                    <span>{isZh ? '模型额度池 (5h 实时流速 / 7d 宏观槽)' : 'Model Quota Pools (5h / 7d Unified)'}</span>
                    <span className="text-[10px] text-gray-400 font-normal">内轨 5h · 底轨 7d</span>
                  </div>
                </th>
                {/* 栏 3：四宫格快捷切换区 */}
                <th className="px-4 py-3 font-semibold text-gray-600 dark:text-zinc-300 w-[170px] whitespace-nowrap text-center">
                  <span>{isZh ? '四宫格多端切号区' : 'Host Matrix (4-Grid)'}</span>
                </th>
                {/* 栏 4：操作工具区 (刷新/指纹/标签管理/删除/导出) */}
                <th className="px-4 py-3 font-semibold text-gray-600 dark:text-zinc-300 w-[160px] text-center sticky right-0 bg-gray-50/90 dark:bg-[#16171b]/90 backdrop-blur-md shadow-[-12px_0_12px_-12px_rgba(0,0,0,0.1)] dark:shadow-[-12px_0_12px_-12px_rgba(255,255,255,0.05)]">
                  {isZh ? '操作工具' : 'Actions'}
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

                        {/* 标签栏 (点击可快速过滤或管理) */}
                        <div className="flex flex-wrap items-center gap-1">
                          {acc.tags.map((t) => (
                            <button
                              key={t}
                              onClick={() => setSelectedTag(t)}
                              className="px-1.5 py-0.2 text-[9px] font-medium rounded bg-gray-100 dark:bg-zinc-800 text-gray-600 dark:text-zinc-300 border border-gray-200/50 dark:border-white/5 hover:border-blue-400 transition-colors"
                              title={`点击筛选标签 [${t}]`}
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

                    {/* 栏 2：Gemini / Claude / Argon 额度条 (5h 为 0 与 5h>7d 均清晰呈现) */}
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

                    {/* 栏 3：四宫格快捷切换区 (Native App / IDE / Plugin / CLI 直点秒切) */}
                    <td className="px-4 py-3 align-middle">
                      <div className="grid grid-cols-2 gap-1.5 w-[140px] mx-auto">
                        {[
                          { id: 'app' as HostTarget, label: isZh ? '原生App' : 'App', icon: AppWindow },
                          { id: 'ide' as HostTarget, label: isZh ? 'IDE' : 'IDE', icon: Code2 },
                          { id: 'plugin' as HostTarget, label: isZh ? '插件' : 'Plugin', icon: Puzzle },
                          { id: 'cli' as HostTarget, label: isZh ? 'CLI' : 'CLI', icon: Terminal },
                        ].map((host) => {
                          const Icon = host.icon;
                          const isActive = acc.activeHosts.includes(host.id);

                          return (
                            <button
                              key={host.id}
                              onClick={() => handleToggleHost(acc.id, host.id)}
                              className={`px-2 py-1.5 rounded-lg text-[10px] font-semibold flex items-center justify-center gap-1.5 transition-all shadow-xs ${
                                isActive
                                  ? 'bg-blue-600 text-white shadow-blue-500/20 ring-1 ring-blue-500'
                                  : 'bg-gray-100/80 hover:bg-gray-200/80 dark:bg-zinc-800/60 dark:hover:bg-zinc-700 text-gray-600 dark:text-zinc-400 hover:text-gray-900 dark:hover:text-zinc-100 border border-transparent dark:border-white/5'
                              }`}
                              title={
                                isActive
                                  ? isZh
                                    ? `已在此【${host.label}】激活 (点击取消绑定)`
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
                    </td>

                    {/* 栏 4：操作工具区 (刷新 / 指纹 / 标签管理 / 导出 / 删除) */}
                    <td className="px-4 py-3 align-middle text-center sticky right-0 bg-white/90 dark:bg-[#121316]/90 backdrop-blur-md shadow-[-12px_0_12px_-12px_rgba(0,0,0,0.1)] dark:shadow-[-12px_0_12px_-12px_rgba(255,255,255,0.05)]">
                      <div className="flex items-center justify-center gap-1">
                        {/* 1. 单账号刷新 */}
                        <button
                          onClick={handleRefreshAll}
                          className="p-1.5 rounded-lg border border-gray-200/80 dark:border-white/10 hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-500 dark:text-zinc-400 transition-colors"
                          title={isZh ? '刷新此账号配额' : 'Refresh'}
                        >
                          <RefreshCw className="w-3.5 h-3.5" />
                        </button>

                        {/* 2. 独立设备指纹查看/重置 */}
                        <button
                          onClick={() => setActiveModal({ type: 'fingerprint', account: acc })}
                          className="p-1.5 rounded-lg border border-gray-200/80 dark:border-white/10 hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-500 hover:text-blue-600 dark:text-zinc-400 transition-colors"
                          title={isZh ? '独立机器码与设备指纹' : 'Device Fingerprint'}
                        >
                          <Fingerprint className="w-3.5 h-3.5" />
                        </button>

                        {/* 3. 标签管理 */}
                        <button
                          onClick={() => setActiveModal({ type: 'tags', account: acc })}
                          className="p-1.5 rounded-lg border border-gray-200/80 dark:border-white/10 hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-500 hover:text-purple-600 dark:text-zinc-400 transition-colors"
                          title={isZh ? '标签管理' : 'Manage Tags'}
                        >
                          <Tag className="w-3.5 h-3.5" />
                        </button>

                        {/* 4. 单账号导出 */}
                        <button
                          onClick={() => handleCopyCredentials(acc.id)}
                          className="p-1.5 rounded-lg border border-gray-200/80 dark:border-white/10 hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-500 hover:text-emerald-600 dark:text-zinc-400 transition-colors"
                          title={isZh ? '导出账号凭据 (JSON)' : 'Export JSON'}
                        >
                          <Download className="w-3.5 h-3.5" />
                        </button>

                        {/* 5. 删除账号 */}
                        <button
                          onClick={() => handleDelete(acc.id)}
                          className="p-1.5 rounded-lg border border-rose-200/80 dark:border-rose-900/30 hover:bg-rose-50 text-rose-600 dark:hover:bg-rose-950/20 dark:text-rose-400 transition-colors"
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

      {/* ============================================================== */}
      {/* 底部批量操作悬浮岛 (仅在勾选选中账号时呼出)                    */}
      {/* ============================================================== */}
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

      {/* ============================================================== */}
      {/* 模态浮层：设备指纹 / 标签管理 / 导出备份                       */}
      {/* ============================================================== */}
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

                {/* 现有标签 */}
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

                {/* 输入新标签 */}
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
