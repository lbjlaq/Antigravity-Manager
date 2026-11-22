import React, { useState } from 'react';
import {
  Search,
  Plus,
  RefreshCw,
  LayoutList,
  Table as TableIcon,
  AlertCircle,
  Download,
  Upload,
  MoreVertical,
  Check,
  ChevronDown,
  Trash2,
  Tag,
  KeyRound,
} from 'lucide-react';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';

export interface QuotaPeriod {
  percentage: number;
  resetText: string;
}

export interface ModelQuotaGroup {
  name: string;
  fiveHour?: QuotaPeriod;
  sevenDay?: QuotaPeriod;
}

export interface AccountItem {
  id: string;
  email: string;
  alias: string;
  tier: 'PRO' | 'FREE';
  tags: string[];
  activeHost?: 'app' | 'ide' | 'plugin' | 'cli' | null;
  quotas: ModelQuotaGroup[];
  isForbidden?: boolean;
}

interface AccountsProps {
  lang: 'zh' | 'en';
}

export const Accounts: React.FC<AccountsProps> = ({ lang }) => {
  const isZh = lang === 'zh';

  // 1. 过滤与视图状态
  const [searchQuery, setSearchQuery] = useState('');
  const [activeFilter, setActiveFilter] = useState<'all' | 'pro' | 'main' | 'claude' | 'warning'>('all');
  const [viewMode, setViewMode] = useState<'card' | 'table'>('card');
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [isRefreshing, setIsRefreshing] = useState(false);

  // 2. 账号 Mock 数据 (契合 accounts.json 与 am-core 真实返回)
  const [accounts, setAccounts] = useState<AccountItem[]>([
    {
      id: '1',
      email: 'ctrler@868666.best',
      alias: '开发主力机 (Master)',
      tier: 'PRO',
      tags: ['🌟 主力', 'VIP', 'Claude池'],
      activeHost: 'app',
      quotas: [
        {
          name: 'Gemini Models',
          fiveHour: { percentage: 100, resetText: '4h 54m' },
          sevenDay: { percentage: 100, resetText: '6d 23h' },
        },
        {
          name: 'Claude and GPT models',
          fiveHour: { percentage: 100, resetText: '4h 50m' },
          sevenDay: { percentage: 50, resetText: '5d 13h' },
        },
        {
          name: 'Gemini 4 Argon',
          sevenDay: { percentage: 100, resetText: '6d 18h' },
        },
      ],
    },
    {
      id: '2',
      email: 'developer.pro@gmail.com',
      alias: 'Cloud Copilot',
      tier: 'PRO',
      tags: ['🌟 主力', 'Team Pro'],
      activeHost: 'ide',
      quotas: [
        {
          name: 'Gemini Models',
          fiveHour: { percentage: 100, resetText: '3h 12m' },
          sevenDay: { percentage: 95, resetText: '4d 08h' },
        },
        {
          name: 'Claude and GPT models',
          fiveHour: { percentage: 85, resetText: '2h 10m' },
          sevenDay: { percentage: 80, resetText: '4d 20h' },
        },
      ],
    },
    {
      id: '3',
      email: 'research.pool1@gmail.com',
      alias: '科研备用池',
      tier: 'FREE',
      tags: ['Claude池', '备用'],
      activeHost: null,
      quotas: [
        {
          name: 'Gemini Models',
          fiveHour: { percentage: 100, resetText: '5h 00m' },
          sevenDay: { percentage: 100, resetText: '7d 00h' },
        },
        {
          name: 'Claude and GPT models',
          fiveHour: { percentage: 40, resetText: '1h 15m' },
          sevenDay: { percentage: 30, resetText: '2d 04h' },
        },
      ],
    },
    {
      id: '4',
      email: 'tester.heavy@gmail.com',
      alias: '压力测试用号',
      tier: 'FREE',
      tags: ['压力测试'],
      activeHost: null,
      isForbidden: true,
      quotas: [
        {
          name: 'Gemini Models',
          fiveHour: { percentage: 0, resetText: '已受限' },
          sevenDay: { percentage: 0, resetText: '需解封' },
        },
      ],
    },
    {
      id: '5',
      email: 'sprint.dev@corp.internal',
      alias: '敏捷研发 05',
      tier: 'PRO',
      tags: ['VIP', 'Argon池'],
      activeHost: 'cli',
      quotas: [
        {
          name: 'Gemini Models',
          fiveHour: { percentage: 100, resetText: '4h 30m' },
          sevenDay: { percentage: 100, resetText: '6d 12h' },
        },
        {
          name: 'Gemini 4 Argon',
          sevenDay: { percentage: 90, resetText: '5d 08h' },
        },
      ],
    },
  ]);

  // 筛选计算
  const filteredAccounts = accounts.filter((acc) => {
    // 文本检索
    const matchText =
      acc.email.toLowerCase().includes(searchQuery.toLowerCase()) ||
      acc.alias.toLowerCase().includes(searchQuery.toLowerCase()) ||
      acc.tags.some((t) => t.toLowerCase().includes(searchQuery.toLowerCase()));

    if (!matchText) return false;

    // 分类筛选
    if (activeFilter === 'pro') return acc.tier === 'PRO';
    if (activeFilter === 'main') return acc.tags.some((t) => t.includes('主力'));
    if (activeFilter === 'claude') return acc.tags.some((t) => t.includes('Claude'));
    if (activeFilter === 'warning') return acc.isForbidden || acc.quotas.some((q) => (q.fiveHour?.percentage ?? 100) < 20);
    return true;
  });

  // 选中管理
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

  // 宿主分配
  const handleAssignHost = (accountId: string, host: 'app' | 'ide' | 'plugin' | 'cli') => {
    setAccounts((prev) =>
      prev.map((acc) => {
        if (acc.id === accountId) {
          return { ...acc, activeHost: host };
        }
        // 如果该宿主原来被其他账号占用，则解除原占有
        if (acc.activeHost === host) {
          return { ...acc, activeHost: null };
        }
        return acc;
      })
    );
  };

  const handleRefreshAll = () => {
    setIsRefreshing(true);
    setTimeout(() => setIsRefreshing(false), 1200);
  };

  const getHostLabel = (host?: 'app' | 'ide' | 'plugin' | 'cli' | null) => {
    if (!host) return isZh ? '⚪ 空闲备用' : '⚪ Standby';
    const map = {
      app: isZh ? '🟢 正在【原生 App】中运行' : '🟢 Active in [Native App]',
      ide: isZh ? '🟢 正在【IDE 定制版】中运行' : '🟢 Active in [IDE Fork]',
      plugin: isZh ? '🟢 正在【ACP 插件】中运行' : '🟢 Active in [ACP Extension]',
      cli: isZh ? '🟢 正在【agy CLI】中运行' : '🟢 Active in [agy CLI]',
    };
    return map[host];
  };

  return (
    <div className="w-full max-w-7xl mx-auto px-6 py-4 flex flex-col gap-5 select-none animate-in fade-in-50 duration-300">
      {/* ============================================================== */}
      {/* 头部子模块 2.1：检索、标签多维透视与视图切换 */}
      {/* ============================================================== */}
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-4 bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-2xl p-4 shadow-xs">
        {/* 左侧：搜索框与分类药丸 */}
        <div className="flex flex-wrap items-center gap-3 flex-1">
          {/* 即时搜索框 */}
          <div className="relative min-w-[220px] max-w-[280px]">
            <Search className="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-gray-400" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder={isZh ? '搜索邮箱、备注或标签...' : 'Search email, alias, tag...'}
              className="w-full pl-9 pr-3 py-1.5 text-xs rounded-xl bg-gray-100/80 dark:bg-[#1a1b1f] border border-transparent focus:border-blue-500/50 dark:focus:border-blue-400/50 text-gray-900 dark:text-zinc-100 placeholder-gray-400 dark:placeholder-zinc-500 outline-none transition-all duration-150"
            />
          </div>

          {/* 分类过滤器 Pills */}
          <div className="flex items-center gap-1.5 overflow-x-auto py-0.5">
            {[
              { id: 'all', label: isZh ? `全部 (${accounts.length})` : `All (${accounts.length})` },
              { id: 'pro', label: isZh ? 'PRO 订阅' : 'PRO Tier' },
              { id: 'main', label: isZh ? '🌟 主力号' : 'Primary' },
              { id: 'claude', label: isZh ? 'Claude 池' : 'Claude Pool' },
              { id: 'warning', label: isZh ? '⚠️ 异常/受限' : 'Alert/Limited' },
            ].map((f) => (
              <button
                key={f.id}
                onClick={() => setActiveFilter(f.id as any)}
                className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-colors whitespace-nowrap ${
                  activeFilter === f.id
                    ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/15 dark:text-blue-400 font-semibold'
                    : 'text-gray-600 dark:text-zinc-400 hover:bg-gray-100 dark:hover:bg-zinc-800/60'
                }`}
              >
                {f.label}
              </button>
            ))}
          </div>
        </div>

        {/* 右侧：视图切换按钮与刷新 */}
        <div className="flex items-center gap-2 shrink-0 self-end md:self-auto">
          {/* 卡片 / 表格切换器 */}
          <div className="flex items-center bg-gray-100/80 dark:bg-[#1a1b1f] p-1 rounded-xl">
            <button
              onClick={() => setViewMode('card')}
              className={`p-1.5 rounded-lg transition-colors ${
                viewMode === 'card'
                  ? 'bg-white dark:bg-[#25262c] text-gray-900 dark:text-zinc-100 shadow-xs'
                  : 'text-gray-400 hover:text-gray-700 dark:hover:text-zinc-200'
              }`}
              title={isZh ? '条状卡片流' : 'Card View'}
            >
              <LayoutList className="w-4 h-4" />
            </button>
            <button
              onClick={() => setViewMode('table')}
              className={`p-1.5 rounded-lg transition-colors ${
                viewMode === 'table'
                  ? 'bg-white dark:bg-[#25262c] text-gray-900 dark:text-zinc-100 shadow-xs'
                  : 'text-gray-400 hover:text-gray-700 dark:hover:text-zinc-200'
              }`}
              title={isZh ? '密集对账表格' : 'Table View'}
            >
              <TableIcon className="w-4 h-4" />
            </button>
          </div>

          {/* 全局配额保鲜刷新 */}
          <button
            onClick={handleRefreshAll}
            disabled={isRefreshing}
            className="px-3 py-1.5 rounded-xl border border-gray-200/80 dark:border-white/5 bg-white dark:bg-[#16171b] hover:bg-gray-50 dark:hover:bg-zinc-800 text-xs font-medium text-gray-700 dark:text-zinc-300 flex items-center gap-1.5 transition-colors shadow-xs"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin text-blue-500' : ''}`} />
            <span>{isZh ? '刷新全量配额' : 'Refresh'}</span>
          </button>
        </div>
      </div>

      {/* ============================================================== */}
      {/* 核心子模块 2.2：高密度账号资产卡片流 (Bento Row Grid) */}
      {/* ============================================================== */}
      {viewMode === 'card' ? (
        <div className="flex flex-col gap-3">
          {filteredAccounts.map((acc) => {
            const isSelected = selectedIds.includes(acc.id);
            return (
              <div
                key={acc.id}
                className={`p-4 rounded-2xl bg-white/80 dark:bg-[#121316]/80 backdrop-blur-xl border transition-all duration-200 shadow-xs hover:shadow-md flex flex-col lg:flex-row items-stretch lg:items-center justify-between gap-4 ${
                  acc.isForbidden
                    ? 'border-rose-300/80 dark:border-rose-900/40 bg-rose-50/20'
                    : isSelected
                    ? 'border-blue-500/80 dark:border-blue-500/50 ring-1 ring-blue-500/20'
                    : 'border-gray-200/70 dark:border-white/5'
                }`}
              >
                {/* ----------------- 列 1：账号身份、宿主与标签 ----------------- */}
                <div className="flex items-start gap-3.5 min-w-[280px] lg:basis-[30%]">
                  {/* 复选框 */}
                  <input
                    type="checkbox"
                    checked={isSelected}
                    onChange={() => handleToggleSelect(acc.id)}
                    className="mt-1 rounded border-gray-300 text-blue-600 focus:ring-blue-500/30 cursor-pointer"
                  />

                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 mb-1">
                      <span className="text-sm font-bold text-gray-900 dark:text-zinc-100 truncate">
                        {acc.email}
                      </span>
                      <span
                        className={`px-1.5 py-0.5 text-[10px] font-bold rounded ${
                          acc.tier === 'PRO'
                            ? 'bg-amber-100 text-amber-800 dark:bg-amber-500/20 dark:text-amber-300'
                            : 'bg-gray-100 text-gray-600 dark:bg-zinc-800 dark:text-zinc-400'
                        }`}
                      >
                        {acc.tier}
                      </span>
                    </div>

                    {/* 备注名称 */}
                    <p className="text-xs text-gray-500 dark:text-zinc-400 mb-2 truncate">
                      {acc.alias}
                    </p>

                    {/* 宿主运行指示器 */}
                    <div className="text-[11px] font-medium text-gray-600 dark:text-zinc-400 mb-2.5">
                      {getHostLabel(acc.activeHost)}
                    </div>

                    {/* 标签栏 */}
                    <div className="flex flex-wrap items-center gap-1.5">
                      {acc.tags.map((t) => (
                        <span
                          key={t}
                          className="px-2 py-0.5 text-[10px] font-medium rounded-md bg-gray-100 dark:bg-zinc-800 text-gray-600 dark:text-zinc-300 border border-gray-200/50 dark:border-white/5"
                        >
                          {t}
                        </span>
                      ))}
                      <button className="px-1.5 py-0.5 text-[10px] rounded-md text-gray-400 hover:text-blue-600 hover:bg-blue-50 dark:hover:bg-blue-500/10 transition-colors">
                        + {isZh ? '标签' : 'Tag'}
                      </button>
                    </div>
                  </div>
                </div>

                {/* ----------------- 列 2：多组配额矩阵 (5h / 7d 双周期) ----------------- */}
                <div className="flex-1 lg:basis-[48%] border-t lg:border-t-0 lg:border-l lg:border-r border-gray-100 dark:border-white/5 py-3 lg:py-0 lg:px-6 flex flex-col justify-center gap-2.5">
                  {acc.isForbidden ? (
                    <div className="p-3 rounded-xl bg-rose-50 dark:bg-rose-950/20 border border-rose-200/60 dark:border-rose-900/30 flex items-center gap-2.5 text-xs text-rose-700 dark:text-rose-400">
                      <AlertCircle className="w-4 h-4 shrink-0" />
                      <div>
                        <p className="font-semibold">{isZh ? '账号异常 / 区域不支持或订阅受限 (403)' : 'Access Forbidden (403)'}</p>
                        <p className="text-[10px] opacity-80 mt-0.5">{isZh ? '建议临时禁用此账号或使用代理网络' : 'Check network proxy or remove'}</p>
                      </div>
                    </div>
                  ) : (
                    acc.quotas.map((group) => {
                      const isClaude = group.name.includes('Claude');
                      const isArgon = group.name.includes('Argon');

                      return (
                        <div key={group.name} className="flex flex-col gap-1 text-xs">
                          <div className="flex items-center justify-between text-[11px]">
                            <span className="font-semibold text-gray-700 dark:text-zinc-300 flex items-center gap-1.5">
                              <span className={`w-1.5 h-1.5 rounded-full ${isClaude ? 'bg-purple-500' : isArgon ? 'bg-amber-500' : 'bg-blue-500'}`} />
                              {group.name}
                            </span>
                            <div className="flex items-center gap-3 text-[10px] text-gray-500 dark:text-zinc-400 font-mono">
                              {group.fiveHour && (
                                <span>
                                  5h: <b className={`${group.fiveHour.percentage < 30 ? 'text-rose-500 font-bold' : 'text-gray-700 dark:text-zinc-200'}`}>{group.fiveHour.percentage}%</b> ({group.fiveHour.resetText})
                                </span>
                              )}
                              {group.sevenDay && (
                                <span>
                                  7d: <b className={`${group.sevenDay.percentage < 30 ? 'text-amber-500 font-bold' : 'text-gray-700 dark:text-zinc-200'}`}>{group.sevenDay.percentage}%</b> ({group.sevenDay.resetText})
                                </span>
                              )}
                            </div>
                          </div>

                          {/* 双时间进度条融合 */}
                          <div className="w-full h-1.5 bg-gray-100 dark:bg-zinc-800 rounded-full overflow-hidden flex gap-0.5">
                            {group.fiveHour && (
                              <div
                                style={{ width: `${group.fiveHour.percentage}%` }}
                                className={`h-full rounded-full transition-all duration-300 ${
                                  group.fiveHour.percentage < 30
                                    ? 'bg-rose-500'
                                    : isClaude
                                    ? 'bg-purple-500'
                                    : 'bg-blue-500'
                                }`}
                              />
                            )}
                            {group.sevenDay && (
                              <div
                                style={{ width: `${group.sevenDay.percentage}%` }}
                                className={`h-full rounded-full opacity-60 transition-all duration-300 ${
                                  group.sevenDay.percentage < 30 ? 'bg-amber-500' : isArgon ? 'bg-amber-500' : 'bg-cyan-500'
                                }`}
                              />
                            )}
                          </div>
                        </div>
                      );
                    })
                  )}
                </div>

                {/* ----------------- 列 3：专属切换部件与敏捷动作 ----------------- */}
                <div className="flex items-center justify-end gap-2 lg:basis-[22%] shrink-0">
                  {/* 多宿主快速推送下拉 */}
                  <DropdownMenu.Root>
                    <DropdownMenu.Trigger asChild>
                      <button className="px-3 py-2 rounded-xl bg-gray-900 text-white hover:bg-gray-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-white text-xs font-medium inline-flex items-center gap-1.5 shadow-sm transition-all">
                        <span>{isZh ? '推至宿主' : 'Push to'}</span>
                        <ChevronDown className="w-3.5 h-3.5 opacity-70" />
                      </button>
                    </DropdownMenu.Trigger>
                    <DropdownMenu.Portal>
                      <DropdownMenu.Content
                        align="end"
                        sideOffset={6}
                        className="w-48 p-1 bg-white dark:bg-[#1a1b20] border border-gray-200 dark:border-zinc-800 rounded-xl shadow-xl z-50 animate-in fade-in-0 zoom-in-95 duration-100"
                      >
                        {[
                          { id: 'app' as const, label: isZh ? '🚀 原生 App' : '🚀 Native App' },
                          { id: 'ide' as const, label: isZh ? '🚀 IDE 定制版' : '🚀 IDE Fork' },
                          { id: 'plugin' as const, label: isZh ? '🚀 ACP 插件' : '🚀 Extension' },
                          { id: 'cli' as const, label: isZh ? '🚀 agy CLI' : '🚀 agy CLI' },
                        ].map((host) => (
                          <DropdownMenu.Item
                            key={host.id}
                            onClick={() => handleAssignHost(acc.id, host.id)}
                            className={`px-3 py-2 text-xs rounded-lg flex items-center justify-between cursor-pointer outline-none transition-colors ${
                              acc.activeHost === host.id
                                ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 font-semibold'
                                : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
                            }`}
                          >
                            <span>{host.label}</span>
                            {acc.activeHost === host.id && <Check className="w-3.5 h-3.5 text-blue-600" />}
                          </DropdownMenu.Item>
                        ))}
                      </DropdownMenu.Content>
                    </DropdownMenu.Portal>
                  </DropdownMenu.Root>

                  {/* 单账号刷新按钮 */}
                  <button
                    onClick={handleRefreshAll}
                    className="p-2 rounded-xl text-gray-500 hover:text-gray-900 dark:text-zinc-400 dark:hover:text-zinc-100 hover:bg-gray-100 dark:hover:bg-zinc-800/80 transition-colors"
                    title={isZh ? '刷新此账号配额' : 'Refresh quota'}
                  >
                    <RefreshCw className="w-4 h-4" />
                  </button>

                  {/* 更多菜单 */}
                  <DropdownMenu.Root>
                    <DropdownMenu.Trigger asChild>
                      <button className="p-2 rounded-xl text-gray-500 hover:text-gray-900 dark:text-zinc-400 dark:hover:text-zinc-100 hover:bg-gray-100 dark:hover:bg-zinc-800/80 transition-colors">
                        <MoreVertical className="w-4 h-4" />
                      </button>
                    </DropdownMenu.Trigger>
                    <DropdownMenu.Portal>
                      <DropdownMenu.Content
                        align="end"
                        sideOffset={6}
                        className="w-40 p-1 bg-white dark:bg-[#1a1b20] border border-gray-200 dark:border-zinc-800 rounded-xl shadow-xl z-50 text-xs"
                      >
                        <DropdownMenu.Item className="px-3 py-2 rounded-lg cursor-pointer hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-700 dark:text-zinc-300 outline-none flex items-center gap-2">
                          <KeyRound className="w-3.5 h-3.5" />
                          <span>{isZh ? '复制 Token' : 'Copy Token'}</span>
                        </DropdownMenu.Item>
                        <DropdownMenu.Item className="px-3 py-2 rounded-lg cursor-pointer hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-700 dark:text-zinc-300 outline-none flex items-center gap-2">
                          <Tag className="w-3.5 h-3.5" />
                          <span>{isZh ? '修改备注' : 'Edit Alias'}</span>
                        </DropdownMenu.Item>
                        <DropdownMenu.Separator className="h-px bg-gray-100 dark:bg-zinc-800 my-1" />
                        <DropdownMenu.Item className="px-3 py-2 rounded-lg cursor-pointer hover:bg-rose-50 text-rose-600 dark:hover:bg-rose-950/20 dark:text-rose-400 outline-none flex items-center gap-2">
                          <Trash2 className="w-3.5 h-3.5" />
                          <span>{isZh ? '删除账号' : 'Delete'}</span>
                        </DropdownMenu.Item>
                      </DropdownMenu.Content>
                    </DropdownMenu.Portal>
                  </DropdownMenu.Root>
                </div>
              </div>
            );
          })}
        </div>
      ) : (
        /* 全景密集对账表格 (Data Table) */
        <div className="bg-white/80 dark:bg-[#121316]/80 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-2xl overflow-hidden shadow-xs">
          <table className="w-full text-left text-xs">
            <thead className="bg-gray-50/70 dark:bg-[#16171b]/70 border-b border-gray-200/60 dark:border-white/5 text-gray-500 dark:text-zinc-400">
              <tr>
                <th className="p-3.5 pl-4 w-10">
                  <input
                    type="checkbox"
                    checked={selectedIds.length === filteredAccounts.length && filteredAccounts.length > 0}
                    onChange={handleSelectAll}
                    className="rounded border-gray-300 text-blue-600 focus:ring-blue-500/30 cursor-pointer"
                  />
                </th>
                <th className="p-3.5 font-semibold">{isZh ? '邮箱与备注' : 'Email & Alias'}</th>
                <th className="p-3.5 font-semibold">{isZh ? '订阅' : 'Tier'}</th>
                <th className="p-3.5 font-semibold">{isZh ? '当前宿主' : 'Active Target'}</th>
                <th className="p-3.5 font-semibold">Gemini (5h / 7d)</th>
                <th className="p-3.5 font-semibold">Claude (5h / 7d)</th>
                <th className="p-3.5 font-semibold text-right pr-4">{isZh ? '操作' : 'Actions'}</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100 dark:divide-white/5">
              {filteredAccounts.map((acc) => {
                const gemini = acc.quotas.find((q) => q.name.includes('Gemini'));
                const claude = acc.quotas.find((q) => q.name.includes('Claude'));
                const isSelected = selectedIds.includes(acc.id);

                return (
                  <tr
                    key={acc.id}
                    className={`hover:bg-gray-50/50 dark:hover:bg-zinc-800/30 transition-colors ${
                      isSelected ? 'bg-blue-50/40 dark:bg-blue-500/5' : ''
                    }`}
                  >
                    <td className="p-3.5 pl-4">
                      <input
                        type="checkbox"
                        checked={isSelected}
                        onChange={() => handleToggleSelect(acc.id)}
                        className="rounded border-gray-300 text-blue-600 focus:ring-blue-500/30 cursor-pointer"
                      />
                    </td>
                    <td className="p-3.5">
                      <div className="font-semibold text-gray-900 dark:text-zinc-100">{acc.email}</div>
                      <div className="text-[11px] text-gray-400 dark:text-zinc-500">{acc.alias}</div>
                    </td>
                    <td className="p-3.5">
                      <span className="px-1.5 py-0.5 text-[10px] font-bold rounded bg-amber-100 text-amber-800 dark:bg-amber-500/20 dark:text-amber-300">
                        {acc.tier}
                      </span>
                    </td>
                    <td className="p-3.5 text-[11px] font-medium text-gray-600 dark:text-zinc-400">
                      {acc.activeHost ? acc.activeHost.toUpperCase() : '—'}
                    </td>
                    <td className="p-3.5 font-mono text-[11px]">
                      {gemini?.fiveHour ? `${gemini.fiveHour.percentage}%` : '—'} / {gemini?.sevenDay ? `${gemini.sevenDay.percentage}%` : '—'}
                    </td>
                    <td className="p-3.5 font-mono text-[11px]">
                      {claude?.fiveHour ? `${claude.fiveHour.percentage}%` : '—'} / {claude?.sevenDay ? `${claude.sevenDay.percentage}%` : '—'}
                    </td>
                    <td className="p-3.5 pr-4 text-right">
                      <button className="text-blue-600 dark:text-blue-400 hover:underline font-medium">
                        {isZh ? '切换' : 'Push'}
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {/* ============================================================== */}
      {/* 底部控制岛子模块 2.3：批量运维与接入 Dock (Batch Ops & Onboarding) */}
      {/* ============================================================== */}
      <div className="sticky bottom-4 z-40 bg-white/90 dark:bg-[#15161b]/90 backdrop-blur-2xl border border-gray-200/80 dark:border-white/10 rounded-2xl p-3 px-5 shadow-lg flex flex-wrap items-center justify-between gap-4">
        {/* 左侧：多选批量动作 */}
        <div className="flex items-center gap-3 text-xs">
          <span className="font-medium text-gray-500 dark:text-zinc-400">
            {isZh ? `已选择 ${selectedIds.length} 个账号` : `Selected ${selectedIds.length}`}
          </span>
          {selectedIds.length > 0 && (
            <div className="flex items-center gap-1.5">
              <button className="px-2.5 py-1.5 rounded-lg bg-gray-100 hover:bg-gray-200 dark:bg-zinc-800 dark:hover:bg-zinc-700 text-gray-700 dark:text-zinc-300 transition-colors flex items-center gap-1 font-medium">
                <RefreshCw className="w-3 h-3" />
                <span>{isZh ? '批量刷新' : 'Batch Refresh'}</span>
              </button>
              <button className="px-2.5 py-1.5 rounded-lg bg-gray-100 hover:bg-gray-200 dark:bg-zinc-800 dark:hover:bg-zinc-700 text-gray-700 dark:text-zinc-300 transition-colors flex items-center gap-1 font-medium">
                <Tag className="w-3 h-3" />
                <span>{isZh ? '打标签' : 'Tag'}</span>
              </button>
              <button className="px-2.5 py-1.5 rounded-lg bg-rose-50 hover:bg-rose-100 dark:bg-rose-950/20 dark:hover:bg-rose-900/30 text-rose-600 dark:text-rose-400 transition-colors flex items-center gap-1 font-medium">
                <Trash2 className="w-3 h-3" />
                <span>{isZh ? '删除' : 'Delete'}</span>
              </button>
            </div>
          )}
        </div>

        {/* 右侧：官方 OAuth 接入与备份导入导出 */}
        <div className="flex items-center gap-2.5">
          <button className="px-3 py-2 rounded-xl border border-gray-200/80 dark:border-white/10 bg-white dark:bg-[#1a1b20] hover:bg-gray-50 dark:hover:bg-zinc-800 text-xs font-medium text-gray-700 dark:text-zinc-300 flex items-center gap-1.5 transition-colors shadow-xs">
            <Upload className="w-3.5 h-3.5" />
            <span>{isZh ? '导入 JSON' : 'Import'}</span>
          </button>
          <button className="px-3 py-2 rounded-xl border border-gray-200/80 dark:border-white/10 bg-white dark:bg-[#1a1b20] hover:bg-gray-50 dark:hover:bg-zinc-800 text-xs font-medium text-gray-700 dark:text-zinc-300 flex items-center gap-1.5 transition-colors shadow-xs">
            <Download className="w-3.5 h-3.5" />
            <span>{isZh ? '备份导出' : 'Export'}</span>
          </button>
          <button className="px-3.5 py-2 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold flex items-center gap-1.5 shadow-md shadow-blue-500/20 transition-all">
            <Plus className="w-3.5 h-3.5" />
            <span>{isZh ? '官方授权登录' : 'Add Account'}</span>
          </button>
        </div>
      </div>
    </div>
  );
};
