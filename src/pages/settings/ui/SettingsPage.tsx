// File: src/pages/settings/ui/SettingsPage.tsx
// Settings page - redesigned to match Accounts/Security page style

import { memo } from 'react';
import { motion, AnimatePresence } from 'framer-motion';

import { useSettings } from '../model';
import { SettingsHeader } from './SettingsHeader';
import { SettingsTabs } from './SettingsTabs';
import {
  GeneralTab,
  AccountTab,
  ProxyTab,
  SecurityTab,
  PerformanceTab,
  AdvancedTab,
  AboutTab,
} from './tabs';

export const SettingsPage = memo(function SettingsPage() {
  const {
    activeTab,
    setActiveTab,
    appVersion,
    dataDirPath,
    formData,
    handleSave,
    handleLanguageChange,
    handleThemeChange,
    handleAutoLaunchChange,
    handleAutoCheckUpdateChange,
    updateFormData,
  } = useSettings();

  return (
    <div className="h-full flex flex-col p-5 gap-4 max-w-7xl mx-auto w-full">
      {/* Main Card - Single container like Accounts */}
      <div className="flex-1 min-h-0 relative flex flex-col">
        <div className="h-full bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 flex flex-col overflow-hidden">
          
          {/* Header */}
          <SettingsHeader onSave={handleSave} />

          {/* Tabs */}
          <SettingsTabs
            activeTab={activeTab}
            onTabChange={setActiveTab}
          />

          {/* Content Area */}
          <div className="flex-1 min-h-0 overflow-y-auto p-6">
            <div className="max-w-4xl mx-auto">
              <AnimatePresence mode="wait">
                <motion.div
                  key={activeTab}
                  initial={{ opacity: 0, y: 10 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -10 }}
                  transition={{ duration: 0.15 }}
                  className="space-y-6"
                >
                  {activeTab === 'general' && (
                    <GeneralTab
                      formData={formData}
                      onLanguageChange={handleLanguageChange}
                      onThemeChange={handleThemeChange}
                      onAutoLaunchChange={handleAutoLaunchChange}
                      onAutoCheckUpdateChange={handleAutoCheckUpdateChange}
                    />
                  )}

                  {activeTab === 'account' && (
                    <AccountTab formData={formData} onUpdate={updateFormData} />
                  )}

                  {activeTab === 'proxy' && (
                    <ProxyTab formData={formData} onUpdate={updateFormData} />
                  )}

                  {activeTab === 'security' && (
                    <SecurityTab formData={formData} onUpdate={updateFormData} />
                  )}

                  {activeTab === 'performance' && (
                    <PerformanceTab formData={formData} onUpdate={updateFormData} />
                  )}

                  {activeTab === 'advanced' && (
                    <AdvancedTab formData={formData} dataDirPath={dataDirPath} onUpdate={updateFormData} />
                  )}

                  {activeTab === 'about' && (
                    <AboutTab appVersion={appVersion} />
                  )}
                </motion.div>
              </AnimatePresence>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
});

export default SettingsPage;
