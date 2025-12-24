

const API_BASE = 'http://localhost:8045/api';
const API_KEY = 'sk-antigravity';

async function fetchApi<T>(path: string, options: RequestInit = {}): Promise<T> {
    const headers = {
        'Content-Type': 'application/json',
        'x-api-key': API_KEY,
        ...options.headers,
    };

    const response = await fetch(`${API_BASE}${path}`, {
        ...options,
        headers,
    });

    if (!response.ok) {
        const errorData = await response.json().catch(() => ({}));
        throw new Error(errorData.error || `Request failed: ${response.status}`);
    }

    return response.json();
}

/**
 * 模拟 Tauri invoke 函数
 */
export async function invoke<T>(cmd: string, args: Record<string, any> = {}): Promise<T> {
    console.log(`[Mock Invoke] ${cmd}`, args);

    switch (cmd) {
        // 账号管理
        case 'list_accounts':
            return fetchApi<T>('/accounts');

        case 'add_account':
            return fetchApi<T>('/accounts', {
                method: 'POST',
                body: JSON.stringify({
                    email: args.email,
                    refresh_token: args.refreshToken,
                }),
            });

        case 'delete_account':
            return fetchApi<T>(`/accounts/${args.accountId}`, { method: 'DELETE' });

        case 'switch_account':
            return fetchApi<T>(`/accounts/${args.accountId}/switch`, { method: 'POST' });

        case 'get_current_account':
            return fetchApi<T>('/accounts/current');

        case 'get_account_quota':
        case 'fetch_account_quota': // 兼容两种调用方式
            return fetchApi<T>(`/accounts/${args.accountId}/quota`);

        case 'refresh_all_quotas': // Match the service export name
            const result = await fetchApi<any>('/accounts/reload', { method: 'POST' });
            // 转换后端返回格式为前端期望的格式
            return {
                total: result.accounts_loaded || 0,
                success: result.quota_success || 0,
                failed: result.quota_failed || 0,
                details: result.details || []
            } as T;

        // 配置
        // 配置
        case 'get_config':
        case 'load_config': // 兼容前端调用名
            return fetchApi<T>('/config');

        case 'save_config':
            return fetchApi<T>('/config', {
                method: 'POST',
                body: JSON.stringify(args.config || args),
            });

        case 'update_model_mapping':
            // 后端暂时没有 update_model_mapping 独立接口，通常包含在 save_config 中
            // 这里仅作为占位，避免报错，实际状态更新应通过 save_config 完成
            console.warn('update_model_mapping should be handled via save_config in Web version');
            return Promise.resolve({} as T);

        case 'generate_api_key':
            // 前端生成 UUID
            return Promise.resolve(crypto.randomUUID() as any);

        // 代理
        case 'start_proxy_service': // 映射到后端的 start_proxy
            // args.config 包含 proxy 配置，后端可能需要
            return fetchApi<T>('/proxy/start', {
                method: 'POST',
                body: JSON.stringify(args.config || {})
            });

        case 'stop_proxy_service':
            return fetchApi<T>('/proxy/stop', { method: 'POST' });

        case 'get_proxy_status':
            const status = await fetchApi<any>('/proxy/status');
            // 转换后端返回格式为前端期望的格式
            const port = status.port || 8045;
            return {
                running: status.running || false,
                port: port,
                base_url: status.base_url || `http://localhost:${port}`,
                active_accounts: status.active_accounts || 0 // 使用后端返回的实际值
            } as T;

        case 'get_data_dir_path':
            // 返回一个假的路径用于展示
            return Promise.resolve("Web Version (Browser Storage)" as any);

        case 'open_data_folder':
            alert('Web 版本不支持直接打开本地文件夹。数据存储在浏览器中。');
            return Promise.resolve(undefined as any);

        case 'check_for_updates':
            // 模拟检查更新
            return Promise.resolve({
                has_update: false,
                latest_version: "3.1.1",
                current_version: "3.1.1",
                download_url: ""
            } as any);

        case 'clear_log_cache':
            console.log('[Mock] Clearing logs...');
            return Promise.resolve(undefined as any);

        case 'save_text_file':
            // Web端实现：使用浏览器下载功能
            let filename = 'download.txt';
            if (args.path) {
                // 处理路径，提取文件名
                const pathParts = args.path.split(/[/\\]/);
                filename = pathParts[pathParts.length - 1] || args.path;
                // 如果没有扩展名，根据内容判断
                if (!filename.includes('.')) {
                    try {
                        JSON.parse(args.content || '');
                        filename = filename + '.json';
                    } catch {
                        filename = filename + '.txt';
                    }
                }
            }
            const blob = new Blob([args.content || ''], { type: 'text/plain;charset=utf-8' });
            const url = URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = filename;
            document.body.appendChild(a);
            a.click();
            document.body.removeChild(a);
            URL.revokeObjectURL(url);
            return Promise.resolve(undefined as any);

        case 'start_oauth_login':
            // Web端不支持OAuth登录流程，仅支持通过refresh_token添加账号
            throw new Error('Web版本不支持OAuth登录。请使用Refresh Token添加账号。');

        case 'cancel_oauth_login':
            // Web端不支持OAuth登录流程
            console.warn('cancel_oauth_login called but OAuth is not supported in Web version');
            return Promise.resolve(undefined as any);

        case 'import_v1_accounts':
            // 后端可能没有此端点，返回空数组
            console.warn('import_v1_accounts is not supported in Web version');
            return Promise.resolve([] as any);

        case 'import_from_db':
            // 后端可能没有此端点，返回错误
            throw new Error('Web版本不支持从数据库导入。请使用Refresh Token添加账号。');

        case 'greet':
            return Promise.resolve("Hello from Web Server!" as any);

        default:
            console.warn(`Unknown command: ${cmd}`);
            // 不要抛出错误，而是返回 null 或空对象，避免页面崩坏
            return Promise.resolve({} as any);
    }
}
