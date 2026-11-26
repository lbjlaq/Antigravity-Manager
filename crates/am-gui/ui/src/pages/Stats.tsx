import { tUi } from "../i18n/ui";
import { api } from "../api";
import React, { useState, useMemo, useEffect, useCallback, useRef } from 'react';
import {
  TrendingUp,
  Activity,
  Sparkles,
  ShieldCheck,
  Calendar,
  Search,
  Terminal,
  X,
  RefreshCw,
  FolderGit2,
  Cpu,
  ChevronRight,
  ChevronLeft,
  ChevronDown,
  Check,
  Share2,
  Copy,
  Download,
  Maximize2,
  BookOpen,
  Layers,
} from 'lucide-react';

interface StatsProps {
  lang: string;
}

type StatsView = 'overview' | 'project' | 'model' | 'ledger';
type TimeRange = 'today' | '7days' | '30days' | 'all' | 'custom';

export interface ProjectUsageGroup {
  workspaceName: string;
  path: string;
  totalSteps: number;
  conversationCount: number;
  totalTokens: number;
  estimatedCostUsd: number;
  preferredModel: string;
  modelBreakdown: [string, number][];
  lastActiveTime: string;
  sessionTitles: string[];
}

export interface ModelPerformanceGroup {
  requestedModel: string;
  physicalModel: string;
  totalRequests: number;
  totalTokens: number;
  totalThinkingTokens?: number;
  cacheHitRate: number;
  sharePercentage: number;
  avgTps?: number;
  peakTps?: number;
  avgTtftMs?: number;
  avgDurationMs?: number;
  isDowngraded?: boolean;
  topWorkspaces?: [string, number][];
}

export interface RequestRecord {
  id: string;
  timestamp?: number;
  time: string;
  requestedModel: string;
  physicalModel?: string;
  isDowngraded?: boolean;
  inputTokens: number;
  cachedTokens: number;
  outputTokens: number;
  workspace: string;
  sessionId: string;
  sessionTitle?: string;
  thinkingTokens?: number;
  tokensPerSecond: number;
  ttftMs: number;
  streamingDurationMs?: number;
  durationMs: number;
  contextUsedTokens?: number;
  contextBudgetTokens?: number;
}

export interface ToolCallMetric {
  toolName: string;
  callCount: number;
  errorCount: number;
  sharePct: number;
}

export interface AgentHierarchyStats {
  totalConversations: number;
  runningCount: number;
  mainSessions: number;
  subagentSessions: number;
  totalSteps: number;
}

export interface ApiErrorEvent {
  timestamp: number;
  timeStr: string;
  code: string;
  model: string;
  workspaceName: string;
  sessionTitle: string;
  conversationId: string;
  message: string;
}

export interface SessionContextItem {
  sessionId: string;
  title: string;
  workspace: string;
  model: string;
  usedTokens: number;
  budgetTokens: number;
  usagePct: number;
  checkpointCount: number;
  isRunning: boolean;
  lastActiveTime: string;
}

export interface TimeBreakdownStats {
  ttftMs: number;
  streamingMs: number;
  toolExecMs: number;
}

export interface PoolSplitStats {
  geminiTokens: number;
  claudeTokens: number;
  geminiRequests: number;
  claudeRequests: number;
}

export interface MasterAnalyticsReport {
  totalRequests: number;
  totalTokens: number;
  totalPromptTokens: number;
  totalCachedTokens: number;
  totalOutputTokens: number;
  totalThinkingTokens?: number;
  globalCacheHitRate: number;
  totalEstimatedCostUsd?: number;
  totalSavedCostUsd: number;
  avgSpeedTps?: number;
  peakSpeedTps?: number;
  avgTtftMs?: number;
  avgDurationMs?: number;
  userPromptCount?: number;
  checkpointCount?: number;
  byProject: ProjectUsageGroup[];
  byModel: ModelPerformanceGroup[];
  recentTraffic: RequestRecord[];
  toolMetrics: ToolCallMetric[];
  agentHierarchy?: AgentHierarchyStats;
  timeBreakdown?: TimeBreakdownStats;
  poolSplit?: PoolSplitStats;
  apiErrors?: ApiErrorEvent[];
  sessionContexts?: SessionContextItem[];
}

interface InteractiveCurveProps {
  series: number[];
  labels?: string[];
  unit?: string;
  color: string;
  gradId: string;
  maxVal: number;
  height?: number;
  refLineY?: number;
}

const InteractiveCurve: React.FC<InteractiveCurveProps> = ({
  series,
  labels,
  unit = '',
  color,
  gradId,
  maxVal,
  height = 72,
  refLineY,
}) => {
  const [activeIdx, setActiveIdx] = useState<number | null>(null);
  const W = 320;
  const H = height;
  const padX = 6;
  const padY = 8;
  const safeSeries = series.length >= 2 ? series : [0, ...series, 0];
  const safeMax = Math.max(maxVal, 1);

  const pts = safeSeries.map((v, i) => {
    const x = padX + (i / (safeSeries.length - 1)) * (W - padX * 2);
    const y = H - padY - (Math.min(Math.max(v, 0), safeMax) / safeMax) * (H - padY * 2);
    return { x, y, v };
  });

  let lineD = `M ${pts[0].x.toFixed(1)} ${pts[0].y.toFixed(1)}`;
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[i];
    const p1 = pts[i + 1];
    const mx = ((p0.x + p1.x) / 2).toFixed(1);
    lineD += ` C ${mx} ${p0.y.toFixed(1)}, ${mx} ${p1.y.toFixed(1)}, ${p1.x.toFixed(1)} ${p1.y.toFixed(1)}`;
  }
  const areaD = `${lineD} L ${pts[pts.length - 1].x.toFixed(1)} ${H - 1} L ${pts[0].x.toFixed(1)} ${H - 1} Z`;

  const activePt = activeIdx !== null && pts[activeIdx] ? pts[activeIdx] : null;
  const activeLabel =
    activeIdx !== null
      ? labels?.[activeIdx] ?? `${String(activeIdx).padStart(2, '0')}:00`
      : '';

  return (
    <div
      className="relative w-full h-full overflow-hidden select-none cursor-crosshair"
      onMouseLeave={() => setActiveIdx(null)}
    >
      <svg viewBox={`0 0 ${W} ${H}`} className="w-full h-full block" preserveAspectRatio="none">
        <defs>
          <linearGradient id={gradId} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={color} stopOpacity="0.30" />
            <stop offset="100%" stopColor={color} stopOpacity="0.02" />
          </linearGradient>
        </defs>
        {refLineY !== undefined && (
          <line
            x1={padX}
            y1={H - padY - (refLineY / safeMax) * (H - padY * 2)}
            x2={W - padX}
            y2={H - padY - (refLineY / safeMax) * (H - padY * 2)}
            stroke={color}
            strokeOpacity="0.28"
            strokeDasharray="3 3"
            strokeWidth="1"
          />
        )}
        <path d={areaD} fill={`url(#${gradId})`} />
        <path d={lineD} fill="none" stroke={color} strokeWidth="2" strokeLinecap="round" vectorEffect="non-scaling-stroke" />
        {activePt && (
          <line
            x1={activePt.x}
            y1={padY}
            x2={activePt.x}
            y2={H - padY}
            stroke={color}
            strokeOpacity="0.55"
            strokeDasharray="2 2"
            strokeWidth="1"
            vectorEffect="non-scaling-stroke"
          />
        )}
      </svg>

      {/* 交互式时间点热区层 (支持悬停与点击查看任意时间点精确数值) */}
      <div className="absolute inset-0 flex">
        {pts.map((_, idx) => (
          <div
            key={idx}
            className="flex-1 h-full"
            onMouseEnter={() => setActiveIdx(idx)}
            onClick={() => setActiveIdx((prev) => (prev === idx ? null : idx))}
          />
        ))}
      </div>

      {/* 悬停/选中时间点的高亮圆点与胶囊浮标 */}
      {activePt && activeIdx !== null && (
        <>
          <div
            style={{
              left: `${(activePt.x / W) * 100}%`,
              top: `${(activePt.y / H) * 100}%`,
              backgroundColor: color,
            }}
            className="absolute w-2.5 h-2.5 -ml-[5px] -mt-[5px] rounded-full ring-2 ring-white dark:ring-[#121316] shadow-sm pointer-events-none"
          />
          <div
            style={{
              left: `${Math.min(82, Math.max(18, (activePt.x / W) * 100))}%`,
            }}
            className="absolute top-0.5 -translate-x-1/2 px-2 py-0.5 rounded-md bg-gray-900/95 dark:bg-zinc-100/95 text-white dark:text-zinc-900 text-[9px] font-mono font-bold shadow-md pointer-events-none whitespace-nowrap z-20"
          >
            {activeLabel} · {activePt.v.toFixed(1)}{unit}
          </div>
        </>
      )}
    </div>
  );
};

interface SpectrumMetricLine {
  id: string;
  label: string;
  series: number[];
  fixedMax?: number;
  color: string;
  format: (v: number) => string;
  avgVal?: number;
  avgLabel?: string;
}

