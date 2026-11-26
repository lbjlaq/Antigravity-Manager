import React, { useState, useEffect, useRef, useCallback } from 'react';
import {
  Settings2,
  Terminal,
  Database,
  Download,
  Upload,
  Trash2,
  Check,
  AppWindow,
  Code2,
  Puzzle,
  Github,
  Heart,
  X,
  Coffee,
  ExternalLink,
  Copy,
  User,
  MessageCircle,
  Send,
  RefreshCw,
} from 'lucide-react';
import { api, APP_VERSION_FALLBACK, AppConfig, isTauri, SystemInfo, UpdateInfo } from '../api';
import { tUi } from '../i18n/ui';

interface SettingsProps {
  lang: string;
}

type SettingsSection = 'general' | 'cli' | 'about';
type CliPlatform = 'macos' | 'windows' | 'linux';

function detectCliPlatform(sysPlatform?: string): CliPlatform {
  const raw = (sysPlatform || '').toLowerCase();
  if (raw === 'windows' || raw === 'win32') return 'windows';
  if (raw === 'linux') return 'linux';
  if (raw === 'macos' || raw === 'darwin') return 'macos';
  if (typeof navigator !== 'undefined') {
    const ua = `${navigator.platform || ''} ${navigator.userAgent || ''}`.toLowerCase();
    if (ua.includes('win')) return 'windows';
    if (ua.includes('linux') || ua.includes('x11')) return 'linux';
  }
  return 'macos';
}

function buildCliRegisterCommand(
  targetPlatform: CliPlatform,
  detectedPlatform: CliPlatform,
  amCliPath?: string | null,
  amInPath?: boolean
): string {
  const isCurrentOs = targetPlatform === detectedPlatform;
  const rawPath = isCurrentOs ? (amCliPath || '').trim() : '';
  const isDevTarget = /[/\\]target[/\\](debug|release)[/\\]/i.test(rawPath);
  const usablePath = rawPath && !isDevTarget ? rawPath : '';

  if (targetPlatform === 'windows') {
    const winDir = usablePath
      ? usablePath.replace(/[/\\]am\.exe$/i, '')
      : '$env:LOCALAPPDATA\\Antigravity Manager';
    return `$d = "${winDir}"; $p = [Environment]::GetEnvironmentVariable("Path", "User"); if ($p -notlike "*$d*") { [Environment]::SetEnvironmentVariable("Path", $(if ($p) { "$p;$d" } else { $d }), "User") }`;
  }

  if (targetPlatform === 'linux') {
    if (isCurrentOs && amInPath) {
      return 'am --version';
    }
    if (usablePath && usablePath !== '/usr/bin/am' && usablePath !== '/usr/local/bin/am') {
      return `sudo ln -sf "${usablePath}" /usr/local/bin/am`;
    }
    return 'sudo install -Dm755 ./am /usr/local/bin/am';
  }

  const macPath = usablePath || '/Applications/Antigravity Manager.app/Contents/MacOS/am';
  return `sudo ln -sf "${macPath}" /usr/local/bin/am`;
}

