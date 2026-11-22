import React, { useState, useMemo } from 'react';
import {
  TrendingUp,
  Activity,
  Layers,
  Sparkles,
  ShieldCheck,
  DollarSign,
  Zap,
  Calendar,
  CheckCircle2,
  AlertTriangle,
  Search,
  AppWindow,
  Code2,
  Puzzle,
  Terminal,
  X
} from 'lucide-react';

interface StatsProps {
  lang: 'zh' | 'en';
}

type StatsView = 'insights' | 'ledger';
type TimeRange = 'today' | '7days' | '30days';
type MetricMetric = 'tokens' | 'cost';
type HostFilter = 'all' | 'app' | 'ide' | 'plugin' | 'cli';

// 趋势图数据点
interface HourlyDataPoint {
  time: string;
  tokens: number;
  cost: number;
  tps: number;
  cacheRate: number;
}

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
  ttft: number;
  costSaved: number;
  accountEmail: string;
  workspace: string;
  sessionId: string;
  thinkingTokens?: number;
  toolTokens?: number;
}

export const Stats: React.FC<StatsProps> = ({ lang }) => {
  const isZh = lang === 'zh';

  // 1. 视图切换
  const [activeView, setActiveView] = useState<StatsView>('insights');
  const [timeRange, setTimeRange] = useState<TimeRange>('today');
  const [activeMetric, setActiveMetric] = useState<MetricMetric>('tokens');

  // 2. 趋势图鼠标交互悬停状态
  const [hoveredPointIndex, setHoveredPointIndex] = useState<number | null>(null);

  // 3. 流水过滤与抽屉
  const [hostFilter, setHostFilter] = useState<HostFilter>('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedRecord, setSelectedRecord] = useState<RequestRecord | null>(null);

  // 趋势图 Mock 数据 (24 小时时间波形)
  const hourlyData: HourlyDataPoint[] = useMemo(() => {
    return [
      { time: '00:00', tokens: 42000, cost: 0.58, tps: 110, cacheRate: 78 },
      { time: '02:00', tokens: 18000, cost: 0.25, tps: 95, cacheRate: 82 },
      { time: '04:00', tokens: 8000, cost: 0.11, tps: 88, cacheRate: 85 },
      { time: '06:00', tokens: 25000, cost: 0.35, tps: 120, cacheRate: 80 },
      { time: '08:00', tokens: 95000, cost: 1.32, tps: 145, cacheRate: 84 },
      { time: '10:00', tokens: 210000, cost: 2.94, tps: 172, cacheRate: 88 },
      { time: '12:00', tokens: 185000, cost: 2.59, tps: 165, cacheRate: 86 },
      { time: '14:00', tokens: 290000, cost: 4.06, tps: 184, cacheRate: 91 },
      { time: '16:00', tokens: 340000, cost: 4.76, tps: 178, cacheRate: 89 },
      { time: '18:00', tokens: 220000, cost: 3.08, tps: 162, cacheRate: 87 },
      { time: '20:00', tokens: 280000, cost: 3.92, tps: 175, cacheRate: 90 },
      { time: '22:00', tokens: 165000, cost: 2.31, tps: 154, cacheRate: 84 },
    ];
  }, []);

  // 计算趋势图 SVG 坐标与 Bézier 路径
  const { pathD, areaD, points } = useMemo(() => {
    const vals = hourlyData.map((d) => (activeMetric === 'tokens' ? d.tokens : d.cost));
    const max = Math.max(...vals, 1);
    const width = 800;
    const height = 180;
    const paddingX = 30;
    const paddingY = 20;
    const chartW = width - paddingX * 2;
    const chartH = height - paddingY * 2;

    const coords = hourlyData.map((d, i) => {
      const val = activeMetric === 'tokens' ? d.tokens : d.cost;
      const x = paddingX + (i / (hourlyData.length - 1)) * chartW;
      const y = height - paddingY - (val / max) * chartH;
      return { x, y, data: d };
    });

    if (coords.length < 2) return { pathD: '', areaD: '', points: [] };

    // 构建平滑三次贝塞尔曲线
    let pD = `M ${coords[0].x} ${coords[0].y}`;
    for (let i = 0; i < coords.length - 1; i++) {
      const curr = coords[i];
      const next = coords[i + 1];
      const cpX1 = curr.x + (next.x - curr.x) / 2;
      const cpY1 = curr.y;
      const cpX2 = curr.x + (next.x - curr.x) / 2;
      const cpY2 = next.y;
      pD += ` C ${cpX1} ${cpY1}, ${cpX2} ${cpY2}, ${next.x} ${next.y}`;
    }

    const first = coords[0];
    const last = coords[coords.length - 1];
    const aD = `${pD} L ${last.x} ${height - paddingY} L ${first.x} ${height - paddingY} Z`;

    return { pathD: pD, areaD: aD, points: coords };
  }, [hourlyData, activeMetric]);

  // Mock 原子级流水数据
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
    <div className="w-full max-w-6xl mx-auto px-4 py-2 flex flex-col gap-3 select-none animate-in fade-in-50 duration-300">
      {/* ============================================================== */}
      {/* 顶部极简分段控制器 (Apple 纯粹轻量质感)                           */}
      {/* ============================================================== */}
      <div className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl p-2 px-3 shadow-2xs flex flex-wrap items-center justify-between gap-2.5">
        {/* 左侧：二级分段按钮 */}
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
            <span>{isZh ? '概览与洞察' : 'Overview & Insights'}</span>
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
            <span>{isZh ? '逐笔流水对账' : 'Traffic Ledger'}</span>
            <span className="px-1 py-0 rounded text-[9px] bg-blue-50 text-blue-600 dark:bg-blue-500/15 dark:text-blue-400 font-bold">
              {records.length}
            </span>
          </button>
        </div>

        {/* 右侧：统计时间窗口切换 */}
        <div className="flex items-center gap-1 text-[11px]">
          <span className="text-[10px] text-gray-400 mr-1 flex items-center gap-1 font-medium">
            <Calendar className="w-3 h-3 opacity-60" />
            <span>{isZh ? '时间范围:' : 'Range:'}</span>
          </span>
          {[
            { id: 'today' as TimeRange, label: isZh ? '今天' : 'Today' },
            { id: '7days' as TimeRange, label: isZh ? '近 7 天' : '7 Days' },
            { id: '30days' as TimeRange, label: isZh ? '近 30 天' : '30 Days' },
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
      {/* 视图 A：🍏 Apple 极简 Bento 质感流 (克制、通透、层次分明)         */}
      {/* ============================================================== */}
      {activeView === 'insights' && (
        <div className="flex flex-col gap-3 animate-in fade-in-30 duration-200">
          {/* 第 1 层：4 个统一质感的核心 KPI 数字 (去色彩堆叠) */}
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-2.5">
            {/* KPI 1: 总 Token 吞吐 */}
            <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
              <div className="flex items-center justify-between text-[10.5px] text-gray-500 dark:text-zinc-400 font-medium">
                <span>{isZh ? '总吞吐 (Tokens)' : 'Total Tokens'}</span>
                <Sparkles className="w-3.5 h-3.5 text-blue-500" />
              </div>
              <div className="my-1.5 flex items-baseline gap-1.5">
                <span className="text-2xl font-bold tracking-tight text-gray-900 dark:text-zinc-100 font-mono">
                  2.75M
                </span>
                <span className="text-[10px] font-semibold text-emerald-600 dark:text-emerald-400">
                  +18.4%
                </span>
              </div>
              <div className="text-[9.5px] text-gray-400 font-mono flex items-center justify-between pt-1 border-t border-gray-100 dark:border-white/5">
                <span>直接输入 390K</span>
                <span>输出 17.3K</span>
              </div>
            </div>

            {/* KPI 2: 估算节省官方费用 */}
            <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
              <div className="flex items-center justify-between text-[10.5px] text-gray-500 dark:text-zinc-400 font-medium">
                <span>{isZh ? '估算节省官方费用' : 'API Cost Saved'}</span>
                <DollarSign className="w-3.5 h-3.5 text-emerald-500" />
              </div>
              <div className="my-1.5 flex items-baseline gap-1">
                <span className="text-2xl font-bold tracking-tight text-emerald-600 dark:text-emerald-400 font-mono">
                  $38.51
                </span>
                <span className="text-[10px] text-gray-400 font-medium">USD</span>
              </div>
              <div className="text-[9.5px] text-gray-400 font-mono flex items-center justify-between pt-1 border-t border-gray-100 dark:border-white/5">
                <span>零代理官方缓存节流</span>
                <span className="text-emerald-600 font-semibold">100% 免费</span>
              </div>
            </div>

            {/* KPI 3: 全局缓存命中率 */}
            <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
              <div className="flex items-center justify-between text-[10.5px] text-gray-500 dark:text-zinc-400 font-medium">
                <span>{isZh ? '全局缓存命中率' : 'Cache Hit Rate'}</span>
                <ShieldCheck className="w-3.5 h-3.5 text-blue-500" />
              </div>
              <div className="my-1.5 flex items-baseline gap-1.5">
                <span className="text-2xl font-bold tracking-tight text-gray-900 dark:text-zinc-100 font-mono">
                  85.7%
                </span>
                <span className="text-[10px] text-emerald-600 font-medium">
                  {isZh ? '优秀' : 'High'}
                </span>
              </div>
              <div className="text-[9.5px] text-gray-400 font-mono flex items-center justify-between pt-1 border-t border-gray-100 dark:border-white/5">
                <span>复用 2.34M 缓存</span>
                <span>首字提速 3.2x</span>
              </div>
            </div>

            {/* KPI 4: 平均流速与响应 */}
            <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
              <div className="flex items-center justify-between text-[10.5px] text-gray-500 dark:text-zinc-400 font-medium">
                <span>{isZh ? '平均生成流速' : 'Throughput Speed'}</span>
                <Zap className="w-3.5 h-3.5 text-amber-500" />
              </div>
              <div className="my-1.5 flex items-baseline gap-1">
                <span className="text-2xl font-bold tracking-tight text-gray-900 dark:text-zinc-100 font-mono">
                  168.4
                </span>
                <span className="text-[10px] text-gray-400 font-medium">tok/s</span>
              </div>
              <div className="text-[9.5px] text-gray-400 font-mono flex items-center justify-between pt-1 border-t border-gray-100 dark:border-white/5">
                <span>首字延迟中位数 2.8s</span>
                <span>无缝打字流</span>
              </div>
            </div>
          </div>

          {/* 第 2 层：全页面核心视觉焦点 —— 优雅平滑波形趋势图 (Hero Trend Chart) */}
          <div className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl p-3 px-4 shadow-2xs flex flex-col gap-2">
            {/* 图表顶部标题与度量切换 */}
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                  <Activity className="w-3.5 h-3.5 text-blue-500" />
                  <span>{isZh ? '时间维度吞吐与负载趋势' : 'Throughput & Cost Trend'}</span>
                </span>
                <span className="text-[9.5px] text-gray-400 font-mono hidden sm:inline">
                  (峰值: {activeMetric === 'tokens' ? '340K Tokens / 184 tok/s' : '$4.76 USD'})
                </span>
              </div>

              {/* 度量切换 pill: [ Token 吞吐 ] [ 节省费用 ] */}
              <div className="inline-flex items-center p-0.5 rounded-lg bg-gray-100 dark:bg-white/[0.04]">
                <button
                  onClick={() => setActiveMetric('tokens')}
                  className={`px-2 py-0.5 rounded-md text-[10px] font-medium transition-all ${
                    activeMetric === 'tokens'
                      ? 'bg-white dark:bg-[#202126] text-blue-600 dark:text-blue-400 font-semibold shadow-2xs'
                      : 'text-gray-500 hover:text-gray-800 dark:text-zinc-400'
                  }`}
                >
                  {isZh ? 'Token 吞吐' : 'Tokens'}
                </button>
                <button
                  onClick={() => setActiveMetric('cost')}
                  className={`px-2 py-0.5 rounded-md text-[10px] font-medium transition-all ${
                    activeMetric === 'cost'
                      ? 'bg-white dark:bg-[#202126] text-blue-600 dark:text-blue-400 font-semibold shadow-2xs'
                      : 'text-gray-500 hover:text-gray-800 dark:text-zinc-400'
                  }`}
                >
                  {isZh ? '节省金额 ($)' : 'Cost Saved ($)'}
                </button>
              </div>
            </div>

            {/* Apple 级优雅平滑 SVG 曲线图 */}
            <div className="relative w-full h-[180px] mt-1 select-none">
              <svg
                viewBox="0 0 800 180"
                className="w-full h-full overflow-visible"
                preserveAspectRatio="none"
              >
                <defs>
                  {/* Apple 蓝雾面渐变遮罩 */}
                  <linearGradient id="area-gradient" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="#3b82f6" stopOpacity="0.25" />
                    <stop offset="100%" stopColor="#3b82f6" stopOpacity="0.0" />
                  </linearGradient>
                </defs>

                {/* 背景水平微弱参考线 */}
                <line x1="30" y1="30" x2="770" y2="30" stroke="currentColor" className="text-gray-100 dark:text-white/[0.04]" strokeDasharray="4 4" />
                <line x1="30" y1="90" x2="770" y2="90" stroke="currentColor" className="text-gray-100 dark:text-white/[0.04]" strokeDasharray="4 4" />
                <line x1="30" y1="150" x2="770" y2="150" stroke="currentColor" className="text-gray-100 dark:text-white/[0.04]" strokeDasharray="4 4" />

                {/* 渐变面积 */}
                <path d={areaD} fill="url(#area-gradient)" />

                {/* 平滑贝塞尔曲线主线条 */}
                <path
                  d={pathD}
                  fill="none"
                  stroke="#3b82f6"
                  strokeWidth="2.5"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />

                {/* 鼠标悬停标尺与数据高亮点 */}
                {hoveredPointIndex !== null && points[hoveredPointIndex] && (
                  <>
                    <line
                      x1={points[hoveredPointIndex].x}
                      y1="20"
                      x2={points[hoveredPointIndex].x}
                      y2="160"
                      stroke="#3b82f6"
                      strokeWidth="1.5"
                      strokeDasharray="3 3"
                      className="opacity-70"
                    />
                    <circle
                      cx={points[hoveredPointIndex].x}
                      cy={points[hoveredPointIndex].y}
                      r="4.5"
                      fill="#3b82f6"
                      stroke="white"
                      strokeWidth="2"
                      className="shadow-md"
                    />
                  </>
                )}
              </svg>

              {/* 交互触发隐形热区 */}
              <div className="absolute inset-0 flex">
                {points.map((_, i) => (
                  <div
                    key={i}
                    onMouseEnter={() => setHoveredPointIndex(i)}
                    onMouseLeave={() => setHoveredPointIndex(null)}
                    className="flex-1 h-full cursor-crosshair relative group"
                  />
                ))}
              </div>

              {/* 浮动微型 Tooltip (跟随鼠标位置呈现) */}
              {hoveredPointIndex !== null && points[hoveredPointIndex] && (
                <div
                  style={{
                    left: `${(points[hoveredPointIndex].x / 800) * 100}%`,
                    top: `${Math.max(10, points[hoveredPointIndex].y - 45)}px`,
                    transform: 'translateX(-50%)',
                  }}
                  className="absolute pointer-events-none z-30 p-1.5 px-2.5 rounded-lg bg-gray-900/95 dark:bg-zinc-100/95 text-white dark:text-zinc-900 shadow-xl text-[10px] flex flex-col gap-0.5 whitespace-nowrap animate-in fade-in-0 zoom-in-95 duration-100"
                >
                  <div className="flex items-center justify-between gap-3 font-semibold">
                    <span>{points[hoveredPointIndex].data.time}</span>
                    <span className="text-emerald-400 dark:text-emerald-600 font-mono">
                      ⚡ {points[hoveredPointIndex].data.tps} tok/s
                    </span>
                  </div>
                  <div className="text-[9px] opacity-80 font-mono">
                    {activeMetric === 'tokens'
                      ? `${points[hoveredPointIndex].data.tokens.toLocaleString()} Tokens`
                      : `已省 $${points[hoveredPointIndex].data.cost.toFixed(2)} USD`} · 缓存 {points[hoveredPointIndex].data.cacheRate}%
                  </div>
                </div>
              )}
            </div>

            {/* 底部 X 轴时间刻度 */}
            <div className="flex items-center justify-between text-[9px] text-gray-400 font-mono px-2 pt-1 border-t border-gray-100 dark:border-white/5">
              <span>00:00</span>
              <span>04:00</span>
              <span>08:00</span>
              <span>12:00 (高峰)</span>
              <span>16:00</span>
              <span>20:00</span>
              <span>22:00</span>
            </div>
          </div>

          {/* 第 3 层：对称双 Bento 优雅卡片 (50% / 50% 黄金呼吸感) */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            {/* 左卡片：模型调用分布与真实物理保真度 */}
            <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col gap-2.5">
              <div className="flex items-center justify-between">
                <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                  <Layers className="w-3.5 h-3.5 text-blue-500" />
                  <span>{isZh ? '模型调用分布与物理保真度' : 'Model Share & Upstream'}</span>
                </span>
                <span className="text-[9.5px] text-gray-400">
                  {isZh ? '按用量排行' : 'By Usage'}
                </span>
              </div>

              <div className="flex flex-col gap-2 mt-0.5">
                {[
                  {
                    name: 'claude-3-7-sonnet',
                    physical: 'gemini-4-hydrogen-high',
                    pct: 52,
                    tokens: '1.43M',
                    tps: '225 tok/s',
                    color: 'bg-blue-600',
                    isDowngraded: false,
                  },
                  {
                    name: 'gemini-2.5-pro',
                    physical: 'gemini-4-hydrogen-high',
                    pct: 38,
                    tokens: '1.05M',
                    tps: '168 tok/s',
                    color: 'bg-indigo-500',
                    isDowngraded: false,
                  },
                  {
                    name: 'gemini-2.5-flash',
                    physical: 'gemini-4-carbon',
                    pct: 10,
                    tokens: '270K',
                    tps: '97 tok/s',
                    color: 'bg-cyan-500',
                    isDowngraded: false,
                  },
                ].map((m) => (
                  <div key={m.name} className="flex flex-col gap-1">
                    <div className="flex items-center justify-between text-[10.5px]">
                      <span className="font-medium text-gray-800 dark:text-zinc-200 truncate pr-2">
                        {m.name}
                      </span>
                      <div className="flex items-center gap-1.5 font-mono text-[9.5px] shrink-0">
                        <span className="text-gray-500">{m.tokens} ({m.pct}%)</span>
                        <span className="text-emerald-600 dark:text-emerald-400 font-semibold">{m.tps}</span>
                      </div>
                    </div>

                    {/* 细进度条 */}
                    <div className="w-full h-1.5 rounded-full bg-gray-100 dark:bg-white/[0.06] overflow-hidden">
                      <div style={{ width: `${m.pct}%` }} className={`h-full rounded-full ${m.color}`} />
                    </div>

                    <div className="flex items-center justify-between text-[9px] text-gray-400 font-mono">
                      <span>物理上游: {m.physical}</span>
                      <span className="text-emerald-600 dark:text-emerald-400 flex items-center gap-0.5 font-semibold">
                        <CheckCircle2 className="w-2.5 h-2.5" />
                        <span>原生保真</span>
                      </span>
                    </div>
                  </div>
                ))}
              </div>
            </div>

            {/* 右卡片：高频主力账号贡献度 */}
            <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col gap-2.5">
              <div className="flex items-center justify-between">
                <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                  <ShieldCheck className="w-3.5 h-3.5 text-emerald-500" />
                  <span>{isZh ? '主力账号归属与成本节约' : 'Account Attribution'}</span>
                </span>
                <span className="text-[9.5px] text-gray-400">
                  {isZh ? '高产账号' : 'Top Contributors'}
                </span>
              </div>

              <div className="flex flex-col gap-1.5 mt-0.5">
                {[
                  {
                    email: 'ctrler@868666.best',
                    tag: '🌟 主力',
                    tier: 'PRO',
                    saved: '$24.10',
                    pct: 66,
                    tokens: '1.82M',
                  },
                  {
                    email: 'developer.pro@gmail.com',
                    tag: 'Team Pro',
                    tier: 'PRO',
                    saved: '$8.45',
                    pct: 23,
                    tokens: '620K',
                  },
                  {
                    email: 'research.pool1@gmail.com',
                    tag: 'Claude池',
                    tier: 'FREE',
                    saved: '$5.96',
                    pct: 11,
                    tokens: '310K',
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
                      <span className="text-[9.5px] text-gray-400 font-mono mt-0.5">
                        产出 {acc.tokens} · 贡献占比 {acc.pct}%
                      </span>
                    </div>

                    <div className="flex flex-col items-end shrink-0">
                      <span className="text-[11px] font-bold text-emerald-600 dark:text-emerald-400 font-mono">
                        已省 {acc.saved}
                      </span>
                      <span className="text-[8.5px] text-gray-400 font-medium">USD</span>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      )}

      {/* ============================================================== */}
      {/* 视图 B：🌊 流量与请求流水 (Traffic Ledger & Sessions)             */}
      {/* 结构严谨的对账流水，支持点击滑出右侧毛玻璃抽屉                      */}
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

          {/* 原子账本全景表格 (table-fixed 杜绝横向滚动条) */}
          <div className="bg-white/80 dark:bg-[#121316]/80 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl overflow-hidden shadow-2xs">
            <table className="w-full text-left border-collapse table-fixed select-none">
              <colgroup>
                <col className="w-[70px]" />
                <col className="w-[65px]" />
                <col className="w-[28%]" />
                <col className="w-[18%]" />
                <col className="w-[90px]" />
                <col className="w-[85px]" />
                <col className="w-[65px]" />
                <col className="w-[75px]" />
              </colgroup>
              <thead className="bg-gray-50/80 dark:bg-[#16171b]/80 border-b border-gray-200/60 dark:border-white/5 text-gray-500 text-[10px] font-medium uppercase tracking-wider">
                <tr>
                  <th className="px-3 py-2">时间</th>
                  <th className="px-1.5 py-2 text-center">宿主</th>
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
                    <td className="px-1.5 py-2 text-center">
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

                    {/* 请求模型 ➔ 物理模型 */}
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
            <div className="fixed inset-y-0 right-0 z-50 w-96 bg-white/95 dark:bg-[#16171b]/95 backdrop-blur-2xl border-l border-gray-200/80 dark:border-white/10 shadow-2xl p-4 flex flex-col gap-3 animate-in slide-in-from-right duration-200">
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
