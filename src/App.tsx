import { createBrowserRouter, RouterProvider, Navigate } from 'react-router-dom';

import Layout from './components/layout/Layout';
import Dashboard from './pages/Dashboard';
import Accounts from './pages/Accounts';
import Settings from './pages/Settings';
import ApiProxy from './pages/ApiProxy';
import Monitor from './pages/Monitor';
import TokenStats from './pages/TokenStats';
import Security from './pages/Security';
import ThemeManager from './components/common/ThemeManager';
import ZoomManager from './components/common/ZoomManager';
import UserToken from './pages/UserToken';
import { ApiKeyFun } from './pages/ApiKeyFun';
import { UpdateNotification } from './components/UpdateNotification';
import SuggestionDeleteThinkingModal from './components/common/SuggestionDeleteThinkingModal';
import DebugConsole from './components/debug/DebugConsole';
import { useEffect, useState } from 'react';
import { useConfigStore, normalizeLanguageCode } from './stores/useConfigStore';
import { useAccountStore } from './stores/useAccountStore';
import { useViewStore } from './stores/useViewStore';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { isTauri } from './utils/env';
import { request as invoke } from './utils/request';
import { AdminAuthGuard } from './components/common/AdminAuthGuard';
import MiniView from './components/layout/MiniView';
import { getCurrentWindow } from '@tauri-apps/api/window';

