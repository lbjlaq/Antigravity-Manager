import { createBrowserRouter, RouterProvider } from 'react-router-dom';

import Layout from './components/layout/Layout';
import Dashboard from './pages/Dashboard';
import Accounts from './pages/Accounts';
import Settings from './pages/Settings';
import ApiProxy from './pages/ApiProxy';
import Monitor from './pages/Monitor';
import TokenStats from './pages/TokenStats';
import Security from './pages/Security';
import ThemeManager from './components/common/ThemeManager';
import { UpdateNotification } from './components/UpdateNotification';
import DebugConsole from './components/debug/DebugConsole';
import { useEffect, useState } from 'react';
import { useConfigStore } from './stores/useConfigStore';
import { useAccountStore } from './stores/useAccountStore';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { isTauri } from './utils/env';
import { request as invoke } from './utils/request';
import { AdminAuthGuard } from './components/common/AdminAuthGuard';
import { showToast } from './components/common/ToastContainer';

const router = createBrowserRouter([
  {
    path: '/',
    element: <Layout />,
    children: [
      {
        index: true,
        element: <Dashboard />,
      },
      {
        path: 'accounts',
        element: <Accounts />,
      },
      {
        path: 'api-proxy',
        element: <ApiProxy />,
      },
      {
        path: 'monitor',
        element: <Monitor />,
      },
      {
        path: 'token-stats',
        element: <TokenStats />,
      },
      {
        path: 'security',
        element: <Security />,
      },
      {
        path: 'settings',
        element: <Settings />,
      },
    ],
  },
]);

function App() {
  const { config, loadConfig } = useConfigStore();
  const { fetchCurrentAccount, fetchAccounts } = useAccountStore();
  const { i18n } = useTranslation();

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  // Sync language from config
  useEffect(() => {
    if (config?.language) {
      i18n.changeLanguage(config.language);
      // Support RTL
      if (config.language === 'ar') {
        document.documentElement.dir = 'rtl';
      } else {
        document.documentElement.dir = 'ltr';
      }
    }
  }, [config?.language, i18n]);

  // Listen for tray events
  useEffect(() => {
    if (!isTauri()) return;
    const unlistenPromises: Promise<() => void>[] = [];

    // 监听托盘切换账号事件
    unlistenPromises.push(
      listen('tray://account-switched', () => {
        console.log('[App] Tray account switched, refreshing...');
        fetchCurrentAccount();
        fetchAccounts();
      })
    );

    // 监听托盘刷新事件
    unlistenPromises.push(
      listen('tray://refresh-current', () => {
        console.log('[App] Tray refresh triggered, refreshing...');
        fetchCurrentAccount();
        fetchAccounts();
      })
    );

    // [NEW] Listen for account validation blocked event
    unlistenPromises.push(
      listen<{ account_id: string; email: string; blocked_until: number; reason: string }>('account-validation-blocked', (event) => {
        console.log('[App] Account validation blocked:', event.payload);
        const { email, blocked_until } = event.payload;
        const blockedUntilDate = new Date(blocked_until * 1000);
        const timeStr = blockedUntilDate.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
        showToast(
          `Account ${email} temporarily blocked until ${timeStr} (verification required)`,
          'warning'
        );
        // Refresh accounts to update UI
        fetchAccounts();
      })
    );

    // Cleanup
    return () => {
      Promise.all(unlistenPromises).then(unlisteners => {
        unlisteners.forEach(unlisten => unlisten());
      });
    };
  }, [fetchCurrentAccount, fetchAccounts]);

  // Update notification state
  const [showUpdateNotification, setShowUpdateNotification] = useState(false);

  // Check for updates on startup
  useEffect(() => {
    const checkUpdates = async () => {
      try {
        console.log('[App] Checking if we should check for updates...');
        const shouldCheck = await invoke<boolean>('should_check_updates');
        console.log('[App] Should check updates:', shouldCheck);

        if (shouldCheck) {
          setShowUpdateNotification(true);
          // 我们这里只负责显示通知组件，通知组件内部会去调用 check_for_updates
          // 我们在显示组件后，标记已经检查过了（即便失败或无更新，组件内部也会处理）
          await invoke('update_last_check_time');
          console.log('[App] Update check cycle initiated and last check time updated.');
        }
      } catch (error) {
        console.error('Failed to check update settings:', error);
      }
    };

    // Delay check to avoid blocking initial render
    const timer = setTimeout(checkUpdates, 2000);
    return () => clearTimeout(timer);
  }, []);

  return (
    <AdminAuthGuard>
      <ThemeManager />
      <DebugConsole />
      {showUpdateNotification && (
        <UpdateNotification onClose={() => setShowUpdateNotification(false)} />
      )}
      <RouterProvider router={router} />
    </AdminAuthGuard>
  );
}

export default App;