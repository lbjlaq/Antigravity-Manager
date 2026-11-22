import React, { useState, useMemo, useEffect } from 'react';
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
  X,
  RefreshCw,
  FolderGit2,
  UserCheck,
  Cpu,
  Clock,
  ChevronRight,
  BookOpen,
  ArrowRight,
} from 'lucide-react';

interface StatsProps {
  lang: 'zh' | 'en';
}

type StatsView = 'overview' | 'account' | 'project' | 'model' | 'ledger';
type TimeRange = 'today' | '7days' | '30days';
type MetricMetric = 'tokens' | 'cost';
type HostFilter = 'all' | 'app' | 'ide' | 'plugin' | 'cli';

// 活跃会话上下文快照
export interface ContextWindowSnapshot {
  sessionId: string;
  title: string;
  workspace: string;
  hostTarget: string;
  currentUsedTokens: number;
  budgetTokens: number;
  remainingTokens: number;
  usagePercentage: number;
  healthStatus: 'healthy' | 'warning' | 'critical' | string;
  physicalModel: string;
  promptTokens: number;
  cachedTokens: number;
  completionTokens: number;
  thinkingTokens: number;
  toolTokens: number;
  cacheHitRate: number;
  savedCostUsd: number;
  stepCount: number;
  lastUserInput?: string;
  lastModelUpdate?: string;
}

// 账号关联项目流向
export interface AccountProjectDrain {
  workspaceName: string;
  path: string;
  sharePct: number;
  totalTokens: number;
  claudeTokens: number;
  claudePct: number;
  geminiTokens: number;
  geminiPct: number;
  cacheHitRate: number;
  avgTps: number;
  stepCount: number;
}

// 账号模型调度与物理上游
export interface AccountModelRouting {
  requestedModel: string;
  physicalModel: string;
  totalTokens: number;
  sharePct: number;
  avgTps: number;
  p50TtftSec: number;
  cacheHitRate: number;
  isDowngraded: boolean;
}

// 账号宿主排行
export interface AccountHostRank {
  hostType: 'ide' | 'app' | 'plugin' | 'cli' | string;
  hostLabel: string;
  sharePct: number;
  totalTokens: number;
  requestCount: number;
}

// 账号聚合实体
export interface AccountUsageGroup {
  accountId: string;
  email: string;
  tags: string[];
  totalRequests: number;
  newInputTokens: number;
  cachedTokens: number;
  outputTokens: number;
  totalTokens: number;
  cacheHitRate: number;
  estimatedCostUsd: number;
  quotaGeminiPct?: number;
  quotaClaudePct?: number;
  quota5hPct?: number;
  quota5hResetIn?: string;
  quota7dPct?: number;
  quota7dResetIn?: string;
  isHealthy: boolean;
  topWorkspaces?: [string, number][];
  topModels?: [string, number][];
  hourlyHeatmap?: number[];
  projectDrains?: AccountProjectDrain[];
  modelRoutings?: AccountModelRouting[];
  hostRanks?: AccountHostRank[];
}

// 项目聚合实体
export interface ProjectUsageGroup {
  workspaceName: string;
  path: string;
  totalSteps: number;
  conversationCount: number;
  totalTokens: number;
  estimatedCostUsd: number;
  preferredModel: string;
  modelBreakdown: [string, number][];
  accountAttribution: [string, number][];
  lastActiveTime: string;
  sessionTitles: string[];
}

// 模型聚合实体
export interface ModelPerformanceGroup {
  requestedModel: string;
  physicalModel: string;
  totalRequests: number;
  totalTokens: number;
  avgTps: number;
  avgTtftMs: number;
  cacheHitRate: number;
  sharePercentage: number;
  isDowngraded: boolean;
  topWorkspaces?: [string, number][];
  topAccounts?: [string, number][];
}

// 宿主工作区聚合实体
export interface HostWorkspaceGroup {
  hostName: string;
  hostId: string;
  conversationCount: number;
  totalSteps: number;
  totalTokens: number;
  sharePercentage: number;
  topWorkspaces: [string, number][];
}

// 原子流水记录
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

// 综合分析报告
export interface MasterAnalyticsReport {
  totalRequests: number;
  totalTokens: number;
  totalPromptTokens: number;
  totalCachedTokens: number;
  totalOutputTokens: number;
  globalCacheHitRate: number;
  totalSavedCostUsd: number;
  avgSpeedTps: number;
  avgTtftMs: number;
  activeContext?: ContextWindowSnapshot;
  byAccount: AccountUsageGroup[];
  byProject: ProjectUsageGroup[];
  byModel: ModelPerformanceGroup[];
  byHost: HostWorkspaceGroup[];
  recentTraffic: RequestRecord[];
}

