import React from 'react';
import { NavLogo } from './NavLogo';
import { NavMenu, TabId } from './NavMenu';
import { NavActions } from './NavActions';

interface NavbarProps {
  currentTab: TabId;
  onTabChange: (tab: TabId) => void;
  theme: 'light' | 'dark';
  onThemeToggle: (e: React.MouseEvent<HTMLButtonElement>) => void;
  lang: 'zh' | 'en';
  onLanguageChange: (lang: 'zh' | 'en') => void;
}

export const Navbar: React.FC<NavbarProps> = ({
  currentTab,
  onTabChange,
  theme,
  onThemeToggle,
  lang,
  onLanguageChange,
}) => {
  return (
    <nav
      style={{ position: 'sticky', top: 0, zIndex: 50 }}
      className="pt-9 transition-all duration-200 bg-[#FAFBFC] dark:bg-[#09090b]"
    >
      {/* 顶部 Tauri 原生拖拽区域 (高为 top-9 到导航区域) */}
      <div
        className="absolute top-0 left-0 right-0 h-24"
        style={{ zIndex: 5, backgroundColor: 'rgba(0,0,0,0.001)', cursor: 'default' }}
        data-tauri-drag-region
      />

      <div className="max-w-7xl mx-auto px-8 relative" style={{ zIndex: 10 }}>
        <div className="flex items-center h-16 gap-4 justify-between">
          {/* 左侧 Logo */}
          <div className="basis-[220px] shrink-0">
            <NavLogo />
          </div>

          {/* 中间 导航胶囊 */}
          <div className="flex-1 flex justify-center">
            <NavMenu currentTab={currentTab} onTabChange={onTabChange} lang={lang} />
          </div>

          {/* 右侧 语言与主题切换圆钮群 */}
          <div className="basis-[220px] shrink-0 flex justify-end">
            <NavActions
              theme={theme}
              onThemeToggle={onThemeToggle}
              lang={lang}
              onLanguageChange={onLanguageChange}
            />
          </div>
        </div>
      </div>
    </nav>
  );
};
