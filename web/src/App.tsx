

// Fixed imports below
import { createBrowserRouter, RouterProvider } from 'react-router-dom';

import Layout from './components/layout/Layout';
import Dashboard from './pages/Dashboard';
import Accounts from './pages/Accounts';
import Settings from './pages/Settings';
import ApiProxy from './pages/ApiProxy';
import ThemeManager from './components/common/ThemeManager';
import { useEffect } from 'react';
import { useConfigStore } from './stores/useConfigStore';
// import { useAccountStore } from './stores/useAccountStore'; // Unused in App
import { useTranslation } from 'react-i18next';
// import { listen } from '@tauri-apps/api/event'; // Removed

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
        path: 'settings',
        element: <Settings />,
      },
    ],
  },
]);

function App() {
  const { config, loadConfig } = useConfigStore();
  // const { fetchCurrentAccount, fetchAccounts } = useAccountStore(); // Unused
  const { i18n } = useTranslation();

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  // Sync language from config
  useEffect(() => {
    if (config?.language) {
      i18n.changeLanguage(config.language);
    }
  }, [config?.language, i18n]);

  // Listen for tray events (Removed for Web Version)
  /*
  useEffect(() => {
    // ... Tauri event listeners removed ...
  }, []);
  */

  return (
    <>
      <ThemeManager />
      <RouterProvider router={router} />
    </>
  );
}

export default App;
