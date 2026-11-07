import { useEffect, useRef } from 'react';
import { useConfigStore } from '../../stores/useConfigStore';
import { useAccountStore } from '../../stores/useAccountStore';

function BackgroundTaskRunner() {
    const { config } = useConfigStore();
    const { refreshAllQuotas } = useAccountStore();

    // Use refs to track previous state to detect "off -> on" transitions
    const prevAutoRefreshRef = useRef(false);
    const hasMountedRef = useRef(false);

    // Auto Refresh Quota Effect
    useEffect(() => {
        if (!config) return;

        let intervalId: ReturnType<typeof setTimeout> | null = null;
        const { auto_refresh, refresh_interval } = config;

        // [FIX #3726] 仅在用户手动从关闭 (false) 切换为开启 (true) 时触发即时刷新。
        // 启动阶段的一次性刷新已下沉到后端 startup_refresh：账号加载就绪后单进程触发一次，
        // 不受多窗口/重复挂载影响，避免应用初始启动时并发轰炸全量配额。
        const isFirstMount = !hasMountedRef.current;
        hasMountedRef.current = true;

        if (auto_refresh && !prevAutoRefreshRef.current && !isFirstMount) {
            console.log('[BackgroundTask] Auto-refresh manually toggled on, executing once...');
            refreshAllQuotas();
        }
        prevAutoRefreshRef.current = auto_refresh;

        if (auto_refresh && refresh_interval > 0) {
            console.log(`[BackgroundTask] Starting auto-refresh quota timer: ${refresh_interval} mins`);
            intervalId = setInterval(() => {
                console.log('[BackgroundTask] Auto-refreshing all quotas...');
                refreshAllQuotas();
            }, Math.min(refresh_interval * 60 * 1000, 2147483647));
        }

        return () => {
            if (intervalId) {
                console.log('[BackgroundTask] Clearing auto-refresh timer');
                clearInterval(intervalId);
            }
        };
    }, [config?.auto_refresh, config?.refresh_interval]);

    // Render nothing
    return null;
}

export default BackgroundTaskRunner;
