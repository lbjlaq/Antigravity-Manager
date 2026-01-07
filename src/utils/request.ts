/**
 * 环境感知的请求适配器
 * 
 * - Tauri 模式：使用原生 IPC (invoke)
 * - Web 模式：使用 HTTP API
 */

// 运行时环境检测
export const isTauri = typeof window !== 'undefined' && '__TAURI__' in window;

// Web 模式下的 API 基础路径
const API_BASE = import.meta.env.VITE_API_BASE || '';

// 命令名称到 HTTP 端点的映射
// unwrapKey: 可选，指定从 args 中提取哪个键作为请求体（解包 Tauri 调用参数）
type EndpointConfig = { 
  method: string; 
  path: string | ((args: any) => string); 
  unwrapKey?: string;
};

const COMMAND_ENDPOINTS: Record<string, EndpointConfig> = {
  // 账号管理
  list_accounts: { method: 'GET', path: '/api/accounts' },
  add_account: { method: 'POST', path: '/api/accounts' },
  get_current_account: { method: 'GET', path: '/api/accounts/current' },
  delete_account: { method: 'DELETE', path: (args) => `/api/accounts/${args.account_id || args.id}` },
  delete_accounts: { method: 'POST', path: '/api/accounts/batch-delete' },
  switch_account: { method: 'POST', path: (args) => `/api/accounts/${args.account_id || args.id}/switch` },
  fetch_account_quota: { method: 'POST', path: (args) => `/api/accounts/${args.account_id || args.id}/quota` },
  refresh_all_quotas: { method: 'POST', path: '/api/accounts/refresh-all' },
  reorder_accounts: { method: 'POST', path: '/api/accounts/reorder' },
  toggle_proxy_status: { method: 'POST', path: (args) => `/api/accounts/${args.account_id || args.id}/proxy-status` },

  // 配置
  load_config: { method: 'GET', path: '/api/config' },
  save_config: { method: 'PUT', path: '/api/config', unwrapKey: 'config' },

  // 反代服务
  start_proxy_service: { method: 'POST', path: '/api/proxy/start', unwrapKey: 'config' },
  stop_proxy_service: { method: 'POST', path: '/api/proxy/stop' },
  get_proxy_status: { method: 'GET', path: '/api/proxy/status' },
  get_proxy_stats: { method: 'GET', path: '/api/proxy/stats' },
  get_proxy_logs: { method: 'GET', path: '/api/proxy/logs' },
  clear_proxy_logs: { method: 'DELETE', path: '/api/proxy/logs' },
  set_proxy_monitor_enabled: { method: 'POST', path: '/api/proxy/monitor' },
  reload_proxy_accounts: { method: 'POST', path: '/api/proxy/reload-accounts' },
  update_model_mapping: { method: 'PUT', path: '/api/proxy/model-mapping', unwrapKey: 'config' },
  get_proxy_scheduling_config: { method: 'GET', path: '/api/proxy/scheduling' },
  update_proxy_scheduling_config: { method: 'PUT', path: '/api/proxy/scheduling', unwrapKey: 'config' },
  clear_proxy_session_bindings: { method: 'DELETE', path: '/api/proxy/sessions' },
  fetch_zai_models: { method: 'POST', path: '/api/proxy/zai-models' },
  generate_api_key: { method: 'POST', path: '/api/proxy/generate-api-key' },

  // OAuth
  prepare_oauth_url: { method: 'POST', path: '/api/oauth/prepare-url' },
  process_oauth_callback: { method: 'POST', path: '/api/oauth/process-callback' },
  start_oauth_login: { method: 'POST', path: '/api/oauth/prepare-url' }, // Web 模式下同上
  complete_oauth_login: { method: 'POST', path: '/api/oauth/prepare-url' },
  cancel_oauth_login: { method: 'POST', path: '/api/oauth/prepare-url' },


  // 导入
  import_v1_accounts: { method: 'POST', path: '/api/import/v1' },
  import_from_db: { method: 'POST', path: '/api/import/db' },
  import_custom_db: { method: 'POST', path: '/api/import/custom-db' },
  sync_account_from_db: { method: 'POST', path: '/api/sync/db' },

  // 系统
  get_data_dir_path: { method: 'GET', path: '/api/system/data-dir' },
  check_for_updates: { method: 'GET', path: '/api/system/check-updates' },
  clear_log_cache: { method: 'POST', path: '/api/system/clear-logs' },
};

// camelCase 转 snake_case
function toSnakeCase(str: string): string {
  return str.replace(/[A-Z]/g, letter => `_${letter.toLowerCase()}`);
}

// 转换对象键名为 snake_case
function convertKeysToSnakeCase(obj: any): any {
  if (obj === null || obj === undefined) return obj;
  if (Array.isArray(obj)) return obj.map(convertKeysToSnakeCase);
  if (typeof obj !== 'object') return obj;

  const converted: Record<string, any> = {};
  for (const [key, value] of Object.entries(obj)) {
    converted[toSnakeCase(key)] = convertKeysToSnakeCase(value);
  }

  return converted;
}

// Web 模式下的 HTTP 请求实现
async function httpRequest<T>(cmd: string, args?: any): Promise<T> {
  const endpoint = COMMAND_ENDPOINTS[cmd];

  if (!endpoint) {
    console.warn(`[Web API] Unknown command: ${cmd}, trying generic POST`);
    const response = await fetch(`${API_BASE}/api/${cmd.replace(/_/g, '-')}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: args ? JSON.stringify(convertKeysToSnakeCase(args)) : undefined,
    });
    const data = await response.json();
    if (!data.success) throw new Error(data.error || 'Request failed');
    return data.data;
  }

  const path = typeof endpoint.path === 'function' 
    ? endpoint.path(args) 
    : endpoint.path;

  const url = `${API_BASE}${path}`;
  const options: RequestInit = {
    method: endpoint.method,
    headers: { 'Content-Type': 'application/json' },
  };

  // GET/DELETE 请求不发送 body（路径参数已在 path 中）
  if (endpoint.method !== 'GET' && endpoint.method !== 'DELETE' && args) {
    // 如果定义了 unwrapKey，从 args 中提取该键的值作为请求体
    let bodyArgs = args;
    if (endpoint.unwrapKey && args[endpoint.unwrapKey] !== undefined) {
      bodyArgs = args[endpoint.unwrapKey];
    } else {
      // 过滤掉已用于路径的参数
      bodyArgs = { ...args };
      delete bodyArgs.account_id;
      delete bodyArgs.id;
    }
    
    if (bodyArgs && (typeof bodyArgs === 'object' ? Object.keys(bodyArgs).length > 0 : true)) {
      options.body = JSON.stringify(convertKeysToSnakeCase(bodyArgs));
    }
  }

  const response = await fetch(url, options);
  const data = await response.json();

  if (!data.success) {
    throw new Error(data.error || `HTTP ${response.status}`);
  }

  return data.data;
}


/**
 * 统一的请求函数 - 自动根据运行环境选择 IPC 或 HTTP
 */
export async function request<T>(cmd: string, args?: any): Promise<T> {
  if (isTauri) {
    // Tauri 模式：使用原生 IPC
    const { invoke } = await import('@tauri-apps/api/core');
    try {
      return await invoke<T>(cmd, args);
    } catch (error) {
      console.error(`[Tauri API] Error [${cmd}]:`, error);
      throw error;
    }
  } else {
    // Web 模式：使用 HTTP API
    try {
      return await httpRequest<T>(cmd, args);
    } catch (error) {
      console.error(`[Web API] Error [${cmd}]:`, error);
      throw error;
    }
  }
}

export default request;
