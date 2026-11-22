import React, { useState, useMemo } from 'react';
import {
  TrendingUp,
  Activity,
  Layers,
  Clock,
  HardDrive,
  Cpu,
  ShieldCheck,
  AlertTriangle,
  Search,
  DollarSign,
  Zap,
  X,
  Calendar,
  Sparkles,
  AppWindow,
  Code2,
  Puzzle,
  Terminal,
  FolderGit2,
  CheckCircle2,
  Gauge
} from 'lucide-react';

interface StatsProps {
  lang: 'zh' | 'en';
}

type StatsView = 'insights' | 'ledger';
type TimeRange = 'today' | '3days' | '7days' | '30days';
type HostFilter = 'all' | 'app' | 'ide' | 'plugin' | 'cli';

// 原子级请求流水记录
export interface RequestRecord {
  id: string;
  time: string;
  host: 'app' | 'ide' | 'plugin' | 'cli';
  requestedModel: string;
  physicalModel: string;
  isDowngraded?: boolean;
  inputTokens: number;
  cachedTokens: number;
  outputTokens: number;
  tps: number;
  ttft: number; // 秒
  costSaved: number;
  accountEmail: string;
  workspace: string;
  sessionId: string;
  thinkingTokens?: number;
  toolTokens?: number;
}

