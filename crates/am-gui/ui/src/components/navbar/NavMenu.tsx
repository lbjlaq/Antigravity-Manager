import React from 'react';
import { motion } from 'framer-motion';
import { LayoutDashboard, Users, BarChart3, Settings } from 'lucide-react';

export type TabId = 'dashboard' | 'accounts' | 'stats' | 'settings';

interface NavMenuProps {
  currentTab: TabId;
  onTabChange: (tab: TabId) => void;
  lang: 'zh' | 'en';
}

export const NavMenu: React.FC<NavMenuProps> = ({ currentTab, onTabChange, lang }) => {
  const tabs = [
    { id: 'dashboard' as TabId, label: lang === 'zh' ? '仪表盘' : 'Dashboard', icon: LayoutDashboard },
    { id: 'accounts' as TabId, label: lang === 'zh' ? '账号管理' : 'Accounts', icon: Users },
    { id: 'stats' as TabId, label: lang === 'zh' ? '统计分析' : 'Analytics', icon: BarChart3 },
    { id: 'settings' as TabId, label: lang === 'zh' ? '系统设置' : 'Settings', icon: Settings },
  ];

  return (
    <div className="flex items-center p-1 bg-slate-200/60 dark:bg-zinc-800/60 backdrop-blur-md rounded-full border border-slate-300/40 dark:border-zinc-700/50 shadow-inner select-none">
      {tabs.map((tab) => {
        const isActive = currentTab === tab.id;
        const Icon = tab.icon;

        return (
          <button
            key={tab.id}
            onClick={() => onTabChange(tab.id)}
            className={`relative flex items-center gap-1.5 px-3.5 py-1.5 text-xs font-medium rounded-full transition-colors duration-150 ${
              isActive
                ? 'text-slate-900 dark:text-white font-semibold'
                : 'text-slate-500 hover:text-slate-800 dark:text-zinc-400 dark:hover:text-zinc-200'
            }`}
          >
            {isActive && (
              <motion.div
                layoutId="nav-active-pill"
                className="absolute inset-0 bg-white dark:bg-zinc-700/90 rounded-full shadow-sm border border-black/[0.04] dark:border-white/[0.08]"
                transition={{ type: 'spring', stiffness: 450, damping: 35 }}
              />
            )}
            <Icon className="w-3.5 h-3.5 relative z-10" />
            <span className="relative z-10">{tab.label}</span>
          </button>
        );
      })}
    </div>
  );
};
