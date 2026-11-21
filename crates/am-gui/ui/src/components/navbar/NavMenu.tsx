import React, { useRef, useState, useEffect } from 'react';
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

  const containerRef = useRef<HTMLDivElement>(null);
  const [indicatorStyle, setIndicatorStyle] = useState<{ left: number; width: number }>({
    left: 0,
    width: 0,
  });

  const activeIndex = tabs.findIndex((t) => t.id === currentTab);

  // 毫秒级计算当前激活项的几何位置，驱动滑动滑块平移
  useEffect(() => {
    if (containerRef.current) {
      const activeElement = containerRef.current.children[activeIndex + 1] as HTMLElement;
      if (activeElement) {
        setIndicatorStyle({
          left: activeElement.offsetLeft,
          width: activeElement.offsetWidth,
        });
      }
    }
  }, [activeIndex, lang]);

  return (
    <nav
      ref={containerRef}
      className="relative flex items-center bg-gray-100 dark:bg-[#15161b] rounded-full p-1 select-none border border-transparent dark:border-[#26272f]/50"
    >
      {/* 纯 CSS 硬件加速智能滑动药丸底盘 (带弹性缓动与精致微阴影) */}
      <div
        className="absolute top-1 bottom-1 bg-gray-900 dark:bg-white rounded-full shadow-[0_2px_8px_rgba(0,0,0,0.12)] dark:shadow-[0_2px_12px_rgba(255,255,255,0.18)] transition-all duration-300 ease-[cubic-bezier(0.2,0.8,0.25,1)]"
        style={{
          transform: `translate3d(${indicatorStyle.left}px, 0, 0)`,
          width: `${indicatorStyle.width}px`,
          willChange: 'transform, width',
        }}
      />

      {tabs.map((tab) => {
        const isActive = currentTab === tab.id;
        const Icon = tab.icon;

        return (
          <button
            key={tab.id}
            onClick={() => onTabChange(tab.id)}
            className={`
              relative z-10
              px-4 xl:px-5 py-2 
              rounded-full 
              text-xs xl:text-sm 
              font-medium 
              whitespace-nowrap
              inline-flex
              items-center
              gap-2
              transition-colors duration-200
              ${isActive
                ? 'text-white dark:text-gray-900 font-semibold'
                : 'text-gray-600 hover:text-gray-900 dark:text-gray-400 dark:hover:text-white'
              }
            `}
          >
            <Icon className="w-4 h-4 shrink-0" />
            <span className="truncate">{tab.label}</span>
          </button>
        );
      })}
    </nav>
  );
};