export const Stats: React.FC<StatsProps> = ({ lang }) => {
  const isZh = lang === 'zh';

  // 1. 视图切换：宏观多维分析 vs 流量流水账本
  const [activeView, setActiveView] = useState<StatsView>('insights');

  // 2. 过滤器状态
  const [timeRange, setTimeRange] = useState<TimeRange>('today');
  const [hostFilter, setHostFilter] = useState<HostFilter>('all');
  const [searchQuery, setSearchQuery] = useState('');

  // 3. 抽屉选中的单条请求详情
  const [selectedRecord, setSelectedRecord] = useState<RequestRecord | null>(null);

  // 4. Mock 原子级流水数据
  const [records] = useState<RequestRecord[]>([
    {
      id: 'req_9801a2',
      time: '12:08:42',
      host: 'app',
      requestedModel: 'gemini-2.5-pro',
      physicalModel: 'gemini-4-hydrogen-high',
      inputTokens: 11620,
      cachedTokens: 154200,
      outputTokens: 6916,
      tps: 168.4,
      ttft: 2.7,
      costSaved: 0.0808,
      accountEmail: 'ctrler@868666.best',
      workspace: 'antigravity-core',
      sessionId: 'sess_mac_sonoma_001',
      thinkingTokens: 2450,
      toolTokens: 1120,
    },
    {
      id: 'req_9800ff',
      time: '12:02:15',
      host: 'ide',
      requestedModel: 'claude-3-7-sonnet',
      physicalModel: 'gemini-4-hydrogen-high',
      inputTokens: 3940,
      cachedTokens: 148000,
      outputTokens: 8722,
      tps: 172.1,
      ttft: 3.1,
      costSaved: 0.0825,
      accountEmail: 'ctrler@868666.best',
      workspace: 'my-web-platform',
      sessionId: 'sess_mac_sonoma_001',
      thinkingTokens: 4120,
      toolTokens: 320,
    },
    {
      id: 'req_97f901',
      time: '11:45:03',
      host: 'cli',
      requestedModel: 'gemini-2.5-flash',
      physicalModel: 'gemini-4-carbon',
      inputTokens: 2810,
      cachedTokens: 92400,
      outputTokens: 335,
      tps: 97.5,
      ttft: 1.8,
      costSaved: 0.0179,
      accountEmail: 'developer.pro@gmail.com',
      workspace: 'infra-scripts',
      sessionId: 'sess_cli_worker_03',
      thinkingTokens: 0,
      toolTokens: 180,
    },
    {
      id: 'req_97e41b',
      time: '11:12:30',
      host: 'plugin',
      requestedModel: 'claude-3-7-sonnet',
      physicalModel: 'used_claude_conservative',
      isDowngraded: true,
      inputTokens: 20850,
      cachedTokens: 45000,
      outputTokens: 362,
      tps: 48.2,
      ttft: 6.7,
      costSaved: 0.0094,
      accountEmail: 'research.pool1@gmail.com',
      workspace: 'cursor-extension-lab',
      sessionId: 'sess_cursor_49a',
      thinkingTokens: 110,
      toolTokens: 0,
    },
    {
      id: 'req_97d312',
      time: '10:55:18',
      host: 'app',
      requestedModel: 'gemini-2.5-pro',
      physicalModel: 'gemini-4-hydrogen-high',
      inputTokens: 4520,
      cachedTokens: 142000,
      outputTokens: 3120,
      tps: 154.2,
      ttft: 2.9,
      costSaved: 0.0412,
      accountEmail: 'ctrler@868666.best',
      workspace: 'antigravity-core',
      sessionId: 'sess_mac_sonoma_001',
      thinkingTokens: 1240,
      toolTokens: 590,
    },
    {
      id: 'req_97b102',
      time: '10:20:04',
      host: 'ide',
      requestedModel: 'gemini-2.5-pro',
      physicalModel: 'gemini-4-hydrogen-high',
      inputTokens: 1890,
      cachedTokens: 112000,
      outputTokens: 1480,
      tps: 148.9,
      ttft: 2.4,
      costSaved: 0.0245,
      accountEmail: 'sprint.dev@corp.internal',
      workspace: 'core-backend',
      sessionId: 'sess_ide_sprint_05',
      thinkingTokens: 620,
      toolTokens: 110,
    },
  ]);

  // 过滤后的流水列表
  const filteredRecords = useMemo(() => {
    return records.filter((r) => {
      if (hostFilter !== 'all' && r.host !== hostFilter) return false;
      if (!searchQuery.trim()) return true;
      const q = searchQuery.toLowerCase();
      return (
        r.accountEmail.toLowerCase().includes(q) ||
        r.requestedModel.toLowerCase().includes(q) ||
        r.physicalModel.toLowerCase().includes(q) ||
        r.workspace.toLowerCase().includes(q)
      );
    });
  }, [records, hostFilter, searchQuery]);

  return (
    <div className="w-full max-w-6xl mx-auto px-4 py-2 flex flex-col gap-3.5 select-none animate-in fade-in-50 duration-300">
      {/* ============================================================== */}
      {/* 顶部自适应控制器：二级分段选项卡 (宏观分析 vs 流量流水)           */}
      {/* ============================================================== */}
      <div className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl p-2 px-3 shadow-2xs flex flex-wrap items-center justify-between gap-2.5">
        {/* 左侧：Apple 风格分段胶囊控制器 */}
        <div className="inline-flex items-center p-0.5 rounded-lg bg-gray-100/90 dark:bg-white/[0.04] border border-gray-200/50 dark:border-white/5">
          <button
            onClick={() => setActiveView('insights')}
            className={`px-3 py-1 rounded-md text-[11px] font-medium flex items-center gap-1.5 transition-all ${
              activeView === 'insights'
                ? 'bg-white dark:bg-[#202126] text-blue-600 dark:text-blue-400 font-semibold shadow-2xs'
                : 'text-gray-600 dark:text-zinc-400 hover:text-gray-900 dark:hover:text-zinc-100'
            }`}
          >
            <TrendingUp className="w-3.5 h-3.5" />
            <span>{isZh ? '📈 宏观多维分析' : 'Aggregated Insights'}</span>
          </button>
          <button
            onClick={() => setActiveView('ledger')}
            className={`px-3 py-1 rounded-md text-[11px] font-medium flex items-center gap-1.5 transition-all ${
              activeView === 'ledger'
                ? 'bg-white dark:bg-[#202126] text-blue-600 dark:text-blue-400 font-semibold shadow-2xs'
                : 'text-gray-600 dark:text-zinc-400 hover:text-gray-900 dark:hover:text-zinc-100'
            }`}
          >
            <Activity className="w-3.5 h-3.5" />
            <span>{isZh ? '🌊 流量与请求流水' : 'Traffic Ledger'}</span>
            <span className="px-1 py-0 rounded text-[9px] bg-blue-50 text-blue-600 dark:bg-blue-500/15 dark:text-blue-400 font-bold">
              {records.length}
            </span>
          </button>
        </div>

        {/* 右侧：全局统计周期药丸 [今天 | 近3天 | 近7天 | 近30天] */}
        <div className="flex items-center gap-1 text-[11px]">
          <span className="text-[10px] text-gray-400 mr-1 flex items-center gap-1 font-medium">
            <Calendar className="w-3 h-3 opacity-60" />
            <span>{isZh ? '统计周期:' : 'Window:'}</span>
          </span>
          {[
            { id: 'today' as TimeRange, label: isZh ? '今天' : 'Today' },
            { id: '3days' as TimeRange, label: isZh ? '近3天' : '3 Days' },
            { id: '7days' as TimeRange, label: isZh ? '近7天' : '7 Days' },
            { id: '30days' as TimeRange, label: isZh ? '近30天' : '30 Days' },
          ].map((item) => (
            <button
              key={item.id}
              onClick={() => setTimeRange(item.id)}
              className={`px-2 py-0.5 rounded-md text-[10px] font-medium transition-all ${
                timeRange === item.id
                  ? 'bg-blue-50 dark:bg-blue-500/10 text-blue-600 dark:text-blue-400 font-semibold border border-blue-200/60 dark:border-blue-500/20 shadow-2xs'
                  : 'text-gray-500 dark:text-zinc-400 hover:bg-gray-100 dark:hover:bg-zinc-800'
              }`}
            >
              {item.label}
            </button>
          ))}
        </div>
      </div>

      {/* ============================================================== */}
      {/* 视图 A：📈 宏观多维分析 (Aggregated Insights)                     */}
      {/* 5 大核心维度自上而下舒展，数据直击架构师与极客痛点                  */}
      {/* ============================================================== */}
      {activeView === 'insights' && (
        <div className="flex flex-col gap-3.5 animate-in fade-in-30 duration-200">
          {/* 1. 顶部 4 大核心 KPI 晶格看板 */}
          <div className="grid grid-cols-1 md:grid-cols-4 gap-3">
            {/* KPI 1: 总 Token 吞吐 */}
            <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col gap-1">
              <span className="text-[10px] text-gray-500 dark:text-zinc-400 font-medium uppercase tracking-wider flex items-center justify-between">
                <span>{isZh ? '累计 Token 吞吐' : 'Token Throughput'}</span>
                <Sparkles className="w-3.5 h-3.5 text-blue-500" />
              </span>
              <div className="flex items-baseline gap-1.5 mt-0.5">
                <span className="text-xl font-bold tracking-tight text-gray-900 dark:text-zinc-100 font-mono">
                  2.75M
                </span>
                <span className="text-[9.5px] text-emerald-600 dark:text-emerald-400 font-semibold">
                  +18.4%
                </span>
              </div>
              <div className="flex items-center justify-between text-[9.5px] text-gray-400 font-mono mt-1 pt-1.5 border-t border-gray-100 dark:border-white/5">
                <span>输入 390.0K</span>
                <span>输出 17.3K</span>
              </div>
            </div>

            {/* KPI 2: 缓存命中率 */}
            <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col gap-1">
              <span className="text-[10px] text-gray-500 dark:text-zinc-400 font-medium uppercase tracking-wider flex items-center justify-between">
                <span>{isZh ? '全局缓存命中率' : 'Cache Hit Rate'}</span>
                <ShieldCheck className="w-3.5 h-3.5 text-emerald-500" />
              </span>
              <div className="flex items-baseline gap-1.5 mt-0.5">
                <span className="text-xl font-bold tracking-tight text-emerald-600 dark:text-emerald-400 font-mono">
                  85.7%
                </span>
                <span className="text-[9.5px] text-gray-400 font-medium">
                  极佳 (High)
                </span>
              </div>
              <div className="flex items-center justify-between text-[9.5px] text-gray-400 font-mono mt-1 pt-1.5 border-t border-gray-100 dark:border-white/5">
                <span>复用 2.34M 缓存</span>
                <span>节省 89% 延迟</span>
              </div>
            </div>

            {/* KPI 3: 累计节省公有云成本 */}
            <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col gap-1">
              <span className="text-[10px] text-gray-500 dark:text-zinc-400 font-medium uppercase tracking-wider flex items-center justify-between">
                <span>{isZh ? '估算节省官方费用' : 'API Cost Saved'}</span>
                <DollarSign className="w-3.5 h-3.5 text-amber-500" />
              </span>
              <div className="flex items-baseline gap-1.5 mt-0.5">
                <span className="text-xl font-bold tracking-tight text-emerald-600 dark:text-emerald-400 font-mono">
                  $38.51
                </span>
                <span className="text-[9.5px] text-gray-400 font-medium">USD</span>
              </div>
              <div className="flex items-center justify-between text-[9.5px] text-gray-400 font-mono mt-1 pt-1.5 border-t border-gray-100 dark:border-white/5">
                <span>比照公有云标价</span>
                <span>纯本地零代理节流</span>
              </div>
            </div>

            {/* KPI 4: 吞吐性能基准 */}
            <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col gap-1">
              <span className="text-[10px] text-gray-500 dark:text-zinc-400 font-medium uppercase tracking-wider flex items-center justify-between">
                <span>{isZh ? '生成速率 & TTFT' : 'Performance (TPS)'}</span>
                <Zap className="w-3.5 h-3.5 text-purple-500" />
              </span>
              <div className="flex items-baseline gap-1.5 mt-0.5">
                <span className="text-xl font-bold tracking-tight text-purple-600 dark:text-purple-400 font-mono">
                  168.4
                </span>
                <span className="text-[9.5px] text-gray-400 font-medium">tok/s</span>
              </div>
              <div className="flex items-center justify-between text-[9.5px] text-gray-400 font-mono mt-1 pt-1.5 border-t border-gray-100 dark:border-white/5">
                <span>首字中位数 2.8s</span>
                <span>79 次成功会话</span>
              </div>
            </div>
          </div>

          {/* 2. 维度 1：物理真实模型谱系与保真度排行 (揭露暗降) */}
          <div className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl p-3 shadow-2xs flex flex-col gap-2.5">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-1.5">
                <Cpu className="w-3.5 h-3.5 text-blue-500" />
                <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100">
                  {isZh ? '维度 1：真实物理模型谱系与保真度排行' : 'Model Performance & Fidelity'}
                </span>
              </div>
              <span className="text-[9.5px] text-gray-400">
                {isZh ? '还原 Google 底层物理路由代号 · 精准识别暗降' : 'Physical Upstream Fidelity'}
              </span>
            </div>

            <div className="overflow-hidden rounded-lg border border-gray-200/60 dark:border-white/5">
              <table className="w-full text-left text-[10.5px] border-collapse table-fixed">
                <colgroup>
                  <col className="w-[28%]" />
                  <col className="w-[28%]" />
                  <col className="w-[18%]" />
                  <col className="w-[14%]" />
                  <col className="w-[12%]" />
                </colgroup>
                <thead className="bg-gray-50/80 dark:bg-[#16171b]/80 border-b border-gray-200/60 dark:border-white/5 text-gray-500 font-medium">
                  <tr>
                    <th className="px-3 py-1.5">请求模型 (Requested)</th>
                    <th className="px-3 py-1.5">真实物理上游 (Physical)</th>
                    <th className="px-3 py-1.5">平均生成流速</th>
                    <th className="px-3 py-1.5">首字延迟 TTFT</th>
                    <th className="px-3 py-1.5 text-center">保真度状态</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-100 dark:divide-white/5 font-sans">
                  <tr className="hover:bg-gray-50/50 dark:hover:bg-zinc-800/20">
                    <td className="px-3 py-2 font-medium text-gray-800 dark:text-zinc-200">
                      claude-3-7-sonnet
                    </td>
                    <td className="px-3 py-2 font-mono text-[10px] text-purple-600 dark:text-purple-400 font-semibold">
                      gemini-4-hydrogen-high
                    </td>
                    <td className="px-3 py-2 font-mono">
                      <div className="flex items-center gap-1.5">
                        <span className="text-emerald-600 font-bold">225 tok/s</span>
                        <div className="w-12 h-1.5 bg-gray-100 dark:bg-zinc-800 rounded-full overflow-hidden">
                          <div className="w-[92%] h-full bg-emerald-500 rounded-full" />
                        </div>
                      </div>
                    </td>
                    <td className="px-3 py-2 font-mono text-gray-600 dark:text-zinc-400">
                      2.8s
                    </td>
                    <td className="px-3 py-2 text-center">
                      <span className="inline-flex items-center gap-1 px-1.5 py-0.2 rounded text-[9px] bg-emerald-50 text-emerald-600 dark:bg-emerald-500/10 dark:text-emerald-400 font-semibold border border-emerald-200/40">
                        <CheckCircle2 className="w-2.5 h-2.5" />
                        <span>原生保真</span>
                      </span>
                    </td>
                  </tr>

                  <tr className="hover:bg-gray-50/50 dark:hover:bg-zinc-800/20">
                    <td className="px-3 py-2 font-medium text-gray-800 dark:text-zinc-200">
                      gemini-2.5-pro
                    </td>
                    <td className="px-3 py-2 font-mono text-[10px] text-blue-600 dark:text-blue-400 font-semibold">
                      gemini-4-hydrogen-high
                    </td>
                    <td className="px-3 py-2 font-mono">
                      <div className="flex items-center gap-1.5">
                        <span className="text-blue-600 font-bold">168 tok/s</span>
                        <div className="w-12 h-1.5 bg-gray-100 dark:bg-zinc-800 rounded-full overflow-hidden">
                          <div className="w-[74%] h-full bg-blue-500 rounded-full" />
                        </div>
                      </div>
                    </td>
                    <td className="px-3 py-2 font-mono text-gray-600 dark:text-zinc-400">
                      2.7s
                    </td>
                    <td className="px-3 py-2 text-center">
                      <span className="inline-flex items-center gap-1 px-1.5 py-0.2 rounded text-[9px] bg-emerald-50 text-emerald-600 dark:bg-emerald-500/10 dark:text-emerald-400 font-semibold border border-emerald-200/40">
                        <CheckCircle2 className="w-2.5 h-2.5" />
                        <span>原生保真</span>
                      </span>
                    </td>
                  </tr>

                  <tr className="hover:bg-gray-50/50 dark:hover:bg-zinc-800/20 bg-rose-50/30 dark:bg-rose-950/15">
                    <td className="px-3 py-2 font-medium text-gray-800 dark:text-zinc-200">
                      claude-3-7-sonnet
                    </td>
                    <td className="px-3 py-2 font-mono text-[10px] text-rose-600 dark:text-rose-400 font-semibold">
                      used_claude_conservative
                    </td>
                    <td className="px-3 py-2 font-mono">
                      <div className="flex items-center gap-1.5">
                        <span className="text-rose-600 font-bold">48 tok/s</span>
                        <div className="w-12 h-1.5 bg-gray-100 dark:bg-zinc-800 rounded-full overflow-hidden">
                          <div className="w-[24%] h-full bg-rose-500 rounded-full" />
                        </div>
                      </div>
                    </td>
                    <td className="px-3 py-2 font-mono text-rose-600 font-bold">
                      6.7s ⚠️
                    </td>
                    <td className="px-3 py-2 text-center">
                      <span className="inline-flex items-center gap-1 px-1.5 py-0.2 rounded text-[9px] bg-rose-100 text-rose-700 dark:bg-rose-950/50 dark:text-rose-400 font-bold border border-rose-200">
                        <AlertTriangle className="w-2.5 h-2.5" />
                        <span>云端暗降!</span>
                      </span>
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>

          {/* 3. 维度 2 & 维度 3 双列并排 */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            {/* 维度 2：账号归属与成本节约 */}
            <div className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl p-3 shadow-2xs flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-1.5">
                  <HardDrive className="w-3.5 h-3.5 text-emerald-500" />
                  <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100">
                    {isZh ? '维度 2：账号归属与成本节约' : 'Account Attribution & Savings'}
                  </span>
                </div>
                <span className="text-[9px] text-gray-400">精确量化 Token 与美金</span>
              </div>

              <div className="flex flex-col gap-1.5 mt-1">
                {[
                  {
                    email: 'ctrler@868666.best',
                    tag: '🌟 主力',
                    tokens: '1.82M',
                    saved: '$24.10',
                    hit: '98.2%',
                  },
                  {
                    email: 'developer.pro@gmail.com',
                    tag: 'Team Pro',
                    tokens: '620K',
                    saved: '$8.45',
                    hit: '87.4%',
                  },
                  {
                    email: 'research.pool1@gmail.com',
                    tag: 'Claude池',
                    tokens: '310K',
                    saved: '$5.96',
                    hit: '71.5%',
                  },
                ].map((acc) => (
                  <div
                    key={acc.email}
                    className="p-2 rounded-lg bg-gray-50/70 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5 flex items-center justify-between"
                  >
                    <div className="flex flex-col min-w-0 pr-2">
                      <div className="flex items-center gap-1.5">
                        <span className="text-[11px] font-medium text-gray-800 dark:text-zinc-200 truncate">
                          {acc.email}
                        </span>
                        <span className="text-[8.5px] px-1 py-0 rounded bg-gray-200/70 dark:bg-zinc-800 text-gray-600 dark:text-zinc-300">
                          {acc.tag}
                        </span>
                      </div>
                      <span className="text-[9.5px] text-gray-400 mt-0.5 font-mono">
                        消耗 Token: {acc.tokens} · 缓存命中 {acc.hit}
                      </span>
                    </div>

                    <div className="flex flex-col items-end shrink-0">
                      <span className="text-[11px] font-bold text-emerald-600 dark:text-emerald-400 font-mono">
                        已省 {acc.saved}
                      </span>
                      <span className="text-[9px] text-gray-400">USD</span>
                    </div>
                  </div>
                ))}
              </div>
            </div>

            {/* 维度 3：多端宿主与工作区工程分布 */}
            <div className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl p-3 shadow-2xs flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-1.5">
                  <Layers className="w-3.5 h-3.5 text-indigo-500" />
                  <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100">
                    {isZh ? '维度 3：多端宿主与工程用量分布' : 'Host & Workspace Distribution'}
                  </span>
                </div>
                <span className="text-[9px] text-gray-400">4 大靶标与活跃工程</span>
              </div>

              {/* 宿主占比微条 */}
              <div className="flex flex-col gap-1 mt-1">
                <div className="flex items-center justify-between text-[10px] text-gray-600 dark:text-zinc-400">
                  <span className="flex items-center gap-1">
                    <AppWindow className="w-3 h-3 text-blue-500" />
                    <span>原生桌面版 (58%)</span>
                  </span>
                  <span className="flex items-center gap-1">
                    <Code2 className="w-3 h-3 text-indigo-500" />
                    <span>IDE (32%)</span>
                  </span>
                  <span className="flex items-center gap-1">
                    <Puzzle className="w-3 h-3 text-amber-500" />
                    <span>插件 (8%)</span>
                  </span>
                  <span className="flex items-center gap-1">
                    <Terminal className="w-3 h-3 text-cyan-500" />
                    <span>CLI (2%)</span>
                  </span>
                </div>
                {/* 拼接色彩微槽 */}
                <div className="w-full h-2 rounded-full overflow-hidden flex bg-gray-100 dark:bg-zinc-800">
                  <div style={{ width: '58%' }} className="h-full bg-blue-500" />
                  <div style={{ width: '32%' }} className="h-full bg-indigo-500" />
                  <div style={{ width: '8%' }} className="h-full bg-amber-500" />
                  <div style={{ width: '2%' }} className="h-full bg-cyan-500" />
                </div>
              </div>

              {/* 高频工程排行 */}
              <div className="flex flex-col gap-1 mt-1.5">
                {[
                  { name: 'antigravity-core', path: '~/at/crates/am-core', pct: '62%' },
                  { name: 'my-web-platform', path: '~/projects/web-ui', pct: '26%' },
                  { name: 'infra-scripts', path: '~/ops/devops-k8s', pct: '12%' },
                ].map((ws) => (
                  <div
                    key={ws.name}
                    className="flex items-center justify-between text-[10px] p-1.5 rounded-lg bg-gray-50/70 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5"
                  >
                    <div className="flex items-center gap-1.5 truncate pr-2">
                      <FolderGit2 className="w-3 h-3 text-gray-400 shrink-0" />
                      <span className="font-semibold text-gray-800 dark:text-zinc-200 truncate">
                        {ws.name}
                      </span>
                      <span className="text-[9px] text-gray-400 truncate opacity-70 font-mono">
                        ({ws.path})
                      </span>
                    </div>
                    <span className="font-mono font-bold text-gray-700 dark:text-zinc-300 shrink-0">
                      {ws.pct}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          </div>

          {/* 4. 维度 4 & 维度 5 并排 */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            {/* 维度 4：24小时生物钟热力分布 */}
            <div className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl p-3 shadow-2xs flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-1.5">
                  <Clock className="w-3.5 h-3.5 text-amber-500" />
                  <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100">
                    {isZh ? '维度 4：24小时编码生物钟与负载' : 'Circadian Rhythm Heatmap'}
                  </span>
                </div>
                <span className="text-[9px] text-gray-400">产出高峰时段透视</span>
              </div>

              <div className="grid grid-cols-4 gap-1.5 mt-1">
                {[
                  { title: '清晨·上午', time: '06-12h', reqs: '31次', flow: '142 tps', active: false },
                  { title: '午后黄金期', time: '12-18h', reqs: '42次', flow: '178 tps', active: true },
                  { title: '深度思考夜', time: '18-24h', reqs: '18次', flow: '165 tps', active: false },
                  { title: '极客深夜', time: '00-06h', reqs: '4次', flow: '120 tps', active: false },
                ].map((c) => (
                  <div
                    key={c.title}
                    className={`p-2 rounded-lg flex flex-col gap-0.5 border ${
                      c.active
                        ? 'bg-amber-50/60 dark:bg-amber-500/10 border-amber-200/60 dark:border-amber-500/20'
                        : 'bg-gray-50/60 dark:bg-white/[0.03] border-gray-100 dark:border-white/5'
                    }`}
                  >
                    <span className="text-[9.5px] font-semibold text-gray-800 dark:text-zinc-200 truncate">
                      {c.title}
                    </span>
                    <span className="text-[8.5px] text-gray-400 font-mono">{c.time}</span>
                    <span className="text-[11px] font-bold text-gray-900 dark:text-zinc-100 font-mono mt-1">
                      {c.reqs}
                    </span>
                    <span className="text-[8.5px] text-emerald-600 dark:text-emerald-400 font-mono font-semibold">
                      ⚡ {c.flow}
                    </span>
                  </div>
                ))}
              </div>
            </div>

            {/* 维度 5：会话深度与分层水库透视 */}
            <div className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl p-3 shadow-2xs flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-1.5">
                  <Gauge className="w-3.5 h-3.5 text-cyan-500" />
                  <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100">
                    {isZh ? '维度 5：当前活跃会话深度水库' : 'Active Context Reservoir'}
                  </span>
                </div>
                <span className="text-[9px] font-bold text-emerald-600 dark:text-emerald-400 bg-emerald-50 dark:bg-emerald-500/10 px-1.5 py-0.2 rounded border border-emerald-200/40">
                  ✓ 健康 36.1%
                </span>
              </div>

              {/* 类似 macOS 存储容量的多色拼接水库条 */}
              <div className="flex flex-col gap-1 mt-1">
                <div className="flex items-center justify-between text-[10px] text-gray-600 dark:text-zinc-400 font-mono">
                  <span>已用 92.5K / 预算上限 256K</span>
                  <span>剩余 163.5K</span>
                </div>

                <div className="w-full h-2.5 rounded-full overflow-hidden flex bg-gray-100 dark:bg-zinc-800 shadow-inner">
                  <div style={{ width: '38%' }} className="h-full bg-emerald-500" title="缓存复用 35.6K" />
                  <div style={{ width: '3%' }} className="h-full bg-blue-500" title="直接输入 2.5K" />
                  <div style={{ width: '20%' }} className="h-full bg-purple-500" title="深度思考 18.7K" />
                  <div style={{ width: '34%' }} className="h-full bg-amber-500" title="工具调用 33.7K" />
                  <div style={{ width: '5%' }} className="h-full bg-rose-500" title="模型生成 5.7K" />
                </div>

                {/* 图例 */}
                <div className="grid grid-cols-5 gap-1 text-[8.5px] text-gray-400 font-mono mt-1 pt-1 border-t border-gray-100 dark:border-white/5">
                  <span className="flex items-center gap-1 truncate">
                    <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 shrink-0" />
                    <span>缓存 35.6K</span>
                  </span>
                  <span className="flex items-center gap-1 truncate">
                    <span className="w-1.5 h-1.5 rounded-full bg-blue-500 shrink-0" />
                    <span>输入 2.5K</span>
                  </span>
                  <span className="flex items-center gap-1 truncate">
                    <span className="w-1.5 h-1.5 rounded-full bg-purple-500 shrink-0" />
                    <span>思考 18.7K</span>
                  </span>
                  <span className="flex items-center gap-1 truncate">
                    <span className="w-1.5 h-1.5 rounded-full bg-amber-500 shrink-0" />
                    <span>工具 33.7K</span>
                  </span>
                  <span className="flex items-center gap-1 truncate">
                    <span className="w-1.5 h-1.5 rounded-full bg-rose-500 shrink-0" />
                    <span>生成 5.7K</span>
                  </span>
                </div>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* ============================================================== */}
      {/* 视图 B：🌊 流量与请求流水 (Traffic Ledger & Sessions)             */}
      {/* 结构严谨的原子账本，支持点击整行平滑滑出毛玻璃抽屉                  */}
      {/* ============================================================== */}
      {activeView === 'ledger' && (
        <div className="flex flex-col gap-3 animate-in fade-in-30 duration-200 relative">
          {/* 流水筛选检索微型栏 */}
          <div className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl p-2 px-3 shadow-2xs flex flex-wrap items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              <div className="relative w-48">
                <Search className="w-3.5 h-3.5 absolute left-2.5 top-1/2 -translate-y-1/2 text-gray-400" />
                <input
                  type="text"
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  placeholder={isZh ? '搜索模型、账号或工程...' : 'Filter ledger...'}
                  className="w-full pl-8 pr-2.5 py-1 text-[11px] rounded-lg bg-gray-100/80 dark:bg-[#1a1b1f] border border-transparent focus:border-blue-500/50 text-gray-900 dark:text-zinc-100 placeholder-gray-400 outline-none"
                />
              </div>

              {/* 宿主分类选择器 */}
              <div className="inline-flex items-center p-0.5 rounded-lg bg-gray-100 dark:bg-white/[0.04]">
                {[
                  { id: 'all' as HostFilter, label: isZh ? '全部宿主' : 'All Hosts' },
                  { id: 'app' as HostFilter, label: 'App' },
                  { id: 'ide' as HostFilter, label: 'IDE' },
                  { id: 'plugin' as HostFilter, label: isZh ? '插件' : 'Plugin' },
                  { id: 'cli' as HostFilter, label: 'CLI' },
                ].map((item) => (
                  <button
                    key={item.id}
                    onClick={() => setHostFilter(item.id)}
                    className={`px-2 py-0.5 rounded-md text-[10px] font-medium transition-all ${
                      hostFilter === item.id
                        ? 'bg-white dark:bg-[#202126] text-blue-600 dark:text-blue-400 font-semibold shadow-2xs'
                        : 'text-gray-500 hover:text-gray-800 dark:text-zinc-400'
                    }`}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
            </div>

            <div className="text-[10px] text-gray-400 font-mono">
              {isZh ? `已捕获 ${filteredRecords.length} 笔本地请求` : `${filteredRecords.length} requests logged`}
            </div>
          </div>

          {/* 原子账本全景表格 */}
          <div className="bg-white/80 dark:bg-[#121316]/80 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl overflow-hidden shadow-2xs">
            <table className="w-full text-left border-collapse table-fixed select-none">
              <colgroup>
                <col className="w-[70px]" />
                <col className="w-[70px]" />
                <col className="w-[28%]" />
                <col className="w-[18%]" />
                <col className="w-[100px]" />
                <col className="w-[90px]" />
                <col className="w-[70px]" />
                <col className="w-[75px]" />
              </colgroup>
              <thead className="bg-gray-50/80 dark:bg-[#16171b]/80 border-b border-gray-200/60 dark:border-white/5 text-gray-500 text-[10px] font-medium uppercase tracking-wider">
                <tr>
                  <th className="px-3 py-2">时间</th>
                  <th className="px-2 py-2 text-center">应用宿主</th>
                  <th className="px-2.5 py-2">请求模型 ➔ 物理上游</th>
                  <th className="px-2.5 py-2">输入 / 缓存复用</th>
                  <th className="px-2 py-2">输出 Tokens</th>
                  <th className="px-2 py-2">流速 (TPS)</th>
                  <th className="px-2 py-2">首字延迟</th>
                  <th className="px-2 py-2 text-right pr-3">已省成本</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100 dark:divide-white/5 font-sans text-[10.5px]">
                {filteredRecords.map((r) => (
                  <tr
                    key={r.id}
                    onClick={() => setSelectedRecord(r)}
                    className="hover:bg-blue-50/40 dark:hover:bg-blue-500/5 transition-colors cursor-pointer group"
                  >
                    {/* 时间 */}
                    <td className="px-3 py-2 font-mono text-[10px] text-gray-500 dark:text-zinc-400">
                      {r.time}
                    </td>

                    {/* 宿主胶囊 */}
                    <td className="px-2 py-2 text-center">
                      <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[9px] font-semibold bg-gray-100 dark:bg-white/[0.05] text-gray-700 dark:text-zinc-300">
                        {r.host === 'app' ? (
                          <AppWindow className="w-2.5 h-2.5 text-blue-500" />
                        ) : r.host === 'ide' ? (
                          <Code2 className="w-2.5 h-2.5 text-indigo-500" />
                        ) : r.host === 'plugin' ? (
                          <Puzzle className="w-2.5 h-2.5 text-amber-500" />
                        ) : (
                          <Terminal className="w-2.5 h-2.5 text-cyan-500" />
                        )}
                        <span>{r.host.toUpperCase()}</span>
                      </span>
                    </td>

                    {/* 请求模型 ➔ 物理模型 (带暗降标注) */}
                    <td className="px-2.5 py-2 truncate">
                      <div className="flex items-center gap-1 truncate">
                        <span className="font-semibold text-gray-800 dark:text-zinc-200 truncate">
                          {r.requestedModel}
                        </span>
                        <span className="text-gray-400 text-[9px]">➔</span>
                        <span
                          className={`font-mono text-[9.5px] truncate ${
                            r.isDowngraded
                              ? 'text-rose-600 dark:text-rose-400 font-bold'
                              : 'text-purple-600 dark:text-purple-400 font-medium'
                          }`}
                        >
                          {r.physicalModel}
                        </span>
                        {r.isDowngraded && (
                          <span className="px-1 py-0 rounded text-[8px] bg-rose-100 text-rose-700 dark:bg-rose-950/60 dark:text-rose-400 font-bold shrink-0">
                            暗降
                          </span>
                        )}
                      </div>
                    </td>

                    {/* 输入 / 缓存复用 */}
                    <td className="px-2.5 py-2 font-mono text-[10px] text-gray-600 dark:text-zinc-400">
                      <span>{(r.inputTokens / 1000).toFixed(1)}K</span>
                      <span className="text-[9px] text-emerald-600 dark:text-emerald-400 ml-1">
                        ({(r.cachedTokens / 1000).toFixed(0)}K Cache)
                      </span>
                    </td>

                    {/* 输出 Tokens */}
                    <td className="px-2 py-2 font-mono font-semibold text-gray-800 dark:text-zinc-200">
                      {r.outputTokens.toLocaleString()}
                    </td>

                    {/* 流速 TPS */}
                    <td className="px-2 py-2 font-mono">
                      <span
                        className={`font-bold ${
                          r.tps > 150
                            ? 'text-emerald-600 dark:text-emerald-400'
                            : r.tps > 80
                            ? 'text-blue-600 dark:text-blue-400'
                            : 'text-rose-500 font-bold'
                        }`}
                      >
                        ⚡ {r.tps.toFixed(0)} tok/s
                      </span>
                    </td>

                    {/* 首字延迟 */}
                    <td className="px-2 py-2 font-mono text-gray-500 dark:text-zinc-400">
                      ⏱ {r.ttft}s
                    </td>

                    {/* 节省成本 */}
                    <td className="px-2 py-2 text-right pr-3 font-mono font-semibold text-emerald-600 dark:text-emerald-400">
                      +${r.costSaved.toFixed(4)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {/* ============================================================== */}
          {/* 单请求毛玻璃抽屉透视卡片 (Slide-over Inspector Sheet)           */}
          {/* ============================================================== */}
          {selectedRecord && (
            <div className="fixed inset-y-0 right-0 z-50 w-96 bg-white/95 dark:bg-[#16171b]/95 backdrop-blur-2xl border-l border-gray-200/80 dark:border-white/10 shadow-2xl p-4 flex flex-col gap-3.5 animate-in slide-in-from-right duration-200">
              {/* 抽屉顶部标题与关闭 */}
              <div className="flex items-center justify-between pb-2 border-b border-gray-200/60 dark:border-white/10">
                <div className="flex items-center gap-2">
                  <Activity className="w-4 h-4 text-blue-500" />
                  <span className="font-semibold text-xs text-gray-900 dark:text-zinc-100">
                    {isZh ? '请求原子流水透视' : 'Request Inspector'}
                  </span>
                </div>
                <button
                  onClick={() => setSelectedRecord(null)}
                  className="p-1 rounded-lg hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-400 hover:text-gray-700 dark:hover:text-zinc-200 transition-colors"
                >
                  <X className="w-4 h-4" />
                </button>
              </div>

              {/* 核心上下文元数据 */}
              <div className="flex flex-col gap-1.5 p-2.5 rounded-xl bg-gray-50/70 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5 text-[10.5px]">
                <div className="flex items-center justify-between">
                  <span className="text-gray-400">Request ID:</span>
                  <span className="font-mono text-gray-800 dark:text-zinc-200 font-semibold">
                    {selectedRecord.id}
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-gray-400">Session ID:</span>
                  <span className="font-mono text-gray-600 dark:text-zinc-400">
                    {selectedRecord.sessionId}
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-gray-400">Workspace:</span>
                  <span className="font-mono text-blue-600 dark:text-blue-400 font-semibold truncate max-w-[200px]">
                    {selectedRecord.workspace}
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-gray-400">Account:</span>
                  <span className="font-mono text-gray-700 dark:text-zinc-300">
                    {selectedRecord.accountEmail}
                  </span>
                </div>
              </div>

              {/* 物理保真度诊断 */}
              <div className="flex flex-col gap-1 p-2.5 rounded-xl bg-gray-50/70 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5 text-[10.5px]">
                <span className="text-[10px] font-semibold text-gray-600 dark:text-zinc-400 uppercase tracking-wider">
                  物理模型拓扑
                </span>
                <div className="flex items-center justify-between mt-0.5">
                  <span className="text-gray-500">请求模型:</span>
                  <span className="font-semibold text-gray-800 dark:text-zinc-200">
                    {selectedRecord.requestedModel}
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-gray-500">物理真实执行:</span>
                  <span className="font-mono font-bold text-purple-600 dark:text-purple-400">
                    {selectedRecord.physicalModel}
                  </span>
                </div>
                {selectedRecord.isDowngraded ? (
                  <div className="p-1.5 mt-1 rounded-lg bg-rose-50 text-rose-700 dark:bg-rose-950/40 dark:text-rose-400 text-[9.5px] font-semibold flex items-center gap-1 border border-rose-200/60">
                    <AlertTriangle className="w-3 h-3 shrink-0" />
                    <span>检测到云端暗降！上游模型未满足预期高算力代号。</span>
                  </div>
                ) : (
                  <div className="p-1.5 mt-1 rounded-lg bg-emerald-50 text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-400 text-[9.5px] font-semibold flex items-center gap-1 border border-emerald-200/40">
                    <CheckCircle2 className="w-3 h-3 shrink-0" />
                    <span>100% 原生高算力保真路由</span>
                  </div>
                )}
              </div>

              {/* Token 细分水库结构 */}
              <div className="flex flex-col gap-1.5 p-2.5 rounded-xl bg-gray-50/70 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5 text-[10.5px]">
                <span className="text-[10px] font-semibold text-gray-600 dark:text-zinc-400 uppercase tracking-wider">
                  Token 构成拆解
                </span>
                <div className="flex items-center justify-between">
                  <span className="text-gray-500">直接输入 (New Input):</span>
                  <span className="font-mono font-semibold">{selectedRecord.inputTokens}</span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-emerald-600 dark:text-emerald-400 font-medium">缓存命中 (Cache Read):</span>
                  <span className="font-mono font-bold text-emerald-600 dark:text-emerald-400">
                    {selectedRecord.cachedTokens}
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-purple-600 dark:text-purple-400">Thinking 思考 Token:</span>
                  <span className="font-mono font-semibold">{selectedRecord.thinkingTokens ?? 0}</span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-amber-600 dark:text-amber-400">工具调用 Token:</span>
                  <span className="font-mono font-semibold">{selectedRecord.toolTokens ?? 0}</span>
                </div>
                <div className="flex items-center justify-between pt-1 border-t border-gray-100 dark:border-white/5">
                  <span className="font-semibold text-gray-800 dark:text-zinc-200">最终回答输出:</span>
                  <span className="font-mono font-bold text-blue-600 dark:text-blue-400">
                    {selectedRecord.outputTokens}
                  </span>
                </div>
              </div>

              {/* 性能与延迟瀑布流 */}
              <div className="flex flex-col gap-1 p-2.5 rounded-xl bg-gray-50/70 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5 text-[10.5px]">
                <span className="text-[10px] font-semibold text-gray-600 dark:text-zinc-400 uppercase tracking-wider">
                  延迟瀑布流与成本
                </span>
                <div className="flex items-center justify-between mt-0.5">
                  <span className="text-gray-500">首字响应 (TTFT):</span>
                  <span className="font-mono font-bold text-gray-800 dark:text-zinc-200">
                    {selectedRecord.ttft} 秒
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-gray-500">持续流速:</span>
                  <span className="font-mono font-bold text-emerald-600 dark:text-emerald-400">
                    {selectedRecord.tps} tok/s
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-gray-500">本地节省官方费用:</span>
                  <span className="font-mono font-bold text-emerald-600 dark:text-emerald-400">
                    +${selectedRecord.costSaved.toFixed(4)} USD
                  </span>
                </div>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
};
