// File: src/pages/logs/ui/LogsTable.tsx
// Logs table component - balanced design

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { ScrollText } from 'lucide-react';

import { formatCompactNumber } from '@/shared/lib';
import type { ProxyRequestLog } from '../model';

interface LogsTableProps {
  logs: ProxyRequestLog[];
  loading: boolean;
  onLogClick: (log: ProxyRequestLog) => void;
}

export const LogsTable = memo(function LogsTable({
  logs,
  loading,
  onLogClick,
}: LogsTableProps) {
  const { t } = useTranslation();

  const getStatusStyle = (status: number) => {
    if (status >= 200 && status < 300) return 'bg-green-500 text-white';
    if (status >= 300 && status < 400) return 'bg-yellow-500 text-white';
    return 'bg-red-500 text-white';
  };

  const getProtocolStyle = (protocol?: string) => {
    const styles: Record<string, string> = {
      openai: 'bg-emerald-100 dark:bg-emerald-900/40 text-emerald-700 dark:text-emerald-300',
      anthropic: 'bg-orange-100 dark:bg-orange-900/40 text-orange-700 dark:text-orange-300',
      gemini: 'bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300',
    };
    return styles[protocol || ''] || 'bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400';
  };

  const getProtocolLabel = (protocol?: string) => {
    const labels: Record<string, string> = {
      openai: 'OpenAI',
      anthropic: 'Claude',
      gemini: 'Gemini',
    };
    return labels[protocol || ''] || protocol || '-';
  };

  // Loading skeleton
  if (loading && logs.length === 0) {
    return (
      <div className="p-3 space-y-2">
        {[...Array(10)].map((_, i) => (
          <div key={i} className="h-11 bg-zinc-100 dark:bg-zinc-800 rounded-lg animate-pulse" />
        ))}
      </div>
    );
  }

  // Empty state
  if (!loading && logs.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-zinc-400">
        <ScrollText className="w-12 h-12 mb-4 opacity-40" />
        <p className="text-sm font-medium">{t('logs.empty', 'No logs found')}</p>
        <p className="text-xs mt-1 text-zinc-400">{t('logs.empty_hint', 'Logs will appear here when requests are made')}</p>
      </div>
    );
  }

  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead className="bg-zinc-50/80 dark:bg-zinc-800/50 sticky top-0 z-10 backdrop-blur-sm">
          <tr className="text-xs text-zinc-500 dark:text-zinc-400 uppercase tracking-wide border-b border-zinc-200 dark:border-zinc-700">
            <th className="px-3 py-2.5 text-left font-semibold">Status</th>
            <th className="px-3 py-2.5 text-left font-semibold">Protocol</th>
            <th className="px-3 py-2.5 text-left font-semibold">Model</th>
            <th className="px-3 py-2.5 text-left font-semibold">Account</th>
            <th className="px-3 py-2.5 text-right font-semibold">In/Out</th>
            <th className="px-3 py-2.5 text-right font-semibold">Duration</th>
            <th className="px-3 py-2.5 text-right font-semibold">Time</th>
          </tr>
        </thead>
        <tbody>
          {logs.map((log, index) => (
            <tr
              key={log.id}
              onClick={() => onLogClick(log)}
              className={`
                cursor-pointer transition-all duration-150
                hover:bg-purple-50 dark:hover:bg-purple-900/20
                ${index % 2 === 0 ? 'bg-white dark:bg-zinc-900' : 'bg-zinc-50/50 dark:bg-zinc-800/20'}
              `}
            >
              {/* Status */}
              <td className="px-3 py-2.5">
                <span className={`inline-flex items-center justify-center min-w-[42px] px-2 py-1 text-xs font-bold rounded-md ${getStatusStyle(log.status)}`}>
                  {log.status}
                </span>
              </td>

              {/* Protocol */}
              <td className="px-3 py-2.5">
                {log.protocol ? (
                  <span className={`inline-flex px-2 py-0.5 text-xs font-medium rounded-md ${getProtocolStyle(log.protocol)}`}>
                    {getProtocolLabel(log.protocol)}
                  </span>
                ) : (
                  <span className="text-zinc-400">-</span>
                )}
              </td>

              {/* Model */}
              <td className="px-3 py-2.5">
                <div className="flex items-center gap-1.5 font-mono text-sm">
                  <span className="text-purple-600 dark:text-purple-400 font-medium truncate max-w-[180px]">
                    {log.model || '-'}
                  </span>
                  {log.mapped_model && log.model !== log.mapped_model && (
                    <>
                      <span className="text-zinc-400">→</span>
                      <span className="text-emerald-600 dark:text-emerald-400 truncate max-w-[120px]">
                        {log.mapped_model}
                      </span>
                    </>
                  )}
                </div>
              </td>

              {/* Account */}
              <td className="px-3 py-2.5">
                <span 
                  className="text-zinc-600 dark:text-zinc-400 truncate block max-w-[140px]" 
                  title={log.account_email}
                >
                  {log.account_email
                    ? log.account_email.replace(/(.{4}).*(@.*)/, '$1***$2')
                    : '-'}
                </span>
              </td>

              {/* Tokens In/Out */}
              <td className="px-3 py-2.5 text-right">
                {log.input_tokens != null || log.output_tokens != null ? (
                  <span className="font-mono text-xs">
                    <span className="text-blue-600 dark:text-blue-400">{formatCompactNumber(log.input_tokens ?? 0)}</span>
                    <span className="text-zinc-400 mx-1">/</span>
                    <span className="text-green-600 dark:text-green-400">{formatCompactNumber(log.output_tokens ?? 0)}</span>
                  </span>
                ) : (
                  <span className="text-zinc-400">-</span>
                )}
              </td>

              {/* Duration */}
              <td className="px-3 py-2.5 text-right">
                <span className={`font-mono text-xs tabular-nums ${
                  log.duration > 5000 ? 'text-red-500 font-semibold' :
                  log.duration > 2000 ? 'text-amber-500' :
                  'text-zinc-600 dark:text-zinc-400'
                }`}>
                  {log.duration >= 1000 ? `${(log.duration / 1000).toFixed(1)}s` : `${log.duration}ms`}
                </span>
              </td>

              {/* Time */}
              <td className="px-3 py-2.5 text-right">
                <span className="text-zinc-500 dark:text-zinc-400 text-xs tabular-nums">
                  {new Date(log.timestamp).toLocaleTimeString('en-US', { 
                    hour12: false, 
                    hour: '2-digit', 
                    minute: '2-digit',
                    second: '2-digit'
                  })}
                </span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
});
