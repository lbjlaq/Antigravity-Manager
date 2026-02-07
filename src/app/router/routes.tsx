// File: src/app/router/routes.tsx
// Application routes configuration

import { lazy, Suspense } from 'react';
import { createBrowserRouter } from 'react-router-dom';

// Layout (FSD)
import { Layout } from '@/widgets/layout';

// Pages (FSD)
import { DashboardPage } from '@/pages/dashboard';
import { AccountsPage } from '@/pages/accounts';
import { SettingsPage } from '@/pages/settings';
import { ApiProxyPage } from '@/pages/api-proxy';
import { SecurityPage } from '@/pages/security';
import { TokenStatsPage } from '@/pages/token-stats';
import { MonitorPage } from '@/pages/monitor';
import { LogsPage } from '@/pages/logs';

// Lazy loaded pages (heavy components)
const ConsolePage = lazy(() => import('@/pages/console/ui/ConsolePage'));

// Loading fallback for lazy pages
const PageLoader = () => (
  <div className="flex items-center justify-center h-full">
    <div className="w-8 h-8 border-2 border-indigo-500 border-t-transparent rounded-full animate-spin" />
  </div>
);

export const router = createBrowserRouter([
  {
    path: '/',
    element: <Layout />,
    children: [
      {
        index: true,
        element: <DashboardPage />,
      },
      {
        path: 'accounts',
        element: <AccountsPage />,
      },
      {
        path: 'api-proxy',
        element: <ApiProxyPage />,
      },
      {
        path: 'monitor',
        element: <MonitorPage />,
      },
      {
        path: 'logs',
        element: <LogsPage />,
      },
      {
        path: 'token-stats',
        element: <TokenStatsPage />,
      },
      {
        path: 'console',
        element: <Suspense fallback={<PageLoader />}><ConsolePage /></Suspense>,
      },
      {
        path: 'security',
        element: <SecurityPage />,
      },
      {
        path: 'settings',
        element: <SettingsPage />,
      },
    ],
  },
]);
