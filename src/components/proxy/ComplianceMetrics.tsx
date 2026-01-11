/**
 * Compliance Monitoring Dashboard Component (Story-003-12)
 *
 * Displays real-time compliance metrics, violation alerts, and histogram visualization
 * for Claude 4.5 Sonnet Thinking compliance monitoring.
 */

import React, { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { request as invoke } from '../../utils/request';
import { useTranslation } from 'react-i18next';
import { AlertCircle, CheckCircle, RefreshCw, Trash2, Download } from 'lucide-react';
import {
  DetailedViolationMetrics,
  AlertLevel,
  getAlertLevel,
  calculateComplianceScore,
  generateComplianceReport,
} from '../../types/compliance';

interface ComplianceMetricsProps {
  className?: string;
}

export const ComplianceMetrics: React.FC<ComplianceMetricsProps> = ({ className }) => {
  const { t } = useTranslation();
  const [metrics, setMetrics] = useState<DetailedViolationMetrics | null>(null);
  const [loading, setLoading] = useState(true);

  // Load metrics on mount
  useEffect(() => {
    loadMetrics();

    // Listen for violation events
    const setupListener = async () => {
      await listen<string>('proxy://violation', async (_event) => {
        // Reload metrics when violation occurs
        await loadMetrics();
      });
    };
    setupListener();
  }, []);

  const loadMetrics = async () => {
    try {
      setLoading(true);
      const data = await invoke<DetailedViolationMetrics>('get_violation_metrics');
      setMetrics(data);
    } catch (e) {
      console.error('Failed to load violation metrics:', e);
    } finally {
      setLoading(false);
    }
  };

  const resetMetrics = async () => {
    if (!confirm(t('compliance.confirm_reset'))) return;

    try {
      await invoke('reset_violation_metrics');
      await loadMetrics();
    } catch (e) {
      console.error('Failed to reset metrics:', e);
    }
  };

  const exportReport = async () => {
    if (!metrics) return;

    const report = generateComplianceReport(metrics);
    const blob = new Blob([report], { type: 'text/markdown' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `compliance-report-${Date.now()}.md`;
    a.click();
    URL.revokeObjectURL(url);
  };

  if (loading) {
    return (
      <div className={`flex items-center justify-center p-8 ${className}`}>
        <span className="loading loading-spinner loading-lg"></span>
      </div>
    );
  }

  if (!metrics) {
    return (
      <div className={`flex items-center justify-center p-8 ${className}`}>
        <p className="text-gray-500">{t('compliance.no_data')}</p>
      </div>
    );
  }

  const complianceScore = calculateComplianceScore(metrics);
  const budgetAlertLevel = getAlertLevel(metrics.rates.budget_violations_per_second);
  const positionAlertLevel = getAlertLevel(metrics.rates.position_violations_per_second);

  return (
    <div
      className={`bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200 ${className}`}
    >
      {/* Header */}
      <div className="p-4 border-b border-gray-100 dark:border-base-200 flex items-center justify-between">
        <h2 className="text-lg font-bold text-gray-900 dark:text-base-content">
          {t('compliance.title')}
        </h2>
        <div className="flex gap-2">
          <button
            onClick={loadMetrics}
            className="btn btn-sm btn-ghost"
            title={t('compliance.refresh')}
          >
            <RefreshCw size={16} />
          </button>
          <button
            onClick={resetMetrics}
            className="btn btn-sm btn-ghost"
            title={t('compliance.reset')}
          >
            <Trash2 size={16} />
          </button>
          <button
            onClick={exportReport}
            className="btn btn-sm btn-ghost"
            title={t('compliance.export')}
          >
            <Download size={16} />
          </button>
        </div>
      </div>

      {/* Compliance Score */}
      <div className="p-4 border-b border-gray-100 dark:border-base-200">
        <div className="flex items-center justify-between">
          <div>
            <div className="text-sm text-gray-500 dark:text-gray-400">
              {t('compliance.score')}
            </div>
            <div className="text-3xl font-bold text-green-600 dark:text-green-400">
              {complianceScore.toFixed(2)}%
            </div>
          </div>
          {complianceScore >= 99.5 ? (
            <CheckCircle size={48} className="text-green-500" />
          ) : (
            <AlertCircle size={48} className="text-yellow-500" />
          )}
        </div>
      </div>

      {/* Violation Cards */}
      <div className="p-4 grid grid-cols-2 gap-4">
        {/* Budget Violations */}
        <ViolationCard
          title={t('compliance.budget_violations')}
          count={metrics.stats.thinking_budget_violations}
          rate={metrics.rates.budget_violations_per_second}
          alertLevel={budgetAlertLevel}
        />

        {/* Position Violations */}
        <ViolationCard
          title={t('compliance.position_violations')}
          count={metrics.stats.thinking_position_violations}
          rate={metrics.rates.position_violations_per_second}
          alertLevel={positionAlertLevel}
          breakdown={{
            user: metrics.stats.thinking_position_violations_user,
            model: metrics.stats.thinking_position_violations_model,
          }}
        />
      </div>

      {/* Position Histogram */}
      {metrics.position_histogram.length > 0 && (
        <div className="p-4 border-t border-gray-100 dark:border-base-200">
          <h3 className="text-sm font-semibold mb-3 text-gray-700 dark:text-gray-300">
            {t('compliance.position_histogram')}
          </h3>
          <PositionHistogram data={metrics.position_histogram} />
        </div>
      )}

      {/* Alerts */}
      {(budgetAlertLevel !== 'GREEN' || positionAlertLevel !== 'GREEN') && (
        <div className="p-4 border-t border-gray-100 dark:border-base-200">
          <AlertPanel
            budgetLevel={budgetAlertLevel}
            positionLevel={positionAlertLevel}
            budgetRate={metrics.rates.budget_violations_per_second}
            positionRate={metrics.rates.position_violations_per_second}
          />
        </div>
      )}
    </div>
  );
};

// ==================================================================================
// SUB-COMPONENTS
// ==================================================================================

/**
 * ViolationCard - Displays violation count, rate, and alert status
 */
interface ViolationCardProps {
  title: string;
  count: number;
  rate: number;
  alertLevel: AlertLevel;
  breakdown?: {
    user: number;
    model: number;
  };
}

const ViolationCard: React.FC<ViolationCardProps> = ({
  title,
  count,
  rate,
  alertLevel,
  breakdown,
}) => {
  const { t } = useTranslation();

  const alertColors = {
    GREEN: 'bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400',
    YELLOW: 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900/30 dark:text-yellow-400',
    RED: 'bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-400',
  };

  return (
    <div className="border border-gray-200 dark:border-base-300 rounded-lg p-3">
      <div className="text-xs text-gray-500 dark:text-gray-400 mb-1">{title}</div>
      <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-2">
        {count.toLocaleString()}
      </div>

      {/* Rate with alert indicator */}
      <div className="flex items-center gap-2 mb-2">
        <span className="text-xs text-gray-600 dark:text-gray-400">
          {rate.toFixed(4)} /sec
        </span>
        <span className={`text-xs px-2 py-0.5 rounded-full ${alertColors[alertLevel]}`}>
          {alertLevel}
        </span>
      </div>

      {/* Breakdown (if provided) */}
      {breakdown && (
        <div className="text-xs text-gray-500 dark:text-gray-400 space-y-1">
          <div className="flex justify-between">
            <span>{t('compliance.user_messages')}:</span>
            <span className="font-medium">{breakdown.user}</span>
          </div>
          <div className="flex justify-between">
            <span>{t('compliance.model_messages')}:</span>
            <span className="font-medium">{breakdown.model}</span>
          </div>
        </div>
      )}
    </div>
  );
};

/**
 * PositionHistogram - Bar chart showing violation distribution by index
 */
interface PositionHistogramProps {
  data: Array<{ bucket: number; count: number }>;
}

const PositionHistogram: React.FC<PositionHistogramProps> = ({ data }) => {
  const maxCount = Math.max(...data.map(d => d.count), 1);

  // Filter out buckets with zero counts for cleaner display
  const nonZeroData = data.filter(entry => entry.count > 0);

  if (nonZeroData.length === 0) {
    return (
      <div className="text-sm text-gray-500 dark:text-gray-400 text-center py-4">
        No position violations recorded
      </div>
    );
  }

  return (
    <div className="space-y-2">
      {nonZeroData.map(entry => {
        const percentage = (entry.count / maxCount) * 100;

        return (
          <div key={entry.bucket} className="flex items-center gap-3">
            <span className="text-xs text-gray-500 dark:text-gray-400 w-16">
              Index {entry.bucket}:
            </span>
            <div className="flex-1 bg-gray-100 dark:bg-base-300 rounded-full h-4 overflow-hidden">
              <div
                className="h-full bg-blue-500 dark:bg-blue-400 transition-all"
                style={{ width: `${percentage}%` }}
              />
            </div>
            <span className="text-xs font-medium text-gray-700 dark:text-gray-300 w-8 text-right">
              {entry.count}
            </span>
          </div>
        );
      })}
    </div>
  );
};

/**
 * AlertPanel - Displays alerts and recommendations based on violation rates
 */
interface AlertPanelProps {
  budgetLevel: AlertLevel;
  positionLevel: AlertLevel;
  budgetRate: number;
  positionRate: number;
}

const AlertPanel: React.FC<AlertPanelProps> = ({
  budgetLevel,
  positionLevel,
  budgetRate,
  positionRate,
}) => {
  const { t } = useTranslation();

  const alerts = [];

  // Budget alerts
  if (budgetLevel === 'RED') {
    alerts.push({
      type: 'error' as const,
      message: t('compliance.alerts.budget_red', { rate: budgetRate.toFixed(4) }),
      recommendation: t('compliance.alerts.budget_red_fix'),
    });
  } else if (budgetLevel === 'YELLOW') {
    alerts.push({
      type: 'warning' as const,
      message: t('compliance.alerts.budget_yellow', { rate: budgetRate.toFixed(4) }),
      recommendation: t('compliance.alerts.budget_yellow_fix'),
    });
  }

  // Position alerts
  if (positionLevel === 'RED') {
    alerts.push({
      type: 'error' as const,
      message: t('compliance.alerts.position_red', { rate: positionRate.toFixed(4) }),
      recommendation: t('compliance.alerts.position_red_fix'),
    });
  } else if (positionLevel === 'YELLOW') {
    alerts.push({
      type: 'warning' as const,
      message: t('compliance.alerts.position_yellow', { rate: positionRate.toFixed(4) }),
      recommendation: t('compliance.alerts.position_yellow_fix'),
    });
  }

  // All GREEN - show success message
  if (alerts.length === 0) {
    return (
      <div className="flex items-center gap-2 p-3 bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800 rounded-lg">
        <CheckCircle size={20} className="text-green-600 dark:text-green-400" />
        <span className="text-sm text-green-800 dark:text-green-300">
          {t('compliance.alerts.all_green')}
        </span>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      {alerts.map((alert, index) => (
        <div
          key={index}
          className={`p-3 rounded-lg border ${
            alert.type === 'error'
              ? 'bg-red-50 dark:bg-red-900/20 border-red-200 dark:border-red-800'
              : 'bg-yellow-50 dark:bg-yellow-900/20 border-yellow-200 dark:border-yellow-800'
          }`}
        >
          <div className="flex items-start gap-2">
            <AlertCircle
              size={20}
              className={
                alert.type === 'error'
                  ? 'text-red-600 dark:text-red-400'
                  : 'text-yellow-600 dark:text-yellow-400'
              }
            />
            <div className="flex-1">
              <p
                className={`text-sm font-medium ${
                  alert.type === 'error'
                    ? 'text-red-800 dark:text-red-300'
                    : 'text-yellow-800 dark:text-yellow-300'
                }`}
              >
                {alert.message}
              </p>
              <p
                className={`text-xs mt-1 ${
                  alert.type === 'error'
                    ? 'text-red-700 dark:text-red-400'
                    : 'text-yellow-700 dark:text-yellow-400'
                }`}
              >
                {alert.recommendation}
              </p>
            </div>
          </div>
        </div>
      ))}
    </div>
  );
};
