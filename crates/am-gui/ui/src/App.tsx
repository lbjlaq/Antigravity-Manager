import { useState, useEffect } from 'react';
import { flushSync } from 'react-dom';
import { Navbar } from './components/navbar/Navbar';
import { TabId } from './components/navbar/NavMenu';
import { Dashboard } from './pages/Dashboard';
import { Accounts } from './pages/Accounts';

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

  // 稳定、100% 生效的圆心水波扩散动画 (对齐 minhvo / baiwumm 标准实现)
  const handleThemeToggle = async (event: React.MouseEvent<HTMLButtonElement>) => {
    const newTheme = theme === 'light' ? 'dark' : 'light';

    if (!document.startViewTransition) {
      setTheme(newTheme);
      return;
    }

    // 1. 获取点击真实视口坐标
    const x = event.clientX;
    const y = event.clientY;

    // 2. 计算覆盖全屏最大外接圆半径
    const endRadius = Math.hypot(
      Math.max(x, window.innerWidth - x),
      Math.max(y, window.innerHeight - y)
    );

    // 3. 强制 React 18 同步提交 DOM 翻转并挂载动画
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

    await transition.ready;

    // 4. 驱动新视图从点击圆心以平滑缓动向全屏展开
    // 时长 550ms：既清晰可见水波扩散，又完全不拖泥带水
    document.documentElement.animate(
      {
        clipPath: [
          `circle(0px at ${x}px ${y}px)`,
          `circle(${endRadius}px at ${x}px ${y}px)`,
        ],
      },
      {
        duration: 550,
        easing: 'cubic-bezier(0.25, 1, 0.5, 1)',
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

      {/* 菜单栏下方内容画布 */}
      <main className="flex-1 overflow-y-auto px-4 py-2 relative">
        {currentTab === 'dashboard' ? (
          <Dashboard lang={lang} />
        ) : currentTab === 'accounts' ? (
          <Accounts lang={lang} />
        ) : (
          <div className="w-full h-full min-h-[500px] flex flex-col items-center justify-center">
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
          </div>
        )}
      </main>
    </div>
  );
}

export default App;
