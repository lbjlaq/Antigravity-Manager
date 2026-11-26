import { BackendAccount, QuotaBucket } from '../api';

export type QuotaWindow = '5h' | '7d';

export type QuotaPaceStatus = 'safe' | 'warn' | 'fast';

export interface PoolWindowQuota {
  window: QuotaWindow;
  percentage: number;
  resetText: string;
  /** 距离周期重置的剩余时间占比 (0..100)，用于在配额条上渲染 Apple 风格 1px 节奏标尺 */
  timeRemainingPct?: number;
  /** 基于官方剩余配额 % vs 周期剩余时间 % 的 100% 精确节奏状态 */
  paceStatus?: QuotaPaceStatus;
}

export interface ParsedPoolQuota {
  fiveHour?: PoolWindowQuota;
  sevenDay?: PoolWindowQuota;
  /** 真实可用水位（各有效窗口中的最小值，用于候选排序与摘要展示） */
  effectivePct: number;
  hasData: boolean;
}

/**
 * 格式化单个配额桶的剩余重置倒计时（始终输出标准倒计时格式）：
 * 1) 优先依据真实 ISO reset_time 计算实时倒计时（确保主窗口与托盘小窗时间完全同步并实时递减）
 * 2) 若无 reset_time（例如满额未消耗或仅带 description），尝试解析 description
 * 3) 若仍无未来倒计时（满额未触发计时或缓存窗口已跨周期），展示对应周期基准倒计时 (4h 59m / 6d 23h)
 */
export function formatBucketReset(
  isoTime?: string,
  desc?: string,
  _percentage?: number,
  is5h: boolean = false
): string {
  const baseline = is5h ? '4h 59m' : '6d 23h';

  if (isoTime) {
    const target = Date.parse(isoTime);
    if (!Number.isNaN(target)) {
      const diff = target - Date.now();
      if (diff > 0) {
        // 若是 5h 轨，但模型 reset_time 超过 24 小时（长周期模型），回退为 5h 基准倒计时
        if (is5h && diff > 24 * 60 * 60 * 1000) {
          return baseline;
        }
        const totalMins = Math.floor(diff / 60000);
        const days = Math.floor(totalMins / 1440);
        const hours = Math.floor((totalMins % 1440) / 60);
        const mins = totalMins % 60;
        if (days > 0) {
          return `${days}d ${hours}h`;
        }
        if (hours > 0) {
          return `${hours}h ${mins}m`;
        }
        if (mins > 0) {
          return `${mins}m`;
        }
        return '<1m';
      }
      // 缓存中的 reset_time 已跨过当前时间，回退周期基准倒计时（同时由后台刷新拉取真实新周期）
      return baseline;
    }
  }

  if (desc) {
    const m = desc.match(/refresh(?:es)? in ([^.]+)/i);
    if (m) {
      let t = m[1].trim();
      t = t.replace(/\s*days?/gi, 'd');
      t = t.replace(/\s*hours?/gi, 'h');
      t = t.replace(/\s*minutes?/gi, 'm');
      t = t.replace(/,/g, '');
      return t;
    }
  }

  return baseline;
}

/**
 * 判断账号缓存配额是否已过期（任意 bucket 的 reset_time 已过当前时间，或距上次更新超过 maxAgeMs）
 */
export function isAccountQuotaStale(
  acc: BackendAccount | null | undefined,
  maxAgeMs: number = 5 * 60 * 1000
): boolean {
  if (!acc?.id || !acc.token?.refresh_token) return false;
  if (!acc.quota) return true;
  const now = Date.now();
  const lastUpdatedMs = (acc.quota.last_updated || 0) * 1000;
  if (lastUpdatedMs <= 0 || now - lastUpdatedMs > maxAgeMs) return true;
  for (const g of acc.quota.quota_groups || []) {
    for (const b of g.buckets || []) {
      if (b.reset_time) {
        const ts = Date.parse(b.reset_time);
        if (!Number.isNaN(ts) && ts <= now) return true;
      }
    }
  }
  return false;
}

function buildWindowQuota(bucket: QuotaBucket, window: QuotaWindow): PoolWindowQuota {
  const pct = Math.max(0, Math.min(100, Math.round((bucket.remaining_fraction ?? 1) * 100)));
  const resetText = formatBucketReset(bucket.reset_time, bucket.description, pct, window === '5h');

  let timeRemainingPct: number | undefined;
  let paceStatus: QuotaPaceStatus | undefined;

  if (bucket.reset_time) {
    const target = Date.parse(bucket.reset_time);
    if (!Number.isNaN(target)) {
      const diff = target - Date.now();
      const windowMs = window === '5h' ? 5 * 3600_000 : 7 * 86400_000;
      if (diff > windowMs * 0.02 && diff < windowMs * 0.98) {
        const remPct = Math.round((diff / windowMs) * 100);
        timeRemainingPct = remPct;
        const delta = pct - remPct;
        paceStatus = delta >= -3 ? 'safe' : delta >= -18 ? 'warn' : 'fast';
      }
    }
  }

  return {
    window,
    percentage: pct,
    resetText,
    timeRemainingPct,
    paceStatus,
  };
}

function findBucket(buckets: QuotaBucket[], window: QuotaWindow): QuotaBucket | undefined {
  if (window === '5h') {
    return buckets.find((b) => {
      const w = (b.window || '').toLowerCase();
      const id = (b.bucket_id || '').toLowerCase();
      return w === '5h' || id.endsWith('-5h') || id.includes('5h');
    });
  }
  return buckets.find((b) => {
    const w = (b.window || '').toLowerCase();
    const id = (b.bucket_id || '').toLowerCase();
    return w === 'weekly' || w === '7d' || id.endsWith('-weekly') || id.includes('weekly');
  });
}

