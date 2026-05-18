import { LayoutDashboard, Users, Network, Activity, BarChart3, Settings, Lock } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useConfigStore } from '../../stores/useConfigStore';
import { isTauri, isLinux } from '../../utils/env';
import { NavLogo } from './NavLogo';
import { NavMenu } from './NavMenu';
import { NavSettings } from './NavSettings';
import type { NavItem } from './constants';

/**
 * Navbar 主组件
 * 
 * 职责: 只负责布局和状态管理,不处理响应式细节
 * 响应式逻辑由各个子组件独立处理
 */
function Navbar() {
    const { t } = useTranslation();
    const { config, saveConfig } = useConfigStore();

    // 创建导航项(包含翻译后的标签)
    const navItems: NavItem[] = [
        { path: '/', label: t('nav.dashboard'), icon: LayoutDashboard, priority: 'high' },
        { path: '/accounts', label: t('nav.accounts'), icon: Users, priority: 'high' },
        { path: '/api-proxy', label: t('nav.proxy'), icon: Network, priority: 'high' },
        { path: '/monitor', label: t('nav.call_records'), icon: Activity, priority: 'medium' },
        { path: '/token-stats', label: t('nav.token_stats', 'Token 统计'), icon: BarChart3, priority: 'low' },
        { path: '/user-token', label: t('nav.user_token', 'User Tokens'), icon: Users, priority: 'low' },
        { path: '/security', label: t('nav.security'), icon: Lock, priority: 'low' },
        { path: '/settings', label: t('nav.settings'), icon: Settings, priority: 'high' },
    ];

    // 主题切换逻辑(带 View Transition 动画)
    const toggleTheme = async (event: React.MouseEvent<HTMLButtonElement>) => {
        if (!config) return;

        const newTheme = config.theme === 'light' ? 'dark' : 'light';

        // Use View Transition API if supported, but skip on Linux (may cause crash)
        if ('startViewTransition' in document && !isLinux()) {
            const x = event.clientX;
            const y = event.clientY;
            const endRadius = Math.hypot(
                Math.max(x, window.innerWidth - x),
                Math.max(y, window.innerHeight - y)
            );

            // @ts-ignore
            const transition = document.startViewTransition(async () => {
                saveConfig({
                    ...config,
                    theme: newTheme,
                    language: config.language
                }, true);
            });

            transition.ready.then(() => {
                const isDarkMode = newTheme === 'dark';
                const clipPath = isDarkMode
                    ? [`circle(${endRadius}px at ${x}px ${y}px)`, `circle(0px at ${x}px ${y}px)`]
                    : [`circle(0px at ${x}px ${y}px)`, `circle(${endRadius}px at ${x}px ${y}px)`];

                document.documentElement.animate(
                    {
                        clipPath: clipPath
                    },
                    {
                        duration: 500,
                        easing: 'ease-in-out',
                        fill: 'forwards',
                        pseudoElement: isDarkMode ? '::view-transition-old(root)' : '::view-transition-new(root)'
                    }
                );
            });
        } else {
            // Fallback: direct switch (Linux or browsers without View Transition)
            await saveConfig({
                ...config,
                theme: newTheme,
                language: config.language
            }, true);
        }
    };

    // 语言切换逻辑
    const handleLanguageChange = async (langCode: string) => {
        if (!config) return;

        await saveConfig({
            ...config,
            language: langCode,
            theme: config.theme
        }, true);
    };

    return (
        <nav
            style={{ position: 'sticky', top: 0, zIndex: 50 }}
            className="pt-9 transition-all duration-200 bg-[#FAFBFC] dark:bg-base-300"
        >
            {/* 窗口拖拽区域 - Tauri 专用 */}
            {isTauri() && (
                <div
                    className="absolute top-9 left-0 right-0 h-16"
                    style={{ zIndex: 5, backgroundColor: 'rgba(0,0,0,0.001)' }}
                    data-tauri-drag-region
                />
            )}

            <div className="max-w-7xl mx-auto px-8 relative" style={{ zIndex: 10 }}>
                {/* Flexbox 布局 - 子组件自己处理响应式 */}
                <div className="flex items-center h-16 gap-4">
                    {/* Logo - 使用父容器宽度做响应式 */}
                    <div className="@container/logo basis-[200px] shrink min-w-0">
                        <NavLogo />
                    </div>

                    {/* 导航菜单 - 自己处理响应式 */}
                    <div className="flex-1 flex justify-center">
                        <NavMenu navItems={navItems} />
                    </div>

                    {/* 设置按钮 - 自己处理响应式 */}
                    <NavSettings
                        theme={(config?.theme as 'light' | 'dark') || 'light'}
                        currentLanguage={config?.language || 'es'}
                        onThemeToggle={toggleTheme}
                        onLanguageChange={handleLanguageChange}
                    />
                </div>
            </div>
        </nav>
    );
}

export default Navbar;
