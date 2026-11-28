import { useState, useEffect, useMemo, useRef, useCallback, lazy, Suspense } from 'react';
import { flushSync } from 'react-dom';
import { Navbar } from './components/navbar/Navbar';
import { TabId } from './components/navbar/NavMenu';
import { LangCode } from './i18n/constants';
import { ensureLocaleLoaded } from './i18n';
import { api, isTauri, UpdateSnapshot } from './api';
import { UpdatePopover } from './components/UpdatePopover';
import { ZoomManager } from './components/common/ZoomManager';
import {
  VALID_TABS,
  consumeFreshPendingTab,
  readStoredLastTab,
  saveLastTab,
  hydrateUiStateFromConfig,
} from './utils/uiState';

// 按窗口/路由懒加载重型页面，确保 mini 悬浮窗不解析 Stats/Accounts/Settings 等大模块
const Dashboard = lazy(() => import('./pages/Dashboard').then((m) => ({ default: m.Dashboard })));
const Accounts = lazy(() => import('./pages/Accounts').then((m) => ({ default: m.Accounts })));
const Stats = lazy(() => import('./pages/Stats').then((m) => ({ default: m.Stats })));
const Toolbox = lazy(() => import('./pages/Toolbox').then((m) => ({ default: m.Toolbox })));
const Settings = lazy(() => import('./pages/Settings').then((m) => ({ default: m.Settings })));
const MiniPopover = lazy(() =>
  import('./components/mini/MiniPopover').then((m) => ({ default: m.MiniPopover }))
);

const isMiniWindow =
  typeof window !== 'undefined' && new URLSearchParams(window.location.search).get('mini') === '1';

