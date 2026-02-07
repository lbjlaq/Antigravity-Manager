// File: src/shared/api/invoke.ts
// Typed Tauri invoke wrapper with Web API fallback

import { invoke as tauriInvoke } from '@tauri-apps/api/core';

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const win = window as any;
const isTauriEnv = typeof window !== 'undefined' && 
  (!!win.__TAURI_INTERNALS__ || !!win.__TAURI__);

// Command to API endpoint mapping for Web mode
const COMMAND_MAPPING: Record<string, { url: string; method: 'GET' | 'POST' | 'DELETE' }> = {
  // Accounts
  'list_accounts': { url: '/api/accounts', method: 'GET' },
  'get_current_account': { url: '/api/accounts/current', method: 'GET' },
  'switch_account': { url: '/api/accounts/switch', method: 'POST' },
  'add_account': { url: '/api/accounts', method: 'POST' },
  'delete_account': { url: '/api/accounts/:accountId', method: 'DELETE' },
  'delete_accounts': { url: '/api/accounts/bulk-delete', method: 'POST' },
  'fetch_account_quota': { url: '/api/accounts/:accountId/quota', method: 'GET' },
  'refresh_account_quota': { url: '/api/accounts/:accountId/quota', method: 'GET' },
  'refresh_all_quotas': { url: '/api/accounts/refresh', method: 'POST' },
  'reorder_accounts': { url: '/api/accounts/reorder', method: 'POST' },
  'toggle_proxy_status': { url: '/api/accounts/:accountId/toggle-proxy', method: 'POST' },
  'warm_up_accounts': { url: '/api/accounts/warmup', method: 'POST' },
  'warm_up_all_accounts': { url: '/api/accounts/warmup', method: 'POST' },
  'warm_up_account': { url: '/api/accounts/:accountId/warmup', method: 'POST' },
  'bind_device_profile': { url: '/api/accounts/:accountId/bind-device', method: 'POST' },
  'get_device_profiles': { url: '/api/accounts/:accountId/device-profiles', method: 'GET' },
  'list_device_versions': { url: '/api/accounts/:accountId/device-versions', method: 'GET' },
  'preview_generate_profile': { url: '/api/accounts/device-preview', method: 'POST' },
  'bind_device_profile_with_profile': { url: '/api/accounts/:accountId/bind-device-profile', method: 'POST' },
  'restore_original_device': { url: '/api/accounts/restore-original', method: 'POST' },
  'restore_device_version': { url: '/api/accounts/:accountId/device-versions/:versionId/restore', method: 'POST' },
  'delete_device_version': { url: '/api/accounts/:accountId/device-versions/:versionId', method: 'DELETE' },
  'open_device_folder': { url: '/api/system/open-folder', method: 'POST' },

  // Proxy Control & Status
  'get_proxy_status': { url: '/api/proxy/status', method: 'GET' },
  'start_proxy_service': { url: '/api/proxy/start', method: 'POST' },
  'stop_proxy_service': { url: '/api/proxy/stop', method: 'POST' },
  'update_model_mapping': { url: '/api/proxy/mapping', method: 'POST' },
  'generate_api_key': { url: '/api/proxy/api-key/generate', method: 'POST' },
  'clear_proxy_session_bindings': { url: '/api/proxy/session-bindings/clear', method: 'POST' },
  'clear_proxy_rate_limit': { url: '/api/proxy/rate-limits/:accountId', method: 'DELETE' },
  'clear_all_proxy_rate_limits': { url: '/api/proxy/rate-limits', method: 'DELETE' },

  'fetch_zai_models': { url: '/api/zai/models/fetch', method: 'POST' },
  'load_config': { url: '/api/config', method: 'GET' },
  'save_config': { url: '/api/config', method: 'POST' },
  'get_proxy_stats': { url: '/api/proxy/stats', method: 'GET' },
  'set_proxy_monitor_enabled': { url: '/api/proxy/monitor/toggle', method: 'POST' },

  // Logs & Monitoring
  'get_proxy_logs_filtered': { url: '/api/logs', method: 'GET' },
  'get_proxy_logs_count_filtered': { url: '/api/logs/count', method: 'GET' },
  'clear_proxy_logs': { url: '/api/logs/clear', method: 'POST' },
  'get_proxy_log_detail': { url: '/api/logs/:log_id', method: 'GET' },

  // CLI Sync
  'get_cli_sync_status': { url: '/api/proxy/cli/status', method: 'POST' },
  'execute_cli_sync': { url: '/api/proxy/cli/sync', method: 'POST' },
  'execute_cli_restore': { url: '/api/proxy/cli/restore', method: 'POST' },
  'get_cli_config_content': { url: '/api/proxy/cli/config', method: 'POST' },

  // OpenCode Sync
  'get_opencode_sync_status': { url: '/api/proxy/opencode/status', method: 'POST' },
  'execute_opencode_sync': { url: '/api/proxy/opencode/sync', method: 'POST' },
  'execute_opencode_restore': { url: '/api/proxy/opencode/restore', method: 'POST' },
  'get_opencode_config_content': { url: '/api/proxy/opencode/config', method: 'POST' },

  // Stats
  'get_token_stats_hourly': { url: '/api/stats/token/hourly', method: 'GET' },
  'get_token_stats_daily': { url: '/api/stats/token/daily', method: 'GET' },
  'get_token_stats_weekly': { url: '/api/stats/token/weekly', method: 'GET' },
  'get_token_stats_by_account': { url: '/api/stats/token/by-account', method: 'GET' },
  'get_token_stats_summary': { url: '/api/stats/token/summary', method: 'GET' },
  'get_token_stats_by_model': { url: '/api/stats/token/by-model', method: 'GET' },
  'get_token_stats_model_trend_hourly': { url: '/api/stats/token/model-trend/hourly', method: 'GET' },
  'get_token_stats_model_trend_daily': { url: '/api/stats/token/model-trend/daily', method: 'GET' },
  'get_token_stats_account_trend_hourly': { url: '/api/stats/token/account-trend/hourly', method: 'GET' },
  'get_token_stats_account_trend_daily': { url: '/api/stats/token/account-trend/daily', method: 'GET' },

  // System
  'get_data_dir_path': { url: '/api/system/data-dir', method: 'GET' },
  'save_text_file': { url: '/api/system/save-file', method: 'POST' },
  'get_update_settings': { url: '/api/system/updates/settings', method: 'GET' },
  'save_update_settings': { url: '/api/system/updates/save', method: 'POST' },
  'is_auto_launch_enabled': { url: '/api/system/autostart/status', method: 'GET' },
  'toggle_auto_launch': { url: '/api/system/autostart/toggle', method: 'POST' },
  'get_http_api_settings': { url: '/api/system/http-api/settings', method: 'GET' },
  'save_http_api_settings': { url: '/api/system/http-api/settings', method: 'POST' },

  // Cloudflared
  'cloudflared_install': { url: '/api/proxy/cloudflared/install', method: 'POST' },
  'cloudflared_start': { url: '/api/proxy/cloudflared/start', method: 'POST' },
  'cloudflared_stop': { url: '/api/proxy/cloudflared/stop', method: 'POST' },
  'cloudflared_get_status': { url: '/api/proxy/cloudflared/status', method: 'GET' },

  // Updates
  'should_check_updates': { url: '/api/system/updates/check-status', method: 'GET' },
  'check_for_updates': { url: '/api/system/updates/check', method: 'POST' },
  'update_last_check_time': { url: '/api/system/updates/touch', method: 'POST' },

  // OAuth
  'prepare_oauth_url': { url: '/api/auth/url', method: 'GET' },
  'start_oauth_login': { url: '/api/accounts/oauth/start', method: 'POST' },
  'complete_oauth_login': { url: '/api/accounts/oauth/complete', method: 'POST' },
  'cancel_oauth_login': { url: '/api/accounts/oauth/cancel', method: 'POST' },
  'submit_oauth_code': { url: '/api/accounts/oauth/submit-code', method: 'POST' },

  // Import
  'import_v1_accounts': { url: '/api/accounts/import/v1', method: 'POST' },
  'import_from_db': { url: '/api/accounts/import/db', method: 'POST' },
  'import_custom_db': { url: '/api/accounts/import/db-custom', method: 'POST' },
  'sync_account_from_db': { url: '/api/accounts/sync/db', method: 'POST' },

  // System Extra
  'open_data_folder': { url: '/api/system/open-folder', method: 'POST' },

  // Security
  'security_init_db': { url: '/api/security/init', method: 'POST' },
  'security_get_blacklist': { url: '/api/security/blacklist', method: 'GET' },
  'security_get_whitelist': { url: '/api/security/whitelist', method: 'GET' },
  'security_add_to_blacklist': { url: '/api/security/blacklist', method: 'POST' },
  'security_add_to_whitelist': { url: '/api/security/whitelist', method: 'POST' },
  'security_remove_from_blacklist': { url: '/api/security/blacklist/by-pattern/:ipPattern', method: 'DELETE' },
  'security_remove_from_blacklist_by_id': { url: '/api/security/blacklist/:id', method: 'DELETE' },
  'security_remove_from_whitelist': { url: '/api/security/whitelist/by-pattern/:ipPattern', method: 'DELETE' },
  'security_remove_from_whitelist_by_id': { url: '/api/security/whitelist/:id', method: 'DELETE' },
  'security_is_ip_blacklisted': { url: '/api/security/blacklist/check', method: 'POST' },
  'security_is_ip_whitelisted': { url: '/api/security/whitelist/check', method: 'POST' },
  'security_get_access_logs': { url: '/api/security/logs', method: 'GET' },
  'security_cleanup_logs': { url: '/api/security/logs/cleanup', method: 'POST' },
  'security_clear_all_logs': { url: '/api/security/logs', method: 'DELETE' },
  'security_get_stats': { url: '/api/security/stats', method: 'GET' },
  'security_clear_blacklist': { url: '/api/security/blacklist/clear', method: 'DELETE' },
  'security_clear_whitelist': { url: '/api/security/whitelist/clear', method: 'DELETE' },
  'security_get_ip_token_stats': { url: '/api/security/token-stats', method: 'GET' },
  'get_security_config': { url: '/api/security/config', method: 'GET' },
  'update_security_config': { url: '/api/security/config', method: 'POST' },

  // Proxy Preferred Account
  'get_preferred_account': { url: '/api/proxy/preferred-account', method: 'GET' },
  'set_preferred_account': { url: '/api/proxy/preferred-account', method: 'POST' },

  // Account Export
  'export_accounts': { url: '/api/accounts/export', method: 'POST' },
  'export_accounts_by_ids': { url: '/api/accounts/export', method: 'POST' },
};

