// File: src/app/App.tsx
// Main application component with all providers and global effects

import { useEffect, useState } from 'react';
import { RouterProvider } from 'react-router-dom';
import { listen } from '@tauri-apps/api/event';
import { useQueryClient } from '@tanstack/react-query';

import { router } from './router';
import { QueryProvider, I18nProvider } from './providers';

// FSD imports
import { useConfigStore } from '@/stores/useConfigStore';
import { useDebugConsole } from '@/widgets/debug-console';
import { isTauri } from '@/shared/lib';
import { invoke } from '@/shared/api';
import { showToast } from '@/components/common/ToastContainer';
import { accountKeys } from '@/features/accounts';

// Global components
import ThemeManager from '@/components/common/ThemeManager';
import { AdminAuthGuard } from '@/components/common/AdminAuthGuard';
import { DebugConsole } from '@/widgets/debug-console';
import { UpdateNotification } from '@/components/UpdateNotification';

function AppContent() {
  const { config, loadConfig } = useConfigStore();
  const checkDebugConsoleEnabled = useDebugConsole(s => s.checkEnabled);
  const queryClient = useQueryClient();

  // Invalidate accounts queries (replaces fetchCurrentAccount/fetchAccounts)
  const refreshAccounts = () => {
    queryClient.invalidateQueries({ queryKey: accountKeys.all });
  };

  // Load config on mount
  useEffect(() => {
    loadConfig();
    checkDebugConsoleEnabled();
  }, [loadConfig, checkDebugConsoleEnabled]);

  // Listen for tray events
  useEffect(() => {
    if (!isTauri()) return;
    const unlistenPromises: Promise<() => void>[] = [];

    // Listen for tray account switch
    unlistenPromises.push(
      listen('tray://account-switched', () => {
        console.log('[App] Tray account switched, refreshing...');
        refreshAccounts();
      })
    );

    // Listen for tray refresh
    unlistenPromises.push(
      listen('tray://refresh-current', () => {
        console.log('[App] Tray refresh triggered, refreshing...');
        refreshAccounts();
      })
    );

    // Listen for account validation blocked event
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
        refreshAccounts();
      })
    );

    return () => {
      Promise.all(unlistenPromises).then(unlisteners => {
        unlisteners.forEach(unlisten => unlisten());
      });
    };
  }, [queryClient]);

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
          await invoke('update_last_check_time');
          console.log('[App] Update check cycle initiated and last check time updated.');
        }
      } catch (error) {
        console.error('Failed to check update settings:', error);
      }
    };

    const timer = setTimeout(checkUpdates, 2000);
    return () => clearTimeout(timer);
  }, []);

  return (
    <I18nProvider language={config?.language}>
      <AdminAuthGuard>
        <ThemeManager />
        <DebugConsole />
        {showUpdateNotification && (
          <UpdateNotification onClose={() => setShowUpdateNotification(false)} />
        )}
        <RouterProvider router={router} />
      </AdminAuthGuard>
    </I18nProvider>
  );
}

export default function App() {
  return (
    <QueryProvider>
      <AppContent />
    </QueryProvider>
  );
}