function detectInitialTheme(): 'light' | 'dark' {
  if (typeof window === 'undefined') return 'light';
  const saved = localStorage.getItem('am_theme');
  if (saved === 'light' || saved === 'dark') return saved;
  return window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

function resolveInitialTab(): { tab: TabId; resolvedFromLocal: boolean } {
  if (typeof window !== 'undefined' && !isMiniWindow) {
    const pending = consumeFreshPendingTab();
    if (pending) {
      return { tab: pending, resolvedFromLocal: true };
    }
    const last = readStoredLastTab();
    if (last) {
      return { tab: last, resolvedFromLocal: true };
    }
  }
  return { tab: 'dashboard', resolvedFromLocal: false };
}

export function App() {
  const initialTabRef = useRef<{ tab: TabId; resolvedFromLocal: boolean } | null>(null);
  if (initialTabRef.current === null) {
    initialTabRef.current = resolveInitialTab();
  }
  const [currentTab, setCurrentTab] = useState<TabId>(initialTabRef.current.tab);
  const userChangedTabRef = useRef(false);
  const handleTabChange = useCallback((tab: TabId) => {
    userChangedTabRef.current = true;
    setCurrentTab(tab);
    saveLastTab(tab);
  }, []);
  const [theme, setTheme] = useState<'light' | 'dark'>(detectInitialTheme);
  const themeRef = useRef<'light' | 'dark'>(theme);
  themeRef.current = theme;
  const isThemeTransitioningRef = useRef(false);
  const [lang, setLang] = useState<LangCode>(() => {
    return (localStorage.getItem('am_language') as LangCode) || 'zh';
  });
  const [localeVersion, setLocaleVersion] = useState(0);
  const [updateState, setUpdateState] = useState<UpdateSnapshot | null>(null);
  const [updatePopoverOpen, setUpdatePopoverOpen] = useState(false);

  const handleLanguageChange = useCallback((newLang: LangCode) => {
    setLang(newLang);
    localStorage.setItem('am_language', newLang);
    void api.syncUiPreferences({ lang: newLang });
  }, []);

  const syncUpdateSnapshot = useCallback((snap: UpdateSnapshot) => {
    setUpdateState(snap);
    if (
      snap.info?.has_update &&
      !snap.banner_dismissed &&
      ((snap.phase === 'available' && snap.triggered_by_auto) || snap.phase === 'ready')
    ) {
      setUpdatePopoverOpen(true);
    }
    if (snap.phase === 'idle' || snap.phase === 'up_to_date') {
      setUpdatePopoverOpen(false);
    }
  }, []);

  const handleUpdateDownload = useCallback((forceNative = false) => {
    setUpdatePopoverOpen(true);
    void api.downloadUpdate(forceNative).catch(() => {
      // 错误详情已由 Rust UpdateController 通过 updater://state 广播并展示
    });
  }, []);

  const handleUpdateRestart = useCallback(() => {
    void api.applyAndRestartUpdate().catch(() => {
      // 错误详情已由 Rust UpdateController 广播
    });
  }, []);

  const handleUpdateClose = useCallback(() => {
    setUpdatePopoverOpen(false);
    void api.dismissUpdateBanner().then((snap) => {
      if (snap) setUpdateState(snap);
    });
  }, []);

  const handleUpdateSkip = useCallback((version: string) => {
    setUpdatePopoverOpen(false);
    void api.skipUpdateVersion(version).then((snap) => {
      if (snap) setUpdateState(snap);
    });
  }, []);

  // 启动时将当前前端语言同步给后端系统原生托盘菜单，并监听跨窗口 Tauri 广播与系统深浅色变化
  useEffect(() => {
    void api.syncUiPreferences({ lang });

    const onStorage = (e: StorageEvent) => {
      if (
        e.key === 'am_theme' &&
        (e.newValue === 'light' || e.newValue === 'dark') &&
        e.newValue !== themeRef.current
      ) {
        setTheme(e.newValue);
      }
      if (e.key === 'am_language' && e.newValue) {
        setLang(e.newValue as LangCode);
      }
    };
    window.addEventListener('storage', onStorage);

    const media = window.matchMedia?.('(prefers-color-scheme: dark)');
    const onMediaChange = (e: MediaQueryListEvent) => {
      if (!localStorage.getItem('am_theme_manual')) {
        const next = e.matches ? 'dark' : 'light';
        setTheme(next);
        localStorage.setItem('am_theme', next);
      }
    };
    media?.addEventListener?.('change', onMediaChange);

    let disposed = false;
    let unlistenPrefs: (() => void) | undefined;
    let unlistenNav: (() => void) | undefined;
    let unlistenUpdate: (() => void) | undefined;
    if (isTauri()) {
      import('@tauri-apps/api/event').then(async ({ listen }) => {
        const offPrefs = await listen<{ lang?: string; theme?: string }>('tray://prefs-synced', (evt) => {
          if (evt.payload?.lang) {
            setLang(evt.payload.lang as LangCode);
          }
          // 过滤自身发起的主题广播回声，避免打断正在执行的 View Transition
          if (
            (evt.payload?.theme === 'light' || evt.payload?.theme === 'dark') &&
            evt.payload.theme !== themeRef.current
          ) {
            setTheme(evt.payload.theme);
          }
        });
        if (disposed) {
          offPrefs();
          return;
        }
        unlistenPrefs = offPrefs;

        if (!isMiniWindow) {
          const offNav = await listen<string>('tray://navigate-tab', (evt) => {
            const target = evt.payload as TabId;
            if (VALID_TABS.includes(target)) {
              userChangedTabRef.current = true;
              setCurrentTab(target);
              saveLastTab(target);
              try {
                localStorage.removeItem('am_pending_tab');
              } catch {}
            }
          });
          if (disposed) {
            offNav();
            return;
          }
          unlistenNav = offNav;

          const offUpdate = await listen<UpdateSnapshot>('updater://state', (evt) => {
            if (!disposed && evt.payload) {
              syncUpdateSnapshot(evt.payload);
            }
          });
          if (disposed) {
            offUpdate();
            return;
          }
          unlistenUpdate = offUpdate;
        }
      });
    }

    if (!isMiniWindow) {
      void api.getUpdateState().then((snap) => {
        if (!disposed) {
          syncUpdateSnapshot(snap);
        }
      }).catch(() => {});

      void hydrateUiStateFromConfig().then((hydrated) => {
        if (
          !disposed &&
          hydrated?.lastTab &&
          !initialTabRef.current?.resolvedFromLocal &&
          !userChangedTabRef.current
        ) {
          setCurrentTab(hydrated.lastTab);
        }
      });
    }

    const syncVisibility = () => {
      document.documentElement.classList.toggle('app-hidden', document.hidden);
    };
    syncVisibility();
    document.addEventListener('visibilitychange', syncVisibility);

    return () => {
      disposed = true;
      document.removeEventListener('visibilitychange', syncVisibility);
      window.removeEventListener('storage', onStorage);
      media?.removeEventListener?.('change', onMediaChange);
      unlistenPrefs?.();
      unlistenNav?.();
      unlistenUpdate?.();
    };
  }, [syncUpdateSnapshot]);

  useEffect(() => {
    const root = document.documentElement;
    if (theme === 'dark') {
      root.classList.add('dark');
    } else {
      root.classList.remove('dark');
    }
    localStorage.setItem('am_theme', theme);
  }, [theme]);

  useEffect(() => {
    document.documentElement.dir = lang === 'ar' ? 'rtl' : 'ltr';
    let cancelled = false;
    void ensureLocaleLoaded(lang).then(() => {
      if (!cancelled) setLocaleVersion((v) => v + 1);
    });
    return () => {
      cancelled = true;
    };
  }, [lang]);

  const handleMiniThemeToggle = () => {
    const newTheme = theme === 'light' ? 'dark' : 'light';
    localStorage.setItem('am_theme_manual', '1');
    localStorage.setItem('am_theme', newTheme);
    setTheme(newTheme);
    void api.syncUiPreferences({ theme: newTheme });
  };

  // 120Hz 零卡顿双向圆形水波切换：
  // 1. 切换前冻结全页子元素 CSS transition，杜绝新快照捕获到半暗半明中间态
  // 2. 浅→深：深色新画布从点击圆心展开；深→浅：深色旧幕布向太阳图标优雅收拢
  // 3. 同步 IPC 延后至动画提交之后，彻底避免广播回声与主线程竞争
  const handleThemeToggle = useCallback(async (event: React.MouseEvent<HTMLButtonElement>) => {
    if (isThemeTransitioningRef.current) return;
    const prevTheme = themeRef.current;
    const newTheme = prevTheme === 'light' ? 'dark' : 'light';
    const isToLight = newTheme === 'light';
    localStorage.setItem('am_theme_manual', '1');
    localStorage.setItem('am_theme', newTheme);

    const root = document.documentElement;
    const prefersReducedMotion = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;

    if (!document.startViewTransition || prefersReducedMotion) {
      root.classList.add('theme-switching');
      if (newTheme === 'dark') {
        root.classList.add('dark');
      } else {
        root.classList.remove('dark');
      }
      setTheme(newTheme);
      requestAnimationFrame(() => {
        root.classList.remove('theme-switching');
      });
      void api.syncUiPreferences({ theme: newTheme });
      return;
    }

    isThemeTransitioningRef.current = true;
    const x = event.clientX;
    const y = event.clientY;
    const endRadius = Math.hypot(
      Math.max(x, window.innerWidth - x),
      Math.max(y, window.innerHeight - y)
    );

    // 冻结子元素 transition，确保 ::view-transition-new(root) 100% 捕获最终色值
    root.classList.add('theme-switching');
    if (isToLight) {
      root.classList.add('theme-to-light');
    } else {
      root.classList.remove('theme-to-light');
    }

    try {
      const transition = document.startViewTransition(() => {
        flushSync(() => {
          themeRef.current = newTheme;
          if (newTheme === 'dark') {
            root.classList.add('dark');
          } else {
            root.classList.remove('dark');
          }
          setTheme(newTheme);
        });
      });

      await transition.ready;

      const clipFrames = isToLight
        ? [`circle(${endRadius}px at ${x}px ${y}px)`, `circle(0px at ${x}px ${y}px)`]
        : [`circle(0px at ${x}px ${y}px)`, `circle(${endRadius}px at ${x}px ${y}px)`];

      const anim = root.animate(
        { clipPath: clipFrames },
        {
          duration: 320,
          easing: 'cubic-bezier(0.22, 1, 0.36, 1)',
          fill: 'both',
          pseudoElement: isToLight ? '::view-transition-old(root)' : '::view-transition-new(root)',
        }
      );

      await Promise.allSettled([transition.finished, anim.finished]);
    } catch {
      // 极端情况回退：确保 DOM 主题状态一致
      if (newTheme === 'dark') {
        root.classList.add('dark');
      } else {
        root.classList.remove('dark');
      }
      setTheme(newTheme);
    } finally {
      requestAnimationFrame(() => {
        root.classList.remove('theme-switching', 'theme-to-light');
      });
      isThemeTransitioningRef.current = false;
      void api.syncUiPreferences({ theme: newTheme });
    }
  }, []);

  // 隔离主题状态与重型页面树：切换深浅色时仅刷新 Navbar 图标，页面内容通过 CSS .dark 零开销响应，跳过 React 重渲染
  const mainPageContent = useMemo(() => {
    if (currentTab === 'dashboard') return <Dashboard lang={lang} />;
    if (currentTab === 'accounts') return <Accounts lang={lang} />;
    if (currentTab === 'stats') return <Stats lang={lang} />;
    if (currentTab === 'toolbox') return <Toolbox lang={lang} />;
    return <Settings lang={lang} />;
  }, [currentTab, lang, localeVersion]);

  if (isMiniWindow) {
    return (
      <Suspense fallback={null}>
        <MiniPopover lang={lang} theme={theme} onToggleTheme={handleMiniThemeToggle} />
      </Suspense>
    );
  }

  return (
    <div className="w-full h-screen flex flex-col bg-[#F3F4F6] dark:bg-[#09090b] text-slate-900 dark:text-zinc-100">
      <ZoomManager />
      {/* 顶部主分支风格 Navbar (带 pt-9 与 macOS 交通灯留白) */}
      <Navbar
        currentTab={currentTab}
        onTabChange={handleTabChange}
        theme={theme}
        onThemeToggle={handleThemeToggle}
        lang={lang}
        onLanguageChange={handleLanguageChange}
        updateState={updateState}
        onUpdatePillClick={() => setUpdatePopoverOpen((v) => !v)}
      />

      {updateState && (
        <UpdatePopover
          lang={lang}
          state={updateState}
          open={updatePopoverOpen}
          onClose={handleUpdateClose}
          onDownload={handleUpdateDownload}
          onRestart={handleUpdateRestart}
          onSkip={handleUpdateSkip}
        />
      )}

      {/* 菜单栏下方内容画布 */}
      <main className="app-scroll flex-1 overflow-y-auto px-4 py-2 relative">
        <Suspense fallback={null}>{mainPageContent}</Suspense>
      </main>
    </div>
  );
}

export default App;
