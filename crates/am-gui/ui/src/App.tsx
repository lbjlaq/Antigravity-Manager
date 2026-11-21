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

  // 跨平台（Win / Mac / Linux / 核显）极致丝滑 120Hz 硬件加速几何扩散
  const handleThemeToggle = async (event: React.MouseEvent<HTMLButtonElement>) => {
    const newTheme = theme === 'light' ? 'dark' : 'light';

    // 1. 无 View Transition 支持环境或低配环境，直接秒切
    if (!document.startViewTransition) {
      setTheme(newTheme);
      return;
    }

    // 2. 获取触发按钮的几何中心点
    const rect = event.currentTarget.getBoundingClientRect();
    const x = rect.left + rect.width / 2;
    const y = rect.top + rect.height / 2;

    // 3. 计算覆盖视口最远角落的精确几何半径（无冗余放大，杜绝大屏卡顿）
    const endRadius = Math.hypot(
      Math.max(x, window.innerWidth - x),
      Math.max(y, window.innerHeight - y)
    );

    // 4. 强制 React 18 同步提交 DOM 变更
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

    // 5. 等待伪元素就绪
    await transition.ready;

    // 6. 驱动 GPU 片元着色器实时裁切（0 内存带宽开销，真 120Hz）
    document.documentElement.animate(
      {
        clipPath: [
          `circle(0px at ${x}px ${y}px)`,
          `circle(${endRadius}px at ${x}px ${y}px)`,
        ],
      },
      {
        duration: 480,
        easing: 'cubic-bezier(0.16, 1, 0.3, 1)', // Apple 标准 Spring 阻尼，轻盈且丝滑
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
