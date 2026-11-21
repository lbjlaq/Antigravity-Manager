// 探测环境
const isTauri = typeof window !== 'undefined' && (!!(window as any).__TAURI_INTERNALS__ || !!(window as any).__TAURI__);

// 命令到 API 的映射 (仅保留账号管理与系统基础能力)
const COMMAND_MAPPING: Record<string, { url: string; method: 'GET' | 'POST' | 'DELETE' | 'PATCH' }> = {
  // Accounts
  'list_accounts': { url: '/api/accounts', method: 'GET' },
  'get_current_account': { url: '/api/accounts/current', method: 'GET' },
  'switch_account': { url: '/api/accounts/switch', method: 'POST' },
  'add_account': { url: '/api/accounts', method: 'POST' },
  'batch_import_accounts': { url: '/api/accounts/batch-import', method: 'POST' },
  'delete_account': { url: '/api/accounts/:accountId', method: 'DELETE' },
  'delete_accounts': { url: '/api/accounts/bulk-delete', method: 'POST' },
  'fetch_account_quota': { url: '/api/accounts/:accountId/quota', method: 'GET' },
  'refresh_account_quota': { url: '/api/accounts/:accountId/quota', method: 'GET' },
  'refresh_all_quotas': { url: '/api/accounts/refresh', method: 'POST' },
  'reorder_accounts': { url: '/api/accounts/reorder', method: 'POST' },
  'toggle_proxy_status': { url: '/api/accounts/:accountId/toggle-proxy', method: 'POST' },
  'clear_account_forbidden': { url: '/api/accounts/:accountId/clear-forbidden', method: 'POST' },
  'update_account_label': { url: '/api/accounts/:accountId/label', method: 'POST' },
  'update_account_priority': { url: '/api/accounts/:accountId/priority', method: 'POST' },
  'update_account_project_id': { url: '/api/accounts/:accountId/project-id', method: 'POST' },
  'auto_fetch_account_project_id': { url: '/api/accounts/:accountId/auto-fetch-project-id', method: 'POST' },
  'export_accounts': { url: '/api/accounts/export', method: 'POST' },
  'bind_device_profile': { url: '/api/accounts/:accountId/bind-device', method: 'POST' },
  'get_device_profiles': { url: '/api/accounts/:accountId/device-profiles', method: 'GET' },
  'list_device_versions': { url: '/api/accounts/:accountId/device-versions', method: 'GET' },
  'preview_generate_profile': { url: '/api/accounts/device-preview', method: 'POST' },
  'bind_device_profile_with_profile': { url: '/api/accounts/:accountId/bind-device-profile', method: 'POST' },
  'restore_original_device': { url: '/api/accounts/restore-original', method: 'POST' },
  'restore_device_version': { url: '/api/accounts/:accountId/device-versions/:versionId/restore', method: 'POST' },
  'delete_device_version': { url: '/api/accounts/:accountId/device-versions/:versionId', method: 'DELETE' },
  'open_device_folder': { url: '/api/system/open-folder', method: 'POST' },

  // Config
  'load_config': { url: '/api/config', method: 'GET' },
  'get_config': { url: '/api/config', method: 'GET' },
  'save_config': { url: '/api/config', method: 'POST' },

  // Debug Console
  'enable_debug_console': { url: '/api/debug/enable', method: 'POST' },
  'disable_debug_console': { url: '/api/debug/disable', method: 'POST' },
  'is_debug_console_enabled': { url: '/api/debug/enabled', method: 'GET' },
  'get_debug_console_logs': { url: '/api/debug/logs', method: 'GET' },
  'clear_debug_console_logs': { url: '/api/debug/logs/clear', method: 'POST' },

  // System
  'get_data_dir_path': { url: '/api/system/data-dir', method: 'GET' },
  'get_internal_error_log_path': { url: '/api/system/error-log-path', method: 'GET' },
  'get_internal_error_log_disk_size': { url: '/api/system/error-log-size', method: 'GET' },
  'set_data_dir': { url: '/api/system/data-dir', method: 'POST' },
  'get_update_settings': { url: '/api/system/updates/settings', method: 'GET' },
  'save_update_settings': { url: '/api/system/updates/save', method: 'POST' },
  'is_auto_launch_enabled': { url: '/api/system/autostart/status', method: 'GET' },
  'toggle_auto_launch': { url: '/api/system/autostart/toggle', method: 'POST' },
  'get_antigravity_path': { url: '/api/system/antigravity/path', method: 'GET' },
  'get_antigravity_args': { url: '/api/system/antigravity/args', method: 'GET' },

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
  'list_oauth_clients': { url: '/api/accounts/oauth/clients', method: 'GET' },
  'get_active_oauth_client': { url: '/api/accounts/oauth/client', method: 'GET' },
  'set_active_oauth_client': { url: '/api/accounts/oauth/client', method: 'POST' },

  // Import / Sync
  'import_v1_accounts': { url: '/api/accounts/import/v1', method: 'POST' },
  'import_from_db': { url: '/api/accounts/import/db', method: 'POST' },
  'import_custom_db': { url: '/api/accounts/import/db-custom', method: 'POST' },
  'sync_account_from_db': { url: '/api/accounts/sync/db', method: 'POST' },

  // System Extra & Cache
  'open_data_folder': { url: '/api/system/open-folder', method: 'POST' },
  'clear_antigravity_cache': { url: '/api/system/cache/clear', method: 'POST' },
  'get_antigravity_cache_paths': { url: '/api/system/cache/paths', method: 'GET' },
  'clear_log_cache': { url: '/api/system/logs/clear-cache', method: 'POST' },
};

