/**
 * Tauri API 兼容层
 * 
 * 提供 Web 模式下 Tauri 特有 API 的替代实现
 */

import { isTauri } from './request';

// ============================================================================
// 事件系统兼容 - 使用 SSE 替代 Tauri emit/listen
// ============================================================================

// SSE 事件源管理
let eventSource: EventSource | null = null;
const eventListeners: Map<string, Set<(payload: any) => void>> = new Map();

/**
 * 初始化 SSE 事件监听（Web 模式）
 */
function initSSE() {
  if (eventSource || isTauri) return;

  const API_BASE = import.meta.env.VITE_API_BASE || '';
  eventSource = new EventSource(`${API_BASE}/api/events`);

  eventSource.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data);
      const eventType = data.type;
      const payload = data.data;

      // 分发事件给监听器
      const handlers = eventListeners.get(eventType) || eventListeners.get('*');
      handlers?.forEach(handler => handler(payload));

      // 映射 SSE 事件类型到 Tauri 事件名称
      if (eventType === 'ProxyRequest') {
        eventListeners.get('proxy://request')?.forEach(h => h(payload));
      } else if (eventType === 'ConfigUpdated') {
        eventListeners.get('config://updated')?.forEach(h => h(null));
      } else if (eventType === 'AccountSwitched') {
        eventListeners.get('tray://account-switched')?.forEach(h => h(null));
      }
    } catch (e) {
      console.error('[SSE] Parse error:', e);
    }
  };

  eventSource.onerror = (error) => {
    console.error('[SSE] Connection error:', error);
    // 尝试重连
    setTimeout(() => {
      eventSource?.close();
      eventSource = null;
      initSSE();
    }, 5000);
  };
}

/**
 * 事件监听 - 兼容 @tauri-apps/api/event
 */
export async function listen<T>(
  event: string,
  handler: (event: { payload: T }) => void
): Promise<() => void> {
  if (isTauri) {
    // Tauri 模式：使用原生 listen
    const { listen: tauriListen } = await import('@tauri-apps/api/event');
    return tauriListen(event, handler);
  } else {
    // Web 模式：使用 SSE
    initSSE();

    const wrappedHandler = (payload: T) => handler({ payload });

    if (!eventListeners.has(event)) {
      eventListeners.set(event, new Set());
    }
    eventListeners.get(event)!.add(wrappedHandler);

    // 返回 unlisten 函数
    return () => {
      eventListeners.get(event)?.delete(wrappedHandler);
    };
  }
}

// ============================================================================
// 窗口 API 兼容 - Web 模式下提供 no-op 实现
// ============================================================================

/**
 * 获取当前窗口 - 兼容 @tauri-apps/api/window
 */
export async function getCurrentWindow(): Promise<{
  show: () => Promise<void>;
  hide: () => Promise<void>;
  setFocus: () => Promise<void>;
  setBackgroundColor: (color: string) => Promise<void>;
}> {
  if (isTauri) {
    const { getCurrentWindow: tauriGetCurrentWindow } = await import('@tauri-apps/api/window');
    return tauriGetCurrentWindow();
  } else {
    // Web 模式：返回可安全调用但无操作的对象
    return {
      show: async () => {},
      hide: async () => {},
      setFocus: async () => {},
      setBackgroundColor: async (color: string) => {
        // 使用 CSS 设置背景色
        document.documentElement.style.backgroundColor = color;
      },
    };
  }
}

// ============================================================================
// 对话框 API 兼容 - 使用浏览器原生方法
// ============================================================================

export interface SaveDialogOptions {
  title?: string;
  filters?: { name: string; extensions: string[] }[];
  defaultPath?: string;
}

export interface OpenDialogOptions {
  title?: string;
  filters?: { name: string; extensions: string[] }[];
  multiple?: boolean;
  directory?: boolean;
}

/**
 * 保存文件对话框 - 兼容 @tauri-apps/plugin-dialog
 */
