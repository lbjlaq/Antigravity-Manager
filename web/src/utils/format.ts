import { formatDistanceToNow } from 'date-fns';
import { zhCN, enUS } from 'date-fns/locale';

export function formatRelativeTime(timestamp: number, language: string = 'zh-CN'): string {
    const locale = language === 'zh-CN' ? zhCN : enUS;
    return formatDistanceToNow(new Date(timestamp * 1000), {
        addSuffix: true,
        locale,
    });
}

export function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 Bytes';

    const k = 1024;
    const sizes = ['Bytes', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));

    return Math.round(bytes / Math.pow(k, i) * 100) / 100 + ' ' + sizes[i];
}

export function getQuotaColor(percentage: number): string {
    if (percentage >= 50) return 'success';
    if (percentage >= 20) return 'warning';
    return 'error';
}

export function formatTimeRemaining(dateStr: string): string {
    const targetDate = new Date(dateStr);
    const now = new Date();
    const diffMs = targetDate.getTime() - now.getTime();

    if (diffMs <= 0) return '0h 0m';

    const diffHrs = Math.floor(diffMs / (1000 * 60 * 60));
    const diffMins = Math.floor((diffMs % (1000 * 60 * 60)) / (1000 * 60));

    if (diffHrs >= 24) {
        const diffDays = Math.floor(diffHrs / 24);
        const remainingHrs = diffHrs % 24;
        return `${diffDays}d ${remainingHrs}h`;
    }

    return `${diffHrs}h ${diffMins}m`;
}

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