export async function request<T>(cmd: string, args?: any): Promise<T> {
  // 1. Tauri 环境：直接使用 invoke ...
  if (isTauri) {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      return await invoke<T>(cmd, args);
    } catch (error) {
      console.error(`Tauri Invoke Error [${cmd}]:`, error);
      throw error;
    }
  }

  // 2. Web 环境：映射到 HTTP API
  const mapping = COMMAND_MAPPING[cmd];
  if (!mapping) {
    console.error(`Command [${cmd}] is not yet mapped for Web mode. Failing.`);
    throw new Error(`Command [${cmd}] not supported in Web mode.`);
  }

  let url = mapping.url;

  // [FIX] 创建 args 副本，用于移除已使用的路径参数
  let bodyArgs = args ? { ...args } : undefined;

  // 通用路径参数处理：替换 :key 为 args[key]
  if (args) {
    Object.keys(args).forEach(key => {
      const placeholder = `:${key}`;
      if (url.includes(placeholder)) {
        url = url.replace(placeholder, encodeURIComponent(String(args[key])));
        // [FIX] 从 body 参数中移除已用于路径的参数
        if (bodyArgs) {
          delete bodyArgs[key];
        }
      }
    });
  }

  const apiKey = typeof window !== 'undefined'
    ? (sessionStorage.getItem('abv_admin_api_key') || localStorage.getItem('abv_admin_api_key'))
    : null;

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

  if ((mapping.method === 'GET' || mapping.method === 'DELETE') && args) {
    const params = new URLSearchParams();
    Object.entries(args).forEach(([key, value]) => {
      // [FIX] 跳过已用于路径替换的参数
      if (url.includes(encodeURIComponent(String(value)))) return;
      if (value !== undefined && value !== null) {
        params.append(key, String(value));
      }
    });
    const qs = params.toString();
    if (qs) url += `?${qs}`;
  } else if ((mapping.method === 'POST' || mapping.method === 'PATCH') && bodyArgs) {
    // [FIX] 如果有 request 包装，提取其内容作为 body
    const body = bodyArgs.request !== undefined ? bodyArgs.request : bodyArgs;
    options.body = JSON.stringify(body);
  }

  try {
    const response = await fetch(url, options);
    if (!response.ok) {
      if (!isTauri && response.status === 401) {
        // [FIX #1163] 增加防抖锁，避免重复事件导致 UI 抖动
        const now = Date.now();
        const lastAuthError = (window as any)._lastAuthErrorTime || 0;
        if (now - lastAuthError > 2000) {
          (window as any)._lastAuthErrorTime = now;
          window.dispatchEvent(new CustomEvent('abv-unauthorized'));
        }
      }
      const errorData = await response.json().catch(() => ({}));
      throw errorData.error || `HTTP Error ${response.status}`;
    }

    // 如果是 204 No Content，直接返回 null
    if (response.status === 204) {
      return null as unknown as T;
    }

    const text = await response.text();
    if (!text) {
      return null as unknown as T;
    }

    try {
      return JSON.parse(text) as T;
    } catch (e) {
      console.warn(`Failed to parse JSON response for [${cmd}]:`, text);
      return text as unknown as T; // Fallback for plain text responses
    }
  } catch (error) {
    console.error(`Web Fetch Error [${cmd}]:`, error);
    throw error;
  }
}
