import { Outlet } from 'react-router-dom';
import { getCurrentWindow } from '@tauri-apps/api/window';
import Navbar from '../navbar/Navbar';
import BackgroundTaskRunner from '../common/BackgroundTaskRunner';
import ToastContainer from '../common/ToastContainer';
import { useViewStore } from '../../stores/useViewStore';
import MiniView from './MiniView';
import { useEffect } from 'react';
import { isTauri } from '../../utils/env';
import { ensureFullViewState } from '../../utils/windowManager';

function Layout() {
    const { isMiniView } = useViewStore();

    // Ensure correct window state when in Full View (not Mini View)
    // This handles the case where the app was closed in Mini View (small size, no decorations)
    // and restarted (defaults to Full View state but keeps last window properties)
    useEffect(() => {
        if (!isMiniView && isTauri()) {
            ensureFullViewState();
        }
    }, [isMiniView]);

    if (isMiniView) {
        return (
            <>
                <BackgroundTaskRunner />
                <ToastContainer />
                <MiniView />
            </>
        );
    }

    return (
        <div className="h-screen flex flex-col bg-[#FAFBFC] dark:bg-base-300">
            {/* 全局窗口拖拽区域 - 使用 JS 手动触发拖拽，解决 HTML 属性失效问题 */}
            <div
                className="fixed top-0 left-0 right-0 h-9"
                style={{
                    zIndex: 9999,
                    backgroundColor: 'rgba(0,0,0,0.001)',
                    cursor: 'default',
                    userSelect: 'none',
                    WebkitUserSelect: 'none'
                }}
                data-tauri-drag-region
                onMouseDown={() => {
                    getCurrentWindow().startDragging();
                }}
            />
            <BackgroundTaskRunner />
            <ToastContainer />
            <Navbar />
            <main className="flex-1 overflow-hidden flex flex-col relative">
                <Outlet />
            </main>
        </div>
    );
}

export default Layout;
