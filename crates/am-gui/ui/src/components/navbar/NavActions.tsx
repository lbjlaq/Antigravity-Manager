import React, { useState, useRef, useEffect } from 'react';
import { Sun, Moon, Languages } from 'lucide-react';

interface NavActionsProps {
  theme: 'light' | 'dark';
  onThemeToggle: (e: React.MouseEvent<HTMLButtonElement>) => void;
  lang: 'zh' | 'en';
  onLanguageChange: (lang: 'zh' | 'en') => void;
}

export const NavActions: React.FC<NavActionsProps> = ({
  theme,
  onThemeToggle,
  lang,
  onLanguageChange,
}) => {
  const [langOpen, setLangOpen] = useState(false);
  const langRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (langRef.current && !langRef.current.contains(event.target as Node)) {
        setLangOpen(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  return (
    <div className="flex items-center gap-2 select-none">
      {/* 语言选择下拉菜单 (对齐主分支圆钮) */}
      <div className="relative" ref={langRef}>
        <button
          onClick={() => setLangOpen(!langOpen)}
          className="w-10 h-10 rounded-full bg-gray-100 dark:bg-[#15161b] hover:bg-gray-200 dark:hover:bg-[#26272f] flex items-center justify-center transition-colors"
          title={lang === 'zh' ? '选择语言' : 'Select Language'}
        >
          <Languages className="w-5 h-5 text-gray-700 dark:text-gray-300" />
        </button>

        {langOpen && (
          <div className="absolute right-0 mt-2 w-32 py-1 bg-white dark:bg-[#15161b] border border-gray-200 dark:border-zinc-800 rounded-xl shadow-lg z-50">
            <button
              onClick={() => {
                onLanguageChange('zh');
                setLangOpen(false);
              }}
              className={`w-full px-4 py-2 text-left text-xs font-medium hover:bg-gray-100 dark:hover:bg-zinc-800 flex items-center justify-between ${
                lang === 'zh' ? 'text-blue-600 font-semibold' : 'text-gray-700 dark:text-gray-300'
              }`}
            >
              <span>简体中文</span>
              {lang === 'zh' && <span>✓</span>}
            </button>
            <button
              onClick={() => {
                onLanguageChange('en');
                setLangOpen(false);
              }}
              className={`w-full px-4 py-2 text-left text-xs font-medium hover:bg-gray-100 dark:hover:bg-zinc-800 flex items-center justify-between ${
                lang === 'en' ? 'text-blue-600 font-semibold' : 'text-gray-700 dark:text-gray-300'
              }`}
            >
              <span>English</span>
              {lang === 'en' && <span>✓</span>}
            </button>
          </div>
        )}
      </div>

      {/* 主题切换圆钮 (对齐主分支 w-10 h-10 rounded-full) */}
      <button
        onClick={onThemeToggle}
        className="w-10 h-10 rounded-full bg-gray-100 dark:bg-[#15161b] hover:bg-gray-200 dark:hover:bg-[#26272f] flex items-center justify-center transition-colors"
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
