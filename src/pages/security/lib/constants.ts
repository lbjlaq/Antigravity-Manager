// File: src/pages/security/lib/constants.ts
// Constants and types for Security page

export type SecurityTab = 'stats' | 'blacklist' | 'whitelist' | 'logs' | 'settings';

export const TABS_CONFIG = [
    { id: 'stats' as const, labelKey: 'security.stats', defaultLabel: 'Stats' },
    { id: 'blacklist' as const, labelKey: 'security.blacklist', defaultLabel: 'Blacklist' },
    { id: 'whitelist' as const, labelKey: 'security.whitelist', defaultLabel: 'Whitelist' },
    { id: 'logs' as const, labelKey: 'security.access_logs', defaultLabel: 'Logs' },
    { id: 'settings' as const, labelKey: 'security.settings', defaultLabel: 'Settings' },
];
