import { Link, useLocation } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { NavigationDropdown } from './NavDropdowns';
import { isActive, getCurrentNavItem, type NavItem } from './constants';
import { useConfigStore } from '../../stores/useConfigStore';

interface NavMenuProps {
    navItems: NavItem[];
}

/**
 * 导航菜单组件 - 独立处理响应式
 * 
 * 响应式策略:
 * - ≥ 768px (md): 文字胶囊
 * - 640px - 768px: 图标胶囊 (Logo 显示文字)
 * - 480px - 640px: 图标胶囊 (Logo 隐藏文字)
 * - 375px - 480px: 图标+文字下拉
 * - < 375px: 图标下拉
 */
export function NavMenu({ navItems }: NavMenuProps) {
    const location = useLocation();
    const { isMenuItemHidden } = useConfigStore();
    const { i18n } = useTranslation();

    const isChinese = i18n.language.startsWith('zh');

    // 过滤隐藏的菜单项
    const visibleNavItems = navItems.filter(item => !isMenuItemHidden(item.path));

    return (
        <>
            {/* 文字胶囊 (中文: ≥1024px, 非中文: ≥1280px) */}
            <nav className={`hidden ${isChinese ? 'lg:flex' : 'xl:flex'} items-center gap-1 bg-gray-100 dark:bg-base-200 rounded-full p-1`}>
                {visibleNavItems.map((item) => (
                    <Link
                        key={item.path}
                        to={item.path}
                        draggable="false"
                        className={`
                            px-4 xl:px-6
                            py-2 
                            rounded-full 
                            text-sm 
                            font-medium 
                            transition-all 
                            whitespace-nowrap
                            ${isActive(location.pathname, item.path)
                                ? 'bg-gray-900 text-white shadow-sm dark:bg-white dark:text-gray-900'
                                : 'text-gray-700 hover:text-gray-900 hover:bg-gray-200 dark:text-gray-400 dark:hover:text-base-content dark:hover:bg-base-100'
                            }
                        `}
                    >
                        {item.label}
                    </Link>
                ))}
            </nav>

            {/* 图标胶囊 (中文: 820px-1024px, 非中文: 820px-1280px) - Logo 不显示文字 */}
            <nav className={`hidden min-[820px]:flex ${isChinese ? 'lg:hidden' : 'xl:hidden'} items-center gap-1 bg-gray-100 dark:bg-base-200 rounded-full p-1`}>
                {visibleNavItems.map((item) => (
                    <Link
                        key={item.path}
                        to={item.path}
                        draggable="false"
                        className={`
                            p-2
                            rounded-full
                            transition-all
                            ${isActive(location.pathname, item.path)
                                ? 'bg-gray-900 text-white shadow-sm dark:bg-white dark:text-gray-900'
                                : 'text-gray-700 hover:text-gray-900 hover:bg-gray-200 dark:text-gray-400 dark:hover:text-base-content dark:hover:bg-base-100'
                            }
                        `}
                        title={item.label}
                    >
                        <item.icon className="w-5 h-5" />
                    </Link>
                ))}
            </nav>

            {/* 图标胶囊 (640px - 820px) - Logo 显示文字 */}
            <nav className="hidden min-[640px]:flex min-[820px]:hidden items-center gap-1 bg-gray-100 dark:bg-base-200 rounded-full p-1">
                {visibleNavItems.map((item) => (
                    <Link
                        key={item.path}
                        to={item.path}
                        draggable="false"
                        className={`
                            p-2
                            rounded-full
                            transition-all
                            ${isActive(location.pathname, item.path)
                                ? 'bg-gray-900 text-white shadow-sm dark:bg-white dark:text-gray-900'
                                : 'text-gray-700 hover:text-gray-900 hover:bg-gray-200 dark:text-gray-400 dark:hover:text-base-content dark:hover:bg-base-100'
                            }
                        `}
                        title={item.label}
                    >
                        <item.icon className="w-5 h-5" />
                    </Link>
                ))}
            </nav>

            {/* 图标胶囊 (480px - 640px) - Logo 隐藏文字 */}
            <nav className="hidden min-[480px]:flex min-[640px]:hidden items-center gap-1 bg-gray-100 dark:bg-base-200 rounded-full p-1">
                {visibleNavItems.map((item) => (
                    <Link
                        key={item.path}
                        to={item.path}
                        draggable="false"
                        className={`
                            p-2
                            rounded-full
                            transition-all
                            ${isActive(location.pathname, item.path)
                                ? 'bg-gray-900 text-white shadow-sm dark:bg-white dark:text-gray-900'
                                : 'text-gray-700 hover:text-gray-900 hover:bg-gray-200 dark:text-gray-400 dark:hover:text-base-content dark:hover:bg-base-100'
                            }
                        `}
                        title={item.label}
                    >
                        <item.icon className="w-5 h-5" />
                    </Link>
                ))}
            </nav>

            {/* 图标+文字下拉 (375px - 480px) */}
            <div className="hidden min-[375px]:block min-[480px]:hidden">
                <NavigationDropdown
                    navItems={visibleNavItems}
                    isActive={(path) => isActive(location.pathname, path)}
                    getCurrentNavItem={() => getCurrentNavItem(location.pathname, visibleNavItems)}
                    onNavigate={() => { }}
                    showLabel={true}
                />
            </div>

            {/* 图标下拉 (< 375px) */}
            <div className="min-[375px]:hidden">
                <NavigationDropdown
                    navItems={visibleNavItems}
                    isActive={(path) => isActive(location.pathname, path)}
                    getCurrentNavItem={() => getCurrentNavItem(location.pathname, visibleNavItems)}
                    onNavigate={() => { }}
                    showLabel={false}
                />
            </div>
        </>
    );
}
