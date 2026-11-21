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
  onLangToggle: () => void;
}

export const Navbar: React.FC<NavbarProps> = ({
  currentTab,
  onTabChange,
  theme,
  onThemeToggle,
  lang,
  onLangToggle,
}) => {
  return (
    <header className="relative w-full h-14 px-3 flex items-center justify-between border-b border-slate-200/80 dark:border-zinc-800/80 bg-white/70 dark:bg-[#0B0C0E]/70 backdrop-blur-xl z-50">
      {/* 顶部防失效系统级拖拽条 (Window Drag Strip) */}
      <div
        className="absolute top-0 left-0 right-0 h-9 z-[-1]"
        style={{
          cursor: 'default',
          userSelect: 'none',
        }}
        data-tauri-drag-region
      />

      {/* 左侧：Logo 拟物标识与状态 */}
      <NavLogo />

      {/* 中间：居中滑动胶囊菜单栏 (4大核心模块) */}
      <NavMenu currentTab={currentTab} onTabChange={onTabChange} lang={lang} />

      {/* 右侧：多语言与暗黑主题切换动作群 */}
      <NavActions
        theme={theme}
        onThemeToggle={onThemeToggle}
        lang={lang}
        onLangToggle={onLangToggle}
      />
    </header>
  );
};
