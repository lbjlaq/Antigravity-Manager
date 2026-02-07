// Background task runner for auto-refresh and auto-sync

import { useEffect, useRef } from 'react';
import { useConfigStore } from '@/entities/config';
import { useRefreshAllQuotas, useSyncAccountFromDb } from '@/features/accounts';

export function BackgroundTaskRunner() {
    const { config } = useConfigStore();
    const refreshAllQuotasMutation = useRefreshAllQuotas();
    const syncAccountMutation = useSyncAccountFromDb();

    // Use refs to track previous state to detect "off -> on" transitions
    const prevAutoRefreshRef = useRef(false);
    const prevAutoSyncRef = useRef(false);

    // Auto Refresh Quota Effect
    useEffect(() => {
        if (!config) return;

        let intervalId: ReturnType<typeof setTimeout> | null = null;
        const { auto_refresh, refresh_interval } = config;

        // Check if we just turned it on
        if (auto_refresh && !prevAutoRefreshRef.current) {
            console.log('[BackgroundTask] Auto-refresh enabled, executing immediately...');
            refreshAllQuotasMutation.mutate();
        }
        prevAutoRefreshRef.current = auto_refresh;

        if (auto_refresh && refresh_interval > 0) {
            console.log(`[BackgroundTask] Starting auto-refresh quota timer: ${refresh_interval} mins`);
            intervalId = setInterval(() => {
                console.log('[BackgroundTask] Auto-refreshing all quotas...');
                refreshAllQuotasMutation.mutate();
            }, refresh_interval * 60 * 1000);
        }

        return () => {
            if (intervalId) {
                console.log('[BackgroundTask] Clearing auto-refresh timer');
                clearInterval(intervalId);
            }
        };
    }, [config?.auto_refresh, config?.refresh_interval, refreshAllQuotasMutation]);

    // Auto Sync Current Account Effect
    useEffect(() => {
        if (!config) return;

        let intervalId: ReturnType<typeof setTimeout> | null = null;
        const { auto_sync, sync_interval } = config;

        // Check if we just turned it on
        if (auto_sync && !prevAutoSyncRef.current) {
            console.log('[BackgroundTask] Auto-sync enabled, executing immediately...');
            syncAccountMutation.mutate();
        }
        prevAutoSyncRef.current = auto_sync;

        if (auto_sync && sync_interval > 0) {
            console.log(`[BackgroundTask] Starting auto-sync account timer: ${sync_interval} seconds`);
            intervalId = setInterval(() => {
                console.log('[BackgroundTask] Auto-syncing account from DB...');
                syncAccountMutation.mutate();
            }, sync_interval * 1000);
        }

        return () => {
            if (intervalId) {
                console.log('[BackgroundTask] Clearing auto-sync timer');
                clearInterval(intervalId);
            }
        };
    }, [config?.auto_sync, config?.sync_interval, syncAccountMutation]);

    return null;
}
