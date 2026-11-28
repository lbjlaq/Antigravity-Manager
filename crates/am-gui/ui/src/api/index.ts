// Antigravity Manager 前后端全量通信桥接层 (Tauri 2 IPC + Browser Dev Fallback)
import packageJson from '../../package.json';

export const APP_VERSION_FALLBACK = packageJson.version;

export type HostTarget = 'app' | 'ide' | 'plugin' | 'cli';

export interface ActiveHostStatus {
  app_account?: string | null;
  app_running: boolean;
  app_version?: string | null;
  ide_account?: string | null;
  ide_running: boolean;
  ide_version?: string | null;
  plugin_account?: string | null;
  plugin_running: boolean;
  agy_account?: string | null;
  agy_running?: boolean;
}

export interface ModelQuota {
  name: string;
  percentage: number;
  reset_time?: string;
  display_name?: string;
}

export interface QuotaBucket {
  bucket_id?: string;
  window?: string;
  remaining_fraction?: number;
  reset_time?: string;
  observed_at?: number;
  display_name?: string;
  description?: string;
}

export interface QuotaGroup {
  display_name?: string;
  description?: string;
  buckets: QuotaBucket[];
}

export interface QuotaSummary {
  gemini_percentage?: number;
  claude_percentage?: number;
  subscription_tier?: string;
  raw_tier?: string;
  is_forbidden?: boolean;
  forbidden_reason?: string;
  quota_groups?: QuotaGroup[];
  models: ModelQuota[];
  last_updated?: number;
}

export interface DeviceProfile {
  machine_id: string;
  mac_machine_id: string;
  dev_device_id: string;
  sqm_id: string;
}

export interface TokenInfo {
  access_token: string;
  refresh_token: string;
  expires_in?: number;
  token_type?: string;
  expiry_timestamp?: number;
  project_id?: string;
}

export interface BackendAccount {
  id: string;
  email: string;
  name?: string;
  priority?: number;
  token: TokenInfo;
  device_profile?: DeviceProfile;
  quota?: QuotaSummary;
  tags: string[];
  disabled: boolean;
  disabled_reason?: string;
  disabled_at?: number;
  banned: boolean;
  banned_reason?: string;
  banned_at?: number;
  banned_code?: string;
  proxy_disabled?: boolean;
  proxy_disabled_reason?: string;
  proxy_disabled_at?: number;
  manual_proxy_disabled?: boolean;
  validation_blocked?: boolean;
  created_at?: number;
  last_used?: number;
}

export interface QuotaPollingConfig {
  enabled: boolean;
  interval_minutes: number;
  active_only: boolean;
}

export interface AutoSwitchConfig {
  enabled: boolean;
  targets: string[];
  trigger_exhaustion_threshold: number;
  trigger_on_rate_limit: boolean;
  strategy: string;
  pool: string;
  desktop_notification: boolean;
}

export interface CustomPathsConfig {
  app?: string;
  ide?: string;
  plugin?: string;
  cli?: string;
}

export type TrayClickAction = 'popover' | 'main' | 'menu';

export interface TrayConfig {
  enabled: boolean;
  left_click_action: TrayClickAction;
  close_to_tray: boolean;
  auto_launch?: boolean;
  quiet_autostart?: boolean;
  lightweight_mode?: boolean;
}

export interface UiStateConfig {
  last_tab?: string;
  dashboard_host?: string;
  dashboard_pick_strategy?: string;
  dashboard_pool_preference?: string;
  accounts_sort_mode?: string;
  accounts_view_mode?: string;
}

export interface UpdateConfig {
  auto_check: boolean;
  check_interval_hours?: number;
  last_check_time?: number;
  skipped_version?: string | null;
}

export interface AppConfig {
  quota_polling: QuotaPollingConfig;
  auto_switch: AutoSwitchConfig;
  custom_paths?: CustomPathsConfig;
  tray?: TrayConfig;
  ui_state?: UiStateConfig;
  update?: UpdateConfig;
}

export type InstallChannel = 'native' | 'homebrew' | 'manual';

