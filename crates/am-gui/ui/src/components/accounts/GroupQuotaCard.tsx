import React from 'react';
import { Clock, Sparkles } from 'lucide-react';

export interface QuotaPeriod {
  percentage: number;
  resetText: string;
}

export interface ModelGroupSummary {
  key: 'gemini' | 'claude' | 'argon';
  name: string;
  fiveHour?: QuotaPeriod;
  sevenDay?: QuotaPeriod;
}

interface GroupQuotaCardProps {
  summary: ModelGroupSummary;
}

export const GroupQuotaCard: React.FC<GroupQuotaCardProps> = ({ summary }) => {
  const { fiveHour, sevenDay, name, key } = summary;

  const isClaude = key === 'claude';
  const isArgon = key === 'argon';

  const fiveHourPct = fiveHour?.percentage ?? 0;
  const sevenDayPct = sevenDay?.percentage ?? 0;

  const getProgressColor = (pct: number, is7d = false) => {
    if (pct < 20) return 'bg-rose-500';
    if (pct < 50) return 'bg-amber-500';
    if (isClaude) return is7d ? 'bg-indigo-500' : 'bg-purple-500';
    if (isArgon) return is7d ? 'bg-amber-500' : 'bg-orange-500';
    return is7d ? 'bg-cyan-500' : 'bg-blue-500';
  };

  const getTextColor = (pct: number) => {
    if (pct < 20) return 'text-rose-600 dark:text-rose-400 font-bold';
    if (pct < 50) return 'text-amber-600 dark:text-amber-400 font-bold';
    return 'text-gray-700 dark:text-zinc-200 font-bold';
  };

  return (
    <div className="flex flex-col gap-1 w-full min-w-[170px]">
      {/* 5小时配额胶囊条 */}
      {fiveHour !== undefined && (
        <div
          className="relative h-[22px] flex items-center px-2 rounded-md overflow-hidden border border-gray-100/60 dark:border-white/5 bg-gray-50/50 dark:bg-zinc-800/40 text-[10px] font-mono leading-none"
          title={`${name} (5H限额: ${fiveHourPct}% · 重置: ${fiveHour.resetText})`}
        >
          {/* 背景填充进度条 */}
          <div className="absolute inset-0 bg-transparent">
            <div
              className={`h-full opacity-20 ${getProgressColor(fiveHourPct)} transition-all duration-300`}
              style={{ width: `${fiveHourPct}%` }}
            />
          </div>

          {/* 前台文本内容 */}
          <div className="relative z-10 w-full flex items-center justify-between gap-1">
            <div className="flex items-center gap-1 truncate text-gray-700 dark:text-zinc-200 font-medium">
              <Sparkles className="w-2.5 h-2.5 shrink-0 opacity-70" />
              <span className="truncate">{name}</span>
            </div>

            <div className="flex items-center gap-1.5 shrink-0">
              <span className="text-[9px] text-gray-400 dark:text-zinc-500 flex items-center gap-0.5">
                <Clock className="w-2.5 h-2.5" />
                {fiveHour.resetText}
              </span>
              <span className={`w-[28px] text-right ${getTextColor(fiveHourPct)}`}>
                {fiveHourPct}%
              </span>
            </div>
          </div>
        </div>
      )}

      {/* 周配额胶囊条 */}
      {sevenDay !== undefined && (
        <div
          className="relative h-[22px] flex items-center px-2 rounded-md overflow-hidden border border-gray-100/60 dark:border-white/5 bg-gray-50/50 dark:bg-zinc-800/40 text-[10px] font-mono leading-none"
          title={`${name} (周限额: ${sevenDayPct}% · 重置: ${sevenDay.resetText})`}
        >
          {/* 背景填充进度条 */}
          <div className="absolute inset-0 bg-transparent">
            <div
              className={`h-full opacity-20 ${getProgressColor(sevenDayPct, true)} transition-all duration-300`}
              style={{ width: `${sevenDayPct}%` }}
            />
          </div>

          {/* 前台文本内容 */}
          <div className="relative z-10 w-full flex items-center justify-between gap-1">
            <div className="flex items-center gap-1 truncate text-gray-700 dark:text-zinc-200 font-medium">
              <span className="w-1.5 h-1.5 rounded-full bg-cyan-500/70 shrink-0" />
              <span className="truncate">{name}</span>
              <span className="text-[9px] px-1 py-0.2 rounded bg-gray-200/50 dark:bg-zinc-700/50 text-gray-500 dark:text-zinc-400">
                周
              </span>
            </div>

            <div className="flex items-center gap-1.5 shrink-0">
              <span className="text-[9px] text-gray-400 dark:text-zinc-500 flex items-center gap-0.5">
                <Clock className="w-2.5 h-2.5" />
                {sevenDay.resetText}
              </span>
              <span className={`w-[28px] text-right ${getTextColor(sevenDayPct)}`}>
                {sevenDayPct}%
              </span>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