/**
 * 统一解析账号的 Claude / Gemini 池配额（主工作台与托盘小窗单一数据源）：
 * - 仅在 display_name 上排除 argon，避免 description 列出 "Gemini 4 Argon" 时误杀整个 Gemini Models 组
 * - 严格按真实存在的 bucket 透出 5h 与 7d，不伪造不存在的 100% 5h 轨
 * - effectivePct 取已存在窗口的最小值，真实反映瓶颈水位
 */
export function extractAccountPoolQuota(
  acc: BackendAccount | null | undefined,
  pool: 'claude' | 'gemini'
): ParsedPoolQuota {
  if (!acc?.quota) {
    return { effectivePct: 0, hasData: false };
  }

  const groups = acc.quota.quota_groups;
  if (groups && groups.length > 0) {
    const group = groups.find((g) => {
      const name = (g.display_name || '').toLowerCase();
      const desc = (g.description || '').toLowerCase();
      if (pool === 'claude') {
        return (
          name.includes('claude') ||
          desc.includes('claude') ||
          name.includes('3p') ||
          desc.includes('3p')
        );
      }
      return (name.includes('gemini') || desc.includes('gemini')) && !name.includes('argon');
    });

    if (group && Array.isArray(group.buckets) && group.buckets.length > 0) {
      const b5 = findBucket(group.buckets, '5h');
      const b7 = findBucket(group.buckets, '7d');

      let fiveHour = b5 ? buildWindowQuota(b5, '5h') : undefined;
      let sevenDay = b7 ? buildWindowQuota(b7, '7d') : undefined;

      // 兼容未知窗口命名：若 buckets 非空但未命中 5h/weekly，根据 reset_time 跨度归类
      if (!fiveHour && !sevenDay) {
        const first = group.buckets[0];
        const ts = first.reset_time ? Date.parse(first.reset_time) : NaN;
        const isLong = !Number.isNaN(ts) && ts - Date.now() > 24 * 3600 * 1000;
        if (isLong) {
          sevenDay = buildWindowQuota(first, '7d');
        } else {
          fiveHour = buildWindowQuota(first, '5h');
        }
      }

      const pcts: number[] = [];
      if (fiveHour) pcts.push(fiveHour.percentage);
      if (sevenDay) pcts.push(sevenDay.percentage);
      const effectivePct = pcts.length > 0 ? Math.min(...pcts) : 0;

      return {
        fiveHour,
        sevenDay,
        effectivePct,
        hasData: pcts.length > 0,
      };
    }
  }

  // Fallback：旧格式账号无 quota_groups 时，从 models 与聚合百分比推导
  const models = acc.quota.models || [];
  const poolModels = models.filter((m) => {
    const lower = m.name.toLowerCase();
    return pool === 'claude' ? lower.includes('claude') || lower.includes('gpt') : lower.includes('gemini');
  });

  const fallbackPct =
    pool === 'claude'
      ? acc.quota.claude_percentage ?? (poolModels.length > 0 ? poolModels[0].percentage : undefined)
      : acc.quota.gemini_percentage ?? (poolModels.length > 0 ? poolModels[0].percentage : undefined);

  if (fallbackPct === undefined && poolModels.length === 0) {
    return { effectivePct: 0, hasData: false };
  }

  const now = Date.now();
  const m5h = poolModels.find((m) => {
    if (!m.reset_time) return false;
    const diff = Date.parse(m.reset_time) - now;
    return !Number.isNaN(diff) && diff > 0 && diff <= 24 * 3600 * 1000;
  });
  const m7d = poolModels.find((m) => {
    if (!m.reset_time) return false;
    const diff = Date.parse(m.reset_time) - now;
    return !Number.isNaN(diff) && diff > 24 * 3600 * 1000;
  });

  let fiveHour: PoolWindowQuota | undefined;
  let sevenDay: PoolWindowQuota | undefined;

  if (m5h) {
    const pct = Math.max(0, Math.min(100, Math.round(m5h.percentage ?? fallbackPct ?? 0)));
    fiveHour = {
      window: '5h',
      percentage: pct,
      resetText: formatBucketReset(m5h.reset_time, undefined, pct, true),
    };
  }
  if (m7d) {
    const pct = Math.max(0, Math.min(100, Math.round(m7d.percentage ?? fallbackPct ?? 0)));
    sevenDay = {
      window: '7d',
      percentage: pct,
      resetText: formatBucketReset(m7d.reset_time, undefined, pct, false),
    };
  }

  if (!fiveHour && !sevenDay && fallbackPct !== undefined) {
    const pct = Math.max(0, Math.min(100, Math.round(fallbackPct)));
    const anyResetModel = poolModels.find((m) => m.reset_time);
    sevenDay = {
      window: '7d',
      percentage: pct,
      resetText: formatBucketReset(anyResetModel?.reset_time, undefined, pct, false),
    };
  }

  const pcts: number[] = [];
  if (fiveHour) pcts.push(fiveHour.percentage);
  if (sevenDay) pcts.push(sevenDay.percentage);

  return {
    fiveHour,
    sevenDay,
    effectivePct: pcts.length > 0 ? Math.min(...pcts) : 0,
    hasData: pcts.length > 0,
  };
}