export type UpdatePhase =
  | 'idle'
  | 'checking'
  | 'up_to_date'
  | 'available'
  | 'downloading'
  | 'installing'
  | 'ready'
  | 'error';

export interface UpdateInfo {
  current_version: string;
  latest_version: string;
  has_update: boolean;
  download_url: string;
  release_notes: string;
  published_at: string;
  install_channel?: InstallChannel;
}

export interface UpdateSnapshot {
  phase: UpdatePhase;
  info: UpdateInfo | null;
  install_channel: InstallChannel;
  progress_pct: number;
  downloaded_bytes: number;
  total_bytes: number;
  error: string | null;
  banner_dismissed: boolean;
  triggered_by_auto: boolean;
  updated_via_brew: boolean;
}

export interface SystemInfo {
  app_version?: string;
  platform?: string;
  am_cli_path?: string | null;
  am_in_path?: boolean;
  db_size_bytes: number;
  db_size_formatted: string;
  total_logged_requests: number;
  memory_usage_mb: number;
  config_path: string;
  app_installed: boolean;
  app_path: string;
  app_live: boolean;
  ide_installed: boolean;
  ide_path: string;
  ide_live: boolean;
  plugin_installed: boolean;
  plugin_path: string;
  plugin_live: boolean;
  cli_installed: boolean;
  cli_path: string;
  cli_live: boolean;
}

export interface ImportReport {
  imported: number;
  overwritten: number;
  skipped: number;
}

export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

async function invokeTauri<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

