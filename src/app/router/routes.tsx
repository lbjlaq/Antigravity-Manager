// File: src/app/router/routes.tsx
// Application routes configuration

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
        path: 'token-stats',
        element: <TokenStatsPage />,
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
