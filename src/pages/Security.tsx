import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Shield, Lock, FileText, Settings, Activity, RefreshCw } from 'lucide-react';
import { motion } from 'framer-motion';
import { IpAccessLogs } from '../components/security/IpAccessLogs';
import { BlacklistManager } from '../components/security/BlacklistManager';
import { WhitelistManager } from '../components/security/WhitelistManager';
import { SecurityConfig } from '../components/security/SecurityConfig';
import { IpStatistics } from '../components/security/IpStatistics';

const Security: React.FC = () => {
    const { t } = useTranslation();
    const [activeTab, setActiveTab] = useState<'logs' | 'stats' | 'blacklist' | 'whitelist' | 'config'>('logs');
    const [refreshKey, setRefreshKey] = useState(0);

    const handleRefresh = () => {
        setRefreshKey(prev => prev + 1);
    };

    const renderContent = () => {
        switch (activeTab) {
            case 'logs':
                return <IpAccessLogs refreshKey={refreshKey} />;
            case 'stats':
                return <IpStatistics refreshKey={refreshKey} />;
            case 'blacklist':
                return <BlacklistManager refreshKey={refreshKey} />;
            case 'whitelist':
                return <WhitelistManager refreshKey={refreshKey} />;
            case 'config':
                return <SecurityConfig />;
            default:
                return <IpAccessLogs refreshKey={refreshKey} />;
        }
    };

    const tabs = [
        { id: 'logs', label: t('security.tab_logs'), icon: FileText },
        { id: 'stats', label: t('security.tab_stats'), icon: Activity },
        { id: 'blacklist', label: t('security.tab_blacklist'), icon: Shield },
        { id: 'whitelist', label: t('security.tab_whitelist'), icon: Lock },
        { id: 'config', label: t('security.tab_config'), icon: Settings },
    ];

    return (
        <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.2, ease: "easeOut" }}
            className="h-full flex flex-col p-5 gap-4 max-w-7xl mx-auto w-full"
        >
            <div className="flex items-center justify-between">
                <h1 className="text-2xl font-bold text-gray-900 dark:text-white flex items-center gap-2">
                    <Shield className="text-blue-500" />
                    {t('security.title')}
                </h1>
                {activeTab !== 'config' && (
                    <button
                        onClick={handleRefresh}
                        className="btn btn-sm btn-ghost gap-2 text-gray-600 dark:text-gray-400"
                        title={t('security.refresh_data')}
                    >
                        <RefreshCw size={16} />
                        {t('security.refresh')}
                    </button>
                )}
            </div>

            <div className="flex-1 overflow-hidden flex flex-col bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200 mt-2">
                <div role="tablist" className="flex border-b border-gray-100 dark:border-base-200 shrink-0">
                    {tabs.map((tab) => {
                        const isSelected = activeTab === tab.id;
                        return (
                            <button
                                key={tab.id}
                                type="button"
                                role="tab"
                                aria-selected={isSelected}
                                onClick={() => setActiveTab(tab.id as any)}
                                className={`flex items-center gap-2 px-6 py-4 text-sm font-medium transition-colors relative focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-blue-500 ${isSelected
                                    ? 'text-blue-600 dark:text-blue-400'
                                    : 'text-gray-600 hover:text-gray-800 dark:text-gray-400 dark:hover:text-gray-200'
                                    }`}
                            >
                                <tab.icon size={18} aria-hidden="true" />
                                {tab.label}
                                {isSelected && (
                                    <div className="absolute bottom-0 inset-x-0 h-0.5 bg-blue-600 dark:bg-blue-400" aria-hidden="true" />
                                )}
                            </button>
                        );
                    })}
                </div>
                <div className="flex-1 overflow-hidden flex flex-col">
                    {renderContent()}
                </div>
            </div>
        </motion.div>
    );
};

export default Security;
