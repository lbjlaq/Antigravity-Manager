// UI & Font Scale Manager (Zoom Levels, Native Tauri Support & Persistence)

import { isTauri } from './env';
import { getCurrentWebview } from '@tauri-apps/api/webview';

export const ZOOM_LEVELS = [85, 90, 100, 110, 120, 130, 140, 150];
export const DEFAULT_ZOOM = 100;

const STORAGE_KEY = 'app-ui-zoom';

/**
 * Snaps any zoom value to the closest valid preset in ZOOM_LEVELS
 */
export function snapToClosestZoom(val: number): number {
    if (!Number.isFinite(val)) return DEFAULT_ZOOM;
    let closest = ZOOM_LEVELS[0];
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
        // Ignore localStorage errors
    }
    return DEFAULT_ZOOM;
}

export function applyZoom(level: number, notify = true): void {
    if (typeof window === 'undefined' || typeof document === 'undefined') {
        return;
    }

    const snapped = snapToClosestZoom(level);

    if (isTauri()) {
        try {
            // In Tauri desktop, prioritize native webview zoom (WebView2 / WKWebView / WebKitGTK)
            getCurrentWebview()
                .setZoom(snapped / 100)
                .then(() => {
                    // Clear CSS zoom when native zoom succeeds to prevent double-zoom
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
        // Web / browser mode fallback
        (document.documentElement.style as any).zoom = `${snapped}%`;
    }

    try {
        localStorage.setItem(STORAGE_KEY, String(snapped));
    } catch {
        // Ignore localStorage errors
    }

    if (notify) {
        window.dispatchEvent(new CustomEvent('app-ui-zoom-change', { detail: snapped }));
    }
}

export function zoomIn(): number {
    const current = getStoredZoom();
    const next = ZOOM_LEVELS.find((l) => l > current) ?? Math.min(150, current + 10);
    applyZoom(next);
    return next;
}

export function zoomOut(): number {
    const current = getStoredZoom();
    const prev = [...ZOOM_LEVELS].reverse().find((l) => l < current) ?? Math.max(85, current - 10);
    applyZoom(prev);
    return prev;
}

export function resetZoom(): number {
    applyZoom(DEFAULT_ZOOM);
    return DEFAULT_ZOOM;
}
