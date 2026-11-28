import React, { useState, useEffect, useCallback } from 'react';
import {
  ShieldCheck,
  RefreshCw,
  Rocket,
  Terminal,
  AppWindow,
  Code2,
  CheckCircle2,
  RotateCcw,
  Plus,
  Puzzle,
  Layers,
  Globe,
  Sparkles,
  Check,
} from 'lucide-react';
import { api, ProxyNode, ProxyIntegrations } from '../api';
import { tUi } from '../i18n/ui';

interface ToolboxProps {
  lang: string;
}

type ToolboxSection = 'all' | 'proxy' | 'plugins';

function formatProxyUrl(node: ProxyNode): string {
  return `${node.protocol}://${node.host}:${node.port}`;
}

export const Toolbox: React.FC<ToolboxProps> = ({ lang }) => {
  const [activeSection, setActiveSection] = useState<ToolboxSection>('all');
  const [nodes, setNodes] = useState<ProxyNode[]>([]);
  const [selectedNode, setSelectedNode] = useState<ProxyNode | null>(null);
  const [integrations, setIntegrations] = useState<
    ProxyIntegrations & { auto_no_tun_on_switch?: boolean }
  >({
    app_wrapper: false,
    ide_wrapper: false,
    shell_hook: false,
    auto_no_tun_on_switch: false,
  });
  const [customUrl, setCustomUrl] = useState('');
  const [detecting, setDetecting] = useState(false);
  const [runtimeStatus, setRuntimeStatus] = useState<
    Record<string, { running: boolean; proxy_active: boolean; proxy_url?: string | null }>
  >({});
  const [pluginStatus, setPluginStatus] = useState<{
    ide_installed: boolean;
    mcp_installed: boolean;
    cli_installed: boolean;
  }>({
    ide_installed: false,
    mcp_installed: false,
    cli_installed: false,
  });
  const [toast, setToast] = useState<string | null>(null);

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast(null), 3200);
  }, []);

  const refreshAll = useCallback(async () => {
    setDetecting(true);
    try {
      const [detected, integ, rtSt, plSt] = await Promise.all([
        api.proxyDetectNodes(),
        api.proxyGetIntegrations(),
        api.proxyRuntimeStatus(),
        api.contextPluginStatus(),
      ]);
      setNodes(detected);
      setIntegrations(integ);
      setRuntimeStatus(rtSt);
      setPluginStatus(plSt);
      setSelectedNode((prev) => {
        if (prev && detected.some((n) => n.host === prev.host && n.port === prev.port)) {
          return prev;
        }
        return detected[0] ?? prev;
      });
    } catch (e) {
      showToast(String(e));
    } finally {
      setDetecting(false);
    }
  }, [showToast]);

  useEffect(() => {
    void refreshAll();
    let inFlight = false;
    const pollRuntime = () => {
      if (document.hidden || inFlight) return;
      inFlight = true;
      void api
        .proxyRuntimeStatus()
        .then(setRuntimeStatus)
        .catch(() => {})
        .finally(() => {
          inFlight = false;
        });
    };
    const timer = window.setInterval(pollRuntime, 5000);
    const onVisibility = () => {
      if (!document.hidden) pollRuntime();
    };
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [refreshAll]);

  const activeProxyUrl = selectedNode ? formatProxyUrl(selectedNode) : undefined;

  const installedPluginsCount =
    Number(pluginStatus.mcp_installed) +
    Number(pluginStatus.ide_installed) +
    Number(pluginStatus.cli_installed);

  const configuredHostsCount =
    Number(integrations.app_wrapper) +
    Number(Boolean(integrations.ide_wrapper || integrations.ide_settings_proxy)) +
    Number(integrations.shell_hook);

  const runningClientsCount =
    Number(Boolean(runtimeStatus.antigravity?.running)) +
    Number(Boolean(runtimeStatus.ide?.running));

  const handleApplyCustomUrl = () => {
    const raw = customUrl.trim();
    if (!raw) return;
    try {
      const parsed = new URL(raw.includes('://') ? raw : `http://${raw}`);
      const protocol = parsed.protocol.startsWith('socks') ? 'socks5' : 'http';
      const port = Number(parsed.port || (protocol === 'socks5' ? 1080 : 8080));
      if (!parsed.hostname || !port || Number.isNaN(port)) {
        showToast(tUi(lang, 'toolbox_toast_invalid_url'));
        return;
      }
      const customNode: ProxyNode = {
        name: `${tUi(lang, 'toolbox_proxy_custom_name')} (${port})`,
        host: parsed.hostname,
        port,
        protocol,
        source: 'custom',
      };
      setNodes((prev) => {
        const exists = prev.some((n) => n.host === customNode.host && n.port === customNode.port);
        return exists ? prev : [customNode, ...prev];
      });
      setSelectedNode(customNode);
      setCustomUrl('');
      showToast(tUi(lang, 'toolbox_toast_selected_proxy', { url: formatProxyUrl(customNode) }));
    } catch {
      showToast(tUi(lang, 'toolbox_toast_invalid_url'));
    }
  };

  const handleLaunch = async (target: 'app' | 'ide') => {
    if (!activeProxyUrl) {
      showToast(tUi(lang, 'toolbox_toast_select_proxy_first'));
      return;
    }
    try {
      const pid = await api.proxyLaunchApp(target, activeProxyUrl);
      const integ = await api.proxyGetIntegrations();
      setIntegrations(integ);
      window.setTimeout(() => {
        void api.proxyRuntimeStatus().then(setRuntimeStatus).catch(() => {});
      }, 1000);
      showToast(
        tUi(lang, 'toolbox_toast_launched', {
          target: target === 'ide' ? 'Antigravity IDE' : 'Antigravity',
          pid,
        })
      );
    } catch (e) {
      showToast(String(e));
    }
  };

  const handleToggleWrapper = async (target: 'app' | 'ide', currentlyInstalled: boolean) => {
    const targetName = target === 'ide' ? 'Antigravity IDE' : 'Antigravity';
    try {
      if (currentlyInstalled) {
        await api.proxyRemoveMacosWrapper(target);
        showToast(tUi(lang, 'toolbox_toast_shortcut_removed', { target: targetName }));
      } else {
        if (!activeProxyUrl) {
          showToast(tUi(lang, 'toolbox_toast_select_proxy_first'));
          return;
        }
        await api.proxyCreateMacosWrapper(target, activeProxyUrl);
        showToast(tUi(lang, 'toolbox_toast_shortcut_created', { target: targetName }));
      }
      const integ = await api.proxyGetIntegrations();
      setIntegrations(integ);
    } catch (e) {
      showToast(String(e));
    }
  };

  const handleToggleShellHook = async (enable: boolean) => {
    try {
      if (enable && !activeProxyUrl) {
        showToast(tUi(lang, 'toolbox_toast_select_proxy_first'));
        return;
      }
      await api.proxyInstallShellHook(enable, activeProxyUrl);
      showToast(
        enable
          ? tUi(lang, 'toolbox_toast_hook_enabled')
          : tUi(lang, 'toolbox_toast_hook_disabled')
      );
      const integ = await api.proxyGetIntegrations();
      setIntegrations(integ);
    } catch (e) {
      showToast(String(e));
    }
  };

  const sectionTabs: { id: ToolboxSection; label: string; icon: React.FC<{ className?: string }> }[] = [
    { id: 'all', label: tUi(lang, 'toolbox_tab_all'), icon: Layers },
    { id: 'proxy', label: tUi(lang, 'toolbox_tab_proxy'), icon: ShieldCheck },
    { id: 'plugins', label: tUi(lang, 'toolbox_tab_plugins'), icon: Puzzle },
  ];

  return (
    <div className="w-full max-w-[1400px] mx-auto px-4 py-3 flex flex-col gap-3 select-none animate-in fade-in-30 duration-200">
      {/* ============================================================== */}
      {/* 【顶部极简 Segmented 控制栏 · 1:1 对齐 Settings / Stats 规范】   */}
      {/* ============================================================== */}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="inline-flex items-center p-1 rounded-xl bg-slate-200/70 dark:bg-[#16181e] border border-slate-200/80 dark:border-white/[0.09] shadow-2xs overflow-x-auto">
          {sectionTabs.map((item) => {
            const Icon = item.icon;
            const isSelected = activeSection === item.id;
            return (
              <button
                key={item.id}
                onClick={() => setActiveSection(item.id)}
                className={`px-3.5 py-1.5 rounded-lg text-[11.5px] font-medium transition-all flex items-center gap-1.5 shrink-0 cursor-pointer ${
                  isSelected
                    ? 'bg-white dark:bg-[#1f2128] text-blue-600 dark:text-blue-400 font-semibold shadow-xs'
                    : 'text-gray-600 dark:text-zinc-400 hover:text-gray-900 dark:hover:text-zinc-200'
                }`}
              >
                <Icon className="w-3.5 h-3.5" />
                <span>{item.label}</span>
              </button>
            );
          })}
        </div>

        {/* 右侧：实时状态反馈胶囊 + 重新探测按钮 */}
        <div className="flex items-center gap-2 shrink-0">
          {toast ? (
            <div className="px-2.5 py-1 rounded-full bg-emerald-500/10 border border-emerald-500/20 text-[10.5px] font-medium text-emerald-600 dark:text-emerald-400 flex items-center gap-1.5">
              <Check className="w-3 h-3 shrink-0" />
              <span className="max-w-[320px] truncate">{toast}</span>
            </div>
          ) : (
            <div className="px-2.5 py-1 rounded-full bg-white dark:bg-[#16181e] border border-slate-200/80 dark:border-white/[0.10] text-[10.5px] font-mono text-slate-500 dark:text-zinc-400 flex items-center gap-1.5">
              <span
                className={`w-1.5 h-1.5 rounded-full ${
                  activeProxyUrl ? 'bg-emerald-500' : 'bg-amber-400'
                }`}
              />
              <span>{activeProxyUrl || tUi(lang, 'toolbox_no_proxy')}</span>
            </div>
          )}

          <button
            onClick={refreshAll}
            disabled={detecting}
            className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-white dark:bg-[#16181e] hover:bg-slate-50 dark:hover:bg-[#1f2128] border border-slate-200/80 dark:border-white/[0.10] text-[11px] font-medium text-slate-600 dark:text-zinc-300 transition-colors cursor-pointer disabled:opacity-50"
          >
            <RefreshCw className={`w-3 h-3 ${detecting ? 'animate-spin' : ''}`} />
            <span>{tUi(lang, 'toolbox_rescan')}</span>
          </button>
        </div>
      </div>

      {/* ============================================================== */}
      {/* 【第一层：4 列 Apple 高密度状态 KPI 矩阵（对齐 Dashboard/Stats）】 */}
      {/* ============================================================== */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-2.5">
        {/* KPI 1: 本机代理出口 */}
        <div className="p-3 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between">
          <div className="flex items-center justify-between text-[10px] text-slate-500 dark:text-zinc-400 font-medium">
            <span className="flex items-center gap-1.5">
              <span
                className={`w-1.5 h-1.5 rounded-full ${
                  selectedNode ? 'bg-emerald-400' : 'bg-amber-400'
                }`}
              />
              {tUi(lang, 'toolbox_kpi_proxy')}
            </span>
            <Globe className="w-3.5 h-3.5 text-slate-400/80 dark:text-zinc-500" />
          </div>
          <div className="my-1 flex items-baseline gap-1.5 min-w-0">
            <span className="text-base font-bold font-mono tabular-nums tracking-tight text-slate-900 dark:text-zinc-50 truncate">
              {selectedNode ? `${selectedNode.host}:${selectedNode.port}` : '--'}
            </span>
            {typeof selectedNode?.latency_ms === 'number' && (
              <span className="text-[10px] font-mono text-emerald-600 dark:text-emerald-400 font-semibold shrink-0">
                {selectedNode.latency_ms}ms
              </span>
            )}
          </div>
          <span className="text-[10px] text-slate-500 dark:text-zinc-400 tabular-nums truncate">
            {selectedNode
              ? `${selectedNode.name} · ${tUi(lang, 'toolbox_kpi_nodes_avail', { count: nodes.length })}`
              : tUi(lang, 'toolbox_kpi_nodes_avail', { count: nodes.length })}
          </span>
        </div>

        {/* KPI 2: 客户端加速状态 */}
        <div className="p-3 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between">
          <div className="flex items-center justify-between text-[10px] text-slate-500 dark:text-zinc-400 font-medium">
            <span className="flex items-center gap-1.5">
              <span className="w-1.5 h-1.5 rounded-full bg-sky-400/90" />
              {tUi(lang, 'toolbox_kpi_clients')}
            </span>
            <Rocket className="w-3.5 h-3.5 text-slate-400/80 dark:text-zinc-500" />
          </div>
          <div className="my-1 flex items-center gap-2">
            <span className="text-xl font-bold font-mono tabular-nums tracking-tight text-slate-900 dark:text-zinc-50">
              {configuredHostsCount}
              <span className="text-xs text-slate-400 dark:text-zinc-400 font-normal"> / 3</span>
            </span>
            <span className="px-1.5 py-0.5 rounded-md text-[9.5px] font-mono bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 font-medium">
              {tUi(lang, 'toolbox_kpi_clients_running', { count: runningClientsCount })}
            </span>
          </div>
          <span className="text-[10px] text-slate-500 dark:text-zinc-400 tabular-nums truncate">
            {tUi(lang, 'toolbox_kpi_clients_configured', { count: configuredHostsCount })}
          </span>
        </div>

        {/* KPI 3: 上下文监控插件 */}
        <div className="p-3 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between">
          <div className="flex items-center justify-between text-[10px] text-slate-500 dark:text-zinc-400 font-medium">
            <span className="flex items-center gap-1.5">
              <span className="w-1.5 h-1.5 rounded-full bg-violet-400/90" />
              {tUi(lang, 'toolbox_kpi_plugins')}
            </span>
            <Puzzle className="w-3.5 h-3.5 text-slate-400/80 dark:text-zinc-500" />
          </div>
          <div className="my-1 flex items-center gap-2">
            <span className="text-xl font-bold font-mono tabular-nums tracking-tight text-slate-900 dark:text-zinc-50">
              {installedPluginsCount}
              <span className="text-xs text-slate-400 dark:text-zinc-400 font-normal"> / 3</span>
            </span>
            <span className="px-1.5 py-0.5 rounded-md text-[9.5px] font-medium bg-blue-500/10 text-blue-600 dark:text-blue-400">
              {tUi(lang, 'toolbox_plugins_badge')}
            </span>
          </div>
          <span className="text-[10px] text-slate-500 dark:text-zinc-400 truncate">
            {tUi(lang, 'toolbox_kpi_plugins_sub')}
          </span>
        </div>

        {/* KPI 4: 增强接入模式 */}
        <div className="p-3 rounded-2xl bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] shadow-xs flex flex-col justify-between">
          <div className="flex items-center justify-between text-[10px] text-slate-500 dark:text-zinc-400 font-medium">
            <span>{tUi(lang, 'toolbox_kpi_mode')}</span>
            <Sparkles className="w-3.5 h-3.5 text-slate-400/80 dark:text-zinc-500" />
          </div>
          <div className="my-1 flex items-baseline gap-1.5">
            <span className="text-lg font-bold tracking-tight text-slate-900 dark:text-zinc-50">
              {tUi(lang, 'toolbox_kpi_mode_val')}
            </span>
          </div>
          <span className="text-[10px] text-slate-500 dark:text-zinc-400 truncate">
            {tUi(lang, 'toolbox_kpi_mode_sub')}
          </span>
        </div>
      </div>

      {/* ============================================================== */}
      {/* 【第二层：7 + 5 非对称核心 Bento 布局（对齐 Dashboard 黄金比例）】 */}
      {/* ============================================================== */}
      <div
        className={`grid grid-cols-1 gap-3 ${
          activeSection === 'all' ? 'lg:grid-cols-12' : ''
        }`}
      >
        {/* 左侧 7 列：免 TUN 代理加速卡片 */}
        {(activeSection === 'all' || activeSection === 'proxy') && (
          <section
            className={`${
              activeSection === 'all' ? 'lg:col-span-7' : ''
            } bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] rounded-2xl p-4 shadow-xs flex flex-col gap-3`}
          >
            {/* 卡片头 */}
            <div className="flex items-center justify-between gap-3 pb-2.5 border-b border-gray-100 dark:border-white/[0.06]">
              <div className="flex items-center gap-2.5 min-w-0">
                <div className="w-7 h-7 rounded-lg bg-blue-500/10 text-blue-600 dark:text-blue-400 flex items-center justify-center shrink-0">
                  <ShieldCheck className="w-4 h-4" />
                </div>
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <h2 className="text-xs font-bold text-gray-900 dark:text-zinc-100">
                      {tUi(lang, 'toolbox_proxy_title')}
                    </h2>
                    {activeProxyUrl && (
                      <span className="px-2 py-0.5 rounded-full text-[10px] font-mono font-semibold bg-emerald-500/10 text-emerald-600 dark:text-emerald-400">
                        {activeProxyUrl}
                      </span>
                    )}
                  </div>
                  <p className="text-[10.5px] text-slate-500 dark:text-zinc-400 truncate mt-0.5">
                    {tUi(lang, 'toolbox_proxy_desc')}
                  </p>
                </div>
              </div>
            </div>

            {/* Step 1: 本机代理出口选择 */}
            <div className="flex flex-col gap-2 pb-2.5 border-b border-gray-100 dark:border-white/[0.06]">
              <div className="text-[10px] font-semibold uppercase tracking-wider text-slate-400 dark:text-zinc-500">
                {tUi(lang, 'toolbox_proxy_step1')}
              </div>

              {nodes.length > 0 ? (
                <div className="flex flex-wrap gap-1.5">
                  {nodes.map((node) => {
                    const isSelected =
                      selectedNode?.host === node.host && selectedNode?.port === node.port;
                    return (
                      <button
                        key={`${node.host}:${node.port}`}
                        onClick={() => setSelectedNode(node)}
                        className={`inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-xl border text-[11px] transition-all cursor-pointer ${
                          isSelected
                            ? 'border-blue-500/50 bg-blue-500/[0.08] text-blue-700 dark:text-blue-300 font-semibold shadow-2xs'
                            : 'border-slate-200/80 dark:border-white/[0.08] bg-slate-50/70 dark:bg-white/[0.02] text-slate-700 dark:text-zinc-300 hover:border-slate-300 dark:hover:border-white/20'
                        }`}
                      >
                        {isSelected && (
                          <CheckCircle2 className="w-3 h-3 text-blue-500 shrink-0" />
                        )}
                        <span>{node.name}</span>
                        <span className="text-[10px] opacity-65 font-mono">
                          {node.host}:{node.port}
                        </span>
                        {typeof node.latency_ms === 'number' && (
                          <span className="text-[10px] font-mono text-emerald-600 dark:text-emerald-400">
                            {node.latency_ms}ms
                          </span>
                        )}
                      </button>
                    );
                  })}
                </div>
              ) : (
                <p className="text-[11px] text-amber-600/90 dark:text-amber-400/90">
                  {tUi(lang, 'toolbox_proxy_empty')}
                </p>
              )}

              <div className="flex items-center gap-2 w-full pt-0.5">
                <input
                  type="text"
                  value={customUrl}
                  onChange={(e) => setCustomUrl(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') handleApplyCustomUrl();
                  }}
                  placeholder={tUi(lang, 'toolbox_proxy_custom_placeholder')}
                  className="flex-1 h-7 px-2.5 rounded-lg text-[11px] font-mono bg-slate-50 dark:bg-white/[0.04] border border-slate-200/80 dark:border-white/[0.08] text-slate-900 dark:text-white placeholder:text-slate-400 dark:placeholder:text-zinc-500 focus:outline-none focus:border-blue-500/60"
                />
                <button
                  onClick={handleApplyCustomUrl}
                  disabled={!customUrl.trim()}
                  className="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg text-[11px] font-medium bg-slate-100 hover:bg-slate-200 disabled:opacity-40 dark:bg-white/[0.08] dark:hover:bg-white/[0.14] text-slate-800 dark:text-zinc-100 transition-colors cursor-pointer"
                >
                  <Plus className="w-3 h-3" />
                  <span>{tUi(lang, 'toolbox_proxy_add')}</span>
                </button>
              </div>
            </div>

            {/* Step 2: 三宿主对等管理 */}
            <div className="flex flex-col gap-2">
              <div className="text-[10px] font-semibold uppercase tracking-wider text-slate-400 dark:text-zinc-500">
                {tUi(lang, 'toolbox_proxy_step2')}
              </div>

              {/* 行 1：Antigravity 桌面经典版 */}
              <div className="p-2.5 rounded-xl bg-slate-50/70 dark:bg-white/[0.025] border border-slate-200/70 dark:border-white/[0.06] flex flex-wrap items-center justify-between gap-2.5">
                <div className="flex items-center gap-2.5 min-w-0 flex-1">
                  <div className="w-7 h-7 rounded-lg bg-slate-200/70 dark:bg-white/[0.06] text-slate-600 dark:text-zinc-300 flex items-center justify-center shrink-0">
                    <AppWindow className="w-3.5 h-3.5" />
                  </div>
                  <div className="min-w-0">
                    <div className="flex items-center gap-1.5 flex-wrap">
                      <span className="text-[11.5px] font-semibold text-gray-900 dark:text-zinc-100">
                        Antigravity
                      </span>
                      {runtimeStatus.antigravity?.running && (
                        <span
                          title={runtimeStatus.antigravity.proxy_url ?? undefined}
                          className={`px-1.5 py-0.5 rounded text-[9.5px] font-medium ${
                            runtimeStatus.antigravity.proxy_active
                              ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400'
                              : 'bg-amber-500/10 text-amber-600 dark:text-amber-400'
                          }`}
                        >
                          {runtimeStatus.antigravity.proxy_active
                            ? tUi(lang, 'toolbox_status_running_proxy')
                            : tUi(lang, 'toolbox_status_running_direct')}
                        </span>
                      )}
                      {integrations.app_wrapper && (
                        <span className="px-1.5 py-0.5 rounded text-[9.5px] font-medium bg-emerald-500/10 text-emerald-600 dark:text-emerald-400">
                          {tUi(lang, 'toolbox_status_shortcut_ready')}
                        </span>
                      )}
                    </div>
                    <p className="text-[10.5px] text-slate-500 dark:text-zinc-400 truncate">
                      {tUi(lang, 'toolbox_client_app_desc')}
                    </p>
                  </div>
                </div>
                <div className="flex items-center gap-1.5 shrink-0">
                  <button
                    onClick={() => handleLaunch('app')}
                    className="inline-flex items-center gap-1 px-2.5 py-1 rounded-lg text-[11px] font-medium bg-blue-600 hover:bg-blue-700 text-white transition-colors cursor-pointer shadow-2xs"
                  >
                    <Rocket className="w-3 h-3" />
                    <span>{tUi(lang, 'toolbox_btn_launch')}</span>
                  </button>
                  <button
                    onClick={() => handleToggleWrapper('app', integrations.app_wrapper)}
                    className={`inline-flex items-center gap-1 px-2.5 py-1 rounded-lg text-[11px] font-medium transition-colors cursor-pointer ${
                      integrations.app_wrapper
                        ? 'bg-rose-500/10 hover:bg-rose-500/20 text-rose-600 dark:text-rose-400'
                        : 'bg-white dark:bg-white/[0.07] hover:bg-slate-100 dark:hover:bg-white/[0.12] border border-slate-200/80 dark:border-white/[0.08] text-slate-700 dark:text-zinc-200'
                    }`}
                  >
                    {integrations.app_wrapper && <RotateCcw className="w-3 h-3" />}
                    <span>
                      {integrations.app_wrapper
                        ? tUi(lang, 'toolbox_btn_remove_shortcut')
                        : tUi(lang, 'toolbox_btn_create_shortcut')}
                    </span>
                  </button>
                </div>
              </div>

              {/* 行 2：Antigravity IDE */}
              <div className="p-2.5 rounded-xl bg-slate-50/70 dark:bg-white/[0.025] border border-slate-200/70 dark:border-white/[0.06] flex flex-wrap items-center justify-between gap-2.5">
                <div className="flex items-center gap-2.5 min-w-0 flex-1">
                  <div className="w-7 h-7 rounded-lg bg-slate-200/70 dark:bg-white/[0.06] text-slate-600 dark:text-zinc-300 flex items-center justify-center shrink-0">
                    <Code2 className="w-3.5 h-3.5" />
                  </div>
                  <div className="min-w-0">
                    <div className="flex items-center gap-1.5 flex-wrap">
                      <span className="text-[11.5px] font-semibold text-gray-900 dark:text-zinc-100">
                        Antigravity IDE
                      </span>
                      {runtimeStatus.ide?.running && (
                        <span
                          title={runtimeStatus.ide.proxy_url ?? undefined}
                          className={`px-1.5 py-0.5 rounded text-[9.5px] font-medium ${
                            runtimeStatus.ide.proxy_active
                              ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400'
                              : 'bg-amber-500/10 text-amber-600 dark:text-amber-400'
                          }`}
                        >
                          {runtimeStatus.ide.proxy_active
                            ? tUi(lang, 'toolbox_status_running_proxy')
                            : tUi(lang, 'toolbox_status_running_direct')}
                        </span>
                      )}
                      {integrations.ide_settings_proxy && (
                        <span className="px-1.5 py-0.5 rounded text-[9.5px] font-medium bg-blue-500/10 text-blue-600 dark:text-blue-400">
                          {tUi(lang, 'toolbox_status_ide_settings_active')}
                        </span>
                      )}
                      {integrations.ide_wrapper && (
                        <span className="px-1.5 py-0.5 rounded text-[9.5px] font-medium bg-emerald-500/10 text-emerald-600 dark:text-emerald-400">
                          {tUi(lang, 'toolbox_status_shortcut_ready')}
                        </span>
                      )}
                    </div>
                    <p className="text-[10.5px] text-slate-500 dark:text-zinc-400 truncate">
                      {tUi(lang, 'toolbox_client_ide_desc')}
                    </p>
                  </div>
                </div>
                <div className="flex items-center gap-1.5 shrink-0">
                  <button
                    onClick={() => handleLaunch('ide')}
                    className="inline-flex items-center gap-1 px-2.5 py-1 rounded-lg text-[11px] font-medium bg-blue-600 hover:bg-blue-700 text-white transition-colors cursor-pointer shadow-2xs"
                  >
                    <Rocket className="w-3 h-3" />
                    <span>{tUi(lang, 'toolbox_btn_launch')}</span>
                  </button>
                  {integrations.ide_settings_proxy && (
                    <button
                      onClick={async () => {
                        try {
                          await api.proxySyncIdeSettings(null);
                          const integ = await api.proxyGetIntegrations();
                          setIntegrations(integ);
                          showToast(tUi(lang, 'toolbox_toast_ide_restored'));
                        } catch (e) {
                          showToast(String(e));
                        }
                      }}
                      className="inline-flex items-center gap-1 px-2 py-1 rounded-lg text-[11px] font-medium bg-rose-500/10 hover:bg-rose-500/20 text-rose-600 dark:text-rose-400 transition-colors cursor-pointer"
                    >
                      <RotateCcw className="w-3 h-3" />
                      <span>{tUi(lang, 'toolbox_btn_restore_default')}</span>
                    </button>
                  )}
                  <button
                    onClick={() => handleToggleWrapper('ide', integrations.ide_wrapper)}
                    className={`inline-flex items-center gap-1 px-2.5 py-1 rounded-lg text-[11px] font-medium transition-colors cursor-pointer ${
                      integrations.ide_wrapper
                        ? 'bg-rose-500/10 hover:bg-rose-500/20 text-rose-600 dark:text-rose-400'
                        : 'bg-white dark:bg-white/[0.07] hover:bg-slate-100 dark:hover:bg-white/[0.12] border border-slate-200/80 dark:border-white/[0.08] text-slate-700 dark:text-zinc-200'
                    }`}
                  >
                    {integrations.ide_wrapper && <RotateCcw className="w-3 h-3" />}
                    <span>
                      {integrations.ide_wrapper
                        ? tUi(lang, 'toolbox_btn_remove_shortcut')
                        : tUi(lang, 'toolbox_btn_create_shortcut')}
                    </span>
                  </button>
                </div>
              </div>

              {/* 行 3：agy CLI */}
              <div className="p-2.5 rounded-xl bg-slate-50/70 dark:bg-white/[0.025] border border-slate-200/70 dark:border-white/[0.06] flex flex-wrap items-center justify-between gap-2.5">
                <div className="flex items-center gap-2.5 min-w-0 flex-1">
                  <div className="w-7 h-7 rounded-lg bg-slate-200/70 dark:bg-white/[0.06] text-slate-600 dark:text-zinc-300 flex items-center justify-center shrink-0">
                    <Terminal className="w-3.5 h-3.5" />
                  </div>
                  <div className="min-w-0">
                    <div className="flex items-center gap-1.5 flex-wrap">
                      <span className="text-[11.5px] font-semibold text-gray-900 dark:text-zinc-100">
                        agy CLI
                      </span>
                      {integrations.shell_hook && (
                        <span className="px-1.5 py-0.5 rounded text-[9.5px] font-medium bg-emerald-500/10 text-emerald-600 dark:text-emerald-400">
                          {tUi(lang, 'toolbox_status_hook_active')}
                        </span>
                      )}
                    </div>
                    <p className="text-[10.5px] text-slate-500 dark:text-zinc-400 truncate">
                      {tUi(lang, 'toolbox_client_cli_desc')}
                    </p>
                  </div>
                </div>
                <div className="flex items-center gap-1.5 shrink-0">
                  {integrations.shell_hook ? (
                    <button
                      onClick={() => handleToggleShellHook(false)}
                      className="inline-flex items-center gap-1 px-2.5 py-1 rounded-lg text-[11px] font-medium bg-rose-500/10 hover:bg-rose-500/20 text-rose-600 dark:text-rose-400 transition-colors cursor-pointer"
                    >
                      <RotateCcw className="w-3 h-3" />
                      <span>{tUi(lang, 'toolbox_btn_restore_default')}</span>
                    </button>
                  ) : (
                    <button
                      onClick={() => handleToggleShellHook(true)}
                      className="inline-flex items-center gap-1 px-2.5 py-1 rounded-lg text-[11px] font-medium bg-white dark:bg-white/[0.07] hover:bg-slate-100 dark:hover:bg-white/[0.12] border border-slate-200/80 dark:border-white/[0.08] text-slate-700 dark:text-zinc-200 transition-colors cursor-pointer"
                    >
                      <span>{tUi(lang, 'toolbox_btn_enable_hook')}</span>
                    </button>
                  )}
                </div>
              </div>
            </div>
          </section>
        )}

        {/* 右侧 5 列：上下文监控插件（原生零侵入 · 三端矩阵） */}
        {(activeSection === 'all' || activeSection === 'plugins') && (
          <section
            className={`${
              activeSection === 'all' ? 'lg:col-span-5' : ''
            } bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] rounded-2xl p-4 shadow-xs flex flex-col gap-3`}
          >
            {/* 卡片头 */}
            <div className="flex items-center justify-between gap-2 pb-2.5 border-b border-gray-100 dark:border-white/[0.06]">
              <div className="flex items-center gap-2.5 min-w-0">
                <div className="w-7 h-7 rounded-lg bg-violet-500/10 text-violet-600 dark:text-violet-400 flex items-center justify-center shrink-0">
                  <Puzzle className="w-4 h-4" />
                </div>
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <h2 className="text-xs font-bold text-gray-900 dark:text-zinc-100">
                      {tUi(lang, 'toolbox_plugins_title')}
                    </h2>
                    <span className="px-1.5 py-0.5 rounded-full text-[9.5px] font-semibold bg-blue-500/10 text-blue-600 dark:text-blue-400">
                      {tUi(lang, 'toolbox_plugins_badge')}
                    </span>
                  </div>
                  <p className="text-[10.5px] text-slate-500 dark:text-zinc-400 truncate mt-0.5">
                    {tUi(lang, 'toolbox_plugins_desc')}
                  </p>
                </div>
              </div>
            </div>

            {/* 三宿主监控插件列表 */}
            <div className="flex flex-col gap-2">
              {/* 插件 1: Antigravity (Sidecar UI) */}
              <div className="p-2.5 rounded-xl bg-slate-50/70 dark:bg-white/[0.025] border border-slate-200/70 dark:border-white/[0.06] flex items-center justify-between gap-3">
                <div className="flex items-center gap-2.5 min-w-0">
                  <div className="w-7 h-7 rounded-lg bg-slate-200/70 dark:bg-white/[0.06] text-slate-600 dark:text-zinc-300 flex items-center justify-center shrink-0">
                    <AppWindow className="w-3.5 h-3.5" />
                  </div>
                  <div className="min-w-0">
                    <div className="flex items-center gap-1.5 flex-wrap">
                      <span className="text-[11.5px] font-semibold text-gray-900 dark:text-zinc-100">
                        {tUi(lang, 'toolbox_plugin_sidecar_title')}
                      </span>
                      <span
                        className={`px-1.5 py-0.5 rounded text-[9.5px] font-medium ${
                          pluginStatus.mcp_installed
                            ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400'
                            : 'bg-slate-200/70 dark:bg-white/[0.06] text-slate-500 dark:text-zinc-400'
                        }`}
                      >
                        {pluginStatus.mcp_installed
                          ? tUi(lang, 'toolbox_plugin_installed')
                          : tUi(lang, 'toolbox_plugin_not_installed')}
                      </span>
                    </div>
                    <p
                      title={tUi(lang, 'toolbox_plugin_sidecar_desc')}
                      className="text-[10.5px] text-slate-500 dark:text-zinc-400 truncate mt-0.5"
                    >
                      {tUi(lang, 'toolbox_plugin_sidecar_desc')}
                    </p>
                  </div>
                </div>
                <button
                  onClick={async () => {
                    try {
                      const st = pluginStatus.mcp_installed
                        ? await api.contextPluginUninstall('mcp')
                        : await api.contextPluginInstall('mcp');
                      setPluginStatus(st);
                      showToast(
                        st.mcp_installed
                          ? tUi(lang, 'toolbox_toast_mcp_installed')
                          : tUi(lang, 'toolbox_toast_mcp_uninstalled')
                      );
                    } catch (e) {
                      showToast(String(e));
                    }
                  }}
                  className={`min-w-[68px] text-center px-2.5 py-1 rounded-lg text-[11px] font-medium transition-colors shrink-0 cursor-pointer ${
                    pluginStatus.mcp_installed
                      ? 'bg-rose-500/10 hover:bg-rose-500/20 text-rose-600 dark:text-rose-400'
                      : 'bg-blue-600 hover:bg-blue-700 text-white shadow-2xs'
                  }`}
                >
                  {pluginStatus.mcp_installed
                    ? tUi(lang, 'toolbox_btn_uninstall')
                    : tUi(lang, 'toolbox_btn_install')}
                </button>
              </div>

              {/* 插件 2: Antigravity IDE */}
              <div className="p-2.5 rounded-xl bg-slate-50/70 dark:bg-white/[0.025] border border-slate-200/70 dark:border-white/[0.06] flex items-center justify-between gap-3">
                <div className="flex items-center gap-2.5 min-w-0">
                  <div className="w-7 h-7 rounded-lg bg-slate-200/70 dark:bg-white/[0.06] text-slate-600 dark:text-zinc-300 flex items-center justify-center shrink-0">
                    <Code2 className="w-3.5 h-3.5" />
                  </div>
                  <div className="min-w-0">
                    <div className="flex items-center gap-1.5 flex-wrap">
                      <span className="text-[11.5px] font-semibold text-gray-900 dark:text-zinc-100">
                        {tUi(lang, 'toolbox_plugin_ide_title')}
                      </span>
                      <span
                        className={`px-1.5 py-0.5 rounded text-[9.5px] font-medium ${
                          pluginStatus.ide_installed
                            ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400'
                            : 'bg-slate-200/70 dark:bg-white/[0.06] text-slate-500 dark:text-zinc-400'
                        }`}
                      >
                        {pluginStatus.ide_installed
                          ? tUi(lang, 'toolbox_plugin_installed')
                          : tUi(lang, 'toolbox_plugin_not_installed')}
                      </span>
                    </div>
                    <p
                      title={tUi(lang, 'toolbox_plugin_ide_desc')}
                      className="text-[10.5px] text-slate-500 dark:text-zinc-400 truncate mt-0.5"
                    >
                      {tUi(lang, 'toolbox_plugin_ide_desc')}
                    </p>
                  </div>
                </div>
                <button
                  onClick={async () => {
                    try {
                      const st = pluginStatus.ide_installed
                        ? await api.contextPluginUninstall('ide')
                        : await api.contextPluginInstall('ide');
                      setPluginStatus(st);
                      showToast(
                        st.ide_installed
                          ? tUi(lang, 'toolbox_toast_ide_installed')
                          : tUi(lang, 'toolbox_toast_ide_uninstalled')
                      );
                    } catch (e) {
                      showToast(String(e));
                    }
                  }}
                  className={`min-w-[68px] text-center px-2.5 py-1 rounded-lg text-[11px] font-medium transition-colors shrink-0 cursor-pointer ${
                    pluginStatus.ide_installed
                      ? 'bg-rose-500/10 hover:bg-rose-500/20 text-rose-600 dark:text-rose-400'
                      : 'bg-blue-600 hover:bg-blue-700 text-white shadow-2xs'
                  }`}
                >
                  {pluginStatus.ide_installed
                    ? tUi(lang, 'toolbox_btn_uninstall')
                    : tUi(lang, 'toolbox_btn_install')}
                </button>
              </div>

              {/* 插件 3: Antigravity CLI (agy 状态栏) */}
              <div className="p-2.5 rounded-xl bg-slate-50/70 dark:bg-white/[0.025] border border-slate-200/70 dark:border-white/[0.06] flex items-center justify-between gap-3">
                <div className="flex items-center gap-2.5 min-w-0">
                  <div className="w-7 h-7 rounded-lg bg-slate-200/70 dark:bg-white/[0.06] text-slate-600 dark:text-zinc-300 flex items-center justify-center shrink-0">
                    <Terminal className="w-3.5 h-3.5" />
                  </div>
                  <div className="min-w-0">
                    <div className="flex items-center gap-1.5 flex-wrap">
                      <span className="text-[11.5px] font-semibold text-gray-900 dark:text-zinc-100">
                        {tUi(lang, 'toolbox_plugin_cli_title')}
                      </span>
                      <span
                        className={`px-1.5 py-0.5 rounded text-[9.5px] font-medium ${
                          pluginStatus.cli_installed
                            ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400'
                            : 'bg-slate-200/70 dark:bg-white/[0.06] text-slate-500 dark:text-zinc-400'
                        }`}
                      >
                        {pluginStatus.cli_installed
                          ? tUi(lang, 'toolbox_plugin_installed')
                          : tUi(lang, 'toolbox_plugin_not_installed')}
                      </span>
                    </div>
                    <p
                      title={tUi(lang, 'toolbox_plugin_cli_desc')}
                      className="text-[10.5px] text-slate-500 dark:text-zinc-400 truncate mt-0.5"
                    >
                      {tUi(lang, 'toolbox_plugin_cli_desc')}
                    </p>
                  </div>
                </div>
                <button
                  onClick={async () => {
                    try {
                      const st = pluginStatus.cli_installed
                        ? await api.contextPluginUninstall('cli')
                        : await api.contextPluginInstall('cli');
                      setPluginStatus(st);
                      showToast(
                        st.cli_installed
                          ? tUi(lang, 'toolbox_toast_cli_installed')
                          : tUi(lang, 'toolbox_toast_cli_uninstalled')
                      );
                    } catch (e) {
                      showToast(String(e));
                    }
                  }}
                  className={`min-w-[68px] text-center px-2.5 py-1 rounded-lg text-[11px] font-medium transition-colors shrink-0 cursor-pointer ${
                    pluginStatus.cli_installed
                      ? 'bg-rose-500/10 hover:bg-rose-500/20 text-rose-600 dark:text-rose-400'
                      : 'bg-blue-600 hover:bg-blue-700 text-white shadow-2xs'
                  }`}
                >
                  {pluginStatus.cli_installed
                    ? tUi(lang, 'toolbox_btn_uninstall')
                    : tUi(lang, 'toolbox_btn_install')}
                </button>
              </div>
            </div>

            {/* 底部紧凑入口速览卡（视觉平衡 + 实用引导） */}
            <div className="mt-auto pt-2.5 border-t border-gray-100 dark:border-white/[0.06]">
              <div className="p-2.5 rounded-xl bg-slate-50/80 dark:bg-white/[0.02] border border-slate-200/60 dark:border-white/[0.05] flex flex-col gap-1.5">
                <div className="text-[10px] font-semibold uppercase tracking-wider text-slate-400 dark:text-zinc-500">
                  {tUi(lang, 'toolbox_plugin_guide_title')}
                </div>
                <div className="flex flex-col gap-1 text-[10.5px] text-slate-500 dark:text-zinc-400">
                  <div className="flex items-center gap-1.5 truncate">
                    <span className="w-1 h-1 rounded-full bg-blue-500/70 shrink-0" />
                    <span className="truncate">{tUi(lang, 'toolbox_plugin_guide_sidecar')}</span>
                  </div>
                  <div className="flex items-center gap-1.5 truncate">
                    <span className="w-1 h-1 rounded-full bg-violet-500/70 shrink-0" />
                    <span className="truncate">{tUi(lang, 'toolbox_plugin_guide_ide')}</span>
                  </div>
                  <div className="flex items-center gap-1.5 truncate">
                    <span className="w-1 h-1 rounded-full bg-emerald-500/70 shrink-0" />
                    <span className="truncate">{tUi(lang, 'toolbox_plugin_guide_cli')}</span>
                  </div>
                </div>
              </div>
            </div>
          </section>
        )}
      </div>
    </div>
  );
};
