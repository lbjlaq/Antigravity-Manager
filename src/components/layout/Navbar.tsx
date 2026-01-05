import { Link, useLocation } from 'react-router-dom';
import { Sun, Moon } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useConfigStore } from '../../stores/useConfigStore';

function Navbar() {
    const location = useLocation();
    const { t, i18n } = useTranslation();
    const { config, saveConfig } = useConfigStore();

    const navItems = [
        { path: '/', label: t('nav.dashboard') },
        { path: '/accounts', label: t('nav.accounts') },
        { path: '/api-proxy', label: t('nav.proxy') },
        { path: '/settings', label: t('nav.settings') },
    ];

    const isActive = (path: string) => {
        if (path === '/') {
            return location.pathname === '/';
        }
        return location.pathname.startsWith(path);
    };

    const toggleTheme = async (event: React.MouseEvent<HTMLButtonElement>) => {
        if (!config) return;

        const newTheme = config.theme === 'light' ? 'dark' : 'light';

        // 如果浏览器支持 View Transition API
        if ('startViewTransition' in document) {
            const x = event.clientX;
            const y = event.clientY;
            const endRadius = Math.hypot(
                Math.max(x, window.innerWidth - x),
                Math.max(y, window.innerHeight - y)
            );

            // @ts-ignore
            const transition = document.startViewTransition(async () => {
                await saveConfig({
                    ...config,
                    theme: newTheme,
                    language: config.language
                });
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
            // 降级方案：直接切换
            await saveConfig({
                ...config,
                theme: newTheme,
                language: config.language
            });
        }
    };

    const toggleLanguage = async () => {
        if (!config) return;
        const newLang = config.language === 'zh' ? 'en' : 'zh';
        await saveConfig({
            ...config,
            language: newLang,
            theme: config.theme
        });
        i18n.changeLanguage(newLang);
    };

    return (
        <nav
            style={{ position: 'sticky', top: 0, zIndex: 50 }}
            className="pt-9 transition-all duration-200 bg-[#FAFBFC] dark:bg-base-300"
        >
            {/* 窗口拖拽区域 2 - 覆盖导航栏内容区域（在交互元素下方） */}
            <div
                className="absolute top-9 left-0 right-0 h-16"
                style={{ zIndex: 5, backgroundColor: 'rgba(0,0,0,0.001)' }}
                data-tauri-drag-region
            />

            <div className="max-w-7xl mx-auto px-8 relative" style={{ zIndex: 10 }}>
                <div className="flex items-center justify-between h-16">
                    {/* Logo - 左侧 */}
                    <div className="flex items-center">
                        <Link to="/" className="text-xl font-semibold text-gray-900 dark:text-base-content flex items-center gap-2">
                            Antigravity Tools
                        </Link>
                    </div>

                    {/* 药丸形状的导航标签 - 居中 */}
                    <div className="flex items-center gap-1 bg-gray-100 dark:bg-base-200 rounded-full p-1">
                        {navItems.map((item) => (
                            <Link
                                key={item.path}
                                to={item.path}
                                className={`px-6 py-2 rounded-full text-sm font-medium transition-all ${isActive(item.path)
                                    ? 'bg-gray-900 text-white shadow-sm dark:bg-white dark:text-gray-900'
                                    : 'text-gray-700 hover:text-gray-900 hover:bg-gray-200 dark:text-gray-400 dark:hover:text-base-content dark:hover:bg-base-100'
                                    }`}
                            >
                                {item.label}
                            </Link>
                        ))}
                    </div>

                    {/* 右侧快捷设置按钮 */}
                    <div className="flex items-center gap-2">
                        {/* 主题切换按钮 */}
                        <button
                            onClick={toggleTheme}
                            className="w-10 h-10 rounded-full bg-gray-100 dark:bg-base-200 hover:bg-gray-200 dark:hover:bg-base-100 flex items-center justify-center transition-colors"
                            title={config?.theme === 'light' ? '切换到深色模式' : '切换到浅色模式'}
                        >
                            {config?.theme === 'light' ? (
                                <Moon className="w-5 h-5 text-gray-700 dark:text-gray-300" />
                            ) : (
                                <Sun className="w-5 h-5 text-gray-700 dark:text-gray-300" />
                            )}
                        </button>

                        {/* 语言切换按钮 */}
                        <button
                            onClick={toggleLanguage}
                            className="w-10 h-10 rounded-full bg-gray-100 dark:bg-base-200 hover:bg-gray-200 dark:hover:bg-base-100 flex items-center justify-center transition-colors"
                            title={config?.language === 'zh' ? 'Switch to English' : '切换到中文'}
                        >
                            <span className="text-sm font-bold text-gray-700 dark:text-gray-300">
                                {config?.language === 'zh' ? 'EN' : '中'}
                            </span>
                        </button>
                    </div>
                </div>
            </div>
        </nav>
    );
}

export default Navbar;
