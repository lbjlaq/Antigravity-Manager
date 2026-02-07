// File: src/pages/settings/model/useSettings.ts
// Settings page business logic hook

import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { getVersion } from '@tauri-apps/api/app';

import { invoke } from '@/shared/api';
import { useConfigStore } from '@/entities/config';
import { showToast } from '@/shared/ui';
import type { AppConfig } from '@/entities/config';
import { DEFAULT_CONFIG, type SectionId } from '../lib';

export function useSettings() {
  const { t, i18n } = useTranslation();
  const { config, loadConfig, saveConfig, updateLanguage, updateTheme } = useConfigStore();
  
  const [activeTab, setActiveTab] = useState<SectionId>('general');
  const [appVersion, setAppVersion] = useState('');
  const [dataDirPath, setDataDirPath] = useState<string>('~/.antigravity_tools/');
  const [formData, setFormData] = useState<AppConfig>(DEFAULT_CONFIG as AppConfig);

  // Initial Load
  useEffect(() => {
    loadConfig();
    getVersion().then(setAppVersion).catch(() => setAppVersion('5.0.5'));
    invoke<string>('get_data_dir_path').then(setDataDirPath).catch(console.error);
    invoke<{ auto_check: boolean; check_interval_hours: number }>('get_update_settings')
      .then(s => setFormData(p => ({ ...p, auto_check_update: s.auto_check, update_check_interval: s.check_interval_hours })))
      .catch(console.error);
    invoke<boolean>('is_auto_launch_enabled')
      .then(e => setFormData(p => ({ ...p, auto_launch: e })))
      .catch(console.error);
  }, [loadConfig]);

  // Sync Config to Form
  useEffect(() => {
    if (config) setFormData(config);
  }, [config]);

  // Handlers
  const handleSave = useCallback(async () => {
    try {
      const proxyEnabled = formData.proxy?.upstream_proxy?.enabled;
      const proxyUrl = formData.proxy?.upstream_proxy?.url?.trim();
      if (proxyEnabled && !proxyUrl) {
        showToast(t('proxy.config.upstream_proxy.validation_error'), 'error');
        return;
      }
      await saveConfig({ ...formData, auto_refresh: true });
      showToast(t('common.saved'), 'success');
      if (proxyEnabled && proxyUrl) {
        showToast(t('proxy.config.upstream_proxy.restart_hint'), 'info');
      }
    } catch (error) {
      showToast(`${t('common.error')}: ${error}`, 'error');
    }
  }, [formData, saveConfig, t]);

  const handleLanguageChange = useCallback((val: string) => {
    setFormData(prev => ({ ...prev, language: val }));
    i18n.changeLanguage(val);
    updateLanguage(val);
  }, [i18n, updateLanguage]);

  const handleThemeChange = useCallback((val: string) => {
    setFormData(prev => ({ ...prev, theme: val }));
    updateTheme(val);
  }, [updateTheme]);

  const handleAutoLaunchChange = useCallback(async (checked: boolean) => {
    try {
      await invoke('toggle_auto_launch', { enable: checked });
      setFormData(prev => ({ ...prev, auto_launch: checked }));
    } catch (e) {
      showToast(String(e), 'error');
    }
  }, []);

  const handleAutoCheckUpdateChange = useCallback(async (checked: boolean) => {
    try {
      await invoke('save_update_settings', {
        settings: {
          auto_check: checked,
          last_check_time: 0,
          check_interval_hours: formData.update_check_interval ?? 24,
        },
      });
      setFormData(prev => ({ ...prev, auto_check_update: checked }));
    } catch (e) {
      showToast(String(e), 'error');
    }
  }, [formData.update_check_interval]);

  const updateFormData = useCallback((updates: Partial<AppConfig>) => {
    setFormData(prev => ({ ...prev, ...updates }));
  }, []);

  const updateProxyConfig = useCallback((updates: Partial<AppConfig['proxy']>) => {
    setFormData(prev => ({
      ...prev,
      proxy: { ...prev.proxy, ...updates },
    }));
  }, []);

  return {
    // State
    activeTab,
    setActiveTab,
    appVersion,
    dataDirPath,
    formData,
    
    // Handlers
    handleSave,
    handleLanguageChange,
    handleThemeChange,
    handleAutoLaunchChange,
    handleAutoCheckUpdateChange,
    updateFormData,
    updateProxyConfig,
  };
}
