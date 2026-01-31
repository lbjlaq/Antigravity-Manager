// File: src/pages/security/ui/SecurityPage.tsx
// Main Security page component

import { AnimatePresence } from 'framer-motion';
import { useSecurity } from '../model';
import { SecurityHeader } from './SecurityHeader';
import { SecurityTabs } from './SecurityTabs';
import { SecurityToolbar } from './SecurityToolbar';
import { BlacklistTab } from './BlacklistTab';
import { WhitelistTab } from './WhitelistTab';
import { LogsTab } from './LogsTab';
import { SettingsTab } from './SettingsTab';
import { AddIpDialog } from '@/components/security/AddIpDialog';

export function SecurityPage() {
    const security = useSecurity();

    return (
        <div className="h-full flex flex-col p-5 gap-4 max-w-5xl mx-auto w-full overflow-y-auto">
            {/* Header Card */}
            <div className="bg-white dark:bg-zinc-900 rounded-2xl border border-gray-200 dark:border-zinc-800 p-5 shadow-sm">
                <SecurityHeader stats={security.stats} />
                <SecurityTabs
                    activeTab={security.activeTab}
                    stats={security.stats}
                    onTabChange={security.setActiveTab}
                />
            </div>

            {/* Content Card */}
            <div className="flex-1 bg-white dark:bg-zinc-900 rounded-2xl border border-gray-200 dark:border-zinc-800 shadow-sm overflow-hidden flex flex-col">
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

                {/* Content */}
                <div className="flex-1 overflow-y-auto p-5">
                    {security.isLoading ? (
                        <div className="space-y-3">
                            {[...Array(3)].map((_, i) => (
                                <div key={i} className="h-16 bg-gray-100 dark:bg-zinc-800 rounded-xl animate-pulse" />
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
}