const InteractiveSpectrumChart: React.FC<{
  lines: SpectrumMetricLine[];
  focusedId: string | null;
  labels?: string[];
  height?: number;
  zoomTooltip?: string;
  footerRight?: string;
}> = ({ lines, focusedId, labels, height = 104, zoomTooltip, footerRight }) => {
  const [activeIdx, setActiveIdx] = useState<number | null>(null);
  const totalCount = Math.max(2, ...lines.map((l) => l.series.length));
  const [viewRange, setViewRange] = useState<[number, number] | null>(null);

  // 数据长度变化时（例如切换时间范围或 活跃/24h）自动重置为完整 1.0x 视图
  useEffect(() => {
    setViewRange(null);
  }, [totalCount]);

  const [startIdx, endIdx] = useMemo<[number, number]>(() => {
    if (viewRange) {
      const s = Math.max(0, Math.min(totalCount - 2, viewRange[0]));
      const e = Math.max(s + 2, Math.min(totalCount, viewRange[1]));
      return [s, e];
    }
    return [0, totalCount];
  }, [viewRange, totalCount]);

  const count = Math.max(2, endIdx - startIdx);
  const zoomRatio = totalCount / count;
  const W = 420;
  const H = height;
  const padX = 6;
  const padY = 10;

  const visibleLines = useMemo(
    () => (focusedId ? lines.filter((l) => l.id === focusedId) : lines),
    [lines, focusedId]
  );
  const isSolo = visibleLines.length === 1;

  const built = visibleLines.map((line) => {
    const sliced = line.series.slice(startIdx, endIdx);
    const localMax = Math.max(...sliced, isSolo ? line.avgVal ?? 0 : 0, 1);
    const safeMax = line.fixedMax ?? Math.max(localMax * 1.15, 1);

    const pts = Array.from({ length: count }, (_, i) => {
      const v = sliced[i] ?? 0;
      const x = padX + (i / (count - 1)) * (W - padX * 2);
      const y = H - padY - (Math.min(Math.max(v, 0), safeMax) / safeMax) * (H - padY * 2);
      return { x, y, v };
    });

    let d = `M ${pts[0].x.toFixed(1)} ${pts[0].y.toFixed(1)}`;
    for (let i = 0; i < pts.length - 1; i++) {
      const p0 = pts[i];
      const p1 = pts[i + 1];
      const mx = ((p0.x + p1.x) / 2).toFixed(1);
      d += ` C ${mx} ${p0.y.toFixed(1)}, ${mx} ${p1.y.toFixed(1)}, ${p1.x.toFixed(1)} ${p1.y.toFixed(1)}`;
    }
    const areaD = `${d} L ${pts[pts.length - 1].x.toFixed(1)} ${H - 1} L ${pts[0].x.toFixed(1)} ${H - 1} Z`;

    const avgY =
      isSolo && line.avgVal !== undefined && line.avgVal > 0
        ? H - padY - (Math.min(Math.max(line.avgVal, 0), safeMax) / safeMax) * (H - padY * 2)
        : null;

    return { ...line, pts, d, areaD, avgY };
  });

  const chartRef = useRef<HTMLDivElement>(null);
  const wheelHandlerRef = useRef<(e: WheelEvent) => void>(() => {});
  wheelHandlerRef.current = (e: WheelEvent) => {
    if (totalCount <= 6) return;
    e.preventDefault();
    const zoomIn = e.deltaY < 0;
    const step = Math.max(2, Math.round(count * 0.2));
    const nextCount = zoomIn
      ? Math.max(6, count - step)
      : Math.min(totalCount, count + step);
    if (nextCount >= totalCount) {
      setViewRange(null);
      return;
    }
    const anchorRatio = activeIdx !== null ? activeIdx / Math.max(1, count - 1) : 0.5;
    const anchorGlobal = startIdx + Math.round(anchorRatio * (count - 1));
    let nextStart = Math.round(anchorGlobal - anchorRatio * (nextCount - 1));
    nextStart = Math.max(0, Math.min(totalCount - nextCount, nextStart));
    setViewRange([nextStart, nextStart + nextCount]);
  };

  useEffect(() => {
    const el = chartRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => wheelHandlerRef.current(e);
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, []);

  const activeX = activeIdx !== null ? built[0]?.pts[activeIdx]?.x ?? null : null;
  const globalActiveIdx = activeIdx !== null ? startIdx + activeIdx : null;
  const soloLine = isSolo ? built[0] : null;

  // 底部首/中/尾时间刻度始终与当前缩放区间严格对齐
  const visLabels = labels ? labels.slice(startIdx, endIdx) : [];
  const firstTick = visLabels[0] || '00:00';
  const midTick = visLabels[Math.floor(Math.max(0, visLabels.length - 1) / 2)] || '12:00';
  const lastTick = visLabels[visLabels.length - 1] || '23:00';

  return (
    <div className="flex flex-col gap-1.5 w-full">
      <div
        ref={chartRef}
        style={{ height }}
        className="relative w-full overflow-hidden select-none cursor-crosshair"
        onMouseLeave={() => setActiveIdx(null)}
        onDoubleClick={() => setViewRange(null)}
        title={zoomTooltip}
      >
        <svg viewBox={`0 0 ${W} ${H}`} className="w-full h-full block" preserveAspectRatio="none">
          <defs>
            {built.map((l) => (
              <linearGradient
                key={l.id}
                id={`sc-${l.id}`}
                gradientUnits="userSpaceOnUse"
                x1="0"
                y1={padY}
                x2="0"
                y2={H}
              >
                <stop offset="0%" stopColor={l.color} stopOpacity={isSolo ? '0.36' : '0.26'} />
                <stop offset="52%" stopColor={l.color} stopOpacity={isSolo ? '0.11' : '0.07'} />
                <stop offset="100%" stopColor={l.color} stopOpacity="0.0" />
              </linearGradient>
            ))}
          </defs>
          {[0.25, 0.5, 0.75].map((ratio) => (
            <line
              key={ratio}
              x1={padX}
              y1={H - padY - ratio * (H - padY * 2)}
              x2={W - padX}
              y2={H - padY - ratio * (H - padY * 2)}
              stroke="rgba(255,255,255,0.07)"
              strokeWidth="1"
            />
          ))}
          {soloLine && soloLine.avgY !== null && (
            <line
              x1={padX}
              y1={soloLine.avgY}
              x2={W - padX}
              y2={soloLine.avgY}
              stroke={soloLine.color}
              strokeOpacity="0.55"
              strokeDasharray="3 3"
              strokeWidth="1"
              vectorEffect="non-scaling-stroke"
            />
          )}
          {/* 仅对单一主线（或当前聚焦线）铺单色丝绒渐隐，避免 4 色叠加发灰发雾；全部曲线保持 Retina 刀锋锐利 */}
          {built.map((l, idx) =>
            isSolo || idx === 0 ? (
              <path key={`area-${l.id}`} d={l.areaD} fill={`url(#sc-${l.id})`} />
            ) : null
          )}
          {built.map((l) => (
            <path
              key={`line-${l.id}`}
              d={l.d}
              fill="none"
              stroke={l.color}
              strokeWidth={isSolo ? '2.1' : '1.85'}
              strokeOpacity="1"
              strokeLinecap="round"
              strokeLinejoin="round"
              vectorEffect="non-scaling-stroke"
            />
          ))}
          {activeX !== null && (
            <line
              x1={activeX}
              y1={padY}
              x2={activeX}
              y2={H - padY}
              stroke="rgba(255,255,255,0.28)"
              strokeDasharray="2 2"
              strokeWidth="1"
              vectorEffect="non-scaling-stroke"
            />
          )}
        </svg>

        {soloLine && soloLine.avgY !== null && soloLine.avgLabel && activeX === null && (
          <div
            style={{
              top: `${Math.min(76, Math.max(8, (soloLine.avgY / H) * 100 - 11))}%`,
              color: soloLine.color,
            }}
            className="absolute right-1.5 px-1.5 py-0.5 rounded bg-[#141720]/85 border border-white/10 text-[8.5px] font-mono font-semibold pointer-events-none"
          >
            {soloLine.avgLabel}
          </div>
        )}

        <div className="absolute inset-0 flex">
          {Array.from({ length: count }, (_, idx) => (
            <div
              key={idx}
              className="flex-1 h-full"
              onMouseEnter={() => setActiveIdx(idx)}
              onClick={() => setActiveIdx((prev) => (prev === idx ? null : idx))}
            />
          ))}
        </div>

        {zoomRatio > 1.05 && (
          <div className="absolute bottom-1 right-1.5 px-1.5 py-0.5 rounded bg-black/45 text-[8px] font-mono text-slate-300/90 pointer-events-none">
            {zoomRatio.toFixed(1)}x · {count}/{totalCount}
          </div>
        )}

        {activeIdx !== null && globalActiveIdx !== null && activeX !== null && (
          <>
            {built.map((l) => {
              const pt = l.pts[activeIdx];
              if (!pt) return null;
              return (
                <div
                  key={l.id}
                  style={{
                    left: `${(pt.x / W) * 100}%`,
                    top: `${(pt.y / H) * 100}%`,
                    backgroundColor: l.color,
                  }}
                  className="absolute w-2 h-2 -ml-1 -mt-1 rounded-full ring-2 ring-[#191c25] shadow-sm pointer-events-none"
                />
              );
            })}
            <div
              style={{
                left: `${Math.min(74, Math.max(26, (activeX / W) * 100))}%`,
              }}
              className="absolute top-0.5 -translate-x-1/2 px-2 py-0.5 rounded-md bg-[#12151e]/95 backdrop-blur-md border border-white/12 text-[8.5px] font-mono text-slate-100 shadow-lg pointer-events-none whitespace-nowrap z-20 flex items-center gap-2"
            >
              <span className="text-slate-400">
                {labels?.[globalActiveIdx] ?? `${String(globalActiveIdx).padStart(2, '0')}:00`}
              </span>
              {built.map((l) => (
                <span key={l.id} style={{ color: l.color }} className="font-semibold">
                  {l.format(l.pts[activeIdx]?.v ?? 0)}
                </span>
              ))}
            </div>
          </>
        )}
      </div>

      <div className="flex items-center justify-between text-[9.5px] tabular-nums text-slate-300/80 pt-1 border-t border-white/[0.08]">
        <span>{firstTick}</span>
        <span>{midTick}</span>
        <span>{lastTick}</span>
        {footerRight && <span className="text-slate-300/90">{footerRight}</span>}
      </div>
    </div>
  );
};

export const Stats: React.FC<StatsProps> = ({ lang }) => {
  const todayStr = useMemo(() => {
    const d = new Date();
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  }, []);

  const [activeView, setActiveView] = useState<StatsView>('overview');
  const [timeRange, setTimeRange] = useState<TimeRange>('today');
  const [customStartDate, setCustomStartDate] = useState<string>(todayStr);
  const [customEndDate, setCustomEndDate] = useState<string>(todayStr);
  const [datePickerOpen, setDatePickerOpen] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const datePickerRef = useRef<HTMLDivElement>(null);

  // 项目下钻与分享战报状态
  const [selectedProjectName, setSelectedProjectName] = useState<string>('__ALL__');
  const [projectDropdownOpen, setProjectDropdownOpen] = useState(false);
  const projectDropdownRef = useRef<HTMLDivElement>(null);
  const [projectSearchQuery, setProjectSearchQuery] = useState('');
  const [shareTemplate, setShareTemplate] = useState<'polar' | 'radial' | 'hexcore' | 'matrix'>('polar');
  const [projectActivityTab, setProjectActivityTab] = useState<'milestones' | 'requests' | 'perf'>('milestones');
  const [overviewRightTab, setOverviewRightTab] = useState<'tools' | 'context' | 'alerts'>('tools');
  const [spectrumFocus, setSpectrumFocus] = useState<'tok' | 'tps' | 'ttft' | 'cache' | null>(null);
  const [spectrumZoom, setSpectrumZoom] = useState<'active' | '24h'>('active');
  const [selectedModelFilter, setSelectedModelFilter] = useState<string>('__ALL__');
  const [modelDetailTab, setModelDetailTab] = useState<'workspaces' | 'requests'>('workspaces');
  const [projPerfModel, setProjPerfModel] = useState<string>('__ALL__');
  const [ledgerCacheFilter, setLedgerCacheFilter] = useState<'all' | 'miss'>('all');
  const [ledgerMode, setLedgerMode] = useState<'requests' | 'sessions'>('requests');
  const [ledgerPage, setLedgerPage] = useState<number>(1);
  const [expandedSessions, setExpandedSessions] = useState<Record<string, boolean>>({});
  const [shareModalOpen, setShareModalOpen] = useState(false);
  const [copiedShare, setCopiedShare] = useState(false);

  // 会话流水过滤与详情弹窗
  const [workspaceFilter, setWorkspaceFilter] = useState<string>('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedRecord, setSelectedRecord] = useState<RequestRecord | null>(null);

  useEffect(() => {
    if (!datePickerOpen && !projectDropdownOpen && !shareModalOpen && !selectedRecord) return;

    const handlePointerDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (datePickerOpen && datePickerRef.current && !datePickerRef.current.contains(target)) {
        setDatePickerOpen(false);
      }
      if (projectDropdownOpen && projectDropdownRef.current && !projectDropdownRef.current.contains(target)) {
        setProjectDropdownOpen(false);
      }
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setDatePickerOpen(false);
        setProjectDropdownOpen(false);
        setShareModalOpen(false);
        setSelectedRecord(null);
      }
    };

    document.addEventListener('mousedown', handlePointerDown);
    document.addEventListener('keydown', handleKeyDown);
    return () => {
      document.removeEventListener('mousedown', handlePointerDown);
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [datePickerOpen, projectDropdownOpen, shareModalOpen, selectedRecord]);

  // 真实本地数据库报告状态
  const [reportData, setReportData] = useState<MasterAnalyticsReport>({
    totalRequests: 0,
    totalTokens: 0,
    totalPromptTokens: 0,
    totalCachedTokens: 0,
    totalOutputTokens: 0,
    totalThinkingTokens: 0,
    globalCacheHitRate: 0,
    totalEstimatedCostUsd: 0,
    totalSavedCostUsd: 0,
    avgSpeedTps: 0,
    peakSpeedTps: 0,
    avgTtftMs: 0,
    avgDurationMs: 0,
    userPromptCount: 0,
    checkpointCount: 0,
    byProject: [],
    byModel: [],
    recentTraffic: [],
    toolMetrics: [],
    apiErrors: [],
    sessionContexts: [],
  });

  const rangeBounds = useMemo(() => {
    const parseStart = (ymd: string) => {
      const [y, m, d] = ymd.split('-').map(Number);
      return new Date(y, (m || 1) - 1, d || 1, 0, 0, 0, 0).getTime();
    };
    const parseEnd = (ymd: string) => {
      const [y, m, d] = ymd.split('-').map(Number);
      return new Date(y, (m || 1) - 1, d || 1, 23, 59, 59, 999).getTime();
    };
    const now = new Date();
    if (timeRange === 'today') {
      const startTsMs = new Date(now.getFullYear(), now.getMonth(), now.getDate(), 0, 0, 0, 0).getTime();
      return { startTsMs, endTsMs: null as number | null, isSingleDay: true };
    }
    if (timeRange === '7days') {
      const startTsMs = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 6, 0, 0, 0, 0).getTime();
      return { startTsMs, endTsMs: null as number | null, isSingleDay: false };
    }
    if (timeRange === '30days') {
      const startTsMs = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 29, 0, 0, 0, 0).getTime();
      return { startTsMs, endTsMs: null as number | null, isSingleDay: false };
    }
    if (timeRange === 'custom') {
      const sDate = customStartDate || todayStr;
      const eDate = customEndDate || sDate;
      const [minD, maxD] = sDate <= eDate ? [sDate, eDate] : [eDate, sDate];
      return {
        startTsMs: parseStart(minD),
        endTsMs: parseEnd(maxD),
        isSingleDay: minD === maxD,
      };
    }
    return { startTsMs: null as number | null, endTsMs: null as number | null, isSingleDay: false };
  }, [timeRange, customStartDate, customEndDate, todayStr]);

  const fetchReport = useCallback(async () => {
    setIsLoading(true);
    try {
      const data = await api.getStatsReport({
        startTsMs: rangeBounds.startTsMs,
        endTsMs: rangeBounds.endTsMs,
      });
      if (data) {
        const projects: ProjectUsageGroup[] = Array.isArray(data.byProject) ? data.byProject : [];
        setReportData({
          totalRequests: data.totalRequests ?? 0,
          totalTokens: data.totalTokens ?? 0,
          totalPromptTokens: data.totalPromptTokens ?? 0,
          totalCachedTokens: data.totalCachedTokens ?? 0,
          totalOutputTokens: data.totalOutputTokens ?? 0,
          totalThinkingTokens: Number(data.totalThinkingTokens || 0),
          globalCacheHitRate: data.globalCacheHitRate ?? 0,
          totalEstimatedCostUsd: Number(data.totalEstimatedCostUsd || 0),
          totalSavedCostUsd: data.totalSavedCostUsd ?? 0,
          avgSpeedTps: Number(data.avgSpeedTps || 0),
          peakSpeedTps: Number(data.peakSpeedTps || 0),
          avgTtftMs: Number(data.avgTtftMs || 0),
          avgDurationMs: Number(data.avgDurationMs || 0),
          userPromptCount: Number(data.userPromptCount || 0),
          checkpointCount: Number(data.checkpointCount || 0),
          byProject: projects,
          byModel: Array.isArray(data.byModel) ? data.byModel : [],
          toolMetrics: Array.isArray(data.toolMetrics) ? data.toolMetrics : [],
          agentHierarchy: data.agentHierarchy,
          timeBreakdown: data.timeBreakdown,
          poolSplit: data.poolSplit,
          apiErrors: Array.isArray(data.apiErrors) ? data.apiErrors : [],
          sessionContexts: Array.isArray(data.sessionContexts) ? data.sessionContexts : [],
          recentTraffic: Array.isArray(data.recentTraffic)
            ? data.recentTraffic.map((r: any) => ({
                id: r.id,
                timestamp: Number(r.timestamp || 0),
                time: r.timeStr || r.time || '-',
                requestedModel: r.requestedModel || 'unknown',
                physicalModel: r.physicalModel || undefined,
                isDowngraded: Boolean(r.isDowngraded),
                inputTokens: r.promptTokens || r.inputTokens || 0,
                cachedTokens: r.cachedTokens || 0,
                outputTokens: r.completionTokens || r.outputTokens || 0,
                workspace: r.workspaceName || r.workspace || 'default',
                sessionId: r.conversationId || r.sessionId || '',
                sessionTitle: r.sessionTitle,
                thinkingTokens: r.thinkingTokens || 0,
                tokensPerSecond: Number(r.tokensPerSecond || 0),
                ttftMs: Number(r.ttftMs || 0),
                streamingDurationMs: Number(r.streamingDurationMs || 0),
                durationMs: Number(r.durationMs || 0),
                contextUsedTokens: Number(r.contextUsedTokens || 0),
                contextBudgetTokens: Number(r.contextBudgetTokens || 0),
              }))
            : [],
        });
        if (projects.length > 0 && !selectedProjectName) {
          setSelectedProjectName(projects[0].workspaceName);
        }
      }
    } catch {
      // 非 Tauri 环境静默降级
    } finally {
      setIsLoading(false);
    }
  }, [rangeBounds.startTsMs, rangeBounds.endTsMs, selectedProjectName]);

  useEffect(() => {
    fetchReport();
  }, [fetchReport]);

  useEffect(() => {
    setLedgerPage(1);
  }, [timeRange, customStartDate, customEndDate, workspaceFilter, ledgerCacheFilter, searchQuery, ledgerMode]);

  const allProjectsAggregated = useMemo<ProjectUsageGroup | null>(() => {
    if (reportData.byProject.length === 0) return null;
    const totalSteps = reportData.byProject.reduce((acc, p) => acc + p.totalSteps, 0);
    const conversationCount = reportData.byProject.reduce((acc, p) => acc + p.conversationCount, 0);
    const totalTokens = reportData.totalTokens || reportData.byProject.reduce((acc, p) => acc + p.totalTokens, 0);
    const estimatedCostUsd = reportData.byProject.reduce((acc, p) => acc + p.estimatedCostUsd, 0);
    const sessionTitles = reportData.byProject.flatMap((p) => p.sessionTitles).slice(0, 12);
    const lastActiveTime = reportData.byProject[0]?.lastActiveTime || '';
    const modelBreakdown: [string, number][] = reportData.byModel.map((m) => [m.requestedModel, m.sharePercentage]);
    return {
      workspaceName: '__ALL__',
      path: tUi(lang, 'stats_all_projects_summary', { count: reportData.byProject.length }),
      totalSteps,
      conversationCount,
      totalTokens,
      estimatedCostUsd,
      preferredModel: reportData.byModel[0]?.requestedModel || reportData.byProject[0]?.preferredModel || 'Gemini',
      modelBreakdown: modelBreakdown.length > 0 ? modelBreakdown : reportData.byProject[0]?.modelBreakdown || [],
      lastActiveTime,
      sessionTitles,
    };
  }, [reportData, lang]);

  const selectableProjects = useMemo(() => {
    if (!allProjectsAggregated) return reportData.byProject;
    return [allProjectsAggregated, ...reportData.byProject];
  }, [allProjectsAggregated, reportData.byProject]);

  const currentProject = useMemo(() => {
    if (selectedProjectName === '__ALL__' && allProjectsAggregated) return allProjectsAggregated;
    return (
      reportData.byProject.find((p) => p.workspaceName === selectedProjectName) ||
      allProjectsAggregated ||
      reportData.byProject[0]
    );
  }, [reportData.byProject, selectedProjectName, allProjectsAggregated]);

  const totalToolCalls = useMemo(
    () => reportData.toolMetrics.reduce((acc, t) => acc + t.callCount, 0),
    [reportData.toolMetrics]
  );

  // 通用平滑贝塞尔面积折线渲染器 (支持交互式时间点悬停与点击查看)
  const renderSmoothCurve = (
    series: number[],
    color: string,
    gradId: string,
    maxVal: number,
    height = 72,
    refLineY?: number,
    labels?: string[],
    unit?: string
  ) => {
    const inferredUnit = unit ?? (gradId.includes('tps') ? ' tok/s' : '%');
    return (
      <InteractiveCurve
        series={series}
        labels={labels}
        unit={inferredUnit}
        color={color}
        gradId={gradId}
        maxVal={maxVal}
        height={height}
        refLineY={refLineY}
      />
    );
  };

  // 按指定时段 (今天 / 近 7 天 / 近 30 天 / 全部 / 自定义日期) 聚合算力、缓存率与 tok/s 曲线
  const computePerfSeries = (records: RequestRecord[], range: TimeRange) => {
    const dayMs = 86400_000;
    const isSingleDay = rangeBounds.isSingleDay;

    let bucketCount = 12;
    let minTs = rangeBounds.startTsMs ?? Date.now() - 29 * dayMs;
    let maxTs = rangeBounds.endTsMs ?? Date.now();

    if (isSingleDay) {
      bucketCount = 24;
    } else if (range === '7days') {
      bucketCount = 7;
    } else if (range === '30days') {
      bucketCount = 10;
    } else if (range === 'custom') {
      const spanDays = Math.max(2, Math.round((maxTs - minTs) / dayMs));
      bucketCount = Math.min(14, spanDays);
    } else {
      // 'all' — 动态横跨真实最早~最晚时间戳
      bucketCount = 12;
      let earliest = Number.POSITIVE_INFINITY;
      let latest = 0;
      for (const r of records) {
        const ts = r.timestamp || Date.parse(r.time.replace(' ', 'T'));
        if (!Number.isNaN(ts) && ts > 0) {
          if (ts < earliest) earliest = ts;
          if (ts > latest) latest = ts;
        }
      }
      if (earliest < Number.POSITIVE_INFINITY && latest >= earliest) {
        minTs = earliest;
        maxTs = Math.max(latest, earliest + dayMs);
      }
    }

    const tokSeries = new Array(bucketCount).fill(0);
    const cacheSum = new Array(bucketCount).fill(0);
    const promptSum = new Array(bucketCount).fill(0);
    const bucketOutTok = new Array(bucketCount).fill(0);
    const bucketDurMs = new Array(bucketCount).fill(0);
    const tpsSum = new Array(bucketCount).fill(0);
    const tpsCnt = new Array(bucketCount).fill(0);
    const ttftSum = new Array(bucketCount).fill(0);
    const ttftCnt = new Array(bucketCount).fill(0);

    let totalCached = 0;
    let totalPrompt = 0;
    let totalMeasuredOut = 0;
    let totalMeasuredDurMs = 0;
    let totalMeasuredDurCnt = 0;
    let totalTps = 0;
    let totalTpsCnt = 0;
    let singlePeakTps = 0;

    for (const r of records) {
      let bIdx = -1;
      const ts = r.timestamp || Date.parse(r.time.replace(' ', 'T'));
      if (isSingleDay) {
        if (ts > 0 && !Number.isNaN(ts)) {
          bIdx = new Date(ts).getHours();
        } else {
          const m = r.time.match(/(\d{2}):\d{2}/);
          if (m) bIdx = Math.min(23, Math.max(0, parseInt(m[1], 10)));
        }
      } else if (!Number.isNaN(ts) && ts > 0) {
        if (range === '7days' && rangeBounds.startTsMs) {
          const dIdx = Math.floor((ts - rangeBounds.startTsMs) / dayMs);
          if (dIdx >= 0 && dIdx < 7) bIdx = dIdx;
        } else {
          const span = Math.max(1, maxTs - minTs);
          bIdx = Math.min(bucketCount - 1, Math.max(0, Math.floor(((ts - minTs) / span) * bucketCount)));
        }
      }

      const hasMeasuredDur = r.durationMs > 0 && r.outputTokens > 0;
      if (bIdx >= 0 && bIdx < bucketCount) {
        tokSeries[bIdx] += r.inputTokens + r.cachedTokens + r.outputTokens;
        cacheSum[bIdx] += r.cachedTokens;
        promptSum[bIdx] += r.inputTokens + r.cachedTokens;
        if (hasMeasuredDur) {
          bucketOutTok[bIdx] += r.outputTokens;
          bucketDurMs[bIdx] += r.durationMs;
        }
        if (r.tokensPerSecond > 0) {
          tpsSum[bIdx] += r.tokensPerSecond;
          tpsCnt[bIdx] += 1;
        }
        if (r.ttftMs > 0) {
          ttftSum[bIdx] += r.ttftMs;
          ttftCnt[bIdx] += 1;
        }
      }
      totalCached += r.cachedTokens;
      totalPrompt += r.inputTokens + r.cachedTokens;
      if (hasMeasuredDur) {
        totalMeasuredOut += r.outputTokens;
        totalMeasuredDurMs += r.durationMs;
        totalMeasuredDurCnt += 1;
      }
      if (r.tokensPerSecond > 0) {
        totalTps += r.tokensPerSecond;
        totalTpsCnt += 1;
        if (r.tokensPerSecond > singlePeakTps) {
          singlePeakTps = r.tokensPerSecond;
        }
      }
    }

    const fmtMd = (ms: number) => {
      const d = new Date(ms);
      return `${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
    };

    const bucketLabels: string[] = isSingleDay
      ? Array.from({ length: 24 }, (_, h) => `${String(h).padStart(2, '0')}:00`)
      : Array.from({ length: bucketCount }, (_, i) => {
          if (range === '7days' && rangeBounds.startTsMs) {
            return fmtMd(rangeBounds.startTsMs + i * dayMs);
          }
          const step = Math.max(1, maxTs - minTs) / bucketCount;
          return fmtMd(minTs + i * step);
        });

    const cacheSeries = promptSum.map((p, i) => (p > 0 ? Math.round((cacheSum[i] / p) * 1000) / 10 : 0));
    const tpsSeries = tpsCnt.map((c, i) =>
      bucketDurMs[i] > 0 && bucketOutTok[i] > 0
        ? Math.round(((bucketOutTok[i] * 1000) / bucketDurMs[i]) * 10) / 10
        : c > 0
        ? Math.round((tpsSum[i] / c) * 10) / 10
        : 0
    );
    const ttftSeries = ttftCnt.map((c, i) =>
      c > 0 ? Math.round((ttftSum[i] / c / 1000) * 100) / 100 : 0
    );
    const avgCache = totalPrompt > 0 ? (totalCached / totalPrompt) * 100 : 0;
    const avgTps =
      totalMeasuredDurMs > 0 && totalMeasuredOut > 0
        ? (totalMeasuredOut * 1000) / totalMeasuredDurMs
        : totalTpsCnt > 0
        ? totalTps / totalTpsCnt
        : 0;
    const avgDurationMs = totalMeasuredDurCnt > 0 ? totalMeasuredDurMs / totalMeasuredDurCnt : 0;
    const seriesPeakTps = Math.max(...tpsSeries, 0);
    const rangeLabel =
      range === 'today'
        ? tUi(lang, 'stats_range_today_zero')
        : range === '7days'
        ? tUi(lang, 'stats_range_7d')
        : range === '30days'
        ? tUi(lang, 'stats_range_30d')
        : range === 'all'
        ? tUi(lang, 'stats_range_all')
        : customStartDate === customEndDate
        ? customStartDate
        : `${customStartDate.slice(5)} ~ ${customEndDate.slice(5)}`;

    const zoomLabel = isSingleDay ? '24h' : range === '7days' ? '7d' : range === '30days' ? '30d' : tUi(lang, 'stats_zoom_full');

    return {
      tokSeries,
      bucketLabels,
      cacheSeries,
      tpsSeries,
      ttftSeries,
      avgCache,
      peakCache: Math.max(...cacheSeries, 0),
      avgTps,
      avgDurationMs,
      peakTps: singlePeakTps,
      seriesPeakTps,
      count: records.length,
      rangeLabel,
      zoomLabel,
    };
  };

  // 过滤会话流水
  const filteredRecords = useMemo(() => {
    return reportData.recentTraffic.filter((r) => {
      if (workspaceFilter !== 'all' && r.workspace !== workspaceFilter) return false;
      if (!searchQuery.trim()) return true;
      const q = searchQuery.toLowerCase();
      return (
        (r.sessionTitle || '').toLowerCase().includes(q) ||
        r.requestedModel.toLowerCase().includes(q) ||
        r.workspace.toLowerCase().includes(q) ||
        r.sessionId.toLowerCase().includes(q)
      );
    });
  }, [reportData.recentTraffic, workspaceFilter, searchQuery]);

  const fmtTok = (n: number) => {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
    return `${n}`;
  };

  const fmtUsd = (n: number) => {
    if (n >= 100) return `$${n.toFixed(0)}`;
    if (n >= 10) return `$${n.toFixed(1)}`;
    if (n >= 0.01) return `$${n.toFixed(2)}`;
    return '$0';
  };

  const fmtDurCompact = (ms: number) => {
    if (ms >= 3_600_000) return `${(ms / 3_600_000).toFixed(1)}h`;
    if (ms >= 60_000) return `${Math.round(ms / 60_000)}m`;
    if (ms >= 1_000) return `${(ms / 1_000).toFixed(1)}s`;
    return `${ms}ms`;
  };

  return (
    <div className="w-full max-w-[1400px] mx-auto px-4 py-3 flex flex-col gap-3 select-none">
      {/* ============================================================== */}
      {/* 顶部 4 视角药丸导航栏（100% 真实本地数据视角）                     */}
      {/* ============================================================== */}
      <div className="relative z-40 bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] rounded-xl p-2 px-3 shadow-2xs flex flex-wrap items-center justify-between gap-2.5">
        <div className="inline-flex items-center p-0.5 rounded-lg bg-gray-100/90 dark:bg-white/[0.04] border border-gray-200/50 dark:border-white/5">
          <button
            onClick={() => setActiveView('overview')}
            className={`px-3 py-1 rounded-md text-[11px] font-medium flex items-center gap-1.5 transition-all ${
              activeView === 'overview'
                ? 'bg-white dark:bg-[#202126] text-blue-600 dark:text-blue-400 font-semibold shadow-2xs'
                : 'text-gray-600 dark:text-zinc-400 hover:text-gray-900 dark:hover:text-zinc-100'
            }`}
          >
            <TrendingUp className="w-3.5 h-3.5" />
            <span>{tUi(lang, 'stats_tab_overview')}</span>
          </button>

          <button
            onClick={() => setActiveView('project')}
            className={`px-3 py-1 rounded-md text-[11px] font-medium flex items-center gap-1.5 transition-all ${
              activeView === 'project'
                ? 'bg-white dark:bg-[#202126] text-blue-600 dark:text-blue-400 font-semibold shadow-2xs'
                : 'text-gray-600 dark:text-zinc-400 hover:text-gray-900 dark:hover:text-zinc-100'
            }`}
          >
            <FolderGit2 className="w-3.5 h-3.5" />
            <span>{tUi(lang, 'stats_tab_projects')}</span>
            <span className="px-1 py-0 rounded text-[9px] bg-indigo-50 text-indigo-600 dark:bg-indigo-500/15 dark:text-indigo-400 font-bold">
              {reportData.byProject.length}
            </span>
          </button>

          <button
            onClick={() => setActiveView('model')}
            className={`px-3 py-1 rounded-md text-[11px] font-medium flex items-center gap-1.5 transition-all ${
              activeView === 'model'
                ? 'bg-white dark:bg-[#202126] text-blue-600 dark:text-blue-400 font-semibold shadow-2xs'
                : 'text-gray-600 dark:text-zinc-400 hover:text-gray-900 dark:hover:text-zinc-100'
            }`}
          >
            <Cpu className="w-3.5 h-3.5" />
            <span>{tUi(lang, 'stats_tab_models')}</span>
            <span className="px-1 py-0 rounded text-[9px] bg-purple-50 text-purple-600 dark:bg-purple-500/15 dark:text-purple-400 font-bold">
              {reportData.byModel.length}
            </span>
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
            <span>{tUi(lang, 'stats_tab_ledger')}</span>
            <span className="px-1 py-0 rounded text-[9px] bg-blue-50 text-blue-600 dark:bg-blue-500/15 dark:text-blue-400 font-bold">
              {reportData.recentTraffic.length}
            </span>
          </button>
        </div>

        <div className="flex items-center gap-2 relative">
          <div className="flex items-center gap-1 text-[11px] flex-wrap">
            <span className="text-[10px] text-gray-400 mr-1 flex items-center gap-1 font-medium">
              <Calendar className="w-3 h-3 opacity-60" />
              <span>{tUi(lang, 'stats_range_label')}</span>
            </span>
            {[
              { id: 'today' as TimeRange, label: tUi(lang, 'stats_range_today_zero') },
              { id: '7days' as TimeRange, label: tUi(lang, 'stats_range_7d') },
              { id: '30days' as TimeRange, label: tUi(lang, 'stats_range_30d') },
              { id: 'all' as TimeRange, label: tUi(lang, 'stats_range_all') },
            ].map((item) => (
              <button
                key={item.id}
                onClick={() => {
                  setTimeRange(item.id);
                  setDatePickerOpen(false);
                }}
                className={`px-2 py-0.5 rounded-md text-[10px] font-medium transition-all ${
                  timeRange === item.id
                    ? 'bg-blue-50 dark:bg-blue-500/10 text-blue-600 dark:text-blue-400 font-semibold border border-blue-200/60 dark:border-blue-500/20 shadow-2xs'
                    : 'text-gray-500 dark:text-zinc-400 hover:bg-gray-100 dark:hover:bg-zinc-800'
                }`}
              >
                {item.label}
              </button>
            ))}

            {/* 自定义单日 / 日期区间选择器 */}
            <div ref={datePickerRef} className="relative">
              <button
                onClick={() => setDatePickerOpen((v) => !v)}
                className={`px-2 py-0.5 rounded-md text-[10px] font-medium flex items-center gap-1 transition-all ${
                  timeRange === 'custom'
                    ? 'bg-blue-50 dark:bg-blue-500/10 text-blue-600 dark:text-blue-400 font-semibold border border-blue-200/60 dark:border-blue-500/20 shadow-2xs'
                    : 'text-gray-500 dark:text-zinc-400 hover:bg-gray-100 dark:hover:bg-zinc-800'
                }`}
              >
                <span>
                  {timeRange === 'custom'
                    ? customStartDate === customEndDate
                      ? customStartDate
                      : `${customStartDate.slice(5)} ~ ${customEndDate.slice(5)}`
                    : tUi(lang, 'stats_range_custom')}
                </span>
                <ChevronDown className="w-2.5 h-2.5 opacity-70" />
              </button>

              {datePickerOpen && (
                <div
                  className="absolute right-0 mt-1.5 z-40 w-64 p-3 rounded-xl bg-white/95 dark:bg-[#18191e]/95 backdrop-blur-xl border border-gray-200/80 dark:border-white/10 shadow-xl flex flex-col gap-2.5 text-[10.5px]"
                  onClick={(e) => e.stopPropagation()}
                >
                    <div className="flex items-center justify-between">
                      <span className="font-semibold text-gray-800 dark:text-zinc-200">
                        {tUi(lang, 'stats_range_custom')}
                      </span>
                      <div className="flex items-center gap-1">
                        <button
                          onClick={() => {
                            const yd = new Date();
                            yd.setDate(yd.getDate() - 1);
                            const yStr = `${yd.getFullYear()}-${String(yd.getMonth() + 1).padStart(2, '0')}-${String(yd.getDate()).padStart(2, '0')}`;
                            setCustomStartDate(yStr);
                            setCustomEndDate(yStr);
                            setTimeRange('custom');
                            setDatePickerOpen(false);
                          }}
                          className="px-1.5 py-0.5 rounded bg-gray-100 dark:bg-white/[0.06] hover:bg-blue-50 dark:hover:bg-blue-500/15 text-gray-600 dark:text-zinc-300 text-[9.5px]"
                        >
                          {tUi(lang, 'stats_date_yesterday')}
                        </button>
                        <button
                          onClick={() => {
                            const now = new Date();
                            const mStart = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-01`;
                            setCustomStartDate(mStart);
                            setCustomEndDate(todayStr);
                            setTimeRange('custom');
                            setDatePickerOpen(false);
                          }}
                          className="px-1.5 py-0.5 rounded bg-gray-100 dark:bg-white/[0.06] hover:bg-blue-50 dark:hover:bg-blue-500/15 text-gray-600 dark:text-zinc-300 text-[9.5px]"
                        >
                          {tUi(lang, 'stats_date_this_month')}
                        </button>
                      </div>
                    </div>

                    <div className="flex flex-col gap-1.5">
                      <label className="flex items-center justify-between gap-2 text-gray-500 dark:text-zinc-400">
                        <span>{tUi(lang, 'stats_date_start')}</span>
                        <input
                          type="date"
                          value={customStartDate}
                          max={todayStr}
                          onChange={(e) => {
                            const v = e.target.value;
                            if (v) {
                              setCustomStartDate(v);
                              if (v > customEndDate) setCustomEndDate(v);
                              setTimeRange('custom');
                            }
                          }}
                          className="px-2 py-1 rounded-lg bg-gray-100 dark:bg-white/[0.06] border border-gray-200/70 dark:border-white/10 text-gray-900 dark:text-zinc-100 font-mono text-[10px] outline-none focus:border-blue-500"
                        />
                      </label>
                      <label className="flex items-center justify-between gap-2 text-gray-500 dark:text-zinc-400">
                        <span>{tUi(lang, 'stats_date_end')}</span>
                        <input
                          type="date"
                          value={customEndDate}
                          min={customStartDate}
                          max={todayStr}
                          onChange={(e) => {
                            const v = e.target.value;
                            if (v) {
                              setCustomEndDate(v);
                              if (v < customStartDate) setCustomStartDate(v);
                              setTimeRange('custom');
                            }
                          }}
                          className="px-2 py-1 rounded-lg bg-gray-100 dark:bg-white/[0.06] border border-gray-200/70 dark:border-white/10 text-gray-900 dark:text-zinc-100 font-mono text-[10px] outline-none focus:border-blue-500"
                        />
                      </label>
                    </div>

                    <div className="flex items-center justify-between pt-1 border-t border-gray-100 dark:border-white/5">
                      <span className="text-[9px] text-gray-400">
                        {customStartDate === customEndDate
                          ? tUi(lang, 'stats_date_single_day')
                          : tUi(lang, 'stats_date_inclusive_days')}
                      </span>
                      <button
                        onClick={() => {
                          setTimeRange('custom');
                          setDatePickerOpen(false);
                        }}
                        className="px-2.5 py-0.5 rounded-md bg-blue-600 hover:bg-blue-500 text-white font-semibold text-[10px]"
                      >
                        {tUi(lang, 'stats_date_apply')}
                      </button>
                    </div>
                  </div>
              )}
            </div>
          </div>

          <button
            onClick={fetchReport}
            disabled={isLoading}
            className="p-1 rounded-lg border border-gray-200/70 dark:border-white/5 hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-500 dark:text-zinc-400 transition-colors"
            title={tUi(lang, 'stats_refresh_title')}
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isLoading ? 'animate-spin text-blue-500' : ''}`} />
          </button>
        </div>
      </div>

      {/* ============================================================== */}
      {/* 视角 1：📊 全局大盘 (100% 真实本地 SQLite + Transcript 数据)      */}
      {/* ============================================================== */}
      {activeView === 'overview' && (() => {
        const globalPerf = computePerfSeries(reportData.recentTraffic, timeRange);
        const ttftVals = reportData.recentTraffic.filter((r) => r.ttftMs > 0).map((r) => r.ttftMs);
        const avgTtftSec =
          (reportData.avgTtftMs ?? 0) > 0
            ? (reportData.avgTtftMs ?? 0) / 1000
            : ttftVals.length > 0
            ? ttftVals.reduce((a, b) => a + b, 0) / ttftVals.length / 1000
            : 0;
        const avgDurSec =
          (reportData.avgDurationMs ?? 0) > 0
            ? (reportData.avgDurationMs ?? 0) / 1000
            : globalPerf.avgDurationMs > 0
            ? globalPerf.avgDurationMs / 1000
            : 0;
        const displayGlobalTps = (reportData.avgSpeedTps ?? 0) > 0 ? (reportData.avgSpeedTps ?? 0) : globalPerf.avgTps;
        const displayGlobalPeakTps = Math.max(reportData.peakSpeedTps ?? 0, globalPerf.peakTps);
        const alertRecords = reportData.recentTraffic.filter((r) => {
          const totalIn = r.inputTokens + r.cachedTokens;
          const cPct = totalIn > 0 ? (r.cachedTokens / totalIn) * 100 : 100;
          // 过滤新会话/子 Agent 首轮正常冷启动，只对长上下文中段异常掉缓存或严重慢速告警
          const midSessionCacheDrop = totalIn >= 30_000 && cPct < 30;
          const severeSlowOutput = r.outputTokens >= 200 && r.tokensPerSecond > 0 && r.tokensPerSecond < 12;
          return midSessionCacheDrop || severeSlowOutput;
        });
        const apiErrors = reportData.apiErrors ?? [];
        const sessionContexts = reportData.sessionContexts ?? [];
        const totalAlertCount = apiErrors.length + alertRecords.length;
        const turnsPerPrompt =
          (reportData.userPromptCount ?? 0) > 0 && reportData.totalRequests > 0
            ? reportData.totalRequests / (reportData.userPromptCount ?? 1)
            : 0;
        const poolSplit = reportData.poolSplit;
        const poolTotalTok = (poolSplit?.geminiTokens ?? 0) + (poolSplit?.claudeTokens ?? 0);
        const geminiPoolPct = poolTotalTok > 0 ? Math.round(((poolSplit?.geminiTokens ?? 0) / poolTotalTok) * 100) : 0;
        const claudePoolPct = poolTotalTok > 0 ? 100 - geminiPoolPct : 0;
        const tb = reportData.timeBreakdown;
        const tbTotalMs = (tb?.ttftMs ?? 0) + (tb?.streamingMs ?? 0) + (tb?.toolExecMs ?? 0);

        return (
          <div className="flex flex-col gap-3 animate-in fade-in-30 duration-200">
            {/* 第一层：4 列 Apple 克制中性排版核心 KPI (色相极简，数值权威统一) */}
            <div className="grid grid-cols-2 md:grid-cols-4 gap-2.5">
              <div className="p-3 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between">
                <div className="flex items-center justify-between text-[10px] text-slate-500 dark:text-zinc-400 font-medium">
                  <span>{tUi(lang, 'stats_kpi_total_tokens')}</span>
                  <Sparkles className="w-3.5 h-3.5 text-slate-400/80 dark:text-zinc-500" />
                </div>
                <div className="my-1.5 flex items-baseline gap-1.5">
                  <span className="text-2xl font-bold font-mono tabular-nums tracking-tight text-slate-900 dark:text-zinc-50">
                    {fmtTok(reportData.totalTokens)}
                  </span>
                  <span className="text-[9.5px] font-medium text-slate-500 dark:text-zinc-400 font-mono">
                    {reportData.totalRequests} reqs
                  </span>
                  {(reportData.totalEstimatedCostUsd ?? 0) >= 0.01 && (
                    <span
                      title={tUi(lang, 'stats_api_equiv_hint')}
                      className="ml-auto text-[10px] font-mono font-medium text-slate-500 dark:text-zinc-400"
                    >
                      ≈ {fmtUsd(reportData.totalEstimatedCostUsd ?? 0)}
                    </span>
                  )}
                </div>
                <div className="text-[10px] text-slate-500 dark:text-zinc-400 tabular-nums flex items-center justify-between pt-1.5 border-t border-slate-100 dark:border-white/[0.08]">
                  <span>{tUi(lang, 'stats_input')} {fmtTok(reportData.totalPromptTokens)}</span>
                  <span
                    title={
                      (reportData.totalThinkingTokens ?? 0) > 0
                        ? `${tUi(lang, 'stats_detail_thinking_tokens')}: ${fmtTok(reportData.totalThinkingTokens ?? 0)}`
                        : undefined
                    }
                    className="flex items-center gap-1"
                  >
                    <span>{tUi(lang, 'stats_output')} {fmtTok(reportData.totalOutputTokens)}</span>
                    {(reportData.totalThinkingTokens ?? 0) > 0 && reportData.totalOutputTokens > 0 && (
                      <span className="text-indigo-500/80 dark:text-indigo-400/80">
                        ({tUi(lang, 'stats_think_short')} {Math.round(((reportData.totalThinkingTokens ?? 0) / reportData.totalOutputTokens) * 100)}%)
                      </span>
                    )}
                  </span>
                </div>
              </div>

              <div className="p-3 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between">
                <div className="flex items-center justify-between text-[10px] text-slate-500 dark:text-zinc-400 font-medium">
                  <span>{tUi(lang, 'stats_kpi_cache_hit')}</span>
                  <ShieldCheck className="w-3.5 h-3.5 text-emerald-500/80 dark:text-emerald-400/80" />
                </div>
                <div className="my-1.5 flex items-baseline gap-1.5">
                  <span className="text-2xl font-bold font-mono tabular-nums tracking-tight text-slate-900 dark:text-zinc-50">
                    {reportData.globalCacheHitRate.toFixed(1)}%
                  </span>
                  {apiErrors.length > 0 ? (
                    <button
                      onClick={() => setOverviewRightTab('alerts')}
                      className="text-[9px] font-mono font-semibold text-rose-500/90 dark:text-rose-400/90 hover:underline"
                      title={tUi(lang, 'stats_alerts_count', { count: totalAlertCount })}
                    >
                      {apiErrors.length} API
                    </button>
                  ) : totalAlertCount > 0 ? (
                    <button
                      onClick={() => setOverviewRightTab('alerts')}
                      className="text-[9px] font-mono text-slate-400 dark:text-zinc-500 hover:text-slate-600 dark:hover:text-zinc-300 hover:underline"
                    >
                      {tUi(lang, 'stats_alerts_count', { count: totalAlertCount })}
                    </button>
                  ) : (
                    <span className="text-[9px] font-mono text-emerald-600/80 dark:text-emerald-400/80">
                      {tUi(lang, 'stats_healthy')}
                    </span>
                  )}
                </div>
                <div className="text-[10px] text-slate-500 dark:text-zinc-400 tabular-nums flex items-center justify-between pt-1.5 border-t border-slate-100 dark:border-white/[0.08]">
                  <span>{tUi(lang, 'stats_reused')} {fmtTok(reportData.totalCachedTokens)}</span>
                  <span className="text-emerald-600/85 dark:text-emerald-400/85 font-medium">
                    {reportData.totalSavedCostUsd >= 0.01
                      ? tUi(lang, 'stats_saved_usd', { val: fmtUsd(reportData.totalSavedCostUsd) })
                      : 'Prompt Cache'}
                  </span>
                </div>
              </div>

              <div className="p-3 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between">
                <div className="flex items-center justify-between text-[10px] text-slate-500 dark:text-zinc-400 font-medium">
                  <span>{tUi(lang, 'stats_kpi_avg_speed')}</span>
                  <TrendingUp className="w-3.5 h-3.5 text-slate-400/80 dark:text-zinc-500" />
                </div>
                <div className="my-1.5 flex items-baseline gap-1.5">
                  <span className="text-2xl font-bold font-mono tabular-nums tracking-tight text-slate-900 dark:text-zinc-50">
                    {displayGlobalTps.toFixed(1)}
                  </span>
                  <span className="text-[9.5px] font-mono text-slate-400 dark:text-zinc-500">tok/s</span>
                </div>
                <div className="text-[10px] text-slate-500 dark:text-zinc-400 tabular-nums flex items-center justify-between pt-1.5 border-t border-slate-100 dark:border-white/[0.08]">
                  <span>{tUi(lang, 'stats_peak')} {displayGlobalPeakTps > 0 ? `${displayGlobalPeakTps.toFixed(0)} tok/s` : '-'}</span>
                  <span
                    title={
                      avgTtftSec > 0 && avgDurSec > 0
                        ? `TTFT ${avgTtftSec.toFixed(2)}s · ${tUi(lang, 'stats_avg_duration')} ${avgDurSec.toFixed(2)}s`
                        : undefined
                    }
                  >
                    {avgTtftSec > 0
                      ? `TTFT ${avgTtftSec.toFixed(1)}s`
                      : avgDurSec > 0
                      ? `${tUi(lang, 'stats_avg_duration')} ${avgDurSec.toFixed(1)}s`
                      : '-'}
                  </span>
                </div>
              </div>

              <div className="p-3 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between">
                <div className="flex items-center justify-between text-[10px] text-slate-500 dark:text-zinc-400 font-medium">
                  <span>{tUi(lang, 'stats_kpi_steps_tools')}</span>
                  <Activity className="w-3.5 h-3.5 text-slate-400/80 dark:text-zinc-500" />
                </div>
                <div className="my-1.5 flex items-baseline gap-1.5">
                  <span className="text-2xl font-bold font-mono tabular-nums tracking-tight text-slate-900 dark:text-zinc-50">
                    {(reportData.agentHierarchy?.totalSteps ?? 0).toLocaleString()}
                  </span>
                  <span className="text-[9.5px] font-mono text-slate-500 dark:text-zinc-400 font-medium">
                    {totalToolCalls.toLocaleString()} calls
                  </span>
                  {turnsPerPrompt > 0 && (
                    <span
                      title={`${reportData.userPromptCount ?? 0} → ${reportData.totalRequests} reqs`}
                      className="ml-auto text-[9.5px] font-mono text-slate-500 dark:text-zinc-400 font-medium"
                    >
                      {tUi(lang, 'stats_turns_per_prompt', { val: turnsPerPrompt.toFixed(1) })}
                    </span>
                  )}
                </div>
                <div className="text-[10px] text-slate-500 dark:text-zinc-400 tabular-nums flex items-center justify-between pt-1.5 border-t border-slate-100 dark:border-white/[0.08]">
                  <span>{tUi(lang, 'stats_sessions_projects', { sessions: reportData.agentHierarchy?.totalConversations ?? 0, projects: reportData.byProject.length })}</span>
                  <span className="inline-flex items-center gap-1 text-emerald-600/85 dark:text-emerald-400/85 font-medium">
                    {(reportData.agentHierarchy?.runningCount ?? 0) > 0 && (
                      <span className="w-1.5 h-1.5 rounded-full bg-emerald-500/85" />
                    )}
                    {tUi(lang, 'stats_running_count', { count: reportData.agentHierarchy?.runningCount ?? 0 })}
                  </span>
                </div>
              </div>
            </div>

            {/* 第二层 (50/50 黄金对称)：Apple HIG 单轴交互光谱 + 耗时三段拆解卡 (6列) + 3-Tab 工具/水位/预警卡 (6列) */}
            <div className="grid grid-cols-1 md:grid-cols-12 gap-3.5 items-stretch">
              {/* 左卡 (6 cols)：单轴可切换遥测光谱 + Apple Storage 风格耗时拆解条 */}
              {(() => {
                const chron = [...reportData.recentTraffic].reverse();
                const useActive = spectrumZoom === 'active' && chron.length >= 2;

                // 当请求数 > 42 时自动聚合为 42 个平滑采样段，既保留完整趋势又避免高频锯齿发糊
                let activeTok: number[] = [];
                let activeCache: number[] = [];
                let activeTps: number[] = [];
                let activeTtft: number[] = [];
                let activeLabels: string[] = [];

                if (useActive) {
                  const maxPts = 42;
                  if (chron.length <= maxPts) {
                    activeTok = chron.map((r) => r.inputTokens + r.cachedTokens + r.outputTokens);
                    activeCache = chron.map((r) =>
                      r.inputTokens + r.cachedTokens > 0 ? (r.cachedTokens / (r.inputTokens + r.cachedTokens)) * 100 : 0
                    );
                    activeTps = chron.map((r) => r.tokensPerSecond || 0);
                    activeTtft = chron.map((r) => (r.ttftMs > 0 ? Math.round((r.ttftMs / 1000) * 100) / 100 : 0));
                    activeLabels = chron.map((r) =>
                      rangeBounds.isSingleDay && r.time.includes(' ') ? r.time.split(' ')[1] : r.time.slice(5)
                    );
                  } else {
                    const step = chron.length / maxPts;
                    for (let i = 0; i < maxPts; i++) {
                      const s = Math.floor(i * step);
                      const e = Math.max(s + 1, Math.floor((i + 1) * step));
                      const slice = chron.slice(s, e);
                      let sumT = 0;
                      let sumC = 0;
                      let sumP = 0;
                      let sumOut = 0;
                      let sumDur = 0;
                      let sumSpd = 0;
                      let spdCnt = 0;
                      let sumTtft = 0;
                      let ttftCnt = 0;
                      for (const r of slice) {
                        sumT += r.inputTokens + r.cachedTokens + r.outputTokens;
                        sumC += r.cachedTokens;
                        sumP += r.inputTokens + r.cachedTokens;
                        if (r.durationMs > 0 && r.outputTokens > 0) {
                          sumOut += r.outputTokens;
                          sumDur += r.durationMs;
                        }
                        if (r.tokensPerSecond > 0) {
                          sumSpd += r.tokensPerSecond;
                          spdCnt += 1;
                        }
                        if (r.ttftMs > 0) {
                          sumTtft += r.ttftMs;
                          ttftCnt += 1;
                        }
                      }
                      activeTok.push(sumT);
                      activeCache.push(sumP > 0 ? (sumC / sumP) * 100 : 0);
                      activeTps.push(
                        sumDur > 0 && sumOut > 0 ? (sumOut * 1000) / sumDur : spdCnt > 0 ? sumSpd / spdCnt : 0
                      );
                      activeTtft.push(ttftCnt > 0 ? Math.round((sumTtft / ttftCnt / 1000) * 100) / 100 : 0);
                      const lastR = slice[slice.length - 1];
                      activeLabels.push(
                        rangeBounds.isSingleDay && lastR.time.includes(' ')
                          ? lastR.time.split(' ')[1]
                          : lastR.time.slice(5)
                      );
                    }
                  }
                }

                const tokSeries = useActive ? activeTok : globalPerf.tokSeries;
                const cacheSeries = useActive ? activeCache : globalPerf.cacheSeries;
                const tpsSeries = useActive ? activeTps : globalPerf.tpsSeries;
                const ttftSeries = useActive ? activeTtft : globalPerf.ttftSeries;
                const timeLabels = useActive ? activeLabels : globalPerf.bucketLabels;

                const nonZeroTok = tokSeries.filter((v) => v > 0);
                const avgTokBucket =
                  nonZeroTok.length > 0 ? nonZeroTok.reduce((a, b) => a + b, 0) / nonZeroTok.length : 0;

                const avgPrefix = tUi(lang, 'stats_avg_ref');
                const metricList: SpectrumMetricLine[] = [
                  {
                    id: 'tok',
                    label: tUi(lang, 'stats_tok_short'),
                    series: tokSeries,
                    color: '#fb7185',
                    format: (v) => fmtTok(Math.round(v)),
                    avgVal: avgTokBucket,
                    avgLabel: avgTokBucket > 0 ? `${avgPrefix} ${fmtTok(Math.round(avgTokBucket))}` : undefined,
                  },
                  {
                    id: 'tps',
                    label: 'tok/s',
                    series: tpsSeries,
                    color: '#38bdf8',
                    format: (v) => `${v.toFixed(1)} t/s`,
                    avgVal: displayGlobalTps,
                    avgLabel: displayGlobalTps > 0 ? `${avgPrefix} ${displayGlobalTps.toFixed(1)} t/s` : undefined,
                  },
                  {
                    id: 'ttft',
                    label: 'TTFT',
                    series: ttftSeries,
                    color: '#c084fc',
                    format: (v) => `${v.toFixed(2)}s`,
                    avgVal: avgTtftSec,
                    avgLabel: avgTtftSec > 0 ? `${avgPrefix} ${avgTtftSec.toFixed(1)}s` : undefined,
                  },
                  {
                    id: 'cache',
                    label: tUi(lang, 'stats_cache'),
                    series: cacheSeries,
                    fixedMax: 100,
                    color: '#34d399',
                    format: (v) => `${v.toFixed(1)}%`,
                    avgVal: reportData.globalCacheHitRate,
                    avgLabel:
                      reportData.globalCacheHitRate > 0
                        ? `${avgPrefix} ${reportData.globalCacheHitRate.toFixed(1)}%`
                        : undefined,
                  },
                ];

                const ttftPct = tbTotalMs > 0 ? Math.round(((tb?.ttftMs ?? 0) / tbTotalMs) * 100) : 0;
                const streamPct = tbTotalMs > 0 ? Math.round(((tb?.streamingMs ?? 0) / tbTotalMs) * 100) : 0;
                const toolPct = tbTotalMs > 0 ? Math.max(0, 100 - ttftPct - streamPct) : 0;
                const chartFooterRight = useActive
                  ? `${chron.length} ${tUi(lang, 'stats_unit_reqs')} ${tUi(lang, 'stats_zoom_live')}`
                  : globalPerf.rangeLabel;

                const ambientGlowRgb =
                  spectrumFocus === 'tok'
                    ? '251,113,133'
                    : spectrumFocus === 'tps'
                    ? '56,189,248'
                    : spectrumFocus === 'ttft'
                    ? '192,132,252'
                    : spectrumFocus === 'cache'
                    ? '52,211,153'
                    : '129,140,248';

                return (
                  <div className="md:col-span-6 relative overflow-hidden p-3.5 rounded-2xl bg-gradient-to-br from-[#232734] via-[#1a1d27] to-[#141720] dark:from-[#07090e] dark:via-[#05070b] dark:to-[#030407] border border-slate-900/15 dark:border-white/[0.14] shadow-[inset_0_1px_0_rgba(255,255,255,0.10),0_14px_34px_-14px_rgba(15,23,42,0.28)] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.12),0_18px_40px_-14px_rgba(0,0,0,0.85)] text-white flex flex-col justify-between gap-2 min-h-[224px]">
                    {/* 纯 CSS radial-gradient 零 GPU 模糊开销氛围层（全平台 120Hz 零负担） */}
                    <div
                      style={{
                        background: `radial-gradient(circle at 0% 0%, rgba(${ambientGlowRgb},0.16) 0%, rgba(${ambientGlowRgb},0) 62%), radial-gradient(circle at 100% 100%, rgba(56,189,248,0.09) 0%, rgba(56,189,248,0) 58%)`,
                      }}
                      className="pointer-events-none absolute inset-0 transition-all duration-300"
                    />

                    <div className="relative flex items-center justify-between gap-2 pb-1.5 border-b border-white/[0.08]">
                      {/* 默认同屏展示全部 4 条曲线；点击任意药丸独显聚焦，再点恢复全显 */}
                      <div className="inline-flex items-center p-0.5 rounded-lg bg-white/[0.06] border border-white/[0.08] gap-0.5">
                        {metricList.map((def) => {
                          const isFocused = spectrumFocus === def.id;
                          const isDimmed = spectrumFocus !== null && !isFocused;
                          return (
                            <button
                              key={def.id}
                              onClick={() =>
                                setSpectrumFocus((prev) =>
                                  prev === def.id ? null : (def.id as 'tok' | 'tps' | 'ttft' | 'cache')
                                )
                              }
                              className={`px-2 py-0.5 rounded-md text-[9.5px] font-medium flex items-center gap-1 transition-all ${
                                isFocused
                                  ? 'bg-white/[0.15] text-white shadow-2xs'
                                  : isDimmed
                                  ? 'text-slate-400/60 hover:text-slate-200'
                                  : 'text-slate-200/90 hover:bg-white/[0.07]'
                              }`}
                            >
                              <span
                                style={{
                                  backgroundColor: def.color,
                                  opacity: isDimmed ? 0.3 : 1,
                                  boxShadow: isDimmed ? 'none' : `0 0 6px ${def.color}bb`,
                                }}
                                className="w-1.5 h-1.5 rounded-full shrink-0"
                              />
                              <span>{def.label}</span>
                            </button>
                          );
                        })}
                      </div>

                      <div className="inline-flex p-0.5 rounded-lg bg-white/[0.06] border border-white/[0.07] shrink-0">
                        <button
                          onClick={() => setSpectrumZoom('active')}
                          className={`px-1.5 py-0.5 rounded-md text-[8.5px] font-medium transition-all ${
                            spectrumZoom === 'active'
                              ? 'bg-white/90 text-slate-900 shadow-2xs'
                              : 'text-slate-400 hover:text-white'
                          }`}
                        >
                          {tUi(lang, 'stats_zoom_active')}
                        </button>
                        <button
                          onClick={() => setSpectrumZoom('24h')}
                          className={`px-1.5 py-0.5 rounded-md text-[8.5px] font-medium transition-all ${
                            spectrumZoom === '24h'
                              ? 'bg-white/90 text-slate-900 shadow-2xs'
                              : 'text-slate-400 hover:text-white'
                          }`}
                        >
                          {globalPerf.zoomLabel}
                        </button>
                      </div>
                    </div>

                    {/* 多维平滑曲线 + 缩放联动时间轴 (单选时叠加虚线均值基线) */}
                    <div className="relative">
                      <InteractiveSpectrumChart
                        height={102}
                        labels={timeLabels}
                        zoomTooltip={tUi(lang, 'stats_chart_zoom_hint')}
                        lines={metricList}
                        focusedId={spectrumFocus}
                        footerRight={chartFooterRight}
                      />
                    </div>

                    {/* Apple Storage 风格三段真实耗时拆解条 (首字 TTFT / 流式生成 / 本地工具实际执行) */}
                    {tbTotalMs > 0 && (
                      <div className="relative flex flex-col gap-1">
                        <div className="w-full h-1.5 rounded-full bg-white/[0.08] overflow-hidden flex gap-px">
                          {ttftPct > 0 && (
                            <div
                              style={{ width: `${ttftPct}%` }}
                              className="h-full bg-violet-400/80 first:rounded-l-full last:rounded-r-full"
                              title={`${tUi(lang, 'stats_time_ttft')} ${ttftPct}% (${fmtDurCompact(tb?.ttftMs ?? 0)})`}
                            />
                          )}
                          {streamPct > 0 && (
                            <div
                              style={{ width: `${streamPct}%` }}
                              className="h-full bg-sky-400/80 first:rounded-l-full last:rounded-r-full"
                              title={`${tUi(lang, 'stats_time_stream')} ${streamPct}% (${fmtDurCompact(tb?.streamingMs ?? 0)})`}
                            />
                          )}
                          {toolPct > 0 && (
                            <div
                              style={{ width: `${toolPct}%` }}
                              className="h-full bg-amber-300/80 first:rounded-l-full last:rounded-r-full"
                              title={`${tUi(lang, 'stats_time_tools')} ${toolPct}% (${fmtDurCompact(tb?.toolExecMs ?? 0)})`}
                            />
                          )}
                        </div>
                        <div className="flex items-center justify-between text-[9.5px] tabular-nums text-slate-200/90">
                          <span className="flex items-center gap-1">
                            <span className="w-1.5 h-1.5 rounded-full bg-violet-400" />
                            {tUi(lang, 'stats_time_ttft')} {ttftPct}%
                            <span className="text-slate-400">({fmtDurCompact(tb?.ttftMs ?? 0)})</span>
                          </span>
                          <span className="flex items-center gap-1">
                            <span className="w-1.5 h-1.5 rounded-full bg-sky-400" />
                            {tUi(lang, 'stats_time_stream')} {streamPct}%
                            <span className="text-slate-400">({fmtDurCompact(tb?.streamingMs ?? 0)})</span>
                          </span>
                          <span className="flex items-center gap-1">
                            <span className="w-1.5 h-1.5 rounded-full bg-amber-300" />
                            {tUi(lang, 'stats_time_tools')} {toolPct}%
                            <span className="text-slate-400">({fmtDurCompact(tb?.toolExecMs ?? 0)})</span>
                          </span>
                        </div>
                      </div>
                    )}
                  </div>
                );
              })()}

              {/* 右卡 (6 cols)：3-Tab 胶囊遥测卡 (工具矩阵 / 上下文水位 / 真实异常预警) */}
              <div className="md:col-span-6 p-3.5 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between gap-2">
                <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5 gap-2">
                  <div className="inline-flex p-0.5 rounded-lg bg-gray-100/90 dark:bg-white/[0.06]">
                    <button
                      onClick={() => setOverviewRightTab('tools')}
                      className={`px-2 py-0.5 rounded-md text-[10px] font-semibold flex items-center gap-1 transition-all ${
                        overviewRightTab === 'tools'
                          ? 'bg-white dark:bg-zinc-800 text-slate-900 dark:text-white shadow-2xs'
                          : 'text-slate-500 dark:text-zinc-400 hover:text-slate-700 dark:hover:text-zinc-200'
                      }`}
                    >
                      <Terminal className={`w-3 h-3 ${overviewRightTab === 'tools' ? 'text-slate-700 dark:text-zinc-200' : 'text-slate-400 dark:text-zinc-500'}`} />
                      <span>{tUi(lang, 'stats_tool_matrix')}</span>
                    </button>
                    <button
                      onClick={() => setOverviewRightTab('context')}
                      className={`px-2 py-0.5 rounded-md text-[10px] font-semibold flex items-center gap-1 transition-all ${
                        overviewRightTab === 'context'
                          ? 'bg-white dark:bg-zinc-800 text-slate-900 dark:text-white shadow-2xs'
                          : 'text-slate-500 dark:text-zinc-400 hover:text-slate-700 dark:hover:text-zinc-200'
                      }`}
                    >
                      <Layers className={`w-3 h-3 ${overviewRightTab === 'context' ? 'text-slate-700 dark:text-zinc-200' : 'text-slate-400 dark:text-zinc-500'}`} />
                      <span>{tUi(lang, 'stats_context_tab')}</span>
                    </button>
                    <button
                      onClick={() => setOverviewRightTab('alerts')}
                      className={`px-2 py-0.5 rounded-md text-[10px] font-semibold flex items-center gap-1 transition-all ${
                        overviewRightTab === 'alerts'
                          ? 'bg-white dark:bg-zinc-800 text-slate-900 dark:text-white shadow-2xs'
                          : 'text-slate-500 dark:text-zinc-400 hover:text-slate-700 dark:hover:text-zinc-200'
                      }`}
                    >
                      <ShieldCheck className={`w-3 h-3 ${apiErrors.length > 0 ? 'text-rose-500/85' : overviewRightTab === 'alerts' ? 'text-slate-700 dark:text-zinc-200' : 'text-slate-400 dark:text-zinc-500'}`} />
                      <span>{`${tUi(lang, 'stats_alerts_title')} (${totalAlertCount})`}</span>
                    </button>
                  </div>

                  <div className="flex items-center gap-1.5 text-[9px] text-gray-400 font-mono shrink-0">
                    {overviewRightTab === 'tools' && (
                      <>
                        {poolTotalTok > 0 && (poolSplit?.claudeTokens ?? 0) > 0 && (
                          <span
                            title={`Gemini ${fmtTok(poolSplit?.geminiTokens ?? 0)} · Claude ${fmtTok(poolSplit?.claudeTokens ?? 0)}`}
                            className="hidden xl:inline-flex px-1.5 py-0.5 rounded bg-gray-100 dark:bg-white/[0.06] text-gray-600 dark:text-zinc-300"
                          >
                            G {geminiPoolPct}% · C {claudePoolPct}%
                          </span>
                        )}
                        <span>{totalToolCalls.toLocaleString()} calls</span>
                      </>
                    )}
                    {overviewRightTab === 'context' && (
                      <span>
                        {(reportData.checkpointCount ?? 0) > 0
                          ? tUi(lang, 'stats_compactions_count', { count: reportData.checkpointCount ?? 0 })
                          : `${sessionContexts.length} ${tUi(lang, 'stats_unit_sessions')}`}
                      </span>
                    )}
                    {overviewRightTab === 'alerts' && apiErrors.length > 0 && (
                      <span className="px-1.5 py-0.5 rounded bg-rose-500/12 text-rose-600 dark:text-rose-400 font-semibold">
                        {apiErrors.length} API
                      </span>
                    )}
                  </div>
                </div>

                {overviewRightTab === 'tools' ? (
                  <div className="flex-1 grid grid-cols-2 gap-x-4 gap-y-2 content-center">
                    {reportData.toolMetrics.slice(0, 6).map((t, idx) => {
                      const filled = Math.max(1, Math.round((t.sharePct / Math.max(reportData.toolMetrics[0]?.sharePct || 1, 1)) * 8));
                      const segColors = [
                        'bg-blue-500/80 dark:bg-blue-400/85',
                        'bg-blue-500/60 dark:bg-blue-400/65',
                        'bg-indigo-400/75 dark:bg-indigo-400/70',
                        'bg-sky-400/75 dark:bg-sky-400/70',
                        'bg-slate-400/70 dark:bg-zinc-400/60',
                        'bg-slate-400/50 dark:bg-zinc-500/50',
                      ];
                      return (
                        <div key={t.toolName} className="flex flex-col gap-1">
                          <div className="flex items-center justify-between text-[10px] font-mono gap-1">
                            <span className="font-semibold text-slate-700 dark:text-zinc-200 truncate">{t.toolName}</span>
                            <span className="text-slate-500 dark:text-zinc-400 shrink-0 flex items-center gap-1">
                              <span>{t.callCount}</span>
                              {t.errorCount > 0 && (
                                <span
                                  title={`${t.errorCount} ${tUi(lang, 'errors')}`}
                                  className="px-1 py-0 rounded bg-rose-500/10 text-rose-600/85 dark:text-rose-400/85 text-[8.5px] font-medium"
                                >
                                  {t.errorCount}!
                                </span>
                              )}
                            </span>
                          </div>
                          <div className="grid grid-cols-8 gap-0.5">
                            {Array.from({ length: 8 }, (_, bIdx) => (
                              <div
                                key={bIdx}
                                className={`h-2 rounded-xs ${
                                  bIdx < filled ? segColors[idx % segColors.length] : 'bg-gray-100 dark:bg-white/[0.06]'
                                }`}
                              />
                            ))}
                          </div>
                        </div>
                      );
                    })}
                  </div>
                ) : overviewRightTab === 'context' ? (
                  <div className="flex-1 flex flex-col justify-center gap-2">
                    {sessionContexts.slice(0, 4).map((sc) => {
                      const pct = Math.min(100, Math.max(0, sc.usagePct));
                      const barColor =
                        pct >= 80 ? 'bg-rose-500' : pct >= 60 ? 'bg-amber-500' : 'bg-indigo-500';
                      const textColor =
                        pct >= 80
                          ? 'text-rose-600 dark:text-rose-400'
                          : pct >= 60
                          ? 'text-amber-600 dark:text-amber-400'
                          : 'text-indigo-600 dark:text-indigo-400';
                      return (
                        <button
                          key={sc.sessionId}
                          onClick={() => {
                            setWorkspaceFilter('all');
                            setSearchQuery(sc.sessionId);
                            setActiveView('ledger');
                          }}
                          className="w-full text-left flex flex-col gap-1 px-2 py-1 rounded-lg hover:bg-gray-50 dark:hover:bg-white/[0.03] transition-colors"
                          title={`${sc.title} · ${sc.model} · ${sc.lastActiveTime}`}
                        >
                          <div className="flex items-center justify-between gap-2 text-[10px] font-mono">
                            <div className="flex items-center gap-1.5 min-w-0">
                              {sc.isRunning && (
                                <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 shrink-0" />
                              )}
                              <span className="px-1 py-0 rounded bg-gray-100 dark:bg-white/[0.07] text-gray-500 dark:text-zinc-400 text-[8.5px] shrink-0">
                                {sc.workspace}
                              </span>
                              <span className="font-semibold text-gray-800 dark:text-zinc-200 truncate">
                                {sc.title}
                              </span>
                            </div>
                            <div className="flex items-center gap-1.5 shrink-0">
                              {sc.checkpointCount > 0 && (
                                <span
                                  title={tUi(lang, 'stats_compactions_count', { count: sc.checkpointCount })}
                                  className="text-[8.5px] text-purple-500 dark:text-purple-400"
                                >
                                  ↻{sc.checkpointCount}
                                </span>
                              )}
                              <span className="text-gray-400">
                                {fmtTok(sc.usedTokens)}/{fmtTok(sc.budgetTokens)}
                              </span>
                              <span className={`font-bold ${textColor}`}>{pct.toFixed(0)}%</span>
                            </div>
                          </div>
                          <div className="w-full h-1 rounded-full bg-gray-100 dark:bg-white/[0.06] overflow-hidden">
                            <div
                              style={{ width: `${Math.max(2, pct)}%` }}
                              className={`h-full rounded-full ${barColor}`}
                            />
                          </div>
                        </button>
                      );
                    })}
                    {sessionContexts.length === 0 && (
                      <div className="py-6 text-center text-[10.5px] text-gray-400 dark:text-zinc-500 font-medium">
                        {tUi(lang, 'stats_no_context_data')}
                      </div>
                    )}
                  </div>
                ) : (
                  <div className="flex-1 flex flex-col justify-center gap-1.5">
                    {apiErrors.slice(0, 4).map((e, idx) => (
                      <button
                        key={`${e.conversationId}-${e.timestamp}-${idx}`}
                        onClick={() => {
                          setWorkspaceFilter('all');
                          setSearchQuery(e.conversationId);
                          setActiveView('ledger');
                        }}
                        title={e.message}
                        className="w-full text-left px-2.5 py-1.5 rounded-xl bg-rose-50/70 dark:bg-rose-500/[0.07] border border-rose-200/60 dark:border-rose-500/20 flex items-center justify-between gap-2 text-[10px] font-mono"
                      >
                        <div className="flex items-center gap-1.5 min-w-0">
                          <span className="px-1.5 py-0 rounded bg-rose-500/15 text-rose-600 dark:text-rose-400 font-bold text-[8.5px] shrink-0">
                            {e.code}
                          </span>
                          <span className="truncate font-semibold text-gray-800 dark:text-zinc-200">
                            {e.workspaceName} · {e.model}
                          </span>
                        </div>
                        <div className="flex items-center gap-2 shrink-0 text-[9px]">
                          <span className="text-rose-500/80 dark:text-rose-400/80 max-w-[110px] truncate hidden sm:inline">
                            {e.message}
                          </span>
                          <span className="text-gray-400">
                            {e.timeStr.includes(' ') ? e.timeStr.split(' ')[1] : e.timeStr}
                          </span>
                        </div>
                      </button>
                    ))}
                    {alertRecords.slice(0, Math.max(0, 4 - Math.min(4, apiErrors.length))).map((r) => {
                      const totalIn = r.inputTokens + r.cachedTokens;
                      const cPct = totalIn > 0 ? Math.round((r.cachedTokens / totalIn) * 100) : 0;
                      return (
                        <button
                          key={r.id}
                          onClick={() => {
                            setSelectedRecord(r);
                            setActiveView('ledger');
                          }}
                          className="w-full text-left px-2.5 py-1.5 rounded-xl bg-amber-50/70 dark:bg-amber-500/[0.06] border border-amber-200/60 dark:border-amber-500/20 flex items-center justify-between gap-2 text-[10px] font-mono"
                        >
                          <span className="truncate font-semibold text-gray-800 dark:text-zinc-200">
                            {r.workspace} · {r.requestedModel}
                          </span>
                          <div className="flex items-center gap-2 shrink-0">
                            <span className="text-amber-600 dark:text-amber-400 font-bold">{cPct}% {tUi(lang, 'stats_cache')}</span>
                            <span className="text-sky-600 dark:text-sky-400">
                              {r.tokensPerSecond > 0 ? `${r.tokensPerSecond.toFixed(0)} t/s` : '-'}
                            </span>
                          </div>
                        </button>
                      );
                    })}
                    {totalAlertCount === 0 && (
                      <div className="py-6 text-center text-[10.5px] text-emerald-600 dark:text-emerald-400 font-medium">
                        {tUi(lang, 'stats_all_healthy')}
                      </div>
                    )}
                  </div>
                )}
              </div>
            </div>

            {/* 第三层 (50/50 双排行榜)：工程消耗排行 + 真实模型性能排行 */}
            <div className="grid grid-cols-1 md:grid-cols-12 gap-3.5 items-stretch">
              <div className="md:col-span-6 p-3.5 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col gap-2">
                <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
                  <span className="text-[11px] font-semibold text-slate-900 dark:text-zinc-100 flex items-center gap-1.5">
                    <FolderGit2 className="w-3.5 h-3.5 text-slate-400 dark:text-zinc-500" />
                    <span>{tUi(lang, 'stats_top_workspaces')}</span>
                  </span>
                  <button onClick={() => setActiveView('project')} className="text-[9.5px] text-slate-500 dark:text-zinc-400 hover:text-blue-600 dark:hover:text-blue-400 transition-colors">
                    {tUi(lang, 'stats_all_arrow')}
                  </button>
                </div>
                <div className="flex flex-col gap-1.5">
                  {reportData.byProject.slice(0, 4).map((p, idx) => {
                    const pct = reportData.totalTokens > 0 ? (p.totalTokens / reportData.totalTokens) * 100 : 0;
                    return (
                      <button
                        key={p.workspaceName}
                        onClick={() => {
                          setSelectedProjectName(p.workspaceName);
                          setActiveView('project');
                        }}
                        className="flex flex-col gap-1 text-left hover:bg-gray-50 dark:hover:bg-white/[0.03] px-2 py-1 rounded-lg transition-colors"
                      >
                        <div className="flex items-center justify-between text-[10.5px]">
                          <span className="flex items-center gap-1.5 min-w-0">
                            <span className="w-4 h-4 rounded-full bg-slate-100 dark:bg-white/[0.07] text-slate-600 dark:text-zinc-300 text-[9px] font-semibold flex items-center justify-center shrink-0">
                              {idx + 1}
                            </span>
                            <span className="font-semibold text-slate-800 dark:text-zinc-200 truncate">{p.workspaceName}</span>
                          </span>
                          <span className="tabular-nums shrink-0 flex items-center gap-1.5 text-[10.5px]">
                            {p.estimatedCostUsd >= 0.1 && (
                              <span className="text-[10px] font-normal text-slate-500 dark:text-zinc-400">
                                ≈ {fmtUsd(p.estimatedCostUsd)}
                              </span>
                            )}
                            <span className="font-semibold text-slate-900 dark:text-zinc-50">
                              {fmtTok(p.totalTokens)} <span className="text-slate-500 dark:text-zinc-400 font-normal">({pct.toFixed(0)}%)</span>
                            </span>
                          </span>
                        </div>
                        <div className="w-full h-1 rounded-full bg-gray-100 dark:bg-white/[0.06] overflow-hidden">
                          <div style={{ width: `${Math.max(4, pct)}%` }} className="h-full rounded-full bg-gradient-to-r from-blue-500/80 to-indigo-400/75" />
                        </div>
                      </button>
                    );
                  })}
                </div>
              </div>

              <div className="md:col-span-6 p-3.5 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col gap-2">
                <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
                  <span className="text-[11px] font-semibold text-slate-900 dark:text-zinc-100 flex items-center gap-1.5">
                    <Cpu className="w-3.5 h-3.5 text-slate-400 dark:text-zinc-500" />
                    <span>{tUi(lang, 'stats_top_models')}</span>
                  </span>
                  <button onClick={() => setActiveView('model')} className="text-[9.5px] text-slate-500 dark:text-zinc-400 hover:text-blue-600 dark:hover:text-blue-400 transition-colors">
                    {tUi(lang, 'stats_compare_arrow')}
                  </button>
                </div>
                <div className="flex flex-col gap-1.5">
                  {reportData.byModel.slice(0, 4).map((m, idx) => (
                    <button
                      key={m.requestedModel}
                      onClick={() => {
                        setSelectedModelFilter(m.requestedModel);
                        setActiveView('model');
                      }}
                      className="flex items-center justify-between gap-2 text-left hover:bg-gray-50 dark:hover:bg-white/[0.03] px-2 py-1.5 rounded-lg transition-colors"
                    >
                      <div className="flex items-center gap-1.5 min-w-0">
                        <span className="w-4 h-4 rounded-full bg-slate-100 dark:bg-white/[0.07] text-slate-600 dark:text-zinc-300 text-[9px] font-semibold flex items-center justify-center shrink-0">
                          {idx + 1}
                        </span>
                        <span className="text-[10.5px] font-semibold font-mono text-slate-800 dark:text-zinc-200 truncate">
                          {m.requestedModel}
                        </span>
                      </div>
                      <div
                        className="flex items-center gap-2 tabular-nums text-[10px] shrink-0"
                        title={`${m.physicalModel} · ${fmtTok(m.totalTokens)} · ${m.totalRequests} reqs`}
                      >
                        <span className="px-1.5 py-0.5 rounded bg-emerald-500/[0.10] text-emerald-700 dark:text-emerald-400 font-medium">
                          {m.cacheHitRate.toFixed(0)}%
                        </span>
                        {(m.avgTtftMs || 0) > 0 && (
                          <span className="text-slate-500 dark:text-zinc-400 w-10 text-right">
                            {((m.avgTtftMs || 0) / 1000).toFixed(1)}s
                          </span>
                        )}
                        <span className="text-slate-700 dark:text-zinc-200 w-12 text-right font-medium">
                          {(m.avgTps || 0) > 0 ? `${(m.avgTps || 0).toFixed(0)} t/s` : '-'}
                        </span>
                        <span className="font-semibold text-slate-900 dark:text-zinc-50 w-9 text-right">
                          {m.sharePercentage.toFixed(0)}%
                        </span>
                      </div>
                    </button>
                  ))}
                </div>
              </div>
            </div>
          </div>
        );
      })()}

      {/* ============================================================== */}
      {/* 视角 2：📁 项目详情 (4:3/3:4 战报卡 + 真实工程遥测，无虚假账号归因) */}
      {/* ============================================================== */}
      {activeView === 'project' && (
        <div className="flex flex-col gap-3 animate-in fade-in-30 duration-200">
          <div className="relative z-30 bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] rounded-xl p-2 px-3 shadow-2xs flex items-center justify-between gap-3">
            <div className="flex items-center gap-2.5 min-w-0">
              <span className="text-[11px] font-semibold text-gray-500 dark:text-zinc-400 shrink-0 flex items-center gap-1.5">
                <FolderGit2 className="w-3.5 h-3.5 text-indigo-500" />
                <span>{tUi(lang, 'stats_target_project')}</span>
              </span>
              <div ref={projectDropdownRef} className="relative">
                <button
                  onClick={() => setProjectDropdownOpen((prev) => !prev)}
                  className="px-3 py-1.5 rounded-lg bg-gray-100/90 dark:bg-white/[0.05] hover:bg-gray-200/80 dark:hover:bg-white/[0.08] border border-gray-200/70 dark:border-white/10 transition-all flex items-center gap-2 text-[11.5px] font-semibold text-gray-900 dark:text-zinc-100"
                >
                  <FolderGit2 className="w-3.5 h-3.5 text-indigo-500 shrink-0" />
                  <span className="truncate max-w-[180px]">
                    {currentProject?.workspaceName === '__ALL__'
                      ? tUi(lang, 'stats_all_projects_agg')
                      : currentProject?.workspaceName || '-'}
                  </span>
                  {currentProject && (
                    <span className="text-[9px] px-1.5 py-[1px] rounded bg-indigo-500/10 text-indigo-600 dark:text-indigo-400 font-mono font-medium shrink-0">
                      {fmtTok(currentProject.totalTokens)}
                    </span>
                  )}
                  <ChevronDown className={`w-3.5 h-3.5 text-gray-400 transition-transform duration-150 ${projectDropdownOpen ? 'rotate-180' : ''}`} />
                </button>
                {projectDropdownOpen && (
                  <div className="absolute left-0 mt-1.5 w-72 rounded-xl bg-white/95 dark:bg-[#18191d]/95 backdrop-blur-2xl border border-gray-200/80 dark:border-white/10 shadow-xl z-30 overflow-hidden">
                      <div className="p-2 border-b border-gray-100 dark:border-white/5 flex items-center gap-2">
                        <Search className="w-3.5 h-3.5 text-gray-400 shrink-0 ml-1" />
                        <input
                          type="text"
                          value={projectSearchQuery}
                          onChange={(e) => setProjectSearchQuery(e.target.value)}
                          placeholder={tUi(lang, 'stats_search_workspace')}
                          autoFocus
                          className="w-full bg-transparent text-[11px] text-gray-800 dark:text-zinc-100 placeholder-gray-400 focus:outline-none"
                        />
                      </div>
                      <div className="mini-scroll max-h-60 overflow-y-auto p-1.5 flex flex-col gap-0.5">
                        {selectableProjects
                          .filter((p) => {
                            const label = p.workspaceName === '__ALL__' ? tUi(lang, 'stats_all_projects_agg') : p.workspaceName;
                            return !projectSearchQuery.trim() || label.toLowerCase().includes(projectSearchQuery.toLowerCase());
                          })
                          .map((p) => {
                            const isSelected = p.workspaceName === currentProject?.workspaceName;
                            const displayTitle = p.workspaceName === '__ALL__' ? tUi(lang, 'stats_all_projects_agg') : p.workspaceName;
                            return (
                              <button
                                key={p.workspaceName}
                                onClick={() => {
                                  setSelectedProjectName(p.workspaceName);
                                  setProjectDropdownOpen(false);
                                  setProjectSearchQuery('');
                                }}
                                className={`w-full px-2.5 py-2 rounded-lg text-left transition-all flex items-center justify-between gap-2 ${
                                  isSelected ? 'bg-indigo-500/10 text-indigo-600 dark:text-indigo-400' : 'hover:bg-gray-100/80 dark:hover:bg-white/[0.04] text-gray-800 dark:text-zinc-200'
                                }`}
                              >
                                <div className="flex flex-col min-w-0">
                                  <span className="text-[11px] font-semibold truncate">{displayTitle}</span>
                                  <span className="text-[9px] text-gray-400 font-mono truncate">
                                    {fmtTok(p.totalTokens)} · {p.totalSteps.toLocaleString()} {tUi(lang, 'stats_unit_steps')} · {p.conversationCount} {tUi(lang, 'stats_unit_sessions')}
                                  </span>
                                </div>
                                {isSelected && <Check className="w-3.5 h-3.5 text-indigo-500 shrink-0" />}
                              </button>
                            );
                          })}
                      </div>
                    </div>
                )}
              </div>
            </div>

            <div className="flex items-center gap-2 shrink-0">
              <button
                onClick={() => setShareModalOpen(true)}
                className="px-2.5 py-1 rounded-lg bg-gradient-to-r from-blue-600 to-indigo-600 hover:from-blue-500 hover:to-indigo-500 text-white text-[10.5px] font-semibold shadow-xs flex items-center gap-1.5 transition-all"
              >
                <Share2 className="w-3 h-3" />
                <span>{tUi(lang, 'stats_share_poster')}</span>
              </button>
              <span className="text-[10px] font-mono text-gray-400 pl-1">
                {Math.max(1, selectableProjects.findIndex((p) => p.workspaceName === currentProject?.workspaceName) + 1)} / {Math.max(1, selectableProjects.length)}
              </span>
              <div className="flex items-center gap-1">
                <button
                  onClick={() => {
                    const idx = selectableProjects.findIndex((p) => p.workspaceName === currentProject?.workspaceName);
                    const prevIdx = idx <= 0 ? selectableProjects.length - 1 : idx - 1;
                    if (selectableProjects[prevIdx]) setSelectedProjectName(selectableProjects[prevIdx].workspaceName);
                  }}
                  className="p-1 rounded-md bg-gray-100/80 dark:bg-white/[0.04] hover:bg-gray-200 dark:hover:bg-white/10 text-gray-600 dark:text-zinc-300"
                >
                  <ChevronLeft className="w-3.5 h-3.5" />
                </button>
                <button
                  onClick={() => {
                    const idx = selectableProjects.findIndex((p) => p.workspaceName === currentProject?.workspaceName);
                    const nextIdx = idx >= selectableProjects.length - 1 ? 0 : idx + 1;
                    if (selectableProjects[nextIdx]) setSelectedProjectName(selectableProjects[nextIdx].workspaceName);
                  }}
                  className="p-1 rounded-md bg-gray-100/80 dark:bg-white/[0.04] hover:bg-gray-200 dark:hover:bg-white/10 text-gray-600 dark:text-zinc-300"
                >
                  <ChevronRight className="w-3.5 h-3.5" />
                </button>
              </div>
            </div>
          </div>

          {currentProject && (() => {
            const displayProjectTitle =
              currentProject.workspaceName === '__ALL__'
                ? tUi(lang, 'stats_all_projects_agg')
                : currentProject.workspaceName;
            const posterRangeBadge =
              timeRange === 'today'
                ? tUi(lang, 'stats_range_today_zero')
                : timeRange === '7days'
                ? tUi(lang, 'stats_range_7d')
                : timeRange === '30days'
                ? tUi(lang, 'stats_range_30d')
                : timeRange === 'all'
                ? tUi(lang, 'stats_range_all')
                : customStartDate === customEndDate
                ? customStartDate
                : `${customStartDate} ~ ${customEndDate}`;

            const projTraffic =
              currentProject.workspaceName === '__ALL__'
                ? reportData.recentTraffic
                : reportData.recentTraffic.filter((r) => r.workspace === currentProject.workspaceName);

            // 基于真实请求时间戳与单步耗时计算活跃研发时长（15 分钟滑动活跃窗口聚合，无流量时回退步数估算）
            const realActiveMs = (() => {
              const tsList: Array<{ ts: number; dur: number }> = [];
              for (const r of projTraffic) {
                const ts = r.timestamp || Date.parse(r.time.replace(' ', 'T'));
                if (!Number.isNaN(ts) && ts > 0) {
                  tsList.push({ ts, dur: Math.min(180_000, Math.max(4_000, r.durationMs || 12_000)) });
                }
              }
              if (tsList.length === 0) return 0;
              tsList.sort((a, b) => a.ts - b.ts);
              const SESSION_GAP_MS = 15 * 60 * 1000;
              let accMs = tsList[0].dur;
              for (let i = 1; i < tsList.length; i++) {
                const gap = tsList[i].ts - tsList[i - 1].ts;
                if (gap >= 0 && gap <= SESSION_GAP_MS) {
                  accMs += gap;
                } else {
                  accMs += tsList[i].dur;
                }
              }
              return accMs;
            })();
            const estHours =
              realActiveMs > 0
                ? Math.max(0.1, realActiveMs / 3_600_000)
                : Math.max(0.1, +(currentProject.totalSteps * 0.0046).toFixed(2));
            const estHrsInt = Math.floor(estHours);
            const estMinsInt = Math.max(realActiveMs > 0 || currentProject.totalSteps > 0 ? 1 : 0, Math.round((estHours - estHrsInt) * 60));

            const recentIn = projTraffic.reduce((s, r) => s + r.inputTokens, 0);
            const recentCache = projTraffic.reduce((s, r) => s + r.cachedTokens, 0);
            const recentOut = projTraffic.reduce((s, r) => s + r.outputTokens, 0);
            const recentSum = recentIn + recentCache + recentOut;

            const projCacheRate =
              recentIn + recentCache > 0 ? (recentCache / (recentIn + recentCache)) * 100 : reportData.globalCacheHitRate;
            const outRatio = recentSum > 0 ? Math.min(0.25, recentOut / recentSum) : 0.015;

            // 当 projTraffic 包含当前时段真实全量流水时直接采用真实累计值；否则按项目总 Token 等比还原
            const projOut = recentSum > 0 ? recentOut : Math.round(currentProject.totalTokens * outRatio);
            const projCache =
              recentSum > 0
                ? recentCache
                : Math.round(Math.max(0, currentProject.totalTokens - projOut) * (projCacheRate / 100));
            const projIn =
              recentSum > 0 ? recentIn : Math.max(0, currentProject.totalTokens - projOut - projCache);
            const projThink = 0;
            const avgStepsPerSession = Math.round(
              currentProject.totalSteps / Math.max(1, currentProject.conversationCount)
            );
            const formattedLastActive = currentProject.lastActiveTime
              ? currentProject.lastActiveTime.replace('T', ' ').split('.')[0]
              : '-';

            // 100% 真实工程级多维遥测聚合：24h 分布、7天×6时段矩阵、模型 6 维能力指标与 32 段全环会话脉冲
            const projHourly = new Array(24).fill(0);
            const rawWeekMatrix: number[][] = Array.from({ length: 7 }, () => new Array(6).fill(0));
            const dayTotals = new Array(7).fill(0);
            const realModelAgg = new Map<
              string,
              {
                reqs: number;
                inp: number;
                cache: number;
                out: number;
                tot: number;
                measuredOut: number;
                measuredDurMs: number;
                tpsSum: number;
                tpsCnt: number;
              }
            >();

            for (const r of projTraffic) {
              const tok = r.inputTokens + r.cachedTokens + r.outputTokens;
              const ts = r.timestamp || Date.parse(r.time.replace(' ', 'T'));
              let hour = -1;
              let dayIdx = -1;
              if (!Number.isNaN(ts) && ts > 0) {
                const d = new Date(ts);
                hour = d.getHours();
                dayIdx = (d.getDay() + 6) % 7;
              } else {
                const m = r.time.match(/(\d{2}):\d{2}/);
                if (m) hour = parseInt(m[1], 10);
              }

              if (hour >= 0 && hour < 24) {
                projHourly[hour] += tok;
                if (dayIdx >= 0 && dayIdx < 7) {
                  const slotIdx = Math.min(5, Math.floor(hour / 4));
                  rawWeekMatrix[dayIdx][slotIdx] += tok;
                  dayTotals[dayIdx] += tok;
                }
              }

              let mEntry = realModelAgg.get(r.requestedModel);
              if (!mEntry) {
                mEntry = { reqs: 0, inp: 0, cache: 0, out: 0, tot: 0, measuredOut: 0, measuredDurMs: 0, tpsSum: 0, tpsCnt: 0 };
                realModelAgg.set(r.requestedModel, mEntry);
              }
              mEntry.reqs += 1;
              mEntry.inp += r.inputTokens;
              mEntry.cache += r.cachedTokens;
              mEntry.out += r.outputTokens;
              mEntry.tot += tok;
              if (r.durationMs > 0 && r.outputTokens > 0) {
                mEntry.measuredOut += r.outputTokens;
                mEntry.measuredDurMs += r.durationMs;
              }
              if (r.tokensPerSecond > 0) {
                mEntry.tpsSum += r.tokensPerSecond;
                mEntry.tpsCnt += 1;
              }
            }

            // Token 构成占比
            const sumComp = Math.max(1, projCache + projIn + projThink + projOut);
            const compCachePct = +((projCache / sumComp) * 100).toFixed(1);
            const compInPct = +((projIn / sumComp) * 100).toFixed(1);
            const compOutPct = Math.max(0.2, +((projOut / sumComp) * 100).toFixed(1));

            // 合并真实模型明细（包含 6 维实测指标：算力占比、缓存率、单步上下文、生成速度、单步输出、调用频次）
            const mergedModelNames = Array.from(
              new Set([
                ...Array.from(realModelAgg.keys()),
                ...currentProject.modelBreakdown.map(([m]) => m),
              ])
            );
            const baseProjectTotal = Math.max(1, recentSum > 0 ? recentSum : currentProject.totalTokens);
            const projModelRows = mergedModelNames
              .map((model) => {
                const real = realModelAgg.get(model);
                const bdPct = currentProject.modelBreakdown.find(([m]) => m === model)?.[1] ?? 0;
                const matchedModel = reportData.byModel.find((m) => m.requestedModel === model);
                if (real && real.tot > 0) {
                  const cRate = real.inp + real.cache > 0 ? (real.cache / (real.inp + real.cache)) * 100 : projCacheRate;
                  const pct = (real.tot / baseProjectTotal) * 100;
                  const avgTps =
                    real.measuredDurMs > 0 && real.measuredOut > 0
                      ? (real.measuredOut * 1000) / real.measuredDurMs
                      : real.tpsCnt > 0
                      ? real.tpsSum / real.tpsCnt
                      : matchedModel?.avgTps || reportData.avgSpeedTps || 0;
                  return {
                    model,
                    reqs: real.reqs,
                    tot: real.tot,
                    cache: real.cache,
                    inp: real.inp,
                    out: real.out,
                    cRate,
                    pct,
                    avgTps,
                    avgCtx: (real.inp + real.cache) / Math.max(1, real.reqs),
                    avgOut: real.out / Math.max(1, real.reqs),
                  };
                }
                const tot = Math.round((currentProject.totalTokens * bdPct) / 100);
                const cRate = matchedModel ? matchedModel.cacheHitRate : projCacheRate;
                const out = Math.round(tot * outRatio);
                const nonOut = Math.max(0, tot - out);
                const cache = Math.round(nonOut * (cRate / 100));
                const inp = Math.max(0, nonOut - cache);
                const reqs = matchedModel
                  ? matchedModel.totalRequests
                  : Math.max(1, Math.round((currentProject.totalSteps * bdPct) / 100));
                const avgTps = matchedModel?.avgTps || reportData.avgSpeedTps || 0;
                return {
                  model,
                  reqs,
                  tot,
                  cache,
                  inp,
                  out,
                  cRate,
                  pct: bdPct,
                  avgTps,
                  avgCtx: nonOut / Math.max(1, reqs),
                  avgOut: out / Math.max(1, reqs),
                };
              })
              .filter((row) => row.tot > 0 || row.pct > 0)
              .sort((a, b) => b.tot - a.tot);

            const wavePalette = ['#34d399', '#a855f7', '#38bdf8', '#f59e0b', '#fb7185', '#818cf8', '#2dd4bf', '#f472b6', '#a3e635', '#22d3ee'];
            const modelColorIdxMap = new Map<string, number>();
            projModelRows.forEach((r, idx) => modelColorIdxMap.set(r.model, idx));

            // 32 柱 360° 全环真实会话脉冲重采样（将当前时段全部真实请求按时间先后映射为 32 个环绕扇区，彻底消除半圈留白）
            const chronProj = [...projTraffic].reverse();
            const PULSE_COUNT = 32;
            const pulseBars32 = (() => {
              const rawToks = new Array(PULSE_COUNT).fill(0);
              const colorIdxs = new Array(PULSE_COUNT).fill(0);
              const labels = new Array(PULSE_COUNT).fill('');

              if (chronProj.length >= PULSE_COUNT) {
                const step = chronProj.length / PULSE_COUNT;
                for (let i = 0; i < PULSE_COUNT; i++) {
                  const s = Math.floor(i * step);
                  const e = Math.max(s + 1, Math.floor((i + 1) * step));
                  const mMap = new Map<string, number>();
                  let sum = 0;
                  for (let k = s; k < e; k++) {
                    const r = chronProj[k];
                    const t = r.inputTokens + r.cachedTokens + r.outputTokens + (r.thinkingTokens ?? 0);
                    sum += t;
                    mMap.set(r.requestedModel, (mMap.get(r.requestedModel) || 0) + t);
                  }
                  rawToks[i] = sum;
                  let topM = chronProj[s].requestedModel;
                  let topV = 0;
                  mMap.forEach((v, m) => {
                    if (v > topV) {
                      topV = v;
                      topM = m;
                    }
                  });
                  colorIdxs[i] = modelColorIdxMap.get(topM) ?? i % wavePalette.length;
                  const sampleTime = chronProj[s].time;
                  labels[i] = sampleTime.includes(' ') ? sampleTime.split(' ')[1].slice(0, 5) : sampleTime.slice(0, 5);
                }
              } else if (chronProj.length > 0) {
                for (let i = 0; i < PULSE_COUNT; i++) {
                  const pos = chronProj.length === 1 ? 0 : (i / (PULSE_COUNT - 1)) * (chronProj.length - 1);
                  const idx = Math.min(chronProj.length - 1, Math.round(pos));
                  const r = chronProj[idx];
                  const t = r.inputTokens + r.cachedTokens + r.outputTokens + (r.thinkingTokens ?? 0);
                  const fracDist = Math.abs(pos - idx);
                  rawToks[i] = t * (0.72 + 0.28 * Math.cos(fracDist * Math.PI));
                  colorIdxs[i] = modelColorIdxMap.get(r.requestedModel) ?? idx % wavePalette.length;
                  labels[i] = r.time.includes(' ') ? r.time.split(' ')[1].slice(0, 5) : r.time.slice(0, 5);
                }
              } else {
                const fallbackRows = projModelRows.length > 0 ? projModelRows : [{ model: 'default', tot: 100, pct: 100 }];
                for (let i = 0; i < PULSE_COUNT; i++) {
                  const mIdx = i % fallbackRows.length;
                  rawToks[i] = Math.max(1, fallbackRows[mIdx].tot);
                  colorIdxs[i] = mIdx;
                  labels[i] = `#${i + 1}`;
                }
              }

              const minT = Math.min(...rawToks);
              const maxT = Math.max(1, ...rawToks);
              const floorBase = minT * 0.62;
              const span = Math.max(1, maxT - floorBase);
              return rawToks.map((tok, i) => ({
                tok,
                norm: 0.28 + 0.72 * Math.pow(Math.max(0, Math.min(1, (tok - floorBase) / span)), 0.58),
                color: wavePalette[colorIdxs[i] % wavePalette.length],
                label: labels[i],
              }));
            })();

            // 自适应热力矩阵：多日范围展示「周一~周日 × 6时段」，单日或仅单天活跃时自动切换为「主力模型 × 6段真实时序切片」
            const weekDays = tUi(lang, 'stats_weekdays').split(',');
            const timeSlots = ['00h', '04h', '08h', '12h', '16h', '20h'];
            const activeWeekdays = dayTotals.filter((v) => v > 0).length;
            const useSessionSliceMatrix = rangeBounds.isSingleDay || activeWeekdays <= 1;

            const splitModelVariant = (name: string) => {
              const m = name.match(/^(.*?)\s*\(([^)]+)\)\s*$/);
              if (!m) return { base: name, variant: '', compactVariant: '' };
              const rawV = m[2].trim();
              const compactMap: Record<string, string> = {
                xhigh: 'XHigh',
                high: 'High',
                medium: 'Med',
                low: 'Low',
                minimal: 'Min',
                thinking: 'Think',
              };
              const compact = compactMap[rawV.toLowerCase()] ?? (rawV.length > 6 ? rawV.slice(0, 5) : rawV);
              return { base: m[1].trim(), variant: `(${rawV})`, compactVariant: `(${compact})` };
            };

            const shortenModelLabel = (name: string) => {
              const { base, variant } = splitModelVariant(name);
              const rawV = variant.replace(/[()]/g, '').trim();
              const shortTagMap: Record<string, string> = {
                xhigh: 'XH',
                high: 'H',
                medium: 'M',
                low: 'L',
                minimal: 'Min',
                thinking: 'T',
              };
              const suffix = rawV
                ? `·${shortTagMap[rawV.toLowerCase()] ?? rawV.slice(0, 2).toUpperCase()}`
                : '';

              let shortBase = base
                .replace(/^gemini[-\s]+/i, 'G')
                .replace(/^claude[-\s]+/i, '')
                .replace(/\bhydrogen\b/i, 'Hydro')
                .replace(/\bcarbon\b/i, 'Carb')
                .replace(/\bpreview\b/i, '')
                .replace(/\s+/g, ' ')
                .trim();

              const maxBaseLen = suffix ? Math.max(5, 11 - suffix.length) : 11;
              if (shortBase.length > maxBaseLen) {
                shortBase = `${shortBase.slice(0, maxBaseLen - 1)}…`;
              }
              return `${shortBase}${suffix}`;
            };

            const adaptiveMatrix = (() => {
              if (!useSessionSliceMatrix) {
                const maxCell = Math.max(1, ...rawWeekMatrix.flatMap((r) => r));
                const heat = rawWeekMatrix.map((row) =>
                  row.map((v) => (v <= 0 ? 0 : Math.min(100, Math.max(15, Math.round(Math.sqrt(v / maxCell) * 100)))))
                );
                return {
                  modeTag: '7d × 24h',
                  colLabels: timeSlots,
                  rowLabels: weekDays,
                  rawVals: rawWeekMatrix,
                  heat,
                };
              }

              // 单日 / 单活跃日精细视图：Top 6 真实模型 × 6 段按时间推进的真实会话切片
              const topM = projModelRows.slice(0, 6);
              const colCount = 6;
              const rawGrid: number[][] = Array.from({ length: topM.length }, () => new Array(colCount).fill(0));
              const colLabels = new Array(colCount).fill('');

              if (chronProj.length > 0 && topM.length > 0) {
                const mRowMap = new Map<string, number>();
                topM.forEach((m, idx) => mRowMap.set(m.model, idx));
                const step = chronProj.length / colCount;
                for (let c = 0; c < colCount; c++) {
                  const s = Math.floor(c * step);
                  const e = Math.max(s + 1, Math.floor((c + 1) * step));
                  const sample = chronProj[Math.min(chronProj.length - 1, s)];
                  colLabels[c] = sample.time.includes(' ') ? sample.time.split(' ')[1].slice(0, 5) : `#${c + 1}`;
                  for (let k = s; k < e && k < chronProj.length; k++) {
                    const r = chronProj[k];
                    const rIdx = mRowMap.get(r.requestedModel);
                    if (rIdx !== undefined) {
                      rawGrid[rIdx][c] += r.inputTokens + r.cachedTokens + r.outputTokens + (r.thinkingTokens ?? 0);
                    }
                  }
                }
              } else {
                for (let c = 0; c < colCount; c++) colLabels[c] = timeSlots[c];
              }

              const maxCell = Math.max(1, ...rawGrid.flatMap((r) => r));
              const heat = rawGrid.map((row) =>
                row.map((v) => (v <= 0 ? 0 : Math.min(100, Math.max(16, Math.round(Math.sqrt(v / maxCell) * 100)))))
              );
              return {
                modeTag: tUi(lang, 'stats_matrix_mode_slices', { count: topM.length }),
                colLabels,
                rowLabels: topM.map((m) => shortenModelLabel(m.model)),
                rawVals: rawGrid,
                heat,
              };
            })();

            // 6 维真实工程遥测极坐标谐波玫瑰曲线（TOK 算力 / HIT 缓存 / CTX 上下文 / TPS 速率 / OUT 输出 / REQ 频次）
            const maxModelPct = Math.max(1, ...projModelRows.map((r) => r.pct));
            const maxModelReqs = Math.max(1, ...projModelRows.map((r) => r.reqs));
            const maxModelAvgCtx = Math.max(1, ...projModelRows.map((r) => r.avgCtx));
            const maxModelAvgOut = Math.max(1, ...projModelRows.map((r) => r.avgOut));
            const maxModelTps = Math.max(10, ...projModelRows.map((r) => r.avgTps));

            const buildPolarWavePoints = (
              row: (typeof projModelRows)[number],
              modelIdx: number,
              cx: number,
              cy: number,
              minR: number,
              maxR: number
            ) => {
              const span = maxR - minR;
              // 6 项 100% 真实实测遥测归一化指标 (0.28 .. 1.0)
              const axes6 = [
                0.30 + 0.70 * Math.sqrt(row.pct / maxModelPct),
                0.28 + 0.72 * Math.min(1, row.cRate / 100),
                0.30 + 0.70 * Math.sqrt(row.avgCtx / maxModelAvgCtx),
                0.30 + 0.70 * Math.min(1, row.avgTps / maxModelTps),
                0.30 + 0.70 * Math.sqrt(row.avgOut / maxModelAvgOut),
                0.30 + 0.70 * Math.sqrt(row.reqs / maxModelReqs),
              ];
              // 12 点闭合谐波玫瑰控制点（偶数点为真实维度波峰，奇数点为呼吸波谷，模型间带微小相位交织防完全重叠）
              const N = 12;
              const phase = (modelIdx % 5) * 0.16;
              return Array.from({ length: N }, (_, i) => {
                const angle = (i * 2 * Math.PI) / N - Math.PI / 2 + phase;
                const axisIdx = Math.floor(i / 2) % 6;
                if (i % 2 === 0) {
                  const r = minR + span * axes6[axisIdx];
                  return { x: cx + r * Math.cos(angle), y: cy + r * Math.sin(angle) };
                }
                const nextAxis = axes6[(axisIdx + 1) % 6];
                const valleyNorm = 0.22 + 0.34 * ((axes6[axisIdx] + nextAxis) / 2);
                const r = minR + span * valleyNorm;
                return { x: cx + r * Math.cos(angle), y: cy + r * Math.sin(angle) };
              });
            };

            const buildPolarWavePath = (row: (typeof projModelRows)[number], modelIdx: number) => {
              const pts = buildPolarWavePoints(row, modelIdx, 80, 80, 22, 63);
              const N = pts.length;
              let d = '';
              for (let i = 0; i < N; i++) {
                const p0 = pts[i];
                const p1 = pts[(i + 1) % N];
                const mx = (p0.x + p1.x) / 2;
                const my = (p0.y + p1.y) / 2;
                if (i === 0) {
                  const prev = pts[N - 1];
                  d += `M ${((prev.x + p0.x) / 2).toFixed(1)} ${((prev.y + p0.y) / 2).toFixed(1)} `;
                }
                d += `Q ${p0.x.toFixed(1)} ${p0.y.toFixed(1)} ${mx.toFixed(1)} ${my.toFixed(1)} `;
              }
              return `${d}Z`;
            };

            const polarAxisLabels = ['TOK', 'HIT', 'CTX', 'TPS', 'OUT', 'REQ'];
            const renderPolarRadar = (sizeClass: string, clipId = 'polar-logo-hero') => (
              <svg viewBox="0 0 160 160" className={`${sizeClass} overflow-visible`}>
                <defs>
                  <filter id="neon-wave-glow" x="-30%" y="-30%" width="160%" height="160%">
                    <feGaussianBlur stdDeviation="2.2" result="blur" />
                    <feMerge>
                      <feMergeNode in="blur" />
                      <feMergeNode in="SourceGraphic" />
                    </feMerge>
                  </filter>
                  <clipPath id={clipId}>
                    <circle cx="80" cy="80" r="16.8" />
                  </clipPath>
                </defs>
                {/* 外圈与内圈极坐标底网 */}
                <circle cx="80" cy="80" r="64" fill="none" stroke="rgba(255,255,255,0.13)" strokeWidth="0.8" />
                <circle cx="80" cy="80" r="44" fill="none" stroke="rgba(255,255,255,0.07)" strokeWidth="0.7" />
                <circle cx="80" cy="80" r="24" fill="none" stroke="rgba(255,255,255,0.11)" strokeWidth="0.8" />
                {[0, 60, 120, 180, 240, 300].map((deg, idx) => {
                  const rad = ((deg - 90) * Math.PI) / 180;
                  return (
                    <g key={deg}>
                      <line
                        x1={80 + 24 * Math.cos(rad)}
                        y1={80 + 24 * Math.sin(rad)}
                        x2={80 + 64 * Math.cos(rad)}
                        y2={80 + 64 * Math.sin(rad)}
                        stroke="rgba(255,255,255,0.08)"
                        strokeWidth="0.7"
                      />
                      <text
                        x={80 + 73 * Math.cos(rad)}
                        y={80 + 73 * Math.sin(rad) + 2.5}
                        textAnchor="middle"
                        className="text-[6.5px] font-mono fill-zinc-500 font-semibold"
                      >
                        {polarAxisLabels[idx]}
                      </text>
                    </g>
                  );
                })}
                {/* 主力模型 6 维实测极坐标谐波玫瑰（叠绘 Top 6 模型保证层次清晰） */}
                {[...projModelRows.slice(0, 6)].reverse().map((row, revIdx, arr) => {
                  const origIdx = arr.length - 1 - revIdx;
                  return (
                    <path
                      key={row.model}
                      d={buildPolarWavePath(row, origIdx)}
                      fill="none"
                      stroke={wavePalette[origIdx % wavePalette.length]}
                      strokeWidth={origIdx < 3 ? '2.1' : '1.5'}
                      strokeLinejoin="round"
                      filter="url(#neon-wave-glow)"
                      opacity={origIdx === 0 ? 0.96 : origIdx < 3 ? 0.85 : 0.68}
                    >
                      <title>{`${row.model}: ${fmtTok(row.tot)} · ${tUi(lang, 'stats_hit_short')} ${row.cRate.toFixed(0)}% · ${row.avgTps.toFixed(0)} tok/s`}</title>
                    </path>
                  );
                })}
                {/* 中央专属黑曜石钛金品牌徽章 */}
                <circle cx="80" cy="80" r="19" fill="none" stroke="rgba(251,191,36,0.22)" strokeWidth="0.8" />
                <circle cx="80" cy="80" r="17" fill="#050507" />
                <image
                  href="/icon.png"
                  x="62"
                  y="62"
                  width="36"
                  height="36"
                  preserveAspectRatio="xMidYMid slice"
                  clipPath={`url(#${clipId})`}
                />
                <circle cx="80" cy="80" r="16.8" fill="none" stroke="rgba(255,255,255,0.2)" strokeWidth="0.9" />
              </svg>
            );

            return (
              <div className="flex flex-col gap-3.5">
                {/* ========================================================= */}
                {/* 【第一层：50/50 黄金对称：极坐标波纹海报 (6列) + 结构占比 (6列)】 */}
                {/* ========================================================= */}
                <div className="grid grid-cols-1 md:grid-cols-12 gap-3.5 items-stretch">
                  {/* 左卡 (6 cols)：Hyper Charts 极坐标多模型发光波纹海报 */}
                  <div className="md:col-span-6 flex flex-col">
                    <div className="relative h-full min-h-[264px] w-full rounded-2xl bg-gradient-to-br from-[#232734] via-[#1a1d27] to-[#141720] dark:from-[#07090e] dark:via-[#05070b] dark:to-[#030407] border border-slate-900/15 dark:border-white/[0.14] shadow-[inset_0_1px_0_rgba(255,255,255,0.10),0_14px_34px_-14px_rgba(15,23,42,0.28)] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.12),0_18px_40px_-14px_rgba(0,0,0,0.85)] p-4 flex flex-col justify-between overflow-hidden text-white">
                      {/* 顶部：工程名 + 路径 + 放大按钮 */}
                      <div className="relative flex items-center justify-between gap-2 pb-2">
                        <div className="flex items-center gap-2 min-w-0">
                          <span className="text-sm font-extrabold tracking-tight text-white truncate">
                            {currentProject.workspaceName === '__ALL__'
                              ? tUi(lang, 'stats_all_projects_short')
                              : currentProject.workspaceName}
                          </span>
                          <span className="text-[10px] text-zinc-500 font-mono truncate hidden sm:inline">
                            {currentProject.path}
                          </span>
                        </div>
                        <button
                          onClick={() => setShareModalOpen(true)}
                          title={tUi(lang, 'stats_expand_poster')}
                          className="p-1.5 rounded-full bg-emerald-500/15 hover:bg-emerald-500/25 border border-emerald-400/30 text-emerald-300 transition-colors shrink-0"
                        >
                          <Maximize2 className="w-3.5 h-3.5" />
                        </button>
                      </div>

                      {/* 主体六宫格 + 中央大尺寸极坐标多模型波纹 */}
                      <div className="relative flex-1 flex items-center justify-between gap-2 pt-1 min-h-0">
                        {/* 左列：左上总 Token + 左中无感渐隐滚动全模型列表 + 左下输入/输出微卡 */}
                        <div className="w-[148px] shrink-0 flex flex-col justify-between h-full py-1 gap-2 min-h-0">
                          <div>
                            <div className="text-[9.5px] text-emerald-400 font-mono flex items-center gap-1 mb-0.5">
                              <span>●</span><span>{tUi(lang, 'stats_total_short')}</span>
                            </div>
                            <div className="flex items-baseline gap-1.5">
                              <span className="text-[22px] font-black font-mono tracking-tight text-white leading-none">
                                {fmtTok(currentProject.totalTokens)}
                              </span>
                              <span className="text-[9.5px] font-mono text-emerald-400 font-bold">
                                ↑{projCacheRate.toFixed(0)}%
                              </span>
                            </div>
                          </div>

                          <div className="flex flex-col gap-1 max-h-[86px] overflow-y-auto no-scrollbar fade-mask-bottom pb-2 text-[9px] font-mono text-zinc-300">
                            {projModelRows.map((row, idx) => {
                              const { base, compactVariant } = splitModelVariant(row.model);
                              return (
                                <div
                                  key={row.model}
                                  title={`${row.model} · ${fmtTok(row.tot)} (${row.pct.toFixed(1)}%)`}
                                  className="flex items-center justify-between gap-1 shrink-0"
                                >
                                  <span className="flex items-center gap-1 min-w-0 flex-1">
                                    <span
                                      className="w-1.5 h-1.5 rounded-full shrink-0"
                                      style={{ backgroundColor: wavePalette[idx % wavePalette.length] }}
                                    />
                                    <span className="truncate">{base}</span>
                                    {compactVariant && (
                                      <span className="text-[8px] text-zinc-400 shrink-0">{compactVariant}</span>
                                    )}
                                  </span>
                                  <span className="text-[8.5px] text-zinc-400 tabular-nums shrink-0">{row.pct.toFixed(0)}%</span>
                                </div>
                              );
                            })}
                          </div>

                          <div className="flex flex-col gap-1.5">
                            <div className="flex items-center gap-2">
                              <span className="w-6 h-6 rounded-lg bg-purple-600/90 flex items-center justify-center text-[8.5px] font-bold shrink-0">IN</span>
                              <div className="leading-tight">
                                <div className="text-[8px] text-zinc-500">{tUi(lang, 'stats_net_input')}</div>
                                <div className="text-[11px] font-bold font-mono text-white">{fmtTok(projIn)}</div>
                              </div>
                            </div>
                            <div className="flex items-center gap-2">
                              <span className="w-6 h-6 rounded-lg bg-white/10 flex items-center justify-center text-[8.5px] font-bold text-zinc-300 shrink-0">OUT</span>
                              <div className="leading-tight">
                                <div className="text-[8px] text-zinc-500">{tUi(lang, 'stats_answer_output')}</div>
                                <div className="text-[11px] font-bold font-mono text-white">{fmtTok(projOut)}</div>
                              </div>
                            </div>
                          </div>
                        </div>

                        {/* 中列：大尺寸极坐标多模型发光波纹 */}
                        <div className="flex-1 flex items-center justify-center min-w-0">
                          {renderPolarRadar('w-[188px] h-[188px]')}
                        </div>

                        {/* 右列：右上耗时 + 右中步数 + 右下会话明细 */}
                        <div className="w-[110px] shrink-0 flex flex-col justify-between h-full py-1 text-right">
                          <div>
                            <div className="text-[9.5px] text-amber-400 font-mono mb-0.5">✦ {tUi(lang, 'stats_active_time')}</div>
                            <div className="text-[19px] font-black font-mono tracking-tight text-white leading-none">
                              {estHrsInt}h {estMinsInt}m
                            </div>
                            <div className="text-[8.5px] text-zinc-500 font-mono mt-1">{tUi(lang, 'stats_session_span')}</div>
                          </div>

                          <div>
                            <div className="text-[9.5px] text-purple-400 font-mono mb-0.5">◆ {tUi(lang, 'stats_steps_label')}</div>
                            <div className="text-[19px] font-black font-mono tracking-tight text-white leading-none">
                              {currentProject.totalSteps.toLocaleString()}
                            </div>
                            <div className="text-[8.5px] text-zinc-500 font-mono mt-1">{tUi(lang, 'stats_cache')} {fmtTok(projCache)}</div>
                          </div>

                          <div className="flex flex-col gap-1 text-[9.5px] font-mono pt-2 border-t border-white/[0.08]">
                            <div className="flex items-center justify-between text-zinc-400">
                              <span>{tUi(lang, 'stats_archived_sessions')}</span>
                              <span className="text-white font-bold">{currentProject.conversationCount}</span>
                            </div>
                            <div className="flex items-center justify-between text-zinc-400">
                              <span>{tUi(lang, 'stats_avg_steps_per_sess')}</span>
                              <span className="text-white font-bold">{avgStepsPerSession}</span>
                            </div>
                          </div>
                        </div>
                      </div>
                    </div>
                  </div>

                  {/* 右卡 (6 cols)：与左卡等高对齐的圆角卡片 */}
                  <div className="md:col-span-6 p-4 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between gap-3">
                    {/* 顶部 4 列极简大数字指标带 (参考 electrys Charge Statistics) */}
                    <div className="grid grid-cols-4 gap-2 pb-2 border-b border-gray-100 dark:border-white/5">
                      <div>
                        <span className="text-[9px] text-gray-400 block">{tUi(lang, 'stats_cache_hit_label')}</span>
                        <span className="text-base font-extrabold font-mono text-emerald-600 dark:text-emerald-400">
                          {projCacheRate.toFixed(1)}%
                        </span>
                      </div>
                      <div>
                        <span className="text-[9px] text-gray-400 block">{tUi(lang, 'stats_cache_reused_label')}</span>
                        <span className="text-base font-extrabold font-mono text-purple-600 dark:text-purple-400">
                          {fmtTok(projCache)}
                        </span>
                      </div>
                      <div>
                        <span className="text-[9px] text-gray-400 block">{tUi(lang, 'stats_net_input')}</span>
                        <span className="text-base font-extrabold font-mono text-amber-500">
                          {fmtTok(projIn)}
                        </span>
                      </div>
                      <div>
                        <span className="text-[9px] text-gray-400 block">{tUi(lang, 'stats_steps_per_sess_label')}</span>
                        <span className="text-base font-extrabold font-mono text-blue-600 dark:text-blue-400">
                          {avgStepsPerSession}
                        </span>
                      </div>
                    </div>

                    {/* 中部：Apple Storage 分段比例条 + 全部模型算力分布列表 */}
                    <div className="flex flex-col gap-2.5 flex-1 min-h-0">
                      <div className="flex items-center justify-between text-[10px]">
                        <span className="font-semibold text-gray-800 dark:text-zinc-200 flex items-center gap-1.5">
                          <TrendingUp className="w-3.5 h-3.5 text-purple-500" />
                          <span>{tUi(lang, 'stats_all_models_breakdown', { count: projModelRows.length })}</span>
                        </span>
                        <span className="text-[9px] text-gray-400 font-mono">{tUi(lang, 'stats_last_active')} {formattedLastActive}</span>
                      </div>

                      {/* Apple Storage 风格连续多色分段条 */}
                      <div className="h-3 w-full rounded-full bg-gray-100 dark:bg-white/[0.06] p-0.5 flex items-center gap-0.5 overflow-hidden">
                        {projModelRows.map((row, idx) => {
                          const segColors = [
                            'bg-emerald-500',
                            'bg-purple-500',
                            'bg-sky-500',
                            'bg-amber-400',
                            'bg-rose-500',
                            'bg-indigo-500',
                            'bg-teal-400',
                          ];
                          return (
                            <div
                              key={row.model}
                              title={`${row.model}: ${row.pct.toFixed(1)}%`}
                              style={{ width: `${Math.max(3, row.pct)}%` }}
                              className={`h-full rounded-full transition-all duration-300 ${segColors[idx % segColors.length]}`}
                            />
                          );
                        })}
                      </div>

                      {/* 全模型明细行 (极细透明滚动条 + 右侧留白绝不压数字) */}
                      <div className="flex-1 flex flex-col gap-1.5 overflow-y-auto mini-scroll fade-mask-bottom pr-1.5 pb-2 max-h-[144px]">
                        {projModelRows.map((row, idx) => {
                          const dotColors = [
                            'bg-emerald-500',
                            'bg-purple-500',
                            'bg-sky-500',
                            'bg-amber-400',
                            'bg-rose-500',
                            'bg-indigo-500',
                            'bg-teal-400',
                          ];
                          return (
                            <div
                              key={row.model}
                              className="flex items-center justify-between gap-2 px-2.5 py-1.5 rounded-xl bg-gray-50/90 dark:bg-white/[0.03] border border-gray-100/80 dark:border-white/[0.04] shrink-0"
                            >
                              <div className="flex items-center gap-2 min-w-0 flex-1">
                                <span className={`w-2 h-2 rounded-full shrink-0 ${dotColors[idx % dotColors.length]}`} />
                                <span className="text-[10.5px] font-semibold font-mono text-gray-800 dark:text-zinc-200 truncate">
                                  {row.model}
                                </span>
                              </div>
                              <div className="flex items-center gap-2 shrink-0 font-mono text-[10px] tabular-nums">
                                <span className="px-1.5 py-0.5 rounded-md bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 text-[9px] font-semibold">
                                  {row.cRate.toFixed(0)}% {tUi(lang, 'stats_cache')}
                                </span>
                                <span className="font-bold text-gray-900 dark:text-zinc-100 w-12 text-right">
                                  {fmtTok(row.tot)}
                                </span>
                                <span className="text-gray-400 w-10 text-right">
                                  {row.pct.toFixed(1)}%
                                </span>
                              </div>
                            </div>
                          );
                        })}
                      </div>
                    </div>

                    {/* 底部结构摘要 */}
                    <div className="pt-2 border-t border-gray-100 dark:border-white/5 flex items-center justify-between text-[9px] font-mono text-gray-500 dark:text-zinc-400">
                      <span className="flex items-center gap-1"><span className="w-2 h-2 rounded-full bg-emerald-500" />{tUi(lang, 'stats_cache')} {compCachePct}%</span>
                      <span className="flex items-center gap-1"><span className="w-2 h-2 rounded-full bg-amber-400" />{tUi(lang, 'stats_input')} {compInPct}%</span>
                      <span className="flex items-center gap-1"><span className="w-2 h-2 rounded-full bg-sky-500" />{tUi(lang, 'stats_output')} {compOutPct}%</span>
                      <span className="text-gray-400">{projModelRows.length} {tUi(lang, 'stats_unit_models')}</span>
                    </div>
                  </div>
                </div>

                {/* ========================================================= */}
                {/* 【第二层：50/50 零滚动收尾：自适应算力点阵热力图 (6列) + 工程动态融合卡 (6列)】 */}
                {/* ========================================================= */}
                <div className="grid grid-cols-1 md:grid-cols-12 gap-3.5 items-stretch">
                  {/* 左卡 (6 cols)：自适应多日星期 / 单日模型时序点阵热力图 */}
                  <div className="md:col-span-6 p-3.5 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between gap-2">
                    <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
                      <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                        <Activity className="w-3.5 h-3.5 text-emerald-500" />
                        <span>
                          {useSessionSliceMatrix
                            ? tUi(lang, 'stats_matrix_session_slice')
                            : tUi(lang, 'stats_matrix_work_hour')}
                        </span>
                      </span>
                      <div className="flex items-center gap-2 text-[8.5px] font-mono text-gray-400">
                        <div className="flex items-center gap-0.5">
                          <span className="w-2 h-2 rounded-xs bg-gray-100 dark:bg-white/[0.06]" />
                          <span className="w-2 h-2 rounded-xs bg-indigo-500" />
                          <span className="w-2 h-2 rounded-xs bg-emerald-400" />
                          <span className="w-2 h-2 rounded-xs bg-lime-400" />
                          <span className="w-2 h-2 rounded-xs bg-amber-400" />
                        </div>
                        <span>{adaptiveMatrix.modeTag}</span>
                      </div>
                    </div>

                    <div className="flex flex-col gap-1">
                      <div className="flex items-center gap-1.5 text-[8px] font-mono text-gray-400 text-center">
                        <span className={useSessionSliceMatrix ? 'w-16 shrink-0' : 'w-8 shrink-0'} />
                        <div className="flex-1 grid grid-cols-6 gap-1.5">
                          {adaptiveMatrix.colLabels.map((s, ci) => (
                            <span key={ci} className="truncate">{s}</span>
                          ))}
                        </div>
                      </div>
                      {adaptiveMatrix.rowLabels.map((rowLabel, dIdx) => (
                        <div key={dIdx} className="flex items-center gap-1.5">
                          <span
                            title={rowLabel}
                            className={`${
                              useSessionSliceMatrix ? 'w-16' : 'w-8'
                            } shrink-0 text-[8.5px] font-mono text-gray-400 text-right pr-1 truncate`}
                          >
                            {rowLabel}
                          </span>
                          <div className="flex-1 grid grid-cols-6 gap-1.5">
                            {adaptiveMatrix.heat[dIdx].map((intensity, sIdx) => (
                              <div
                                key={sIdx}
                                title={`${rowLabel} · ${adaptiveMatrix.colLabels[sIdx]} · ${fmtTok(adaptiveMatrix.rawVals[dIdx]?.[sIdx] || 0)}`}
                                className={`h-4 rounded flex items-center justify-center text-[8px] font-mono font-bold transition-all ${
                                  intensity >= 80
                                    ? 'bg-amber-400 text-gray-950'
                                    : intensity >= 60
                                    ? 'bg-lime-400 text-gray-950'
                                    : intensity >= 40
                                    ? 'bg-emerald-400 text-gray-950'
                                    : intensity >= 15
                                    ? 'bg-indigo-500 text-white'
                                    : 'bg-gray-100 dark:bg-white/[0.04] text-gray-400 dark:text-zinc-600'
                                }`}
                              >
                                {intensity > 0 ? String(intensity).padStart(2, '0') : '·'}
                              </div>
                            ))}
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>

                  {/* 右卡 (6 cols)：合并「关键会话里程碑」+「最近真实请求」分段融合卡 */}
                  <div className="md:col-span-6 p-3.5 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between gap-2">
                    <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
                      <div className="inline-flex p-0.5 rounded-lg bg-gray-100/90 dark:bg-white/[0.06]">
                        <button
                          onClick={() => setProjectActivityTab('milestones')}
                          className={`px-2.5 py-0.5 rounded-md text-[10.5px] font-semibold flex items-center gap-1 transition-all ${
                            projectActivityTab === 'milestones'
                              ? 'bg-white dark:bg-zinc-800 text-gray-900 dark:text-white shadow-2xs'
                              : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                          }`}
                        >
                          <BookOpen className="w-3 h-3 text-indigo-500" />
                          <span>{tUi(lang, 'stats_milestones')}</span>
                          <span className="text-[9px] font-mono opacity-60">{currentProject.sessionTitles.length}</span>
                        </button>
                        <button
                          onClick={() => setProjectActivityTab('requests')}
                          className={`px-2 py-0.5 rounded-md text-[10px] font-semibold flex items-center gap-1 transition-all ${
                            projectActivityTab === 'requests'
                              ? 'bg-white dark:bg-zinc-800 text-gray-900 dark:text-white shadow-2xs'
                              : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                          }`}
                        >
                          <Terminal className="w-3 h-3 text-blue-500" />
                          <span>{tUi(lang, 'stats_recent_requests')}</span>
                        </button>
                        <button
                          onClick={() => setProjectActivityTab('perf')}
                          className={`px-2 py-0.5 rounded-md text-[10px] font-semibold flex items-center gap-1 transition-all ${
                            projectActivityTab === 'perf'
                              ? 'bg-white dark:bg-zinc-800 text-gray-900 dark:text-white shadow-2xs'
                              : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                          }`}
                        >
                          <TrendingUp className="w-3 h-3 text-emerald-500" />
                          <span>{tUi(lang, 'stats_perf_curves')}</span>
                        </button>
                      </div>

                      <button
                        onClick={() => {
                          setWorkspaceFilter(currentProject.workspaceName === '__ALL__' ? 'all' : currentProject.workspaceName);
                          setSearchQuery('');
                          setActiveView('ledger');
                        }}
                        className="text-[9.5px] text-blue-600 dark:text-blue-400 hover:underline flex items-center gap-0.5 font-medium"
                      >
                        <span>{tUi(lang, 'stats_all_ledger')}</span>
                        <ChevronRight className="w-3 h-3" />
                      </button>
                    </div>

                    {projectActivityTab === 'perf' ? (() => {
                      const projModelsList = Array.from(
                        new Set([
                          ...currentProject.modelBreakdown.map(([m]) => m),
                          ...projTraffic.map((r) => r.requestedModel),
                        ])
                      );
                      const activeProjModel =
                        projPerfModel === '__ALL__' || !projModelsList.includes(projPerfModel) ? '__ALL__' : projPerfModel;
                      const filteredProjPerfTraffic =
                        activeProjModel === '__ALL__'
                          ? projTraffic
                          : projTraffic.filter((r) => r.requestedModel === activeProjModel);
                      const perf = computePerfSeries(filteredProjPerfTraffic, timeRange);
                      const matchedMeta =
                        activeProjModel === '__ALL__'
                          ? null
                          : reportData.byModel.find((m) => m.requestedModel === activeProjModel) || null;
                      const displayProjCache =
                        perf.count > 0 ? perf.avgCache : matchedMeta ? matchedMeta.cacheHitRate : projCacheRate;
                      const displayProjTps =
                        perf.avgTps > 0 ? perf.avgTps : matchedMeta ? matchedMeta.avgTps || 0 : 0;
                      return (
                        <div className="flex-1 flex flex-col gap-1.5 min-h-0">
                          {/* 项目内全模型微筛选条 */}
                          <div className="flex items-center justify-between gap-2 min-w-0">
                            <div className="flex items-center gap-1 min-w-0 flex-1 overflow-x-auto no-scrollbar fade-mask-right pr-4">
                              <button
                                onClick={() => setProjPerfModel('__ALL__')}
                                className={`px-1.5 py-0.5 rounded text-[8.5px] font-semibold shrink-0 transition-all ${
                                  activeProjModel === '__ALL__'
                                    ? 'bg-gray-900 dark:bg-white text-white dark:text-gray-900'
                                    : 'bg-gray-100 dark:bg-white/[0.06] text-gray-500 dark:text-zinc-400 hover:bg-gray-200/70 dark:hover:bg-white/[0.1]'
                                }`}
                              >
                                {`${tUi(lang, 'all')} (${projModelsList.length})`}
                              </button>
                              {projModelsList.map((m) => (
                                <button
                                  key={m}
                                  onClick={() => setProjPerfModel(m)}
                                  className={`px-1.5 py-0.5 rounded text-[8.5px] font-mono font-semibold shrink-0 transition-all ${
                                    activeProjModel === m
                                      ? 'bg-blue-600 text-white shadow-2xs'
                                      : 'bg-gray-100 dark:bg-white/[0.06] text-gray-600 dark:text-zinc-300 hover:bg-gray-200/70 dark:hover:bg-white/[0.1]'
                                  }`}
                                >
                                  {m}
                                </button>
                              ))}
                            </div>
                            <span className="text-[8px] font-mono text-gray-400 shrink-0">{perf.rangeLabel}</span>
                          </div>

                          <div className="flex-1 grid grid-cols-2 gap-2 items-stretch">
                            <div className="p-2 rounded-xl bg-gray-50/90 dark:bg-white/[0.03] border border-gray-100/80 dark:border-white/[0.04] flex flex-col justify-between gap-1">
                              <div className="flex items-baseline justify-between">
                                <span className="text-[9px] font-semibold text-gray-600 dark:text-zinc-300">
                                  {tUi(lang, 'stats_cache_hit_label')}
                                </span>
                                <span className="text-[11px] font-extrabold font-mono text-emerald-600 dark:text-emerald-400">
                                  {displayProjCache.toFixed(1)}%
                                </span>
                              </div>
                              <div className="h-[58px] w-full">
                                {renderSmoothCurve(perf.cacheSeries, '#10b981', 'proj-cache-grad', 100, 58, 90)}
                              </div>
                              <div className="flex items-center justify-between text-[8px] font-mono text-gray-400">
                                <span>{perf.count > 0 ? `${perf.count} ${tUi(lang, 'stats_unit_reqs')}` : matchedMeta ? `${matchedMeta.totalRequests} ${tUi(lang, 'stats_unit_reqs')}` : `0 ${tUi(lang, 'stats_unit_reqs')}`}</span>
                                <span>{tUi(lang, 'stats_peak')} {(perf.peakCache > 0 ? perf.peakCache : displayProjCache).toFixed(0)}%</span>
                              </div>
                            </div>

                            <div className="p-2 rounded-xl bg-gray-50/90 dark:bg-white/[0.03] border border-gray-100/80 dark:border-white/[0.04] flex flex-col justify-between gap-1">
                              <div className="flex items-baseline justify-between">
                                <span className="text-[9px] font-semibold text-gray-600 dark:text-zinc-300">
                                  {tUi(lang, 'stats_gen_speed')}
                                </span>
                                <span className="text-[11px] font-extrabold font-mono text-sky-600 dark:text-sky-400">
                                  {displayProjTps.toFixed(1)} <span className="text-[8px] font-normal">tok/s</span>
                                </span>
                              </div>
                              <div className="h-[58px] w-full">
                                {renderSmoothCurve(perf.tpsSeries, '#0ea5e9', 'proj-tps-grad', Math.max(perf.seriesPeakTps, 10) * 1.15, 58)}
                              </div>
                              <div className="flex items-center justify-between text-[8px] font-mono text-gray-400">
                                <span>{tUi(lang, 'stats_peak')} {(perf.peakTps > 0 ? perf.peakTps : displayProjTps).toFixed(1)} tok/s</span>
                                <span>{perf.rangeLabel}</span>
                              </div>
                            </div>
                          </div>
                        </div>
                      );
                    })() : projectActivityTab === 'milestones' ? (
                      <div className="flex-1 relative pl-3.5 flex flex-col justify-center gap-2 before:content-[''] before:absolute before:left-[5px] before:top-2 before:bottom-2 before:w-px before:bg-gray-200 dark:before:bg-white/10">
                        {currentProject.sessionTitles.slice(0, 5).map((title, idx) => (
                          <button
                            key={idx}
                            onClick={() => {
                              setWorkspaceFilter(currentProject.workspaceName);
                              setSearchQuery(title.slice(0, 12));
                              setActiveView('ledger');
                            }}
                            className="relative flex items-center justify-between gap-2 text-[11px] text-left hover:text-indigo-600 dark:hover:text-indigo-400 transition-colors group"
                          >
                            <span className="absolute -left-[13px] w-2 h-2 rounded-full bg-indigo-500 ring-2 ring-white dark:ring-[#121316]" />
                            <span className="font-medium text-gray-800 dark:text-zinc-200 group-hover:text-indigo-600 dark:group-hover:text-indigo-400 truncate">
                              {title}
                            </span>
                            <ChevronRight className="w-3 h-3 text-gray-300 opacity-0 group-hover:opacity-100 shrink-0" />
                          </button>
                        ))}
                      </div>
                    ) : (
                      <div className="flex-1 flex flex-col justify-center gap-1.5">
                        {projTraffic.slice(0, 4).map((r) => {
                          const tot = r.inputTokens + r.cachedTokens + r.outputTokens;
                          const cPct =
                            r.inputTokens + r.cachedTokens > 0
                              ? Math.round((r.cachedTokens / (r.inputTokens + r.cachedTokens)) * 100)
                              : 0;
                          return (
                            <button
                              key={r.id}
                              onClick={() => {
                                setSelectedRecord(r);
                                setActiveView('ledger');
                              }}
                              className="w-full text-left px-2.5 py-1.5 rounded-xl bg-gray-50/90 dark:bg-white/[0.03] hover:bg-blue-50/50 dark:hover:bg-blue-500/[0.06] border border-gray-100/80 dark:border-white/[0.04] flex items-center justify-between gap-2 text-[10px] transition-colors"
                            >
                              <div className="flex items-center gap-2 min-w-0">
                                <span className="font-mono font-semibold text-purple-600 dark:text-purple-400 truncate">
                                  {r.requestedModel}
                                </span>
                                <span className="text-[8.5px] text-gray-400 font-mono shrink-0">
                                  {r.time.split(' ')[1] || r.time}
                                </span>
                              </div>
                              <div className="flex items-center gap-2 font-mono shrink-0">
                                <span className="text-[9px] text-emerald-600 dark:text-emerald-400">{cPct}% {tUi(lang, 'stats_cache')}</span>
                                <span className="font-bold text-gray-800 dark:text-zinc-200">{fmtTok(tot)}</span>
                              </div>
                            </button>
                          );
                        })}
                        {projTraffic.length === 0 && (
                          <div className="py-6 text-center text-[10px] text-gray-400">
                            {tUi(lang, 'stats_no_recent_requests')}
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                </div>

                {/* 16:9 四模板战报分享弹窗 */}
                {shareModalOpen && (
                  <div className="fixed inset-0 z-50 bg-black/60 backdrop-blur-md flex items-center justify-center p-4" onClick={() => setShareModalOpen(false)}>
                    <div
                      className="bg-white/95 dark:bg-[#14151a]/95 border border-gray-200/80 dark:border-white/10 rounded-2xl p-5 shadow-2xl flex flex-col items-center gap-4 w-full max-w-[700px]"
                      onClick={(e) => e.stopPropagation()}
                    >
                      {/* 顶部工具栏：4 模板选择 + 16:9 标识 + 关闭 */}
                      <div className="w-full flex items-center justify-between gap-2">
                        <div className="flex items-center gap-2.5">
                          <span className="text-xs font-bold text-gray-900 dark:text-zinc-100">
                            {tUi(lang, 'stats_poster_title')}
                          </span>
                          <div className="inline-flex p-0.5 rounded-lg bg-gray-100 dark:bg-white/[0.06]">
                            {([
                              { id: 'polar', label: tUi(lang, 'stats_tpl_polar') },
                              { id: 'radial', label: tUi(lang, 'stats_tpl_radial') },
                              { id: 'hexcore', label: tUi(lang, 'stats_tpl_hexcore') },
                              { id: 'matrix', label: tUi(lang, 'stats_tpl_matrix') },
                            ] as const).map((tpl) => (
                              <button
                                key={tpl.id}
                                onClick={() => setShareTemplate(tpl.id)}
                                className={`px-2.5 py-0.5 rounded-md text-[10.5px] font-semibold transition-all ${
                                  shareTemplate === tpl.id
                                    ? 'bg-gray-900 dark:bg-white text-white dark:text-gray-900 shadow-xs'
                                    : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                                }`}
                              >
                                {tpl.label}
                              </button>
                            ))}
                          </div>
                        </div>

                        <div className="flex items-center gap-2">
                          <span className="px-2 py-0.5 rounded-md bg-indigo-500/10 text-indigo-600 dark:text-indigo-400 text-[10px] font-mono font-bold">
                            16:9
                          </span>
                          <button onClick={() => setShareModalOpen(false)} className="p-1 rounded-lg text-gray-400 hover:bg-gray-100 dark:hover:bg-white/5">
                            <X className="w-4 h-4" />
                          </button>
                        </div>
                      </div>

                      {/* 16:9 电影宽屏海报画布：4 套 100% 真实遥测且 360° 饱满构图模板 */}
                      {(() => {
                        const canvasClass = 'relative aspect-[16/9] w-full max-w-[656px] rounded-2xl bg-gradient-to-br from-[#1f2432] via-[#151822] to-[#0f1118] border border-indigo-300/15 shadow-2xl p-6 flex flex-col justify-between overflow-hidden text-white';

                        // 海报专用无滚动条精选模型榜单：展示 Top 5 主力模型（带微缩算力条与完整对齐数值）+ 剩余模型汇总胶囊
                        const posterTopCount = 5;
                        const posterTopModels = projModelRows.slice(0, posterTopCount);
                        const posterRestModels = projModelRows.slice(posterTopCount);
                        const posterRestPct = posterRestModels.reduce((s, r) => s + r.pct, 0);
                        const posterRestTok = posterRestModels.reduce((s, r) => s + r.tot, 0);
                        const topMaxPct = Math.max(1, posterTopModels[0]?.pct || 100);

                        const renderPosterModelList = (valMode: 'pct' | 'tok', showBar = true) => (
                          <div className="flex flex-col gap-1.5 text-[9.5px] font-mono">
                            {posterTopModels.map((row, idx) => {
                              const color = wavePalette[idx % wavePalette.length];
                              const barW = Math.max(10, Math.round((row.pct / topMaxPct) * 100));
                              const { base, compactVariant } = splitModelVariant(row.model);
                              return (
                                <div key={row.model} className="flex items-center justify-between gap-2" title={row.model}>
                                  <span className="flex items-center gap-1 min-w-0 flex-1 text-zinc-200">
                                    <span className="w-1.5 h-1.5 rounded-full shrink-0" style={{ backgroundColor: color }} />
                                    <span className="truncate">{base}</span>
                                    {compactVariant && (
                                      <span className="text-[8.5px] text-zinc-400 shrink-0">{compactVariant}</span>
                                    )}
                                  </span>
                                  {showBar && (
                                    <span className="w-9 h-1 rounded-full bg-white/[0.07] overflow-hidden shrink-0 hidden sm:inline-block">
                                      <span
                                        className="block h-full rounded-full"
                                        style={{ width: `${barW}%`, backgroundColor: color }}
                                      />
                                    </span>
                                  )}
                                  <span className="text-zinc-300 font-bold tabular-nums shrink-0 text-right min-w-[34px]">
                                    {valMode === 'tok' ? fmtTok(row.tot) : `${row.pct.toFixed(0)}%`}
                                  </span>
                                </div>
                              );
                            })}
                            {posterRestModels.length > 0 && (
                              <div className="mt-0.5 px-2 py-0.5 rounded-md bg-white/[0.05] border border-white/[0.07] flex items-center justify-between text-[8.5px] text-zinc-400">
                                <span>
                                  ✦ +{posterRestModels.length} {tUi(lang, 'stats_more_models')}
                                </span>
                                <span className="font-semibold text-zinc-300 tabular-nums">
                                  {valMode === 'tok' ? fmtTok(posterRestTok) : `${posterRestPct.toFixed(1)}%`}
                                </span>
                              </div>
                            )}
                          </div>
                        );

                        // 真实蜂巢聚变热核计算器：中心金黄聚变核心由真实缓存命中率点亮，外围日冕由 32 段真实会话脉冲 360° 调制！
                        const computeRealHexHeat = (distRatio: number, normAngleRad: number) => {
                          const sector32 = Math.floor((normAngleRad / (Math.PI * 2)) * PULSE_COUNT) % PULSE_COUNT;
                          const prev32 = (sector32 + PULSE_COUNT - 1) % PULSE_COUNT;
                          const pulseNorm = (pulseBars32[sector32].norm * 0.65 + pulseBars32[prev32].norm * 0.35);
                          const cacheFactor = 0.62 + 0.38 * Math.min(1, projCacheRate / 100);
                          const radialDecay = Math.pow(Math.max(0, 1 - distRatio * 0.94), 0.85);
                          // 中心聚变核饱满明亮，外围随真实会话脉冲呈现星冕起伏
                          return Math.min(1, radialDecay * cacheFactor * (0.68 + 0.54 * pulseNorm));
                        };

                        // 模板 1：极坐标 6 维实测遥测谐波玫瑰 (Hyper Charts Polar Wave)
                        if (shareTemplate === 'polar') {
                          return (
                            <div className={canvasClass}>
                              <div className="flex items-center justify-between gap-2">
                                <div className="flex items-center gap-2 min-w-0">
                                  <span className="w-2.5 h-2.5 rounded-full bg-emerald-400 shadow-[0_0_8px_#34d399] shrink-0" />
                                  <div className="min-w-0">
                                    <div className="text-base font-black tracking-tight truncate">{displayProjectTitle}</div>
                                    <div className="text-[10px] text-zinc-500 font-mono truncate">{currentProject.path}</div>
                                  </div>
                                </div>
                                <span className="px-2.5 py-0.5 rounded-full bg-white/[0.08] border border-white/10 text-[9.5px] font-mono text-emerald-300 shrink-0">
                                  {posterRangeBadge} · POLAR WAVE
                                </span>
                              </div>

                              <div className="flex-1 flex items-center justify-between gap-4 py-2 min-h-0">
                                <div className="w-[206px] shrink-0 flex flex-col justify-between h-full py-1 gap-2 min-h-0">
                                  <div>
                                    <div className="text-[10px] text-emerald-400 font-mono mb-0.5">● {tUi(lang, 'stats_total_tokens_upper')}</div>
                                    <div className="flex items-baseline gap-1.5">
                                      <span className="text-2xl font-black font-mono text-white leading-none">{fmtTok(currentProject.totalTokens)}</span>
                                      <span className="text-[10px] font-mono text-emerald-400 font-bold">↑{projCacheRate.toFixed(0)}%</span>
                                    </div>
                                    <div className="text-[9px] text-zinc-500 font-mono mt-1">{tUi(lang, 'stats_reused')} {fmtTok(projCache)}</div>
                                  </div>
                                  {renderPosterModelList('pct', false)}
                                  <div className="flex items-center gap-4 pt-0.5">
                                    <div>
                                      <div className="text-[8.5px] text-zinc-500">{tUi(lang, 'stats_net_input')}</div>
                                      <div className="text-xs font-bold font-mono text-white">{fmtTok(projIn)}</div>
                                    </div>
                                    <div>
                                      <div className="text-[8.5px] text-zinc-500">{tUi(lang, 'stats_answer_output')}</div>
                                      <div className="text-xs font-bold font-mono text-white">{fmtTok(projOut)}</div>
                                    </div>
                                  </div>
                                </div>

                                <div className="flex-1 flex items-center justify-center min-w-0">
                                  {renderPolarRadar('w-[224px] h-[224px]', 'polar-logo-modal')}
                                </div>

                                <div className="w-[144px] shrink-0 flex flex-col justify-between h-full py-1 text-right">
                                  <div>
                                    <div className="text-[10px] text-amber-400 font-mono mb-0.5">✦ {tUi(lang, 'stats_active_time_upper')}</div>
                                    <div className="text-2xl font-black font-mono text-white leading-none">{estHrsInt}h {estMinsInt}m</div>
                                    <div className="text-[9px] text-zinc-500 font-mono mt-1">{tUi(lang, 'stats_session_span')}</div>
                                  </div>
                                  <div>
                                    <div className="text-[10px] text-purple-400 font-mono mb-0.5">◆ {tUi(lang, 'stats_steps_label')}</div>
                                    <div className="text-2xl font-black font-mono text-white leading-none">{currentProject.totalSteps.toLocaleString()}</div>
                                    <div className="text-[9px] text-zinc-500 font-mono mt-1">{tUi(lang, 'stats_avg_steps_fmt', { count: avgStepsPerSession })}</div>
                                  </div>
                                  <div className="flex flex-col gap-1 text-[10px] font-mono pt-2 border-t border-white/10">
                                    <div className="flex items-center justify-between text-zinc-400">
                                      <span>{tUi(lang, 'stats_archived_sessions')}</span>
                                      <span className="text-white font-bold">{currentProject.conversationCount}</span>
                                    </div>
                                    <div className="flex items-center justify-between text-zinc-400">
                                      <span>{tUi(lang, 'stats_used_models')}</span>
                                      <span className="text-white font-bold">{projModelRows.length}</span>
                                    </div>
                                  </div>
                                </div>
                              </div>

                              <div className="pt-2 border-t border-white/10 flex items-center justify-between text-[9px] font-mono text-zinc-500">
                                <span>6-Axis Telemetry · {posterRangeBadge}</span>
                                <span className="text-zinc-300">Antigravity Manager</span>
                              </div>
                            </div>
                          );
                        }

                        // 模板 2：放射光谱 (32 根 360° 全环真实会话脉冲重采样光谱柱)
                        if (shareTemplate === 'radial') {
                          return (
                            <div className={canvasClass}>
                              <div className="flex items-center justify-between gap-2">
                                <div className="min-w-0">
                                  <div className="text-base font-black tracking-tight truncate">{displayProjectTitle}</div>
                                  <div className="text-[10px] text-zinc-500 font-mono truncate">{currentProject.path}</div>
                                </div>
                                <span className="px-2.5 py-0.5 rounded-full bg-amber-500/15 border border-amber-400/30 text-[9.5px] font-mono text-amber-300 shrink-0">
                                  {posterRangeBadge} · RADIAL BURST
                                </span>
                              </div>

                              <div className="flex-1 flex items-center justify-between gap-4 py-2 min-h-0">
                                <div className="w-[208px] shrink-0 flex flex-col justify-between h-full py-1 gap-2 min-h-0">
                                  <div>
                                    <div className="text-[10px] text-emerald-400 font-mono">● {tUi(lang, 'stats_cache_hit_upper')}</div>
                                    <div className="text-2xl font-black font-mono">{projCacheRate.toFixed(1)}%</div>
                                    <div className="text-[9px] text-zinc-500 font-mono">{tUi(lang, 'stats_reused')} {fmtTok(projCache)}</div>
                                  </div>
                                  {renderPosterModelList('pct', true)}
                                </div>

                                <div className="flex-1 flex items-center justify-center min-w-0">
                                  <svg viewBox="0 0 200 200" className="w-[228px] h-[228px] overflow-visible">
                                    <defs>
                                      <filter id="radial-bar-glow" x="-30%" y="-30%" width="160%" height="160%">
                                        <feGaussianBlur stdDeviation="1.8" result="blur" />
                                        <feMerge>
                                          <feMergeNode in="blur" />
                                          <feMergeNode in="SourceGraphic" />
                                        </feMerge>
                                      </filter>
                                    </defs>
                                    <circle
                                      cx="100"
                                      cy="100"
                                      r="92"
                                      fill="none"
                                      stroke="rgba(255,255,255,0.08)"
                                      strokeWidth="0.8"
                                      strokeDasharray="2 4"
                                    />
                                    <circle cx="100" cy="100" r="64" fill="none" stroke="rgba(255,255,255,0.05)" strokeWidth="0.7" />
                                    {pulseBars32.map((bar, i) => {
                                      const angle = ((i * 360) / PULSE_COUNT - 90) * (Math.PI / 180);
                                      const r1 = 42;
                                      const r2 = r1 + bar.norm * 46;
                                      return (
                                        <line
                                          key={i}
                                          x1={100 + r1 * Math.cos(angle)}
                                          y1={100 + r1 * Math.sin(angle)}
                                          x2={100 + r2 * Math.cos(angle)}
                                          y2={100 + r2 * Math.sin(angle)}
                                          stroke={bar.color}
                                          strokeWidth="4.2"
                                          strokeLinecap="round"
                                          filter="url(#radial-bar-glow)"
                                          opacity={0.72 + bar.norm * 0.28}
                                        >
                                          <title>{`${bar.label} · ${fmtTok(Math.round(bar.tok))}`}</title>
                                        </line>
                                      );
                                    })}
                                    <circle cx="100" cy="100" r="35" fill="#07080c" stroke="rgba(255,255,255,0.16)" strokeWidth="1.2" />
                                    <text x="100" y="97" textAnchor="middle" className="fill-white font-mono font-black text-[15px]">
                                      {fmtTok(currentProject.totalTokens)}
                                    </text>
                                    <text x="100" y="111" textAnchor="middle" className="fill-zinc-500 font-mono text-[8px]">
                                      TOKENS
                                    </text>
                                  </svg>
                                </div>

                                <div className="w-[142px] shrink-0 flex flex-col justify-between h-full py-2 text-right">
                                  <div>
                                    <div className="text-[10px] text-amber-400 font-mono">✦ {tUi(lang, 'stats_active_time_upper')}</div>
                                    <div className="text-2xl font-black font-mono">{estHrsInt}h {estMinsInt}m</div>
                                    <div className="text-[9px] text-zinc-500 font-mono">{tUi(lang, 'stats_archived_sessions_count', { count: currentProject.conversationCount })}</div>
                                  </div>
                                  <div>
                                    <div className="text-[10px] text-sky-400 font-mono">◆ {tUi(lang, 'stats_steps_label')}</div>
                                    <div className="text-2xl font-black font-mono">{currentProject.totalSteps.toLocaleString()}</div>
                                    <div className="text-[9px] text-zinc-500 font-mono">{tUi(lang, 'stats_avg_steps_fmt', { count: avgStepsPerSession })}</div>
                                  </div>
                                </div>
                              </div>

                              <div className="pt-2 border-t border-white/10 flex items-center justify-between text-[9px] font-mono text-zinc-500">
                                <span>360° Session Pulse · {posterRangeBadge}</span>
                                <span className="text-zinc-300">Antigravity Manager</span>
                              </div>
                            </div>
                          );
                        }

                        // 模板 3：蜂巢热核 (真实缓存聚变核心 + 32 扇区真实会话星冕辐射 + 无滚动条精选榜单)
                        if (shareTemplate === 'hexcore') {
                          const hexCells: Array<{ cx: number; cy: number; fill: string; opacity: number; label: string }> = [];
                          const hexR = 9.5;
                          const dx = hexR * 1.78;
                          const dy = hexR * 1.54;
                          for (let row = -4; row <= 4; row++) {
                            for (let col = -4; col <= 4; col++) {
                              const cx = 110 + col * dx + (Math.abs(row) % 2 === 1 ? dx / 2 : 0);
                              const cy = 100 + row * dy;
                              const dist = Math.hypot(cx - 110, cy - 100);
                              if (dist > 84) continue;
                              const angleRad = Math.atan2(cy - 100, cx - 110);
                              const normAngle = (angleRad + Math.PI * 2.5) % (Math.PI * 2);
                              const heat = computeRealHexHeat(dist / 84, normAngle);
                              let fill = '#181c30';
                              let opacity = 0.55;
                              if (heat >= 0.70) {
                                fill = '#facc15';
                                opacity = 0.98;
                              } else if (heat >= 0.52) {
                                fill = '#ef4444';
                                opacity = 0.93;
                              } else if (heat >= 0.34) {
                                fill = '#9f1239';
                                opacity = 0.86;
                              } else if (heat >= 0.18) {
                                fill = '#312e81';
                                opacity = 0.78;
                              }
                              hexCells.push({
                                cx,
                                cy,
                                fill,
                                opacity,
                                label: tUi(lang, 'stats_fusion_intensity', { pct: Math.round(heat * 100) }),
                              });
                            }
                          }
                          const hexPoints = (cx: number, cy: number, r: number) =>
                            Array.from({ length: 6 }, (_, i) => {
                              const a = ((60 * i - 30) * Math.PI) / 180;
                              return `${(cx + r * Math.cos(a)).toFixed(1)},${(cy + r * Math.sin(a)).toFixed(1)}`;
                            }).join(' ');

                          return (
                            <div className={canvasClass}>
                              <div className="flex items-center justify-between gap-2">
                                <div className="min-w-0">
                                  <div className="text-base font-black tracking-tight truncate">{displayProjectTitle}</div>
                                  <div className="text-[10px] text-zinc-500 font-mono truncate">{currentProject.path}</div>
                                </div>
                                <span className="px-2.5 py-0.5 rounded-full bg-rose-500/15 border border-rose-400/30 text-[9.5px] font-mono text-rose-300 shrink-0">
                                  {posterRangeBadge} · HEX CORE
                                </span>
                              </div>

                              <div className="flex-1 flex items-center justify-between gap-6 py-1 min-h-0">
                                <div className="flex-1 flex items-center justify-center">
                                  <svg viewBox="0 0 220 200" className="w-[240px] h-[218px]">
                                    {hexCells.map((cell, idx) => (
                                      <polygon
                                        key={idx}
                                        points={hexPoints(cell.cx, cell.cy, hexR - 1.1)}
                                        fill={cell.fill}
                                        opacity={cell.opacity}
                                        stroke="#07080c"
                                        strokeWidth="1.2"
                                      >
                                        <title>{cell.label}</title>
                                      </polygon>
                                    ))}
                                  </svg>
                                </div>

                                <div className="w-[292px] shrink-0 flex flex-col justify-between h-full py-1.5 min-h-0 gap-2">
                                  <div className="grid grid-cols-2 gap-3 pb-2.5 border-b border-white/10 shrink-0">
                                    <div className="min-w-0">
                                      <div className="text-[9.5px] text-emerald-400 font-mono mb-0.5">✦ {tUi(lang, 'stats_total_tokens_upper')}</div>
                                      <div className="text-2xl font-black font-mono text-white leading-none">{fmtTok(currentProject.totalTokens)}</div>
                                      <div className="text-[8.5px] text-emerald-400 font-mono mt-1 whitespace-nowrap">↑{projCacheRate.toFixed(1)}% {tUi(lang, 'stats_cache')}</div>
                                    </div>
                                    <div className="min-w-0">
                                      <div className="text-[9.5px] text-amber-400 font-mono mb-0.5">● {tUi(lang, 'stats_active_time')}</div>
                                      <div className="text-2xl font-black font-mono text-white leading-none">{estHrsInt}h {estMinsInt}m</div>
                                      <div className="text-[8.5px] text-zinc-400 font-mono mt-1 whitespace-nowrap truncate">{currentProject.conversationCount} {tUi(lang, 'stats_unit_sessions')} · {currentProject.totalSteps.toLocaleString()} {tUi(lang, 'stats_unit_steps')}</div>
                                    </div>
                                  </div>

                                  {renderPosterModelList('tok', true)}
                                </div>
                              </div>

                              <div className="pt-2 border-t border-white/10 flex items-center justify-between text-[9px] font-mono text-zinc-500">
                                <span>Thermal Fusion Telemetry · {posterRangeBadge}</span>
                                <span className="text-zinc-300">Antigravity Manager</span>
                              </div>
                            </div>
                          );
                        }

                        // 模板 4：暗夜矩阵 (自适应单日「模型×时序切片」/ 多日「星期×时段」饱满矩阵 + 无滚动条精选榜单)
                        return (
                          <div className={canvasClass}>
                            <div className="flex items-center justify-between gap-2">
                              <div className="min-w-0">
                                <div className="text-base font-black tracking-tight truncate">{displayProjectTitle}</div>
                                <div className="text-[10px] text-zinc-500 font-mono truncate">{currentProject.path}</div>
                              </div>
                              <span className="px-2.5 py-0.5 rounded-full bg-indigo-500/15 border border-indigo-400/30 text-[9.5px] font-mono text-indigo-300 shrink-0">
                                {posterRangeBadge} · {adaptiveMatrix.modeTag}
                              </span>
                            </div>

                            <div className="flex-1 flex items-center gap-5 py-2 min-h-0">
                              <div className="flex-1 flex flex-col gap-1">
                                <div className="flex items-center gap-1 text-[8px] font-mono text-zinc-500 text-center">
                                  <span className={useSessionSliceMatrix ? 'w-16 shrink-0' : 'w-8 shrink-0'} />
                                  <div className="flex-1 grid grid-cols-6 gap-1">
                                    {adaptiveMatrix.colLabels.map((s, ci) => (
                                      <span key={ci} className="truncate">{s}</span>
                                    ))}
                                  </div>
                                </div>
                                {adaptiveMatrix.rowLabels.map((rLabel, rIdx) => (
                                  <div key={rIdx} className="flex items-center gap-1">
                                    <span
                                      className={`${
                                        useSessionSliceMatrix ? 'w-16' : 'w-8'
                                      } shrink-0 text-[8.5px] font-mono text-zinc-400 text-right pr-1 truncate`}
                                    >
                                      {rLabel}
                                    </span>
                                    <div className="flex-1 grid grid-cols-6 gap-1">
                                      {adaptiveMatrix.heat[rIdx].map((val, cIdx) => (
                                        <div
                                          key={cIdx}
                                          title={`${rLabel} · ${adaptiveMatrix.colLabels[cIdx]} · ${fmtTok(adaptiveMatrix.rawVals[rIdx]?.[cIdx] || 0)}`}
                                          className={`h-6 rounded-md flex items-center justify-center text-[9.5px] font-mono font-bold ${
                                            val >= 80
                                              ? 'bg-amber-400 text-gray-950'
                                              : val >= 60
                                              ? 'bg-lime-400 text-gray-950'
                                              : val >= 40
                                              ? 'bg-emerald-400 text-gray-950'
                                              : val >= 15
                                              ? 'bg-indigo-500 text-white'
                                              : 'bg-white/[0.06] text-zinc-600'
                                          }`}
                                        >
                                          {val > 0 ? String(val).padStart(2, '0') : '·'}
                                        </div>
                                      ))}
                                    </div>
                                  </div>
                                ))}
                              </div>

                              <div className="w-[232px] shrink-0 flex flex-col justify-between h-full py-1 gap-2 min-h-0">
                                <div className="grid grid-cols-2 gap-2.5 py-2 border-y border-white/10 shrink-0">
                                  <div>
                                    <span className="text-[8.5px] text-emerald-400 font-mono block">✦ {tUi(lang, 'stats_total_power')}</span>
                                    <span className="text-lg font-black font-mono text-white">{fmtTok(currentProject.totalTokens)}</span>
                                  </div>
                                  <div>
                                    <span className="text-[8.5px] text-amber-400 font-mono block">● {tUi(lang, 'stats_cache_rate_short')}</span>
                                    <span className="text-lg font-black font-mono text-emerald-400">{projCacheRate.toFixed(1)}%</span>
                                  </div>
                                  <div>
                                    <span className="text-[8.5px] text-zinc-400 font-mono block">{tUi(lang, 'stats_active_time')}</span>
                                    <span className="text-xs font-bold font-mono text-white">{estHrsInt}h {estMinsInt}m</span>
                                  </div>
                                  <div>
                                    <span className="text-[8.5px] text-zinc-400 font-mono block">{tUi(lang, 'stats_sess_and_steps')}</span>
                                    <span className="text-xs font-bold font-mono text-indigo-300">
                                      {currentProject.conversationCount} · {currentProject.totalSteps.toLocaleString()}
                                    </span>
                                  </div>
                                </div>
                                {renderPosterModelList('pct', false)}
                              </div>
                            </div>

                            <div className="pt-2 border-t border-white/10 flex items-center justify-between text-[9px] font-mono text-zinc-500">
                              <span>Antigravity Telemetry · {posterRangeBadge}</span>
                              <span className="text-zinc-300">Antigravity Manager</span>
                            </div>
                          </div>
                        );
                      })()}

                      {(() => {
                        const renderPosterPngBlob = async (): Promise<Blob | null> => {
                          const logoImg = await new Promise<HTMLImageElement | null>((resImg) => {
                            const img = new window.Image();
                            img.onload = () => resImg(img);
                            img.onerror = () => resImg(null);
                            img.src = '/icon.png';
                          });

                          return new Promise((resolve) => {
                            const W = 1280;
                            const H = 720;
                            const canvas = document.createElement('canvas');
                            canvas.width = W;
                            canvas.height = H;
                            const ctx = canvas.getContext('2d');
                            if (!ctx) return resolve(null);

                            const SANS = '-apple-system, BlinkMacSystemFont, "SF Pro Display", "Segoe UI", sans-serif';
                            const MONO = 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace';

                            const drawRoundRect = (x: number, y: number, w: number, h: number, r: number) => {
                              const rr = Math.min(r, w / 2, h / 2);
                              ctx.beginPath();
                              if (typeof ctx.roundRect === 'function') {
                                ctx.roundRect(x, y, w, h, rr);
                              } else {
                                ctx.moveTo(x + rr, y);
                                ctx.arcTo(x + w, y, x + w, y + h, rr);
                                ctx.arcTo(x + w, y + h, x, y + h, rr);
                                ctx.arcTo(x, y + h, x, y, rr);
                                ctx.arcTo(x, y, x + w, y, rr);
                                ctx.closePath();
                              }
                            };

                            const fitText = (text: string, maxW: number) => {
                              if (ctx.measureText(text).width <= maxW) return text;
                              let s = text;
                              while (s.length > 1 && ctx.measureText(`${s}…`).width > maxW) {
                                s = s.slice(0, -1);
                              }
                              return `${s}…`;
                            };

                            const fitModelName = (name: string, maxW: number) => {
                              if (ctx.measureText(name).width <= maxW) return name;
                              const { base, compactVariant } = splitModelVariant(name);
                              if (!compactVariant) return fitText(name, maxW);
                              const suffix = ` ${compactVariant}`;
                              const suffixW = ctx.measureText(suffix).width;
                              if (suffixW >= maxW - 24) return fitText(name, maxW);
                              return `${fitText(base, maxW - suffixW)}${suffix}`;
                            };

                            // 1:1 深空渐变背景 + 柔和描边
                            const bgGrad = ctx.createLinearGradient(0, 0, W, H);
                            bgGrad.addColorStop(0, '#1f2432');
                            bgGrad.addColorStop(0.5, '#151822');
                            bgGrad.addColorStop(1, '#0f1118');
                            ctx.fillStyle = bgGrad;
                            ctx.fillRect(0, 0, W, H);
                            ctx.strokeStyle = 'rgba(165, 180, 252, 0.16)';
                            ctx.lineWidth = 2;
                            ctx.strokeRect(1, 1, W - 2, H - 2);

                            // 顶部标题与模板专属徽标 (1:1 对齐 4 套模板预览)
                            const titleStartX = shareTemplate === 'polar' ? 78 : 56;
                            if (shareTemplate === 'polar') {
                              ctx.save();
                              ctx.fillStyle = '#34d399';
                              ctx.shadowColor = '#34d399';
                              ctx.shadowBlur = 12;
                              ctx.beginPath();
                              ctx.arc(60, 64, 6.5, 0, Math.PI * 2);
                              ctx.fill();
                              ctx.restore();
                            }

                            const badgeMeta =
                              shareTemplate === 'radial'
                                ? {
                                    text: `${posterRangeBadge} · RADIAL BURST`,
                                    bg: 'rgba(245, 158, 11, 0.16)',
                                    border: 'rgba(251, 191, 36, 0.34)',
                                    color: '#fcd34d',
                                    footer: `360° Session Pulse · ${posterRangeBadge}`,
                                  }
                                : shareTemplate === 'hexcore'
                                ? {
                                    text: `${posterRangeBadge} · HEX CORE`,
                                    bg: 'rgba(244, 63, 94, 0.16)',
                                    border: 'rgba(251, 113, 133, 0.34)',
                                    color: '#fda4af',
                                    footer: `Thermal Fusion Telemetry · ${posterRangeBadge}`,
                                  }
                                : shareTemplate === 'matrix'
                                ? {
                                    text: `${posterRangeBadge} · ${adaptiveMatrix.modeTag}`,
                                    bg: 'rgba(99, 102, 241, 0.16)',
                                    border: 'rgba(129, 140, 248, 0.34)',
                                    color: '#a5b4fc',
                                    footer: `Antigravity Telemetry · ${posterRangeBadge}`,
                                  }
                                : {
                                    text: `${posterRangeBadge} · POLAR WAVE`,
                                    bg: 'rgba(255, 255, 255, 0.08)',
                                    border: 'rgba(255, 255, 255, 0.14)',
                                    color: '#6ee7b7',
                                    footer: `6-Axis Telemetry · ${posterRangeBadge}`,
                                  };

                            ctx.font = `600 16px ${MONO}`;
                            const tagW = ctx.measureText(badgeMeta.text).width + 34;
                            const tagX = W - 56 - tagW;
                            drawRoundRect(tagX, 46, tagW, 34, 17);
                            ctx.fillStyle = badgeMeta.bg;
                            ctx.fill();
                            ctx.strokeStyle = badgeMeta.border;
                            ctx.lineWidth = 1.2;
                            ctx.stroke();
                            ctx.fillStyle = badgeMeta.color;
                            ctx.textAlign = 'left';
                            ctx.fillText(badgeMeta.text, tagX + 17, 68);

                            const titleMaxW = Math.max(240, tagX - titleStartX - 24);
                            ctx.fillStyle = '#ffffff';
                            ctx.font = `800 30px ${SANS}`;
                            ctx.fillText(fitText(displayProjectTitle, titleMaxW), titleStartX, 70);
                            ctx.fillStyle = '#71717a';
                            ctx.font = `500 17px ${MONO}`;
                            ctx.fillText(fitText(currentProject.path, titleMaxW), titleStartX, 96);

                            // 共享 Top 5 模型榜单绘制器（与预览 1:1 保持颜色点、可选进度条、数值、其余汇总胶囊）
                            const showRows = projModelRows.slice(0, 5);
                            const restRows = projModelRows.slice(5);
                            const topMaxPct = Math.max(1, showRows[0]?.pct || 100);
                            const drawModelList = (
                              x: number,
                              startY: number,
                              width: number,
                              valMode: 'pct' | 'tok',
                              showBar: boolean
                            ) => {
                              const rowGap = 36;
                              const valColW = valMode === 'tok' ? 88 : 58;
                              const barW = showBar ? 74 : 0;
                              const barGap = showBar ? 16 : 0;
                              const nameMaxW = Math.max(110, width - 24 - valColW - barW - barGap - 12);

                              showRows.forEach((row, idx) => {
                                const y = startY + idx * rowGap;
                                const color = wavePalette[idx % wavePalette.length];
                                ctx.fillStyle = color;
                                ctx.beginPath();
                                ctx.arc(x + 6, y - 5, 4.5, 0, Math.PI * 2);
                                ctx.fill();

                                ctx.fillStyle = '#e4e4e7';
                                ctx.font = `500 16px ${MONO}`;
                                ctx.textAlign = 'left';
                                ctx.fillText(fitModelName(row.model, nameMaxW), x + 20, y);

                                if (showBar) {
                                  const bx = x + width - valColW - barW - 14;
                                  const by = y - 8;
                                  drawRoundRect(bx, by, barW, 6, 3);
                                  ctx.fillStyle = 'rgba(255, 255, 255, 0.08)';
                                  ctx.fill();
                                  const fillW = Math.max(8, Math.round((row.pct / topMaxPct) * barW));
                                  drawRoundRect(bx, by, fillW, 6, 3);
                                  ctx.fillStyle = color;
                                  ctx.fill();
                                }

                                ctx.fillStyle = '#d4d4d8';
                                ctx.font = `700 16px ${MONO}`;
                                ctx.textAlign = 'right';
                                ctx.fillText(
                                  valMode === 'tok' ? fmtTok(row.tot) : `${row.pct.toFixed(0)}%`,
                                  x + width,
                                  y
                                );
                                ctx.textAlign = 'left';
                              });

                              if (restRows.length > 0) {
                                const y = startY + showRows.length * rowGap + 4;
                                const restPct = restRows.reduce((s, r) => s + r.pct, 0);
                                const restTok = restRows.reduce((s, r) => s + r.tot, 0);
                                drawRoundRect(x, y - 22, width, 30, 8);
                                ctx.fillStyle = 'rgba(255, 255, 255, 0.055)';
                                ctx.fill();
                                ctx.strokeStyle = 'rgba(255, 255, 255, 0.08)';
                                ctx.lineWidth = 1;
                                ctx.stroke();

                                ctx.fillStyle = '#a1a1aa';
                                ctx.font = `500 14.5px ${MONO}`;
                                ctx.textAlign = 'left';
                                ctx.fillText(`✦ +${restRows.length} ${tUi(lang, 'stats_more_models')}`, x + 12, y - 2);
                                ctx.fillStyle = '#e4e4e7';
                                ctx.font = `600 14.5px ${MONO}`;
                                ctx.textAlign = 'right';
                                ctx.fillText(
                                  valMode === 'tok' ? fmtTok(restTok) : `${restPct.toFixed(1)}%`,
                                  x + width - 12,
                                  y - 2
                                );
                                ctx.textAlign = 'left';
                              }
                            };

                            if (shareTemplate === 'matrix') {
                              // ========================================================
                              // 模板 4：暗夜矩阵 (1:1 双栏布局 + 完整时间段列头 + 右侧 2x2 KPI & 榜单)
                              // ========================================================
                              const rowCnt = Math.max(1, adaptiveMatrix.heat.length);
                              const colCnt = Math.max(1, adaptiveMatrix.colLabels.length || 6);
                              const labelW = useSessionSliceMatrix ? 132 : 76;
                              const gap = 10;
                              const gridX = 56 + labelW + 12;
                              const gridAvailW = 772 - gridX;
                              const cellW = Math.floor((gridAvailW - (colCnt - 1) * gap) / colCnt);
                              const cellH = rowCnt <= 6 ? 50 : 42;
                              const headerH = 30;
                              const totalGridH = headerH + rowCnt * cellH + (rowCnt - 1) * gap;
                              const gridTopY = Math.round(376 - totalGridH / 2);

                              // 顶部时间段 / 时序切片列头 (00:00, 01:15, ... 或 00h, 04h, ...)
                              ctx.fillStyle = '#a1a1aa';
                              ctx.font = `600 15px ${MONO}`;
                              ctx.textAlign = 'center';
                              adaptiveMatrix.colLabels.forEach((colLabel, cIdx) => {
                                const cxCell = gridX + cIdx * (cellW + gap) + cellW / 2;
                                ctx.fillText(fitText(colLabel, cellW + 4), cxCell, gridTopY + 14);
                              });

                              // 矩阵每行标签与热力格
                              adaptiveMatrix.heat.forEach((row, rIdx) => {
                                const ry = gridTopY + headerH + rIdx * (cellH + gap);
                                ctx.textAlign = 'right';
                                ctx.fillStyle = '#a1a1aa';
                                ctx.font = `500 15px ${MONO}`;
                                ctx.fillText(
                                  fitText(adaptiveMatrix.rowLabels[rIdx] || '', labelW),
                                  gridX - 12,
                                  ry + cellH / 2 + 5
                                );

                                ctx.textAlign = 'center';
                                row.forEach((val, cIdx) => {
                                  const x = gridX + cIdx * (cellW + gap);
                                  ctx.fillStyle =
                                    val >= 80
                                      ? '#fbbf24'
                                      : val >= 60
                                      ? '#a3e635'
                                      : val >= 40
                                      ? '#34d399'
                                      : val >= 15
                                      ? '#6366f1'
                                      : 'rgba(255, 255, 255, 0.06)';
                                  drawRoundRect(x, ry, cellW, cellH, 10);
                                  ctx.fill();

                                  ctx.fillStyle = val >= 40 ? '#09090b' : val >= 15 ? '#ffffff' : '#52525b';
                                  ctx.font = `700 17px ${MONO}`;
                                  ctx.fillText(
                                    val > 0 ? String(val).padStart(2, '0') : '·',
                                    x + cellW / 2,
                                    ry + cellH / 2 + 6
                                  );
                                });
                              });
                              ctx.textAlign = 'left';

                              // 右侧 2×2 核心 KPI（上下细分割线）+ 模型占比榜单
                              const rx = 812;
                              const rw = W - 56 - rx;
                              const col2X = rx + Math.round(rw * 0.52);

                              ctx.strokeStyle = 'rgba(255, 255, 255, 0.1)';
                              ctx.lineWidth = 1.2;
                              ctx.beginPath();
                              ctx.moveTo(rx, 148);
                              ctx.lineTo(W - 56, 148);
                              ctx.moveTo(rx, 336);
                              ctx.lineTo(W - 56, 336);
                              ctx.stroke();

                              ctx.fillStyle = '#34d399';
                              ctx.font = `600 15px ${MONO}`;
                              ctx.fillText(`✦ ${tUi(lang, 'stats_total_power')}`, rx, 178);
                              ctx.fillStyle = '#ffffff';
                              ctx.font = `900 34px ${MONO}`;
                              ctx.fillText(fmtTok(currentProject.totalTokens), rx, 218);

                              ctx.fillStyle = '#fbbf24';
                              ctx.font = `600 15px ${MONO}`;
                              ctx.fillText(`● ${tUi(lang, 'stats_cache_rate_short')}`, col2X, 178);
                              ctx.fillStyle = '#34d399';
                              ctx.font = `900 34px ${MONO}`;
                              ctx.fillText(`${projCacheRate.toFixed(1)}%`, col2X, 218);

                              ctx.fillStyle = '#a1a1aa';
                              ctx.font = `500 15px ${MONO}`;
                              ctx.fillText(tUi(lang, 'stats_active_time'), rx, 268);
                              ctx.fillStyle = '#ffffff';
                              ctx.font = `700 23px ${MONO}`;
                              ctx.fillText(`${estHrsInt}h ${estMinsInt}m`, rx, 300);

                              ctx.fillStyle = '#a1a1aa';
                              ctx.font = `500 15px ${MONO}`;
                              ctx.fillText(tUi(lang, 'stats_sess_and_steps'), col2X, 268);
                              ctx.fillStyle = '#a5b4fc';
                              ctx.font = `700 23px ${MONO}`;
                              ctx.fillText(
                                `${currentProject.conversationCount} · ${currentProject.totalSteps.toLocaleString()}`,
                                col2X,
                                300
                              );

                              drawModelList(rx, 376, rw, 'pct', false);
                            } else if (shareTemplate === 'hexcore') {
                              // ========================================================
                              // 模板 3：蜂巢热核 (1:1 双栏布局 + 大蜂巢描边 + 右侧双 KPI & Token 条)
                              // ========================================================
                              const hcx = 340;
                              const hcy = 376;
                              const hexR = 25;
                              const dx = hexR * 1.78;
                              const dy = hexR * 1.54;
                              for (let row = -4; row <= 4; row++) {
                                for (let col = -4; col <= 4; col++) {
                                  const hx = hcx + col * dx + (Math.abs(row) % 2 === 1 ? dx / 2 : 0);
                                  const hy = hcy + row * dy;
                                  const dist = Math.hypot(hx - hcx, hy - hcy);
                                  if (dist > 218) continue;
                                  const ang = (Math.atan2(hy - hcy, hx - hcx) + Math.PI * 2.5) % (Math.PI * 2);
                                  const distRatio = dist / 218;
                                  const sec32 = Math.floor((ang / (Math.PI * 2)) * PULSE_COUNT) % PULSE_COUNT;
                                  const prev32 = (sec32 + PULSE_COUNT - 1) % PULSE_COUNT;
                                  const pNorm = pulseBars32[sec32].norm * 0.65 + pulseBars32[prev32].norm * 0.35;
                                  const cFac = 0.62 + 0.38 * Math.min(1, projCacheRate / 100);
                                  const rDec = Math.pow(Math.max(0, 1 - distRatio * 0.94), 0.85);
                                  const heat = Math.min(1, rDec * cFac * (0.68 + 0.54 * pNorm));

                                  let fill = '#181c30';
                                  let alpha = 0.55;
                                  if (heat >= 0.7) {
                                    fill = '#facc15';
                                    alpha = 0.98;
                                  } else if (heat >= 0.52) {
                                    fill = '#ef4444';
                                    alpha = 0.93;
                                  } else if (heat >= 0.34) {
                                    fill = '#9f1239';
                                    alpha = 0.86;
                                  } else if (heat >= 0.18) {
                                    fill = '#312e81';
                                    alpha = 0.78;
                                  }

                                  ctx.save();
                                  ctx.globalAlpha = alpha;
                                  ctx.fillStyle = fill;
                                  ctx.beginPath();
                                  for (let k = 0; k < 6; k++) {
                                    const a = ((60 * k - 30) * Math.PI) / 180;
                                    const px = hx + (hexR - 2.2) * Math.cos(a);
                                    const py = hy + (hexR - 2.2) * Math.sin(a);
                                    if (k === 0) ctx.moveTo(px, py);
                                    else ctx.lineTo(px, py);
                                  }
                                  ctx.closePath();
                                  ctx.fill();
                                  ctx.globalAlpha = 1;
                                  ctx.strokeStyle = '#07080c';
                                  ctx.lineWidth = 3;
                                  ctx.stroke();
                                  ctx.restore();
                                }
                              }

                              const rx = 656;
                              const rw = W - 56 - rx;
                              const col2X = rx + Math.round(rw * 0.5);

                              ctx.fillStyle = '#34d399';
                              ctx.font = `600 16px ${MONO}`;
                              ctx.fillText(`✦ ${tUi(lang, 'stats_total_tokens_upper')}`, rx, 174);
                              ctx.fillStyle = '#ffffff';
                              ctx.font = `900 44px ${MONO}`;
                              ctx.fillText(fmtTok(currentProject.totalTokens), rx, 226);
                              ctx.fillStyle = '#34d399';
                              ctx.font = `600 16px ${MONO}`;
                              ctx.fillText(`↑${projCacheRate.toFixed(1)}% ${tUi(lang, 'stats_cache')}`, rx, 256);

                              ctx.fillStyle = '#fbbf24';
                              ctx.font = `600 16px ${MONO}`;
                              ctx.fillText(`● ${tUi(lang, 'stats_active_time')}`, col2X, 174);
                              ctx.fillStyle = '#ffffff';
                              ctx.font = `900 44px ${MONO}`;
                              ctx.fillText(`${estHrsInt}h ${estMinsInt}m`, col2X, 226);
                              ctx.fillStyle = '#a1a1aa';
                              ctx.font = `500 15.5px ${MONO}`;
                              const sessStepsText = `${currentProject.conversationCount} ${tUi(lang, 'stats_unit_sessions')} · ${currentProject.totalSteps.toLocaleString()} ${tUi(lang, 'stats_unit_steps')}`;
                              ctx.fillText(fitText(sessStepsText, W - 56 - col2X), col2X, 256);

                              ctx.strokeStyle = 'rgba(255, 255, 255, 0.1)';
                              ctx.lineWidth = 1.2;
                              ctx.beginPath();
                              ctx.moveTo(rx, 290);
                              ctx.lineTo(W - 56, 290);
                              ctx.stroke();

                              drawModelList(rx, 338, rw, 'tok', true);
                            } else if (shareTemplate === 'radial') {
                              // ========================================================
                              // 模板 2：放射光谱 (1:1 左 Cache Hit + 进度条榜单 / 中 32 束光谱 / 右耗时与步数)
                              // ========================================================
                              ctx.fillStyle = '#34d399';
                              ctx.font = `600 17px ${MONO}`;
                              ctx.fillText(`● ${tUi(lang, 'stats_cache_hit_upper')}`, 56, 170);
                              ctx.fillStyle = '#ffffff';
                              ctx.font = `900 46px ${MONO}`;
                              ctx.fillText(`${projCacheRate.toFixed(1)}%`, 56, 224);
                              ctx.fillStyle = '#71717a';
                              ctx.font = `500 16px ${MONO}`;
                              ctx.fillText(`${tUi(lang, 'stats_reused')} ${fmtTok(projCache)}`, 56, 252);

                              drawModelList(56, 316, 380, 'pct', true);

                              const rcx = 664;
                              const rcy = 376;
                              ctx.strokeStyle = 'rgba(255, 255, 255, 0.09)';
                              ctx.lineWidth = 1.5;
                              ctx.setLineDash([4, 8]);
                              ctx.beginPath();
                              ctx.arc(rcx, rcy, 194, 0, Math.PI * 2);
                              ctx.stroke();
                              ctx.setLineDash([]);

                              ctx.strokeStyle = 'rgba(255, 255, 255, 0.06)';
                              ctx.lineWidth = 1.2;
                              ctx.beginPath();
                              ctx.arc(rcx, rcy, 135, 0, Math.PI * 2);
                              ctx.stroke();

                              pulseBars32.forEach((bar, i) => {
                                const ang = (((i * 360) / PULSE_COUNT - 90) * Math.PI) / 180;
                                const r1 = 88;
                                const r2 = r1 + bar.norm * 98;
                                ctx.strokeStyle = bar.color;
                                ctx.shadowColor = bar.color;
                                ctx.shadowBlur = 10;
                                ctx.lineWidth = 8.5;
                                ctx.lineCap = 'round';
                                ctx.globalAlpha = 0.72 + bar.norm * 0.28;
                                ctx.beginPath();
                                ctx.moveTo(rcx + r1 * Math.cos(ang), rcy + r1 * Math.sin(ang));
                                ctx.lineTo(rcx + r2 * Math.cos(ang), rcy + r2 * Math.sin(ang));
                                ctx.stroke();
                              });
                              ctx.shadowBlur = 0;
                              ctx.globalAlpha = 1;

                              ctx.fillStyle = '#07080c';
                              ctx.beginPath();
                              ctx.arc(rcx, rcy, 74, 0, Math.PI * 2);
                              ctx.fill();
                              ctx.strokeStyle = 'rgba(255, 255, 255, 0.16)';
                              ctx.lineWidth = 2;
                              ctx.stroke();

                              ctx.fillStyle = '#ffffff';
                              ctx.textAlign = 'center';
                              ctx.font = `900 30px ${MONO}`;
                              ctx.fillText(fmtTok(currentProject.totalTokens), rcx, rcy + 4);
                              ctx.fillStyle = '#71717a';
                              ctx.font = `600 15px ${MONO}`;
                              ctx.fillText('TOKENS', rcx, rcy + 28);

                              ctx.textAlign = 'right';
                              ctx.fillStyle = '#fbbf24';
                              ctx.font = `600 17px ${MONO}`;
                              ctx.fillText(`✦ ${tUi(lang, 'stats_active_time_upper')}`, W - 56, 212);
                              ctx.fillStyle = '#ffffff';
                              ctx.font = `900 44px ${MONO}`;
                              ctx.fillText(`${estHrsInt}h ${estMinsInt}m`, W - 56, 264);
                              ctx.fillStyle = '#71717a';
                              ctx.font = `500 16px ${MONO}`;
                              ctx.fillText(
                                tUi(lang, 'stats_archived_sessions_count', { count: currentProject.conversationCount }),
                                W - 56,
                                294
                              );

                              ctx.fillStyle = '#38bdf8';
                              ctx.font = `600 17px ${MONO}`;
                              ctx.fillText(`◆ ${tUi(lang, 'stats_steps_label')}`, W - 56, 434);
                              ctx.fillStyle = '#ffffff';
                              ctx.font = `900 44px ${MONO}`;
                              ctx.fillText(currentProject.totalSteps.toLocaleString(), W - 56, 486);
                              ctx.fillStyle = '#71717a';
                              ctx.font = `500 16px ${MONO}`;
                              ctx.fillText(tUi(lang, 'stats_avg_steps_fmt', { count: avgStepsPerSession }), W - 56, 516);
                              ctx.textAlign = 'left';
                            } else {
                              // ========================================================
                              // 模板 1：极坐标波纹 (1:1 补齐 6 维径向轴线 + TOK/HIT/CTX/TPS/OUT/REQ 标签)
                              // ========================================================
                              ctx.fillStyle = '#34d399';
                              ctx.font = `600 17px ${MONO}`;
                              ctx.fillText(`● ${tUi(lang, 'stats_total_tokens_upper')}`, 56, 162);
                              const totalTokStr = fmtTok(currentProject.totalTokens);
                              ctx.fillStyle = '#ffffff';
                              ctx.font = `900 44px ${MONO}`;
                              ctx.fillText(totalTokStr, 56, 212);
                              const totalTokW = ctx.measureText(totalTokStr).width;
                              ctx.fillStyle = '#34d399';
                              ctx.font = `700 18px ${MONO}`;
                              ctx.fillText(`↑${projCacheRate.toFixed(0)}%`, 56 + totalTokW + 12, 210);
                              ctx.fillStyle = '#71717a';
                              ctx.font = `500 16px ${MONO}`;
                              ctx.fillText(`${tUi(lang, 'stats_reused')} ${fmtTok(projCache)}`, 56, 240);

                              drawModelList(56, 290, 372, 'pct', false);

                              // 左下 Input / Output
                              ctx.fillStyle = '#71717a';
                              ctx.font = `500 15px ${SANS}`;
                              ctx.fillText(tUi(lang, 'stats_net_input'), 56, 566);
                              ctx.fillText(tUi(lang, 'stats_answer_output'), 196, 566);
                              ctx.fillStyle = '#ffffff';
                              ctx.font = `700 23px ${MONO}`;
                              ctx.fillText(fmtTok(projIn), 56, 596);
                              ctx.fillText(fmtTok(projOut), 196, 596);

                              // 中央 6 维极坐标雷达 + 径向轴线 + 6 轴标签
                              const pcx = 654;
                              const pcy = 374;
                              [166, 114, 62].forEach((r, idx) => {
                                ctx.strokeStyle =
                                  idx === 0
                                    ? 'rgba(255, 255, 255, 0.14)'
                                    : idx === 1
                                    ? 'rgba(255, 255, 255, 0.08)'
                                    : 'rgba(255, 255, 255, 0.12)';
                                ctx.lineWidth = 1.5;
                                ctx.beginPath();
                                ctx.arc(pcx, pcy, r, 0, Math.PI * 2);
                                ctx.stroke();
                              });

                              [0, 60, 120, 180, 240, 300].forEach((deg, idx) => {
                                const rad = ((deg - 90) * Math.PI) / 180;
                                ctx.strokeStyle = 'rgba(255, 255, 255, 0.09)';
                                ctx.lineWidth = 1.2;
                                ctx.beginPath();
                                ctx.moveTo(pcx + 62 * Math.cos(rad), pcy + 62 * Math.sin(rad));
                                ctx.lineTo(pcx + 166 * Math.cos(rad), pcy + 166 * Math.sin(rad));
                                ctx.stroke();

                                ctx.fillStyle = '#71717a';
                                ctx.font = `600 15px ${MONO}`;
                                ctx.textAlign = 'center';
                                ctx.fillText(
                                  polarAxisLabels[idx],
                                  pcx + 190 * Math.cos(rad),
                                  pcy + 190 * Math.sin(rad) + 5
                                );
                              });
                              ctx.textAlign = 'left';

                              [...projModelRows.slice(0, 6)].reverse().forEach((row, revIdx, arr) => {
                                const mIdx = arr.length - 1 - revIdx;
                                const pts = buildPolarWavePoints(row, mIdx, pcx, pcy, 56, 164);
                                const N = pts.length;
                                ctx.strokeStyle = wavePalette[mIdx % wavePalette.length];
                                ctx.lineWidth = mIdx < 3 ? 4.8 : 3.4;
                                ctx.globalAlpha = mIdx === 0 ? 0.96 : mIdx < 3 ? 0.85 : 0.68;
                                ctx.shadowColor = wavePalette[mIdx % wavePalette.length];
                                ctx.shadowBlur = 12;
                                ctx.beginPath();
                                const prev = pts[N - 1];
                                ctx.moveTo((prev.x + pts[0].x) / 2, (prev.y + pts[0].y) / 2);
                                for (let i = 0; i < N; i++) {
                                  const p0 = pts[i];
                                  const p1 = pts[(i + 1) % N];
                                  ctx.quadraticCurveTo(p0.x, p0.y, (p0.x + p1.x) / 2, (p0.y + p1.y) / 2);
                                }
                                ctx.closePath();
                                ctx.stroke();
                                ctx.shadowBlur = 0;
                                ctx.globalAlpha = 1;
                              });

                              ctx.strokeStyle = 'rgba(251, 191, 36, 0.24)';
                              ctx.lineWidth = 2;
                              ctx.beginPath();
                              ctx.arc(pcx, pcy, 48, 0, Math.PI * 2);
                              ctx.stroke();

                              ctx.fillStyle = '#050507';
                              ctx.beginPath();
                              ctx.arc(pcx, pcy, 43, 0, Math.PI * 2);
                              ctx.fill();

                              if (logoImg) {
                                ctx.save();
                                ctx.beginPath();
                                ctx.arc(pcx, pcy, 42, 0, Math.PI * 2);
                                ctx.clip();
                                ctx.drawImage(logoImg, pcx - 44, pcy - 44, 88, 88);
                                ctx.restore();
                              }

                              ctx.strokeStyle = 'rgba(255, 255, 255, 0.22)';
                              ctx.lineWidth = 2;
                              ctx.beginPath();
                              ctx.arc(pcx, pcy, 42.5, 0, Math.PI * 2);
                              ctx.stroke();

                              // 右侧数据列
                              ctx.textAlign = 'right';
                              ctx.fillStyle = '#fbbf24';
                              ctx.font = `600 17px ${MONO}`;
                              ctx.fillText(`✦ ${tUi(lang, 'stats_active_time_upper')}`, W - 56, 166);
                              ctx.fillStyle = '#ffffff';
                              ctx.font = `900 44px ${MONO}`;
                              ctx.fillText(`${estHrsInt}h ${estMinsInt}m`, W - 56, 216);
                              ctx.fillStyle = '#71717a';
                              ctx.font = `500 16px ${MONO}`;
                              ctx.fillText(tUi(lang, 'stats_session_span'), W - 56, 244);

                              ctx.fillStyle = '#c084fc';
                              ctx.font = `600 17px ${MONO}`;
                              ctx.fillText(`◆ ${tUi(lang, 'stats_steps_label')}`, W - 56, 344);
                              ctx.fillStyle = '#ffffff';
                              ctx.font = `900 44px ${MONO}`;
                              ctx.fillText(currentProject.totalSteps.toLocaleString(), W - 56, 394);
                              ctx.fillStyle = '#71717a';
                              ctx.font = `500 16px ${MONO}`;
                              ctx.fillText(tUi(lang, 'stats_avg_steps_fmt', { count: avgStepsPerSession }), W - 56, 422);

                              ctx.strokeStyle = 'rgba(255, 255, 255, 0.1)';
                              ctx.lineWidth = 1.2;
                              ctx.beginPath();
                              ctx.moveTo(944, 510);
                              ctx.lineTo(W - 56, 510);
                              ctx.stroke();

                              ctx.textAlign = 'left';
                              ctx.fillStyle = '#a1a1aa';
                              ctx.font = `500 17px ${MONO}`;
                              ctx.fillText(tUi(lang, 'stats_archived_sessions'), 944, 548);
                              ctx.fillText(tUi(lang, 'stats_used_models'), 944, 584);
                              ctx.textAlign = 'right';
                              ctx.fillStyle = '#ffffff';
                              ctx.font = `700 17px ${MONO}`;
                              ctx.fillText(String(currentProject.conversationCount), W - 56, 548);
                              ctx.fillText(String(projModelRows.length), W - 56, 584);
                              ctx.textAlign = 'left';
                            }

                            // 底部水印条（与当前模板预览 1:1 对齐）
                            ctx.strokeStyle = 'rgba(255, 255, 255, 0.1)';
                            ctx.lineWidth = 1.5;
                            ctx.beginPath();
                            ctx.moveTo(56, H - 64);
                            ctx.lineTo(W - 56, H - 64);
                            ctx.stroke();
                            ctx.textAlign = 'left';
                            ctx.fillStyle = '#71717a';
                            ctx.font = `500 17px ${MONO}`;
                            ctx.fillText(badgeMeta.footer, 56, H - 30);
                            ctx.textAlign = 'right';
                            ctx.fillStyle = '#d4d4d8';
                            ctx.fillText('Antigravity Manager', W - 56, H - 30);

                            canvas.toBlob((blob) => resolve(blob), 'image/png');
                          });
                        };

                        const triggerDownload = (blob: Blob) => {
                          const url = URL.createObjectURL(blob);
                          const a = document.createElement('a');
                          a.href = url;
                          const safeProjSlug =
                            currentProject.workspaceName === '__ALL__' ? 'all-projects' : currentProject.workspaceName;
                          a.download = `antigravity-${safeProjSlug}-${shareTemplate}.png`;
                          document.body.appendChild(a);
                          a.click();
                          document.body.removeChild(a);
                          URL.revokeObjectURL(url);
                        };

                        const handleCopyImage = async () => {
                          const blob = await renderPosterPngBlob();
                          if (!blob) return;
                          try {
                            if (navigator.clipboard && typeof ClipboardItem !== 'undefined') {
                              await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })]);
                            } else {
                              triggerDownload(blob);
                            }
                            setCopiedShare(true);
                            setTimeout(() => setCopiedShare(false), 1800);
                          } catch {
                            triggerDownload(blob);
                            setCopiedShare(true);
                            setTimeout(() => setCopiedShare(false), 1800);
                          }
                        };

                        const handleDownloadImage = async () => {
                          const blob = await renderPosterPngBlob();
                          if (blob) triggerDownload(blob);
                        };

                        return (
                          <div className="w-full flex items-center justify-between pt-1">
                            <span className="text-[10px] text-gray-400">
                              {tUi(lang, 'stats_poster_hd_hint')}
                            </span>
                            <div className="flex items-center gap-2">
                              <button
                                onClick={handleDownloadImage}
                                className="px-3 py-1.5 rounded-lg bg-gray-100 hover:bg-gray-200 dark:bg-white/[0.08] dark:hover:bg-white/[0.14] text-gray-700 dark:text-zinc-200 text-[11px] font-semibold flex items-center gap-1.5 transition-colors"
                              >
                                <Download className="w-3.5 h-3.5" />
                                <span>{tUi(lang, 'stats_download_png')}</span>
                              </button>
                              <button
                                onClick={handleCopyImage}
                                className="px-3.5 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-[11px] font-semibold flex items-center gap-1.5 transition-colors shadow-xs"
                              >
                                {copiedShare ? <Check className="w-3.5 h-3.5" /> : <Copy className="w-3.5 h-3.5" />}
                                <span>{copiedShare ? tUi(lang, 'stats_image_copied') : tUi(lang, 'stats_copy_poster')}</span>
                              </button>
                            </div>
                          </div>
                        );
                      })()}
                    </div>
                  </div>
                )}
              </div>
            );
          })()}
        </div>
      )}

      {/* ============================================================== */}
      {/* 视角 3：⚡ 模型全景对比 (无 Tab，100% 真实本地解析模型)           */}
      {/* ============================================================== */}
      {activeView === 'model' && (() => {
        const models = reportData.byModel;
        const palette = [
          { bar: 'bg-purple-500', text: 'text-purple-600 dark:text-purple-400', badge: 'bg-purple-500/10 text-purple-600 dark:text-purple-400', dot: 'bg-purple-500' },
          { bar: 'bg-blue-500', text: 'text-blue-600 dark:text-blue-400', badge: 'bg-blue-500/10 text-blue-600 dark:text-blue-400', dot: 'bg-blue-500' },
          { bar: 'bg-cyan-500', text: 'text-cyan-600 dark:text-cyan-400', badge: 'bg-cyan-500/10 text-cyan-600 dark:text-cyan-400', dot: 'bg-cyan-500' },
          { bar: 'bg-emerald-500', text: 'text-emerald-600 dark:text-emerald-400', badge: 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400', dot: 'bg-emerald-500' },
        ];

        const modelRecords =
          selectedModelFilter === '__ALL__'
            ? reportData.recentTraffic
            : reportData.recentTraffic.filter((r) => r.requestedModel === selectedModelFilter);
        const modelPerf = computePerfSeries(modelRecords, timeRange);
        const activeModelMeta =
          selectedModelFilter === '__ALL__'
            ? null
            : models.find((m) => m.requestedModel === selectedModelFilter) || null;
        const displayCacheRate =
          modelPerf.count > 0
            ? modelPerf.avgCache
            : activeModelMeta
            ? activeModelMeta.cacheHitRate
            : reportData.globalCacheHitRate;
        const displayAvgTps =
          modelPerf.avgTps > 0 ? modelPerf.avgTps : activeModelMeta ? activeModelMeta.avgTps || 0 : 0;
        const displayModelPeakTps =
          selectedModelFilter === '__ALL__'
            ? Math.max(reportData.peakSpeedTps ?? 0, modelPerf.peakTps, displayAvgTps)
            : Math.max(activeModelMeta?.peakTps ?? 0, modelPerf.peakTps, displayAvgTps);

        return (
          <div className="flex flex-col gap-3 animate-in fade-in-30 duration-200">
            {/* 模型筛选 Pill 条：支持「全部模型」或单个模型按时段查看缓存率与 tok/s 曲线 */}
            <div className="bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] rounded-xl p-2 px-3 shadow-2xs flex items-center justify-between gap-2 min-w-0">
              <div className="flex items-center gap-1.5 min-w-0 flex-1 overflow-x-auto no-scrollbar fade-mask-right pr-5">
                <button
                  onClick={() => setSelectedModelFilter('__ALL__')}
                  className={`px-2.5 py-1 rounded-lg text-[10.5px] font-semibold shrink-0 transition-all ${
                    selectedModelFilter === '__ALL__'
                      ? 'bg-gray-900 dark:bg-white text-white dark:text-gray-900 shadow-xs'
                      : 'text-gray-500 dark:text-zinc-400 hover:bg-gray-100 dark:hover:bg-white/[0.06]'
                  }`}
                >
                  {tUi(lang, 'stats_all_models_count', { count: models.length })}
                </button>
                {models.map((m, idx) => {
                  const isSel = selectedModelFilter === m.requestedModel;
                  const color = palette[idx % palette.length];
                  return (
                    <button
                      key={m.requestedModel}
                      onClick={() => setSelectedModelFilter(m.requestedModel)}
                      className={`px-2.5 py-1 rounded-lg text-[10.5px] font-mono font-semibold shrink-0 flex items-center gap-1.5 transition-all ${
                        isSel
                          ? 'bg-blue-600 text-white shadow-xs'
                          : 'bg-gray-100/80 dark:bg-white/[0.04] text-gray-700 dark:text-zinc-300 hover:bg-gray-200/70 dark:hover:bg-white/[0.08]'
                      }`}
                    >
                      <span className={`w-1.5 h-1.5 rounded-full ${isSel ? 'bg-white' : color.dot}`} />
                      <span>{m.requestedModel}</span>
                    </button>
                  );
                })}
              </div>
              <span className="text-[9.5px] font-mono text-gray-400 shrink-0 hidden sm:inline">
                {modelPerf.rangeLabel} · {modelPerf.count} reqs
              </span>
            </div>

            {/* 第一行 (50/50 黄金对称)：深空黑模型性能 Hero 卡 (6列) + 交互式双曲线遥测卡 (6列) */}
            <div className="grid grid-cols-1 md:grid-cols-12 gap-3.5 items-stretch">
              {/* 左卡 (6 cols)：深空黑 Model Performance Hero Card (Electrys / Hyper Charts 风格) */}
              {(() => {
                const heroTokens = activeModelMeta ? activeModelMeta.totalTokens : reportData.totalTokens;
                const heroReqs = activeModelMeta ? activeModelMeta.totalRequests : reportData.totalRequests;
                const heroShare = activeModelMeta ? activeModelMeta.sharePercentage : 100;
                const heroTtftMs = activeModelMeta ? activeModelMeta.avgTtftMs ?? 0 : reportData.avgTtftMs ?? 0;
                const heroDurMs = activeModelMeta ? activeModelMeta.avgDurationMs ?? 0 : reportData.avgDurationMs ?? 0;
                const topWs = activeModelMeta?.topWorkspaces?.[0]?.[0] || reportData.byProject[0]?.workspaceName || '-';
                const outerR = 54;
                const outerC = 2 * Math.PI * outerR;
                const outerDash = outerC * (Math.min(100, Math.max(0, displayCacheRate)) / 100);
                const innerR = 40;
                const innerC = 2 * Math.PI * innerR;
                const speedNorm = Math.min(100, (displayAvgTps / 150) * 100);
                const innerDash = innerC * (Math.max(8, speedNorm) / 100);

                return (
                  <div className="md:col-span-6 relative min-h-[246px] rounded-2xl bg-gradient-to-br from-[#232734] via-[#1a1d27] to-[#141720] dark:from-[#07090e] dark:via-[#05070b] dark:to-[#030407] border border-slate-900/15 dark:border-white/[0.14] shadow-[inset_0_1px_0_rgba(255,255,255,0.10),0_14px_34px_-14px_rgba(15,23,42,0.28)] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.12),0_18px_40px_-14px_rgba(0,0,0,0.85)] p-4 flex flex-col justify-between overflow-hidden text-white">
                    <div className="flex items-center justify-between gap-2">
                      <div className="flex items-center gap-2 min-w-0">
                        <span className="w-2.5 h-2.5 rounded-full bg-purple-400 shadow-[0_0_8px_#a855f7] shrink-0" />
                        <span className="text-sm font-extrabold tracking-tight truncate">
                          {selectedModelFilter === '__ALL__'
                            ? tUi(lang, 'stats_all_models_telemetry')
                            : selectedModelFilter}
                        </span>
                      </div>
                      <span className="px-2 py-0.5 rounded-full bg-white/[0.08] border border-white/10 text-[9px] font-mono text-purple-300 shrink-0">
                        {selectedModelFilter === '__ALL__'
                          ? tUi(lang, 'stats_models_badge', { count: models.length })
                          : tUi(lang, 'stats_share_badge', { pct: heroShare.toFixed(1) })}
                      </span>
                    </div>

                    <div className="flex-1 flex items-center justify-between gap-4 py-1">
                      {/* 左侧核心指标 */}
                      <div className="flex flex-col justify-between gap-3">
                        <div>
                          <div className="text-[9px] text-emerald-400 font-mono mb-0.5">● {tUi(lang, 'stats_total_throughput')}</div>
                          <div className="text-2xl font-black font-mono tracking-tight text-white leading-none">
                            {fmtTok(heroTokens)}
                          </div>
                          <div className="text-[9px] text-zinc-500 font-mono mt-1">{heroReqs.toLocaleString()} {tUi(lang, 'stats_unit_reqs')}</div>
                        </div>
                        <div className="flex items-center gap-3.5">
                          <div>
                            <div className="text-[8.5px] text-sky-400 font-mono">✦ {tUi(lang, 'stats_avg_speed_upper')}</div>
                            <div className="text-base font-black font-mono text-white leading-tight">
                              {displayAvgTps.toFixed(1)} <span className="text-[9px] text-zinc-400 font-normal">tok/s</span>
                            </div>
                          </div>
                          <div>
                            <div className="text-[8.5px] text-amber-400 font-mono">◆ {tUi(lang, 'stats_peak_speed_upper')}</div>
                            <div className="text-base font-black font-mono text-white leading-tight">
                              {displayModelPeakTps.toFixed(1)}
                            </div>
                          </div>
                          {heroTtftMs > 0 && (
                            <div
                              title={
                                heroDurMs > 0
                                  ? `TTFT ${(heroTtftMs / 1000).toFixed(2)}s · ${tUi(lang, 'stats_avg_duration')} ${(heroDurMs / 1000).toFixed(2)}s`
                                  : undefined
                              }
                            >
                              <div className="text-[8.5px] text-purple-400 font-mono">◉ TTFT</div>
                              <div className="text-base font-black font-mono text-white leading-tight">
                                {(heroTtftMs / 1000).toFixed(1)}<span className="text-[9px] text-zinc-400 font-normal">s</span>
                              </div>
                            </div>
                          )}
                        </div>
                      </div>

                      {/* 右侧双环同心霓虹性能仪表 (外环=缓存命中率，内环=生成速率) */}
                      <div className="relative w-[138px] h-[138px] shrink-0 flex items-center justify-center">
                        <svg viewBox="0 0 132 132" className="w-full h-full -rotate-90">
                          <circle cx="66" cy="66" r={outerR} fill="none" stroke="rgba(255,255,255,0.08)" strokeWidth="7" />
                          <circle
                            cx="66"
                            cy="66"
                            r={outerR}
                            fill="none"
                            stroke="#34d399"
                            strokeWidth="7"
                            strokeLinecap="round"
                            strokeDasharray={`${outerDash} ${outerC}`}
                          />
                          <circle cx="66" cy="66" r={innerR} fill="none" stroke="rgba(255,255,255,0.06)" strokeWidth="6" />
                          <circle
                            cx="66"
                            cy="66"
                            r={innerR}
                            fill="none"
                            stroke="#38bdf8"
                            strokeWidth="6"
                            strokeLinecap="round"
                            strokeDasharray={`${innerDash} ${innerC}`}
                          />
                        </svg>
                        <div className="absolute inset-0 flex flex-col items-center justify-center text-center">
                          <span className="text-lg font-black font-mono text-white leading-none">
                            {displayCacheRate.toFixed(1)}%
                          </span>
                          <span className="text-[8px] font-mono text-emerald-400 mt-0.5">
                            {tUi(lang, 'stats_cache_hit_short')}
                          </span>
                        </div>
                      </div>
                    </div>

                    <div className="pt-2 border-t border-white/[0.08] flex items-center justify-between text-[9px] font-mono text-zinc-400">
                      <span className="flex items-center gap-1.5">
                        <span className="w-2 h-2 rounded-full bg-emerald-400" />
                        {tUi(lang, 'stats_outer_cache')}
                      </span>
                      <span className="flex items-center gap-1.5">
                        <span className="w-2 h-2 rounded-full bg-sky-400" />
                        {tUi(lang, 'stats_inner_speed')}
                      </span>
                      <span className="text-zinc-300 truncate max-w-[120px]">
                        {tUi(lang, 'stats_top_workspace_prefix')} {topWs}
                      </span>
                    </div>
                  </div>
                );
              })()}

              {/* 右卡 (6 cols)：交互式双曲线性能遥测卡 (缓存率 % + tok/s 速度) */}
              <div className="md:col-span-6 p-3.5 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between gap-2.5">
                <div className="flex-1 flex flex-col justify-between gap-1 min-h-0">
                  <div className="flex items-center justify-between text-[10.5px]">
                    <span className="font-semibold text-gray-800 dark:text-zinc-200 flex items-center gap-1.5">
                      <ShieldCheck className="w-3.5 h-3.5 text-emerald-500 shrink-0" />
                      <span>{tUi(lang, 'stats_cache_hit_curve')}</span>
                    </span>
                    <span className="font-mono font-bold text-emerald-600 dark:text-emerald-400">
                      {displayCacheRate.toFixed(1)}% <span className="text-[9px] text-gray-400 font-normal">· {modelPerf.rangeLabel}</span>
                    </span>
                  </div>
                  <div className="h-[74px] w-full overflow-hidden">
                    {renderSmoothCurve(modelPerf.cacheSeries, '#10b981', 'mod-cache-grad', 100, 74, 90)}
                  </div>
                </div>

                <div className="border-t border-gray-100 dark:border-white/5 pt-2 flex-1 flex flex-col justify-between gap-1 min-h-0">
                  <div className="flex items-center justify-between text-[10.5px]">
                    <span className="font-semibold text-gray-800 dark:text-zinc-200 flex items-center gap-1.5">
                      <TrendingUp className="w-3.5 h-3.5 text-sky-500 shrink-0" />
                      <span>{tUi(lang, 'stats_gen_speed_curve')}</span>
                    </span>
                    <span className="font-mono font-bold text-sky-600 dark:text-sky-400">
                      {displayAvgTps.toFixed(1)} tok/s <span className="text-[9px] text-gray-400 font-normal">· {tUi(lang, 'stats_peak')} {displayModelPeakTps.toFixed(1)}</span>
                    </span>
                  </div>
                  <div className="h-[74px] w-full overflow-hidden">
                    {renderSmoothCurve(modelPerf.tpsSeries, '#0ea5e9', 'mod-tps-grad', Math.max(modelPerf.seriesPeakTps, 10) * 1.15, 74)}
                  </div>
                </div>
              </div>
            </div>

            {/* 第二行 (50/50 零滚动收尾)：全模型横向 PK 榜 (6列，点击即联动) + 工程分布/模型调用融合卡 (6列) */}
            <div className="grid grid-cols-1 md:grid-cols-12 gap-3.5 items-stretch">
              {/* 左卡 (6 cols)：全模型横向 PK 榜 (三合一去重，点击任意行聚焦该模型) */}
              <div className="md:col-span-6 p-3.5 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col gap-2">
                <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
                  <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                    <Cpu className="w-3.5 h-3.5 text-purple-500" />
                    <span>{tUi(lang, 'stats_models_leaderboard')}</span>
                  </span>
                  <span className="text-[9px] text-gray-400 font-mono">{models.length} {tUi(lang, 'stats_unit_models')}</span>
                </div>

                {/* Apple Storage 风格连续比例条 */}
                <div className="h-2.5 w-full rounded-full bg-gray-100 dark:bg-white/[0.06] p-0.5 flex items-center gap-0.5 overflow-hidden">
                  {models.map((m, idx) => (
                    <div
                      key={m.requestedModel}
                      title={`${m.requestedModel}: ${m.sharePercentage.toFixed(1)}%`}
                      style={{ width: `${Math.max(4, m.sharePercentage)}%` }}
                      className={`h-full rounded-full ${palette[idx % palette.length].bar}`}
                    />
                  ))}
                </div>

                <div className="mini-scroll fade-mask-bottom flex flex-col gap-1.5 max-h-[168px] overflow-y-auto pr-1 pb-3">
                  {models.map((m, idx) => {
                    const color = palette[idx % palette.length];
                    const isSel = selectedModelFilter === m.requestedModel;
                    return (
                      <button
                        key={m.requestedModel}
                        onClick={() => setSelectedModelFilter(isSel ? '__ALL__' : m.requestedModel)}
                        className={`w-full text-left px-2.5 py-1.5 rounded-xl border transition-all flex items-center justify-between gap-2 ${
                          isSel
                            ? 'bg-indigo-500/10 border-indigo-500/40 ring-1 ring-indigo-500/20'
                            : 'bg-gray-50/90 dark:bg-white/[0.03] border-gray-100/80 dark:border-white/[0.04] hover:bg-gray-100/80 dark:hover:bg-white/[0.06]'
                        }`}
                      >
                        <div className="flex items-center gap-2 min-w-0 flex-1">
                          <span className={`w-2 h-2 rounded-full shrink-0 ${color.dot}`} />
                          <span className="text-[10.5px] font-semibold font-mono text-gray-800 dark:text-zinc-200 truncate">
                            {m.requestedModel}
                          </span>
                        </div>
                        <div className="flex items-center gap-2 shrink-0 font-mono text-[9.5px]">
                          <span className="px-1.5 py-0.5 rounded bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 font-semibold">
                            {m.cacheHitRate.toFixed(0)}%
                          </span>
                          <span className="text-sky-600 dark:text-sky-400 w-14 text-right">
                            {(m.avgTps || 0) > 0 ? `${(m.avgTps || 0).toFixed(0)} t/s` : '-'}
                          </span>
                          <span className="font-bold text-gray-900 dark:text-zinc-100 w-12 text-right">
                            {fmtTok(m.totalTokens)}
                          </span>
                        </div>
                      </button>
                    );
                  })}
                </div>
              </div>

              {/* 右卡 (6 cols)：主力工程分布 & 该模型最近调用融合卡 */}
              <div className="md:col-span-6 p-3.5 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between gap-2">
                <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
                  <div className="inline-flex p-0.5 rounded-lg bg-gray-100/90 dark:bg-white/[0.06]">
                    <button
                      onClick={() => setModelDetailTab('workspaces')}
                      className={`px-2.5 py-0.5 rounded-md text-[10px] font-semibold flex items-center gap-1 transition-all ${
                        modelDetailTab === 'workspaces'
                          ? 'bg-white dark:bg-zinc-800 text-gray-900 dark:text-white shadow-2xs'
                          : 'text-gray-500 dark:text-zinc-400'
                      }`}
                    >
                      <FolderGit2 className="w-3 h-3 text-indigo-500" />
                      <span>{tUi(lang, 'stats_top_workspaces_dist')}</span>
                    </button>
                    <button
                      onClick={() => setModelDetailTab('requests')}
                      className={`px-2.5 py-0.5 rounded-md text-[10px] font-semibold flex items-center gap-1 transition-all ${
                        modelDetailTab === 'requests'
                          ? 'bg-white dark:bg-zinc-800 text-gray-900 dark:text-white shadow-2xs'
                          : 'text-gray-500 dark:text-zinc-400'
                      }`}
                    >
                      <Terminal className="w-3 h-3 text-blue-500" />
                      <span>{tUi(lang, 'stats_recent_calls')}</span>
                    </button>
                  </div>
                  <button
                    onClick={() => {
                      setSearchQuery(selectedModelFilter === '__ALL__' ? '' : selectedModelFilter);
                      setWorkspaceFilter('all');
                      setActiveView('ledger');
                    }}
                    className="text-[9.5px] text-blue-600 dark:text-blue-400 hover:underline flex items-center gap-0.5 font-medium"
                  >
                    <span>{tUi(lang, 'stats_all_ledger')}</span>
                    <ChevronRight className="w-3 h-3" />
                  </button>
                </div>

                {modelDetailTab === 'workspaces' ? (
                  <div className="flex-1 flex flex-col justify-center gap-2">
                    {(() => {
                      const wsList: [string, number][] = activeModelMeta?.topWorkspaces?.length
                        ? activeModelMeta.topWorkspaces
                        : reportData.byProject.slice(0, 5).map((p) => [
                            p.workspaceName,
                            reportData.totalTokens > 0 ? (p.totalTokens / reportData.totalTokens) * 100 : 0,
                          ]);
                      return wsList.slice(0, 5).map(([ws, pct]) => (
                        <button
                          key={ws}
                          onClick={() => {
                            setSelectedProjectName(ws);
                            setActiveView('project');
                          }}
                          className="w-full text-left flex flex-col gap-1 group"
                        >
                          <div className="flex items-center justify-between text-[10.5px]">
                            <span className="font-semibold text-gray-800 dark:text-zinc-200 group-hover:text-indigo-600 dark:group-hover:text-indigo-400 truncate">
                              {ws}
                            </span>
                            <span className="font-mono font-bold text-gray-600 dark:text-zinc-300 shrink-0">
                              {pct.toFixed(1)}%
                            </span>
                          </div>
                          <div className="w-full h-1.5 rounded-full bg-gray-100 dark:bg-white/[0.06] overflow-hidden">
                            <div
                              style={{ width: `${Math.max(4, pct)}%` }}
                              className="h-full rounded-full bg-gradient-to-r from-blue-500/80 to-indigo-400/75"
                            />
                          </div>
                        </button>
                      ));
                    })()}
                  </div>
                ) : (
                  <div className="flex-1 flex flex-col justify-center gap-1.5">
                    {modelRecords.slice(0, 4).map((r) => {
                      const tot = r.inputTokens + r.cachedTokens + r.outputTokens;
                      const cPct =
                        r.inputTokens + r.cachedTokens > 0
                          ? Math.round((r.cachedTokens / (r.inputTokens + r.cachedTokens)) * 100)
                          : 0;
                      return (
                        <button
                          key={r.id}
                          onClick={() => {
                            setSelectedRecord(r);
                            setActiveView('ledger');
                          }}
                          className="w-full text-left px-2.5 py-1.5 rounded-xl bg-gray-50/90 dark:bg-white/[0.03] hover:bg-blue-50/50 dark:hover:bg-blue-500/[0.06] border border-gray-100/80 dark:border-white/[0.04] flex items-center justify-between gap-2 text-[10px] font-mono transition-colors"
                        >
                          <div className="flex items-center gap-2 min-w-0">
                            <span className="px-1.5 py-0.5 rounded bg-indigo-500/10 text-indigo-600 dark:text-indigo-400 text-[9px] font-semibold truncate">
                              {r.workspace}
                            </span>
                            <span className="text-[8.5px] text-gray-400 shrink-0">{r.time.split(' ')[1] || r.time}</span>
                          </div>
                          <div className="flex items-center gap-2 shrink-0">
                            <span className="text-[9px] text-sky-600 dark:text-sky-400">
                              {r.tokensPerSecond > 0 ? `${r.tokensPerSecond.toFixed(0)} t/s` : '-'}
                            </span>
                            <span className="text-[9px] text-emerald-600 dark:text-emerald-400">{cPct}%</span>
                            <span className="font-bold text-gray-800 dark:text-zinc-200">{fmtTok(tot)}</span>
                          </div>
                        </button>
                      );
                    })}
                    {modelRecords.length === 0 && (
                      <div className="py-6 text-center text-[10px] text-gray-400">
                        {tUi(lang, 'stats_no_model_calls')}
                      </div>
                    )}
                  </div>
                )}
              </div>
            </div>
          </div>
        );
      })()}

      {/* ============================================================== */}
      {/* 视角 4：📜 会话流水 (按工程/关键词筛选 + 真实 Token 列表 + 弹窗)   */}
      {/* ============================================================== */}
      {activeView === 'ledger' && (() => {
        const displayRecords = filteredRecords.filter((r) => {
          if (ledgerCacheFilter === 'miss') {
            const totalIn = r.inputTokens + r.cachedTokens;
            const cPct = totalIn > 0 ? (r.cachedTokens / totalIn) * 100 : 0;
            return cPct < 50;
          }
          return true;
        });
        const sumTokens = displayRecords.reduce((acc, r) => acc + r.inputTokens + r.cachedTokens + r.outputTokens, 0);
        const sumCached = displayRecords.reduce((acc, r) => acc + r.cachedTokens, 0);
        const sumIn = displayRecords.reduce((acc, r) => acc + r.inputTokens + r.cachedTokens, 0);
        const filteredCachePct = sumIn > 0 ? (sumCached / sumIn) * 100 : 0;

        const fmtRowTime = (t: string) => {
          if (rangeBounds.isSingleDay) {
            return t.includes(' ') ? t.split(' ')[1] : t;
          }
          // 多日范围保留 MM-DD HH:mm，便于跨天检索
          if (t.length >= 16 && t.includes('-')) {
            return t.slice(5, 16);
          }
          return t;
        };

        // 按会话聚合（仅在 sessions 模式下渲染，使用 displayRecords 已按最新时间倒序的特性）
        const sessionGroups = (() => {
          if (ledgerMode !== 'sessions') return [];
          const map = new Map<
            string,
            {
              sessionKey: string;
              sessionTitle: string;
              workspace: string;
              primaryModel: string;
              latestTime: string;
              inputTokens: number;
              cachedTokens: number;
              outputTokens: number;
              thinkingTokens: number;
              measuredOut: number;
              measuredDurMs: number;
              tpsSum: number;
              tpsCnt: number;
              records: RequestRecord[];
            }
          >();
          for (const r of displayRecords) {
            const key = r.sessionId || r.id;
            let g = map.get(key);
            if (!g) {
              g = {
                sessionKey: key,
                sessionTitle: r.sessionTitle || `#${key.slice(0, 8)}`,
                workspace: r.workspace,
                primaryModel: r.requestedModel,
                latestTime: r.time,
                inputTokens: 0,
                cachedTokens: 0,
                outputTokens: 0,
                thinkingTokens: 0,
                measuredOut: 0,
                measuredDurMs: 0,
                tpsSum: 0,
                tpsCnt: 0,
                records: [],
              };
              map.set(key, g);
            }
            if (!g.sessionTitle && r.sessionTitle) g.sessionTitle = r.sessionTitle;
            g.inputTokens += r.inputTokens;
            g.cachedTokens += r.cachedTokens;
            g.outputTokens += r.outputTokens;
            g.thinkingTokens += r.thinkingTokens ?? 0;
            if (r.durationMs > 0 && r.outputTokens > 0) {
              g.measuredOut += r.outputTokens;
              g.measuredDurMs += r.durationMs;
            }
            if (r.tokensPerSecond > 0) {
              g.tpsSum += r.tokensPerSecond;
              g.tpsCnt += 1;
            }
            g.records.push(r);
          }
          return Array.from(map.values());
        })();

        const pageSize = ledgerMode === 'requests' ? 50 : 25;
        const totalItems = ledgerMode === 'requests' ? displayRecords.length : sessionGroups.length;
        const totalPages = Math.max(1, Math.ceil(totalItems / pageSize));
        const safePage = Math.min(ledgerPage, totalPages);
        const pagedRequests =
          ledgerMode === 'requests' ? displayRecords.slice((safePage - 1) * pageSize, safePage * pageSize) : [];
        const pagedSessions =
          ledgerMode === 'sessions' ? sessionGroups.slice((safePage - 1) * pageSize, safePage * pageSize) : [];

        const renderRequestRow = (r: RequestRecord, isNested = false) => {
          const totalIn = r.inputTokens + r.cachedTokens;
          const cachePct = totalIn > 0 ? (r.cachedTokens / totalIn) * 100 : 0;
          const timeText = fmtRowTime(r.time);
          const ttftSec =
            r.ttftMs > 0 ? `${(r.ttftMs / 1000).toFixed(1)}s` : r.durationMs > 0 ? `${(r.durationMs / 1000).toFixed(1)}s` : '-';
          const timingTooltip =
            r.ttftMs > 0 || r.durationMs > 0
              ? [
                  r.ttftMs > 0 ? `TTFT ${(r.ttftMs / 1000).toFixed(2)}s` : null,
                  (r.streamingDurationMs ?? 0) > 0
                    ? `${tUi(lang, 'stats_detail_streaming')} ${((r.streamingDurationMs ?? 0) / 1000).toFixed(2)}s`
                    : null,
                  r.durationMs > 0 ? `${tUi(lang, 'stats_detail_total_dur')} ${(r.durationMs / 1000).toFixed(2)}s` : null,
                ]
                  .filter(Boolean)
                  .join(' · ')
              : undefined;
          return (
            <div
              key={r.id}
              onClick={() => setSelectedRecord(r)}
              className={`grid grid-cols-12 gap-2 px-4 py-2.5 items-center hover:bg-blue-50/40 dark:hover:bg-blue-500/[0.06] transition-colors cursor-pointer text-[10.5px] font-mono ${
                isNested ? 'bg-gray-50/55 dark:bg-white/[0.015] pl-7 border-l-2 border-blue-500/40' : ''
              }`}
            >
              <div className="col-span-1 text-gray-500 dark:text-zinc-400 truncate text-[9.5px]" title={r.time}>
                {timeText}
              </div>
              <div className="col-span-2 min-w-0 flex items-center gap-1.5">
                <span className="px-1.5 py-0.5 rounded-md bg-indigo-500/10 text-indigo-600 dark:text-indigo-400 text-[9.5px] font-semibold truncate max-w-[84px]">
                  {r.workspace}
                </span>
                <span className="text-[9.5px] text-gray-400 truncate">
                  {r.sessionTitle || `#${r.id.slice(0, 6)}`}
                </span>
              </div>
              <div className="col-span-2 min-w-0 flex items-center gap-1.5">
                <span className="w-1.5 h-1.5 rounded-full bg-blue-500 shrink-0" />
                <span className="font-semibold text-gray-800 dark:text-zinc-200 truncate text-[10px]">
                  {r.requestedModel}
                </span>
              </div>
              <div className="col-span-1 text-right font-bold text-gray-800 dark:text-zinc-200">
                {fmtTok(totalIn)}
              </div>
              <div className="col-span-1 text-right font-semibold text-emerald-600 dark:text-emerald-400">
                {r.cachedTokens > 0 ? fmtTok(r.cachedTokens) : '0'}
              </div>
              <div className="col-span-1 text-right text-gray-600 dark:text-zinc-300">
                {fmtTok(r.inputTokens)}
              </div>
              <div className="col-span-1 text-right flex flex-col items-end leading-tight">
                <span className="font-semibold text-gray-800 dark:text-zinc-200">{r.outputTokens.toLocaleString()}</span>
                {(r.thinkingTokens ?? 0) > 0 && (
                  <span className="text-[8.5px] px-1 rounded bg-purple-500/10 text-purple-600 dark:text-purple-400">
                    {tUi(lang, 'stats_think_short')} {fmtTok(r.thinkingTokens ?? 0)}
                  </span>
                )}
              </div>
              <div className="col-span-1 text-right flex flex-col items-end leading-tight" title={timingTooltip}>
                <span className="font-semibold text-gray-700 dark:text-zinc-300">{ttftSec}</span>
                <span className="text-[8.5px] text-sky-600 dark:text-sky-400">
                  {r.tokensPerSecond > 0 ? `${r.tokensPerSecond.toFixed(1)} tok/s` : '-'}
                </span>
              </div>
              <div className="col-span-2 flex items-center justify-end gap-2">
                <div className="w-10 h-1.5 rounded-full bg-gray-100 dark:bg-white/[0.06] overflow-hidden hidden sm:block">
                  <div
                    style={{ width: `${Math.min(100, cachePct)}%` }}
                    className={`h-full rounded-full ${
                      cachePct >= 80 ? 'bg-emerald-500' : cachePct >= 40 ? 'bg-amber-400' : 'bg-rose-400'
                    }`}
                  />
                </div>
                <span className="text-[10px] font-bold text-gray-700 dark:text-zinc-300 w-10 text-right">
                  {cachePct.toFixed(1)}%
                </span>
                <span
                  className={`px-1.5 py-0.5 rounded text-[8.5px] font-sans font-semibold shrink-0 ${
                    cachePct >= 80
                      ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400'
                      : cachePct > 0
                      ? 'bg-amber-500/10 text-amber-600 dark:text-amber-400'
                      : 'bg-gray-100 dark:bg-white/[0.06] text-gray-400'
                  }`}
                >
                  {cachePct >= 80 ? tUi(lang, 'stats_cache_high') : cachePct > 0 ? tUi(lang, 'stats_cache_low') : tUi(lang, 'stats_cache_cold')}
                </span>
              </div>
            </div>
          );
        };

        return (
          <div className="flex flex-col gap-3 animate-in fade-in-30 duration-200">
            <div className="bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] rounded-xl p-2 px-3 shadow-2xs flex flex-wrap items-center justify-between gap-2">
              <div className="flex items-center gap-2 flex-wrap">
                {/* 视图粒度切换：单步请求流 | 按会话分组 */}
                <div className="inline-flex items-center p-0.5 rounded-lg bg-gray-100/90 dark:bg-white/[0.05] border border-gray-200/50 dark:border-white/[0.04]">
                  <button
                    onClick={() => setLedgerMode('requests')}
                    className={`px-2 py-0.5 rounded-md text-[10px] font-semibold transition-all ${
                      ledgerMode === 'requests'
                        ? 'bg-white dark:bg-zinc-800 text-blue-600 dark:text-blue-400 shadow-2xs dark:ring-1 dark:ring-white/[0.08]'
                        : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                    }`}
                  >
                    {tUi(lang, 'stats_ledger_mode_requests')}
                  </button>
                  <button
                    onClick={() => setLedgerMode('sessions')}
                    className={`px-2 py-0.5 rounded-md text-[10px] font-semibold transition-all ${
                      ledgerMode === 'sessions'
                        ? 'bg-white dark:bg-zinc-800 text-blue-600 dark:text-blue-400 shadow-2xs dark:ring-1 dark:ring-white/[0.08]'
                        : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                    }`}
                  >
                    {tUi(lang, 'stats_ledger_mode_sessions')}
                  </button>
                </div>

                <div className="relative w-56">
                  <Search className="w-3.5 h-3.5 absolute left-2.5 top-1/2 -translate-y-1/2 text-gray-400" />
                  <input
                    type="text"
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    placeholder={tUi(lang, 'stats_ledger_search_placeholder')}
                    className="w-full pl-8 pr-6 py-1 text-[11px] rounded-lg bg-gray-100/90 dark:bg-white/[0.05] border border-transparent focus:border-blue-500/50 text-gray-900 dark:text-zinc-100 placeholder-gray-400 outline-none"
                  />
                  {searchQuery && (
                    <button onClick={() => setSearchQuery('')} className="absolute right-2 top-1/2 -translate-y-1/2 text-gray-400">
                      <X className="w-3 h-3" />
                    </button>
                  )}
                </div>

                {/* 状态筛选：全部状态 | 掉缓存 */}
                <div className="inline-flex items-center p-0.5 rounded-lg bg-gray-100/90 dark:bg-white/[0.05] border border-gray-200/50 dark:border-white/[0.04]">
                  <button
                    onClick={() => setLedgerCacheFilter('all')}
                    className={`px-2 py-0.5 rounded-md text-[10px] font-semibold transition-all ${
                      ledgerCacheFilter === 'all'
                        ? 'bg-white dark:bg-zinc-800 text-gray-900 dark:text-zinc-100 shadow-2xs dark:ring-1 dark:ring-white/[0.08]'
                        : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                    }`}
                  >
                    {tUi(lang, 'stats_ledger_all_status')}
                  </button>
                  <button
                    onClick={() => setLedgerCacheFilter('miss')}
                    className={`px-2 py-0.5 rounded-md text-[10px] font-semibold transition-all ${
                      ledgerCacheFilter === 'miss' ? 'bg-amber-500 text-white shadow-2xs' : 'text-gray-500 dark:text-zinc-400 hover:text-amber-600 dark:hover:text-amber-400'
                    }`}
                  >
                    {tUi(lang, 'stats_ledger_cache_miss')}
                  </button>
                </div>

                {/* 按真实工作区筛选 */}
                <div className="inline-flex items-center p-0.5 rounded-lg bg-gray-100/90 dark:bg-white/[0.05] border border-gray-200/50 dark:border-white/[0.04]">
                  <button
                    onClick={() => setWorkspaceFilter('all')}
                    className={`px-2 py-0.5 rounded-md text-[10px] font-medium transition-all ${
                      workspaceFilter === 'all' ? 'bg-white dark:bg-zinc-800 text-blue-600 dark:text-blue-400 font-semibold shadow-2xs dark:ring-1 dark:ring-white/[0.08]' : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                    }`}
                  >
                    {tUi(lang, 'stats_ledger_all_workspaces')}
                  </button>
                  {reportData.byProject.slice(0, 4).map((p) => (
                    <button
                      key={p.workspaceName}
                      onClick={() => setWorkspaceFilter(p.workspaceName)}
                      className={`px-2 py-0.5 rounded-md text-[10px] font-medium transition-all ${
                        workspaceFilter === p.workspaceName ? 'bg-white dark:bg-zinc-800 text-blue-600 dark:text-blue-400 font-semibold shadow-2xs dark:ring-1 dark:ring-white/[0.08]' : 'text-gray-500 dark:text-zinc-400 hover:text-gray-800 dark:hover:text-zinc-200'
                      }`}
                    >
                      {p.workspaceName}
                    </button>
                  ))}
                </div>
              </div>

              <div className="flex items-center gap-2 text-[9.5px] font-mono">
                {ledgerMode === 'sessions' && (
                  <span className="px-2 py-0.5 rounded-md bg-indigo-500/10 text-indigo-600 dark:text-indigo-400 font-semibold">
                    {sessionGroups.length} {tUi(lang, 'stats_unit_sessions')}
                  </span>
                )}
                <span className="px-2 py-0.5 rounded-md bg-gray-100/80 dark:bg-white/[0.04] text-gray-600 dark:text-zinc-300">
                  {displayRecords.length} {tUi(lang, 'stats_unit_reqs')}
                </span>
                <span className="px-2 py-0.5 rounded-md bg-blue-500/10 text-blue-600 dark:text-blue-400 font-semibold">{fmtTok(sumTokens)}</span>
                <span className="px-2 py-0.5 rounded-md bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 font-semibold">{filteredCachePct.toFixed(1)}% {tUi(lang, 'stats_cache')}</span>
              </div>
            </div>

            <div className="bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] rounded-2xl overflow-hidden shadow-xs">
              <div className="grid grid-cols-12 gap-2 px-4 py-2.5 bg-gray-50/90 dark:bg-[#16171b]/90 border-b border-gray-200/60 dark:border-white/5 text-[10px] font-semibold text-gray-400 dark:text-zinc-500">
                <div className="col-span-1">{tUi(lang, 'stats_th_time')}</div>
                <div className="col-span-2">{tUi(lang, 'stats_th_workspace')}</div>
                <div className="col-span-2">{tUi(lang, 'stats_th_model')}</div>
                <div className="col-span-1 text-right">{tUi(lang, 'stats_th_context')}</div>
                <div className="col-span-1 text-right">{tUi(lang, 'stats_th_cached')}</div>
                <div className="col-span-1 text-right">{tUi(lang, 'stats_th_input')}</div>
                <div className="col-span-1 text-right">{tUi(lang, 'stats_th_output')}</div>
                <div className="col-span-1 text-right">{tUi(lang, 'stats_th_speed')}</div>
                <div className="col-span-2 text-right">{tUi(lang, 'stats_th_cache_hit')}</div>
              </div>
              <div className="divide-y divide-gray-100/90 dark:divide-white/[0.05]">
                {displayRecords.length === 0 ? (
                  <div className="py-12 text-center text-xs text-gray-400 dark:text-zinc-500">
                    {tUi(lang, 'stats_no_ledger_records')}
                  </div>
                ) : ledgerMode === 'requests' ? (
                  pagedRequests.map((r) => renderRequestRow(r, false))
                ) : (
                  pagedSessions.map((g) => {
                    const isExpanded = !!expandedSessions[g.sessionKey];
                    const gTotalIn = g.inputTokens + g.cachedTokens;
                    const gCachePct = gTotalIn > 0 ? (g.cachedTokens / gTotalIn) * 100 : 0;
                    const gAvgTps =
                      g.measuredDurMs > 0 && g.measuredOut > 0
                        ? (g.measuredOut * 1000) / g.measuredDurMs
                        : g.tpsCnt > 0
                        ? g.tpsSum / g.tpsCnt
                        : 0;
                    return (
                      <React.Fragment key={g.sessionKey}>
                        <div
                          onClick={() =>
                            setExpandedSessions((prev) => ({
                              ...prev,
                              [g.sessionKey]: !prev[g.sessionKey],
                            }))
                          }
                          className="grid grid-cols-12 gap-2 px-4 py-2.5 items-center hover:bg-indigo-50/40 dark:hover:bg-indigo-500/[0.06] transition-colors cursor-pointer text-[10.5px] font-mono"
                        >
                          <div className="col-span-1 text-gray-500 dark:text-zinc-400 truncate text-[9.5px] flex items-center gap-1" title={g.latestTime}>
                            {isExpanded ? (
                              <ChevronDown className="w-3 h-3 text-blue-500 shrink-0" />
                            ) : (
                              <ChevronRight className="w-3 h-3 text-gray-400 shrink-0" />
                            )}
                            <span className="truncate">{fmtRowTime(g.latestTime)}</span>
                          </div>
                          <div className="col-span-2 min-w-0 flex items-center gap-1.5">
                            <span className="px-1.5 py-0.5 rounded-md bg-indigo-500/10 text-indigo-600 dark:text-indigo-400 text-[9.5px] font-semibold truncate max-w-[76px]">
                              {g.workspace}
                            </span>
                            <span className="text-[10px] font-sans font-semibold text-gray-800 dark:text-zinc-200 truncate">
                              {g.sessionTitle}
                            </span>
                            <span className="px-1 py-0 rounded bg-blue-500/10 text-blue-600 dark:text-blue-400 text-[8.5px] font-bold shrink-0">
                              {g.records.length}
                            </span>
                          </div>
                          <div className="col-span-2 min-w-0 flex items-center gap-1.5">
                            <span className="w-1.5 h-1.5 rounded-full bg-purple-500 shrink-0" />
                            <span className="font-semibold text-gray-800 dark:text-zinc-200 truncate text-[10px]">
                              {g.primaryModel}
                            </span>
                          </div>
                          <div className="col-span-1 text-right font-bold text-gray-800 dark:text-zinc-200">
                            {fmtTok(gTotalIn)}
                          </div>
                          <div className="col-span-1 text-right font-semibold text-emerald-600 dark:text-emerald-400">
                            {g.cachedTokens > 0 ? fmtTok(g.cachedTokens) : '0'}
                          </div>
                          <div className="col-span-1 text-right text-gray-600 dark:text-zinc-300">
                            {fmtTok(g.inputTokens)}
                          </div>
                          <div className="col-span-1 text-right flex flex-col items-end leading-tight">
                            <span className="font-semibold text-gray-800 dark:text-zinc-200">{fmtTok(g.outputTokens)}</span>
                            {g.thinkingTokens > 0 && (
                              <span className="text-[8.5px] px-1 rounded bg-purple-500/10 text-purple-600 dark:text-purple-400">
                                {tUi(lang, 'stats_think_short')} {fmtTok(g.thinkingTokens)}
                              </span>
                            )}
                          </div>
                          <div className="col-span-1 text-right flex flex-col items-end leading-tight">
                            <span className="text-[9.5px] text-gray-500">{g.records.length} {tUi(lang, 'stats_unit_steps')}</span>
                            <span className="text-[8.5px] text-sky-600 dark:text-sky-400">
                              {gAvgTps > 0 ? `${gAvgTps.toFixed(1)} tok/s` : '-'}
                            </span>
                          </div>
                          <div className="col-span-2 flex items-center justify-end gap-2">
                            <div className="w-10 h-1.5 rounded-full bg-gray-100 dark:bg-white/[0.06] overflow-hidden hidden sm:block">
                              <div
                                style={{ width: `${Math.min(100, gCachePct)}%` }}
                                className={`h-full rounded-full ${
                                  gCachePct >= 80 ? 'bg-emerald-500' : gCachePct >= 40 ? 'bg-amber-400' : 'bg-rose-400'
                                }`}
                              />
                            </div>
                            <span className="text-[10px] font-bold text-gray-700 dark:text-zinc-300 w-10 text-right">
                              {gCachePct.toFixed(1)}%
                            </span>
                          </div>
                        </div>
                        {isExpanded && g.records.map((r) => renderRequestRow(r, true))}
                      </React.Fragment>
                    );
                  })
                )}
              </div>

              {/* 底部分页控制器（每页 50 条单步请求 / 25 个会话） */}
              {totalPages > 1 && (
                <div className="px-4 py-2.5 bg-gray-50/80 dark:bg-[#16171b]/80 border-t border-gray-200/60 dark:border-white/5 flex items-center justify-between text-[10.5px]">
                  <span className="text-gray-500 dark:text-zinc-400 font-mono">
                    {tUi(lang, 'stats_pagination_summary', {
                      start: (safePage - 1) * pageSize + 1,
                      end: Math.min(safePage * pageSize, totalItems),
                      total: totalItems,
                      unit: tUi(lang, ledgerMode === 'requests' ? 'stats_unit_reqs_full' : 'stats_unit_sessions_full'),
                    })}
                  </span>
                  <div className="flex items-center gap-1.5 font-mono">
                    <button
                      onClick={() => setLedgerPage(1)}
                      disabled={safePage <= 1}
                      className="px-2 py-0.5 rounded-md border border-gray-200/80 dark:border-white/10 disabled:opacity-40 hover:bg-white dark:hover:bg-white/10 text-gray-700 dark:text-zinc-300 transition-colors"
                    >
                      1
                    </button>
                    <button
                      onClick={() => setLedgerPage((p) => Math.max(1, p - 1))}
                      disabled={safePage <= 1}
                      className="px-2 py-0.5 rounded-md border border-gray-200/80 dark:border-white/10 disabled:opacity-40 hover:bg-white dark:hover:bg-white/10 text-gray-700 dark:text-zinc-300 flex items-center gap-0.5 transition-colors"
                    >
                      <ChevronLeft className="w-3 h-3" />
                      <span>{tUi(lang, 'stats_prev_page')}</span>
                    </button>
                    <span className="px-2.5 py-0.5 rounded-md bg-blue-500/10 text-blue-600 dark:text-blue-400 font-bold">
                      {safePage} / {totalPages}
                    </span>
                    <button
                      onClick={() => setLedgerPage((p) => Math.min(totalPages, p + 1))}
                      disabled={safePage >= totalPages}
                      className="px-2 py-0.5 rounded-md border border-gray-200/80 dark:border-white/10 disabled:opacity-40 hover:bg-white dark:hover:bg-white/10 text-gray-700 dark:text-zinc-300 flex items-center gap-0.5 transition-colors"
                    >
                      <span>{tUi(lang, 'stats_next_page')}</span>
                      <ChevronRight className="w-3 h-3" />
                    </button>
                    <button
                      onClick={() => setLedgerPage(totalPages)}
                      disabled={safePage >= totalPages}
                      className="px-2 py-0.5 rounded-md border border-gray-200/80 dark:border-white/10 disabled:opacity-40 hover:bg-white dark:hover:bg-white/10 text-gray-700 dark:text-zinc-300 transition-colors"
                    >
                      {totalPages}
                    </button>
                  </div>
                </div>
              )}
            </div>

            {selectedRecord && (() => {
              const thinkTok = Math.min(selectedRecord.thinkingTokens ?? 0, selectedRecord.outputTokens);
              const visibleOutTok = Math.max(0, selectedRecord.outputTokens - thinkTok);
              const totalTok = selectedRecord.inputTokens + selectedRecord.cachedTokens + selectedRecord.outputTokens;
              const pct = (v: number) => (totalTok > 0 && v > 0 ? Math.max(2, (v / totalTok) * 100) : 0);
              const hasTiming = selectedRecord.ttftMs > 0 || selectedRecord.durationMs > 0 || selectedRecord.tokensPerSecond > 0;
              const ctxBudget = selectedRecord.contextBudgetTokens ?? 0;
              const ctxUsed = selectedRecord.contextUsedTokens ?? 0;
              const ctxPct = ctxBudget > 0 ? Math.min(100, (ctxUsed / ctxBudget) * 100) : 0;
              return (
                <div className="fixed inset-0 z-50 bg-black/45 backdrop-blur-xs flex items-center justify-center p-4" onClick={() => setSelectedRecord(null)}>
                  <div className="w-full max-w-md bg-white/95 dark:bg-[#16171b]/95 backdrop-blur-2xl border border-gray-200/80 dark:border-white/10 rounded-2xl shadow-2xl p-4 flex flex-col gap-3 animate-in zoom-in-95 duration-150" onClick={(e) => e.stopPropagation()}>
                    <div className="flex items-center justify-between pb-2 border-b border-gray-200/60 dark:border-white/10">
                      <div className="min-w-0">
                        <div className="text-xs font-bold text-gray-900 dark:text-zinc-100 truncate">
                          {selectedRecord.sessionTitle || selectedRecord.workspace}
                        </div>
                        <div className="text-[9.5px] text-gray-400 font-mono">{selectedRecord.id} · {selectedRecord.time}</div>
                      </div>
                      <button onClick={() => setSelectedRecord(null)} className="p-1 rounded-lg hover:bg-gray-100 dark:hover:bg-white/10 text-gray-400"><X className="w-4 h-4" /></button>
                    </div>
                    <div className="p-3 rounded-xl bg-gray-50/80 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5 flex flex-col gap-1.5 text-[10.5px]">
                      <div className="flex items-center justify-between"><span className="text-gray-400">Session ID</span><span className="font-mono text-gray-700 dark:text-zinc-300 truncate max-w-[220px]">{selectedRecord.sessionId}</span></div>
                      <div className="flex items-center justify-between">
                        <span className="text-gray-400">{tUi(lang, 'stats_detail_workspace')}</span>
                        <button onClick={() => { setSelectedProjectName(selectedRecord.workspace); setSelectedRecord(null); setActiveView('project'); }} className="font-mono text-indigo-600 dark:text-indigo-400 font-semibold hover:underline flex items-center gap-0.5">
                          <span>{selectedRecord.workspace}</span><ChevronRight className="w-3 h-3" />
                        </button>
                      </div>
                      <div className="flex items-center justify-between gap-2">
                        <span className="text-gray-400 shrink-0">{tUi(lang, 'stats_detail_model')}</span>
                        <span className="font-mono font-bold text-purple-600 dark:text-purple-400 truncate">
                          {selectedRecord.requestedModel}
                          {selectedRecord.physicalModel && selectedRecord.physicalModel !== selectedRecord.requestedModel && (
                            <span className="text-[9px] font-normal text-gray-400 dark:text-zinc-500 ml-1.5">({selectedRecord.physicalModel})</span>
                          )}
                        </span>
                      </div>
                    </div>
                    {hasTiming && (
                      <div className="p-3 rounded-xl bg-gray-50/80 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5 flex flex-col gap-2 text-[10px]">
                        <div className="grid grid-cols-4 gap-2 font-mono text-center">
                          <div className="p-1.5 rounded-lg bg-white/70 dark:bg-white/[0.04]">
                            <div className="text-[8.5px] text-gray-400">TTFT</div>
                            <div className="font-bold text-purple-600 dark:text-purple-400">
                              {selectedRecord.ttftMs > 0 ? `${(selectedRecord.ttftMs / 1000).toFixed(2)}s` : '-'}
                            </div>
                          </div>
                          <div className="p-1.5 rounded-lg bg-white/70 dark:bg-white/[0.04]">
                            <div className="text-[8.5px] text-gray-400">{tUi(lang, 'stats_detail_streaming')}</div>
                            <div className="font-bold text-emerald-600 dark:text-emerald-400">
                              {(selectedRecord.streamingDurationMs ?? 0) > 0 ? `${((selectedRecord.streamingDurationMs ?? 0) / 1000).toFixed(2)}s` : '-'}
                            </div>
                          </div>
                          <div className="p-1.5 rounded-lg bg-white/70 dark:bg-white/[0.04]">
                            <div className="text-[8.5px] text-gray-400">{tUi(lang, 'stats_detail_total_dur')}</div>
                            <div className="font-bold text-gray-800 dark:text-zinc-200">
                              {selectedRecord.durationMs > 0 ? `${(selectedRecord.durationMs / 1000).toFixed(2)}s` : '-'}
                            </div>
                          </div>
                          <div className="p-1.5 rounded-lg bg-white/70 dark:bg-white/[0.04]">
                            <div className="text-[8.5px] text-gray-400">{tUi(lang, 'stats_gen_speed')}</div>
                            <div className="font-bold text-sky-600 dark:text-sky-400">
                              {selectedRecord.tokensPerSecond > 0 ? `${selectedRecord.tokensPerSecond.toFixed(1)} t/s` : '-'}
                            </div>
                          </div>
                        </div>
                        {ctxBudget > 0 && (
                          <div className="flex flex-col gap-1 pt-1 border-t border-gray-200/60 dark:border-white/[0.06]">
                            <div className="flex items-center justify-between text-[9.5px] font-mono">
                              <span className="text-gray-400">{tUi(lang, 'stats_detail_context_window')}</span>
                              <span className="text-gray-700 dark:text-zinc-300 font-semibold">
                                {fmtTok(ctxUsed)} / {fmtTok(ctxBudget)} ({ctxPct.toFixed(1)}%)
                              </span>
                            </div>
                            <div className="w-full h-1.5 rounded-full bg-gray-200 dark:bg-white/[0.08] overflow-hidden">
                              <div
                                style={{ width: `${Math.max(2, ctxPct)}%` }}
                                className={`h-full rounded-full ${
                                  ctxPct > 80 ? 'bg-rose-500' : ctxPct > 60 ? 'bg-amber-400' : 'bg-indigo-500'
                                }`}
                              />
                            </div>
                          </div>
                        )}
                      </div>
                    )}
                    <div className="p-3 rounded-xl bg-gray-50/80 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5 flex flex-col gap-2 text-[10.5px]">
                      <div className="flex items-center justify-between"><span className="font-semibold text-gray-700 dark:text-zinc-300">{tUi(lang, 'stats_detail_token_breakdown')}</span><span className="font-mono text-gray-400">{totalTok.toLocaleString()}</span></div>
                      <div className="w-full h-2 rounded-full bg-gray-200 dark:bg-white/[0.08] overflow-hidden flex">
                        <div style={{ width: `${pct(selectedRecord.cachedTokens)}%` }} className="h-full bg-emerald-500" />
                        <div style={{ width: `${pct(selectedRecord.inputTokens)}%` }} className="h-full bg-blue-500" />
                        <div style={{ width: `${pct(thinkTok)}%` }} className="h-full bg-purple-500" />
                        <div style={{ width: `${pct(visibleOutTok)}%` }} className="h-full bg-cyan-400" />
                      </div>
                      <div className="grid grid-cols-2 gap-x-4 gap-y-1 font-mono text-[10px]">
                        <div className="flex items-center justify-between"><span className="text-emerald-600 dark:text-emerald-400">● {tUi(lang, 'stats_detail_cache_read')}</span><span className="font-bold">{selectedRecord.cachedTokens.toLocaleString()}</span></div>
                        <div className="flex items-center justify-between"><span className="text-blue-600 dark:text-blue-400">● {tUi(lang, 'stats_detail_direct_input')}</span><span>{selectedRecord.inputTokens.toLocaleString()}</span></div>
                        <div className="flex items-center justify-between"><span className="text-purple-600 dark:text-purple-400">● {tUi(lang, 'stats_detail_thinking_tokens')}</span><span>{thinkTok.toLocaleString()}</span></div>
                        <div className="flex items-center justify-between"><span className="text-cyan-600 dark:text-cyan-400 font-semibold">● {tUi(lang, 'stats_detail_output_tokens')}</span><span className="font-bold">{visibleOutTok.toLocaleString()}</span></div>
                      </div>
                    </div>
                  </div>
                </div>
              );
            })()}
          </div>
        );
      })()}
    </div>
  );
};
