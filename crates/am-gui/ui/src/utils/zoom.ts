import { isTauri } from '../api';
import { getCurrentWebview } from '@tauri-apps/api/webview';

export const ZOOM_LEVELS = [85, 90, 100, 110, 120, 130, 140, 150] as const;
export const DEFAULT_ZOOM = 120;

const STORAGE_KEY = 'app-ui-zoom';

/**
 * 将任意缩放值吸附到最近的预设档位，确保与 Antigravity Tools 保持一致的步进逻辑
 */
export function snapToClosestZoom(val: number): number {
  if (!Number.isFinite(val)) return DEFAULT_ZOOM;
  let closest: number = ZOOM_LEVELS[0];
  let minDiff = Math.abs(val - closest);
  for (let i = 1; i < ZOOM_LEVELS.length; i++) {
    const diff = Math.abs(val - ZOOM_LEVELS[i]);
    if (diff < minDiff) {
      minDiff = diff;
      closest = ZOOM_LEVELS[i];
    }
  }
  return closest;
}

/**
 * 托盘迷你悬浮窗 (`index.html?mini=1`) 使用固定像素尺寸，绝不继承主窗口缩放，避免裁切
 */
export function isMiniWindow(): boolean {
  if (typeof window === 'undefined') return false;
  if (window.location.search.includes('mini')) return true;
  return (window as any).__TAURI_INTERNALS__?.metadata?.currentWindow?.label === 'mini';
}

export function getStoredZoom(): number {
  if (typeof window === 'undefined') return DEFAULT_ZOOM;
  try {
    const val = localStorage.getItem(STORAGE_KEY);
    if (val) {
      const num = parseInt(val, 10);
      if (Number.isFinite(num)) {
        return snapToClosestZoom(num);
      }
    }
  } catch {
    // 忽略 localStorage 读取异常
  }
  return DEFAULT_ZOOM;
}

export function applyZoom(level: number, notify = true): void {
  if (typeof window === 'undefined' || typeof document === 'undefined') {
    return;
  }

  if (isMiniWindow()) {
    return;
  }

  const snapped = snapToClosestZoom(level);

  if (isTauri()) {
    try {
      getCurrentWebview()
        .setZoom(snapped / 100)
        .then(() => {
          document.documentElement.style.zoom = '';
        })
        .catch((err) => {
          console.warn('[Zoom] Native setZoom failed, fallback to CSS zoom:', err);
          (document.documentElement.style as any).zoom = `${snapped}%`;
        });
    } catch {
      (document.documentElement.style as any).zoom = `${snapped}%`;
    }
  } else {
    (document.documentElement.style as any).zoom = `${snapped}%`;
  }

  try {
    localStorage.setItem(STORAGE_KEY, String(snapped));
  } catch {
    // 忽略 localStorage 写入异常
  }

  if (notify) {
    window.dispatchEvent(new CustomEvent('app-ui-zoom-change', { detail: snapped }));
  }
}

export function zoomIn(): number {
  const current = getStoredZoom();
  const next = ZOOM_LEVELS.find((l) => l > current) ?? current;
  applyZoom(next);
  return next;
}

export function zoomOut(): number {
  const current = getStoredZoom();
  const prev = [...ZOOM_LEVELS].reverse().find((l) => l < current) ?? current;
  applyZoom(prev);
  return prev;
}

export function resetZoom(): number {
  applyZoom(DEFAULT_ZOOM);
  return DEFAULT_ZOOM;
}
