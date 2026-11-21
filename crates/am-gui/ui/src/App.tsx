import { useState } from 'react';
import { useThemeAnimation, ThemeAnimationType } from 'theme-switch-animation/react';
import { Navbar } from './components/navbar/Navbar';
import { TabId } from './components/navbar/NavMenu';

export function App() {
  const [currentTab, setCurrentTab] = useState<TabId>('dashboard');
  const [lang, setLang] = useState<'zh' | 'en'>('zh');

  // 直接使用官方作者 baiwumm 原汁原味的 CIRCLE_BLUR 动画引擎
  // 持续时间设为标准 750ms，缓动 ease-in-out，具有真正的边缘高斯模糊羽化效果
  const { ref: themeButtonRef, toggleTheme, isDark } = useThemeAnimation<HTMLButtonElement>({
    animationType: ThemeAnimationType.CIRCLE_BLUR,
    duration: 750,
    easing: 'ease-in-out',
    blurAmount: 2,
  });

  const theme = isDark ? 'dark' : 'light';

  return (
    <div className="w-full h-screen flex flex-col bg-[#FAFBFC] dark:bg-[#09090b] text-slate-900 dark:text-zinc-100">
      {/* 顶部主分支风格 Navbar (带 pt-9 与 macOS 交通灯留白) */}
      <Navbar
        currentTab={currentTab}
        onTabChange={setCurrentTab}
        theme={theme}
        onThemeToggle={toggleTheme}
        themeButtonRef={themeButtonRef}
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