const Toggle: React.FC<{ checked: boolean; onChange: (v: boolean) => void }> = ({ checked, onChange }) => (
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    onClick={() => onChange(!checked)}
    className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 focus:outline-none ${
      checked ? 'bg-blue-600' : 'bg-gray-200 dark:bg-white/15'
    }`}
  >
    <span
      className={`pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow-sm transition duration-200 ${
        checked ? 'translate-x-4' : 'translate-x-0'
      }`}
    />
  </button>
);

export const Settings: React.FC<SettingsProps> = ({ lang }) => {
  const [activeSection, setActiveSection] = useState<SettingsSection>('general');
  const [savedNotice, setSavedNotice] = useState(false);
  const [copiedCmd, setCopiedCmd] = useState<string | null>(null);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [installingUpdate, setInstallingUpdate] = useState(false);
  const [installPct, setInstallPct] = useState<number>(0);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [supportModalOpen, setSupportModalOpen] = useState(false);
  const [appVersion, setAppVersion] = useState(APP_VERSION_FALLBACK);
  const [cliPlatform, setCliPlatform] = useState<CliPlatform>(() => detectCliPlatform());
  const cliPlatformManual = useRef(false);

  // 通用偏好状态
  const [pollInterval, setPollInterval] = useState<'5m' | '15m' | '30m' | 'manual'>('15m');
  const [defaultStrategy, setDefaultStrategy] = useState<'max-quota' | 'expiring-soon' | 'tier-first' | 'least-used'>('max-quota');
  const [defaultPool, setDefaultPool] = useState<'balanced' | 'claude' | 'gemini'>('balanced');
  const [launchAtLogin, setLaunchAtLogin] = useState(false);
  const [quietAutostart, setQuietAutostart] = useState(true);
  const [lightweightMode, setLightweightMode] = useState(false);
  const [trayMode, setTrayMode] = useState(true);
  const [trayClickAction, setTrayClickAction] = useState<'popover' | 'main' | 'menu'>('popover');
  const [closeToTray, setCloseToTray] = useState(true);

  // 自定义宿主路径状态
  const [customPaths, setCustomPaths] = useState<{
    app: string;
    ide: string;
    plugin: string;
    cli: string;
  }>({
    app: '',
    ide: '',
    plugin: '',
    cli: '',
  });

  // 系统遥测与默认路径信息
  const [systemInfo, setSystemInfo] = useState<SystemInfo>({
    db_size_bytes: 4500000,
    db_size_formatted: '4.3 MB',
    total_logged_requests: 2024,
    memory_usage_mb: 28.4,
    config_path: '~/.antigravity_tools/config.json',
    app_installed: true,
    app_path: '~/Library/Application Support/Antigravity/User/globalStorage',
    app_live: false,
    ide_installed: true,
    ide_path: '~/Library/Application Support/Antigravity IDE/User/globalStorage',
    ide_live: false,
    plugin_installed: false,
    plugin_path: '~/.antigravity/acp-daemon.json',
    plugin_live: false,
    cli_installed: false,
    cli_path: '~/.config/agy/credentials.json',
    cli_live: false,
  });

  const isInitialLoad = useRef(true);

  const triggerSaved = useCallback(() => {
    setSavedNotice(true);
    setTimeout(() => setSavedNotice(false), 2000);
  }, []);

  // 加载真实配置与系统状态
  const loadConfigAndSystem = useCallback(async () => {
    try {
      const [cfg, sys] = await Promise.all([
        api.getAppConfig(),
        api.getSystemInfo(),
      ]);

      if (cfg) {
        if (!cfg.quota_polling.enabled) {
          setPollInterval('manual');
        } else {
          const mins = cfg.quota_polling.interval_minutes;
          if (mins <= 5) setPollInterval('5m');
          else if (mins <= 15) setPollInterval('15m');
          else setPollInterval('30m');
        }

        if (cfg.auto_switch) {
          const s = (cfg.auto_switch.strategy || 'max-quota').toLowerCase();
          if (s === 'expiring-soon' || s === 'expiring_soon') setDefaultStrategy('expiring-soon');
          else if (s === 'tier-first' || s === 'tier_first') setDefaultStrategy('tier-first');
          else if (s === 'least-used' || s === 'least_used' || s === 'balanced') setDefaultStrategy('least-used');
          else setDefaultStrategy('max-quota');

          const p = (cfg.auto_switch.pool || 'balanced').toLowerCase();
          if (p === 'claude' || p === 'gemini') setDefaultPool(p);
          else setDefaultPool('balanced');
        }

        if (cfg.custom_paths) {
          setCustomPaths({
            app: cfg.custom_paths.app || '',
            ide: cfg.custom_paths.ide || '',
            plugin: cfg.custom_paths.plugin || '',
            cli: cfg.custom_paths.cli || '',
          });
        }

        if (cfg.tray) {
          setTrayMode(cfg.tray.enabled ?? true);
          const act = (cfg.tray.left_click_action || 'popover').toLowerCase();
          if (act === 'main' || act === 'menu') {
            setTrayClickAction(act);
          } else {
            setTrayClickAction('popover');
          }
          setCloseToTray(cfg.tray.close_to_tray ?? true);
          setQuietAutostart(cfg.tray.quiet_autostart ?? true);
          setLightweightMode(cfg.tray.lightweight_mode ?? false);
          if (typeof cfg.tray.auto_launch === 'boolean') {
            setLaunchAtLogin(cfg.tray.auto_launch);
          }
        }
      }

      try {
        const autoEnabled = await api.isAutoLaunchEnabled();
        setLaunchAtLogin(autoEnabled);
      } catch {}

      if (sys) {
        setSystemInfo(sys);
        if (sys.platform && !cliPlatformManual.current) {
          setCliPlatform(detectCliPlatform(sys.platform));
        }
        if (sys.app_version) {
          setAppVersion(sys.app_version);
        }
        // 如果自定义路径为空，则以检测到的默认路径为准展示
        setCustomPaths((prev) => ({
          app: prev.app || sys.app_path,
          ide: prev.ide || sys.ide_path,
          plugin: prev.plugin || sys.plugin_path,
          cli: prev.cli || sys.cli_path,
        }));
      }

      try {
        const ver = await api.getAppVersion();
        if (ver) setAppVersion(ver);
      } catch {
        // ignore fallback
      }
    } catch (e) {
      console.warn('Settings load fallback:', e);
    } finally {
      isInitialLoad.current = false;
    }
  }, []);

  useEffect(() => {
    loadConfigAndSystem();
  }, [loadConfigAndSystem]);

  useEffect(() => {
    if (!supportModalOpen) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setSupportModalOpen(false);
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [supportModalOpen]);

  // 当用户调节设置时，自动持久化写回 config.json
  const persistConfig = useCallback(async (overrides?: {
    pollInterval?: '5m' | '15m' | '30m' | 'manual';
    strategy?: 'max-quota' | 'expiring-soon' | 'tier-first' | 'least-used';
    pool?: 'balanced' | 'claude' | 'gemini';
    trayEnabled?: boolean;
    trayClick?: 'popover' | 'main' | 'menu';
    closeToTray?: boolean;
    autoLaunch?: boolean;
    quietAutostart?: boolean;
    lightweightMode?: boolean;
    customPaths?: {
      app: string;
      ide: string;
      plugin: string;
      cli: string;
    };
  }) => {
    if (isInitialLoad.current) return;

    const effPoll = overrides?.pollInterval ?? pollInterval;
    const effStrategy = overrides?.strategy ?? defaultStrategy;
    const effPool = overrides?.pool ?? defaultPool;
    const effTrayEnabled = overrides?.trayEnabled ?? trayMode;
    const effTrayClick = overrides?.trayClick ?? trayClickAction;
    const effCloseToTray = overrides?.closeToTray ?? closeToTray;
    const effAutoLaunch = overrides?.autoLaunch ?? launchAtLogin;
    const effQuietAutostart = overrides?.quietAutostart ?? quietAutostart;
    const effLightweight = overrides?.lightweightMode ?? lightweightMode;
    const effPaths = overrides?.customPaths ?? customPaths;

    const intervalMinutes =
      effPoll === '5m' ? 5 : effPoll === '15m' ? 15 : effPoll === '30m' ? 30 : 30;

    const existing = await api.getAppConfig().catch(() => null);

    const updated: AppConfig = {
      quota_polling: {
        enabled: effPoll !== 'manual',
        interval_minutes: intervalMinutes,
        active_only: existing?.quota_polling?.active_only ?? true,
      },
      auto_switch: {
        enabled: existing?.auto_switch?.enabled ?? false,
        targets: existing?.auto_switch?.targets ?? ['app', 'ide'],
        trigger_exhaustion_threshold: existing?.auto_switch?.trigger_exhaustion_threshold ?? 5.0,
        trigger_on_rate_limit: existing?.auto_switch?.trigger_on_rate_limit ?? true,
        strategy: effStrategy,
        pool: effPool,
        desktop_notification: existing?.auto_switch?.desktop_notification ?? true,
      },
      custom_paths: {
        app: effPaths.app.trim() || undefined,
        ide: effPaths.ide.trim() || undefined,
        plugin: effPaths.plugin.trim() || undefined,
        cli: effPaths.cli.trim() || undefined,
      },
      tray: {
        enabled: effTrayEnabled,
        left_click_action: effTrayClick,
        close_to_tray: effCloseToTray,
        auto_launch: effAutoLaunch,
        quiet_autostart: effQuietAutostart,
        lightweight_mode: effLightweight,
      },
      ui_state: existing?.ui_state,
    };

    try {
      await api.saveAppConfig(updated);
      triggerSaved();
    } catch (e) {
      console.warn('Failed to save config:', e);
      triggerSaved();
    }
  }, [pollInterval, defaultStrategy, defaultPool, trayMode, trayClickAction, closeToTray, launchAtLogin, quietAutostart, lightweightMode, customPaths, triggerSaved]);

  const handleCopy = (cmd: string) => {
    navigator.clipboard?.writeText(cmd);
    setCopiedCmd(cmd);
    setTimeout(() => setCopiedCmd(null), 1500);
  };

  const handleExportBackup = async () => {
    try {
      const data = await api.exportAccountsBackup();
      const blob = new Blob([data], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `antigravity-accounts-backup-${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e: any) {
      alert(tUi(lang, 'toast_export_failed') + (e?.message || String(e)));
    }
  };

  const handleImportBackup = () => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = '.json';
    input.onchange = async (e: any) => {
      const file = e.target.files?.[0];
      if (!file) return;
      const reader = new FileReader();
      reader.onload = async (evt) => {
        try {
          const content = evt.target?.result as string;
          const res = await api.importAccountsBackup(content);
          alert(
            tUi(lang, 'toast_import_report', {
              imported: res.imported,
              overwritten: res.overwritten,
              skipped: res.skipped,
            })
          );
        } catch (err: any) {
          alert(tUi(lang, 'toast_import_failed') + (err?.message || String(err)));
        }
      };
      reader.readAsText(file);
    };
    input.click();
  };

  const handlePruneLogs = async () => {
    if (!confirm(tUi(lang, 'settings_prune_logs') + '?')) return;
    try {
      await api.pruneLogs(30);
      alert(tUi(lang, 'settings_saved'));
    } catch {
      alert(tUi(lang, 'settings_saved'));
    }
  };

  const navItems: { id: SettingsSection; label: string; icon: React.FC<{ className?: string }> }[] = [
    {
      id: 'general',
      label: tUi(lang, 'settings_general'),
      icon: Settings2,
    },
    {
      id: 'cli',
      label: tUi(lang, 'settings_cli'),
      icon: Terminal,
    },
    {
      id: 'about',
      label: tUi(lang, 'settings_storage'),
      icon: Database,
    },
  ];

  return (
    <div className="w-full max-w-[1400px] mx-auto px-4 py-3 flex flex-col gap-3 select-none animate-in fade-in-30 duration-200">
      {/* 顶部极简 Tab 导航栏 */}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="inline-flex items-center p-1 rounded-xl bg-slate-200/70 dark:bg-[#16181e] border border-slate-200/80 dark:border-white/[0.09] shadow-2xs overflow-x-auto">
          {navItems.map((item) => {
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

        {/* 右侧：配置文件同步状态胶囊 */}
        <div className="px-2.5 py-1 rounded-full bg-white dark:bg-[#16181e] border border-slate-200/80 dark:border-white/[0.10] text-[10px] font-mono text-slate-500 dark:text-zinc-400 flex items-center gap-2 shrink-0">
          <span title={systemInfo.config_path}>config.json</span>
          <span className="text-gray-300 dark:text-zinc-700">·</span>
          {savedNotice ? (
            <span className="text-emerald-600 dark:text-emerald-400 flex items-center gap-0.5 font-sans font-semibold">
              <Check className="w-3 h-3" />
              {tUi(lang, 'settings_saved')}
            </span>
          ) : (
            <span className="font-sans">{tUi(lang, 'settings_synced')}</span>
          )}
        </div>
      </div>

      {/* 主设置内容区 */}
      <section className="flex flex-col gap-3">
        {/* ========================================================= */}
        {/* TAB 1: 通用偏好 (静默轮询 + 系统行为 + 宿主路径自定义)     */}
        {/* ========================================================= */}
        {activeSection === 'general' && (
          <div className="flex flex-col gap-3">
            {/* 配额保鲜与系统行为卡片 */}
            <div className="bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] rounded-2xl p-4 shadow-2xs flex flex-col gap-3">
              <div className="pb-1.5 border-b border-gray-100 dark:border-white/5">
                <h3 className="text-xs font-bold text-gray-900 dark:text-zinc-100">
                  {tUi(lang, 'settings_quota_sync_title')}
                </h3>
              </div>

              {/* 静默配额轮询 */}
              <div className="flex items-center justify-between gap-4 py-1">
                <div>
                  <div className="text-[11.5px] font-semibold text-gray-800 dark:text-zinc-200">
                    {tUi(lang, 'settings_polling_title')}
                  </div>
                  <div className="text-[10px] text-gray-400">
                    {tUi(lang, 'settings_polling_desc')}
                  </div>
                </div>
                <div className="inline-flex p-0.5 rounded-lg bg-gray-100 dark:bg-white/[0.05]">
                  {(['5m', '15m', '30m', 'manual'] as const).map((intv) => (
                    <button
                      key={intv}
                      onClick={() => {
                        setPollInterval(intv);
                        persistConfig({ pollInterval: intv });
                      }}
                      className={`px-2.5 py-1 rounded-md text-[10px] font-mono font-semibold transition-all cursor-pointer ${
                        pollInterval === intv
                          ? 'bg-white dark:bg-[#22242b] text-blue-600 dark:text-blue-400 shadow-2xs'
                          : 'text-gray-500 dark:text-zinc-400'
                      }`}
                    >
                      {intv === 'manual' ? tUi(lang, 'settings_manual') : intv}
                    </button>
                  ))}
                </div>
              </div>

              {/* 默认智能选号策略 */}
              <div className="flex flex-wrap items-center justify-between gap-4 pt-2 border-t border-gray-100 dark:border-white/5">
                <div>
                  <div className="text-[11.5px] font-semibold text-gray-800 dark:text-zinc-200">
                    {tUi(lang, 'settings_default_strategy_title')}
                  </div>
                  <div className="text-[10px] text-gray-400">
                    {tUi(lang, 'settings_default_strategy_desc')}
                  </div>
                </div>
                <div className="inline-flex p-0.5 rounded-lg bg-gray-100 dark:bg-white/[0.05]">
                  {([
                    { id: 'max-quota', key: 'strategy_max_quota' },
                    { id: 'expiring-soon', key: 'strategy_expiring_soon' },
                    { id: 'tier-first', key: 'strategy_tier_first' },
                    { id: 'least-used', key: 'strategy_least_used' },
                  ] as const).map((item) => (
                    <button
                      key={item.id}
                      onClick={() => {
                        setDefaultStrategy(item.id);
                        persistConfig({ strategy: item.id });
                      }}
                      className={`px-2.5 py-1 rounded-md text-[10px] font-semibold transition-all cursor-pointer ${
                        defaultStrategy === item.id
                          ? 'bg-white dark:bg-[#22242b] text-blue-600 dark:text-blue-400 shadow-2xs'
                          : 'text-gray-500 dark:text-zinc-400'
                      }`}
                    >
                      {tUi(lang, item.key)}
                    </button>
                  ))}
                </div>
              </div>

              {/* 默认偏好模型池 */}
              <div className="flex flex-wrap items-center justify-between gap-4 pt-2 border-t border-gray-100 dark:border-white/5">
                <div>
                  <div className="text-[11.5px] font-semibold text-gray-800 dark:text-zinc-200">
                    {tUi(lang, 'settings_default_pool_title')}
                  </div>
                  <div className="text-[10px] text-gray-400">
                    {tUi(lang, 'settings_default_pool_desc')}
                  </div>
                </div>
                <div className="inline-flex p-0.5 rounded-lg bg-gray-100 dark:bg-white/[0.05]">
                  {([
                    { id: 'balanced', label: tUi(lang, 'pool_balanced') },
                    { id: 'claude', label: 'Claude' },
                    { id: 'gemini', label: 'Gemini' },
                  ] as const).map((item) => (
                    <button
                      key={item.id}
                      onClick={() => {
                        setDefaultPool(item.id);
                        persistConfig({ pool: item.id });
                      }}
                      className={`px-2.5 py-1 rounded-md text-[10px] font-semibold transition-all cursor-pointer ${
                        defaultPool === item.id
                          ? 'bg-white dark:bg-[#22242b] text-blue-600 dark:text-blue-400 shadow-2xs'
                          : 'text-gray-500 dark:text-zinc-400'
                      }`}
                    >
                      {item.label}
                    </button>
                  ))}
                </div>
              </div>

              {/* 开机自启动 + 免打扰启动（留在托盘） */}
              <div className="flex flex-col gap-2.5 pt-2 border-t border-gray-100 dark:border-white/5">
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-[11.5px] font-semibold text-gray-800 dark:text-zinc-200">
                      {tUi(lang, 'settings_launch_at_login')}
                    </div>
                    <div className="text-[10px] text-gray-400">
                      {tUi(lang, 'settings_launch_at_login_desc')}
                    </div>
                  </div>
                  <Toggle
                    checked={launchAtLogin}
                    onChange={async (v) => {
                      setLaunchAtLogin(v);
                      try {
                        await api.toggleAutoLaunch(v);
                      } catch {}
                      persistConfig({ autoLaunch: v });
                    }}
                  />
                </div>

                {launchAtLogin && (
                  <div className="ml-3 pl-3 border-l-2 border-blue-500/20 dark:border-blue-400/20 flex items-center justify-between gap-4">
                    <div>
                      <div className="text-[11px] font-medium text-gray-700 dark:text-zinc-300">
                        {tUi(lang, 'settings_quiet_autostart')}
                      </div>
                      <div className="text-[9.5px] text-gray-400 leading-relaxed">
                        {tUi(lang, 'settings_quiet_autostart_desc')}
                      </div>
                    </div>
                    <Toggle
                      checked={quietAutostart}
                      onChange={(v) => {
                        setQuietAutostart(v);
                        persistConfig({ quietAutostart: v });
                      }}
                    />
                  </div>
                )}
              </div>

              {/* 菜单栏常驻与自定义点击行为 */}
              <div className="flex flex-col gap-2.5 pt-2 border-t border-gray-100 dark:border-white/5">
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-[11.5px] font-semibold text-gray-800 dark:text-zinc-200">
                      {tUi(lang, 'settings_menubar_popover')}
                    </div>
                    <div className="text-[10px] text-gray-400">
                      {tUi(lang, 'settings_menubar_popover_desc')}
                    </div>
                  </div>
                  <Toggle
                    checked={trayMode}
                    onChange={(v) => {
                      setTrayMode(v);
                      persistConfig({ trayEnabled: v });
                    }}
                  />
                </div>

                {trayMode && (
                  <div className="ml-3 pl-3 border-l-2 border-blue-500/20 dark:border-blue-400/20 flex flex-col gap-2">
                    {/* 托盘左键点击行为 */}
                    <div className="flex flex-wrap items-center justify-between gap-3">
                      <div>
                        <div className="text-[11px] font-medium text-gray-700 dark:text-zinc-300">
                          {tUi(lang, 'settings_tray_click_title')}
                        </div>
                        <div className="text-[9.5px] text-gray-400">
                          {tUi(lang, 'settings_tray_click_desc')}
                        </div>
                      </div>
                      <div className="inline-flex p-0.5 rounded-lg bg-gray-100 dark:bg-white/[0.05]">
                        {(
                          [
                            { id: 'popover', key: 'settings_tray_click_popover' },
                            { id: 'main', key: 'settings_tray_click_main' },
                            { id: 'menu', key: 'settings_tray_click_menu' },
                          ] as const
                        ).map((item) => (
                          <button
                            key={item.id}
                            onClick={() => {
                              setTrayClickAction(item.id);
                              persistConfig({ trayClick: item.id });
                            }}
                            className={`px-2.5 py-1 rounded-md text-[10px] font-semibold transition-all cursor-pointer ${
                              trayClickAction === item.id
                                ? 'bg-white dark:bg-[#22242b] text-blue-600 dark:text-blue-400 shadow-2xs'
                                : 'text-gray-500 dark:text-zinc-400'
                            }`}
                          >
                            {tUi(lang, item.key)}
                          </button>
                        ))}
                      </div>
                    </div>

                    {/* 主窗口关闭按钮行为 */}
                    <div className="flex flex-wrap items-center justify-between gap-3">
                      <div>
                        <div className="text-[11px] font-medium text-gray-700 dark:text-zinc-300">
                          {tUi(lang, 'settings_close_action_title')}
                        </div>
                        <div className="text-[9.5px] text-gray-400">
                          {tUi(lang, 'settings_close_action_desc')}
                        </div>
                      </div>
                      <div className="inline-flex p-0.5 rounded-lg bg-gray-100 dark:bg-white/[0.05]">
                        {(
                          [
                            { val: true, key: 'settings_close_hide' },
                            { val: false, key: 'settings_close_quit' },
                          ] as const
                        ).map((item) => (
                          <button
                            key={String(item.val)}
                            onClick={() => {
                              setCloseToTray(item.val);
                              persistConfig({ closeToTray: item.val });
                            }}
                            className={`px-2.5 py-1 rounded-md text-[10px] font-semibold transition-all cursor-pointer ${
                              closeToTray === item.val
                                ? 'bg-white dark:bg-[#22242b] text-blue-600 dark:text-blue-400 shadow-2xs'
                                : 'text-gray-500 dark:text-zinc-400'
                            }`}
                          >
                            {tUi(lang, item.key)}
                          </button>
                        ))}
                      </div>
                    </div>
                    {/* 轻量模式（释放前端进程） */}
                    <div className="flex items-center justify-between gap-4 pt-1 border-t border-gray-100/80 dark:border-white/[0.04]">
                      <div>
                        <div className="text-[11px] font-medium text-gray-700 dark:text-zinc-300">
                          {tUi(lang, 'settings_lightweight_title')}
                        </div>
                        <div className="text-[9.5px] text-gray-400">
                          {tUi(lang, 'settings_lightweight_desc')}
                        </div>
                      </div>
                      <Toggle
                        checked={lightweightMode}
                        onChange={(v) => {
                          setLightweightMode(v);
                          persistConfig({ lightweightMode: v });
                        }}
                      />
                    </div>
                  </div>
                )}
              </div>
            </div>

            {/* 宿主配置存储路径 (不轮询扫描状态，仅展示路径并支持自定义修改) */}
            <div className="bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] rounded-2xl p-4 shadow-2xs flex flex-col gap-3">
              <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
                <div>
                  <h3 className="text-xs font-bold text-gray-900 dark:text-zinc-100">
                    {tUi(lang, 'settings_storage_paths_title')}
                  </h3>
                  <p className="text-[10px] text-gray-400">
                    {tUi(lang, 'settings_storage_paths_desc')}
                  </p>
                </div>
                <button
                  onClick={() => {
                    const defaults = {
                      app: systemInfo.app_path,
                      ide: systemInfo.ide_path,
                      plugin: systemInfo.plugin_path,
                      cli: systemInfo.cli_path,
                    };
                    setCustomPaths(defaults);
                    void persistConfig({ customPaths: defaults });
                  }}
                  className="px-2.5 py-1 rounded-lg bg-gray-100 hover:bg-gray-200 dark:bg-white/[0.06] dark:hover:bg-white/10 text-[10px] font-semibold text-gray-600 dark:text-zinc-300 transition-colors cursor-pointer"
                >
                  {tUi(lang, 'settings_reset_defaults')}
                </button>
              </div>

              <div className="flex flex-col gap-2.5">
                {[
                  {
                    key: 'app' as const,
                    name: 'Antigravity App',
                    desc: tUi(lang, 'settings_desktop_desc'),
                    icon: AppWindow,
                  },
                  {
                    key: 'ide' as const,
                    name: 'Antigravity IDE',
                    desc: tUi(lang, 'settings_ide_desc'),
                    icon: Code2,
                  },
                  {
                    key: 'plugin' as const,
                    name: tUi(lang, 'settings_host_plugin_name'),
                    desc: tUi(lang, 'settings_plugin_desc'),
                    icon: Puzzle,
                  },
                  {
                    key: 'cli' as const,
                    name: 'agy CLI',
                    desc: tUi(lang, 'settings_cli_desc'),
                    icon: Terminal,
                  },
                ].map((item) => {
                  const Icon = item.icon;
                  return (
                    <div
                      key={item.key}
                      className="p-2.5 rounded-xl bg-gray-50/80 dark:bg-white/[0.02] border border-gray-100 dark:border-white/5 flex flex-col md:flex-row md:items-center justify-between gap-2.5"
                    >
                      <div className="flex items-center gap-2.5 min-w-[200px] shrink-0">
                        <div className="w-7 h-7 rounded-lg bg-blue-500/10 text-blue-600 dark:text-blue-400 flex items-center justify-center shrink-0">
                          <Icon className="w-3.5 h-3.5" />
                        </div>
                        <div>
                          <div className="text-[11px] font-semibold text-gray-900 dark:text-zinc-100">
                            {item.name}
                          </div>
                          <div className="text-[9.5px] text-gray-400">{item.desc}</div>
                        </div>
                      </div>

                      {/* 路径输入框 (可自由修改) */}
                      <div className="flex-1 flex items-center gap-2">
                        <input
                          type="text"
                          value={customPaths[item.key]}
                          onChange={(e) => {
                            const val = e.target.value;
                            setCustomPaths((prev) => ({ ...prev, [item.key]: val }));
                          }}
                          onBlur={() => persistConfig()}
                          className="w-full px-2.5 py-1.5 rounded-lg bg-white dark:bg-[#1a1b20] border border-gray-200 dark:border-zinc-800 text-[10.5px] font-mono text-gray-800 dark:text-zinc-200 focus:outline-none focus:border-blue-500 transition-colors"
                          placeholder={tUi(lang, 'settings_custom_path_placeholder')}
                        />
                        <button
                          onClick={() => handleCopy(customPaths[item.key])}
                          title={tUi(lang, 'settings_copy_path')}
                          className="p-1.5 rounded-lg hover:bg-gray-200/80 dark:hover:bg-white/10 text-gray-400 hover:text-gray-700 dark:hover:text-zinc-200 shrink-0 cursor-pointer"
                        >
                          {copiedCmd === customPaths[item.key] ? (
                            <Check className="w-3.5 h-3.5 text-emerald-500" />
                          ) : (
                            <Copy className="w-3.5 h-3.5" />
                          )}
                        </button>
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          </div>
        )}

        {/* ========================================================= */}
        {/* TAB 2: CLI 命令行支持 (专业开发者工具箱与命令矩阵)         */}
        {/* ========================================================= */}
        {activeSection === 'cli' && (
          <div className="flex flex-col gap-3">
            {/* CLI 终端命令全局支持说明 */}
            <div className="bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] rounded-2xl p-4 shadow-2xs flex flex-col gap-3">
              <div className="flex items-center justify-between pb-1.5 border-b border-gray-100 dark:border-white/5">
                <div className="flex items-center gap-2">
                  <div className="w-8 h-8 rounded-xl bg-indigo-500/10 text-indigo-600 dark:text-indigo-400 flex items-center justify-center shrink-0">
                    <Terminal className="w-4 h-4" />
                  </div>
                  <div>
                    <h3 className="text-xs font-bold text-gray-900 dark:text-zinc-100">
                      {tUi(lang, 'settings_cli_title')}
                    </h3>
                    <p className="text-[10px] text-gray-400">
                      {tUi(lang, 'settings_cli_subtitle')}
                    </p>
                  </div>
                </div>
                <span className="px-2.5 py-0.5 rounded-full bg-indigo-500/10 text-indigo-600 dark:text-indigo-400 text-[10px] font-mono font-semibold">
                  am v{appVersion}
                </span>
              </div>

              {/* 跨平台全局注册命令（自动识别当前系统，支持手动切换查看） */}
              {(() => {
                const detectedPlatform = detectCliPlatform(systemInfo.platform);
                const cliRegCmd = buildCliRegisterCommand(
                  cliPlatform,
                  detectedPlatform,
                  systemInfo.am_cli_path,
                  systemInfo.am_in_path
                );
                const regTitle =
                  cliPlatform === 'windows'
                    ? tUi(lang, 'settings_cli_register_win_title')
                    : cliPlatform === 'linux'
                      ? tUi(lang, 'settings_cli_register_linux_title')
                      : tUi(lang, 'settings_cli_symlink_title');
                const regDesc =
                  cliPlatform === 'windows'
                    ? tUi(lang, 'settings_cli_register_win_desc')
                    : cliPlatform === 'linux'
                      ? tUi(lang, 'settings_cli_register_linux_desc')
                      : tUi(lang, 'settings_cli_symlink_desc');
                const showInPathBadge = cliPlatform === detectedPlatform && Boolean(systemInfo.am_in_path);

                return (
                  <div className="p-3 rounded-xl bg-gray-50/80 dark:bg-white/[0.02] border border-gray-100 dark:border-white/5 flex flex-col gap-2.5">
                    <div className="flex flex-wrap items-start justify-between gap-2">
                      <div>
                        <div className="flex items-center gap-2">
                          <span className="text-[11px] font-semibold text-gray-800 dark:text-zinc-200">
                            {regTitle}
                          </span>
                          {showInPathBadge && (
                            <span className="inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded-full bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 text-[9.5px] font-semibold">
                              <Check className="w-2.5 h-2.5" />
                              {tUi(lang, 'settings_cli_in_path')}
                            </span>
                          )}
                        </div>
                        <div className="text-[9.5px] text-gray-400 mt-0.5">{regDesc}</div>
                      </div>
                      <div className="inline-flex p-0.5 rounded-lg bg-gray-100 dark:bg-white/[0.05] shrink-0">
                        {(
                          [
                            { id: 'macos', label: 'macOS' },
                            { id: 'windows', label: 'Windows' },
                            { id: 'linux', label: 'Linux' },
                          ] as const
                        ).map((os) => (
                          <button
                            key={os.id}
                            type="button"
                            onClick={() => {
                              cliPlatformManual.current = true;
                              setCliPlatform(os.id);
                            }}
                            className={`px-2 py-0.5 rounded-md text-[10px] font-semibold transition-all cursor-pointer ${
                              cliPlatform === os.id
                                ? 'bg-white dark:bg-[#22242b] text-indigo-600 dark:text-indigo-400 shadow-2xs'
                                : 'text-gray-500 dark:text-zinc-400 hover:text-gray-700 dark:hover:text-zinc-200'
                            }`}
                          >
                            {os.label}
                          </button>
                        ))}
                      </div>
                    </div>

                    <div className="flex items-center gap-2">
                      <code className="flex-1 min-w-0 px-2.5 py-1.5 rounded-md bg-gray-100 dark:bg-black/30 text-[10.5px] font-mono text-indigo-600 dark:text-indigo-400 select-all overflow-x-auto whitespace-nowrap">
                        {cliRegCmd}
                      </code>
                      <button
                        type="button"
                        onClick={() => handleCopy(cliRegCmd)}
                        className="p-1.5 rounded-md hover:bg-gray-200 dark:hover:bg-white/10 text-gray-500 shrink-0 cursor-pointer"
                      >
                        {copiedCmd === cliRegCmd ? (
                          <Check className="w-3.5 h-3.5 text-emerald-500" />
                        ) : (
                          <Copy className="w-3.5 h-3.5" />
                        )}
                      </button>
                    </div>
                  </div>
                );
              })()}
            </div>

            {/* CLI 命令矩阵速查表 */}
            <div className="bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] rounded-2xl p-4 shadow-2xs flex flex-col gap-3">
              <div className="pb-1.5 border-b border-gray-100 dark:border-white/5">
                <h3 className="text-xs font-bold text-gray-900 dark:text-zinc-100">
                  {tUi(lang, 'settings_cli_subcommands_title')}
                </h3>
              </div>

              <div className="grid grid-cols-1 md:grid-cols-2 gap-2.5">
                {[
                  {
                    category: tUi(lang, 'settings_cat_account_switch'),
                    items: [
                      { cmd: 'am list', desc: tUi(lang, 'settings_cli_list_desc') },
                      { cmd: 'am current', desc: tUi(lang, 'settings_cli_current_desc') },
                      { cmd: 'am switch --target ide --email <email>', desc: tUi(lang, 'settings_cli_switch_ide_desc') },
                      { cmd: 'am switch --target app --email <email>', desc: tUi(lang, 'settings_cli_switch_app_desc') },
                    ],
                  },
                  {
                    category: tUi(lang, 'settings_cat_refresh_health'),
                    items: [
                      { cmd: 'am refresh --all', desc: tUi(lang, 'settings_cli_refresh_all_desc') },
                      { cmd: 'am refresh --id <id>', desc: tUi(lang, 'settings_cli_refresh_id_desc') },
                      { cmd: 'am login', desc: tUi(lang, 'settings_cli_login_desc') },
                      { cmd: 'am add --email <email> --refresh-token <rt>', desc: tUi(lang, 'settings_cli_add_rt_desc') },
                    ],
                  },
                  {
                    category: tUi(lang, 'settings_cat_telemetry_stats'),
                    items: [
                      { cmd: 'am stats --view all', desc: tUi(lang, 'settings_cli_stats_desc') },
                      { cmd: 'am watch', desc: tUi(lang, 'settings_cli_watch_desc') },
                      { cmd: 'am context', desc: tUi(lang, 'settings_cli_context_desc') },
                    ],
                  },
                  {
                    category: tUi(lang, 'settings_cat_backup_config'),
                    items: [
                      { cmd: 'am export > backup.json', desc: tUi(lang, 'settings_cli_export_desc') },
                      { cmd: 'am import backup.json', desc: tUi(lang, 'settings_cli_import_desc') },
                      { cmd: 'am config --show', desc: tUi(lang, 'settings_cli_config_desc') },
                    ],
                  },
                ].map((sec) => (
                  <div
                    key={sec.category}
                    className="p-3 rounded-xl bg-gray-50/80 dark:bg-white/[0.02] border border-gray-100 dark:border-white/5 flex flex-col gap-2"
                  >
                    <div className="text-[11px] font-bold text-gray-900 dark:text-zinc-200 pb-1 border-b border-gray-100 dark:border-white/5">
                      {sec.category}
                    </div>
                    <div className="flex flex-col gap-2">
                      {sec.items.map((it) => (
                        <div key={it.cmd} className="flex items-center justify-between gap-2">
                          <div className="min-w-0 flex-1">
                            <div className="flex items-center gap-1.5">
                              <code className="text-[10.5px] font-mono text-indigo-600 dark:text-indigo-400 font-semibold truncate block">
                                {it.cmd}
                              </code>
                            </div>
                            <div className="text-[9.5px] text-gray-400 mt-0.5">{it.desc}</div>
                          </div>
                          <button
                            onClick={() => handleCopy(it.cmd)}
                            className="p-1 rounded-md hover:bg-gray-200 dark:hover:bg-white/10 text-gray-400 hover:text-gray-700 dark:hover:text-zinc-200 shrink-0 cursor-pointer"
                          >
                            {copiedCmd === it.cmd ? (
                              <Check className="w-3.5 h-3.5 text-emerald-500" />
                            ) : (
                              <Copy className="w-3.5 h-3.5" />
                            )}
                          </button>
                        </div>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </div>
        )}

        {/* ========================================================= */}
        {/* TAB 3: 数据存储与关于 (本地 SQLite 备份 + 版本与开源赞助)    */}
        {/* ========================================================= */}
        {activeSection === 'about' && (
          <div className="flex flex-col gap-3">
            {/* 本地 SQLite 存储与备份管理 */}
            <div className="bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] rounded-2xl p-4 shadow-2xs flex flex-col gap-3">
              <div className="pb-2 border-b border-gray-100 dark:border-white/5">
                <h3 className="text-xs font-bold text-gray-900 dark:text-zinc-100">
                  {tUi(lang, 'settings_storage_backup_title')}
                </h3>
                <p className="text-[10px] text-gray-400">
                  {tUi(lang, 'settings_storage_backup_desc')}
                </p>
              </div>

              <div className="grid grid-cols-3 gap-2.5">
                <div className="p-2.5 rounded-lg bg-gray-50/90 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5">
                  <span className="text-[9.5px] text-gray-400 block">{tUi(lang, 'settings_db_size')}</span>
                  <span className="text-sm font-bold font-mono text-gray-900 dark:text-zinc-100">
                    {systemInfo.db_size_formatted}
                  </span>
                </div>
                <div className="p-2.5 rounded-lg bg-gray-50/90 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5">
                  <span className="text-[9.5px] text-gray-400 block">{tUi(lang, 'settings_logged_reqs')}</span>
                  <span className="text-sm font-bold font-mono text-blue-600 dark:text-blue-400">
                    {systemInfo.total_logged_requests.toLocaleString()}
                  </span>
                </div>
                <div className="p-2.5 rounded-lg bg-gray-50/90 dark:bg-white/[0.03] border border-gray-100 dark:border-white/5">
                  <span className="text-[9.5px] text-gray-400 block">{tUi(lang, 'settings_memory')}</span>
                  <span className="text-sm font-bold font-mono text-emerald-600 dark:text-emerald-400">
                    {systemInfo.memory_usage_mb} MB
                  </span>
                </div>
              </div>

              <div className="flex flex-wrap items-center gap-2 pt-2 border-t border-gray-100 dark:border-white/5">
                <button
                  onClick={handleExportBackup}
                  className="px-3 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-[11px] font-semibold flex items-center gap-1.5 transition-colors cursor-pointer"
                >
                  <Download className="w-3.5 h-3.5" />
                  <span>{tUi(lang, 'settings_export_backup')}</span>
                </button>
                <button
                  onClick={handleImportBackup}
                  className="px-3 py-1.5 rounded-lg bg-gray-100 hover:bg-gray-200 dark:bg-white/[0.06] dark:hover:bg-white/10 text-gray-800 dark:text-zinc-200 text-[11px] font-semibold flex items-center gap-1.5 transition-colors cursor-pointer"
                >
                  <Upload className="w-3.5 h-3.5" />
                  <span>{tUi(lang, 'settings_import_backup')}</span>
                </button>
                <button
                  onClick={handlePruneLogs}
                  className="px-3 py-1.5 rounded-lg bg-rose-50 hover:bg-rose-100 dark:bg-rose-500/10 dark:hover:bg-rose-500/20 text-rose-600 dark:text-rose-400 text-[11px] font-semibold flex items-center gap-1.5 transition-colors ml-auto cursor-pointer"
                >
                  <Trash2 className="w-3.5 h-3.5" />
                  <span>{tUi(lang, 'settings_prune_logs')}</span>
                </button>
              </div>
            </div>

            {/* 关于与开源社区 */}
            <div className="bg-white dark:bg-[#16181e] border border-slate-200/90 dark:border-white/[0.10] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] rounded-2xl p-5 shadow-2xs flex flex-col items-center gap-5">
              <div className="text-center space-y-2.5">
                <div className="relative inline-block group">
                  <div className="absolute inset-0 bg-blue-500/20 rounded-3xl blur-xl group-hover:blur-2xl transition-all duration-500" />
                  <img
                    src="/icon.png"
                    onError={(e) => {
                      (e.currentTarget as HTMLImageElement).src = '/logo.png';
                    }}
                    alt="Antigravity Manager"
                    className="w-16 h-16 rounded-2xl shadow-xl relative z-10 transition-transform duration-300 group-hover:scale-105"
                  />
                </div>
                <div>
                  <h2 className="text-base font-black text-gray-900 dark:text-zinc-100 tracking-tight">
                    Antigravity Manager
                  </h2>
                  <div className="flex items-center justify-center gap-2 mt-1">
                    <span className="px-2 py-0.5 rounded-md bg-blue-50 dark:bg-blue-500/10 text-blue-600 dark:text-blue-400 text-[10px] font-mono font-bold">
                      v{appVersion}
                    </span>
                    <span className="text-[10px] text-gray-400">{tUi(lang, 'settings_about_tagline')}</span>
                  </div>
                </div>
              </div>

              {/* 5 列作者与社区信息网格 (对齐主分支) */}
              <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-5 gap-3 w-full max-w-4xl">
                {/* 1. 作者 */}
                <div className="bg-gray-50/80 dark:bg-white/[0.03] p-3 rounded-xl border border-gray-100 dark:border-white/5 flex flex-col items-center text-center gap-2">
                  <div className="p-2.5 bg-blue-50 dark:bg-blue-500/10 rounded-xl">
                    <User className="w-4 h-4 text-blue-500" />
                  </div>
                  <div>
                    <div className="text-[9.5px] text-gray-400 uppercase tracking-wider font-semibold">
                      {tUi(lang, 'settings_author')}
                    </div>
                    <div className="text-xs font-bold text-gray-900 dark:text-zinc-100 mt-0.5">Ctrler</div>
                  </div>
                </div>

                {/* 2. 微信公众号 */}
                <div className="bg-gray-50/80 dark:bg-white/[0.03] p-3 rounded-xl border border-gray-100 dark:border-white/5 flex flex-col items-center text-center gap-2">
                  <div className="p-2.5 bg-emerald-50 dark:bg-emerald-500/10 rounded-xl">
                    <MessageCircle className="w-4 h-4 text-emerald-500" />
                  </div>
                  <div>
                    <div className="text-[9.5px] text-gray-400 uppercase tracking-wider font-semibold">
                      {tUi(lang, 'settings_wechat')}
                    </div>
                    <div className="text-xs font-bold text-gray-900 dark:text-zinc-100 mt-0.5">Ctrler</div>
                  </div>
                </div>

                {/* 3. Telegram 频道 */}
                <a
                  href="https://t.me/AntigravityManager"
                  target="_blank"
                  rel="noreferrer"
                  onClick={(e) => {
                    e.preventDefault();
                    void api.openExternalUrl('https://t.me/AntigravityManager');
                  }}
                  className="bg-gray-50/80 dark:bg-white/[0.03] p-3 rounded-xl border border-gray-100 dark:border-white/5 hover:border-sky-300/60 dark:hover:border-sky-500/30 transition-all group flex flex-col items-center text-center gap-2 cursor-pointer"
                >
                  <div className="p-2.5 bg-sky-50 dark:bg-sky-500/10 rounded-xl group-hover:scale-105 transition-transform">
                    <Send className="w-4 h-4 text-sky-500" />
                  </div>
                  <div>
                    <div className="text-[9.5px] text-gray-400 uppercase tracking-wider font-semibold">Telegram</div>
                    <div className="text-xs font-bold text-sky-600 dark:text-sky-400 mt-0.5 flex items-center gap-0.5 justify-center">
                      <span>{tUi(lang, 'settings_channel')}</span>
                      <ExternalLink className="w-2.5 h-2.5" />
                    </div>
                  </div>
                </a>

                {/* 4. 开源地址 */}
                <a
                  href="https://github.com/lbjlaq/Antigravity-Manager"
                  target="_blank"
                  rel="noreferrer"
                  onClick={(e) => {
                    e.preventDefault();
                    void api.openExternalUrl('https://github.com/lbjlaq/Antigravity-Manager');
                  }}
                  className="bg-gray-50/80 dark:bg-white/[0.03] p-3 rounded-xl border border-gray-100 dark:border-white/5 hover:border-gray-400/60 dark:hover:border-white/20 transition-all group flex flex-col items-center text-center gap-2 cursor-pointer"
                >
                  <div className="p-2.5 bg-gray-100 dark:bg-white/10 rounded-xl group-hover:scale-105 transition-transform">
                    <Github className="w-4 h-4 text-gray-800 dark:text-zinc-100" />
                  </div>
                  <div>
                    <div className="text-[9.5px] text-gray-400 uppercase tracking-wider font-semibold">
                      {tUi(lang, 'settings_github')}
                    </div>
                    <div className="text-xs font-bold text-gray-900 dark:text-zinc-100 mt-0.5 flex items-center gap-0.5 justify-center">
                      <span>{tUi(lang, 'settings_view_code')}</span>
                      <ExternalLink className="w-2.5 h-2.5 text-gray-400" />
                    </div>
                  </div>
                </a>

                {/* 5. 赞助支持 */}
                <div
                  onClick={() => setSupportModalOpen(true)}
                  className="bg-gray-50/80 dark:bg-white/[0.03] p-3 rounded-xl border border-gray-100 dark:border-white/5 hover:border-pink-300/70 dark:hover:border-pink-500/40 transition-all group flex flex-col items-center text-center gap-2 cursor-pointer"
                >
                  <div className="p-2.5 bg-pink-50 dark:bg-pink-500/10 rounded-xl group-hover:scale-105 transition-transform">
                    <Heart className="w-4 h-4 text-pink-500 fill-pink-500" />
                  </div>
                  <div>
                    <div className="text-[9.5px] text-gray-400 uppercase tracking-wider font-semibold">
                      {tUi(lang, 'settings_support')}
                    </div>
                    <div className="text-xs font-bold text-pink-600 dark:text-pink-400 mt-0.5">
                      {tUi(lang, 'settings_sponsor_author')}
                    </div>
                  </div>
                </div>
              </div>

              {/* 技术栈徽章 + 更新通道切换 */}
              <div className="w-full pt-3 border-t border-gray-100 dark:border-white/5 flex flex-col gap-2">
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div className="flex items-center gap-1.5">
                    {['Tauri v2', 'Rust am-core', 'React 18', 'TypeScript'].map((tech) => (
                      <span
                        key={tech}
                        className="px-2.5 py-0.5 rounded-md bg-gray-100/80 dark:bg-white/[0.04] text-[10px] font-mono text-gray-500 dark:text-zinc-400"
                      >
                        {tech}
                      </span>
                    ))}
                  </div>

                  <div className="flex flex-wrap items-center gap-2 text-[10.5px]">
                    <span className="px-2 py-0.5 rounded-md bg-blue-500/10 text-blue-600 dark:text-blue-400 text-[10px] font-semibold">
                      {tUi(lang, 'settings_channel_stable')}
                    </span>
                    <button
                      onClick={async () => {
                        setCheckingUpdate(true);
                        setUpdateError(null);
                        try {
                          const info = await api.checkForUpdates();
                          setUpdateInfo(info);
                        } catch (e: any) {
                          setUpdateError(e?.message || String(e));
                        } finally {
                          setCheckingUpdate(false);
                        }
                      }}
                      disabled={checkingUpdate}
                      className="px-2.5 py-1 rounded-lg bg-gray-100 hover:bg-gray-200 dark:bg-white/[0.06] dark:hover:bg-white/10 text-gray-700 dark:text-zinc-200 text-[10px] font-semibold inline-flex items-center gap-1 transition-colors cursor-pointer disabled:opacity-50"
                    >
                      <RefreshCw className={`w-3 h-3 ${checkingUpdate ? 'animate-spin text-blue-500' : ''}`} />
                      <span>
                        {checkingUpdate
                          ? tUi(lang, 'settings_update_checking')
                          : tUi(lang, 'settings_update_check')}
                      </span>
                    </button>
                    {updateInfo && !checkingUpdate && (
                      updateInfo.has_update ? (
                        <div className="inline-flex items-center gap-1.5">
                          <button
                            disabled={installingUpdate}
                            onClick={async () => {
                              setInstallingUpdate(true);
                              setInstallPct(0);
                              setUpdateError(null);
                              let unlisten: (() => void) | undefined;
                              try {
                                if (isTauri()) {
                                  const { listen } = await import('@tauri-apps/api/event');
                                  unlisten = await listen<{ pct?: number }>('updater://progress', (evt) => {
                                    if (typeof evt.payload?.pct === 'number') {
                                      setInstallPct(evt.payload.pct);
                                    }
                                  });
                                }
                                await api.installUpdate();
                              } catch (e: any) {
                                setUpdateError(e?.message || String(e));
                              } finally {
                                unlisten?.();
                                setInstallingUpdate(false);
                              }
                            }}
                            className="px-2.5 py-1 rounded-lg bg-emerald-500 hover:bg-emerald-600 disabled:opacity-60 text-white text-[10px] font-semibold inline-flex items-center gap-1 transition-colors cursor-pointer shadow-2xs"
                          >
                            {installingUpdate ? (
                              <RefreshCw className="w-3 h-3 animate-spin" />
                            ) : (
                              <Download className="w-3 h-3" />
                            )}
                            <span>
                              {installingUpdate
                                ? tUi(lang, 'settings_update_installing', {
                                    pct: installPct > 0 ? `${installPct}%` : '...',
                                  })
                                : tUi(lang, 'settings_update_available', { version: updateInfo.latest_version })}
                            </span>
                          </button>
                          <button
                            onClick={() => api.openExternalUrl(updateInfo.download_url)}
                            title={tUi(lang, 'settings_update_manual_download')}
                            className="p-1 rounded-lg bg-gray-100 hover:bg-gray-200 dark:bg-white/[0.06] dark:hover:bg-white/10 text-gray-600 dark:text-zinc-300 transition-colors cursor-pointer"
                          >
                            <ExternalLink className="w-3 h-3" />
                          </button>
                        </div>
                      ) : (
                        <span className="text-[10px] font-semibold text-emerald-600 dark:text-emerald-400 inline-flex items-center gap-1">
                          <Check className="w-3 h-3" />
                          {tUi(lang, 'settings_update_latest', { version: updateInfo.latest_version })}
                        </span>
                      )
                    )}
                    {updateError && !checkingUpdate && (
                      <span className="text-[10px] text-rose-500 truncate max-w-[220px]" title={updateError}>
                        {tUi(lang, 'settings_update_failed')}
                      </span>
                    )}
                  </div>
                </div>

                {updateInfo && !checkingUpdate && updateInfo.release_notes.trim().length > 0 && (
                  <div className="mt-2 p-3 rounded-xl bg-gray-50/90 dark:bg-white/[0.03] border border-gray-200/70 dark:border-white/10 text-left">
                    <div className="flex items-center justify-between gap-2 mb-1.5">
                      <span className="text-[10.5px] font-bold text-gray-800 dark:text-zinc-200">
                        {tUi(lang, 'settings_update_release_notes', { version: updateInfo.latest_version })}
                      </span>
                      {updateInfo.published_at && (
                        <span className="text-[9.5px] font-mono text-gray-400">
                          {updateInfo.published_at.slice(0, 10)}
                        </span>
                      )}
                    </div>
                    <pre className="text-[10.5px] leading-relaxed text-gray-600 dark:text-zinc-300 whitespace-pre-wrap font-sans max-h-40 overflow-y-auto">
                      {updateInfo.release_notes}
                    </pre>
                  </div>
                )}
              </div>

              <div className="text-center text-[10px] text-gray-400 dark:text-zinc-500 font-sans pt-1">
                Copyright © 2025-2026 Antigravity. All rights reserved.
              </div>
            </div>

            {/* 赞助弹窗 */}
            {supportModalOpen && (
              <div
                className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex items-center justify-center p-4 animate-in fade-in duration-200"
                onClick={() => setSupportModalOpen(false)}
              >
                <div
                  className="relative w-full max-w-xl bg-white dark:bg-[#16171b] border border-gray-200/80 dark:border-white/10 rounded-3xl p-6 md:p-8 shadow-2xl flex flex-col items-center text-center gap-5 animate-in zoom-in-95 duration-200"
                  onClick={(e) => e.stopPropagation()}
                >
                  <button
                    onClick={() => setSupportModalOpen(false)}
                    className="absolute top-5 right-5 p-1.5 rounded-full hover:bg-gray-100 dark:hover:bg-white/10 text-gray-400 hover:text-gray-600 dark:hover:text-zinc-200 transition-colors cursor-pointer"
                  >
                    <X className="w-4 h-4" />
                  </button>

                  <div className="w-14 h-14 bg-pink-50 dark:bg-pink-900/20 rounded-2xl flex items-center justify-center shadow-xs">
                    <Coffee className="w-7 h-7 text-pink-500" />
                  </div>

                  <div>
                    <h3 className="text-xl font-black text-gray-900 dark:text-zinc-100 mb-1.5">
                      {tUi(lang, 'settings_sponsor_title')}
                    </h3>
                    <p className="text-xs text-gray-500 dark:text-zinc-400 max-w-md leading-relaxed mx-auto">
                      {tUi(lang, 'settings_sponsor_desc')}
                    </p>
                  </div>

                  <div className="grid grid-cols-1 sm:grid-cols-3 gap-3.5 w-full">
                    <div className="flex flex-col items-center gap-2 p-3 rounded-2xl bg-gray-50 dark:bg-white/[0.02] border border-gray-100 dark:border-white/5">
                      <div className="w-full aspect-square relative bg-white rounded-xl overflow-hidden shadow-xs border border-gray-100 flex items-center justify-center p-1">
                        <img src="/images/donate/alipay.png" alt="Alipay" className="w-full h-full object-contain" />
                      </div>
                      <span className="text-xs font-bold text-gray-700 dark:text-zinc-200">
                        {tUi(lang, 'settings_alipay')}
                      </span>
                    </div>

                    <div className="flex flex-col items-center gap-2 p-3 rounded-2xl bg-gray-50 dark:bg-white/[0.02] border border-gray-100 dark:border-white/5">
                      <div className="w-full aspect-square relative bg-white rounded-xl overflow-hidden shadow-xs border border-gray-100 flex items-center justify-center p-1">
                        <img src="/images/donate/wechat.png" alt="WeChat" className="w-full h-full object-contain" />
                      </div>
                      <span className="text-xs font-bold text-gray-700 dark:text-zinc-200">
                        {tUi(lang, 'settings_wechat_pay')}
                      </span>
                    </div>

                    <a
                      href="https://www.buymeacoffee.com/Ctrler"
                      target="_blank"
                      rel="noreferrer"
                      onClick={(e) => {
                        e.preventDefault();
                        void api.openExternalUrl('https://www.buymeacoffee.com/Ctrler');
                      }}
                      className="flex flex-col items-center gap-2 p-3 rounded-2xl bg-gray-50 dark:bg-white/[0.02] border border-gray-100 dark:border-white/5 hover:border-amber-300/70 dark:hover:border-amber-500/40 transition-colors cursor-pointer group"
                    >
                      <div className="w-full aspect-square relative bg-white rounded-xl overflow-hidden shadow-xs border border-gray-100 flex items-center justify-center p-1">
                        <img src="/images/donate/coffee.png" alt="Buy Me A Coffee" className="w-full h-full object-contain" />
                      </div>
                      <span className="text-xs font-bold text-gray-700 dark:text-zinc-200 group-hover:text-amber-600 dark:group-hover:text-amber-400 flex items-center gap-1 transition-colors">
                        <span>Buy Me a Coffee</span>
                        <ExternalLink className="w-2.5 h-2.5 opacity-70" />
                      </span>
                    </a>
                  </div>

                  <div className="flex items-center gap-3 w-full justify-center pt-1">
                    <a
                      href="https://github.com/sponsors/lbjlaq"
                      target="_blank"
                      rel="noreferrer"
                      onClick={(e) => {
                        e.preventDefault();
                        void api.openExternalUrl('https://github.com/sponsors/lbjlaq');
                      }}
                      className="px-5 py-2 rounded-xl bg-pink-500 hover:bg-pink-600 text-white font-semibold text-xs transition-colors shadow-xs inline-flex items-center gap-1.5 cursor-pointer"
                    >
                      <span>GitHub Sponsors ❤️</span>
                      <ExternalLink className="w-3 h-3 opacity-85" />
                    </a>
                    <button
                      onClick={() => setSupportModalOpen(false)}
                      className="px-6 py-2 rounded-xl bg-gray-100 hover:bg-gray-200 dark:bg-white/[0.06] dark:hover:bg-white/10 text-gray-700 dark:text-zinc-200 font-semibold text-xs transition-colors cursor-pointer"
                    >
                      {tUi(lang, 'settings_close')}
                    </button>
                  </div>
                </div>
              </div>
            )}
          </div>
        )}
      </section>
    </div>
  );
};