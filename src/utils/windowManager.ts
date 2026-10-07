import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
import { isTauri } from './env';

/**
 * Enter mini view mode
 * @param contentHeight The height of the content to fit
 * @param shouldCenter Whether to center the window (default: false)
 */
export const enterMiniMode = async (contentHeight: number, shouldCenter: boolean = false) => {
    if (!isTauri()) return;
    try {
        const win = getCurrentWindow();

        // Hide window decorations (title bar) first to ensure accurate sizing
        await win.setDecorations(false);

        // Set window size: width 328, height = content height 
        await win.setSize(new LogicalSize(328, contentHeight+2));

        await win.setAlwaysOnTop(true);
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
        await win.center();
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
 * Used on app startup to recover from improper shutdown in mini mode
 */
export const ensureFullViewState = async () => {
    if (!isTauri()) return;
    try {
        const win = getCurrentWindow();
        const size = await win.outerSize();
        // If window is suspiciously narrow (likely leftover from Mini View), restore default size
        if (size.width < 500) {
            await win.setSize(new LogicalSize(1200, 800));
            await win.center();
        }
        // Always enforce standard window properties for Full View
        await win.setDecorations(true);
        await win.setResizable(true);
        await win.setAlwaysOnTop(false);
    } catch (error) {
        console.error('Failed to ensure full view state:', error);
    }
};
