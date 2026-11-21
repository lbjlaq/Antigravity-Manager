import React from 'react';
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
    <nav className="flex items-center gap-1 bg-gray-100 dark:bg-[#15161b] rounded-full p-1 select-none">
      {tabs.map((tab) => {
        const isActive = currentTab === tab.id;
        const Icon = tab.icon;

        return (
          <button
            key={tab.id}
            onClick={() => onTabChange(tab.id)}
            className={`
              px-5 py-2 
              rounded-full 
              text-sm 
              font-medium 
              transition-all duration-200
              whitespace-nowrap
              inline-flex
              items-center
              gap-2
              ${isActive
                ? 'bg-gray-900 text-white shadow-sm dark:bg-white dark:text-gray-900 font-semibold'
                : 'text-gray-700 hover:text-gray-900 hover:bg-gray-200 dark:text-gray-400 dark:hover:text-white dark:hover:bg-[#26272f]'
              }
            `}
          >
            <Icon className="w-4 h-4" />
            <span>{tab.label}</span>
          </button>
        );
      })}
    </nav>
  );
};
