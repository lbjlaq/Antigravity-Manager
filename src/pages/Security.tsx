// File: src/pages/Security.tsx
// Main Security page with IP blacklist/whitelist management

import React, { useState, useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import {
  Shield,
  ShieldCheck,
  FileText,
  Settings,
  Plus,
  RefreshCw,
  Trash2,
  Ban,
  CheckCircle,
  Clock,
  Hash,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { showToast } from '../components/common/ToastContainer';
import { AddIpDialog } from '../components/security/AddIpDialog';
import {
  IpBlacklistEntry,
  IpWhitelistEntry,
  AccessLogEntry,
  SecurityStats,
  SecurityMonitorConfig,
  SecurityTab,
  AddToBlacklistRequest,
  AddToWhitelistRequest,
} from '../types/security';
import { formatDistanceToNow } from 'date-fns';
import { cn } from '../lib/utils';

export const Security: React.FC = () => {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<SecurityTab>('blacklist');
  const [isAddDialogOpen, setIsAddDialogOpen] = useState(false);
  const [isSubmitting, setIsSubmitting] = useState(false);

  // Data states
  const [blacklist, setBlacklist] = useState<IpBlacklistEntry[]>([]);
  const [whitelist, setWhitelist] = useState<IpWhitelistEntry[]>([]);
  const [accessLogs, setAccessLogs] = useState<AccessLogEntry[]>([]);
  const [stats, setStats] = useState<SecurityStats | null>(null);
  const [config, setConfig] = useState<SecurityMonitorConfig | null>(null);

  // Loading states
  const [isLoading, setIsLoading] = useState(false);

  // Initialize security database
  useEffect(() => {
    invoke('security_init_db').catch((e) => {
      console.error('Failed to init security db:', e);
    });
    loadStats();
  }, []);

  // Load data based on active tab
  useEffect(() => {
    if (activeTab === 'blacklist') loadBlacklist();
    else if (activeTab === 'whitelist') loadWhitelist();
    else if (activeTab === 'logs') loadAccessLogs();
    else if (activeTab === 'settings') loadConfig();
  }, [activeTab]);

  const loadBlacklist = async () => {
    setIsLoading(true);
    try {
      const data = await invoke<IpBlacklistEntry[]>('security_get_blacklist');
      setBlacklist(data);
    } catch (e) {
      console.error('Failed to load blacklist:', e);
    } finally {
      setIsLoading(false);
    }
  };

  const loadWhitelist = async () => {
    setIsLoading(true);
    try {
      const data = await invoke<IpWhitelistEntry[]>('security_get_whitelist');
      setWhitelist(data);
    } catch (e) {
      console.error('Failed to load whitelist:', e);
    } finally {
      setIsLoading(false);
    }
  };

  const loadAccessLogs = async () => {
    setIsLoading(true);
    try {
      const data = await invoke<AccessLogEntry[]>('security_get_access_logs', {
        request: { limit: 50, offset: 0, blockedOnly: false },
      });
      setAccessLogs(data);
    } catch (e) {
      console.error('Failed to load logs:', e);
    } finally {
      setIsLoading(false);
    }
  };

  const loadStats = async () => {
    try {
      const data = await invoke<SecurityStats>('security_get_stats');
      setStats(data);
    } catch (e) {
      console.error('Failed to load stats:', e);
    }
  };

  const loadConfig = async () => {
    setIsLoading(true);
    try {
      const data = await invoke<SecurityMonitorConfig>('get_security_config');
      setConfig(data);
    } catch (e) {
      console.error('Failed to load config:', e);
    } finally {
      setIsLoading(false);
    }
  };

  const handleAddToBlacklist = async (data: {
    ipPattern: string;
    reason?: string;
    expiresInSeconds?: number;
  }) => {
    setIsSubmitting(true);
    try {
      const request: AddToBlacklistRequest = {
        ipPattern: data.ipPattern,
        reason: data.reason || '',
        expiresInSeconds: data.expiresInSeconds,
        createdBy: 'user',
      };
      await invoke('security_add_to_blacklist', { request });
      showToast(t('security.ip_blocked', 'IP blocked successfully'), 'success');
      setIsAddDialogOpen(false);
      loadBlacklist();
      loadStats();
    } catch (e) {
      showToast(String(e), 'error');
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleAddToWhitelist = async (data: {
    ipPattern: string;
    description?: string;
  }) => {
    setIsSubmitting(true);
    try {
      const request: AddToWhitelistRequest = {
        ipPattern: data.ipPattern,
        description: data.description || '',
        createdBy: 'user',
      };
      await invoke('security_add_to_whitelist', { request });
      showToast(t('security.ip_whitelisted', 'IP whitelisted successfully'), 'success');
      setIsAddDialogOpen(false);
      loadWhitelist();
      loadStats();
    } catch (e) {
      showToast(String(e), 'error');
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleRemoveFromBlacklist = async (id: number) => {
    try {
      await invoke('security_remove_from_blacklist_by_id', { id });
      showToast(t('security.ip_unblocked', 'IP removed from blacklist'), 'success');
      loadBlacklist();
      loadStats();
    } catch (e) {
      showToast(String(e), 'error');
    }
  };

  const handleRemoveFromWhitelist = async (id: number) => {
    try {
      await invoke('security_remove_from_whitelist_by_id', { id });
      showToast(t('security.ip_removed', 'IP removed from whitelist'), 'success');
      loadWhitelist();
      loadStats();
    } catch (e) {
      showToast(String(e), 'error');
    }
  };

  const handleClearLogs = async () => {
    if (!confirm(t('security.confirm_clear_logs', 'Clear all access logs?'))) return;
    try {
      await invoke('security_clear_all_logs');
      showToast(t('security.logs_cleared', 'Access logs cleared'), 'success');
      loadAccessLogs();
      loadStats();
    } catch (e) {
      showToast(String(e), 'error');
    }
  };

  const handleSaveConfig = async (newConfig: SecurityMonitorConfig) => {
    try {
      await invoke('update_security_config', { config: newConfig });
      setConfig(newConfig);
      showToast(t('security.config_saved', 'Configuration saved'), 'success');
    } catch (e) {
      showToast(String(e), 'error');
    }
  };

  const formatTimestamp = (timestamp: number): string => {
    try {
      return formatDistanceToNow(new Date(timestamp * 1000), { addSuffix: true });
    } catch {
      return 'Unknown';
    }
  };

  const formatExpiresAt = (expiresAt: number | null): string => {
    if (!expiresAt) return t('security.permanent', 'Permanent');
    const now = Date.now() / 1000;
    if (expiresAt < now) return t('security.expired', 'Expired');
    try {
      return formatDistanceToNow(new Date(expiresAt * 1000), { addSuffix: true });
    } catch {
      return 'Unknown';
    }
  };

  const tabs: { id: SecurityTab; label: string; icon: React.ReactNode; count?: number }[] = [
    {
      id: 'blacklist',
      label: t('security.blacklist', 'Blacklist'),
      icon: <Ban className="w-4 h-4" />,
      count: stats?.blacklistCount,
    },
    {
      id: 'whitelist',
      label: t('security.whitelist', 'Whitelist'),
      icon: <CheckCircle className="w-4 h-4" />,
      count: stats?.whitelistCount,
    },
    {
      id: 'logs',
      label: t('security.access_logs', 'Logs'),
      icon: <FileText className="w-4 h-4" />,
    },
    {
      id: 'settings',
      label: t('security.settings', 'Settings'),
      icon: <Settings className="w-4 h-4" />,
    },
  ];

  return (
    <div className="h-full flex flex-col p-5 gap-4 max-w-5xl mx-auto w-full overflow-y-auto">
      {/* Header Card */}
      <div className="bg-white dark:bg-zinc-900 rounded-2xl border border-gray-200 dark:border-zinc-800 p-5 shadow-sm">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <div className="p-3 rounded-xl bg-gradient-to-br from-indigo-500 to-purple-600 shadow-lg shadow-indigo-500/25">
              <Shield className="w-6 h-6 text-white" />
            </div>
            <div>
              <h1 className="text-xl font-bold text-gray-900 dark:text-white">
                {t('security.title', 'IP Security')}
              </h1>
              <p className="text-sm text-gray-500 dark:text-zinc-500">
                {t('security.subtitle', 'Manage access control for your proxy')}
              </p>
            </div>
          </div>

          {/* Stats Pills */}
          {stats && (
            <div className="flex items-center gap-3">
              <div className="flex items-center gap-2 px-3 py-1.5 rounded-full bg-red-50 dark:bg-red-500/10 border border-red-200 dark:border-red-500/20">
                <Ban className="w-3.5 h-3.5 text-red-500" />
                <span className="text-sm font-semibold text-red-600 dark:text-red-400">{stats.blockedRequests}</span>
                <span className="text-xs text-red-500/70">{t('security.blocked', 'blocked')}</span>
              </div>
              <div className="flex items-center gap-2 px-3 py-1.5 rounded-full bg-blue-50 dark:bg-blue-500/10 border border-blue-200 dark:border-blue-500/20">
                <Hash className="w-3.5 h-3.5 text-blue-500" />
                <span className="text-sm font-semibold text-blue-600 dark:text-blue-400">{stats.uniqueIps}</span>
                <span className="text-xs text-blue-500/70">{t('security.ips', 'IPs')}</span>
              </div>
            </div>
          )}
        </div>

        {/* Tabs */}
        <div className="flex items-center gap-1 mt-5 p-1 bg-gray-100 dark:bg-zinc-800 rounded-xl">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={cn(
                'flex-1 flex items-center justify-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-all',
                activeTab === tab.id
                  ? 'bg-white dark:bg-zinc-700 text-gray-900 dark:text-white shadow-sm'
                  : 'text-gray-500 dark:text-zinc-400 hover:text-gray-700 dark:hover:text-zinc-300'
              )}
            >
              {tab.icon}
              {tab.label}
              {tab.count !== undefined && tab.count > 0 && (
                <span className={cn(
                  'px-1.5 py-0.5 text-[10px] font-bold rounded-full',
                  activeTab === tab.id
                    ? 'bg-indigo-100 dark:bg-indigo-500/20 text-indigo-600 dark:text-indigo-400'
                    : 'bg-gray-200 dark:bg-zinc-600 text-gray-600 dark:text-zinc-300'
                )}>
                  {tab.count}
                </span>
              )}
            </button>
          ))}
        </div>
      </div>

      {/* Content Card */}
      <div className="flex-1 bg-white dark:bg-zinc-900 rounded-2xl border border-gray-200 dark:border-zinc-800 shadow-sm overflow-hidden flex flex-col">
        {/* Toolbar */}
        <div className="flex items-center justify-between px-5 py-3 border-b border-gray-100 dark:border-zinc-800">
          <div className="text-sm text-gray-500 dark:text-zinc-500">
            {activeTab === 'blacklist' && `${blacklist.length} ${t('security.entries', 'entries')}`}
            {activeTab === 'whitelist' && `${whitelist.length} ${t('security.entries', 'entries')}`}
            {activeTab === 'logs' && `${accessLogs.length} ${t('security.records', 'records')}`}
            {activeTab === 'settings' && t('security.configure', 'Configure security settings')}
          </div>
          
          <div className="flex items-center gap-2">
            {(activeTab === 'blacklist' || activeTab === 'whitelist') && (
              <motion.button
                whileHover={{ scale: 1.02 }}
                whileTap={{ scale: 0.98 }}
                onClick={() => setIsAddDialogOpen(true)}
                className={cn(
                  'flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-semibold transition-all',
                  activeTab === 'blacklist'
                    ? 'bg-red-500 hover:bg-red-600 text-white'
                    : 'bg-emerald-500 hover:bg-emerald-600 text-white'
                )}
              >
                <Plus className="w-4 h-4" />
                {t('common.add', 'Add')}
              </motion.button>
            )}

            {activeTab === 'logs' && (
              <>
                <button
                  onClick={loadAccessLogs}
                  className="flex items-center gap-2 px-3 py-2 rounded-lg bg-gray-100 dark:bg-zinc-800 text-gray-600 dark:text-zinc-300 hover:bg-gray-200 dark:hover:bg-zinc-700 transition-colors text-sm"
                >
                  <RefreshCw className="w-4 h-4" />
                </button>
                <button
                  onClick={handleClearLogs}
                  className="flex items-center gap-2 px-3 py-2 rounded-lg bg-red-50 dark:bg-red-500/10 text-red-600 dark:text-red-400 hover:bg-red-100 dark:hover:bg-red-500/20 transition-colors text-sm"
                >
                  <Trash2 className="w-4 h-4" />
                </button>
              </>
            )}
          </div>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-5">
          {isLoading ? (
            <div className="space-y-3">
              {[...Array(3)].map((_, i) => (
                <div key={i} className="h-16 bg-gray-100 dark:bg-zinc-800 rounded-xl animate-pulse" />
              ))}
            </div>
          ) : (
            <AnimatePresence mode="wait">
              {/* Blacklist */}
              {activeTab === 'blacklist' && (
                <motion.div
                  key="blacklist"
                  initial={{ opacity: 0, y: 10 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -10 }}
                  className="space-y-2"
                >
                  {blacklist.length === 0 ? (
                    <div className="flex flex-col items-center justify-center py-12 text-gray-400 dark:text-zinc-500">
                      <Shield className="w-12 h-12 mb-3 opacity-30" />
                      <p className="text-sm">{t('security.no_blacklist', 'No blocked IPs')}</p>
                    </div>
                  ) : (
                    blacklist.map((entry) => (
                      <div
                        key={entry.id}
                        className="group flex items-center justify-between p-4 rounded-xl bg-gray-50 dark:bg-zinc-800/50 border border-gray-100 dark:border-zinc-700/50 hover:border-red-200 dark:hover:border-red-500/30 transition-all"
                      >
                        <div className="flex items-center gap-4">
                          <div className="p-2 rounded-lg bg-red-100 dark:bg-red-500/10">
                            <Ban className="w-4 h-4 text-red-500" />
                          </div>
                          <div>
                            <div className="flex items-center gap-2">
                              <span className="font-mono text-sm font-semibold text-gray-900 dark:text-white">
                                {entry.ipPattern}
                              </span>
                              {entry.ipPattern.includes('/') && (
                                <span className="px-1.5 py-0.5 text-[10px] font-bold rounded bg-blue-100 dark:bg-blue-500/20 text-blue-600 dark:text-blue-400">
                                  CIDR
                                </span>
                              )}
                            </div>
                            {entry.reason && (
                              <p className="text-xs text-gray-500 dark:text-zinc-500 mt-0.5">{entry.reason}</p>
                            )}
                          </div>
                        </div>
                        <div className="flex items-center gap-4">
                          <div className="flex items-center gap-3 text-xs text-gray-400 dark:text-zinc-500">
                            <span className="flex items-center gap-1">
                              <Hash className="w-3 h-3" />
                              {entry.hitCount}
                            </span>
                            <span className="flex items-center gap-1">
                              <Clock className="w-3 h-3" />
                              {formatExpiresAt(entry.expiresAt)}
                            </span>
                          </div>
                          <button
                            onClick={() => handleRemoveFromBlacklist(entry.id)}
                            className="p-2 rounded-lg text-gray-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-500/10 opacity-0 group-hover:opacity-100 transition-all"
                          >
                            <Trash2 className="w-4 h-4" />
                          </button>
                        </div>
                      </div>
                    ))
                  )}
                </motion.div>
              )}

              {/* Whitelist */}
              {activeTab === 'whitelist' && (
                <motion.div
                  key="whitelist"
                  initial={{ opacity: 0, y: 10 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -10 }}
                  className="space-y-2"
                >
                  {whitelist.length === 0 ? (
                    <div className="flex flex-col items-center justify-center py-12 text-gray-400 dark:text-zinc-500">
                      <ShieldCheck className="w-12 h-12 mb-3 opacity-30" />
                      <p className="text-sm">{t('security.no_whitelist', 'No whitelisted IPs')}</p>
                    </div>
                  ) : (
                    whitelist.map((entry) => (
                      <div
                        key={entry.id}
                        className="group flex items-center justify-between p-4 rounded-xl bg-gray-50 dark:bg-zinc-800/50 border border-gray-100 dark:border-zinc-700/50 hover:border-emerald-200 dark:hover:border-emerald-500/30 transition-all"
                      >
                        <div className="flex items-center gap-4">
                          <div className="p-2 rounded-lg bg-emerald-100 dark:bg-emerald-500/10">
                            <CheckCircle className="w-4 h-4 text-emerald-500" />
                          </div>
                          <div>
                            <div className="flex items-center gap-2">
                              <span className="font-mono text-sm font-semibold text-gray-900 dark:text-white">
                                {entry.ipPattern}
                              </span>
                              {entry.ipPattern.includes('/') && (
                                <span className="px-1.5 py-0.5 text-[10px] font-bold rounded bg-blue-100 dark:bg-blue-500/20 text-blue-600 dark:text-blue-400">
                                  CIDR
                                </span>
                              )}
                            </div>
                            {entry.description && (
                              <p className="text-xs text-gray-500 dark:text-zinc-500 mt-0.5">{entry.description}</p>
                            )}
                          </div>
                        </div>
                        <div className="flex items-center gap-4">
                          <span className="text-xs text-gray-400 dark:text-zinc-500">
                            {formatTimestamp(entry.createdAt)}
                          </span>
                          <button
                            onClick={() => handleRemoveFromWhitelist(entry.id)}
                            className="p-2 rounded-lg text-gray-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-500/10 opacity-0 group-hover:opacity-100 transition-all"
                          >
                            <Trash2 className="w-4 h-4" />
                          </button>
                        </div>
                      </div>
                    ))
                  )}
                </motion.div>
              )}

              {/* Access Logs */}
              {activeTab === 'logs' && (
                <motion.div
                  key="logs"
                  initial={{ opacity: 0, y: 10 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -10 }}
                  className="space-y-2"
                >
                  {accessLogs.length === 0 ? (
                    <div className="flex flex-col items-center justify-center py-12 text-gray-400 dark:text-zinc-500">
                      <FileText className="w-12 h-12 mb-3 opacity-30" />
                      <p className="text-sm">{t('security.no_logs', 'No access logs')}</p>
                    </div>
                  ) : (
                    accessLogs.map((log) => (
                      <div
                        key={log.id}
                        className={cn(
                          'flex items-center justify-between p-3 rounded-xl border transition-all',
                          log.blocked
                            ? 'bg-red-50 dark:bg-red-500/5 border-red-200 dark:border-red-500/20'
                            : 'bg-gray-50 dark:bg-zinc-800/50 border-gray-100 dark:border-zinc-700/50'
                        )}
                      >
                        <div className="flex items-center gap-3">
                          <span
                            className={cn(
                              'px-2 py-1 text-xs font-mono font-bold rounded',
                              log.blocked
                                ? 'bg-red-100 dark:bg-red-500/20 text-red-600 dark:text-red-400'
                                : 'bg-gray-100 dark:bg-zinc-700 text-gray-600 dark:text-zinc-300'
                            )}
                          >
                            {log.statusCode}
                          </span>
                          <span className="font-mono text-sm text-gray-700 dark:text-zinc-300">{log.ipAddress}</span>
                          <span className="text-xs text-gray-400 dark:text-zinc-500 truncate max-w-[200px]">
                            {log.method} {log.path}
                          </span>
                        </div>
                        <div className="flex items-center gap-3 text-xs text-gray-400 dark:text-zinc-500">
                          {log.blocked && log.blockReason && (
                            <span className="text-red-500 dark:text-red-400">{log.blockReason}</span>
                          )}
                          <span>{formatTimestamp(log.timestamp)}</span>
                        </div>
                      </div>
                    ))
                  )}
                </motion.div>
              )}

              {/* Settings */}
              {activeTab === 'settings' && config && (
                <motion.div
                  key="settings"
                  initial={{ opacity: 0, y: 10 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -10 }}
                  className="space-y-4 max-w-xl"
                >
                  {/* Blacklist Settings */}
                  <div className="p-4 rounded-xl bg-gray-50 dark:bg-zinc-800/50 border border-gray-100 dark:border-zinc-700/50">
                    <div className="flex items-center gap-3 mb-4">
                      <div className="p-2 rounded-lg bg-red-100 dark:bg-red-500/10">
                        <Ban className="w-4 h-4 text-red-500" />
                      </div>
                      <h3 className="font-semibold text-gray-900 dark:text-white">
                        {t('security.blacklist_settings', 'Blacklist')}
                      </h3>
                    </div>
                    <label className="flex items-center justify-between cursor-pointer">
                      <span className="text-sm text-gray-600 dark:text-zinc-400">
                        {t('security.enable_blacklist', 'Enable Blacklist')}
                      </span>
                      <input
                        type="checkbox"
                        checked={config.blacklist.enabled}
                        onChange={(e) =>
                          handleSaveConfig({
                            ...config,
                            blacklist: { ...config.blacklist, enabled: e.target.checked },
                          })
                        }
                        className="w-5 h-5 rounded bg-gray-200 dark:bg-zinc-700 border-0 text-red-500 focus:ring-red-500"
                      />
                    </label>
                  </div>

                  {/* Whitelist Settings */}
                  <div className="p-4 rounded-xl bg-gray-50 dark:bg-zinc-800/50 border border-gray-100 dark:border-zinc-700/50">
                    <div className="flex items-center gap-3 mb-4">
                      <div className="p-2 rounded-lg bg-emerald-100 dark:bg-emerald-500/10">
                        <CheckCircle className="w-4 h-4 text-emerald-500" />
                      </div>
                      <h3 className="font-semibold text-gray-900 dark:text-white">
                        {t('security.whitelist_settings', 'Whitelist')}
                      </h3>
                    </div>
                    <div className="space-y-3">
                      <label className="flex items-center justify-between cursor-pointer">
                        <span className="text-sm text-gray-600 dark:text-zinc-400">
                          {t('security.enable_whitelist', 'Enable Whitelist')}
                        </span>
                        <input
                          type="checkbox"
                          checked={config.whitelist.enabled}
                          onChange={(e) =>
                            handleSaveConfig({
                              ...config,
                              whitelist: { ...config.whitelist, enabled: e.target.checked },
                            })
                          }
                          className="w-5 h-5 rounded bg-gray-200 dark:bg-zinc-700 border-0 text-emerald-500 focus:ring-emerald-500"
                        />
                      </label>
                      <label className="flex items-center justify-between cursor-pointer">
                        <div>
                          <span className="text-sm text-gray-600 dark:text-zinc-400">
                            {t('security.strict_mode', 'Strict Mode')}
                          </span>
                          <p className="text-xs text-gray-400 dark:text-zinc-500">
                            {t('security.strict_mode_desc', 'Only whitelisted IPs can access')}
                          </p>
                        </div>
                        <input
                          type="checkbox"
                          checked={config.whitelist.strictMode}
                          onChange={(e) =>
                            handleSaveConfig({
                              ...config,
                              whitelist: { ...config.whitelist, strictMode: e.target.checked },
                            })
                          }
                          className="w-5 h-5 rounded bg-gray-200 dark:bg-zinc-700 border-0 text-emerald-500 focus:ring-emerald-500"
                        />
                      </label>
                    </div>
                  </div>
                </motion.div>
              )}
            </AnimatePresence>
          )}
        </div>
      </div>

      {/* Add IP Dialog */}
      <AddIpDialog
        isOpen={isAddDialogOpen}
        type={activeTab === 'whitelist' ? 'whitelist' : 'blacklist'}
        onClose={() => setIsAddDialogOpen(false)}
        onSubmit={activeTab === 'whitelist' ? handleAddToWhitelist : handleAddToBlacklist}
        isSubmitting={isSubmitting}
      />
    </div>
  );
};

export default Security;
