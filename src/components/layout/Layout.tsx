import { Outlet } from 'react-router-dom';
import Navbar from './Navbar';
import BackgroundTaskRunner from '../common/BackgroundTaskRunner';
import ToastContainer from '../common/ToastContainer';
import { isTauriEnvironment } from '../../utils/tauriEnv';

function Layout() {
    const isTauri = isTauriEnvironment();

    return (
        <div className="h-screen flex flex-col bg-[#FAFBFC] dark:bg-base-300">
            {!isTauri ? (
                <div className="fixed top-0 left-0 right-0 z-[9999] bg-amber-50 text-amber-900 border-b border-amber-200 px-4 py-2 text-sm">
                    You are viewing the Vite dev server in a browser. This UI is a desktop app and needs the Tauri backend to
                    load real data (accounts, config, proxy controls). Open the desktop window launched by{" "}
                    <code className="px-1 py-0.5 bg-amber-100 rounded">npm run tauri dev</code>.
                    If the desktop window is hidden, use the tray icon menu to show it.
                </div>
            ) : (
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
                        try {
                            const { getCurrentWindow } = await import('@tauri-apps/api/window');
                            await getCurrentWindow().startDragging();
                        } catch {
                            // ignore
                        }
                    }}
                />
            )}
            {isTauri ? <BackgroundTaskRunner /> : null}
            <ToastContainer />
            <Navbar />
            <main className="flex-1 overflow-hidden flex flex-col relative">
                <Outlet />
            </main>
        </div>
    );
}

export default Layout;
