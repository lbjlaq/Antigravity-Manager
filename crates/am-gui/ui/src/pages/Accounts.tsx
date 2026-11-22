import React, { useState } from 'react';
import {
  Search,
  Plus,
  RefreshCw,
  AlertCircle,
  Download,
  Upload,
  MoreVertical,
  Check,
  ChevronDown,
  Trash2,
  Tag,
  BookCopy,
  Zap,
  CheckCircle2,
  Lock,
  ArrowUpDown
} from 'lucide-react';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { GroupQuotaCard, ModelGroupSummary } from '../components/accounts/GroupQuotaCard';

export interface AccountItem {
  id: string;
  email: string;
  alias: string;
  tier: 'PRO' | 'FREE';
  tags: string[];
  activeHost?: 'app' | 'ide' | 'plugin' | 'cli' | null;
  quotas: ModelGroupSummary[];
  isForbidden?: boolean;
}

interface AccountsProps {
  lang: 'zh' | 'en';
}

export const Accounts: React.FC<AccountsProps> = ({ lang }) => {
  const isZh = lang === 'zh';

  // 1. 检索与分类状态
  const [searchQuery, setSearchQuery] = useState('');
  const [activeFilter, setActiveFilter] = useState<'all' | 'pro' | 'main' | 'claude' | 'warning'>('all');
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [copiedId, setCopiedId] = useState<string | null>(null);

  // 2. 账号 Mock 数据 (符合主分支生态结构，支持几十甚至数百个账号密集排版)
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
          key: 'gemini',
          name: 'Gemini',
          fiveHour: { percentage: 100, resetText: '4h 54m' },
          sevenDay: { percentage: 100, resetText: '6d 23h' },
        },
        {
          key: 'claude',
          name: 'Claude / GPT',
          fiveHour: { percentage: 100, resetText: '4h 50m' },
          sevenDay: { percentage: 50, resetText: '5d 13h' },
        },
        {
          key: 'argon',
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
          key: 'gemini',
          name: 'Gemini',
          fiveHour: { percentage: 100, resetText: '3h 12m' },
          sevenDay: { percentage: 95, resetText: '4d 08h' },
        },
        {
          key: 'claude',
          name: 'Claude / GPT',
          fiveHour: { percentage: 85, resetText: '2h 10m' },
          sevenDay: { percentage: 80, resetText: '4d 20h' },
        },
      ],
    },
    {
      id: '3',
      email: 'research.pool1@gmail.com',
      alias: '科研备用池 01',
      tier: 'FREE',
      tags: ['Claude池', '备用'],
      activeHost: 'plugin',
      quotas: [
        {
          key: 'gemini',
          name: 'Gemini',
          fiveHour: { percentage: 100, resetText: '5h 00m' },
          sevenDay: { percentage: 100, resetText: '7d 00h' },
        },
        {
          key: 'claude',
          name: 'Claude / GPT',
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
          key: 'gemini',
          name: 'Gemini',
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
          key: 'gemini',
          name: 'Gemini',
          fiveHour: { percentage: 100, resetText: '4h 30m' },
          sevenDay: { percentage: 100, resetText: '6d 12h' },
        },
        {
          key: 'argon',
          name: 'Gemini 4 Argon',
          sevenDay: { percentage: 90, resetText: '5d 08h' },
        },
      ],
    },
    {
      id: '6',
      email: 'team.backup.core@gmail.com',
      alias: '应急核心池',
      tier: 'PRO',
      tags: ['备用', 'Claude池'],
      activeHost: null,
      quotas: [
        {
          key: 'gemini',
          name: 'Gemini',
          fiveHour: { percentage: 90, resetText: '2h 15m' },
          sevenDay: { percentage: 85, resetText: '5d 10h' },
        },
        {
          key: 'claude',
          name: 'Claude / GPT',
          fiveHour: { percentage: 70, resetText: '3h 40m' },
          sevenDay: { percentage: 65, resetText: '3d 12h' },
        },
      ],
    },
  ]);

  // 筛选计算
  const filteredAccounts = accounts.filter((acc) => {
    const matchText =
      acc.email.toLowerCase().includes(searchQuery.toLowerCase()) ||
      acc.alias.toLowerCase().includes(searchQuery.toLowerCase()) ||
      acc.tags.some((t) => t.toLowerCase().includes(searchQuery.toLowerCase()));

    if (!matchText) return false;

    if (activeFilter === 'pro') return acc.tier === 'PRO';
    if (activeFilter === 'main') return acc.tags.some((t) => t.includes('主力'));
    if (activeFilter === 'claude') return acc.tags.some((t) => t.includes('Claude'));
    if (activeFilter === 'warning') return acc.isForbidden || acc.quotas.some((q) => (q.fiveHour?.percentage ?? 100) < 20);
    return true;
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

  const handleAssignHost = (accountId: string, host: 'app' | 'ide' | 'plugin' | 'cli') => {
    setAccounts((prev) =>
      prev.map((acc) => {
        if (acc.id === accountId) return { ...acc, activeHost: host };
        if (acc.activeHost === host) return { ...acc, activeHost: null };
        return acc;
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

  return (
    <div className="w-full max-w-7xl mx-auto px-6 py-4 flex flex-col gap-4 select-none animate-in fade-in-50 duration-300">
      
      {/* 顶部透视检索与操作栏 */}
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-4 bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-2xl p-4 shadow-xs">
        {/* 左侧：搜索框与分类过滤器 */}
        <div className="flex flex-wrap items-center gap-3 flex-1">
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

        {/* 右侧：刷新配额按钮 */}
        <div className="flex items-center gap-2 shrink-0">
          <button
            onClick={handleRefreshAll}
            disabled={isRefreshing}
            className="px-3 py-1.5 rounded-xl border border-gray-200/80 dark:border-white/5 bg-white dark:bg-[#16171b] hover:bg-gray-50 dark:hover:bg-zinc-800 text-xs font-medium text-gray-700 dark:text-zinc-300 flex items-center gap-1.5 transition-colors shadow-xs"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin text-blue-500' : ''}`} />
            <span>{isZh ? '刷新全量配额' : 'Refresh Quota'}</span>
          </button>
        </div>
      </div>

      {/* ============================================================== */}
      {/* 核心主分支风格高密度对账表格 (Account Master Table) */}
      {/* 专为支持 50+ 账号高密度呈现优化，完全避免卡片式垂直空间过大 */}
      {/* ============================================================== */}
      <div className="bg-white/80 dark:bg-[#121316]/80 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-2xl overflow-hidden shadow-xs relative">
        <div className="overflow-x-auto">
          <table className="w-full text-left text-xs border-collapse">
            {/* 表头 (参考主分支标准表头架构与列宽) */}
            <thead className="bg-gray-50/80 dark:bg-[#16171b]/80 border-b border-gray-200/60 dark:border-white/5 text-gray-500 dark:text-zinc-400 select-none">
              <tr>
                <th className="pl-3.5 py-2.5 w-9">
                  <input
                    type="checkbox"
                    checked={selectedIds.length === filteredAccounts.length && filteredAccounts.length > 0}
                    onChange={handleSelectAll}
                    className="rounded border-gray-300 dark:border-zinc-700 text-blue-600 focus:ring-blue-500/30 cursor-pointer"
                  />
                </th>
                <th className="px-3 py-2.5 font-semibold text-gray-600 dark:text-zinc-300 w-[240px] whitespace-nowrap">
                  <div className="flex items-center gap-1.5">
                    <span>{isZh ? '账号身份 / 宿主状态' : 'Account & Host'}</span>
                    <ArrowUpDown className="w-3 h-3 opacity-40" />
                  </div>
                </th>
                <th className="px-3 py-2.5 font-semibold text-gray-600 dark:text-zinc-300 min-w-[360px] whitespace-nowrap">
                  <div className="flex items-center gap-1.5">
                    <span>{isZh ? '动态模型配额池 (5h / 7d 双时间周期)' : 'Model Quota Pools (5h / 7d Windows)'}</span>
                    <span className="text-[10px] text-gray-400 font-normal">支持 Gemini / Claude / Argon 独立核算</span>
                  </div>
                </th>
                <th className="px-3 py-2.5 font-semibold text-gray-600 dark:text-zinc-300 w-[140px] whitespace-nowrap">
                  <span>{isZh ? '标签与备注' : 'Tags & Alias'}</span>
                </th>
                <th className="px-3 py-2.5 font-semibold text-gray-600 dark:text-zinc-300 w-[148px] text-center sticky right-0 bg-gray-50/90 dark:bg-[#16171b]/90 backdrop-blur-md shadow-[-12px_0_12px_-12px_rgba(0,0,0,0.1)] dark:shadow-[-12px_0_12px_-12px_rgba(255,255,255,0.05)]">
                  {isZh ? '操作' : 'Actions'}
                </th>
              </tr>
            </thead>

            {/* 表体 */}
            <tbody className="divide-y divide-gray-100 dark:divide-white/5 font-sans">
              {filteredAccounts.map((acc) => {
                const isSelected = selectedIds.includes(acc.id);
                const hasArgon = acc.quotas.some((q) => q.key === 'argon');

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

                    {/* 列 1：邮箱、订阅级别徽章与当前宿主 */}
                    <td className="px-3 py-3 align-middle">
                      <div className="flex flex-col gap-1 min-w-0 pr-2">
                        {/* 邮箱 + PRO 徽章 + 复制凭证 */}
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

                        {/* 宿主运行徽章 (参考主分支经典微标样式) */}
                        <div className="flex flex-wrap items-center gap-1.5 mt-0.5">
                          {acc.activeHost === 'app' && (
                            <span className="px-2 py-0.5 rounded-md bg-blue-100 dark:bg-blue-900/50 text-blue-700 dark:text-blue-300 text-[10px] font-bold shadow-xs border border-blue-200/50 dark:border-blue-800/50 flex items-center gap-1">
                              <span className="w-1.5 h-1.5 rounded-full bg-blue-500" />
                              <span>{isZh ? '原生 App' : 'Native App'}</span>
                            </span>
                          )}
                          {acc.activeHost === 'ide' && (
                            <span className="px-2 py-0.5 rounded-md bg-sky-100 dark:bg-sky-900/50 text-sky-700 dark:text-sky-300 text-[10px] font-bold shadow-xs border border-sky-200/50 dark:border-sky-800/50 flex items-center gap-1">
                              <span className="w-1.5 h-1.5 rounded-full bg-sky-500" />
                              <span>{isZh ? 'IDE 定制' : 'IDE Fork'}</span>
                            </span>
                          )}
                          {acc.activeHost === 'plugin' && (
                            <span className="px-2 py-0.5 rounded-md bg-violet-100 dark:bg-violet-900/50 text-violet-700 dark:text-violet-300 text-[10px] font-bold shadow-xs border border-violet-200/50 dark:border-violet-800/50 flex items-center gap-1">
                              <span className="w-1.5 h-1.5 rounded-full bg-violet-500" />
                              <span>{isZh ? 'ACP 插件' : 'Extension'}</span>
                            </span>
                          )}
                          {acc.activeHost === 'cli' && (
                            <span className="px-2 py-0.5 rounded-md bg-emerald-100 dark:bg-emerald-900/50 text-emerald-700 dark:text-emerald-300 text-[10px] font-bold shadow-xs border border-emerald-200/50 dark:border-emerald-800/50 flex items-center gap-1">
                              <span className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
                              <span>{isZh ? 'agy CLI' : 'agy CLI'}</span>
                            </span>
                          )}
                          {!acc.activeHost && (
                            <span className="px-2 py-0.5 rounded-md bg-gray-100/70 dark:bg-zinc-800/60 text-gray-400 dark:text-zinc-500 text-[10px] font-medium">
                              {isZh ? '⚪ 空闲' : '⚪ Standby'}
                            </span>
                          )}

                          {/* 403 异常预警 */}
                          {acc.isForbidden && (
                            <span className="px-2 py-0.5 rounded-md bg-rose-100 dark:bg-rose-950/50 text-rose-600 dark:text-rose-400 text-[10px] font-bold flex items-center gap-1 shadow-xs border border-rose-200/50">
                              <Lock className="w-2.5 h-2.5" />
                              <span>{isZh ? '已受限 (403)' : 'Forbidden'}</span>
                            </span>
                          )}
                        </div>
                      </div>
                    </td>

                    {/* 列 2：核心 GroupQuotaCard 配额胶囊条矩阵 (5h + 周额度进度条) */}
                    <td className="px-3 py-3 align-middle">
                      {acc.isForbidden ? (
                        <div className="p-2 rounded-xl bg-rose-50 dark:bg-rose-950/20 border border-rose-200/60 dark:border-rose-900/30 flex items-center gap-2 text-xs text-rose-700 dark:text-rose-400">
                          <AlertCircle className="w-3.5 h-3.5 shrink-0" />
                          <span>{isZh ? '账号存在风控拦截或订阅失效，请重登或设置网络' : 'Account rate limited or token invalid'}</span>
                        </div>
                      ) : (
                        <div className="flex flex-col gap-1.5 max-w-[520px]">
                          {/* 上方：并排展示 Gemini 与 Claude 两大核心池 */}
                          <div className="grid grid-cols-1 sm:grid-cols-2 gap-1.5">
                            {acc.quotas
                              .filter((q) => q.key !== 'argon')
                              .map((summary) => (
                                <GroupQuotaCard key={summary.key} summary={summary} />
                              ))}
                          </div>

                          {/* 下方：独立展示 Gemini 4 Argon（若存在） */}
                          {hasArgon && (
                            <div className="w-full">
                              {acc.quotas
                                .filter((q) => q.key === 'argon')
                                .map((summary) => (
                                  <GroupQuotaCard key={summary.key} summary={summary} />
                                ))}
                            </div>
                          )}
                        </div>
                      )}
                    </td>

                    {/* 列 3：标签与备注 */}
                    <td className="px-3 py-3 align-middle">
                      <div className="flex flex-col gap-1 min-w-0">
                        <span className="text-xs text-gray-500 dark:text-zinc-400 truncate font-medium">
                          {acc.alias}
                        </span>
                        <div className="flex flex-wrap items-center gap-1">
                          {acc.tags.map((t) => (
                            <span
                              key={t}
                              className="px-1.5 py-0.2 text-[9px] font-medium rounded bg-gray-100 dark:bg-zinc-800 text-gray-600 dark:text-zinc-300 border border-gray-200/50 dark:border-white/5"
                            >
                              {t}
                            </span>
                          ))}
                        </div>
                      </div>
                    </td>

                    {/* 列 4：极简固定操作列 (对齐主分支右侧吸附架构) */}
                    <td className="px-3 py-3 align-middle text-center sticky right-0 bg-white/90 dark:bg-[#121316]/90 backdrop-blur-md shadow-[-12px_0_12px_-12px_rgba(0,0,0,0.1)] dark:shadow-[-12px_0_12px_-12px_rgba(255,255,255,0.05)]">
                      <div className="grid grid-cols-2 gap-1 w-full max-w-[130px] mx-auto">
                        {/* 1. 切换按钮（下拉选择推送靶标） */}
                        <DropdownMenu.Root>
                          <DropdownMenu.Trigger asChild>
                            <button className="flex items-center justify-center gap-0.5 px-2 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-700 text-white text-[11px] font-semibold transition-all shadow-xs">
                              <Zap className="w-3 h-3" />
                              <span>{isZh ? '切换' : 'Push'}</span>
                              <ChevronDown className="w-3 h-3 opacity-70" />
                            </button>
                          </DropdownMenu.Trigger>
                          <DropdownMenu.Portal>
                            <DropdownMenu.Content
                              align="end"
                              sideOffset={6}
                              className="w-44 p-1 bg-white dark:bg-[#1a1b20] border border-gray-200 dark:border-zinc-800 rounded-xl shadow-xl z-50 animate-in fade-in-0 zoom-in-95 duration-100 text-xs"
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
                                  className={`px-2.5 py-1.5 text-xs rounded-lg flex items-center justify-between cursor-pointer outline-none transition-colors ${
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

                        {/* 2. 单账号刷新与更多菜单 */}
                        <div className="flex items-center gap-0.5">
                          <button
                            onClick={handleRefreshAll}
                            className="p-1.5 rounded-lg border border-gray-200/80 dark:border-white/10 hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-500 dark:text-zinc-400 transition-colors"
                            title={isZh ? '刷新此账号配额' : 'Refresh'}
                          >
                            <RefreshCw className="w-3.5 h-3.5" />
                          </button>

                          <DropdownMenu.Root>
                            <DropdownMenu.Trigger asChild>
                              <button className="p-1.5 rounded-lg border border-gray-200/80 dark:border-white/10 hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-500 dark:text-zinc-400 transition-colors">
                                <MoreVertical className="w-3.5 h-3.5" />
                              </button>
                            </DropdownMenu.Trigger>
                            <DropdownMenu.Portal>
                              <DropdownMenu.Content
                                align="end"
                                sideOffset={6}
                                className="w-36 p-1 bg-white dark:bg-[#1a1b20] border border-gray-200 dark:border-zinc-800 rounded-xl shadow-xl z-50 text-xs"
                              >
                                <DropdownMenu.Item className="px-2.5 py-1.5 rounded-lg cursor-pointer hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-700 dark:text-zinc-300 outline-none flex items-center gap-2">
                                  <Tag className="w-3.5 h-3.5" />
                                  <span>{isZh ? '修改备注' : 'Edit Alias'}</span>
                                </DropdownMenu.Item>
                                <DropdownMenu.Separator className="h-px bg-gray-100 dark:bg-zinc-800 my-1" />
                                <DropdownMenu.Item className="px-2.5 py-1.5 rounded-lg cursor-pointer hover:bg-rose-50 text-rose-600 dark:hover:bg-rose-950/20 dark:text-rose-400 outline-none flex items-center gap-2">
                                  <Trash2 className="w-3.5 h-3.5" />
                                  <span>{isZh ? '删除账号' : 'Delete'}</span>
                                </DropdownMenu.Item>
                              </DropdownMenu.Content>
                            </DropdownMenu.Portal>
                          </DropdownMenu.Root>
                        </div>
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
      {/* 底部吸底控制岛 (Batch Dock & Ingestion) */}
      {/* ============================================================== */}
      <div className="sticky bottom-3 z-30 bg-white/90 dark:bg-[#15161b]/90 backdrop-blur-2xl border border-gray-200/80 dark:border-white/10 rounded-2xl p-2.5 px-5 shadow-lg flex flex-wrap items-center justify-between gap-4">
        {/* 左侧：多选批量运维 */}
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

        {/* 右侧：导入导出与官方登录 */}
        <div className="flex items-center gap-2">
          <button className="px-3 py-1.5 rounded-xl border border-gray-200/80 dark:border-white/10 bg-white dark:bg-[#1a1b20] hover:bg-gray-50 dark:hover:bg-zinc-800 text-xs font-medium text-gray-700 dark:text-zinc-300 flex items-center gap-1.5 transition-colors shadow-xs">
            <Upload className="w-3.5 h-3.5" />
            <span>{isZh ? '导入 JSON' : 'Import'}</span>
          </button>
          <button className="px-3 py-1.5 rounded-xl border border-gray-200/80 dark:border-white/10 bg-white dark:bg-[#1a1b20] hover:bg-gray-50 dark:hover:bg-zinc-800 text-xs font-medium text-gray-700 dark:text-zinc-300 flex items-center gap-1.5 transition-colors shadow-xs">
            <Download className="w-3.5 h-3.5" />
            <span>{isZh ? '备份导出' : 'Export'}</span>
          </button>
          <button className="px-3.5 py-1.5 rounded-xl bg-blue-600 hover:bg-blue-700 text-white text-xs font-semibold flex items-center gap-1.5 shadow-md shadow-blue-500/20 transition-all">
            <Plus className="w-3.5 h-3.5" />
            <span>{isZh ? '官方授权登录' : 'Add Account'}</span>
          </button>
        </div>
      </div>
    </div>
  );
};
