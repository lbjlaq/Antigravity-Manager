import React from 'react';
import { Sun, Moon, Languages, Check } from 'lucide-react';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';

interface NavActionsProps {
  theme: 'light' | 'dark';
  onThemeToggle: () => void;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  themeButtonRef?: any;
  lang: 'zh' | 'en';
  onLanguageChange: (lang: 'zh' | 'en') => void;
}

export const NavActions: React.FC<NavActionsProps> = ({
  theme,
  onThemeToggle,
  themeButtonRef,
  lang,
  onLanguageChange,
}) => {
  return (
    <div className="flex items-center gap-2 select-none">
      {/* 语言选择下拉菜单 (Radix UI) */}
      <DropdownMenu.Root>
        <DropdownMenu.Trigger asChild>
          <button
            className="w-10 h-10 rounded-full bg-gray-100 dark:bg-[#15161b] hover:bg-gray-200 dark:hover:bg-[#26272f] flex items-center justify-center transition-colors duration-150 border border-transparent dark:border-[#26272f]/40 outline-none"
            title={lang === 'zh' ? '选择语言' : 'Select Language'}
          >
            <Languages className="w-4 h-4 text-gray-700 dark:text-gray-300" />
          </button>
        </DropdownMenu.Trigger>

        <DropdownMenu.Portal>
          <DropdownMenu.Content
            align="end"
            sideOffset={8}
            className="w-36 py-1 bg-white dark:bg-[#15161b] border border-gray-200 dark:border-zinc-800 rounded-xl shadow-lg z-50 animate-in fade-in-0 zoom-in-95 duration-100 ease-out"
          >
            <DropdownMenu.Item
              onClick={() => onLanguageChange('zh')}
              className={`px-3 py-2 text-xs font-medium rounded-lg mx-1 flex items-center justify-between cursor-pointer outline-none transition-colors ${
                lang === 'zh'
                  ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 font-semibold'
                  : 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
              }`}
            >
              <span>简体中文</span>
              {lang === 'zh' && <Check className="w-3.5 h-3.5" />}
            </DropdownMenu.Item>

            <DropdownMenu.Item
              onClick={() => onLanguageChange('en')}
              className={`px-3 py-2 text-xs font-medium rounded-lg mx-1 flex items-center justify-between cursor-pointer outline-none transition-colors ${
                lang === 'en'
                  ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 font-semibold'
                  : 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
              }`}
            >
              <span>English</span>
              {lang === 'en' && <Check className="w-3.5 h-3.5" />}
            </DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>

      {/* 主题切换圆钮 (挂载官方 theme-switch-animation 触发引用) */}
      <button
        ref={themeButtonRef}
        onClick={onThemeToggle}
        className="w-10 h-10 rounded-full bg-gray-100 dark:bg-[#15161b] hover:bg-gray-200 dark:hover:bg-[#26272f] flex items-center justify-center transition-colors duration-150 border border-transparent dark:border-[#26272f]/40 outline-none"
        title={theme === 'light' ? '切换到暗色主题' : 'Switch to Light Theme'}
      >
        {theme === 'light' ? (
          <Moon className="w-5 h-5 text-gray-700 dark:text-gray-300" />
        ) : (
          <Sun className="w-5 h-5 text-amber-400" />
        )}
      </button>
    </div>
  );
};