export const api = {
  // 1. 获取全景统计报告（可选传入毫秒级时间范围 startTsMs / endTsMs）
  async getStatsReport(range?: { startTsMs?: number | null; endTsMs?: number | null }): Promise<any> {
    if (isTauri()) {
      return invokeTauri<any>('get_stats_report', {
        startTsMs: range?.startTsMs ?? null,
        endTsMs: range?.endTsMs ?? null,
      });
    }
    try {
      const res = await fetch('/api/stats');
      if (res.ok) return await res.json();
    } catch {}
    return null;
  },

  // 2. 获取多端宿主激活状态
  async getActiveHosts(): Promise<ActiveHostStatus | null> {
    if (isTauri()) {
      return invokeTauri<ActiveHostStatus>('get_active_hosts');
    }
    try {
      const res = await fetch('/api/hosts');
      if (res.ok) return await res.json();
    } catch {}
    return null;
  },

  // 3. 列出所有完整账号
  async getAccounts(): Promise<BackendAccount[] | null> {
    if (isTauri()) {
      return invokeTauri<BackendAccount[]>('get_accounts');
    }
    try {
      const res = await fetch('/api/accounts');
      if (res.ok) return await res.json();
    } catch {}
    return null;
  },

  // 4. 切换特定宿主的账号
  async switchAccount(account: string, target: HostTarget, restart?: boolean): Promise<BackendAccount> {
    if (!isTauri()) {
      console.warn('[Web Mock] switchAccount invoked in browser mode:', { account, target, restart });
      return {
        id: account,
        email: account.includes('@') ? account : `${account}@example.com`,
        token: { access_token: '', refresh_token: '' },
        tags: [],
        disabled: false,
        banned: false,
      };
    }
    return invokeTauri<BackendAccount>('switch_account', {
      account,
      target,
      restart: restart ?? false,
    });
  },

  // 5. 智能为宿主选号
  async smartPickAccount(
    target: HostTarget,
    strategy?: string,
    prefer?: string,
    tag?: string
  ): Promise<BackendAccount | null> {
    if (!isTauri()) {
      console.warn('[Web Mock] smartPickAccount in browser mode');
      return null;
    }
    return invokeTauri<BackendAccount | null>('smart_pick_account', {
      target,
      strategy,
      prefer,
      tag,
    });
  },

  // 6. 刷新单账号配额
  async refreshAccountQuota(accountId: string): Promise<QuotaSummary> {
    if (!isTauri()) {
      console.warn('[Web Mock] refreshAccountQuota:', accountId);
      return { models: [] };
    }
    return invokeTauri<QuotaSummary>('refresh_account_quota', { accountId });
  },

  // 7. 刷新全量账号配额
  async refreshAllQuotas(): Promise<Array<[string, boolean]>> {
    if (!isTauri()) {
      console.warn('[Web Mock] refreshAllQuotas in browser mode');
      return [];
    }
    return invokeTauri<Array<[string, boolean]>>('refresh_all_quotas');
  },

  // 8. 重新持久化账号排序
  async reorderAccounts(accountIds: string[]): Promise<void> {
    if (!isTauri()) {
      console.warn('[Web Mock] reorderAccounts in browser mode:', accountIds);
      return;
    }
    return invokeTauri<void>('reorder_accounts', { accountIds });
  },

  // 9. 删除账号
  async deleteAccount(accountId: string): Promise<boolean> {
    if (!isTauri()) {
      console.warn('[Web Mock] deleteAccount in browser mode:', accountId);
      return true;
    }
    return invokeTauri<boolean>('delete_account', { accountId });
  },

  // 9. 批量删除账号
  async deleteAccounts(accountIds: string[]): Promise<number> {
    if (!isTauri()) {
      console.warn('[Web Mock] deleteAccounts in browser mode:', accountIds);
      return accountIds.length;
    }
    return invokeTauri<number>('delete_accounts', { accountIds });
  },

  // 10. 设置账号标签
  async setAccountTags(accountId: string, tags: string[]): Promise<BackendAccount> {
    if (!isTauri()) {
      console.warn('[Web Mock] setAccountTags in browser mode:', accountId, tags);
      return {
        id: accountId,
        email: 'mock@example.com',
        token: { access_token: '', refresh_token: '' },
        tags,
        disabled: false,
        banned: false,
      };
    }
    return invokeTauri<BackendAccount>('set_account_tags', { accountId, tags });
  },

  // 11. 手工根据 Refresh Token 添加账号
  async addAccountByToken(
    refreshToken: string,
    tags: string[] = [],
    oauthClientKey?: string | null
  ): Promise<BackendAccount> {
    if (!isTauri()) {
      console.warn('[Web Mock] addAccountByToken in browser mode');
      return {
        id: 'mock_token_acc',
        email: 'token_user@example.com',
        token: { access_token: '', refresh_token: refreshToken },
        tags,
        disabled: false,
        banned: false,
      };
    }
    return invokeTauri<BackendAccount>('add_account_by_token', {
      refreshToken,
      tags,
      oauthClientKey: oauthClientKey ?? null,
    });
  },

  // 12. Google OAuth 授权登录 (预生成链接 / 打开浏览器 / 等待完成 / 取消 / 手动提交码)
  async prepareOAuthUrl(oauthClientKey?: string | null): Promise<string> {
    if (!isTauri()) throw new Error('OAuth login requires native Tauri runtime');
    return invokeTauri<string>('prepare_oauth_url', {
      oauthClientKey: oauthClientKey ?? null,
    });
  },

  async startOAuthLogin(oauthClientKey?: string | null): Promise<BackendAccount> {
    if (!isTauri()) throw new Error('OAuth login requires native Tauri runtime');
    return invokeTauri<BackendAccount>('start_oauth_login', {
      oauthClientKey: oauthClientKey ?? null,
    });
  },

  async completeOAuthLogin(): Promise<BackendAccount> {
    if (!isTauri()) throw new Error('OAuth login requires native Tauri runtime');
    return invokeTauri<BackendAccount>('complete_oauth_login');
  },

  async cancelOAuthLogin(): Promise<void> {
    if (!isTauri()) return;
    return invokeTauri<void>('cancel_oauth_login');
  },

  async submitOAuthCode(code: string, state?: string | null): Promise<BackendAccount> {
    if (!isTauri()) throw new Error('OAuth login requires native Tauri runtime');
    return invokeTauri<BackendAccount>('submit_oauth_code', {
      code,
      state: state ?? null,
    });
  },

  // 12.1 从本地 Antigravity/IDE/CLI 或自定义 state.vscdb 导入账号
  async importFromLocalDb(): Promise<BackendAccount[]> {
    if (!isTauri()) throw new Error('Storage import only works in desktop app.');
    return invokeTauri<BackendAccount[]>('import_from_local_db');
  },

  async importFromCustomDb(dbPath: string): Promise<BackendAccount> {
    if (!isTauri()) throw new Error('Storage import only works in desktop app.');
    return invokeTauri<BackendAccount>('import_from_custom_db', { dbPath });
  },

  async pickVscdbFile(): Promise<string | null> {
    if (!isTauri()) return null;
    const selected = await invokeTauri<string | string[] | null>('plugin:dialog|open', {
      options: {
        multiple: false,
        filters: [
          { name: 'VSCode DB', extensions: ['vscdb'] },
          { name: 'All Files', extensions: ['*'] },
        ],
      },
    });
    return typeof selected === 'string' ? selected : null;
  },

  // 13. 重新生成并隔离设备指纹
  async regenerateDeviceProfile(accountId: string): Promise<BackendAccount> {
    if (!isTauri()) {
      console.warn('[Web Mock] regenerateDeviceProfile:', accountId);
      return {
        id: accountId,
        email: 'mock@example.com',
        token: { access_token: '', refresh_token: '' },
        tags: [],
        disabled: false,
        banned: false,
      };
    }
    return invokeTauri<BackendAccount>('regenerate_device_profile', { accountId });
  },

  // 13.1 手动解除账号 403 锁定与验证限制
  async clearAccountForbidden(accountId: string): Promise<BackendAccount> {
    if (!isTauri()) {
      console.warn('[Web Mock] clearAccountForbidden:', accountId);
      return {
        id: accountId,
        email: 'mock@example.com',
        token: { access_token: '', refresh_token: '' },
        tags: [],
        disabled: false,
        banned: false,
      };
    }
    return invokeTauri<BackendAccount>('clear_account_forbidden', { accountId });
  },

  // 14. 禁用账号
  async disableAccount(accountId: string, reason?: string): Promise<BackendAccount> {
    if (!isTauri()) {
      return {
        id: accountId,
        email: 'mock@example.com',
        token: { access_token: '', refresh_token: '' },
        tags: [],
        disabled: true,
        disabled_reason: reason,
        banned: false,
      };
    }
    return invokeTauri<BackendAccount>('disable_account', { accountId, reason });
  },

  // 15. 启用账号
  async enableAccount(accountId: string): Promise<BackendAccount> {
    if (!isTauri()) {
      return {
        id: accountId,
        email: 'mock@example.com',
        token: { access_token: '', refresh_token: '' },
        tags: [],
        disabled: false,
        banned: false,
      };
    }
    return invokeTauri<BackendAccount>('enable_account', { accountId });
  },

  // 16. 导出备份包 JSON 字符串
  async exportAccountsBackup(): Promise<string> {
    if (!isTauri()) return JSON.stringify([], null, 2);
    return invokeTauri<string>('export_accounts_backup');
  },

  // 17. 导入备份包
  async importAccountsBackup(backupJson: string, overwrite: boolean = false): Promise<ImportReport> {
    if (!isTauri()) {
      return { imported: 1, overwritten: 0, skipped: 0 };
    }
    return invokeTauri<ImportReport>('import_accounts_backup', { backupJson, overwrite });
  },

  // 18. 获取全局管理配置
  async getAppConfig(): Promise<AppConfig | null> {
    if (!isTauri()) return null;
    return invokeTauri<AppConfig>('get_app_config');
  },

  // 19. 保存全局管理配置
  async saveAppConfig(config: AppConfig): Promise<void> {
    if (!isTauri()) {
      console.warn('[Web Mock] saveAppConfig in browser mode:', config);
      return;
    }
    return invokeTauri<void>('save_app_config', { config });
  },

  // 19.1 增量持久化 UI 记忆状态到 config.json
  async patchUiState(patch: UiStateConfig): Promise<UiStateConfig | null> {
    if (!isTauri()) return patch;
    return invokeTauri<UiStateConfig>('patch_ui_state', { patch });
  },

  // 20. 获取系统宿主与遥测库状态
  async getSystemInfo(): Promise<SystemInfo | null> {
    if (!isTauri()) return null;
    return invokeTauri<SystemInfo>('get_system_info');
  },

  // 20.1 获取应用版本号
  async getAppVersion(): Promise<string> {
    if (!isTauri()) return APP_VERSION_FALLBACK;
    return invokeTauri<string>('get_app_version');
  },

  // 21. 打开系统外部默认浏览器链接
  async openExternalUrl(url: string): Promise<void> {
    if (isTauri()) {
      try {
        await invokeTauri<void>('open_external_url', { url });
        return;
      } catch (e) {
        console.warn('invoke open_external_url failed, falling back to window.open:', e);
      }
    }
    window.open(url, '_blank', 'noreferrer,noopener');
  },

  // 22. 清理过期流水日志
  async pruneLogs(days?: number): Promise<number> {
    if (!isTauri()) return 0;
    return invokeTauri<number>('prune_logs', { days });
  },

  // 23. 检查 GitHub Releases 最新正式版在线更新 (Stable Only)
  async getUpdateState(): Promise<UpdateSnapshot> {
    if (!isTauri()) {
      return {
        phase: 'idle',
        info: null,
        install_channel: 'native',
        progress_pct: 0,
        downloaded_bytes: 0,
        total_bytes: 0,
        error: null,
        banner_dismissed: false,
        triggered_by_auto: false,
        updated_via_brew: false,
      };
    }
    return invokeTauri<UpdateSnapshot>('get_update_state');
  },

  async checkForUpdates(): Promise<UpdateInfo> {
    if (!isTauri()) {
      return {
        current_version: APP_VERSION_FALLBACK,
        latest_version: APP_VERSION_FALLBACK,
        has_update: false,
        download_url: 'https://github.com/lbjlaq/Antigravity-Manager/releases/tag/antigravity-manager',
        release_notes: '',
        published_at: '',
        install_channel: 'native',
      };
    }
    return invokeTauri<UpdateInfo>('check_for_updates');
  },

  // 23.1 第一阶段：下载并验签（或通过 Homebrew Cask 升级），不提前覆盖磁盘应用包
  async downloadUpdate(forceNative = false): Promise<void> {
    if (!isTauri()) return;
    return invokeTauri<void>('download_update', { forceNative });
  },

  // 23.2 第二阶段：将已验签的更新落盘替换并安全重启
  async applyAndRestartUpdate(): Promise<void> {
    if (!isTauri()) return;
    return invokeTauri<void>('apply_and_restart_update');
  },

  async dismissUpdateBanner(): Promise<UpdateSnapshot | null> {
    if (!isTauri()) return null;
    return invokeTauri<UpdateSnapshot>('dismiss_update_banner');
  },

  async skipUpdateVersion(version: string): Promise<UpdateSnapshot | null> {
    if (!isTauri()) return null;
    return invokeTauri<UpdateSnapshot>('skip_update_version', { version });
  },

  // 23.3 兼容接口：应用内一键下载、验签、安装并重启更新
  async installUpdate(forceNative = false): Promise<void> {
    if (!isTauri()) return;
    return invokeTauri<void>('install_update', { forceNative });
  },

  // 24. 托盘与悬浮窗控制接口
  async showMainWindow(tab?: 'dashboard' | 'accounts' | 'stats' | 'settings'): Promise<void> {
    if (tab && typeof window !== 'undefined') {
      try {
        localStorage.setItem('am_pending_tab', JSON.stringify({ tab, ts: Date.now() }));
      } catch {}
    }
    if (!isTauri()) return;
    return invokeTauri<void>('show_main_window', { tab: tab ?? null });
  },

  async hideMiniPopover(): Promise<void> {
    if (!isTauri()) return;
    return invokeTauri<void>('hide_mini_popover');
  },

  async setMiniPinned(pinned: boolean): Promise<boolean> {
    if (!isTauri()) return pinned;
    return invokeTauri<boolean>('set_mini_pinned', { pinned });
  },

  async getMiniPinned(): Promise<boolean> {
    if (!isTauri()) return false;
    return invokeTauri<boolean>('get_mini_pinned');
  },

  async quitApp(): Promise<void> {
    if (!isTauri()) return;
    return invokeTauri<void>('quit_app');
  },

  async toggleAutoLaunch(enable: boolean): Promise<boolean> {
    if (!isTauri()) return enable;
    return invokeTauri<boolean>('toggle_auto_launch', { enable });
  },

  async isAutoLaunchEnabled(): Promise<boolean> {
    if (!isTauri()) return false;
    return invokeTauri<boolean>('is_auto_launch_enabled');
  },

  async syncUiPreferences(prefs: { lang?: string; theme?: string }): Promise<void> {
    if (!isTauri()) return;
    return invokeTauri<void>('sync_ui_preferences', prefs);
  },

  // 25. 免 TUN 代理与工具箱接口
  async proxyDetectNodes(): Promise<ProxyNode[]> {
    if (!isTauri()) {
      return [
        {
          name: 'Clash / Mihomo (Mock)',
          host: '127.0.0.1',
          port: 7890,
          protocol: 'http',
          latency_ms: 12,
          source: 'auto_detected',
        },
      ];
    }
    return invokeTauri<ProxyNode[]>('proxy_detect_nodes');
  },

  async proxyGetStatus(): Promise<ProxyStatus> {
    if (!isTauri()) {
      return { running: false, local_addr: null, upstream: null };
    }
    return invokeTauri<ProxyStatus>('proxy_get_status');
  },

  async proxyStartOrSwitch(node: ProxyNode | null, port?: number): Promise<ProxyStatus> {
    if (!isTauri()) {
      return { running: true, local_addr: `http://127.0.0.1:${port ?? 17890}`, upstream: node };
    }
    return invokeTauri<ProxyStatus>('proxy_start_or_switch', { node, port: port ?? null });
  },

  async proxyStop(): Promise<ProxyStatus> {
    if (!isTauri()) {
      return { running: false, local_addr: null, upstream: null };
    }
    return invokeTauri<ProxyStatus>('proxy_stop');
  },

  async proxyLaunchApp(target: 'app' | 'ide' | 'cli', proxyUrl?: string): Promise<number> {
    if (!isTauri()) return 0;
    return invokeTauri<number>('proxy_launch_app', { target, proxyUrl: proxyUrl ?? null });
  },

  async proxyInstallShellHook(enable: boolean, proxyUrl?: string): Promise<string> {
    if (!isTauri()) return '~/.zshrc';
    return invokeTauri<string>('proxy_install_shell_hook', { enable, proxyUrl: proxyUrl ?? null });
  },

  async proxyCreateMacosWrapper(target: 'app' | 'ide', proxyUrl?: string): Promise<string> {
    if (!isTauri()) return '~/Desktop/Antigravity (No-TUN).app';
    return invokeTauri<string>('proxy_create_macos_wrapper', { target, proxyUrl: proxyUrl ?? null });
  },

  async proxyRemoveMacosWrapper(target: 'app' | 'ide'): Promise<boolean> {
    if (!isTauri()) return true;
    return invokeTauri<boolean>('proxy_remove_macos_wrapper', { target });
  },

  async proxyGetIntegrations(): Promise<ProxyIntegrations> {
    if (!isTauri()) {
      return { app_wrapper: false, ide_wrapper: false, shell_hook: false, ide_settings_proxy: null };
    }
    return invokeTauri<ProxyIntegrations>('proxy_get_integrations');
  },

  async proxyRuntimeStatus(): Promise<Record<string, { running: boolean; proxy_active: boolean; proxy_url?: string | null }>> {
    if (!isTauri()) return {};
    return invokeTauri<Record<string, { running: boolean; proxy_active: boolean; proxy_url?: string | null }>>('proxy_runtime_status');
  },

  async proxySetAutoNoTun(enabled: boolean, proxyUrl?: string | null): Promise<void> {
    if (!isTauri()) return;
    return invokeTauri<void>('proxy_set_auto_no_tun', { enabled, proxyUrl: proxyUrl ?? null });
  },

  async contextPluginStatus(): Promise<{ ide_installed: boolean; mcp_installed: boolean; cli_installed: boolean }> {
    if (!isTauri()) return { ide_installed: false, mcp_installed: false, cli_installed: false };
    return invokeTauri<{ ide_installed: boolean; mcp_installed: boolean; cli_installed: boolean }>('context_plugin_status');
  },

  async contextPluginInstall(target: 'ide' | 'mcp' | 'cli'): Promise<{ ide_installed: boolean; mcp_installed: boolean; cli_installed: boolean }> {
    if (!isTauri()) return { ide_installed: false, mcp_installed: false, cli_installed: false };
    return invokeTauri<{ ide_installed: boolean; mcp_installed: boolean; cli_installed: boolean }>('context_plugin_install', { target });
  },

  async contextPluginUninstall(target: 'ide' | 'mcp' | 'cli'): Promise<{ ide_installed: boolean; mcp_installed: boolean; cli_installed: boolean }> {
    if (!isTauri()) return { ide_installed: false, mcp_installed: false, cli_installed: false };
    return invokeTauri<{ ide_installed: boolean; mcp_installed: boolean; cli_installed: boolean }>('context_plugin_uninstall', { target });
  },

  async proxySyncIdeSettings(proxyUrl: string | null): Promise<void> {
    if (!isTauri()) return;
    return invokeTauri<void>('proxy_sync_ide_settings', { proxyUrl });
  },

  // 26. 多语言本地化（零 ASAR 侵入）
  async localizeListLocales(): Promise<LocaleOption[]> {
    if (!isTauri()) {
      return [
        { code: 'zh-cn', label: '简体中文', native_label: '简体中文 (中国大陆)' },
        { code: 'zh-tw', label: '繁體中文', native_label: '繁體中文 (台灣)' },
        { code: 'ja', label: '日语', native_label: '日本語' },
        { code: 'ko', label: '韩语', native_label: '한국어' },
        { code: 'en', label: 'English (原版)', native_label: 'English (Default)' },
      ];
    }
    return invokeTauri<LocaleOption[]>('localize_list_locales');
  },

  async localizeGetStatus(): Promise<LocalizeStatus> {
    if (!isTauri()) return { ide_locale: null, app_locale: null, locales_dir: '~/.antigravity_tools/locales' };
    return invokeTauri<LocalizeStatus>('localize_get_status');
  },

  async localizeSetIdeLocale(locale: string): Promise<LocalizeStatus> {
    if (!isTauri()) return { ide_locale: locale === 'en' ? null : locale, app_locale: null, locales_dir: '' };
    return invokeTauri<LocalizeStatus>('localize_set_ide_locale', { locale });
  },

  async localizeSetAppLocale(locale: string): Promise<LocalizeStatus> {
    if (!isTauri()) return { ide_locale: null, app_locale: locale === 'en' ? null : locale, locales_dir: '' };
    return invokeTauri<LocalizeStatus>('localize_set_app_locale', { locale });
  },
};

export interface LocaleOption {
  code: string;
  label: string;
  native_label: string;
}

export interface LocalizeStatus {
  ide_locale: string | null;
  app_locale: string | null;
  locales_dir: string;
}

export interface ProxyIntegrations {
  app_wrapper: boolean;
  ide_wrapper: boolean;
  shell_hook: boolean;
  ide_settings_proxy?: string | null;
}

export interface ProxyNode {
  name: string;
  host: string;
  port: number;
  protocol: 'http' | 'socks5';
  username?: string | null;
  password?: string | null;
  latency_ms?: number | null;
  source: 'system' | 'auto_detected' | 'custom';
}

export interface ProxyStatus {
  running: boolean;
  local_addr: string | null;
  upstream: ProxyNode | null;
}
