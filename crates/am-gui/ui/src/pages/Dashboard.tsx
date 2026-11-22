import React, { useState } from 'react';
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
  Coins,
  Cpu,
  Clock,
  Gauge,
  Layers,
  ArrowUpRight
} from 'lucide-react';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';

export type HostTarget = 'app' | 'ide' | 'plugin' | 'cli';

interface DashboardProps {
  lang: 'zh' | 'en';
}

export const Dashboard: React.FC<DashboardProps> = ({ lang }) => {
  const isZh = lang === 'zh';

  // 1. 快捷多端切号状态
  const [selectedHost, setSelectedHost] = useState<HostTarget>('app');
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [justSwitched, setJustSwitched] = useState(false);

  // 模拟数据 (与 am-core 数据契约对齐)
  const [hostAccounts, setHostAccounts] = useState({
    app: { email: 'developer.pro@gmail.com', name: 'Alpha Dev', tag: 'VIP · 主力', claude: 92, gemini: 100, active: true },
    ide: { email: 'work.coding@corp.com', name: 'Cloud Copilot', tag: 'Team Pro', claude: 65, gemini: 88, active: true },
    plugin: { email: 'backup.research@gmail.com', name: 'Res Matrix', tag: 'Claude池', claude: 40, gemini: 95, active: false },
    cli: { email: 'developer.pro@gmail.com', name: 'Alpha Dev', tag: 'VIP · 主力', claude: 92, gemini: 100, active: false },
  });

  const availableAccounts = [
    { email: 'developer.pro@gmail.com', name: 'Alpha Dev', claude: 92, gemini: 100, reset: '4h 12m' },
    { email: 'work.coding@corp.com', name: 'Cloud Copilot', claude: 65, gemini: 88, reset: '2h 45m' },
    { email: 'backup.research@gmail.com', name: 'Res Matrix', claude: 40, gemini: 95, reset: '1h 20m' },
    { email: 'fast.test@gmail.com', name: 'Sprint Runner', claude: 100, gemini: 100, reset: '6h 00m' },
  ];

  // 3. 时间范围选择
  const [timeRange, setTimeRange] = useState<'today' | '7d' | '30d'>('today');

  const currentHostAcc = hostAccounts[selectedHost];

  const handleSwitchAccount = (email: string) => {
    const acc = availableAccounts.find(a => a.email === email);
    if (!acc) return;
    setHostAccounts(prev => ({
      ...prev,
      [selectedHost]: {
        ...prev[selectedHost],
        email: acc.email,
        name: acc.name,
        claude: acc.claude,
        gemini: acc.gemini,
      }
    }));
    setJustSwitched(true);
    setTimeout(() => setJustSwitched(false), 2000);
  };

  const handleSmartSelect = () => {
    // 智能选号：寻找 Claude 配额最高的账号
    const best = [...availableAccounts].sort((a, b) => b.claude - a.claude)[0];
    if (best) handleSwitchAccount(best.email);
  };

  const handleRefresh = () => {
    setIsRefreshing(true);
    setTimeout(() => setIsRefreshing(false), 1200);
  };

  return (
    <div className="w-full max-w-7xl mx-auto px-6 py-4 flex flex-col gap-6 select-none animate-in fade-in-50 duration-300">
      
      {/* 2 × 2 黄金网格便当盒 */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">

        {/* ============================================================== */}
        {/* 卡片 1：快捷多端切号中心 (Quick Switch & Host Matrix) */}
        {/* ============================================================== */}
        <section className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-2xl p-6 flex flex-col justify-between shadow-sm hover:shadow-md transition-shadow duration-200 relative overflow-hidden group">
          {/* 背景微光 */}
          <div className="absolute -right-16 -top-16 w-44 h-44 bg-blue-500/5 dark:bg-blue-400/5 rounded-full blur-3xl pointer-events-none" />

          <div>
            {/* 卡片头部 */}
            <div className="flex items-center justify-between mb-5">
              <div className="flex items-center gap-2.5">
                <div className="w-8 h-8 rounded-xl bg-blue-50 dark:bg-blue-500/10 text-blue-600 dark:text-blue-400 flex items-center justify-center font-bold text-sm">
                  <Zap className="w-4 h-4" />
                </div>
                <div>
                  <h3 className="text-sm font-semibold text-gray-900 dark:text-zinc-100">
                    {isZh ? '快捷多端切号中心' : 'Quick Switch & Host Matrix'}
                  </h3>
                  <p className="text-[11px] text-gray-500 dark:text-zinc-400">
                    {isZh ? '直接向目标宿主注入凭证，秒级无感热切换' : 'Inject credentials directly into targets with 0 proxy overhead'}
                  </p>
                </div>
              </div>

              {/* 智能选号与保鲜刷新 */}
              <div className="flex items-center gap-2">
                <button
                  onClick={handleSmartSelect}
                  className="px-2.5 py-1.5 text-xs font-medium rounded-lg bg-blue-50 hover:bg-blue-100 text-blue-600 dark:bg-blue-500/10 dark:hover:bg-blue-500/20 dark:text-blue-400 transition-colors flex items-center gap-1.5"
                  title={isZh ? '智能选取最高配额账号' : 'Auto select best quota account'}
                >
                  <Sparkles className="w-3.5 h-3.5" />
                  <span>{isZh ? '智能选号' : 'Smart Pick'}</span>
                </button>
                <button
                  onClick={handleRefresh}
                  disabled={isRefreshing}
                  className="p-1.5 text-xs rounded-lg text-gray-500 hover:text-gray-900 dark:text-zinc-400 dark:hover:text-zinc-100 hover:bg-gray-100 dark:hover:bg-zinc-800 transition-colors"
                  title={isZh ? '刷新当前宿主凭证' : 'Refresh token'}
                >
                  <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin text-blue-500' : ''}`} />
                </button>
              </div>
            </div>

            {/* 4 宿主状态胶囊选择器 */}
            <div className="grid grid-cols-4 gap-2 p-1 bg-gray-100/80 dark:bg-[#1a1b1f] rounded-xl mb-5">
              {[
                { id: 'app' as HostTarget, label: isZh ? '原生 App' : 'Native App', icon: AppWindow, active: hostAccounts.app.active },
                { id: 'ide' as HostTarget, label: isZh ? 'IDE 定制' : 'IDE Fork', icon: Code2, active: hostAccounts.ide.active },
                { id: 'plugin' as HostTarget, label: isZh ? 'ACP 插件' : 'Extension', icon: Puzzle, active: hostAccounts.plugin.active },
                { id: 'cli' as HostTarget, label: isZh ? 'agy CLI' : 'Core CLI', icon: Terminal, active: hostAccounts.cli.active },
              ].map((host) => {
                const Icon = host.icon;
                const isSelected = selectedHost === host.id;
                return (
                  <button
                    key={host.id}
                    onClick={() => setSelectedHost(host.id)}
                    className={`py-2 px-2 rounded-lg flex flex-col items-center justify-center gap-1 text-xs font-medium transition-all duration-150 relative ${
                      isSelected
                        ? 'bg-white dark:bg-[#25262c] text-gray-900 dark:text-zinc-100 shadow-sm'
                        : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                    }`}
                  >
                    <div className="flex items-center gap-1.5">
                      <Icon className="w-3.5 h-3.5" />
                      <span className="truncate">{host.label}</span>
                    </div>
                    {/* 心跳呼吸绿点 / 灰点 */}
                    <div className="flex items-center gap-1 text-[10px]">
                      <span className={`w-1.5 h-1.5 rounded-full ${host.active ? 'bg-emerald-500 shadow-[0_0_6px_rgba(16,185,129,0.5)]' : 'bg-gray-300 dark:bg-zinc-600'}`} />
                      <span className="text-[10px] scale-90 text-gray-400 dark:text-zinc-500">
                        {host.active ? (isZh ? '运行中' : 'Live') : (isZh ? '离线' : 'Idle')}
                      </span>
                    </div>
                  </button>
                );
              })}
            </div>

            {/* 当前目标宿主绑定的账号卡片 */}
            <div className="p-4 rounded-xl border border-gray-200/60 dark:border-white/5 bg-gray-50/50 dark:bg-[#16171b]/60 flex items-center justify-between gap-4">
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2 mb-1">
                  <span className="text-xs font-bold text-gray-800 dark:text-zinc-200 truncate">
                    {currentHostAcc.email}
                  </span>
                  <span className="px-1.5 py-0.5 text-[10px] font-semibold rounded bg-blue-100/70 text-blue-700 dark:bg-blue-500/20 dark:text-blue-300">
                    {currentHostAcc.tag}
                  </span>
                </div>
                <div className="flex items-center gap-3 text-xs text-gray-500 dark:text-zinc-400">
                  <span>{currentHostAcc.name}</span>
                  <span>·</span>
                  <div className="flex items-center gap-1">
                    <span className="text-purple-600 dark:text-purple-400 font-medium">Claude:</span>
                    <span className="font-semibold text-gray-700 dark:text-zinc-200">{currentHostAcc.claude}%</span>
                  </div>
                  <span>·</span>
                  <div className="flex items-center gap-1">
                    <span className="text-blue-600 dark:text-blue-400 font-medium">Gemini:</span>
                    <span className="font-semibold text-gray-700 dark:text-zinc-200">{currentHostAcc.gemini}%</span>
                  </div>
                </div>
              </div>

              {/* 快捷切换下拉 */}
              <DropdownMenu.Root>
                <DropdownMenu.Trigger asChild>
                  <button className="px-3 py-2 rounded-xl bg-gray-900 text-white hover:bg-gray-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-white text-xs font-medium inline-flex items-center gap-1.5 shadow-sm transition-all duration-150 shrink-0">
                    <span>{justSwitched ? (isZh ? '已切换 ✓' : 'Switched ✓') : (isZh ? '切换账号' : 'Switch')}</span>
                    <ChevronDown className="w-3.5 h-3.5 opacity-70" />
                  </button>
                </DropdownMenu.Trigger>
                <DropdownMenu.Portal>
                  <DropdownMenu.Content
                    align="end"
                    sideOffset={6}
                    className="w-64 p-1.5 bg-white dark:bg-[#1a1b20] border border-gray-200 dark:border-zinc-800 rounded-xl shadow-xl z-50 animate-in fade-in-0 zoom-in-95 duration-100"
                  >
                    <div className="px-2.5 py-1.5 text-[11px] font-semibold text-gray-400 dark:text-zinc-500">
                      {isZh ? '按可用配额降序推荐' : 'Recommended by quota'}
                    </div>
                    {availableAccounts.map((acc) => (
                      <DropdownMenu.Item
                        key={acc.email}
                        onClick={() => handleSwitchAccount(acc.email)}
                        className={`p-2 rounded-lg text-xs flex items-center justify-between cursor-pointer outline-none transition-colors ${
                          acc.email === currentHostAcc.email
                            ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 font-semibold'
                            : 'text-gray-700 dark:text-zinc-300 hover:bg-gray-100 dark:hover:bg-zinc-800/80'
                        }`}
                      >
                        <div className="min-w-0">
                          <p className="truncate font-medium">{acc.email}</p>
                          <div className="flex items-center gap-2 text-[10px] opacity-75 mt-0.5">
                            <span className="text-purple-500 font-semibold">C: {acc.claude}%</span>
                            <span className="text-blue-500 font-semibold">G: {acc.gemini}%</span>
                            <span>重置: {acc.reset}</span>
                          </div>
                        </div>
                        {acc.email === currentHostAcc.email && <CheckCircle2 className="w-4 h-4 text-blue-600 shrink-0 ml-2" />}
                      </DropdownMenu.Item>
                    ))}
                  </DropdownMenu.Content>
                </DropdownMenu.Portal>
              </DropdownMenu.Root>
            </div>
          </div>

          {/* 底部保障提示 */}
          <div className="mt-4 pt-3 border-t border-gray-100 dark:border-white/5 flex items-center justify-between text-[11px] text-gray-400 dark:text-zinc-500">
            <span className="inline-flex items-center gap-1.5">
              <ShieldCheck className="w-3.5 h-3.5 text-emerald-500" />
              {isZh ? '独立机器码隔离 · 免封禁免串号' : 'Hardware fingerprint isolated'}
            </span>
            <span className="font-mono text-[10px]">SQLite 零代理写入</span>
          </div>
        </section>

        {/* ============================================================== */}
        {/* 卡片 2：全局配额池健康度 (Global Quota Pools & Reserve) */}
        {/* ============================================================== */}
        <section className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-2xl p-6 flex flex-col justify-between shadow-sm hover:shadow-md transition-shadow duration-200 relative overflow-hidden">
          <div className="absolute -right-16 -top-16 w-44 h-44 bg-purple-500/5 dark:bg-purple-400/5 rounded-full blur-3xl pointer-events-none" />

          <div>
            {/* 卡片头部 */}
            <div className="flex items-center justify-between mb-5">
              <div className="flex items-center gap-2.5">
                <div className="w-8 h-8 rounded-xl bg-purple-50 dark:bg-purple-500/10 text-purple-600 dark:text-purple-400 flex items-center justify-center font-bold text-sm">
                  <Gauge className="w-4 h-4" />
                </div>
                <div>
                  <h3 className="text-sm font-semibold text-gray-900 dark:text-zinc-100">
                    {isZh ? '全局配额池健康度' : 'Global Quota Pools & Reserve'}
                  </h3>
                  <p className="text-[11px] text-gray-500 dark:text-zinc-400">
                    {isZh ? '宏观掌控账号军团弹药库，消除额度焦虑' : 'High-level assets overview across your entire account fleet'}
                  </p>
                </div>
              </div>

              {/* 账号可用数胶囊 */}
              <div className="px-2.5 py-1 rounded-full bg-emerald-50 text-emerald-600 dark:bg-emerald-500/10 dark:text-emerald-400 text-xs font-semibold flex items-center gap-1.5">
                <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse" />
                <span>39 / 41 {isZh ? '正常' : 'Healthy'}</span>
              </div>
            </div>

            {/* 双配额进度条卡片 */}
            <div className="grid grid-cols-2 gap-3 mb-5">
              {/* Claude 配额池 */}
              <div className="p-3.5 rounded-xl bg-gradient-to-br from-purple-50/50 to-pink-50/30 dark:from-purple-950/20 dark:to-pink-950/10 border border-purple-100/60 dark:border-purple-500/10">
                <div className="flex items-center justify-between text-xs mb-1.5">
                  <span className="font-semibold text-purple-900 dark:text-purple-300">Claude 3.7 / 3.5</span>
                  <span className="font-bold text-purple-600 dark:text-purple-400">78.4%</span>
                </div>
                <div className="w-full h-2 bg-purple-100 dark:bg-purple-950/60 rounded-full overflow-hidden">
                  <div className="h-full bg-gradient-to-r from-purple-500 to-indigo-500 rounded-full transition-all duration-500" style={{ width: '78.4%' }} />
                </div>
                <div className="mt-2 text-[10px] text-purple-700/70 dark:text-purple-400/60 flex justify-between">
                  <span>{isZh ? '加权可用' : 'Weighted pool'}</span>
                  <span>32 账号充沛</span>
                </div>
              </div>

              {/* Gemini 配额池 */}
              <div className="p-3.5 rounded-xl bg-gradient-to-br from-blue-50/50 to-cyan-50/30 dark:from-blue-950/20 dark:to-cyan-950/10 border border-blue-100/60 dark:border-blue-500/10">
                <div className="flex items-center justify-between text-xs mb-1.5">
                  <span className="font-semibold text-blue-900 dark:text-blue-300">Gemini 2.5 Pro</span>
                  <span className="font-bold text-blue-600 dark:text-blue-400">92.1%</span>
                </div>
                <div className="w-full h-2 bg-blue-100 dark:bg-blue-950/60 rounded-full overflow-hidden">
                  <div className="h-full bg-gradient-to-r from-blue-500 to-cyan-400 rounded-full transition-all duration-500" style={{ width: '92.1%' }} />
                </div>
                <div className="mt-2 text-[10px] text-blue-700/70 dark:text-blue-400/60 flex justify-between">
                  <span>{isZh ? '极高富余' : 'Ultra surplus'}</span>
                  <span>38 账号满载</span>
                </div>
              </div>
            </div>

            {/* 账号阵列四态标签流 */}
            <div className="grid grid-cols-4 gap-2 text-center text-xs">
              <div className="p-2 rounded-lg bg-gray-50 dark:bg-[#17181c] border border-gray-100 dark:border-white/5">
                <p className="text-[10px] text-gray-400 dark:text-zinc-500">{isZh ? '正常可用' : 'Active'}</p>
                <p className="text-sm font-bold text-emerald-600 dark:text-emerald-400 mt-0.5">39</p>
              </div>
              <div className="p-2 rounded-lg bg-gray-50 dark:bg-[#17181c] border border-gray-100 dark:border-white/5">
                <p className="text-[10px] text-gray-400 dark:text-zinc-500">{isZh ? '额度告警' : 'Low Quota'}</p>
                <p className="text-sm font-bold text-amber-500 mt-0.5">2</p>
              </div>
              <div className="p-2 rounded-lg bg-gray-50 dark:bg-[#17181c] border border-gray-100 dark:border-white/5">
                <p className="text-[10px] text-gray-400 dark:text-zinc-500">{isZh ? '异常封禁' : 'Banned'}</p>
                <p className="text-sm font-bold text-rose-500 mt-0.5">0</p>
              </div>
              <div className="p-2 rounded-lg bg-gray-50 dark:bg-[#17181c] border border-gray-100 dark:border-white/5">
                <p className="text-[10px] text-gray-400 dark:text-zinc-500">{isZh ? '主动禁用' : 'Disabled'}</p>
                <p className="text-sm font-bold text-gray-400 dark:text-zinc-500 mt-0.5">0</p>
              </div>
            </div>
          </div>

          {/* 最早额度恢复雷达 */}
          <div className="mt-4 pt-3 border-t border-gray-100 dark:border-white/5 flex items-center justify-between text-[11px] text-gray-500 dark:text-zinc-400">
            <span className="inline-flex items-center gap-1.5 truncate">
              <Clock className="w-3.5 h-3.5 text-blue-500 shrink-0" />
              <span>{isZh ? '最早额度恢复: 1小时 15分后 (user3 · Claude 恢复至 100%)' : 'Next Reset: In 1h 15m (user3 · Claude -> 100%)'}</span>
            </span>
            <span className="text-blue-600 dark:text-blue-400 hover:underline cursor-pointer font-medium shrink-0 ml-2">
              {isZh ? '查看明细' : 'Details'}
            </span>
          </div>
        </section>

        {/* ============================================================== */}
        {/* 卡片 3：全局 Token 用量与成本看板 (Global Token & Cost Metrics) */}
        {/* ============================================================== */}
        <section className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-2xl p-6 flex flex-col justify-between shadow-sm hover:shadow-md transition-shadow duration-200 relative overflow-hidden">
          <div className="absolute -right-16 -top-16 w-44 h-44 bg-emerald-500/5 dark:bg-emerald-400/5 rounded-full blur-3xl pointer-events-none" />

          <div>
            {/* 卡片头部与时间筛选药丸 */}
            <div className="flex items-center justify-between mb-5">
              <div className="flex items-center gap-2.5">
                <div className="w-8 h-8 rounded-xl bg-emerald-50 dark:bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 flex items-center justify-center font-bold text-sm">
                  <Coins className="w-4 h-4" />
                </div>
                <div>
                  <h3 className="text-sm font-semibold text-gray-900 dark:text-zinc-100">
                    {isZh ? '全局 Token 用量与成本看板' : 'Global Token & Cost Metrics'}
                  </h3>
                  <p className="text-[11px] text-gray-500 dark:text-zinc-400">
                    {isZh ? '基于本地 SQLite 零代理分析，呈现真实生产力与费用节省' : 'Local zero-proxy ledger metrics and cumulative savings'}
                  </p>
                </div>
              </div>

              {/* 时间筛选 */}
              <div className="flex items-center bg-gray-100 dark:bg-[#1a1b1f] rounded-lg p-0.5 text-[11px] font-medium">
                {(['today', '7d', '30d'] as const).map((r) => (
                  <button
                    key={r}
                    onClick={() => setTimeRange(r)}
                    className={`px-2 py-1 rounded-md transition-colors ${
                      timeRange === r
                        ? 'bg-white dark:bg-[#25262c] text-gray-900 dark:text-zinc-100 shadow-xs font-semibold'
                        : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                    }`}
                  >
                    {r === 'today' ? (isZh ? '今日' : 'Today') : r === '7d' ? (isZh ? '近7天' : '7 Days') : (isZh ? '近30天' : '30 Days')}
                  </button>
                ))}
              </div>
            </div>

            {/* 核心 4 大指标数据方块 */}
            <div className="grid grid-cols-2 gap-3 mb-4">
              {/* 方块 1：总吞吐 */}
              <div className="p-3.5 rounded-xl bg-gray-50/80 dark:bg-[#17181c] border border-gray-100 dark:border-white/5">
                <div className="flex items-center justify-between text-xs text-gray-500 dark:text-zinc-400 mb-1">
                  <span>{isZh ? '总 Token 吞吐' : 'Total Tokens'}</span>
                  <ArrowUpRight className="w-3.5 h-3.5 text-gray-400" />
                </div>
                <div className="text-xl font-bold tracking-tight text-gray-900 dark:text-zinc-100">
                  2.75M
                </div>
                <div className="mt-1 text-[10px] text-gray-400 dark:text-zinc-500 truncate">
                  {isZh ? '输入 390.0K · 输出 17.3K' : 'Prompt 390K · Gen 17.3K'}
                </div>
              </div>

              {/* 方块 2：缓存命中率 */}
              <div className="p-3.5 rounded-xl bg-gray-50/80 dark:bg-[#17181c] border border-gray-100 dark:border-white/5">
                <div className="flex items-center justify-between text-xs text-gray-500 dark:text-zinc-400 mb-1">
                  <span>{isZh ? '上下文缓存命中率' : 'Cache Hit Rate'}</span>
                  <span className="w-2 h-2 rounded-full bg-emerald-500" />
                </div>
                <div className="text-xl font-bold tracking-tight text-emerald-600 dark:text-emerald-400">
                  85.7%
                </div>
                <div className="mt-1 text-[10px] text-emerald-600/80 dark:text-emerald-400/80 truncate">
                  {isZh ? '大幅压降上游首字延迟' : 'Massive latency reduction'}
                </div>
              </div>

              {/* 方块 3：费用节省 */}
              <div className="p-3.5 rounded-xl bg-gray-50/80 dark:bg-[#17181c] border border-gray-100 dark:border-white/5">
                <div className="flex items-center justify-between text-xs text-gray-500 dark:text-zinc-400 mb-1">
                  <span>{isZh ? '估算节省费用' : 'Estimated Saved'}</span>
                  <span className="text-[10px] font-semibold text-emerald-600">ROI 8.4x</span>
                </div>
                <div className="text-xl font-bold tracking-tight text-emerald-600 dark:text-emerald-400">
                  $38.51 <span className="text-xs font-normal text-gray-400">USD</span>
                </div>
                <div className="mt-1 text-[10px] text-gray-400 dark:text-zinc-500 truncate">
                  {isZh ? '对比官方云端按量标准' : 'vs standard cloud billing'}
                </div>
              </div>

              {/* 方块 4：请求性能 */}
              <div className="p-3.5 rounded-xl bg-gray-50/80 dark:bg-[#17181c] border border-gray-100 dark:border-white/5">
                <div className="flex items-center justify-between text-xs text-gray-500 dark:text-zinc-400 mb-1">
                  <span>{isZh ? '平均生成流速' : 'Avg Throughput'}</span>
                  <TrendingUp className="w-3.5 h-3.5 text-blue-500" />
                </div>
                <div className="text-xl font-bold tracking-tight text-blue-600 dark:text-blue-400">
                  118.6 <span className="text-xs font-normal text-gray-400">tok/s</span>
                </div>
                <div className="mt-1 text-[10px] text-gray-400 dark:text-zinc-500 truncate">
                  {isZh ? '首字延迟中位数 2.8s · 79次' : 'Median TTFT 2.8s · 79 reqs'}
                </div>
              </div>
            </div>
          </div>

          {/* 底部 SQLite 统计状态 */}
          <div className="mt-2 pt-3 border-t border-gray-100 dark:border-white/5 flex items-center justify-between text-[11px] text-gray-400 dark:text-zinc-500">
            <span>{isZh ? '数据来源于 local.db 会话日志' : 'Aggregated from local session logs'}</span>
            <span className="text-blue-600 dark:text-blue-400 hover:underline cursor-pointer font-medium">
              {isZh ? '前往深入统计 →' : 'View Insights →'}
            </span>
          </div>
        </section>

        {/* ============================================================== */}
        {/* 卡片 4：当前活跃会话深度与流速 (Active Session Depth & Live Stream) */}
        {/* ============================================================== */}
        <section className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-2xl p-6 flex flex-col justify-between shadow-sm hover:shadow-md transition-shadow duration-200 relative overflow-hidden">
          <div className="absolute -right-16 -top-16 w-44 h-44 bg-cyan-500/5 dark:bg-cyan-400/5 rounded-full blur-3xl pointer-events-none" />

          <div>
            {/* 卡片头部与动态心跳药丸 */}
            <div className="flex items-center justify-between mb-5">
              <div className="flex items-center gap-2.5">
                <div className="w-8 h-8 rounded-xl bg-cyan-50 dark:bg-cyan-500/10 text-cyan-600 dark:text-cyan-400 flex items-center justify-center font-bold text-sm">
                  <Cpu className="w-4 h-4" />
                </div>
                <div>
                  <h3 className="text-sm font-semibold text-gray-900 dark:text-zinc-100">
                    {isZh ? '当前活跃会话深度与流速' : 'Active Session Depth & Live Stream'}
                  </h3>
                  <p className="text-[11px] text-gray-500 dark:text-zinc-400">
                    {isZh ? '上下文窗口分层水位与实时物理模型保真度' : 'Realtime context reservoir & physical model fidelity'}
                  </p>
                </div>
              </div>

              {/* 实时 TPS 呼吸药丸 */}
              <div className="px-2.5 py-1 rounded-full bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 text-xs font-semibold flex items-center gap-1.5">
                <Zap className="w-3.5 h-3.5 animate-pulse text-amber-500" />
                <span className="font-mono">163 tok/s</span>
              </div>
            </div>

            {/* 会话水位进度条 */}
            <div className="mb-4">
              <div className="flex items-center justify-between text-xs mb-2">
                <span className="font-semibold text-gray-800 dark:text-zinc-200">
                  {isZh ? '上下文占用率' : 'Context Window'}: 92.5K / 256K (36.1%)
                </span>
                <span className="text-[11px] text-emerald-600 dark:text-emerald-400 font-medium">
                  {isZh ? '剩余 163.5K · 极其充裕' : '163.5K Free · Healthy'}
                </span>
              </div>

              {/* 多彩分层水库进度条 */}
              <div className="w-full h-3 bg-gray-100 dark:bg-zinc-800 rounded-full overflow-hidden flex shadow-inner">
                <div style={{ width: '13.9%' }} className="bg-emerald-500 h-full" title="Cache Read: 35.6K" />
                <div style={{ width: '1.0%' }} className="bg-blue-500 h-full" title="Direct Prompt: 2.5K" />
                <div style={{ width: '7.3%' }} className="bg-purple-500 h-full" title="Thinking: 18.7K" />
                <div style={{ width: '13.1%' }} className="bg-amber-400 h-full" title="Tool Stream: 33.7K" />
                <div style={{ width: '2.2%' }} className="bg-rose-500 h-full" title="Model Output: 5.7K" />
              </div>

              {/* 分层图例 */}
              <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5 mt-2.5 text-[10px] text-gray-500 dark:text-zinc-400">
                <span className="inline-flex items-center gap-1">
                  <span className="w-2 h-2 rounded-full bg-emerald-500" />
                  <span>缓存命中 (35.6K)</span>
                </span>
                <span className="inline-flex items-center gap-1">
                  <span className="w-2 h-2 rounded-full bg-blue-500" />
                  <span>提示词 (2.5K)</span>
                </span>
                <span className="inline-flex items-center gap-1">
                  <span className="w-2 h-2 rounded-full bg-purple-500" />
                  <span>思考推理 (18.7K)</span>
                </span>
                <span className="inline-flex items-center gap-1">
                  <span className="w-2 h-2 rounded-full bg-amber-400" />
                  <span>工具调用 (33.7K)</span>
                </span>
                <span className="inline-flex items-center gap-1">
                  <span className="w-2 h-2 rounded-full bg-rose-500" />
                  <span>生成文本 (5.7K)</span>
                </span>
              </div>
            </div>

            {/* 真实物理模型保真度指示器 */}
            <div className="p-3 rounded-xl bg-gray-50 dark:bg-[#17181c] border border-gray-100 dark:border-white/5 flex items-center justify-between text-xs">
              <div className="flex items-center gap-2">
                <Layers className="w-4 h-4 text-gray-400 dark:text-zinc-500" />
                <span className="text-gray-500 dark:text-zinc-400">{isZh ? '物理执行模型:' : 'Physical Model:'}</span>
                <span className="font-mono font-semibold text-gray-900 dark:text-zinc-100">gemini-4-hydrogen</span>
              </div>
              <span className="px-2 py-0.5 rounded text-[10px] font-semibold bg-emerald-100/70 text-emerald-700 dark:bg-emerald-500/20 dark:text-emerald-300">
                {isZh ? '无暗降 · 100% 保真' : '100% Fidelity'}
              </span>
            </div>
          </div>

          {/* 底部会话健康诊断 */}
          <div className="mt-4 pt-3 border-t border-gray-100 dark:border-white/5 flex items-center justify-between text-[11px] text-gray-400 dark:text-zinc-500">
            <span className="text-emerald-600 dark:text-emerald-400 font-medium">
              ✓ {isZh ? '当前处于安全水位，可顺畅继续多轮对话' : 'Safe reservoir level, ready for complex tasks'}
            </span>
            <span className="font-mono text-[10px]">TPS Watcher: Live</span>
          </div>
        </section>

      </div>
    </div>
  );
};
