// File: src/pages/dashboard/model/useDashboard.ts
// Dashboard page business logic hook

import { useMemo, useRef, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { save } from '@tauri-apps/plugin-dialog';

import { invoke } from '@/shared/api';
import { isTauri } from '@/shared/lib';
import { showToast } from '@/shared/ui';
import {
  useAccounts,
  useCurrentAccount,
  useAddAccount,
  useSwitchAccount,
  useRefreshQuota,
} from '@/features/accounts';
import type { Account } from '@/entities/account';

export interface DashboardStats {
  total: number;
  avgGemini: number;
  avgGeminiImage: number;
  avgClaude: number;
  lowQuota: number;
}

export function useDashboard() {
  const { t } = useTranslation();
  const navigate = useNavigate();

  // FSD Queries
  const { data: accounts = [], isLoading: isLoadingAccounts } = useAccounts();
  const { data: currentAccount } = useCurrentAccount();

  // FSD Mutations
  const addAccountMutation = useAddAccount();
  const switchAccountMutation = useSwitchAccount();
  const refreshQuotaMutation = useRefreshQuota();

  const isRefreshing = refreshQuotaMutation.isPending;
  const isSwitchingRef = useRef(false);

  // Derived State (Stats)
  const stats = useMemo<DashboardStats>(() => {
    if (!accounts || accounts.length === 0) {
      return { total: 0, avgGemini: 0, avgGeminiImage: 0, avgClaude: 0, lowQuota: 0 };
    }

    const geminiQuotas = accounts
      .map(a => a.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-pro-high')?.percentage || 0)
      .filter(q => q > 0);

    const geminiImageQuotas = accounts
      .map(a => a.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-pro-image')?.percentage || 0)
      .filter(q => q > 0);

    const claudeQuotas = accounts
      .map(a => a.quota?.models.find(m => m.name.toLowerCase() === 'claude-sonnet-4-5')?.percentage || 0)
      .filter(q => q > 0);

    const lowQuotaCount = accounts.filter(a => {
      if (a.quota?.is_forbidden) return false;
      const gemini = a.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-pro-high')?.percentage || 0;
      const claude = a.quota?.models.find(m => m.name.toLowerCase() === 'claude-sonnet-4-5')?.percentage || 0;
      return gemini < 20 || claude < 20;
    }).length;

    return {
      total: accounts.length,
      avgGemini: geminiQuotas.length > 0
        ? Math.round(geminiQuotas.reduce((a, b) => a + b, 0) / geminiQuotas.length)
        : 0,
      avgGeminiImage: geminiImageQuotas.length > 0
        ? Math.round(geminiImageQuotas.reduce((a, b) => a + b, 0) / geminiImageQuotas.length)
        : 0,
      avgClaude: claudeQuotas.length > 0
        ? Math.round(claudeQuotas.reduce((a, b) => a + b, 0) / claudeQuotas.length)
        : 0,
      lowQuota: lowQuotaCount,
    };
  }, [accounts]);

  // Handlers
  const handleSwitch = useCallback(async (accountId: string) => {
    if (isSwitchingRef.current) return;

    isSwitchingRef.current = true;
    try {
      await switchAccountMutation.mutateAsync(accountId);
      showToast(t('dashboard.toast.switch_success'), 'success');
    } catch (error) {
      console.error('Switch account failed:', error);
      showToast(`${t('dashboard.toast.switch_error')}: ${error}`, 'error');
    } finally {
      setTimeout(() => {
        isSwitchingRef.current = false;
      }, 500);
    }
  }, [switchAccountMutation, t]);

  const handleAddAccount = useCallback(async (email: string, refreshToken: string) => {
    await addAccountMutation.mutateAsync({ email, refreshToken });
  }, [addAccountMutation]);

  const handleRefreshCurrent = useCallback(async () => {
    if (!currentAccount) return;

    try {
      await refreshQuotaMutation.mutateAsync(currentAccount.id);
    } catch (error) {
      console.error('[Dashboard] Refresh failed:', error);
    }
  }, [currentAccount, refreshQuotaMutation]);

  const exportAccountsToJson = useCallback(async (accountsToExport: Account[]) => {
    try {
      if (!accountsToExport || accountsToExport.length === 0) {
        showToast(t('dashboard.toast.export_no_accounts'), 'warning');
        return;
      }

      const exportData = accountsToExport.map(acc => ({
        email: acc.email,
        refresh_token: acc.token.refresh_token,
      }));
      const content = JSON.stringify(exportData, null, 2);
      const fileName = `antigravity_accounts_${new Date().toISOString().split('T')[0]}.json`;

      if (isTauri()) {
        const path = await save({
          filters: [{ name: 'JSON', extensions: ['json'] }],
          defaultPath: fileName,
        });

        if (!path) return;

        await invoke('save_text_file', { path, content });
        showToast(t('dashboard.toast.export_success', { path }), 'success');
      } else {
        const blob = new Blob([content], { type: 'application/json' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = fileName;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
        showToast(t('dashboard.toast.export_success', { path: fileName }), 'success');
      }
    } catch (error: any) {
      console.error('Export failed:', error);
      showToast(`${t('dashboard.toast.export_error')}: ${error.toString()}`, 'error');
    }
  }, [t]);

  const handleExport = useCallback(() => {
    exportAccountsToJson(accounts);
  }, [accounts, exportAccountsToJson]);

  const navigateToAccounts = useCallback(() => {
    navigate('/accounts');
  }, [navigate]);

  return {
    // Data
    accounts,
    currentAccount,
    stats,
    isLoadingAccounts,
    isRefreshing,

    // Handlers
    handleSwitch,
    handleAddAccount,
    handleRefreshCurrent,
    handleExport,
    navigateToAccounts,
  };
}
