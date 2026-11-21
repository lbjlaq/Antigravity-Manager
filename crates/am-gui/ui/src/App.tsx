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

  // 跨平台 0 延迟、真正的径向高斯模糊羽化光晕扩散 (GPU Fragment Shader 硬件加速)
  const handleThemeToggle = async (event: React.MouseEvent<HTMLButtonElement>) => {
    const newTheme = theme === 'light' ? 'dark' : 'light';

    // 降级兼容：不支持 View Transition 则直接切换
    if (!document.startViewTransition) {
      setTheme(newTheme);
      return;
    }

    // 1. 获取点击物理圆心
    const rect = event.currentTarget.getBoundingClientRect();
    const x = Math.round(rect.left + rect.width / 2);
    const y = Math.round(rect.top + rect.height / 2);

    // 2. 精确计算覆盖视口所需最大半径，加上 60px 羽化外延防止角落断层
    const maxRadius = Math.hypot(
      Math.max(x, window.innerWidth - x),
      Math.max(y, window.innerHeight - y)
    );
    const endRadius = Math.round(maxRadius + 80);

    const STYLE_ID = 'am-theme-feather-style';
    document.getElementById(STYLE_ID)?.remove();

    // 3. 注入静态蒙版盒子（100% 100%，盒子不移动、不缩放，杜绝 GPU 重绘）
    // 渐变通过 CSS 自定义属性 --am-theme-radius 进行插值，边缘 60px 物理柔和羽化
    const style = document.createElement('style');
    style.id = STYLE_ID;
    style.textContent = `
      ::view-transition-new(root) {
        -webkit-mask-image: radial-gradient(circle at ${x}px ${y}px, #000 calc(var(--am-theme-radius) - 60px), transparent var(--am-theme-radius));
        mask-image: radial-gradient(circle at ${x}px ${y}px, #000 calc(var(--am-theme-radius) - 60px), transparent var(--am-theme-radius));
        -webkit-mask-size: 100% 100%;
        mask-size: 100% 100%;
        -webkit-mask-repeat: no-repeat;
        mask-repeat: no-repeat;
      }
    `;
    document.head.appendChild(style);

    try {
      // 4. 同步提交 DOM 翻转
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

      // 5. 由 GPU 动画驱动半径属性展开（650ms 舒适节奏，配合 Apple 阻尼曲线）
      const animation = document.documentElement.animate(
        [
          { ['--am-theme-radius' as string]: '0px' },
          { ['--am-theme-radius' as string]: `${endRadius}px` },
        ],
        {
          duration: 650,
          easing: 'cubic-bezier(0.2, 0.8, 0.25, 1)',
          fill: 'forwards',
        }
      );

      await animation.finished;
    } finally {
      document.getElementById(STYLE_ID)?.remove();
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
