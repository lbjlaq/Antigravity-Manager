import { Outlet } from 'react-router-dom';
import { getCurrentWindow } from '../../utils/tauriCompat';
import { isTauri } from '../../utils/request';

import Navbar from './Navbar';
import BackgroundTaskRunner from '../common/BackgroundTaskRunner';
import ToastContainer from '../common/ToastContainer';

function Layout() {
    return (
        <div className="h-screen flex flex-col bg-[#FAFBFC] dark:bg-base-300">
            {/* 全局窗口拖拽区域 - 仅在 Tauri 模式下启用 */}
            {isTauri && (
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
                    onMouseDown={async () => {
                        const win = await getCurrentWindow();
                        (win as any).startDragging?.();
                    }}
                />
            )}
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

