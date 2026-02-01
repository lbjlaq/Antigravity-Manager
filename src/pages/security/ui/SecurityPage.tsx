// File: src/pages/security/ui/SecurityPage.tsx
// Security page - redesigned to match Accounts page style

import { memo } from 'react';
import { AnimatePresence } from 'framer-motion';

import { useSecurity } from '../model';
import { SecurityHeader } from './SecurityHeader';
import { SecurityTabs } from './SecurityTabs';
import { SecurityToolbar } from './SecurityToolbar';
import { BlacklistTab } from './BlacklistTab';
import { WhitelistTab } from './WhitelistTab';
import { LogsTab } from './LogsTab';
import { SettingsTab } from './SettingsTab';
import { AddIpDialog } from '@/features/security';

export const SecurityPage = memo(function SecurityPage() {
    const security = useSecurity();

    return (
        <div className="h-full flex flex-col p-5 gap-4 max-w-7xl mx-auto w-full">
            {/* Main Card - Single container like Accounts */}
            <div className="flex-1 min-h-0 relative flex flex-col">
                <div className="h-full bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 flex flex-col overflow-hidden">
                    
                    {/* Header */}
                    <SecurityHeader stats={security.stats} />

                    {/* Tabs in toolbar style */}
                    <SecurityTabs
                        activeTab={security.activeTab}
                        stats={security.stats}
                        onTabChange={security.setActiveTab}
                    />

                    {/* Toolbar */}
                    <SecurityToolbar
                        activeTab={security.activeTab}
                        blacklistCount={security.blacklist.length}
                        whitelistCount={security.whitelist.length}
                        logsCount={security.accessLogs.length}
                        onAddClick={() => security.setIsAddDialogOpen(true)}
                        onRefreshLogs={security.loadAccessLogs}
                        onClearLogs={security.handleClearLogs}
                    />

                    {/* Content Area */}
                    <div className="flex-1 min-h-0 overflow-y-auto p-4">
                        {security.isLoading ? (
                            <div className="space-y-3">
                                {[...Array(5)].map((_, i) => (
                                    <div key={i} className="h-14 bg-zinc-100 dark:bg-zinc-800 rounded-lg animate-pulse" />
                                ))}
                            </div>
                        ) : (
                            <AnimatePresence mode="wait">
                                {security.activeTab === 'blacklist' && (
                                    <BlacklistTab
                                        blacklist={security.blacklist}
                                        onRemove={security.handleRemoveFromBlacklist}
                                        formatExpiresAt={security.formatExpiresAt}
                                    />
                                )}

                                {security.activeTab === 'whitelist' && (
                                    <WhitelistTab
                                        whitelist={security.whitelist}
                                        onRemove={security.handleRemoveFromWhitelist}
                                        formatTimestamp={security.formatTimestamp}
                                    />
                                )}

                                {security.activeTab === 'logs' && (
                                    <LogsTab
                                        accessLogs={security.accessLogs}
                                        formatTimestamp={security.formatTimestamp}
                                    />
                                )}

                                {security.activeTab === 'settings' && security.config && (
                                    <SettingsTab
                                        config={security.config}
                                        onSaveConfig={security.handleSaveConfig}
                                    />
                                )}
                            </AnimatePresence>
                        )}
                    </div>
                </div>
            </div>

            {/* Add IP Dialog */}
            <AddIpDialog
                isOpen={security.isAddDialogOpen}
                type={security.activeTab === 'whitelist' ? 'whitelist' : 'blacklist'}
                onClose={() => security.setIsAddDialogOpen(false)}
                onSubmit={security.activeTab === 'whitelist' ? security.handleAddToWhitelist : security.handleAddToBlacklist}
                isSubmitting={security.isSubmitting}
            />
        </div>
    );
});

export default SecurityPage;
