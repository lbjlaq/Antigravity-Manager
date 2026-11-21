import { useState, useEffect } from 'react';
import { flushSync } from 'react-dom';
import { Navbar } from './components/navbar/Navbar';
import { TabId } from './components/navbar/NavMenu';
import { injectBlurCircleStyle, removeThemeStyle } from './utils/themeAnimation';

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

  // 完全对齐 baiwumm/theme-switch-animation 的 CIRCLE_BLUR 圆形高斯模糊蒙版扩散实现
  const handleThemeToggle = async (event: React.MouseEvent<HTMLButtonElement>) => {
    const newTheme = theme === 'light' ? 'dark' : 'light';

    // 降级兼容：不支持 View Transition 则直接切换
    if (!document.startViewTransition) {
      setTheme(newTheme);
      return;
    }

    // 1. 获取点击物理坐标作为圆心
    const rect = event.currentTarget.getBoundingClientRect();
    const x = rect.left + rect.width / 2;
    const y = rect.top + rect.height / 2;

    const DURATION = 700;

    // 2. 注入带 feGaussianBlur 的 SVG Mask 转场动画样式
    injectBlurCircleStyle({ x, y }, DURATION, 2.4);

    try {
      // 3. 通过 flushSync 强制 React 18 同步提交 DOM 变更
      const transition = document.startViewTransition(() => {
        flushSync(() => {
          setTheme(newTheme);
          const root = document.documentElement;
          if (newTheme === 'dark') {
            root.classList.add('dark');
          } else {
            root.classList.remove('dark');
          }
        });
      });

      // 4. 等待动画自然完成，清理临时注入的 style 标签
      await transition.finished;
    } finally {
      removeThemeStyle();
    }
  };

  return (
    <div className="w-full h-screen flex flex-col bg-[#FAFBFC] dark:bg-[#09090b] text-slate-900 dark:text-zinc-100">
      {/* 顶部主分支风格 Navbar (带 pt-9 与 macOS 交通灯留白) */}
      <Navbar
        currentTab={currentTab}
        onTabChange={setCurrentTab}
        theme={theme}
        onThemeToggle={handleThemeToggle}
        lang={lang}
        onLanguageChange={setLang}
      />

      {/* 菜单栏下方内容画布 (预留空白，带 Tab 切换淡入平移动效) */}
      <main className="flex-1 flex flex-col items-center justify-center p-8 overflow-hidden relative">
        <div
          key={currentTab}
          className="text-center space-y-2 select-none opacity-20 hover:opacity-50 transition-opacity duration-150 animate-in fade-in zoom-in-95 duration-200"
        >
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
