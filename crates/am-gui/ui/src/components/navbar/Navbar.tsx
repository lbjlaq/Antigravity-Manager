import React from 'react';
import { NavLogo } from './NavLogo';
import { NavMenu, TabId } from './NavMenu';
import { NavActions } from './NavActions';
import { LangCode } from '../../i18n/constants';
import { UpdateSnapshot } from '../../api';

interface NavbarProps {
  currentTab: TabId;
  onTabChange: (tab: TabId) => void;
  theme: 'light' | 'dark';
  onThemeToggle: (e: React.MouseEvent<HTMLButtonElement>) => void;
  lang: string;
  onLanguageChange: (lang: LangCode) => void;
  updateState?: UpdateSnapshot | null;
  onUpdatePillClick?: () => void;
}

export const Navbar: React.FC<NavbarProps> = ({
  currentTab,
  onTabChange,
  theme,
  onThemeToggle,
  lang,
  onLanguageChange,
  updateState,
  onUpdatePillClick,
}) => {
  const isMac =
    typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform || navigator.userAgent);

  return (
    <nav
      style={{ position: 'sticky', top: 0, zIndex: 50 }}
      className={`${isMac ? 'pt-7' : 'pt-2'} transition-colors duration-200 bg-[#F3F4F6] dark:bg-[#09090b] select-none`}
      data-tauri-drag-region
    >
      <div className="w-full max-w-[1400px] mx-auto px-4 relative" data-tauri-drag-region>
        <div className="flex items-center h-13 gap-3 justify-between" data-tauri-drag-region>
          {/* 左侧 Logo (弹性宽度，900px 窄窗下不挤压中间导航) */}
          <div className="flex-1 min-w-0 flex items-center" data-tauri-drag-region>
            <NavLogo />
          </div>

          {/* 中间 导航胶囊 */}
          <div className="shrink-0 flex justify-center" data-tauri-drag-region>
            <NavMenu currentTab={currentTab} onTabChange={onTabChange} lang={lang} />
          </div>

          {/* 右侧 语言与主题切换圆钮群 */}
          <div className="flex-1 min-w-0 flex justify-end" data-tauri-drag-region>
            <NavActions
              theme={theme}
              onThemeToggle={onThemeToggle}
              lang={lang}
              onLanguageChange={onLanguageChange}
              updateState={updateState}
              onUpdatePillClick={onUpdatePillClick}
            />
          </div>
        </div>
      </div>
    </nav>
  );
};
