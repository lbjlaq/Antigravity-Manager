import { useState, useEffect } from 'react';
import { flushSync } from 'react-dom';
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

  // 对齐 baiwumm / minhvo 的标准圆心水波纹 View Transition 扩散算法
  const handleThemeToggle = async (event: React.MouseEvent<HTMLButtonElement>) => {
    const newTheme = theme === 'light' ? 'dark' : 'light';

    // 降级兼容：不支持 View Transition 则直接切换
    if (!document.startViewTransition) {
      setTheme(newTheme);
      return;
    }

    // 1. 获取点击物理坐标作为圆心
    const x = event.clientX;
    const y = event.clientY;

    // 2. 计算覆盖全屏所需的最大外接圆半径
    const endRadius = Math.hypot(
      Math.max(x, window.innerWidth - x),
      Math.max(y, window.innerHeight - y)
    );

    // 3. 必须通过 flushSync 强制 React 18 同步提交 DOM 变更
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

    // 4. 等待视图过渡就绪
    await transition.ready;

    // 5. 驱动新视图从圆心平滑展开
    // 调优扩散节奏：时长从 450ms 调整至 650ms，采用柔和缓动曲线
    const clipPath = [
      `circle(0px at ${x}px ${y}px)`,
      `circle(${endRadius}px at ${x}px ${y}px)`,
    ];

    document.documentElement.animate(
      {
        clipPath: clipPath,
      },
      {
        duration: 650,
        easing: 'cubic-bezier(0.2, 0.8, 0.25, 1)',
        pseudoElement: '::view-transition-new(root)',
      }
    );
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