const router = createBrowserRouter([
  {
    path: '/',
    element: <Layout />,
    errorElement: <Navigate to="/" replace />,
    children: [
      {
        index: true,
        element: <Dashboard />,
      },
      {
        path: 'index.html',
        element: <Navigate to="/" replace />,
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
        path: 'user-token',
        element: <UserToken />,
      },
      {
        path: 'apikey-fun',
        element: <ApiKeyFun />,
      },
      {
        path: 'security',
        element: <Security />,
      },
      {
        path: 'settings',
        element: <Settings />,
      },
      {
        path: '*',
        element: <Navigate to="/" replace />,
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

  // Sync language from config (仅在不同步时调度，避免无效重入)
  useEffect(() => {
    if (!config?.language) return;
    const targetLang = normalizeLanguageCode(config.language);
    const currentLang = normalizeLanguageCode(i18n.language);
    if (targetLang !== currentLang) {
      i18n.changeLanguage(targetLang);
      document.documentElement.dir = targetLang === 'ar' ? 'rtl' : 'ltr';
    }
  }, [config?.language]);

  // 全局滚动条微交互：仅在滚动时显示，停止滚动 1 秒后自动平滑淡出隐藏
  useEffect(() => {
    const scrollTimers = new WeakMap<HTMLElement, ReturnType<typeof setTimeout>>();

    const handleScroll = (e: Event) => {
      const target = e.target;
      if (!target || !(target instanceof HTMLElement)) return;

      target.setAttribute('data-scrolling', 'true');
      const existingTimer = scrollTimers.get(target);
      if (existingTimer) {
        clearTimeout(existingTimer);
      }

      const timer = setTimeout(() => {
        target.removeAttribute('data-scrolling');
        scrollTimers.delete(target);
      }, 1000);

      scrollTimers.set(target, timer);
    };

    window.addEventListener('scroll', handleScroll, { capture: true, passive: true });
    return () => {
      window.removeEventListener('scroll', handleScroll, { capture: true });
    };
  }, []);

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

    // 监听托盘显示主窗口事件
    unlistenPromises.push(
      listen('tray://show-window', async () => {
        if (!isTauri() && useViewStore.getState().isMiniView) {
          useViewStore.getState().setMiniView(false);
        }
      })
    );

    // 监听后端全量刷新事件 (Command / Scheduler)
    unlistenPromises.push(
      listen('accounts://refreshed', () => {
        console.log('[App] Backend triggered quota refresh, syncing UI...');
        fetchCurrentAccount();
        fetchAccounts();
      })
    );

    // 监听手动触发自动更新事件
    unlistenPromises.push(
      listen('app://trigger-update', () => {
        console.log('[App] Received app://trigger-update event, showing updater...');
        setShowUpdateNotification(true);
      })
    );

    // 监听后端全局配置更新事件
    unlistenPromises.push(
      listen('config://updated', async () => {
        console.log('[App] Received config://updated, syncing latest config...');
        await useConfigStore.getState().loadConfig();
        const cfg = useConfigStore.getState().config;
        if (cfg) {
          if (cfg.theme) {
            const isDark = cfg.theme === 'dark' || (cfg.theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
            document.documentElement.classList.toggle('dark', isDark);
            document.documentElement.setAttribute('data-theme', isDark ? 'dark' : 'light');
            localStorage.setItem('app-theme-preference', cfg.theme);
          }
          if (cfg.language) {
            const normalized = normalizeLanguageCode(cfg.language);
            i18n.changeLanguage(normalized);
            document.documentElement.dir = normalized === 'ar' ? 'rtl' : 'ltr';
          }
        }
      })
    );

    // 监听托盘唤起 Mini 悬浮窗事件 (每次打开即刻同步最新配置与深浅状态)
    unlistenPromises.push(
      listen('tray://show-mini-view', async () => {
        console.log('[Mini] Tray opened mini view, syncing theme and language...');
        await useConfigStore.getState().loadConfig();
        const cfg = useConfigStore.getState().config;
        if (cfg) {
          if (cfg.theme) {
            const isDark = cfg.theme === 'dark' || (cfg.theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
            document.documentElement.classList.toggle('dark', isDark);
            document.documentElement.setAttribute('data-theme', isDark ? 'dark' : 'light');
            localStorage.setItem('app-theme-preference', cfg.theme);
          }
          if (cfg.language) {
            const normalized = normalizeLanguageCode(cfg.language);
            i18n.changeLanguage(normalized);
            document.documentElement.dir = normalized === 'ar' ? 'rtl' : 'ltr';
          }
        }
      })
    );

    // 监听跨窗口主题同步事件
    unlistenPromises.push(
      listen<{ theme: string }>('app://theme-changed', (event) => {
        console.log('[App] Received app://theme-changed:', event.payload);
        const theme = event.payload.theme;
        localStorage.setItem('app-theme-preference', theme);
        const isDark = theme === 'dark' || (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
        document.documentElement.classList.toggle('dark', isDark);
        document.documentElement.setAttribute('data-theme', isDark ? 'dark' : 'light');
        const current = useConfigStore.getState().config;
        if (current) {
          useConfigStore.setState({ config: { ...current, theme } });
        }
      })
    );

    // 监听跨窗口多语言同步事件
    unlistenPromises.push(
      listen<{ language: string }>('app://language-changed', (event) => {
        console.log('[App] Received app://language-changed:', event.payload);
        const lang = event.payload.language;
        const normalized = normalizeLanguageCode(lang);
        i18n.changeLanguage(normalized);
        document.documentElement.dir = normalized === 'ar' ? 'rtl' : 'ltr';
        const current = useConfigStore.getState().config;
        if (current) {
          useConfigStore.setState({ config: { ...current, language: normalized } });
        }
      })
    );

    // Cleanup
    return () => {
      Promise.all(unlistenPromises).then(unlisteners => {
        unlisteners.forEach(unlisten => unlisten());
      });
    };
  }, [fetchCurrentAccount, fetchAccounts, i18n]);

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

  const isMini =
    (typeof window !== 'undefined' && window.location.search.includes('mini')) ||
    (typeof window !== 'undefined' &&
      (window as any).__TAURI_INTERNALS__?.metadata?.currentWindow?.label === 'mini') ||
    (isTauri() &&
      (() => {
        try {
          return getCurrentWindow().label === 'mini';
        } catch {
          return false;
        }
      })());

  if (isMini) {
    if (typeof document !== 'undefined') {
      document.documentElement.classList.add('mini-window');
      document.documentElement.style.setProperty('background-color', 'transparent', 'important');
      document.documentElement.style.setProperty('background', 'transparent', 'important');
      if (document.body) {
        document.body.classList.add('mini-window');
        document.body.style.setProperty('background-color', 'transparent', 'important');
        document.body.style.setProperty('background', 'transparent', 'important');
      }
      const rootEl = document.getElementById('root');
      if (rootEl) {
        rootEl.classList.add('mini-root');
        rootEl.style.setProperty('background-color', 'transparent', 'important');
        rootEl.style.setProperty('background', 'transparent', 'important');
      }
    }
    return (
      <AdminAuthGuard>
        <ThemeManager />
        <MiniView />
      </AdminAuthGuard>
    );
  }

  return (
    <AdminAuthGuard>
      <ThemeManager />
      <ZoomManager />
      <DebugConsole />
      <SuggestionDeleteThinkingModal />
      {showUpdateNotification && (
        <UpdateNotification onClose={() => setShowUpdateNotification(false)} />
      )}
      <RouterProvider router={router} />
    </AdminAuthGuard>
  );
}

export default App;