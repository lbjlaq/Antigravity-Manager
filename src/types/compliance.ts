/**
 * Compliance monitoring types for Story-003-12
 *
 * These types map to the Rust structs in monitor.rs:
 * - ProxyStats (with violation metrics)
 * - DetailedViolationMetrics
 * - ViolationRates
 */

/**
 * Detailed violation metrics from Story-003-08
 * Maps to DetailedViolationMetrics Rust struct
 */
export interface DetailedViolationMetrics {
  // ProxyStats fields
  stats: {
    total_requests: number;
    success_count: number;
    error_count: number;
    thinking_budget_violations: number;
    thinking_position_violations: number;
    thinking_position_violations_user: number;
    thinking_position_violations_model: number;
  };

  // Position histogram data
  position_histogram: Array<{
    bucket: number;  // Index bucket (1, 2, 3, 5, 10, 20, 50)
    count: number;    // Number of violations in this bucket
  }>;

  // Rate metrics (violations per second)
  rates: {
    budget_violations_per_second: number;
    position_violations_per_second: number;
  };
}

/**
 * Alert configuration thresholds
 */
export interface AlertThresholds {
  green_max: number;   // 0.03 violations/sec
  yellow_max: number;  // 0.28 violations/sec
  // red: > yellow_max
}

/**
 * Alert level type
 */
export type AlertLevel = 'GREEN' | 'YELLOW' | 'RED';

/**
 * Default alert thresholds from Story-003-08
 */
export const DEFAULT_ALERT_THRESHOLDS: AlertThresholds = {
  green_max: 0.03,
  yellow_max: 0.28,
};

/**
 * Calculate alert level based on violation rate
 */
export function getAlertLevel(rate: number, thresholds: AlertThresholds = DEFAULT_ALERT_THRESHOLDS): AlertLevel {
  if (rate <= thresholds.green_max) {
    return 'GREEN';
  } else if (rate <= thresholds.yellow_max) {
    return 'YELLOW';
  } else {
    return 'RED';
  }
}

/**
 * Calculate compliance score based on violation rates
 *
 * Algorithm:
 * - Start at 100%
 * - Deduct 0.1% per 0.01 violations/sec over GREEN threshold
 * - Apply to both budget and position violations
 * - Minimum score: 0%
 */
export function calculateComplianceScore(metrics: DetailedViolationMetrics): number {
  let score = 100.0;

  const budgetRate = metrics.rates.budget_violations_per_second;
  const positionRate = metrics.rates.position_violations_per_second;

  // Budget violations penalty
  const budgetOverage = Math.max(0, budgetRate - DEFAULT_ALERT_THRESHOLDS.green_max);
  const budgetPenalty = budgetOverage * 10;  // -0.1% per 0.01/sec
  score -= budgetPenalty;

  // Position violations penalty
  const positionOverage = Math.max(0, positionRate - DEFAULT_ALERT_THRESHOLDS.green_max);
  const positionPenalty = positionOverage * 10;  // -0.1% per 0.01/sec
  score -= positionPenalty;

  // Cap at 0%
  return Math.max(0, score);
}

/**
 * Generate compliance report in Markdown format
 */
export function generateComplianceReport(metrics: DetailedViolationMetrics): string {
  const score = calculateComplianceScore(metrics);
  const timestamp = new Date().toISOString();
  const stats = metrics.stats;
  const rates = metrics.rates;

  const budgetAlertLevel = getAlertLevel(rates.budget_violations_per_second);
  const positionAlertLevel = getAlertLevel(rates.position_violations_per_second);

  return `# Antigravity Compliance Report

**Generated**: ${timestamp}
**Compliance Score**: ${score.toFixed(2)}%

## Violation Summary

### Budget Violations
- **Total**: ${stats.thinking_budget_violations}
- **Rate**: ${rates.budget_violations_per_second.toFixed(4)} violations/sec
- **Alert Level**: ${budgetAlertLevel}

### Position Violations
- **Total**: ${stats.thinking_position_violations}
  - User messages: ${stats.thinking_position_violations_user}
  - Model messages: ${stats.thinking_position_violations_model}
- **Rate**: ${rates.position_violations_per_second.toFixed(4)} violations/sec
- **Alert Level**: ${positionAlertLevel}

### Position Histogram

${metrics.position_histogram
  .map(entry => `- Bucket ${entry.bucket}: ${entry.count} violations`)
  .join('\n')}

## Overall Assessment

**Request Statistics**:
- Total requests: ${stats.total_requests}
- Successful: ${stats.success_count}
- Errors: ${stats.error_count}

## Recommendations

${score >= 99.5
    ? '✅ Compliance is excellent. Continue monitoring.'
    : score >= 95
      ? '⚠️ Minor compliance issues detected. Review client configurations.'
      : '❌ Significant compliance issues. Immediate review recommended.'}

${budgetAlertLevel !== 'GREEN'
    ? '\n**Budget Violations**: Review client configurations to ensure maxOutputTokens > thinkingBudget + 100'
    : ''}
${positionAlertLevel !== 'GREEN'
    ? '\n**Position Violations**: Review client implementations to ensure thinking blocks are sent first'
    : ''}
`;
}
