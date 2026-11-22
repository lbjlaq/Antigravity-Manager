import React, { useState } from 'react';
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
  activeHosts: HostTarget[]; // 支持该账号在多个宿主中被激活
  geminiQuota?: ModelGroupQuota;
  claudeQuota?: ModelGroupQuota;
  argonQuota?: ModelGroupQuota;
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

  // 2. 账号 Mock 数据
  const [accounts, setAccounts] = useState<AccountItem[]>([
    {
      id: '1',
      email: 'ctrler@868666.best',
      alias: '开发主力机 (Master)',
      tier: 'PRO',
      tags: ['🌟 主力', 'VIP', 'Claude池'],
      activeHosts: ['app', 'ide'],
      geminiQuota: {
        fiveHour: { percentage: 100, resetText: '4h 54m' },
        sevenDay: { percentage: 100, resetText: '6d 23h' },
      },
      claudeQuota: {
        fiveHour: { percentage: 100, resetText: '4h 50m' },
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
      geminiQuota: {
        fiveHour: { percentage: 100, resetText: '3h 12m' },
        sevenDay: { percentage: 95, resetText: '4d 08h' },
      },
      claudeQuota: {
        fiveHour: { percentage: 85, resetText: '2h 10m' },
        sevenDay: { percentage: 80, resetText: '4d 20h' },
      },
    },
    {
      id: '3',
      email: 'research.pool1@gmail.com',
      alias: '科研备用池 01',
      tier: 'FREE',
      tags: ['Claude池', '备用'],
      activeHosts: ['plugin'],
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
      geminiQuota: {
        fiveHour: { percentage: 100, resetText: '4h 30m' },
        sevenDay: { percentage: 100, resetText: '6d 12h' },
      },
      argonQuota: {
        sevenDay: { percentage: 90, resetText: '5d 08h' },
      },
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
    if (activeFilter === 'warning') return acc.isForbidden || (acc.geminiQuota?.fiveHour?.percentage ?? 100) < 20;
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

  // 四宫格一键直切：点击某宿主徽章，直接将该账号推向该宿主，或者取消
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
        // 如果宿主已被其他账号占用，将其他账号上的该宿主移除（保持宿主唯一归属）
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

  // 单条 5h/7d 统一进度条渲染组件
  const renderQuotaBar = (
    name: string,
    key: 'gemini' | 'claude' | 'argon',
    quota?: ModelGroupQuota
  ) => {
    if (!quota) return null;
    const { fiveHour, sevenDay } = quota;
    const isClaude = key === 'claude';
    const isArgon = key === 'argon';

    const fiveHourPct = fiveHour?.percentage ?? 100;
    const sevenDayPct = sevenDay?.percentage ?? 100;

    const getPrimaryColor = (pct: number) => {
      if (pct < 20) return 'bg-rose-500';
      if (pct < 50) return 'bg-amber-500';
      if (isClaude) return 'bg-purple-500';
      if (isArgon) return 'bg-amber-500';
      return 'bg-blue-500';
    };

    const getSecondaryColor = (pct: number) => {
      if (pct < 20) return 'bg-rose-500/60';
      if (pct < 50) return 'bg-amber-500/60';
      if (isClaude) return 'bg-indigo-400/60';
      if (isArgon) return 'bg-orange-400/60';
      return 'bg-cyan-400/60';
    };

    return (
      <div className="flex flex-col gap-1 w-full">
        <div className="flex items-center justify-between text-[11px] leading-tight">
          <span className="font-semibold text-gray-700 dark:text-zinc-300 flex items-center gap-1.5">
            <span
              className={`w-1.5 h-1.5 rounded-full ${
                isClaude ? 'bg-purple-500' : isArgon ? 'bg-amber-500' : 'bg-blue-500'
              }`}
            />
            <span className="truncate">{name}</span>
          </span>

          <div className="flex items-center gap-2.5 text-[10px] text-gray-500 dark:text-zinc-400 font-mono">
            {fiveHour && (
              <span>
                5h:{' '}
                <b className={fiveHourPct < 25 ? 'text-rose-500 font-bold' : 'text-gray-700 dark:text-zinc-200'}>
                  {fiveHourPct}%
                </b>{' '}
                <span className="opacity-75">({fiveHour.resetText})</span>
              </span>
            )}
            {sevenDay && (
              <span>
                7d:{' '}
                <b className={sevenDayPct < 25 ? 'text-rose-500 font-bold' : 'text-gray-700 dark:text-zinc-200'}>
                  {sevenDayPct}%
                </b>{' '}
                <span className="opacity-75">({sevenDay.resetText})</span>
              </span>
            )}
          </div>
        </div>

        {/* 统一融合进度条 */}
        <div className="w-full h-1.5 bg-gray-100 dark:bg-zinc-800 rounded-full overflow-hidden flex gap-0.5 p-0.5">
          {fiveHour && (
            <div
              style={{ width: `${fiveHourPct}%` }}
              className={`h-full rounded-full transition-all duration-300 ${getPrimaryColor(fiveHourPct)}`}
              title={`5H 限额: ${fiveHourPct}%`}
            />
          )}
          {sevenDay && (
            <div
              style={{ width: `${sevenDayPct}%` }}
              className={`h-full rounded-full transition-all duration-300 ${getSecondaryColor(sevenDayPct)}`}
              title={`周限额: ${sevenDayPct}%`}
            />
          )}
        </div>
      </div>
    );
  };

  return (
    <div className="w-full max-w-7xl mx-auto px-6 py-4 flex flex-col gap-4 select-none animate-in fade-in-50 duration-300">
      
      {/* 顶部检索与透视栏 */}
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-4 bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-2xl p-4 shadow-xs">
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
      {/* 全新 4 栏式黄金表格：                                            */}
      {/* [账号与标签] | [三组配额统一进度条] | [四宫格快捷切换] | [操作区] */}
      {/* ============================================================== */}
      <div className="bg-white/80 dark:bg-[#121316]/80 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-2xl overflow-hidden shadow-xs relative">
        <div className="overflow-x-auto">
          <table className="w-full text-left text-xs border-collapse">
            {/* 表头 */}
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
                {/* 栏 2：Gemini / Claude / Argon 三大模型额度进度 */}
                <th className="px-4 py-3 font-semibold text-gray-600 dark:text-zinc-300 min-w-[320px] whitespace-nowrap">
                  <span>{isZh ? '模型额度池 (5h / 7d 统一进度)' : 'Quota Pools (5h / 7d Unified)'}</span>
                </th>
                {/* 栏 3：四宫格快捷切换区 */}
                <th className="px-4 py-3 font-semibold text-gray-600 dark:text-zinc-300 w-[170px] whitespace-nowrap text-center">
                  <span>{isZh ? '四宫格多端切号区' : 'Host Matrix (4-Grid)'}</span>
                </th>
                {/* 栏 4：刷新 / 删除操作区 */}
                <th className="px-4 py-3 font-semibold text-gray-600 dark:text-zinc-300 w-[110px] text-center sticky right-0 bg-gray-50/90 dark:bg-[#16171b]/90 backdrop-blur-md shadow-[-12px_0_12px_-12px_rgba(0,0,0,0.1)] dark:shadow-[-12px_0_12px_-12px_rgba(255,255,255,0.05)]">
                  {isZh ? '操作' : 'Actions'}
                </th>
              </tr>
            </thead>

            {/* 表体 */}
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

                    {/* 栏 1：账号与标签 (上下堆叠结构) */}
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
                            <span
                              key={t}
                              className="px-1.5 py-0.2 text-[9px] font-medium rounded bg-gray-100 dark:bg-zinc-800 text-gray-600 dark:text-zinc-300 border border-gray-200/50 dark:border-white/5"
                            >
                              {t}
                            </span>
                          ))}
                          <button className="px-1 py-0.2 text-[9px] rounded text-gray-400 hover:text-blue-600 hover:bg-blue-50 dark:hover:bg-blue-500/10 transition-colors">
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

                    {/* 栏 2：Gemini / Claude / Argon 额度统一进度条 (上下三行堆叠) */}
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

                    {/* 栏 3：四宫格快捷切换区 (Native App / IDE / Plugin / CLI 矩阵点击按钮) */}
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
                                    ? `已在此【${host.label}】激活 (点击可解除)`
                                    : `Active in ${host.label} (Click to unbind)`
                                  : isZh
                                  ? `点击一键切换至【${host.label}】`
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

                    {/* 栏 4：刷新 / 删除等快捷操作区 */}
                    <td className="px-4 py-3 align-middle text-center sticky right-0 bg-white/90 dark:bg-[#121316]/90 backdrop-blur-md shadow-[-12px_0_12px_-12px_rgba(0,0,0,0.1)] dark:shadow-[-12px_0_12px_-12px_rgba(255,255,255,0.05)]">
                      <div className="flex items-center justify-center gap-1">
                        {/* 刷新单账号 */}
                        <button
                          onClick={handleRefreshAll}
                          className="p-1.5 rounded-lg border border-gray-200/80 dark:border-white/10 hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-500 dark:text-zinc-400 transition-colors"
                          title={isZh ? '刷新此账号配额' : 'Refresh'}
                        >
                          <RefreshCw className="w-3.5 h-3.5" />
                        </button>

                        {/* 修改备注 */}
                        <button
                          className="p-1.5 rounded-lg border border-gray-200/80 dark:border-white/10 hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-500 dark:text-zinc-400 transition-colors"
                          title={isZh ? '修改标签与备注' : 'Edit'}
                        >
                          <Tag className="w-3.5 h-3.5" />
                        </button>

                        {/* 删除 */}
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

      {/* 底部吸底控制岛 */}
      <div className="sticky bottom-3 z-30 bg-white/90 dark:bg-[#15161b]/90 backdrop-blur-2xl border border-gray-200/80 dark:border-white/10 rounded-2xl p-2.5 px-5 shadow-lg flex flex-wrap items-center justify-between gap-4">
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