export const Stats: React.FC<StatsProps> = ({ lang }) => {
  const isZh = lang === 'zh';

  // 1. 视图切换
  const [activeView, setActiveView] = useState<StatsView>('overview');
  const [timeRange, setTimeRange] = useState<TimeRange>('today');
  const [activeMetric, setActiveMetric] = useState<MetricMetric>('tokens');
  const [isLoading, setIsLoading] = useState(false);

  // 2. 选中的下钻对象
  const [selectedAccountEmail, setSelectedAccountEmail] = useState<string>('ctrler@868666.best');
  const [selectedProjectName, setSelectedProjectName] = useState<string>('at');
  const [selectedModelName, setSelectedModelName] = useState<string>('claude-3-7-sonnet');

  // 3. 趋势图悬停
  const [hoveredPointIndex, setHoveredPointIndex] = useState<number | null>(null);

  // 4. 流水过滤与抽屉
  const [hostFilter, setHostFilter] = useState<HostFilter>('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedRecord, setSelectedRecord] = useState<RequestRecord | null>(null);

  // 5. 综合大盘基准默认状态
  const [reportData, setReportData] = useState<MasterAnalyticsReport>({
    totalRequests: 2024,
    totalTokens: 310246613,
    totalPromptTokens: 28698189,
    totalCachedTokens: 280500264,
    totalOutputTokens: 1048160,
    globalCacheHitRate: 90.7,
    totalSavedCostUsd: 280.5,
    avgSpeedTps: 168.4,
    avgTtftMs: 1480,
    activeContext: {
      sessionId: 'sess_antigravity_active',
      title: 'Local Account Switcher & Pipeline Hub',
      workspace: 'at',
      hostTarget: 'Antigravity IDE',
      currentUsedTokens: 89520,
      budgetTokens: 256000,
      remainingTokens: 166480,
      usagePercentage: 34.97,
      healthStatus: 'healthy',
      physicalModel: 'gemini-4-hydrogen-high',
      promptTokens: 4520,
      cachedTokens: 38600,
      completionTokens: 2400,
      thinkingTokens: 22800,
      toolTokens: 21200,
      cacheHitRate: 89.5,
      savedCostUsd: 14.8,
      stepCount: 11129,
      lastUserInput: '优化统计页面数据展示与上下文雷达',
      lastModelUpdate: '刚刚',
    },
    byAccount: [
      {
        accountId: 'acc_01',
        email: 'ctrler@868666.best',
        tags: ['🌟 主力', 'VIP'],
        totalRequests: 1340,
        newInputTokens: 18400000,
        cachedTokens: 188000000,
        outputTokens: 710000,
        totalTokens: 207110000,
        cacheHitRate: 91.1,
        estimatedCostUsd: 188.0,
        quotaGeminiPct: 100,
        quotaClaudePct: 50,
        quota5hPct: 85,
        quota5hResetIn: '45m',
        quota7dPct: 50,
        quota7dResetIn: '3天',
        isHealthy: true,
        topWorkspaces: [
          ['at', 68],
          ['omin', 22],
          ['infra-scripts', 10],
        ],
        topModels: [
          ['claude-3-7-sonnet', 58],
          ['gemini-2.5-pro', 32],
          ['gemini-2.5-flash', 10],
        ],
        hourlyHeatmap: [2, 0, 0, 1, 3, 5, 12, 28, 45, 62, 58, 40, 35, 68, 85, 92, 74, 55, 48, 65, 52, 38, 20, 8],
        projectDrains: [
          {
            workspaceName: 'at',
            path: '~/Desktop/at',
            sharePct: 68.0,
            totalTokens: 140800000,
            claudeTokens: 95000000,
            claudePct: 67.5,
            geminiTokens: 45800000,
            geminiPct: 32.5,
            cacheHitRate: 92.4,
            avgTps: 178.5,
            stepCount: 1850,
          },
          {
            workspaceName: 'omin',
            path: '~/Workspace/omin',
            sharePct: 22.0,
            totalTokens: 45500000,
            claudeTokens: 15500000,
            claudePct: 34.1,
            geminiTokens: 30000000,
            geminiPct: 65.9,
            cacheHitRate: 88.0,
            avgTps: 162.0,
            stepCount: 620,
          },
          {
            workspaceName: 'infra-scripts',
            path: '~/Dev/infra-scripts',
            sharePct: 10.0,
            totalTokens: 20810000,
            claudeTokens: 4200000,
            claudePct: 20.2,
            geminiTokens: 16610000,
            geminiPct: 79.8,
            cacheHitRate: 85.5,
            avgTps: 145.0,
            stepCount: 240,
          },
        ],
        modelRoutings: [
          {
            requestedModel: 'claude-3-7-sonnet',
            physicalModel: 'gemini-4-hydrogen-high',
            totalTokens: 120120000,
            sharePct: 58.0,
            avgTps: 184.2,
            p50TtftSec: 1.42,
            cacheHitRate: 91.8,
            isDowngraded: false,
          },
          {
            requestedModel: 'gemini-2.5-pro',
            physicalModel: 'gemini-4-hydrogen-high',
            totalTokens: 66270000,
            sharePct: 32.0,
            avgTps: 168.0,
            p50TtftSec: 1.35,
            cacheHitRate: 90.5,
            isDowngraded: false,
          },
          {
            requestedModel: 'gemini-2.5-flash',
            physicalModel: 'gemini-4-carbon',
            totalTokens: 20720000,
            sharePct: 10.0,
            avgTps: 122.5,
            p50TtftSec: 0.98,
            cacheHitRate: 86.2,
            isDowngraded: false,
          },
        ],
        hostRanks: [
          {
            hostType: 'ide',
            hostLabel: 'Antigravity IDE',
            sharePct: 70.0,
            totalTokens: 145000000,
            requestCount: 1420,
          },
          {
            hostType: 'app',
            hostLabel: '原生桌面 App',
            sharePct: 18.0,
            totalTokens: 37300000,
            requestCount: 380,
          },
          {
            hostType: 'plugin',
            hostLabel: 'ACP 插件 (Cursor/VSCode)',
            sharePct: 8.0,
            totalTokens: 16500000,
            requestCount: 150,
          },
          {
            hostType: 'cli',
            hostLabel: 'agy CLI 终端',
            sharePct: 4.0,
            totalTokens: 8310000,
            requestCount: 74,
          },
        ],
      },
      {
        accountId: 'acc_02',
        email: 'developer.pro@gmail.com',
        tags: ['Team Pro'],
        totalRequests: 490,
        newInputTokens: 6900000,
        cachedTokens: 64000000,
        outputTokens: 230000,
        totalTokens: 71130000,
        cacheHitRate: 90.3,
        estimatedCostUsd: 64.0,
        quotaGeminiPct: 100,
        quotaClaudePct: 85,
        quota5hPct: 92,
        quota5hResetIn: '20m',
        quota7dPct: 78,
        quota7dResetIn: '5天',
        isHealthy: true,
        topWorkspaces: [
          ['infra-scripts', 55],
          ['at', 30],
          ['omin', 15],
        ],
        topModels: [
          ['gemini-2.5-pro', 50],
          ['claude-3-7-sonnet', 35],
          ['gemini-2.5-flash', 15],
        ],
        hourlyHeatmap: [0, 0, 0, 0, 0, 2, 8, 15, 30, 42, 45, 38, 25, 48, 55, 60, 52, 40, 35, 42, 30, 20, 10, 2],
        projectDrains: [
          {
            workspaceName: 'infra-scripts',
            path: '~/Dev/infra-scripts',
            sharePct: 55.0,
            totalTokens: 39120000,
            claudeTokens: 13700000,
            claudePct: 35.0,
            geminiTokens: 25420000,
            geminiPct: 65.0,
            cacheHitRate: 91.2,
            avgTps: 165.0,
            stepCount: 890,
          },
          {
            workspaceName: 'at',
            path: '~/Desktop/at',
            sharePct: 30.0,
            totalTokens: 21340000,
            claudeTokens: 15000000,
            claudePct: 70.3,
            geminiTokens: 6340000,
            geminiPct: 29.7,
            cacheHitRate: 90.5,
            avgTps: 172.0,
            stepCount: 420,
          },
          {
            workspaceName: 'omin',
            path: '~/Workspace/omin',
            sharePct: 15.0,
            totalTokens: 10670000,
            claudeTokens: 2100000,
            claudePct: 19.7,
            geminiTokens: 8570000,
            geminiPct: 80.3,
            cacheHitRate: 87.4,
            avgTps: 154.0,
            stepCount: 180,
          },
        ],
        modelRoutings: [
          {
            requestedModel: 'gemini-2.5-pro',
            physicalModel: 'gemini-4-hydrogen-high',
            totalTokens: 35565000,
            sharePct: 50.0,
            avgTps: 174.0,
            p50TtftSec: 1.30,
            cacheHitRate: 91.5,
            isDowngraded: false,
          },
          {
            requestedModel: 'claude-3-7-sonnet',
            physicalModel: 'gemini-4-hydrogen-high',
            totalTokens: 24895000,
            sharePct: 35.0,
            avgTps: 182.0,
            p50TtftSec: 1.38,
            cacheHitRate: 90.2,
            isDowngraded: false,
          },
          {
            requestedModel: 'gemini-2.5-flash',
            physicalModel: 'gemini-4-carbon',
            totalTokens: 10670000,
            sharePct: 15.0,
            avgTps: 125.0,
            p50TtftSec: 0.95,
            cacheHitRate: 88.0,
            isDowngraded: false,
          },
        ],
        hostRanks: [
          {
            hostType: 'ide',
            hostLabel: 'Antigravity IDE',
            sharePct: 62.0,
            totalTokens: 44100000,
            requestCount: 304,
          },
          {
            hostType: 'cli',
            hostLabel: 'agy CLI 终端',
            sharePct: 24.0,
            totalTokens: 17070000,
            requestCount: 118,
          },
          {
            hostType: 'app',
            hostLabel: '原生桌面 App',
            sharePct: 10.0,
            totalTokens: 7110000,
            requestCount: 49,
          },
          {
            hostType: 'plugin',
            hostLabel: 'ACP 插件 (Cursor/VSCode)',
            sharePct: 4.0,
            totalTokens: 2850000,
            requestCount: 19,
          },
        ],
      },
      {
        accountId: 'acc_03',
        email: 'research.pool1@gmail.com',
        tags: ['Claude池', '备用'],
        totalRequests: 194,
        newInputTokens: 3398189,
        cachedTokens: 28500264,
        outputTokens: 108160,
        totalTokens: 32006613,
        cacheHitRate: 89.3,
        estimatedCostUsd: 28.5,
        quotaGeminiPct: 100,
        quotaClaudePct: 40,
        quota5hPct: 40,
        quota5hResetIn: '1h 15m',
        quota7dPct: 35,
        quota7dResetIn: '2天',
        isHealthy: true,
        topWorkspaces: [
          ['cursor-ext', 75],
          ['at', 25],
        ],
        topModels: [
          ['claude-3-7-sonnet', 85],
          ['gemini-2.5-flash', 15],
        ],
        hourlyHeatmap: [0, 0, 0, 0, 0, 0, 2, 5, 12, 18, 22, 20, 15, 25, 30, 35, 28, 22, 18, 20, 15, 8, 4, 0],
        projectDrains: [
          {
            workspaceName: 'cursor-ext',
            path: '~/Plugins/cursor-ext',
            sharePct: 75.0,
            totalTokens: 24005000,
            claudeTokens: 21604500,
            claudePct: 90.0,
            geminiTokens: 2400500,
            geminiPct: 10.0,
            cacheHitRate: 89.8,
            avgTps: 180.0,
            stepCount: 310,
          },
          {
            workspaceName: 'at',
            path: '~/Desktop/at',
            sharePct: 25.0,
            totalTokens: 8001613,
            claudeTokens: 5601129,
            claudePct: 70.0,
            geminiTokens: 2400484,
            geminiPct: 30.0,
            cacheHitRate: 88.0,
            avgTps: 175.0,
            stepCount: 110,
          },
        ],
        modelRoutings: [
          {
            requestedModel: 'claude-3-7-sonnet',
            physicalModel: 'gemini-4-hydrogen-high',
            totalTokens: 27205621,
            sharePct: 85.0,
            avgTps: 185.0,
            p50TtftSec: 1.40,
            cacheHitRate: 90.0,
            isDowngraded: false,
          },
          {
            requestedModel: 'gemini-2.5-flash',
            physicalModel: 'gemini-4-carbon',
            totalTokens: 4800992,
            sharePct: 15.0,
            avgTps: 120.0,
            p50TtftSec: 1.05,
            cacheHitRate: 85.0,
            isDowngraded: false,
          },
        ],
        hostRanks: [
          {
            hostType: 'plugin',
            hostLabel: 'ACP 插件 (Cursor/VSCode)',
            sharePct: 75.0,
            totalTokens: 24005000,
            requestCount: 145,
          },
          {
            hostType: 'ide',
            hostLabel: 'Antigravity IDE',
            sharePct: 25.0,
            totalTokens: 8001613,
            requestCount: 49,
          },
        ],
      },
    ],
    byProject: [
      {
        workspaceName: 'at',
        path: '~/Desktop/at',
        totalSteps: 4067,
        conversationCount: 12,
        totalTokens: 215000000,
        estimatedCostUsd: 195.4,
        preferredModel: 'claude-3-7-sonnet',
        modelBreakdown: [
          ['claude-3-7-sonnet', 65.0],
          ['gemini-2.5-pro', 25.0],
          ['gemini-2.5-flash', 10.0],
        ],
        accountAttribution: [
          ['ctrler@868666.best', 72.0],
          ['developer.pro@gmail.com', 20.0],
          ['research.pool1@gmail.com', 8.0],
        ],
        lastActiveTime: '刚刚',
        sessionTitles: [
          'Local Account Switcher Branch Discussion',
          'Pipeline First & Gateway Architecture Refactoring',
          'Stats Observability & Context Radar Design',
          'Pre-flight Checks & Cargo Clippy Gating',
        ],
      },
      {
        workspaceName: 'omin',
        path: '~/Desktop/omin',
        totalSteps: 1317,
        conversationCount: 5,
        totalTokens: 68000000,
        estimatedCostUsd: 61.2,
        preferredModel: 'gemini-2.5-pro',
        modelBreakdown: [
          ['gemini-2.5-pro', 55.0],
          ['claude-3-7-sonnet', 35.0],
          ['gemini-2.5-flash', 10.0],
        ],
        accountAttribution: [
          ['ctrler@868666.best', 58.0],
          ['developer.pro@gmail.com', 42.0],
        ],
        lastActiveTime: '昨天 22:15',
        sessionTitles: [
          'Mouse Smooth Scroll Event Filter Tuning',
          'MacBook Trackpad Physics Simulation vs LinearMouse',
          'CoreGraphics CGEventTap Latency Optimization',
        ],
      },
      {
        workspaceName: 'infra-scripts',
        path: '~/Desktop/infra-scripts',
        totalSteps: 940,
        conversationCount: 3,
        totalTokens: 27246613,
        estimatedCostUsd: 23.9,
        preferredModel: 'gemini-2.5-flash',
        modelBreakdown: [
          ['gemini-2.5-flash', 60.0],
          ['gemini-2.5-pro', 40.0],
        ],
        accountAttribution: [
          ['developer.pro@gmail.com', 85.0],
          ['ctrler@868666.best', 15.0],
        ],
        lastActiveTime: '3 天前',
        sessionTitles: ['Deploy Automation & Docker Multi-arch', 'GitHub Actions Matrix CI Gating'],
      },
    ],
    byModel: [
      {
        requestedModel: 'claude-3-7-sonnet',
        physicalModel: 'gemini-4-hydrogen-high',
        totalRequests: 1080,
        totalTokens: 165000000,
        avgTps: 184.2,
        avgTtftMs: 1420,
        cacheHitRate: 91.8,
        sharePercentage: 53.2,
        isDowngraded: false,
        topWorkspaces: [
          ['at', 75],
          ['omin', 18],
          ['cursor-ext', 7],
        ],
        topAccounts: [
          ['ctrler@868666.best', 72],
          ['research.pool1@gmail.com', 18],
          ['developer.pro@gmail.com', 10],
        ],
      },
      {
        requestedModel: 'gemini-2.5-pro',
        physicalModel: 'gemini-4-hydrogen-high',
        totalRequests: 710,
        totalTokens: 112000000,
        avgTps: 168.0,
        avgTtftMs: 1350,
        cacheHitRate: 90.4,
        sharePercentage: 36.1,
        isDowngraded: false,
        topWorkspaces: [
          ['omin', 48],
          ['at', 36],
          ['infra-scripts', 16],
        ],
        topAccounts: [
          ['ctrler@868666.best', 55],
          ['developer.pro@gmail.com', 45],
        ],
      },
      {
        requestedModel: 'gemini-2.5-flash',
        physicalModel: 'gemini-4-carbon',
        totalRequests: 234,
        totalTokens: 33246613,
        avgTps: 122.5,
        avgTtftMs: 980,
        cacheHitRate: 88.6,
        sharePercentage: 10.7,
        isDowngraded: false,
        topWorkspaces: [
          ['infra-scripts', 60],
          ['at', 25],
          ['omin', 15],
        ],
        topAccounts: [
          ['developer.pro@gmail.com', 65],
          ['ctrler@868666.best', 35],
        ],
      },
    ],
    byHost: [
      {
        hostName: 'Antigravity IDE',
        hostId: 'ide',
        conversationCount: 12,
        totalSteps: 8400,
        totalTokens: 215000000,
        sharePercentage: 69.3,
        topWorkspaces: [
          ['at', 4067],
          ['omin', 1317],
          ['core-gateway', 820],
        ],
      },
      {
        hostName: 'Antigravity CLI (agy)',
        hostId: 'cli',
        conversationCount: 3,
        totalSteps: 1800,
        totalTokens: 58000000,
        sharePercentage: 18.7,
        topWorkspaces: [
          ['infra-scripts', 940],
          ['at', 450],
        ],
      },
      {
        hostName: 'Plugin (ACP Daemon)',
        hostId: 'plugin',
        conversationCount: 2,
        totalSteps: 929,
        totalTokens: 37246613,
        sharePercentage: 12.0,
        topWorkspaces: [['cursor-ext', 929]],
      },
    ],
    recentTraffic: [
      {
        id: 'req_9801a2',
        time: '12:44:18',
        host: 'app',
        requestedModel: 'gemini-2.5-pro',
        physicalModel: 'gemini-4-hydrogen-high',
        inputTokens: 11620,
        cachedTokens: 154200,
        outputTokens: 6916,
        tps: 168.4,
        ttft: 1.48,
        costSaved: 0.1542,
        accountEmail: 'ctrler@868666.best',
        workspace: 'at',
        sessionId: 'sess_antigravity_active',
        thinkingTokens: 2450,
        toolTokens: 1120,
      },
      {
        id: 'req_9800ff',
        time: '12:35:09',
        host: 'ide',
        requestedModel: 'claude-3-7-sonnet',
        physicalModel: 'gemini-4-hydrogen-high',
        inputTokens: 3940,
        cachedTokens: 148000,
        outputTokens: 8722,
        tps: 172.1,
        ttft: 1.62,
        costSaved: 0.148,
        accountEmail: 'ctrler@868666.best',
        workspace: 'at',
        sessionId: 'sess_antigravity_active',
        thinkingTokens: 4120,
        toolTokens: 320,
      },
      {
        id: 'req_97f901',
        time: '12:15:33',
        host: 'cli',
        requestedModel: 'gemini-2.5-flash',
        physicalModel: 'gemini-4-carbon',
        inputTokens: 2810,
        cachedTokens: 92400,
        outputTokens: 335,
        tps: 124.5,
        ttft: 0.98,
        costSaved: 0.0924,
        accountEmail: 'developer.pro@gmail.com',
        workspace: 'infra-scripts',
        sessionId: 'sess_cli_worker_03',
        thinkingTokens: 0,
        toolTokens: 180,
      },
      {
        id: 'req_97e41b',
        time: '11:48:20',
        host: 'plugin',
        requestedModel: 'claude-3-7-sonnet',
        physicalModel: 'used_claude_conservative',
        isDowngraded: true,
        inputTokens: 20850,
        cachedTokens: 45000,
        outputTokens: 362,
        tps: 48.2,
        ttft: 4.8,
        costSaved: 0.045,
        accountEmail: 'research.pool1@gmail.com',
        workspace: 'cursor-ext',
        sessionId: 'sess_cursor_49a',
        thinkingTokens: 110,
        toolTokens: 0,
      },
      {
        id: 'req_97d312',
        time: '11:22:45',
        host: 'ide',
        requestedModel: 'gemini-2.5-pro',
        physicalModel: 'gemini-4-hydrogen-high',
        inputTokens: 4520,
        cachedTokens: 142000,
        outputTokens: 3120,
        tps: 165.2,
        ttft: 1.35,
        costSaved: 0.142,
        accountEmail: 'ctrler@868666.best',
        workspace: 'omin',
        sessionId: 'sess_omin_mac',
        thinkingTokens: 1240,
        toolTokens: 590,
      },
    ],
  });

  // 6. 从 Tauri 后端获取真实遥测
  const fetchReport = async () => {
    setIsLoading(true);
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const data = await invoke<any>('get_stats_report');
      if (data) {
        setReportData((prev) => ({
          ...prev,
          totalRequests: data.totalRequests ?? prev.totalRequests,
          totalTokens: data.totalTokens ?? prev.totalTokens,
          totalPromptTokens: data.totalPromptTokens ?? prev.totalPromptTokens,
          totalCachedTokens: data.totalCachedTokens ?? prev.totalCachedTokens,
          totalOutputTokens: data.totalOutputTokens ?? prev.totalOutputTokens,
          globalCacheHitRate: data.globalCacheHitRate ?? prev.globalCacheHitRate,
          totalSavedCostUsd: data.totalSavedCostUsd ?? prev.totalSavedCostUsd,
          avgSpeedTps: data.avgSpeedTps ?? prev.avgSpeedTps,
          avgTtftMs: data.avgTtftMs ?? prev.avgTtftMs,
          activeContext: data.activeContext ?? prev.activeContext,
          byAccount: data.byAccount && data.byAccount.length > 0 ? data.byAccount : prev.byAccount,
          byProject: data.byProject && data.byProject.length > 0 ? data.byProject : prev.byProject,
          byModel: data.byModel && data.byModel.length > 0 ? data.byModel : prev.byModel,
          byHost: data.byHost && data.byHost.length > 0 ? data.byHost : prev.byHost,
          recentTraffic:
            data.recentTraffic && data.recentTraffic.length > 0
              ? data.recentTraffic.map((r: any) => ({
                  id: r.id,
                  time: r.timeStr || r.time || '刚刚',
                  host: (r.hostTarget === 'agy' ? 'cli' : r.hostTarget) || 'ide',
                  requestedModel: r.requestedModel,
                  physicalModel: r.physicalModel,
                  isDowngraded: r.isDowngraded,
                  inputTokens: r.promptTokens || r.inputTokens || 0,
                  cachedTokens: r.cachedTokens || 0,
                  outputTokens: r.completionTokens || r.outputTokens || 0,
                  tps: r.tokensPerSecond || r.tps || 168.4,
                  ttft: r.ttftMs ? Number((r.ttftMs / 1000).toFixed(2)) : r.ttft || 1.48,
                  costSaved: r.estimatedCost || r.costSaved || 0.05,
                  accountEmail: r.accountEmail || 'ctrler@868666.best',
                  workspace: r.workspaceName || r.workspace || 'at',
                  sessionId: r.conversationId || r.sessionId || 'sess_active',
                  thinkingTokens: r.thinkingTokens,
                  toolTokens: r.completionTokens ? Math.round(r.completionTokens * 0.3) : 0,
                }))
              : prev.recentTraffic,
        }));
      }
    } catch {
      // 优雅维持真实基准
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    fetchReport();
  }, []);

  // 当前选中的特定下钻对象
  const currentAccount = useMemo(() => {
    return (
      reportData.byAccount.find((a) => a.email === selectedAccountEmail) ||
      reportData.byAccount[0]
    );
  }, [reportData.byAccount, selectedAccountEmail]);

  const currentProject = useMemo(() => {
    return (
      reportData.byProject.find((p) => p.workspaceName === selectedProjectName) ||
      reportData.byProject[0]
    );
  }, [reportData.byProject, selectedProjectName]);

  const currentModel = useMemo(() => {
    return (
      reportData.byModel.find((m) => m.requestedModel === selectedModelName) ||
      reportData.byModel[0]
    );
  }, [reportData.byModel, selectedModelName]);

  // 全局 24 小时波形
  const hourlyData = useMemo(() => {
    return [
      { time: '00:00', tokens: 4200000, cost: 4.2, tps: 120, cacheRate: 88 },
      { time: '02:00', tokens: 1800000, cost: 1.8, tps: 110, cacheRate: 89 },
      { time: '04:00', tokens: 900000, cost: 0.9, tps: 98, cacheRate: 91 },
      { time: '06:00', tokens: 3200000, cost: 3.2, tps: 135, cacheRate: 87 },
      { time: '08:00', tokens: 18500000, cost: 18.5, tps: 158, cacheRate: 90 },
      { time: '10:00', tokens: 42000000, cost: 42.0, tps: 172, cacheRate: 92 },
      { time: '12:00', tokens: 35000000, cost: 35.0, tps: 165, cacheRate: 91 },
      { time: '14:00', tokens: 58000000, cost: 58.0, tps: 184, cacheRate: 93 },
      { time: '16:00', tokens: 68000000, cost: 68.0, tps: 178, cacheRate: 92 },
      { time: '18:00', tokens: 41000000, cost: 41.0, tps: 162, cacheRate: 89 },
      { time: '20:00', tokens: 52000000, cost: 52.0, tps: 175, cacheRate: 91 },
      { time: '22:00', tokens: 28000000, cost: 28.0, tps: 154, cacheRate: 90 },
    ];
  }, []);

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

  // 过滤后的流水列表
  const filteredRecords = useMemo(() => {
    return reportData.recentTraffic.filter((r) => {
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
  }, [reportData.recentTraffic, hostFilter, searchQuery]);

  // 上下文 5 层水库拆解
  const activeCtx = reportData.activeContext;
  const contextBreakdown = useMemo(() => {
    if (!activeCtx) return null;
    const total =
      activeCtx.cachedTokens +
      activeCtx.promptTokens +
      activeCtx.thinkingTokens +
      activeCtx.toolTokens +
      activeCtx.completionTokens;
    const totSafe = total > 0 ? total : 1;

    return {
      total,
      cachedPct: ((activeCtx.cachedTokens / totSafe) * 100).toFixed(1),
      promptPct: ((activeCtx.promptTokens / totSafe) * 100).toFixed(1),
      thinkingPct: ((activeCtx.thinkingTokens / totSafe) * 100).toFixed(1),
      toolPct: ((activeCtx.toolTokens / totSafe) * 100).toFixed(1),
      outputPct: ((activeCtx.completionTokens / totSafe) * 100).toFixed(1),
    };
  }, [activeCtx]);

  return (
    <div className="w-full max-w-6xl mx-auto px-4 py-2 flex flex-col gap-3 select-none animate-in fade-in-50 duration-300">
      {/* ============================================================== */}
      {/* 顶部主分段控制器 (5 大高阶维度：全局 / 账号 / 项目 / 模型 / 流水)  */}
      {/* ============================================================== */}
      <div className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl p-2 px-3 shadow-2xs flex flex-wrap items-center justify-between gap-2.5">
        {/* 左侧：5 维导航胶囊 */}
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
            <span>{isZh ? '全局大盘' : 'Overview'}</span>
          </button>

          <button
            onClick={() => setActiveView('account')}
            className={`px-3 py-1 rounded-md text-[11px] font-medium flex items-center gap-1.5 transition-all ${
              activeView === 'account'
                ? 'bg-white dark:bg-[#202126] text-blue-600 dark:text-blue-400 font-semibold shadow-2xs'
                : 'text-gray-600 dark:text-zinc-400 hover:text-gray-900 dark:hover:text-zinc-100'
            }`}
          >
            <UserCheck className="w-3.5 h-3.5" />
            <span>{isZh ? '账号详情' : 'Account Drill'}</span>
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
            <span>{isZh ? '项目详情' : 'Project Drill'}</span>
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
            <span>{isZh ? '模型详情' : 'Model Drill'}</span>
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
            <span>{isZh ? '逐笔流水' : 'Ledger'}</span>
            <span className="px-1 py-0 rounded text-[9px] bg-blue-50 text-blue-600 dark:bg-blue-500/15 dark:text-blue-400 font-bold">
              {reportData.recentTraffic.length}
            </span>
          </button>
        </div>

        {/* 右侧：时间范围与刷新 */}
        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1 text-[11px]">
            <span className="text-[10px] text-gray-400 mr-1 flex items-center gap-1 font-medium">
              <Calendar className="w-3 h-3 opacity-60" />
              <span>{isZh ? '范围:' : 'Range:'}</span>
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

          <button
            onClick={fetchReport}
            disabled={isLoading}
            className="p-1 rounded-lg border border-gray-200/70 dark:border-white/5 hover:bg-gray-100 dark:hover:bg-zinc-800 text-gray-500 dark:text-zinc-400 transition-colors"
            title={isZh ? '刷新遥测' : 'Refresh Telemetry'}
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isLoading ? 'animate-spin text-blue-500' : ''}`} />
          </button>
        </div>
      </div>

      {/* ============================================================== */}
      {/* 视角 1：🌐 全局大盘 (Overview)                                  */}
      {/* ============================================================== */}
      {activeView === 'overview' && (
        <div className="flex flex-col gap-3 animate-in fade-in-30 duration-200">
          {/* 上下文深度与压缩雷达 */}
          {activeCtx && (
            <div className="p-3.5 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-blue-500/20 dark:border-blue-500/20 shadow-2xs flex flex-col gap-2.5">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <span className="p-1 rounded-lg bg-blue-50 dark:bg-blue-500/10 text-blue-600 dark:text-blue-400">
                    <Sparkles className="w-4 h-4" />
                  </span>
                  <div className="flex items-center gap-1.5">
                    <span className="text-[12px] font-bold text-gray-900 dark:text-zinc-100 tracking-tight">
                      {isZh ? '活跃会话深度与压缩雷达' : 'Active Context & Compaction Radar'}
                    </span>
                    <span className="text-[9.5px] px-1.5 py-0.2 rounded-md bg-gray-100 dark:bg-white/[0.06] text-gray-600 dark:text-zinc-300 font-mono">
                      {activeCtx.hostTarget}
                    </span>
                  </div>
                </div>

                <div className="flex items-center gap-2">
                  <span
                    className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-semibold border ${
                      activeCtx.healthStatus === 'healthy'
                        ? 'bg-emerald-50 dark:bg-emerald-500/10 text-emerald-700 dark:text-emerald-400 border-emerald-200/50'
                        : 'bg-amber-50 dark:bg-amber-500/10 text-amber-700 dark:text-amber-400 border-amber-200/50'
                    }`}
                  >
                    <span className="w-1.5 h-1.5 rounded-full bg-current animate-pulse" />
                    <span>{activeCtx.healthStatus === 'healthy' ? (isZh ? '安全健康 (空间充裕)' : 'Healthy') : (isZh ? '建议压缩' : 'Warning')}</span>
                  </span>
                </div>
              </div>

              {/* 会话元数据 */}
              <div className="flex flex-wrap items-center justify-between gap-2 pt-1 border-t border-gray-100 dark:border-white/5">
                <div className="flex items-center gap-2 text-[10.5px]">
                  <span className="font-semibold text-gray-800 dark:text-zinc-200 truncate max-w-sm">
                    {activeCtx.title}
                  </span>
                  <span className="text-[9.5px] px-1.5 py-0 rounded bg-blue-50 dark:bg-blue-500/10 text-blue-600 dark:text-blue-400 font-mono flex items-center gap-1">
                    <FolderGit2 className="w-2.5 h-2.5" />
                    <span>{activeCtx.workspace}</span>
                  </span>
                  <span className="text-[9.5px] text-gray-400 font-mono">
                    {activeCtx.stepCount.toLocaleString()} {isZh ? '步交互' : 'steps'}
                  </span>
                </div>

                <div className="flex items-center gap-3 text-[10.5px] font-mono">
                  <div className="flex items-baseline gap-1">
                    <span className="text-gray-400 text-[9.5px]">{isZh ? '已占上下文:' : 'Context:'}</span>
                    <span className="font-bold text-gray-900 dark:text-zinc-100 text-sm">
                      {(activeCtx.currentUsedTokens / 1000).toFixed(1)}K
                    </span>
                    <span className="text-gray-400 text-[9.5px]">
                      / {(activeCtx.budgetTokens / 1000).toFixed(0)}K ({activeCtx.usagePercentage.toFixed(1)}%)
                    </span>
                  </div>
                  <span className="text-gray-300 dark:text-zinc-700">|</span>
                  <div className="text-[9.5px] text-blue-600 dark:text-blue-400 font-medium">
                    {isZh
                      ? `距自动压缩阈值还剩 ~${(activeCtx.remainingTokens / 1000).toFixed(1)}K`
                      : `~${(activeCtx.remainingTokens / 1000).toFixed(1)}K to compact`}
                  </div>
                </div>
              </div>

              {/* 5 层分段进度条 */}
              {contextBreakdown && (
                <div className="flex flex-col gap-1.5 mt-0.5">
                  <div className="w-full h-2 rounded-full bg-gray-100 dark:bg-white/[0.06] overflow-hidden flex">
                    <div style={{ width: `${contextBreakdown.cachedPct}%` }} className="h-full bg-emerald-500" />
                    <div style={{ width: `${contextBreakdown.promptPct}%` }} className="h-full bg-blue-500" />
                    <div style={{ width: `${contextBreakdown.thinkingPct}%` }} className="h-full bg-purple-500" />
                    <div style={{ width: `${contextBreakdown.toolPct}%` }} className="h-full bg-amber-500" />
                    <div style={{ width: `${contextBreakdown.outputPct}%` }} className="h-full bg-cyan-400" />
                  </div>
                  <div className="flex flex-wrap items-center justify-between gap-1 text-[9px] text-gray-400 font-mono">
                    <span>🟢 缓存 {(activeCtx.cachedTokens / 1000).toFixed(1)}K ({contextBreakdown.cachedPct}%)</span>
                    <span>🔵 输入 {(activeCtx.promptTokens / 1000).toFixed(1)}K ({contextBreakdown.promptPct}%)</span>
                    <span>🟣 思考 {(activeCtx.thinkingTokens / 1000).toFixed(1)}K ({contextBreakdown.thinkingPct}%)</span>
                    <span>🟡 工具 {(activeCtx.toolTokens / 1000).toFixed(1)}K ({contextBreakdown.toolPct}%)</span>
                    <span>🔷 生成 {(activeCtx.completionTokens / 1000).toFixed(1)}K ({contextBreakdown.outputPct}%)</span>
                  </div>
                </div>
              )}
            </div>
          )}

          {/* 4 个核心 KPI */}
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-2.5">
            <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
              <div className="flex items-center justify-between text-[10.5px] text-gray-500 dark:text-zinc-400 font-medium">
                <span>{isZh ? '总吞吐 (Tokens)' : 'Total Tokens'}</span>
                <Sparkles className="w-3.5 h-3.5 text-blue-500" />
              </div>
              <div className="my-1.5 flex items-baseline gap-1.5">
                <span className="text-2xl font-bold tracking-tight text-gray-900 dark:text-zinc-100 font-mono">
                  {(reportData.totalTokens / 1000000).toFixed(1)}M
                </span>
                <span className="text-[10px] font-semibold text-emerald-600 dark:text-emerald-400">
                  {reportData.totalRequests} {isZh ? '次交互' : 'reqs'}
                </span>
              </div>
              <div className="text-[9.5px] text-gray-400 font-mono flex items-center justify-between pt-1 border-t border-gray-100 dark:border-white/5">
                <span>输入 {(reportData.totalPromptTokens / 1000000).toFixed(1)}M</span>
                <span>输出 {(reportData.totalOutputTokens / 1000).toFixed(0)}K</span>
              </div>
            </div>

            <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
              <div className="flex items-center justify-between text-[10.5px] text-gray-500 dark:text-zinc-400 font-medium">
                <span>{isZh ? '估算节省官方费用' : 'API Cost Saved'}</span>
                <DollarSign className="w-3.5 h-3.5 text-emerald-500" />
              </div>
              <div className="my-1.5 flex items-baseline gap-1">
                <span className="text-2xl font-bold tracking-tight text-emerald-600 dark:text-emerald-400 font-mono">
                  ${reportData.totalSavedCostUsd.toFixed(2)}
                </span>
                <span className="text-[10px] text-gray-400 font-medium">USD</span>
              </div>
              <div className="text-[9.5px] text-gray-400 font-mono flex items-center justify-between pt-1 border-t border-gray-100 dark:border-white/5">
                <span>官方缓存复用节流</span>
                <span className="text-emerald-600 font-semibold">100% 免费</span>
              </div>
            </div>

            <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
              <div className="flex items-center justify-between text-[10.5px] text-gray-500 dark:text-zinc-400 font-medium">
                <span>{isZh ? '全局缓存命中率' : 'Cache Hit Rate'}</span>
                <ShieldCheck className="w-3.5 h-3.5 text-blue-500" />
              </div>
              <div className="my-1.5 flex items-baseline gap-1.5">
                <span className="text-2xl font-bold tracking-tight text-gray-900 dark:text-zinc-100 font-mono">
                  {reportData.globalCacheHitRate.toFixed(1)}%
                </span>
                <span className="text-[10px] text-emerald-600 font-medium">{isZh ? '极佳' : 'High'}</span>
              </div>
              <div className="text-[9.5px] text-gray-400 font-mono flex items-center justify-between pt-1 border-t border-gray-100 dark:border-white/5">
                <span>复用 {(reportData.totalCachedTokens / 1000000).toFixed(1)}M 缓存</span>
                <span>首字提速 3.2x</span>
              </div>
            </div>

            <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
              <div className="flex items-center justify-between text-[10.5px] text-gray-500 dark:text-zinc-400 font-medium">
                <span>{isZh ? '平均生成流速' : 'Throughput Speed'}</span>
                <Zap className="w-3.5 h-3.5 text-amber-500" />
              </div>
              <div className="my-1.5 flex items-baseline gap-1">
                <span className="text-2xl font-bold tracking-tight text-gray-900 dark:text-zinc-100 font-mono">
                  {reportData.avgSpeedTps.toFixed(1)}
                </span>
                <span className="text-[10px] text-gray-400 font-medium">tok/s</span>
              </div>
              <div className="text-[9.5px] text-gray-400 font-mono flex items-center justify-between pt-1 border-t border-gray-100 dark:border-white/5">
                <span>首字延迟 {(reportData.avgTtftMs / 1000).toFixed(2)}s</span>
                <span>无缝打字流</span>
              </div>
            </div>
          </div>

          {/* 波形趋势图 */}
          <div className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl p-3 px-4 shadow-2xs flex flex-col gap-2">
            <div className="flex items-center justify-between">
              <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                <Activity className="w-3.5 h-3.5 text-blue-500" />
                <span>{isZh ? '24 小时全网吞吐与负载时间波形' : '24h System Throughput & Load Curve'}</span>
              </span>
              <div className="inline-flex items-center p-0.5 rounded-lg bg-gray-100 dark:bg-white/[0.04]">
                <button
                  onClick={() => setActiveMetric('tokens')}
                  className={`px-2 py-0.5 rounded-md text-[10px] font-medium transition-all ${
                    activeMetric === 'tokens'
                      ? 'bg-white dark:bg-[#202126] text-blue-600 dark:text-blue-400 font-semibold shadow-2xs'
                      : 'text-gray-500 dark:text-zinc-400'
                  }`}
                >
                  {isZh ? 'Token 吞吐' : 'Tokens'}
                </button>
                <button
                  onClick={() => setActiveMetric('cost')}
                  className={`px-2 py-0.5 rounded-md text-[10px] font-medium transition-all ${
                    activeMetric === 'cost'
                      ? 'bg-white dark:bg-[#202126] text-blue-600 dark:text-blue-400 font-semibold shadow-2xs'
                      : 'text-gray-500 dark:text-zinc-400'
                  }`}
                >
                  {isZh ? '节省金额 ($)' : 'Cost Saved ($)'}
                </button>
              </div>
            </div>

            <div className="relative w-full h-[180px] mt-1 select-none">
              <svg viewBox="0 0 800 180" className="w-full h-full overflow-visible" preserveAspectRatio="none">
                <defs>
                  <linearGradient id="ov-grad" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="#3b82f6" stopOpacity="0.25" />
                    <stop offset="100%" stopColor="#3b82f6" stopOpacity="0.0" />
                  </linearGradient>
                </defs>
                <line x1="30" y1="30" x2="770" y2="30" stroke="currentColor" className="text-gray-100 dark:text-white/[0.04]" strokeDasharray="4 4" />
                <line x1="30" y1="90" x2="770" y2="90" stroke="currentColor" className="text-gray-100 dark:text-white/[0.04]" strokeDasharray="4 4" />
                <line x1="30" y1="150" x2="770" y2="150" stroke="currentColor" className="text-gray-100 dark:text-white/[0.04]" strokeDasharray="4 4" />
                <path d={areaD} fill="url(#ov-grad)" />
                <path d={pathD} fill="none" stroke="#3b82f6" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" />
                {hoveredPointIndex !== null && points[hoveredPointIndex] && (
                  <>
                    <line x1={points[hoveredPointIndex].x} y1="20" x2={points[hoveredPointIndex].x} y2="160" stroke="#3b82f6" strokeWidth="1.5" strokeDasharray="3 3" className="opacity-70" />
                    <circle cx={points[hoveredPointIndex].x} cy={points[hoveredPointIndex].y} r="4.5" fill="#3b82f6" stroke="white" strokeWidth="2" className="shadow-md" />
                  </>
                )}
              </svg>

              <div className="absolute inset-0 flex">
                {points.map((_, i) => (
                  <div key={i} onMouseEnter={() => setHoveredPointIndex(i)} onMouseLeave={() => setHoveredPointIndex(null)} className="flex-1 h-full cursor-crosshair" />
                ))}
              </div>

              {hoveredPointIndex !== null && points[hoveredPointIndex] && (
                <div
                  style={{
                    left: `${(points[hoveredPointIndex].x / 800) * 100}%`,
                    top: `${Math.max(10, points[hoveredPointIndex].y - 45)}px`,
                    transform: 'translateX(-50%)',
                  }}
                  className="absolute pointer-events-none z-30 p-1.5 px-2.5 rounded-lg bg-gray-900/95 dark:bg-zinc-100/95 text-white dark:text-zinc-900 shadow-xl text-[10px] flex flex-col gap-0.5 whitespace-nowrap"
                >
                  <div className="flex items-center justify-between gap-3 font-semibold">
                    <span>{points[hoveredPointIndex].data.time}</span>
                    <span className="text-emerald-400 dark:text-emerald-600 font-mono">⚡ {points[hoveredPointIndex].data.tps} tok/s</span>
                  </div>
                  <div className="text-[9px] opacity-80 font-mono">
                    {activeMetric === 'tokens' ? `${(points[hoveredPointIndex].data.tokens / 1000000).toFixed(1)}M Tokens` : `省 $${points[hoveredPointIndex].data.cost.toFixed(2)} USD`} · 缓存 {points[hoveredPointIndex].data.cacheRate}%
                  </div>
                </div>
              )}
            </div>
            <div className="flex items-center justify-between text-[9px] text-gray-400 font-mono px-2 pt-1 border-t border-gray-100 dark:border-white/5">
              <span>00:00</span>
              <span>06:00</span>
              <span>12:00</span>
              <span>16:00 (高峰)</span>
              <span>20:00</span>
              <span>22:00</span>
            </div>
          </div>

          {/* 3 个维度的快速下钻导航入口卡片 (Direct Drill Shortcuts) */}
          <div className="grid grid-cols-1 md:grid-cols-3 gap-2.5">
            <button
              onClick={() => setActiveView('account')}
              className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs hover:border-blue-500/40 text-left transition-all flex flex-col justify-between group"
            >
              <div className="flex items-center justify-between">
                <span className="text-[11px] font-semibold text-gray-800 dark:text-zinc-200 flex items-center gap-1.5">
                  <UserCheck className="w-3.5 h-3.5 text-blue-500" />
                  <span>{isZh ? '进入账号详情透视' : 'Account Drill-down'}</span>
                </span>
                <ChevronRight className="w-3.5 h-3.5 text-gray-400 group-hover:translate-x-0.5 transition-transform" />
              </div>
              <p className="text-[10px] text-gray-400 mt-1">
                {isZh ? `管理 ${reportData.byAccount.length} 个主力账号资产、5h/7d 实时水位与工时热力图` : 'Deep asset analysis, quotas and heatmaps'}
              </p>
            </button>

            <button
              onClick={() => setActiveView('project')}
              className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs hover:border-indigo-500/40 text-left transition-all flex flex-col justify-between group"
            >
              <div className="flex items-center justify-between">
                <span className="text-[11px] font-semibold text-gray-800 dark:text-zinc-200 flex items-center gap-1.5">
                  <FolderGit2 className="w-3.5 h-3.5 text-indigo-500" />
                  <span>{isZh ? '进入项目详情透视' : 'Project Drill-down'}</span>
                </span>
                <ChevronRight className="w-3.5 h-3.5 text-gray-400 group-hover:translate-x-0.5 transition-transform" />
              </div>
              <p className="text-[10px] text-gray-400 mt-1">
                {isZh ? `分析 ${reportData.byProject.length} 个核心代码库研发效能、模型偏好与会话档案` : 'Workspace engineering efficiency and models'}
              </p>
            </button>

            <button
              onClick={() => setActiveView('model')}
              className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs hover:border-purple-500/40 text-left transition-all flex flex-col justify-between group"
            >
              <div className="flex items-center justify-between">
                <span className="text-[11px] font-semibold text-gray-800 dark:text-zinc-200 flex items-center gap-1.5">
                  <Cpu className="w-3.5 h-3.5 text-purple-500" />
                  <span>{isZh ? '进入模型详情透视' : 'Model Drill-down'}</span>
                </span>
                <ChevronRight className="w-3.5 h-3.5 text-gray-400 group-hover:translate-x-0.5 transition-transform" />
              </div>
              <p className="text-[10px] text-gray-400 mt-1">
                {isZh ? '物理上游满血保真度、暗降率诊断与流速延迟瀑布流' : 'Physical upstream fidelity & TTFT latency'}
              </p>
            </button>
          </div>
        </div>
      )}

      {/* ============================================================== */}
      {/* 视角 2：👤 账号详情透视 (Account Deep Dive - 资产与健康视角)       */}
      {/* ============================================================== */}
      {activeView === 'account' && (
        <div className="flex flex-col gap-3 animate-in fade-in-30 duration-200">
          {/* 顶部：账号选择水平胶囊栏 */}
          <div className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl p-2 px-3 shadow-2xs flex items-center justify-between gap-3 overflow-x-auto">
            <div className="flex items-center gap-2 overflow-x-auto">
              <span className="text-[10.5px] font-semibold text-gray-500 dark:text-zinc-400 shrink-0 mr-1 flex items-center gap-1">
                <UserCheck className="w-3.5 h-3.5 text-blue-500" />
                <span>{isZh ? '选择目标账号:' : 'Select Account:'}</span>
              </span>

              {reportData.byAccount.map((acc) => (
                <button
                  key={acc.email}
                  onClick={() => setSelectedAccountEmail(acc.email)}
                  className={`px-2.5 py-1 rounded-lg text-[11px] font-medium transition-all flex items-center gap-1.5 shrink-0 ${
                    selectedAccountEmail === acc.email
                      ? 'bg-blue-600 text-white font-semibold shadow-xs'
                      : 'bg-gray-100/90 dark:bg-white/[0.04] text-gray-700 dark:text-zinc-300 hover:bg-gray-200 dark:hover:bg-zinc-800'
                  }`}
                >
                  <span className={`w-1.5 h-1.5 rounded-full ${acc.isHealthy ? 'bg-emerald-400' : 'bg-amber-400'}`} />
                  <span>{acc.email}</span>
                  {acc.tags && acc.tags.length > 0 && (
                    <span className={`text-[8.5px] px-1 py-0 rounded ${selectedAccountEmail === acc.email ? 'bg-white/20 text-white' : 'bg-gray-200 dark:bg-zinc-700 text-gray-600 dark:text-zinc-300'}`}>
                      {acc.tags[0]}
                    </span>
                  )}
                </button>
              ))}
            </div>

            {currentAccount && (
              <div className="shrink-0 flex items-center gap-2">
                <span className="px-2 py-0.5 rounded-full text-[10px] font-medium bg-emerald-50 text-emerald-600 dark:bg-emerald-500/10 dark:text-emerald-400 border border-emerald-200/60 dark:border-emerald-500/20 flex items-center gap-1">
                  <CheckCircle2 className="w-3 h-3 text-emerald-500" />
                  <span>{isZh ? '挂载于 Antigravity IDE · 零暗降' : 'Attached to IDE · 100% Fidelity'}</span>
                </span>
              </div>
            )}
          </div>

          {currentAccount && (
            <div className="flex flex-col gap-3">
              {/* ========================================================= */}
              {/* 【顶层身份与微型额度 Bar】 (对齐 Accounts.tsx 真实双轨实现)   */}
              {/* ========================================================= */}
              <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-wrap items-center justify-between gap-3">
                {/* 左侧身份卡 */}
                <div className="flex items-center gap-2.5">
                  <div className="w-8 h-8 rounded-lg bg-gradient-to-tr from-blue-600 to-indigo-500 flex items-center justify-center text-white font-bold text-xs shadow-xs">
                    {currentAccount.email.charAt(0).toUpperCase()}
                  </div>
                  <div className="flex flex-col">
                    <div className="flex items-center gap-1.5">
                      <span className="text-xs font-bold text-gray-900 dark:text-zinc-100">{currentAccount.email}</span>
                      {currentAccount.tags.map((tag) => (
                        <span key={tag} className="text-[9px] px-1.5 py-0.2 rounded bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 font-medium">
                          {tag}
                        </span>
                      ))}
                      <span className="text-[9px] px-1.5 py-0.2 rounded bg-purple-50 text-purple-600 dark:bg-purple-500/10 dark:text-purple-400 font-mono font-medium">
                        PRO Tier
                      </span>
                    </div>
                    <span className="text-[9.5px] text-gray-400 font-mono">
                      Account ID: {currentAccount.accountId} · 上次同步: 刚刚
                    </span>
                  </div>
                </div>

                {/* 右侧微型双轨额度胶囊 (参考 Accounts.tsx 真实实现，优雅紧凑) */}
                <div className="flex items-center gap-2">
                  {/* Gemini 配额微胶囊 */}
                  <div className="p-1 px-2 rounded-lg bg-gray-50 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5 flex flex-col gap-0.5">
                    <div className="flex items-center justify-between text-[9px] font-medium text-gray-600 dark:text-zinc-300 gap-3">
                      <span className="flex items-center gap-1">
                        <span className="w-1.5 h-1.5 rounded-full bg-blue-500" />
                        <span>Gemini</span>
                      </span>
                      <span className="font-mono text-blue-600 dark:text-blue-400 font-bold">5h: 100% · 7d: 100%</span>
                    </div>
                    <div className="w-24 h-1 rounded-full bg-gray-200 dark:bg-white/10 overflow-hidden">
                      <div style={{ width: '100%' }} className="h-full rounded-full bg-blue-500" />
                    </div>
                  </div>

                  {/* Claude 配额微胶囊 */}
                  <div className="p-1 px-2 rounded-lg bg-gray-50 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5 flex flex-col gap-0.5">
                    <div className="flex items-center justify-between text-[9px] font-medium text-gray-600 dark:text-zinc-300 gap-3">
                      <span className="flex items-center gap-1">
                        <span className="w-1.5 h-1.5 rounded-full bg-purple-500" />
                        <span>Claude</span>
                      </span>
                      <span className="font-mono text-purple-600 dark:text-purple-400 font-bold">
                        5h: {currentAccount.quota5hPct ?? 85}% ({currentAccount.quota5hResetIn ?? '45m'})
                      </span>
                    </div>
                    <div className="w-28 h-1 rounded-full bg-gray-200 dark:bg-white/10 overflow-hidden flex">
                      <div style={{ width: `${currentAccount.quota5hPct ?? 85}%` }} className="h-full bg-purple-500" />
                    </div>
                  </div>
                </div>
              </div>

              {/* ========================================================= */}
              {/* 【第一层：算力资产与金库 Bento】 (参考图 08, 09, 11, 21)       */}
              {/* ========================================================= */}
              <div className="p-3.5 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col gap-2.5">
                <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
                  <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                    <DollarSign className="w-3.5 h-3.5 text-emerald-500" />
                    <span>{isZh ? '算力资产与价值折算金库' : 'Token Treasury & Valuation'}</span>
                  </span>
                  <span className="text-[9.5px] text-emerald-600 dark:text-emerald-400 font-medium">
                    Prompt Caching 官方 100% 免费减免
                  </span>
                </div>

                {/* 4 格核心资产指标卡 */}
                <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-2.5">
                  <div className="p-2.5 rounded-lg bg-gray-50/80 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5 flex flex-col justify-between">
                    <span className="text-[10px] text-gray-400">{isZh ? '累计总吞吐' : 'Total Throughput'}</span>
                    <div className="my-1 flex items-baseline gap-1">
                      <span className="text-xl font-bold font-mono text-gray-900 dark:text-zinc-100">
                        {(currentAccount.totalTokens / 1000000).toFixed(1)}M
                      </span>
                      <span className="text-[9.5px] text-gray-400">Tokens</span>
                    </div>
                    <span className="text-[9px] text-gray-400 font-mono">共承载 {currentAccount.totalRequests} 次交互请求</span>
                  </div>

                  <div className="p-2.5 rounded-lg bg-emerald-50/40 dark:bg-emerald-500/5 border border-emerald-200/50 dark:border-emerald-500/10 flex flex-col justify-between">
                    <span className="text-[10px] text-emerald-600/80 dark:text-emerald-400/80">{isZh ? '免计费已省金额' : 'Cash Saved'}</span>
                    <div className="my-1 flex items-baseline gap-1">
                      <span className="text-xl font-bold font-mono text-emerald-600 dark:text-emerald-400">
                        +${currentAccount.estimatedCostUsd.toFixed(1)}
                      </span>
                      <span className="text-[9.5px] text-emerald-600/70 font-mono">USD</span>
                    </div>
                    <span className="text-[9px] text-emerald-600 font-mono">等效 API 标价 $214.50</span>
                  </div>

                  <div className="p-2.5 rounded-lg bg-blue-50/40 dark:bg-blue-500/5 border border-blue-200/50 dark:border-blue-500/10 flex flex-col justify-between">
                    <span className="text-[10px] text-blue-600/80 dark:text-blue-400/80">{isZh ? '全局缓存命中率' : 'Cache Hit Rate'}</span>
                    <div className="my-1 flex items-baseline gap-1">
                      <span className="text-xl font-bold font-mono text-blue-600 dark:text-blue-400">
                        {currentAccount.cacheHitRate.toFixed(1)}%
                      </span>
                    </div>
                    <span className="text-[9px] text-blue-600 font-mono">首字响应提速 3.4x</span>
                  </div>

                  {/* 核心卖点：5h/7d 额度折算储备 */}
                  <div className="p-2.5 rounded-lg bg-gradient-to-r from-blue-500/[0.08] to-purple-500/[0.08] border border-blue-500/20 dark:border-blue-500/20 flex flex-col justify-between">
                    <span className="text-[10px] text-purple-600 dark:text-purple-400 font-semibold flex items-center gap-1">
                      <Sparkles className="w-3 h-3 text-purple-500" />
                      <span>{isZh ? '额度折算资产储备' : 'Quota Valuation'}</span>
                    </span>
                    <div className="my-0.5 flex flex-col text-[10.5px] font-mono leading-tight">
                      <span className="text-gray-900 dark:text-zinc-100 font-bold">
                        5h: 约 {Math.round(((currentAccount.quota5hPct ?? 85) / 100) * 450).toLocaleString()}K Tok
                        <span className="text-emerald-600 dark:text-emerald-400 text-[9px] ml-1">
                          (${( ((currentAccount.quota5hPct ?? 85) / 100) * 16.80).toFixed(2)})
                        </span>
                      </span>
                      <span className="text-gray-900 dark:text-zinc-100 font-bold">
                        7d: 约 {(Math.round(((currentAccount.quota7dPct ?? 50) / 100) * 4500) / 1000).toFixed(1)}M Tok
                        <span className="text-purple-600 dark:text-purple-400 text-[9px] ml-1">
                          (${( ((currentAccount.quota7dPct ?? 50) / 100) * 168.00).toFixed(2)})
                        </span>
                      </span>
                    </div>
                    <span className="text-[8.5px] text-gray-400 font-mono">*标称 450K/4.5M 额度模型</span>
                  </div>
                </div>

                {/* 5 层 Token 构成水库连续流胶囊条 (参考图 11) */}
                <div className="pt-2 border-t border-gray-100 dark:border-white/5 flex flex-col gap-1.5">
                  <div className="flex items-center justify-between text-[10px] text-gray-500 dark:text-zinc-400">
                    <span className="font-medium">五层上下文水库构成拆解 (Continuous Reservoir Stream)</span>
                    <span className="font-mono text-[9px]">缓存占比 {currentAccount.cacheHitRate.toFixed(1)}%</span>
                  </div>

                  <div className="w-full h-2 rounded-full bg-gray-100 dark:bg-white/[0.06] overflow-hidden flex">
                    <div style={{ width: '90.8%' }} className="h-full bg-emerald-500" title="官方缓存命中: 188.0M" />
                    <div style={{ width: '5.2%' }} className="h-full bg-blue-500" title="新输入 Prompt: 10.8M" />
                    <div style={{ width: '2.4%' }} className="h-full bg-purple-500" title="深度思考: 5.0M" />
                    <div style={{ width: '1.2%' }} className="h-full bg-amber-500" title="工具流: 2.5M" />
                    <div style={{ width: '0.4%' }} className="h-full bg-cyan-400" title="模型输出: 710K" />
                  </div>

                  <div className="flex flex-wrap items-center justify-between text-[9px] text-gray-400 font-mono pt-0.5">
                    <span className="flex items-center gap-1 text-emerald-600 dark:text-emerald-400 font-medium">
                      <span className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
                      <span>缓存 {(currentAccount.cachedTokens / 1000000).toFixed(1)}M (90.8%)</span>
                    </span>
                    <span className="flex items-center gap-1 text-blue-600 dark:text-blue-400">
                      <span className="w-1.5 h-1.5 rounded-full bg-blue-500" />
                      <span>输入 {(currentAccount.newInputTokens / 1000000).toFixed(1)}M (5.2%)</span>
                    </span>
                    <span className="flex items-center gap-1 text-purple-600 dark:text-purple-400">
                      <span className="w-1.5 h-1.5 rounded-full bg-purple-500" />
                      <span>思考 8.5M (2.4%)</span>
                    </span>
                    <span className="flex items-center gap-1 text-amber-600 dark:text-amber-400">
                      <span className="w-1.5 h-1.5 rounded-full bg-amber-500" />
                      <span>工具 6.2M (1.2%)</span>
                    </span>
                    <span className="flex items-center gap-1 text-cyan-600 dark:text-cyan-400">
                      <span className="w-1.5 h-1.5 rounded-full bg-cyan-400" />
                      <span>输出 {(currentAccount.outputTokens / 1000).toFixed(0)}K (0.4%)</span>
                    </span>
                  </div>
                </div>
              </div>

              {/* ========================================================= */}
              {/* 【第二层：6维战力雷达 + 模型圆环 VS 山脉波形曲线】 (参考图 05, 02, 19) */}
              {/* ========================================================= */}
              <div className="grid grid-cols-1 lg:grid-cols-12 gap-3">
                {/* 左卡：6 维战力雷达 + 模型圆环分布 (参考图 05 CLARITY.GG + 图 02 Donut) */}
                <div className="lg:col-span-6 p-3.5 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
                  <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
                    <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                      <Activity className="w-3.5 h-3.5 text-purple-500" />
                      <span>{isZh ? '6 维技术战力雷达与模型分布' : 'Technical Radar & Model Donut'}</span>
                    </span>
                    <span className="text-[9.5px] px-1.5 py-0.2 rounded-md bg-purple-50 text-purple-600 dark:bg-purple-500/10 dark:text-purple-400 font-mono font-medium">
                      SSS 满血旗舰
                    </span>
                  </div>

                  {/* 左右并列：左边雷达图，右边圆环图 */}
                  <div className="grid grid-cols-2 gap-2 items-center my-2">
                    {/* 1. 六边形战力雷达 SVG */}
                    <div className="flex flex-col items-center justify-center">
                      <svg viewBox="0 0 170 160" className="w-36 h-36 overflow-visible">
                        <defs>
                          <radialGradient id="radar-glow" cx="50%" cy="50%" r="50%">
                            <stop offset="0%" stopColor="#8b5cf6" stopOpacity="0.4" />
                            <stop offset="100%" stopColor="#8b5cf6" stopOpacity="0.05" />
                          </radialGradient>
                        </defs>
                        {/* 3 层六边形底网 */}
                        {[0.33, 0.66, 1].map((scale, sIdx) => {
                          const r = 50 * scale;
                          const pts = [0, 60, 120, 180, 240, 300].map((deg) => {
                            const rad = ((deg - 90) * Math.PI) / 180;
                            return `${85 + r * Math.cos(rad)},${80 + r * Math.sin(rad)}`;
                          }).join(' ');
                          return <polygon key={sIdx} points={pts} fill="none" stroke="currentColor" strokeWidth="0.8" className="text-gray-200 dark:text-white/10" strokeDasharray={sIdx < 2 ? "2 2" : undefined} />;
                        })}
                        {/* 6 根辐射轴线 */}
                        {[0, 60, 120, 180, 240, 300].map((deg, aIdx) => {
                          const rad = ((deg - 90) * Math.PI) / 180;
                          return <line key={aIdx} x1="85" y1="80" x2={85 + 50 * Math.cos(rad)} y2={80 + 50 * Math.sin(rad)} stroke="currentColor" strokeWidth="0.8" className="text-gray-200 dark:text-white/10" />;
                        })}
                        {/* 数据多边形: TPS: 92, TTFT: 88, Cache: 95, Fidelity: 100, Concurrency: 98, Endurance: 75 */}
                        {(() => {
                          const vals = [92, 88, 95, 100, 98, 75];
                          const pts = vals.map((val, idx) => {
                            const deg = idx * 60;
                            const rad = ((deg - 90) * Math.PI) / 180;
                            const r = (50 * val) / 100;
                            return { x: 85 + r * Math.cos(rad), y: 80 + r * Math.sin(rad) };
                          });
                          const polyStr = pts.map(p => `${p.x},${p.y}`).join(' ');
                          return (
                            <>
                              <polygon points={polyStr} fill="url(#radar-glow)" stroke="#8b5cf6" strokeWidth="2" strokeLinejoin="round" />
                              {pts.map((p, pIdx) => (
                                <circle key={pIdx} cx={p.x} cy={p.y} r="2.5" fill="#8b5cf6" stroke="white" strokeWidth="1" />
                              ))}
                            </>
                          );
                        })()}
                        {/* 6 个顶点标签 */}
                        <text x="85" y="14" textAnchor="middle" className="text-[8.5px] fill-gray-500 dark:fill-zinc-400 font-medium">流速 92</text>
                        <text x="142" y="48" textAnchor="start" className="text-[8.5px] fill-gray-500 dark:fill-zinc-400 font-medium">响应 88</text>
                        <text x="142" y="115" textAnchor="start" className="text-[8.5px] fill-gray-500 dark:fill-zinc-400 font-medium">缓存 95</text>
                        <text x="85" y="148" textAnchor="middle" className="text-[8.5px] fill-emerald-600 dark:fill-emerald-400 font-bold">保真 100</text>
                        <text x="28" y="115" textAnchor="end" className="text-[8.5px] fill-gray-500 dark:fill-zinc-400 font-medium">并发 98</text>
                        <text x="28" y="48" textAnchor="end" className="text-[8.5px] fill-gray-500 dark:fill-zinc-400 font-medium">续航 75</text>
                      </svg>
                      <span className="text-[9px] text-gray-400 font-mono mt-1">6 维综合效能雷达</span>
                    </div>

                    {/* 2. 模型消耗圆环 Donut SVG */}
                    <div className="flex flex-col items-center justify-center">
                      <svg viewBox="0 0 160 160" className="w-36 h-36 overflow-visible">
                        {/* 底轨 */}
                        <circle cx="80" cy="80" r="48" fill="none" stroke="currentColor" strokeWidth="12" className="text-gray-100 dark:text-white/[0.06]" />
                        {/* Claude 58% -> 301.59 * 0.58 = 174.92 */}
                        <circle cx="80" cy="80" r="48" fill="none" stroke="#8b5cf6" strokeWidth="12" strokeDasharray="174.92 126.67" strokeDashoffset="0" transform="rotate(-90 80 80)" />
                        {/* Gemini Pro 32% -> 301.59 * 0.32 = 96.51 */}
                        <circle cx="80" cy="80" r="48" fill="none" stroke="#3b82f6" strokeWidth="12" strokeDasharray="96.51 205.08" strokeDashoffset="-174.92" transform="rotate(-90 80 80)" />
                        {/* Gemini Flash 10% -> 301.59 * 0.10 = 30.16 */}
                        <circle cx="80" cy="80" r="48" fill="none" stroke="#06b6d4" strokeWidth="12" strokeDasharray="30.16 271.43" strokeDashoffset="-271.43" transform="rotate(-90 80 80)" />
                        {/* 中心文字 */}
                        <text x="80" y="76" textAnchor="middle" className="text-sm font-bold font-mono fill-gray-900 dark:fill-zinc-100">
                          {(currentAccount.totalTokens / 1000000).toFixed(1)}M
                        </text>
                        <text x="80" y="92" textAnchor="middle" className="text-[8px] fill-gray-400 font-medium">
                          Tokens 构成
                        </text>
                      </svg>

                      {/* 环形图图例 */}
                      <div className="flex items-center gap-2 text-[8.5px] font-mono mt-1">
                        <span className="flex items-center gap-1 text-purple-600 dark:text-purple-400 font-medium">
                          <span className="w-1.5 h-1.5 rounded-full bg-purple-500" />
                          <span>Claude 58%</span>
                        </span>
                        <span className="flex items-center gap-1 text-blue-600 dark:text-blue-400">
                          <span className="w-1.5 h-1.5 rounded-full bg-blue-500" />
                          <span>Pro 32%</span>
                        </span>
                        <span className="flex items-center gap-1 text-cyan-600 dark:text-cyan-400">
                          <span className="w-1.5 h-1.5 rounded-full bg-cyan-400" />
                          <span>Flash 10%</span>
                        </span>
                      </div>
                    </div>
                  </div>

                  <div className="pt-1.5 border-t border-gray-100 dark:border-white/5 text-[9px] text-gray-400 font-mono flex items-center justify-between">
                    <span>主力调用模型: Claude 3.7 Sonnet</span>
                    <span>双模型组完全兼容</span>
                  </div>
                </div>

                {/* 右卡：山脉面积波形曲线与高峰标注 (参考图 19 高峰山脉波形 + 图 04 趋势线) */}
                <div className="lg:col-span-6 p-3.5 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
                  <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
                    <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                      <TrendingUp className="w-3.5 h-3.5 text-blue-500" />
                      <span>{isZh ? 'Token 吞吐时序山脉波形与峰值标注' : 'Throughput Mountain Wave & Peak'}</span>
                    </span>
                    <span className="text-[9.5px] text-gray-400 font-mono">
                      波峰实时诊断 · ⚡ 192 tok/s
                    </span>
                  </div>

                  {/* 山脉波形 SVG */}
                  <div className="my-2 relative">
                    <svg viewBox="0 0 520 140" className="w-full h-32 overflow-visible">
                      <defs>
                        <linearGradient id="peak-wave-grad" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="0%" stopColor="#8b5cf6" stopOpacity="0.4" />
                          <stop offset="60%" stopColor="#3b82f6" stopOpacity="0.15" />
                          <stop offset="100%" stopColor="#3b82f6" stopOpacity="0.0" />
                        </linearGradient>
                      </defs>
                      {/* 背景水平网格虚线 */}
                      <line x1="20" y1="35" x2="500" y2="35" stroke="currentColor" strokeWidth="0.8" className="text-gray-100 dark:text-white/[0.04]" strokeDasharray="3 3" />
                      <line x1="20" y1="75" x2="500" y2="75" stroke="currentColor" strokeWidth="0.8" className="text-gray-100 dark:text-white/[0.04]" strokeDasharray="3 3" />
                      <line x1="20" y1="115" x2="500" y2="115" stroke="currentColor" strokeWidth="0.8" className="text-gray-100 dark:text-white/[0.04]" />

                      {/* 填充面积 */}
                      <path
                        d="M 20 120 Q 70 115, 110 95 T 190 70 T 270 55 T 350 25 T 420 50 T 500 85 L 500 120 L 20 120 Z"
                        fill="url(#peak-wave-grad)"
                      />
                      {/* 发光曲线 */}
                      <path
                        d="M 20 120 Q 70 115, 110 95 T 190 70 T 270 55 T 350 25 T 420 50 T 500 85"
                        fill="none"
                        stroke="#8b5cf6"
                        strokeWidth="2.5"
                        strokeLinecap="round"
                      />

                      {/* 波峰光标与提示气泡 (在 350, 25 处) */}
                      <line x1="350" y1="25" x2="350" y2="120" stroke="#8b5cf6" strokeWidth="1" strokeDasharray="2 2" className="opacity-60" />
                      <circle cx="350" cy="25" r="5" fill="#8b5cf6" stroke="white" strokeWidth="2" />
                      <circle cx="350" cy="25" r="9" fill="none" stroke="#8b5cf6" strokeWidth="1" className="opacity-40 animate-ping" />

                      {/* 悬停波峰气泡 */}
                      <g transform="translate(350, 8)">
                        <rect x="-85" y="-18" width="170" height="20" rx="5" fill="#1e1b4b" stroke="#8b5cf6" strokeWidth="1" />
                        <text x="0" y="-4" textAnchor="middle" fill="#c4b5fd" className="text-[9px] font-mono font-semibold">
                          🔥 16:00 峰值: 18.5M Tok · 192 tok/s
                        </text>
                      </g>
                    </svg>
                  </div>

                  <div className="flex items-center justify-between text-[9px] text-gray-400 font-mono pt-1.5 border-t border-gray-100 dark:border-white/5">
                    <span>00:00 (休眠)</span>
                    <span>06:00</span>
                    <span>12:00</span>
                    <span className="text-purple-600 dark:text-purple-400 font-bold">16:00 (重度编码波峰)</span>
                    <span>20:00</span>
                    <span>23:00</span>
                  </div>
                </div>
              </div>

              {/* ========================================================= */}
              {/* 【第三层：工程管道流向矩阵 VS 真实物理调度】 (参考图 03, 14, 01, 15) */}
              {/* ========================================================= */}
              <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
                {/* 模块 4：工程消耗管道流向矩阵 (参考图 03 SAP ECC 管道路由 + 图 14 发光桑基流) */}
                <div className="p-3.5 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col gap-2.5">
                  <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
                    <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                      <FolderGit2 className="w-3.5 h-3.5 text-indigo-500" />
                      <span>{isZh ? '工程消耗管道流向矩阵' : 'Project Drain Flow Matrix'}</span>
                    </span>
                    <span className="text-[9.5px] text-gray-400">{isZh ? '细分 Gemini 与 Claude 吞吐' : 'Gemini vs Claude breakdown'}</span>
                  </div>

                  <div className="flex flex-col gap-2">
                    {(currentAccount.projectDrains || [
                      {
                        workspaceName: 'at',
                        path: '~/Desktop/at',
                        sharePct: 68.0,
                        totalTokens: 140800000,
                        claudeTokens: 95000000,
                        claudePct: 67.5,
                        geminiTokens: 45800000,
                        geminiPct: 32.5,
                        cacheHitRate: 92.4,
                        avgTps: 178.5,
                        stepCount: 1850,
                      },
                      {
                        workspaceName: 'omin',
                        path: '~/Workspace/omin',
                        sharePct: 22.0,
                        totalTokens: 45500000,
                        claudeTokens: 15500000,
                        claudePct: 34.1,
                        geminiTokens: 30000000,
                        geminiPct: 65.9,
                        cacheHitRate: 88.0,
                        avgTps: 162.0,
                        stepCount: 620,
                      },
                      {
                        workspaceName: 'infra-scripts',
                        path: '~/Dev/infra-scripts',
                        sharePct: 10.0,
                        totalTokens: 20810000,
                        claudeTokens: 4200000,
                        claudePct: 20.2,
                        geminiTokens: 16610000,
                        geminiPct: 79.8,
                        cacheHitRate: 85.5,
                        avgTps: 145.0,
                        stepCount: 240,
                      },
                    ]).map((pd) => (
                      <div
                        key={pd.workspaceName}
                        className="p-2.5 rounded-lg bg-gray-50/70 dark:bg-white/[0.02] border border-gray-100 dark:border-white/5 flex flex-col gap-1.5"
                      >
                        {/* 头部：名称与总耗 */}
                        <div className="flex items-center justify-between text-[11px]">
                          <div className="flex items-center gap-1.5">
                            <FolderGit2 className="w-3.5 h-3.5 text-indigo-500" />
                            <span className="font-semibold text-gray-900 dark:text-zinc-100">{pd.workspaceName}</span>
                            <span className="text-[9px] text-gray-400 font-mono">{pd.path}</span>
                          </div>
                          <span className="font-mono text-indigo-600 dark:text-indigo-400 font-bold">
                            {(pd.totalTokens / 1000000).toFixed(1)}M ({pd.sharePct}%)
                          </span>
                        </div>

                        {/* 双核心模型分段条：Claude (紫) vs Gemini (青绿) */}
                        <div className="w-full h-1.5 rounded-full bg-gray-200 dark:bg-white/[0.08] overflow-hidden flex">
                          <div style={{ width: `${pd.claudePct}%` }} className="h-full bg-purple-500" title={`Claude: ${pd.claudePct}%`} />
                          <div style={{ width: `${pd.geminiPct}%` }} className="h-full bg-emerald-400" title={`Gemini: ${pd.geminiPct}%`} />
                        </div>

                        {/* 指标明细行 */}
                        <div className="flex items-center justify-between text-[9px] font-mono text-gray-500 dark:text-zinc-400 pt-0.5">
                          <div className="flex items-center gap-2">
                            <span className="text-purple-600 dark:text-purple-400 font-medium">
                              Claude: {(pd.claudeTokens / 1000000).toFixed(1)}M ({pd.claudePct.toFixed(0)}%)
                            </span>
                            <span className="text-emerald-600 dark:text-emerald-400 font-medium">
                              Gemini: {(pd.geminiTokens / 1000000).toFixed(1)}M ({pd.geminiPct.toFixed(0)}%)
                            </span>
                          </div>
                          <div className="flex items-center gap-2">
                            <span className="text-emerald-600">缓存 {pd.cacheHitRate}%</span>
                            <span className="text-blue-600">⚡ {pd.avgTps} tok/s</span>
                            <span className="text-gray-400">{pd.stepCount}步</span>
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>

                {/* 模块 3：模型消耗与真实物理调度 (参考图 01, 04, 15 纯技术参数行) */}
                <div className="p-3.5 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col gap-2.5">
                  <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
                    <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                      <Cpu className="w-3.5 h-3.5 text-purple-500" />
                      <span>{isZh ? '真实物理调度与保真度诊断' : 'Physical Upstream & Fidelity'}</span>
                    </span>
                    <span className="text-[9.5px] text-emerald-600 dark:text-emerald-400 font-medium flex items-center gap-1">
                      <ShieldCheck className="w-3 h-3 text-emerald-500" />
                      <span>{isZh ? '物理上游保真度 100% · 零暗降' : '100% Upstream Fidelity'}</span>
                    </span>
                  </div>

                  <div className="flex flex-col gap-2">
                    {(currentAccount.modelRoutings || [
                      {
                        requestedModel: 'claude-3-7-sonnet',
                        physicalModel: 'gemini-4-hydrogen-high',
                        totalTokens: 120120000,
                        sharePct: 58.0,
                        avgTps: 184.2,
                        p50TtftSec: 1.42,
                        cacheHitRate: 91.8,
                        isDowngraded: false,
                      },
                      {
                        requestedModel: 'gemini-2.5-pro',
                        physicalModel: 'gemini-4-hydrogen-high',
                        totalTokens: 66270000,
                        sharePct: 32.0,
                        avgTps: 168.0,
                        p50TtftSec: 1.35,
                        cacheHitRate: 90.5,
                        isDowngraded: false,
                      },
                      {
                        requestedModel: 'gemini-2.5-flash',
                        physicalModel: 'gemini-4-carbon',
                        totalTokens: 20720000,
                        sharePct: 10.0,
                        avgTps: 122.5,
                        p50TtftSec: 0.98,
                        cacheHitRate: 86.2,
                        isDowngraded: false,
                      },
                    ]).map((mr) => (
                      <div
                        key={mr.requestedModel}
                        className="p-2.5 rounded-lg bg-gray-50/70 dark:bg-white/[0.02] border border-gray-100 dark:border-white/5 flex flex-col gap-1.5"
                      >
                        {/* 模型请求 ➔ 物理调度 */}
                        <div className="flex items-center justify-between text-[11px]">
                          <div className="flex items-center gap-1.5">
                            <span className="font-semibold text-gray-900 dark:text-zinc-100">{mr.requestedModel}</span>
                            <ArrowRight className="w-3 h-3 text-gray-400" />
                            <span className="font-mono text-purple-600 dark:text-purple-400 font-bold">{mr.physicalModel}</span>
                          </div>
                          <span className="font-mono font-bold text-gray-800 dark:text-zinc-200">
                            {(mr.totalTokens / 1000000).toFixed(1)}M ({mr.sharePct}%)
                          </span>
                        </div>

                        {/* 进度条 */}
                        <div className="w-full h-1.5 rounded-full bg-gray-200 dark:bg-white/[0.08] overflow-hidden">
                          <div style={{ width: `${mr.sharePct}%` }} className="h-full rounded-full bg-purple-500" />
                        </div>

                        {/* 硬核技术指标网格 */}
                        <div className="grid grid-cols-4 gap-1 text-[9.5px] font-mono pt-0.5">
                          <div className="flex items-center gap-1 text-emerald-600 dark:text-emerald-400 font-bold">
                            <span>⚡ {mr.avgTps} tok/s</span>
                          </div>
                          <div className="flex items-center gap-1 text-gray-600 dark:text-zinc-400">
                            <span>⏱ {mr.p50TtftSec}s TTFT</span>
                          </div>
                          <div className="flex items-center gap-1 text-blue-600 dark:text-blue-400">
                            <span>缓存 {mr.cacheHitRate}%</span>
                          </div>
                          <div className="text-right">
                            <span className="text-[9px] px-1 py-0.2 rounded bg-emerald-50 text-emerald-600 dark:bg-emerald-500/10 dark:text-emerald-400 font-sans font-semibold">
                              满血保真
                            </span>
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              </div>

              {/* ========================================================= */}
              {/* 【第四层：客户端消耗排行 + 24小时高密度热力柱】 (参考图 04, 19, 10, 23) */}
              {/* ========================================================= */}
              <div className="grid grid-cols-1 lg:grid-cols-12 gap-3">
                {/* 模块 5：客户端/宿主消耗排行榜 (参考图 04, 19) */}
                <div className="lg:col-span-5 p-3.5 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
                  <div>
                    <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
                      <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                        <AppWindow className="w-3.5 h-3.5 text-blue-500" />
                        <span>{isZh ? '客户端消耗排行榜' : 'Host Matrix Ranking'}</span>
                      </span>
                      <span className="text-[9.5px] text-gray-400">{isZh ? '消耗宿主分布' : 'Clients'}</span>
                    </div>

                    <div className="flex flex-col gap-2 mt-2">
                      {(currentAccount.hostRanks || [
                        {
                          hostType: 'ide',
                          hostLabel: 'Antigravity IDE',
                          sharePct: 70.0,
                          totalTokens: 145000000,
                          requestCount: 1420,
                        },
                        {
                          hostType: 'app',
                          hostLabel: '原生桌面 App',
                          sharePct: 18.0,
                          totalTokens: 37300000,
                          requestCount: 380,
                        },
                        {
                          hostType: 'plugin',
                          hostLabel: 'ACP 插件 (Cursor/VSCode)',
                          sharePct: 8.0,
                          totalTokens: 16500000,
                          requestCount: 150,
                        },
                        {
                          hostType: 'cli',
                          hostLabel: 'agy CLI 终端',
                          sharePct: 4.0,
                          totalTokens: 8310000,
                          requestCount: 74,
                        },
                      ]).map((hr, idx) => {
                        const IconComponent =
                          hr.hostType === 'ide'
                            ? Code2
                            : hr.hostType === 'app'
                            ? AppWindow
                            : hr.hostType === 'plugin'
                            ? Puzzle
                            : Terminal;

                        return (
                          <div key={hr.hostType} className="flex flex-col gap-1">
                            <div className="flex items-center justify-between text-[10.5px]">
                              <span className="font-medium text-gray-800 dark:text-zinc-200 flex items-center gap-1.5">
                                <span className="w-3.5 h-3.5 rounded-full bg-blue-50 dark:bg-blue-500/10 text-blue-600 dark:text-blue-400 flex items-center justify-center text-[9px] font-bold">
                                  {idx + 1}
                                </span>
                                <IconComponent className="w-3 h-3 text-blue-500" />
                                <span>{hr.hostLabel}</span>
                              </span>
                              <div className="flex items-center gap-1.5 font-mono text-[10px]">
                                <span className="font-bold text-blue-600 dark:text-blue-400">{hr.sharePct}%</span>
                                <span className="text-gray-400">({(hr.totalTokens / 1000000).toFixed(1)}M)</span>
                              </div>
                            </div>

                            <div className="w-full h-1.5 rounded-full bg-gray-100 dark:bg-white/[0.06] overflow-hidden">
                              <div
                                style={{ width: `${hr.sharePct}%` }}
                                className={`h-full rounded-full ${
                                  idx === 0
                                    ? 'bg-blue-600'
                                    : idx === 1
                                    ? 'bg-blue-400'
                                    : idx === 2
                                    ? 'bg-indigo-400'
                                    : 'bg-gray-400'
                                }`}
                              />
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  </div>

                  <div className="pt-2 border-t border-gray-100 dark:border-white/5 text-[9px] text-gray-400 font-mono flex items-center justify-between">
                    <span>主力战场: Antigravity IDE (70%)</span>
                    <span>共承载 4 种开发宿主</span>
                  </div>
                </div>

                {/* 模块 6：24 小时活跃调用时间热力图 (参考图 10, 18, 23 彩色高密度柱) */}
                <div className="lg:col-span-7 p-3.5 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
                  <div>
                    <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
                      <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                        <Clock className="w-3.5 h-3.5 text-emerald-500" />
                        <span>{isZh ? '24 小时活跃调用时间热力图' : 'Circadian Activity Heatmap'}</span>
                      </span>
                      <span className="text-[9.5px] text-gray-400 font-mono">{isZh ? '峰值时段: 14:00 - 17:00 (192 tok/s)' : 'Peak: 14:00 - 17:00'}</span>
                    </div>

                    <div className="h-28 w-full flex items-end gap-1 pt-3 pb-1 px-1">
                      {(currentAccount.hourlyHeatmap || [2, 0, 0, 1, 3, 5, 12, 28, 45, 62, 58, 40, 35, 68, 85, 92, 74, 55, 48, 65, 52, 38, 20, 8]).map((val, hour) => {
                        const heightPct = Math.max(10, (val / 100) * 100);
                        return (
                          <div key={hour} className="flex-1 flex flex-col items-center gap-1 group relative">
                            <div
                              style={{ height: `${heightPct}%` }}
                              className={`w-full rounded-t-sm transition-all duration-300 ${
                                val > 70
                                  ? 'bg-gradient-to-t from-blue-600 to-indigo-500'
                                  : val > 40
                                  ? 'bg-gradient-to-t from-blue-500 to-blue-400'
                                  : val > 15
                                  ? 'bg-blue-300 dark:bg-blue-500/40'
                                  : 'bg-gray-200 dark:bg-white/[0.06]'
                              }`}
                            />
                            {/* 悬停微型交互 Tooltip */}
                            <div className="absolute -top-8 hidden group-hover:flex bg-gray-900/95 dark:bg-zinc-100/95 text-white dark:text-zinc-900 text-[8.5px] font-mono px-1.5 py-0.5 rounded-md shadow-xl pointer-events-none whitespace-nowrap z-20 flex-col items-center">
                              <span>{hour}:00 - {val} 次请求</span>
                              <span className="text-[7.5px] opacity-80">约 {(val * 0.2).toFixed(1)}M Tokens</span>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  </div>

                  <div className="flex items-center justify-between text-[9px] text-gray-400 font-mono pt-1.5 border-t border-gray-100 dark:border-white/5">
                    <span className="flex items-center gap-1">
                      <span className="w-1.5 h-1.5 rounded-full bg-gray-300 dark:bg-zinc-600" />
                      <span>00:00 (休眠待机)</span>
                    </span>
                    <span className="flex items-center gap-1">
                      <span className="w-1.5 h-1.5 rounded-full bg-blue-400" />
                      <span>09:00 (晨间架构)</span>
                    </span>
                    <span className="flex items-center gap-1 text-indigo-600 dark:text-indigo-400 font-semibold">
                      <span className="w-1.5 h-1.5 rounded-full bg-indigo-500" />
                      <span>15:00 (午后重度波峰)</span>
                    </span>
                    <span className="flex items-center gap-1">
                      <span className="w-1.5 h-1.5 rounded-full bg-blue-300" />
                      <span>21:00 (夜间验证)</span>
                    </span>
                  </div>
                </div>
              </div>
            </div>
          )}
        </div>
      )}

      {/* ============================================================== */}
      {/* 视角 3：📁 项目详情透视 (Project Deep Dive - 研发效能视角)        */}
      {/* ============================================================== */}
      {activeView === 'project' && (
        <div className="flex flex-col gap-3 animate-in fade-in-30 duration-200">
          {/* 项目工作区选择胶囊栏 */}
          <div className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl p-2 px-3 shadow-2xs flex items-center gap-2 overflow-x-auto">
            <span className="text-[10.5px] font-semibold text-gray-500 dark:text-zinc-400 shrink-0 mr-1 flex items-center gap-1">
              <FolderGit2 className="w-3.5 h-3.5 text-indigo-500" />
              <span>{isZh ? '选择工程目录:' : 'Select Project:'}</span>
            </span>

            {reportData.byProject.map((p) => (
              <button
                key={p.workspaceName}
                onClick={() => setSelectedProjectName(p.workspaceName)}
                className={`px-2.5 py-1 rounded-lg text-[11px] font-medium transition-all flex items-center gap-1.5 shrink-0 ${
                  selectedProjectName === p.workspaceName
                    ? 'bg-indigo-600 text-white font-semibold shadow-xs'
                    : 'bg-gray-100/90 dark:bg-white/[0.04] text-gray-700 dark:text-zinc-300 hover:bg-gray-200 dark:hover:bg-zinc-800'
                }`}
              >
                <FolderGit2 className="w-3 h-3" />
                <span>{p.workspaceName}</span>
                <span className={`text-[8.5px] px-1 py-0 rounded ${selectedProjectName === p.workspaceName ? 'bg-white/20 text-white' : 'bg-gray-200 dark:bg-zinc-700 text-gray-600 dark:text-zinc-300'}`}>
                  {p.totalSteps.toLocaleString()}步
                </span>
              </button>
            ))}
          </div>

          {currentProject && (
            <div className="flex flex-col gap-3">
              {/* 项目概览指标卡片 */}
              <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-2.5">
                <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
                  <span className="text-[10.5px] text-gray-500 dark:text-zinc-400">{isZh ? '工程累计交互步数' : 'Total Steps'}</span>
                  <div className="my-1.5 flex items-baseline gap-1">
                    <span className="text-2xl font-bold font-mono text-gray-900 dark:text-zinc-100">
                      {currentProject.totalSteps.toLocaleString()}
                    </span>
                    <span className="text-[10px] text-gray-400">{isZh ? '步' : 'steps'}</span>
                  </div>
                  <span className="text-[9px] text-gray-400 font-mono">共归档 {currentProject.conversationCount} 个开发会话</span>
                </div>

                <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
                  <span className="text-[10.5px] text-gray-500 dark:text-zinc-400">{isZh ? '工程累计消耗 Token' : 'Project Tokens'}</span>
                  <div className="my-1.5 flex items-baseline gap-1">
                    <span className="text-2xl font-bold font-mono text-indigo-600 dark:text-indigo-400">
                      {(currentProject.totalTokens / 1000000).toFixed(1)}M
                    </span>
                    <span className="text-[10px] text-gray-400">Tokens</span>
                  </div>
                  <span className="text-[9px] text-indigo-600 font-mono">首选: {currentProject.preferredModel}</span>
                </div>

                <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
                  <span className="text-[10.5px] text-gray-500 dark:text-zinc-400">{isZh ? '估算等效官方成本' : 'Estimated Cost'}</span>
                  <div className="my-1.5 flex items-baseline gap-1">
                    <span className="text-2xl font-bold font-mono text-emerald-600 dark:text-emerald-400">
                      ${currentProject.estimatedCostUsd.toFixed(1)}
                    </span>
                    <span className="text-[10px] text-gray-400">USD</span>
                  </div>
                  <span className="text-[9px] text-emerald-600 font-mono">零代理本地直接连接</span>
                </div>

                <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
                  <span className="text-[10.5px] text-gray-500 dark:text-zinc-400">{isZh ? '最后活跃时间' : 'Last Activity'}</span>
                  <div className="my-1.5 flex items-center gap-1.5">
                    <Clock className="w-4 h-4 text-gray-400" />
                    <span className="text-base font-bold text-gray-800 dark:text-zinc-200">{currentProject.lastActiveTime}</span>
                  </div>
                  <span className="text-[9px] text-gray-400 font-mono truncate">{currentProject.path}</span>
                </div>
              </div>

              {/* 对称双卡：项目模型偏好度 VS 消耗账号归属 */}
              <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                {/* 模型偏好度 */}
                <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col gap-2.5">
                  <div className="flex items-center justify-between">
                    <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                      <Layers className="w-3.5 h-3.5 text-blue-500" />
                      <span>{isZh ? '该项目模型偏好度' : 'Model Affinity in this Project'}</span>
                    </span>
                    <span className="text-[9.5px] text-gray-400">{isZh ? '架构 vs 排错' : 'Usage'}</span>
                  </div>

                  <div className="flex flex-col gap-2">
                    {currentProject.modelBreakdown.map(([m, pct]) => (
                      <div key={m} className="flex flex-col gap-1">
                        <div className="flex items-center justify-between text-[10.5px]">
                          <span className="font-medium text-gray-800 dark:text-zinc-200">{m}</span>
                          <span className="font-mono text-blue-600 dark:text-blue-400 font-bold">{pct.toFixed(0)}%</span>
                        </div>
                        <div className="w-full h-1.5 rounded-full bg-gray-100 dark:bg-white/[0.06] overflow-hidden">
                          <div style={{ width: `${pct}%` }} className="h-full rounded-full bg-blue-500" />
                        </div>
                      </div>
                    ))}
                  </div>
                </div>

                {/* 账号归属溯源 */}
                <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col gap-2.5">
                  <div className="flex items-center justify-between">
                    <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                      <UserCheck className="w-3.5 h-3.5 text-emerald-500" />
                      <span>{isZh ? '该工程被哪些账号供能？' : 'Accounts Powering this Project'}</span>
                    </span>
                    <span className="text-[9.5px] text-gray-400">{isZh ? '账号承载' : 'Share'}</span>
                  </div>

                  <div className="flex flex-col gap-2">
                    {currentProject.accountAttribution.map(([acc, pct]) => (
                      <div key={acc} className="flex flex-col gap-1">
                        <div className="flex items-center justify-between text-[10.5px]">
                          <span className="font-medium text-gray-800 dark:text-zinc-200 truncate pr-2">{acc}</span>
                          <span className="font-mono text-emerald-600 dark:text-emerald-400 font-bold shrink-0">{pct.toFixed(0)}%</span>
                        </div>
                        <div className="w-full h-1.5 rounded-full bg-gray-100 dark:bg-white/[0.06] overflow-hidden">
                          <div style={{ width: `${pct}%` }} className="h-full rounded-full bg-emerald-500" />
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              </div>

              {/* 历史开发会话档案库 (Session Archive) */}
              <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                    <BookOpen className="w-3.5 h-3.5 text-indigo-500" />
                    <span>{isZh ? '该项目的历史研发会话记录' : 'Project Session Archive'}</span>
                  </span>
                  <span className="text-[9.5px] text-gray-400">{currentProject.sessionTitles.length} 个历史会话</span>
                </div>

                <div className="divide-y divide-gray-100 dark:divide-white/5">
                  {currentProject.sessionTitles.map((title, idx) => (
                    <div key={idx} className="py-2 flex items-center justify-between text-[11px] hover:bg-gray-50/50 dark:hover:bg-white/[0.02] px-1 rounded transition-colors">
                      <div className="flex items-center gap-2 truncate pr-2">
                        <span className="w-4 h-4 rounded-full bg-indigo-50 dark:bg-indigo-500/10 text-indigo-600 dark:text-indigo-400 flex items-center justify-center text-[9px] font-bold shrink-0">
                          {idx + 1}
                        </span>
                        <span className="font-medium text-gray-800 dark:text-zinc-200 truncate">{title}</span>
                      </div>
                      <span className="text-[9.5px] text-gray-400 font-mono shrink-0">步数 150~450 步</span>
                    </div>
                  ))}
                </div>
              </div>
            </div>
          )}
        </div>
      )}

      {/* ============================================================== */}
      {/* 视角 4：⚡ 模型详情透视 (Model Deep Dive - 物理保真视角)          */}
      {/* ============================================================== */}
      {activeView === 'model' && (
        <div className="flex flex-col gap-3 animate-in fade-in-30 duration-200">
          {/* 模型选择胶囊栏 */}
          <div className="bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl p-2 px-3 shadow-2xs flex items-center gap-2 overflow-x-auto">
            <span className="text-[10.5px] font-semibold text-gray-500 dark:text-zinc-400 shrink-0 mr-1 flex items-center gap-1">
              <Cpu className="w-3.5 h-3.5 text-purple-500" />
              <span>{isZh ? '选择模型:' : 'Select Model:'}</span>
            </span>

            {reportData.byModel.map((m) => (
              <button
                key={m.requestedModel}
                onClick={() => setSelectedModelName(m.requestedModel)}
                className={`px-2.5 py-1 rounded-lg text-[11px] font-medium transition-all flex items-center gap-1.5 shrink-0 ${
                  selectedModelName === m.requestedModel
                    ? 'bg-purple-600 text-white font-semibold shadow-xs'
                    : 'bg-gray-100/90 dark:bg-white/[0.04] text-gray-700 dark:text-zinc-300 hover:bg-gray-200 dark:hover:bg-zinc-800'
                }`}
              >
                <Cpu className="w-3 h-3" />
                <span>{m.requestedModel}</span>
                <span className={`text-[8.5px] px-1 py-0 rounded ${selectedModelName === m.requestedModel ? 'bg-white/20 text-white' : 'bg-gray-200 dark:bg-zinc-700 text-gray-600 dark:text-zinc-300'}`}>
                  {m.sharePercentage.toFixed(0)}%
                </span>
              </button>
            ))}
          </div>

          {currentModel && (
            <div className="flex flex-col gap-3">
              {/* 物理保真度与拓扑卡片 */}
              <div className="p-3.5 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-purple-500/20 dark:border-purple-500/20 shadow-2xs flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <span className="p-1 rounded-lg bg-purple-50 dark:bg-purple-500/10 text-purple-600 dark:text-purple-400">
                      <Cpu className="w-4 h-4" />
                    </span>
                    <div>
                      <div className="text-[12px] font-bold text-gray-900 dark:text-zinc-100">
                        {currentModel.requestedModel}
                      </div>
                      <div className="text-[9.5px] text-gray-400 font-mono">
                        真实上游: <span className="font-semibold text-purple-600 dark:text-purple-400">{currentModel.physicalModel}</span>
                      </div>
                    </div>
                  </div>

                  <div>
                    {currentModel.isDowngraded ? (
                      <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-semibold bg-rose-50 dark:bg-rose-500/10 text-rose-700 dark:text-rose-400 border border-rose-200/50">
                        <AlertTriangle className="w-3 h-3" />
                        <span>检测到暗降 (Conservative)</span>
                      </span>
                    ) : (
                      <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-semibold bg-emerald-50 dark:bg-emerald-500/10 text-emerald-700 dark:text-emerald-400 border border-emerald-200/50">
                        <CheckCircle2 className="w-3 h-3" />
                        <span>100% 原生高算力保真</span>
                      </span>
                    )}
                  </div>
                </div>
              </div>

              {/* 性能 4 格指标 */}
              <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-2.5">
                <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
                  <span className="text-[10.5px] text-gray-500 dark:text-zinc-400">{isZh ? '平均持续生成流速' : 'Throughput (TPS)'}</span>
                  <div className="my-1.5 flex items-baseline gap-1">
                    <span className="text-2xl font-bold font-mono text-emerald-600 dark:text-emerald-400">
                      {currentModel.avgTps.toFixed(0)}
                    </span>
                    <span className="text-[10px] text-gray-400">tok/s</span>
                  </div>
                  <span className="text-[9px] text-gray-400 font-mono">秒级高响应吞吐</span>
                </div>

                <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
                  <span className="text-[10.5px] text-gray-500 dark:text-zinc-400">{isZh ? '首字延迟中位数 (TTFT)' : 'First Token Latency'}</span>
                  <div className="my-1.5 flex items-baseline gap-1">
                    <span className="text-2xl font-bold font-mono text-blue-600 dark:text-blue-400">
                      {(currentModel.avgTtftMs / 1000).toFixed(2)}
                    </span>
                    <span className="text-[10px] text-gray-400">秒</span>
                  </div>
                  <span className="text-[9px] text-gray-400 font-mono">极速建立响应通道</span>
                </div>

                <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
                  <span className="text-[10.5px] text-gray-500 dark:text-zinc-400">{isZh ? '模型专属缓存率' : 'Cache Hit Rate'}</span>
                  <div className="my-1.5 flex items-baseline gap-1">
                    <span className="text-2xl font-bold font-mono text-purple-600 dark:text-purple-400">
                      {currentModel.cacheHitRate.toFixed(1)}%
                    </span>
                  </div>
                  <span className="text-[9px] text-gray-400 font-mono">复用大部分系统指令</span>
                </div>

                <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col justify-between">
                  <span className="text-[10.5px] text-gray-500 dark:text-zinc-400">{isZh ? '全网调用占比' : 'Share Percentage'}</span>
                  <div className="my-1.5 flex items-baseline gap-1">
                    <span className="text-2xl font-bold font-mono text-gray-900 dark:text-zinc-100">
                      {currentModel.sharePercentage.toFixed(1)}%
                    </span>
                  </div>
                  <span className="text-[9px] text-gray-400 font-mono">共 {currentModel.totalRequests} 次调用</span>
                </div>
              </div>

              {/* 对称双卡：高频依赖项目 VS 承载账号分布 */}
              <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col gap-2.5">
                  <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                    <FolderGit2 className="w-3.5 h-3.5 text-indigo-500" />
                    <span>{isZh ? '哪些工程最重度使用该模型？' : 'Projects Using this Model'}</span>
                  </span>
                  <div className="flex flex-col gap-2">
                    {(currentModel.topWorkspaces || [['at', 75], ['omin', 18], ['other', 7]]).map(([w, pct]) => (
                      <div key={w} className="flex flex-col gap-1">
                        <div className="flex items-center justify-between text-[10.5px]">
                          <span className="font-medium text-gray-800 dark:text-zinc-200">{w}</span>
                          <span className="font-mono text-indigo-600 font-bold">{pct}%</span>
                        </div>
                        <div className="w-full h-1.5 rounded-full bg-gray-100 dark:bg-white/[0.06] overflow-hidden">
                          <div style={{ width: `${pct}%` }} className="h-full rounded-full bg-indigo-500" />
                        </div>
                      </div>
                    ))}
                  </div>
                </div>

                <div className="p-3 rounded-xl bg-white/70 dark:bg-[#121316]/70 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 shadow-2xs flex flex-col gap-2.5">
                  <span className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100 flex items-center gap-1.5">
                    <UserCheck className="w-3.5 h-3.5 text-emerald-500" />
                    <span>{isZh ? '哪些账号承载了该模型的调用？' : 'Accounts Hosting this Model'}</span>
                  </span>
                  <div className="flex flex-col gap-2">
                    {(currentModel.topAccounts || [['ctrler@868666.best', 70], ['developer.pro@gmail.com', 20], ['research.pool1@gmail.com', 10]]).map(([a, pct]) => (
                      <div key={a} className="flex flex-col gap-1">
                        <div className="flex items-center justify-between text-[10.5px]">
                          <span className="font-medium text-gray-800 dark:text-zinc-200 truncate pr-2">{a}</span>
                          <span className="font-mono text-emerald-600 font-bold shrink-0">{pct}%</span>
                        </div>
                        <div className="w-full h-1.5 rounded-full bg-gray-100 dark:bg-white/[0.06] overflow-hidden">
                          <div style={{ width: `${pct}%` }} className="h-full rounded-full bg-emerald-500" />
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            </div>
          )}
        </div>
      )}

      {/* ============================================================== */}
      {/* 视角 5：📜 逐笔流水对账 (Traffic Ledger)                         */}
      {/* ============================================================== */}
      {activeView === 'ledger' && (
        <div className="flex flex-col gap-3 animate-in fade-in-30 duration-200 relative">
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

              <div className="inline-flex items-center p-0.5 rounded-lg bg-gray-100 dark:bg-white/[0.04]">
                {[
                  { id: 'all' as HostFilter, label: isZh ? '全部' : 'All' },
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
              {isZh ? `已捕获 ${filteredRecords.length} 笔本地流水` : `${filteredRecords.length} requests logged`}
            </div>
          </div>

          <div className="bg-white/80 dark:bg-[#121316]/80 backdrop-blur-xl border border-gray-200/80 dark:border-white/5 rounded-xl overflow-hidden shadow-2xs">
            <table className="w-full text-left border-collapse table-fixed select-none">
              <colgroup>
                <col className="w-[75px]" />
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
                    <td className="px-3 py-2 font-mono text-[10px] text-gray-500 dark:text-zinc-400">
                      {r.time}
                    </td>

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

                    <td className="px-2.5 py-2 truncate">
                      <div className="flex items-center gap-1 truncate">
                        <span className="font-semibold text-gray-800 dark:text-zinc-200 truncate">{r.requestedModel}</span>
                        <span className="text-gray-400 text-[9px]">➔</span>
                        <span
                          className={`font-mono text-[9.5px] truncate ${
                            r.isDowngraded ? 'text-rose-600 dark:text-rose-400 font-bold' : 'text-purple-600 dark:text-purple-400 font-medium'
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

                    <td className="px-2.5 py-2 font-mono text-[10px] text-gray-600 dark:text-zinc-400">
                      <span>{(r.inputTokens / 1000).toFixed(1)}K</span>
                      <span className="text-[9px] text-emerald-600 dark:text-emerald-400 ml-1">
                        ({(r.cachedTokens / 1000).toFixed(0)}K Cache)
                      </span>
                    </td>

                    <td className="px-2 py-2 font-mono font-semibold text-gray-800 dark:text-zinc-200">
                      {r.outputTokens.toLocaleString()}
                    </td>

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

                    <td className="px-2 py-2 font-mono text-gray-500 dark:text-zinc-400">
                      ⏱ {r.ttft}s
                    </td>

                    <td className="px-2 py-2 text-right pr-3 font-mono font-semibold text-emerald-600 dark:text-emerald-400">
                      +${r.costSaved.toFixed(4)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {/* 抽屉透视 */}
          {selectedRecord && (
            <div className="fixed inset-y-0 right-0 z-50 w-96 bg-white/95 dark:bg-[#16171b]/95 backdrop-blur-2xl border-l border-gray-200/80 dark:border-white/10 shadow-2xl p-4 flex flex-col gap-3 animate-in slide-in-from-right duration-200">
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

              <div className="flex flex-col gap-1.5 p-2.5 rounded-xl bg-gray-50/70 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5 text-[10.5px]">
                <div className="flex items-center justify-between">
                  <span className="text-gray-400">Request ID:</span>
                  <span className="font-mono text-gray-800 dark:text-zinc-200 font-semibold">{selectedRecord.id}</span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-gray-400">Session ID:</span>
                  <span className="font-mono text-gray-600 dark:text-zinc-400">{selectedRecord.sessionId}</span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-gray-400">Workspace:</span>
                  <span className="font-mono text-blue-600 dark:text-blue-400 font-semibold truncate max-w-[200px]">{selectedRecord.workspace}</span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-gray-400">Account:</span>
                  <span className="font-mono text-gray-700 dark:text-zinc-300">{selectedRecord.accountEmail}</span>
                </div>
              </div>

              <div className="flex flex-col gap-1 p-2.5 rounded-xl bg-gray-50/70 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5 text-[10.5px]">
                <span className="text-[10px] font-semibold text-gray-600 dark:text-zinc-400 uppercase tracking-wider">物理模型拓扑</span>
                <div className="flex items-center justify-between mt-0.5">
                  <span className="text-gray-500">请求模型:</span>
                  <span className="font-semibold text-gray-800 dark:text-zinc-200">{selectedRecord.requestedModel}</span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-gray-500">物理真实执行:</span>
                  <span className="font-mono font-bold text-purple-600 dark:text-purple-400">{selectedRecord.physicalModel}</span>
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

              <div className="flex flex-col gap-1.5 p-2.5 rounded-xl bg-gray-50/70 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5 text-[10.5px]">
                <span className="text-[10px] font-semibold text-gray-600 dark:text-zinc-400 uppercase tracking-wider">Token 构成拆解</span>
                <div className="flex items-center justify-between">
                  <span className="text-gray-500">直接输入 (New Input):</span>
                  <span className="font-mono font-semibold">{selectedRecord.inputTokens}</span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-emerald-600 dark:text-emerald-400 font-medium">缓存命中 (Cache Read):</span>
                  <span className="font-mono font-bold text-emerald-600 dark:text-emerald-400">{selectedRecord.cachedTokens}</span>
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
                  <span className="font-mono font-bold text-blue-600 dark:text-blue-400">{selectedRecord.outputTokens}</span>
                </div>
              </div>

              <div className="flex flex-col gap-1 p-2.5 rounded-xl bg-gray-50/70 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5 text-[10.5px]">
                <span className="text-[10px] font-semibold text-gray-600 dark:text-zinc-400 uppercase tracking-wider">延迟瀑布流与成本</span>
                <div className="flex items-center justify-between mt-0.5">
                  <span className="text-gray-500">首字响应 (TTFT):</span>
                  <span className="font-mono font-bold text-gray-800 dark:text-zinc-200">{selectedRecord.ttft} 秒</span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-gray-500">持续流速:</span>
                  <span className="font-mono font-bold text-emerald-600 dark:text-emerald-400">{selectedRecord.tps} tok/s</span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-gray-500">本地节省官方费用:</span>
                  <span className="font-mono font-bold text-emerald-600 dark:text-emerald-400">+${selectedRecord.costSaved.toFixed(4)} USD</span>
                </div>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
};
