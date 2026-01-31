// File: src/pages/security/model/useSecurity.ts
// Business logic hook for Security page

import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { formatDistanceToNow } from 'date-fns';
import { invoke } from '@/shared/api';
import { showToast } from '@/components/common/ToastContainer';
import type {
    IpBlacklistEntry,
    IpWhitelistEntry,
    AccessLogEntry,
    SecurityStats,
    SecurityMonitorConfig,
    AddToBlacklistRequest,
    AddToWhitelistRequest,
} from '@/types/security';
import type { SecurityTab } from '../lib/constants';

export function useSecurity() {
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

    const loadBlacklist = useCallback(async () => {
        setIsLoading(true);
        try {
            const data = await invoke<IpBlacklistEntry[]>('security_get_blacklist');
            setBlacklist(data);
        } catch (e) {
            console.error('Failed to load blacklist:', e);
        } finally {
            setIsLoading(false);
        }
    }, []);

    const loadWhitelist = useCallback(async () => {
        setIsLoading(true);
        try {
            const data = await invoke<IpWhitelistEntry[]>('security_get_whitelist');
            setWhitelist(data);
        } catch (e) {
            console.error('Failed to load whitelist:', e);
        } finally {
            setIsLoading(false);
        }
    }, []);

    const loadAccessLogs = useCallback(async () => {
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
    }, []);

    const loadStats = useCallback(async () => {
        try {
            const data = await invoke<SecurityStats>('security_get_stats');
            setStats(data);
        } catch (e) {
            console.error('Failed to load stats:', e);
        }
    }, []);

    const loadConfig = useCallback(async () => {
        setIsLoading(true);
        try {
            const data = await invoke<SecurityMonitorConfig>('get_security_config');
            setConfig(data);
        } catch (e) {
            console.error('Failed to load config:', e);
        } finally {
            setIsLoading(false);
        }
    }, []);

    const handleAddToBlacklist = useCallback(async (data: {
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
    }, [t, loadBlacklist, loadStats]);

    const handleAddToWhitelist = useCallback(async (data: {
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
    }, [t, loadWhitelist, loadStats]);

    const handleRemoveFromBlacklist = useCallback(async (id: number) => {
        try {
            await invoke('security_remove_from_blacklist_by_id', { id });
            showToast(t('security.ip_unblocked', 'IP removed from blacklist'), 'success');
            loadBlacklist();
            loadStats();
        } catch (e) {
            showToast(String(e), 'error');
        }
    }, [t, loadBlacklist, loadStats]);

    const handleRemoveFromWhitelist = useCallback(async (id: number) => {
        try {
            await invoke('security_remove_from_whitelist_by_id', { id });
            showToast(t('security.ip_removed', 'IP removed from whitelist'), 'success');
            loadWhitelist();
            loadStats();
        } catch (e) {
            showToast(String(e), 'error');
        }
    }, [t, loadWhitelist, loadStats]);

    const handleClearLogs = useCallback(async () => {
        if (!confirm(t('security.confirm_clear_logs', 'Clear all access logs?'))) return;
        try {
            await invoke('security_clear_all_logs');
            showToast(t('security.logs_cleared', 'Access logs cleared'), 'success');
            loadAccessLogs();
            loadStats();
        } catch (e) {
            showToast(String(e), 'error');
        }
    }, [t, loadAccessLogs, loadStats]);

    const handleSaveConfig = useCallback(async (newConfig: SecurityMonitorConfig) => {
        try {
            await invoke('update_security_config', { config: newConfig });
            setConfig(newConfig);
            showToast(t('security.config_saved', 'Configuration saved'), 'success');
        } catch (e) {
            showToast(String(e), 'error');
        }
    }, [t]);

    const formatTimestamp = useCallback((timestamp: number): string => {
        try {
            return formatDistanceToNow(new Date(timestamp * 1000), { addSuffix: true });
        } catch {
            return 'Unknown';
        }
    }, []);

    const formatExpiresAt = useCallback((expiresAt: number | null): string => {
        if (!expiresAt) return t('security.permanent', 'Permanent');
        const now = Date.now() / 1000;
        if (expiresAt < now) return t('security.expired', 'Expired');
        try {
            return formatDistanceToNow(new Date(expiresAt * 1000), { addSuffix: true });
        } catch {
            return 'Unknown';
        }
    }, [t]);

    return {
        // State
        activeTab,
        isAddDialogOpen,
        isSubmitting,
        blacklist,
        whitelist,
        accessLogs,
        stats,
        config,
        isLoading,

        // Setters
        setActiveTab,
        setIsAddDialogOpen,

        // Actions
        loadBlacklist,
        loadWhitelist,
        loadAccessLogs,
        loadStats,
        loadConfig,
        handleAddToBlacklist,
        handleAddToWhitelist,
        handleRemoveFromBlacklist,
        handleRemoveFromWhitelist,
        handleClearLogs,
        handleSaveConfig,

        // Helpers
        formatTimestamp,
        formatExpiresAt,
    };
}
