import React from 'react';
import { tUi } from '../../i18n/ui';

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
  lang?: string;
}

export const GroupQuotaCard: React.FC<GroupQuotaCardProps> = ({ summary, lang = 'en' }) => {
  const { fiveHour, sevenDay, name, key } = summary;

  const isClaude = key === 'claude';
  const isArgon = key === 'argon';

  const fiveHourPct = fiveHour?.percentage ?? 100;
  const sevenDayPct = sevenDay?.percentage ?? 100;

  // 颜色定义
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
    <div className="flex flex-col gap-1 w-full min-w-[200px]">
      {/* 头部：模型组名称 + 5h / 7d 详细数字 */}
      <div className="flex items-center justify-between text-[11px] leading-tight">
        <span className="font-semibold text-gray-700 dark:text-zinc-300 flex items-center gap-1">
          <span
            className={`w-1.5 h-1.5 rounded-full ${
              isClaude ? 'bg-purple-500' : isArgon ? 'bg-amber-500' : 'bg-blue-500'
            }`}
          />
          <span className="truncate">{name}</span>
        </span>

        {/* 5h 与 7d 的详细数据指标 */}
        <div className="flex items-center gap-2.5 text-[10px] text-gray-500 dark:text-zinc-400 font-mono">
          {fiveHour && (
            <span>
              5h:{' '}
              <b
                className={
                  fiveHourPct < 25
                    ? 'text-rose-500 font-bold'
                    : fiveHourPct < 50
                    ? 'text-amber-500 font-bold'
                    : 'text-gray-700 dark:text-zinc-200'
                }
              >
                {fiveHourPct}%
              </b>{' '}
              <span className="opacity-75">({fiveHour.resetText})</span>
            </span>
          )}
          {sevenDay && (
            <span>
              7d:{' '}
              <b
                className={
                  sevenDayPct < 25
                    ? 'text-rose-500 font-bold'
                    : sevenDayPct < 50
                    ? 'text-amber-500 font-bold'
                    : 'text-gray-700 dark:text-zinc-200'
                }
              >
                {sevenDayPct}%
              </b>{' '}
              <span className="opacity-75">({sevenDay.resetText})</span>
            </span>
          )}
        </div>
      </div>

      {/* 统一融合进度条 (双时间周期合一) */}
      <div className="w-full h-2 bg-gray-100 dark:bg-zinc-800 rounded-full overflow-hidden flex gap-0.5 p-0.5">
        {fiveHour && (
          <div
            style={{ width: `${fiveHourPct}%` }}
            className={`h-full rounded-full transition-all duration-300 ${getPrimaryColor(fiveHourPct)}`}
            title={tUi(lang, 'quota_5h_tooltip', { pct: fiveHourPct })}
          />
        )}
        {sevenDay && (
          <div
            style={{ width: `${sevenDayPct}%` }}
            className={`h-full rounded-full transition-all duration-300 ${getSecondaryColor(sevenDayPct)}`}
            title={tUi(lang, 'quota_7d_tooltip', { pct: sevenDayPct })}
          />
        )}
      </div>
    </div>
  );
};
