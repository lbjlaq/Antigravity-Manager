import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
import { isTauri } from './env';

export const MINI_MODE_WIDTH = 360;
export const MINI_MODE_HEIGHT = 360;

/**
 * Enter mini view mode
 * @param contentHeight The height of the content to fit (optional, defaults to MINI_MODE_HEIGHT)
 * @param shouldCenter Whether to center the window (default: false)
 */
export const enterMiniMode = async (contentHeight?: number, shouldCenter: boolean = false, isPinned: boolean = false) => {
    if (!isTauri()) return;
    try {
        const win = getCurrentWindow();

        // Hide window decorations (title bar) first to ensure accurate sizing
        await win.setDecorations(false);

        // Enforce fixed, stable window size to prevent jumping/stretching during tab or account switches
        const targetHeight = contentHeight && contentHeight > 0 ? contentHeight : MINI_MODE_HEIGHT;
        await win.setSize(new LogicalSize(MINI_MODE_WIDTH, targetHeight));

        await win.setAlwaysOnTop(isPinned);
        // Enable window shadow
        await win.setShadow(true);
        // Disable resizing in mini mode
        await win.setResizable(false);

        // Center window only if requested (usually on first load)
        if (shouldCenter) {
            await win.center();
        }
    } catch (error) {
        console.error('Failed to enter mini mode:', error);
    }
};

/**
 * Exit mini view mode and restore default window state
 */
export const exitMiniMode = async () => {
    if (!isTauri()) return;
    try {
        const win = getCurrentWindow();
        // Restore to a reasonable default size
        await win.setSize(new LogicalSize(1200, 800));
        await win.setAlwaysOnTop(false);
        // Restore window decorations (title bar)
        await win.setDecorations(true);
        // Re-enable resizing
        await win.setResizable(true);
    } catch (error) {
        console.error('Failed to exit mini mode:', error);
    }
};

/**
 * Ensure window is in valid full view state (Self-healing)
 * Fixes #3985: 避免使用 win.center() 强行将副显示器上的主控制台重置拉扯到主屏
 */
export const ensureFullViewState = async () => {
    if (!isTauri()) return;
    try {
        const win = getCurrentWindow();
        // Always enforce standard window properties for Full View
        await win.setDecorations(true);
        await win.setResizable(true);
        await win.setAlwaysOnTop(false);
    } catch (error) {
        console.error('Failed to ensure full view state:', error);
    }
};
