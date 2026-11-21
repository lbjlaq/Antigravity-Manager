import React, { useState, useEffect } from 'react';
import { Navbar } from './components/navbar/Navbar';
import { TabId } from './components/navbar/NavMenu';

export function App() {
  const [currentTab, setCurrentTab] = useState<TabId>('dashboard');
  const [theme, setTheme] = useState<'light' | 'dark'>('light');
  const [lang, setLang] = useState<'zh' | 'en'>('zh');

  useEffect(() => {
    const root = document.documentElement;
    if (theme === 'dark') {
      root.classList.add('dark');
    } else {
      root.classList.remove('dark');
    }
  }, [theme]);

  const handleThemeToggle = (event: React.MouseEvent<HTMLButtonElement>) => {
    const newTheme = theme === 'light' ? 'dark' : 'light';

    if ('startViewTransition' in document) {
      const x = event.clientX;
      const y = event.clientY;
      const endRadius = Math.hypot(
        Math.max(x, window.innerWidth - x),
        Math.max(y, window.innerHeight - y)
      );

      // @ts-ignore
      const transition = document.startViewTransition(() => {
        setTheme(newTheme);
      });

      transition.ready.then(() => {
        const isDarkMode = newTheme === 'dark';
        const clipPath = isDarkMode
          ? [`circle(0px at ${x}px ${y}px)`, `circle(${endRadius}px at ${x}px ${y}px)`]
          : [`circle(${endRadius}px at ${x}px ${y}px)`, `circle(0px at ${x}px ${y}px)`];

        document.documentElement.animate(
          { clipPath },
          {
            duration: 450,
            easing: 'ease-in-out',
            fill: 'forwards',
            pseudoElement: isDarkMode ? '::view-transition-old(root)' : '::view-transition-new(root)',
          }
        );
      });
    } else {
      setTheme(newTheme);
    }
  };

  return (
    <div className="w-full h-screen flex flex-col bg-[#FAFBFC] dark:bg-[#09090b] text-slate-900 dark:text-zinc-100 transition-colors duration-200">
      {/* 顶部主分支风格 Navbar (带 pt-9 与 macOS 交通灯留白) */}
      <Navbar
        currentTab={currentTab}
        onTabChange={setCurrentTab}
        theme={theme}
        onThemeToggle={handleThemeToggle}
        lang={lang}
        onLanguageChange={setLang}
      />

      {/* 菜单栏下方内容画布 (预留空白) */}
      <main className="flex-1 flex flex-col items-center justify-center p-8 overflow-hidden relative">
        <div className="text-center space-y-2 select-none opacity-20 hover:opacity-50 transition-opacity">
          <p className="text-xs font-mono uppercase tracking-widest text-slate-400 dark:text-zinc-500">
            {lang === 'zh' ? '内容区域就绪 · 当前模块' : 'CANVAS READY · ACTIVE MODULE'}
          </p>
          <h2 className="text-2xl font-bold tracking-tight text-slate-700 dark:text-zinc-300">
            {currentTab.toUpperCase()}
          </h2>
        </div>
      </main>
    </div>
  );
}

export default App;
