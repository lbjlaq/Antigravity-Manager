// File: src/pages/logs/ui/LogsHeader.tsx
// Logs page header component

import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { ScrollText, Circle, RefreshCw } from 'lucide-react';
import { motion } from 'framer-motion';

import { formatCompactNumber } from '@/shared/lib';
import type { ProxyStats } from '../model';

interface LogsHeaderProps {
  stats: ProxyStats;
  isLoggingEnabled: boolean;
  isAutoRefresh: boolean;
  onToggleLogging: () => void;
  onToggleAutoRefresh: () => void;
}

export const LogsHeader = memo(function LogsHeader({
  stats,
  isLoggingEnabled,
  isAutoRefresh,
  onToggleLogging,
  onToggleAutoRefresh,
}: LogsHeaderProps) {
  const { t } = useTranslation();

  return (
    <div className="px-5 py-4 border-b border-zinc-200 dark:border-zinc-800">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="p-2 bg-purple-100 dark:bg-purple-900/30 rounded-lg">
            <ScrollText className="w-5 h-5 text-purple-600 dark:text-purple-400" />
          </div>
          <div>
            <h1 className="text-lg font-semibold text-zinc-900 dark:text-zinc-100">
              {t('logs.title', 'Traffic Logs')}
            </h1>
            <p className="text-sm text-zinc-500 dark:text-zinc-400">
              {t('logs.subtitle', 'Monitor API requests and responses')}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-3">
          {/* Stats badges */}
          <div className="hidden md:flex items-center gap-2 text-xs font-medium">
            <span className="px-2.5 py-1 rounded-full bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400">
              {formatCompactNumber(stats.total_requests)} {t('logs.stats.total', 'Total')}
            </span>
            <span className="px-2.5 py-1 rounded-full bg-green-100 dark:bg-green-900/30 text-green-600 dark:text-green-400">
              {formatCompactNumber(stats.success_count)} {t('logs.stats.success', 'OK')}
            </span>
            <span className="px-2.5 py-1 rounded-full bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400">
              {formatCompactNumber(stats.error_count)} {t('logs.stats.errors', 'Errors')}
            </span>
          </div>

          {/* Auto-refresh toggle */}
          <motion.button
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.98 }}
            onClick={onToggleAutoRefresh}
            className={`
              flex items-center gap-2 px-3 py-2 rounded-lg font-medium text-sm
              transition-colors
              ${isAutoRefresh
                ? 'bg-green-100 dark:bg-green-900/30 text-green-600 dark:text-green-400'
                : 'bg-zinc-100 dark:bg-zinc-800 text-zinc-500 dark:text-zinc-400'
              }
            `}
            title={isAutoRefresh ? t('logs.auto_refresh.on', 'Auto-refresh ON') : t('logs.auto_refresh.off', 'Auto-refresh OFF')}
          >
            <RefreshCw className={`w-4 h-4 ${isAutoRefresh ? 'animate-spin' : ''}`} style={{ animationDuration: '3s' }} />
            <span className="hidden sm:inline">{t('logs.auto_refresh.label', 'Auto')}</span>
          </motion.button>

          {/* Logging toggle button */}
          <motion.button
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.98 }}
            onClick={onToggleLogging}
            className={`
              flex items-center gap-2 px-4 py-2 rounded-lg font-medium text-sm
              transition-colors
              ${isLoggingEnabled
                ? 'bg-red-500 text-white hover:bg-red-600'
                : 'bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 hover:bg-zinc-200 dark:hover:bg-zinc-700'
              }
            `}
          >
            <Circle
              className={`w-2.5 h-2.5 ${isLoggingEnabled ? 'fill-current animate-pulse' : ''}`}
            />
            {isLoggingEnabled
              ? t('logs.logging.active', 'Recording')
              : t('logs.logging.paused', 'Paused')
            }
          </motion.button>
        </div>
      </div>
    </div>
  );
});
