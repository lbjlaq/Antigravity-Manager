import React, { useRef, useState, useEffect, useCallback } from 'react';
import { LayoutDashboard, Users, BarChart3, Settings } from 'lucide-react';
import { tUi } from '../../i18n/ui';

export type TabId = 'dashboard' | 'accounts' | 'stats' | 'settings';

interface NavMenuProps {
  currentTab: TabId;
  onTabChange: (tab: TabId) => void;
  lang: string;
}

export const NavMenu: React.FC<NavMenuProps> = ({ currentTab, onTabChange, lang }) => {
  const tabs = [
    {
      id: 'dashboard' as TabId,
      label: tUi(lang, 'nav_dashboard'),
      icon: LayoutDashboard,
    },
    {
      id: 'accounts' as TabId,
      label: tUi(lang, 'nav_accounts'),
      icon: Users,
    },
    {
      id: 'stats' as TabId,
      label: tUi(lang, 'nav_stats'),
      icon: BarChart3,
    },
    {
      id: 'settings' as TabId,
      label: tUi(lang, 'nav_settings'),
      icon: Settings,
    },
  ];

  const containerRef = useRef<HTMLDivElement>(null);
  const [indicatorStyle, setIndicatorStyle] = useState<{ left: number; width: number }>({
    left: 0,
    width: 0,
  });

  const activeIndex = tabs.findIndex((t) => t.id === currentTab);

  // 毫秒级计算当前激活项的几何位置，驱动滑动滑块平移（并响应窗口尺寸变化）
  const updateIndicator = useCallback(() => {
    if (!containerRef.current) return;
    const activeElement = containerRef.current.children[activeIndex + 1] as HTMLElement | undefined;
    if (activeElement) {
      setIndicatorStyle({
        left: activeElement.offsetLeft,
        width: activeElement.offsetWidth,
      });
    }
  }, [activeIndex]);

  useEffect(() => {
    updateIndicator();
    const container = containerRef.current;
    if (!container || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(updateIndicator);
    observer.observe(container);
    return () => observer.disconnect();
  }, [updateIndicator, lang]);

  return (
    <nav
      ref={containerRef}
      className="relative flex items-center bg-slate-200/75 dark:bg-[#16181e] rounded-full p-1 select-none border border-slate-200/80 dark:border-white/[0.09]"
    >
      {/* 纯 CSS 硬件加速智能滑动药丸底盘 (Apple 原生分段控制器手感，深色下柔和浮起不刺眼) */}
      <div
        className="absolute left-0 top-1 bottom-1 bg-white dark:bg-white/[0.14] rounded-full shadow-[0_1px_3px_rgba(15,23,42,0.10)] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.14),0_1px_3px_rgba(0,0,0,0.35)] transition-all duration-300 ease-[cubic-bezier(0.2,0.8,0.25,1)]"
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
              px-3.5 xl:px-4.5 py-1.5
              rounded-full 
              text-xs xl:text-[13px]
              font-medium 
              whitespace-nowrap
              inline-flex
              items-center
              gap-1.5
              transition-colors duration-200
              ${isActive
                ? 'text-slate-900 dark:text-white font-semibold'
                : 'text-slate-600 hover:text-slate-900 dark:text-zinc-400 dark:hover:text-zinc-100'
              }
            `}
          >
            <Icon className="w-3.5 h-3.5 shrink-0" />
            <span className="truncate">{tab.label}</span>
          </button>
        );
      })}
    </nav>
  );
};
