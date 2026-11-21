import React from 'react';
import { Sun, Moon, Languages } from 'lucide-react';

interface NavActionsProps {
  theme: 'light' | 'dark';
  onThemeToggle: (e: React.MouseEvent<HTMLButtonElement>) => void;
  lang: 'zh' | 'en';
  onLangToggle: () => void;
}

export const NavActions: React.FC<NavActionsProps> = ({
  theme,
  onThemeToggle,
  lang,
  onLangToggle,
}) => {
  return (
    <div className="flex items-center gap-1.5 pr-2 select-none">
      {/* 语言切换按钮 (中/英胶囊) */}
      <button
        onClick={onLangToggle}
        title={lang === 'zh' ? '切换为 English' : 'Switch to 中文'}
        className="flex items-center gap-1 px-2.5 py-1 text-xs font-medium rounded-lg text-slate-600 hover:text-slate-900 dark:text-zinc-400 dark:hover:text-zinc-100 hover:bg-slate-200/60 dark:hover:bg-zinc-800/60 border border-transparent hover:border-slate-300/40 dark:hover:border-zinc-700/50 transition-all duration-150"
      >
        <Languages className="w-3.5 h-3.5" />
        <span className="font-mono text-[11px]">{lang === 'zh' ? 'ZH' : 'EN'}</span>
      </button>

      {/* 主题切换按钮 (带圆圈光圈动画) */}
      <button
        onClick={onThemeToggle}
        title={theme === 'dark' ? '切换为浅色模式' : 'Switch to Dark Mode'}
        className="p-1.5 rounded-lg text-slate-600 hover:text-slate-900 dark:text-zinc-400 dark:hover:text-zinc-100 hover:bg-slate-200/60 dark:hover:bg-zinc-800/60 border border-transparent hover:border-slate-300/40 dark:hover:border-zinc-700/50 transition-all duration-150"
      >
        {theme === 'dark' ? (
          <Sun className="w-4 h-4 text-amber-400" />
        ) : (
          <Moon className="w-4 h-4 text-slate-600" />
        )}
      </button>
    </div>
  );
};
