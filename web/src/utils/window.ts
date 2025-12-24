
/**
 * 模拟 Tauri window API
 */
export function getCurrentWindow() {
    return {
        show: async () => {
            console.log('[Mock Window] show() called');
            return Promise.resolve();
        },
        hide: async () => {
            console.log('[Mock Window] hide() called');
            return Promise.resolve();
        },
        close: async () => {
            console.log('[Mock Window] close() called');
            return Promise.resolve();
        },
        minimize: async () => {
            console.log('[Mock Window] minimize() called');
            return Promise.resolve();
        },
        maximize: async () => {
            console.log('[Mock Window] maximize() called');
            return Promise.resolve();
        },
        setBackgroundColor: async (color: string) => {
            console.log(`[Mock Window] setBackgroundColor(${color}) called`);
            return Promise.resolve();
        },
        startDragging: async () => {
            console.log('[Mock Window] startDragging() called');
            return Promise.resolve();
        }
    };
}
