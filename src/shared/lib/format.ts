// File: src/shared/lib/format.ts
// Formatting utility functions

import { formatDistanceToNow } from 'date-fns';
import { zhCN, zhTW, enUS, ja, tr, vi, ptBR } from 'date-fns/locale';

/**
 * Format timestamp to relative time string
 */
export function formatRelativeTime(timestamp: number, language: string = 'zh-CN'): string {
  let locale = enUS;
  if (language === 'zh-CN' || language === 'zh') locale = zhCN;
  else if (language === 'zh-TW') locale = zhTW;
  else if (language === 'ja') locale = ja;
  else if (language === 'tr') locale = tr;
  else if (language === 'vi') locale = vi;
  else if (language === 'pt' || language === 'pt-BR') locale = ptBR;

  return formatDistanceToNow(new Date(timestamp * 1000), {
    addSuffix: true,
    locale,
  });
}

/**
 * Format bytes to human readable string
 */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 Bytes';

  const k = 1024;
  const sizes = ['Bytes', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));

  return Math.round(bytes / Math.pow(k, i) * 100) / 100 + ' ' + sizes[i];
}

/**
 * Get color based on quota percentage
 * Thresholds: < 20% = error (red), 20-60% = warning (yellow), > 60% = success (green)
 */
export function getQuotaColor(percentage: number): string {
  if (percentage > 60) return 'success';
  if (percentage >= 20) return 'warning';
  return 'error';
}

/**
 * Format time remaining until date
 */
export function formatTimeRemaining(dateStr: string | undefined | null): string {
  if (!dateStr) return '—';
  
  const targetDate = new Date(dateStr);
  
  if (isNaN(targetDate.getTime())) return '—';
  
  const now = new Date();
  const diffMs = targetDate.getTime() - now.getTime();

  if (diffMs <= 0) return 'Ready';

  const diffHrs = Math.floor(diffMs / (1000 * 60 * 60));
  const diffMins = Math.floor((diffMs % (1000 * 60 * 60)) / (1000 * 60));

  if (diffHrs >= 24) {
    const diffDays = Math.floor(diffHrs / 24);
    const remainingHrs = diffHrs % 24;
    return `${diffDays}d ${remainingHrs}h`;
  }

  return `${diffHrs}h ${diffMins}m`;
}

/**
 * Get color for time remaining display
 */
export function getTimeRemainingColor(dateStr: string | undefined): string {
  if (!dateStr) return 'gray';
  const targetDate = new Date(dateStr);
  const now = new Date();
  const diffMs = targetDate.getTime() - now.getTime();

  if (diffMs <= 0) return 'success';

  const diffHrs = diffMs / (1000 * 60 * 60);

  if (diffHrs < 1) return 'success';
  if (diffHrs < 6) return 'warning';
  return 'neutral';
}

/**
 * Format timestamp to locale date string
 */
export function formatDate(timestamp: string | number | undefined | null): string | null {
  if (!timestamp) return null;
  const date = typeof timestamp === 'number'
    ? new Date(timestamp * 1000)
    : new Date(timestamp);

  if (isNaN(date.getTime())) return null;

  return date.toLocaleString(undefined, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false
  });
}

/**
 * Format number to compact string (1000 -> 1k)
 */
export function formatCompactNumber(num: number): string {
  if (num === 0) return '0';
  if (num < 1000 && num > -1000) return num.toString();

  const units = ['', 'k', 'M', 'G', 'T', 'P'];
  const absNum = Math.abs(num);
  const i = Math.floor(Math.log10(absNum) / 3);
  const value = num / Math.pow(1000, i);

  const formatted = value.toFixed(Math.abs(value) < 10 && i > 0 ? 1 : 0);
  return `${formatted.replace(/\.0$/, '')}${units[i]}`;
}
