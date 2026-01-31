// File: src/pages/settings/ui/SettingsPage.tsx
import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { motion, AnimatePresence } from 'framer-motion';
import { Save } from 'lucide-react';

import { Button } from '@/shared/ui';
import { SECTIONS } from '../lib';
import { useSettings } from '../model';
import { SidebarItem } from './SidebarItem';
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
  const { t } = useTranslation();
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

  const currentSection = SECTIONS.find(s => s.id === activeTab);

  return (
    <div className="h-full flex flex-col p-5 gap-4 max-w-7xl mx-auto w-full">
      <div className="flex-1 h-full min-h-0 relative flex flex-col">
        <div className="h-full bg-white dark:bg-zinc-900/40 backdrop-blur-xl rounded-2xl border border-zinc-200 dark:border-white/5 flex overflow-hidden shadow-2xl">

          {/* Sidebar */}
          <aside className="w-64 flex-shrink-0 border-r border-zinc-200 dark:border-white/5 bg-zinc-50/50 dark:bg-black/20 flex flex-col">
            <div className="p-6">
              <h2 className="px-4 text-xs font-bold text-zinc-400 uppercase tracking-widest mb-4">
                {t('settings.configuration', 'SYSTEM CONFIG')}
              </h2>
              <div className="space-y-1">
                {SECTIONS.map((section) => (
                  <SidebarItem
                    key={section.id}
                    active={activeTab === section.id}
                    icon={section.icon}
                    label={t(section.label)}
                    onClick={() => setActiveTab(section.id)}
                  />
                ))}
              </div>
            </div>
          </aside>

          {/* Main Content */}
          <main className="flex-1 h-full overflow-hidden relative flex flex-col bg-white/50 dark:bg-transparent">
            {/* Header */}
            <header className="flex-shrink-0 px-8 py-8 border-b border-zinc-200 dark:border-white/5 flex items-center justify-between bg-white/50 dark:bg-transparent backdrop-blur-sm sticky top-0 z-10">
              <div>
                <motion.h2
                  key={activeTab}
                  initial={{ opacity: 0, x: -10 }}
                  animate={{ opacity: 1, x: 0 }}
                  className="text-2xl font-bold text-zinc-900 dark:text-white tracking-tight"
                >
                  {t(currentSection?.label || 'Settings')}
                </motion.h2>
                <p className="text-zinc-500 text-sm mt-1">
                  {currentSection?.desc}
                </p>
              </div>
              <Button
                onClick={handleSave}
                className="group relative px-5 py-2.5 rounded-xl bg-indigo-500 hover:bg-indigo-600 active:scale-95 transition-all text-white font-medium shadow-[0_0_20px_rgba(99,102,241,0.3)] hover:shadow-[0_0_30px_rgba(99,102,241,0.5)] overflow-hidden border-none"
              >
                <div className="absolute inset-0 bg-white/20 translate-y-full group-hover:translate-y-0 transition-transform duration-300" />
                <div className="relative flex items-center gap-2">
                  <Save className="h-4 w-4" />
                  <span>{t('settings.save')}</span>
                </div>
              </Button>
            </header>

            {/* Scrollable Content */}
            <div className="flex-1 overflow-y-auto p-8 relative z-10 custom-scrollbar">
              <div className="max-w-4xl mx-auto pb-20">
                <AnimatePresence mode="wait">
                  <motion.div
                    key={activeTab}
                    initial={{ opacity: 0, x: 20 }}
                    animate={{ opacity: 1, x: 0 }}
                    exit={{ opacity: 0, x: -20 }}
                    transition={{ duration: 0.2 }}
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
          </main>
        </div>
      </div>
    </div>
  );
});

export default SettingsPage;