export async function save(options?: SaveDialogOptions): Promise<string | null> {
  if (isTauri) {
    const { save: tauriSave } = await import('@tauri-apps/plugin-dialog');
    return tauriSave(options);
  } else {
    // Web 模式：返回 null，让调用方自行处理
    console.warn('[Web] File save dialog not available in web mode');
    return null;
  }
}

/**
 * 打开文件对话框 - 兼容 @tauri-apps/plugin-dialog
 */
export async function open(options?: OpenDialogOptions): Promise<string | string[] | null> {
  if (isTauri) {
    const { open: tauriOpen } = await import('@tauri-apps/plugin-dialog');
    return tauriOpen(options);
  } else {
    // Web 模式：使用 input[type=file] + FileReader
    return new Promise((resolve) => {
      const input = document.createElement('input');
      input.type = 'file';
      if (options?.multiple) input.multiple = true;
      if (options?.directory) input.webkitdirectory = true;
      if (options?.filters) {
        input.accept = options.filters
          .flatMap(f => f.extensions.map(ext => `.${ext}`))
          .join(',');
      }

      input.onchange = (e) => {
        const files = (e.target as HTMLInputElement).files;
        if (!files || files.length === 0) {
          resolve(null);
          return;
        }

        if (options?.multiple) {
          resolve(Array.from(files).map(f => f.name));
        } else {
          resolve(files[0].name);
        }
      };

      input.oncancel = () => resolve(null);
      input.click();
    });
  }
}

// ============================================================================
// 路径 API 兼容
// ============================================================================

/**
 * 路径拼接 - 兼容 @tauri-apps/api/path
 */
export async function join(...paths: string[]): Promise<string> {
  if (isTauri) {
    const { join: tauriJoin } = await import('@tauri-apps/api/path');
    return tauriJoin(...paths);
  } else {
    // Web 模式：简单的路径拼接
    return paths
      .map(p => p.replace(/^\/|\/$/g, ''))
      .filter(Boolean)
      .join('/');
  }
}

// ============================================================================
// 自启动 API 兼容
// ============================================================================

/**
 * 检查自启动状态 - Web 模式下始终返回 false
 */
export async function isAutoLaunchEnabled(): Promise<boolean> {
  if (isTauri) {
    const { request } = await import('./request');
    return request<boolean>('is_auto_launch_enabled');
  } else {
    return false;
  }
}

/**
 * 切换自启动 - Web 模式下 no-op
 */
export async function toggleAutoLaunch(enable: boolean): Promise<boolean> {
  if (isTauri) {
    const { request } = await import('./request');
    return request<boolean>('toggle_auto_launch', { enable });
  } else {
    console.warn('[Web] Auto launch not available in web mode');
    return false;
  }
}

// ============================================================================
// Opener API 兼容
// ============================================================================

/**
 * 打开 URL - 兼容 @tauri-apps/plugin-opener
 */
export async function openUrl(url: string): Promise<void> {
  if (isTauri) {
    const { openUrl: tauriOpenUrl } = await import('@tauri-apps/plugin-opener');
    await tauriOpenUrl(url);
  } else {
    // Web 模式：使用 window.open
    window.open(url, '_blank');
  }
}


/**
 * 打开数据文件夹 - Web 模式下不支持
 */
export async function openDataFolder(): Promise<void> {
  if (isTauri) {
    const { request } = await import('./request');
    await request('open_data_folder');
  } else {
    console.warn('[Web] Cannot open local folder in web mode');
  }
}

// ============================================================================
// 文件系统 API 兼容
// ============================================================================

/**
 * 保存文本文件
 */
export async function saveTextFile(content: string, filename: string): Promise<void> {
  if (isTauri) {
    const { request } = await import('./request');
    await request('save_text_file', { content, filename });
  } else {
    // Web 模式：使用 Blob + download
    const blob = new Blob([content], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
  }
}
