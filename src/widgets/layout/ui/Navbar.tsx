// File: src/widgets/layout/ui/Navbar.tsx
// Main navigation bar widget

import { Link, useLocation, useNavigate } from 'react-router-dom';
import { motion } from 'framer-motion';
import { 
  LayoutDashboard, 
  Users, 
  Globe, 
  Activity, 
  Settings, 
  Sun, 
  Moon,
  Shield,
  MoreHorizontal
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useConfigStore } from '@/entities/config';
import { isLinux, cn } from '@/shared/lib';
import { memo, useCallback, useState, useEffect, useRef } from 'react';
import { getVersion } from '@tauri-apps/api/app';
import { DebugConsoleButton } from '@/widgets/debug-console';

const Navbar = function Navbar() {
  const location = useLocation();
  const navigate = useNavigate();
  const { t } = useTranslation();
  const { config, saveConfig } = useConfigStore();
  const [appVersion, setAppVersion] = useState<string>('');
  const [showMore, setShowMore] = useState(false);
  const moreRef = useRef<HTMLDivElement>(null);

  // Fetch app version from Tauri
  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => setAppVersion(''));
  }, []);

  // Close dropdown on outside click
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (moreRef.current && !moreRef.current.contains(e.target as Node)) {
        setShowMore(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  // Primary nav items (always visible)
  const primaryItems = [
    { path: '/', label: t('nav.dashboard'), icon: LayoutDashboard },
    { path: '/accounts', label: t('nav.accounts'), icon: Users },
    { path: '/api-proxy', label: t('nav.proxy'), icon: Globe },
    { path: '/monitor', label: t('nav.call_records'), icon: Activity },
  ];

  // Secondary nav items (in dropdown)
  const secondaryItems = [
    { path: '/security', label: t('nav.security', 'Security'), icon: Shield },
    { path: '/settings', label: t('nav.settings'), icon: Settings },
  ];

  const toggleTheme = useCallback(async (event: React.MouseEvent<HTMLButtonElement>) => {
    if (!config) return;

    const newTheme = config.theme === 'light' ? 'dark' : 'light';

    // View Transition API for smooth theme switch
    if ('startViewTransition' in document && !isLinux()) {
      const x = event.clientX;
      const y = event.clientY;
      const endRadius = Math.hypot(
        Math.max(x, window.innerWidth - x),
        Math.max(y, window.innerHeight - y)
      );

      // @ts-ignore View Transition API
      const transition = document.startViewTransition(async () => {
        await saveConfig({ ...config, theme: newTheme }, true);
      });

      transition.ready.then(() => {
        const isDarkMode = newTheme === 'dark';
        const clipPath = isDarkMode
          ? [`circle(${endRadius}px at ${x}px ${y}px)`, `circle(0px at ${x}px ${y}px)`]
          : [`circle(0px at ${x}px ${y}px)`, `circle(${endRadius}px at ${x}px ${y}px)`];

        document.documentElement.animate(
          { clipPath },
          {
            duration: 500,
            easing: 'ease-in-out',
            fill: 'forwards',
            pseudoElement: isDarkMode ? '::view-transition-old(root)' : '::view-transition-new(root)'
          }
        );
      });
    } else {
      await saveConfig({ ...config, theme: newTheme }, true);
    }
  }, [config, saveConfig]);

  const isActive = useCallback((path: string) => {
    if (path === '/') return location.pathname === '/';
    return location.pathname.startsWith(path);
  }, [location.pathname]);

  const isSecondaryActive = secondaryItems.some(item => isActive(item.path));

  return (
    <header 
      className="fixed top-0 left-0 right-0 z-[999] border-b border-zinc-200/60 dark:border-zinc-800/60 bg-white/80 dark:bg-zinc-900/80 backdrop-blur-xl"
      data-tauri-drag-region
    >
      <div className="max-w-7xl mx-auto px-4 h-16 flex items-center justify-between" data-tauri-drag-region>
        
        {/* LOGO */}
        <Link to="/" className="flex items-center gap-2.5 shrink-0" data-tauri-drag-region>
          <div className="h-8 w-8 rounded-lg bg-gradient-to-br from-indigo-500 to-purple-600 flex items-center justify-center shadow-lg shadow-indigo-500/25">
            <span className="font-bold text-white text-sm">A</span>
          </div>
          <div className="hidden sm:flex flex-col">
            <span className="text-sm font-bold text-zinc-900 dark:text-white leading-none">
              Antigravity
            </span>
            <span className="text-[10px] text-zinc-400 dark:text-zinc-500 font-mono">
              {appVersion ? `v${appVersion}` : 'Manager'}
            </span>
          </div>
        </Link>

        {/* NAVIGATION (CENTER) */}
        <nav className="flex items-center gap-1 px-2 py-1.5 rounded-xl bg-zinc-100/80 dark:bg-zinc-800/80" data-tauri-drag-region>
          {primaryItems.map((item) => {
            const active = isActive(item.path);
            return (
              <button
                key={item.path}
                onClick={() => navigate(item.path)}
                className={cn(
                  "relative flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-all duration-200",
                  active
                    ? "text-zinc-900 dark:text-white"
                    : "text-zinc-500 dark:text-zinc-400 hover:text-zinc-700 dark:hover:text-zinc-200"
                )}
              >
                {active && (
                  <motion.span
                    layoutId="navbar-pill"
                    className="absolute inset-0 rounded-lg bg-white dark:bg-zinc-700 shadow-sm -z-10"
                    transition={{ type: "spring", stiffness: 400, damping: 30 }}
                  />
                )}
                <item.icon className={cn("h-4 w-4", active && "text-indigo-500")} />
                <span className="hidden lg:inline">{item.label}</span>
              </button>
            );
          })}

          {/* More dropdown */}
          <div className="relative" ref={moreRef}>
            <button
              onClick={() => setShowMore(!showMore)}
              className={cn(
                "relative flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-all duration-200",
                isSecondaryActive
                  ? "text-zinc-900 dark:text-white"
                  : "text-zinc-500 dark:text-zinc-400 hover:text-zinc-700 dark:hover:text-zinc-200"
              )}
            >
              {isSecondaryActive && (
                <motion.span
                  layoutId="navbar-pill"
                  className="absolute inset-0 rounded-lg bg-white dark:bg-zinc-700 shadow-sm -z-10"
                  transition={{ type: "spring", stiffness: 400, damping: 30 }}
                />
              )}
              <MoreHorizontal className={cn("h-4 w-4", isSecondaryActive && "text-indigo-500")} />
              <span className="hidden lg:inline">{t('nav.more', 'More')}</span>
            </button>

            {/* Dropdown */}
            {showMore && (
              <motion.div
                initial={{ opacity: 0, y: -5 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -5 }}
                className="absolute top-full right-0 mt-2 w-44 py-1.5 bg-white dark:bg-zinc-800 rounded-xl shadow-xl border border-zinc-200 dark:border-zinc-700 z-50"
              >
                {secondaryItems.map((item) => {
                  const active = isActive(item.path);
                  return (
                    <button
                      key={item.path}
                      onClick={() => {
                        navigate(item.path);
                        setShowMore(false);
                      }}
                      className={cn(
                        "w-full flex items-center gap-2.5 px-3 py-2 text-sm transition-colors",
                        active
                          ? "text-indigo-600 dark:text-indigo-400 bg-indigo-50 dark:bg-indigo-500/10"
                          : "text-zinc-600 dark:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-700"
                      )}
                    >
                      <item.icon className="h-4 w-4" />
                      {item.label}
                    </button>
                  );
                })}
              </motion.div>
            )}
          </div>
        </nav>

        {/* ACTIONS (RIGHT) */}
        <div className="flex items-center gap-1" data-tauri-drag-region>
          <DebugConsoleButton />
          <button 
            onClick={toggleTheme}
            className="p-2 rounded-lg text-zinc-500 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-white hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors"
            title={config?.theme === 'light' ? t('nav.theme_to_dark') : t('nav.theme_to_light')}
          >
            {config?.theme === 'light' ? (
              <Moon className="h-4 w-4" />
            ) : (
              <Sun className="h-4 w-4" />
            )}
          </button>
        </div>
      </div>
    </header>
  );
};

export default memo(Navbar);
