// File: src/features/proxy/api/index.ts
// Public API for proxy feature

export { proxyKeys } from './keys';

export {
  useProxyStatus,
  useProxyStats,
  useProxyLogs,
  useProxyLogDetail,
  useCloudflaredStatus,
  type ProxyStatus,
  type ProxyStats,
  type ProxyLog,
  type CloudflaredStatus,
} from './queries';

export {
  useStartProxy,
  useStopProxy,
  useGenerateApiKey,
  useUpdateModelMapping,
  useClearProxyLogs,
  useClearSessionBindings,
  useClearRateLimits,
  useSetProxyMonitorEnabled,
  useInstallCloudflared,
  useStartCloudflared,
  useStopCloudflared,
  useExecuteCliSync,
  useExecuteCliRestore,
} from './mutations';