/**
 * Universal invoke function - uses Tauri IPC in desktop, HTTP API in web
 */
export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  // Tauri environment: use native invoke
  if (isTauriEnv) {
    try {
      return await tauriInvoke<T>(cmd, args);
    } catch (error) {
      console.error(`[Tauri Invoke Error] ${cmd}:`, error);
      throw error;
    }
  }

  // Web environment: map to HTTP API
  const mapping = COMMAND_MAPPING[cmd];
  if (!mapping) {
    console.error(`[API Error] Command "${cmd}" is not mapped for Web mode`);
    throw new Error(`Command "${cmd}" not supported in Web mode`);
  }

  let url = mapping.url;
  
  // Replace path parameters :key with args[key]
  if (args) {
    Object.keys(args).forEach(key => {
      const placeholder = `:${key}`;
      if (url.includes(placeholder)) {
        url = url.replace(placeholder, encodeURIComponent(String(args[key])));
      }
    });
  }

  const apiKey = typeof window !== 'undefined' ? sessionStorage.getItem('abv_admin_api_key') : null;

  const options: RequestInit = {
    method: mapping.method,
    headers: {
      'Content-Type': 'application/json',
      ...(apiKey ? {
        'Authorization': `Bearer ${apiKey}`,
        'x-api-key': apiKey
      } : {}),
    },
  };

  if (mapping.method === 'GET' && args) {
    const params = new URLSearchParams();
    Object.entries(args).forEach(([key, value]) => {
      if (value !== undefined && value !== null) {
        params.append(key, String(value));
      }
    });
    const qs = params.toString();
    if (qs) url += `?${qs}`;
  } else if (mapping.method === 'POST' && args) {
    options.body = JSON.stringify(args);
  }

  try {
    const response = await fetch(url, options);
    
    if (!response.ok) {
      if (!isTauriEnv && response.status === 401) {
        // Debounce auth errors to prevent UI flickering
        const now = Date.now();
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const lastAuthError = (window as any)._lastAuthErrorTime || 0;
        if (now - lastAuthError > 2000) {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          (window as any)._lastAuthErrorTime = now;
          window.dispatchEvent(new CustomEvent('abv-unauthorized'));
        }
      }
      const errorData = await response.json().catch(() => ({}));
      throw (errorData as Record<string, string>).error || `HTTP Error ${response.status}`;
    }

    // Handle 204 No Content
    if (response.status === 204) {
      return null as unknown as T;
    }

    const text = await response.text();
    if (!text) {
      return null as unknown as T;
    }

    try {
      return JSON.parse(text) as T;
    } catch {
      console.warn(`[API] Failed to parse JSON for "${cmd}":`, text);
      return text as unknown as T;
    }
  } catch (error) {
    console.error(`[Web Fetch Error] ${cmd}:`, error);
    throw error;
  }
}

// Re-export for backwards compatibility
export { invoke as request };
