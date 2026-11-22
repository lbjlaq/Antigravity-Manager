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
      className="pt-7 transition-colors duration-200 bg-[#FAFBFC] dark:bg-[#09090b] select-none"
      data-tauri-drag-region
    >
      <div className="max-w-7xl mx-auto px-6 relative" data-tauri-drag-region>
        <div className="flex items-center h-14 gap-4 justify-between" data-tauri-drag-region>
          {/* 左侧 Logo (支持拖拽区域) */}
          <div className="basis-[220px] shrink-0" data-tauri-drag-region>
            <NavLogo />
          </div>

          {/* 中间 导航胶囊 */}
          <div className="flex-1 flex justify-center" data-tauri-drag-region>
            <NavMenu currentTab={currentTab} onTabChange={onTabChange} lang={lang} />
          </div>

          {/* 右侧 语言与主题切换圆钮群 */}
          <div className="basis-[220px] shrink-0 flex justify-end" data-tauri-drag-region>
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
