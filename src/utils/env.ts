/**
 * Detect if the app is running in a Tauri environment
 */
export const isTauri = () => false;

/**
 * Detect if running on Linux
 */
export const isLinux = () => {
    return typeof navigator !== 'undefined' && navigator.userAgent.toLowerCase().includes('linux');
};

