import { Link, useLocation } from 'react-router-dom';
import { NavigationDropdown } from './NavDropdowns';
import { isActive, getCurrentNavItem, type NavItem } from './constants';
import { useConfigStore } from '../../stores/useConfigStore';
import { motion } from 'framer-motion';

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

    // 过滤隐藏的菜单项
    const visibleNavItems = navItems.filter(item => !isMenuItemHidden(item.path));

    return (
        <>
            {/* 文字胶囊 (≥ 1120px) */}
            <nav className="max-[1119px]:hidden flex items-center gap-1 bg-gray-100/60 dark:bg-white/5 rounded-full p-1 border border-gray-200/50 dark:border-white/5 relative">
                {visibleNavItems.map((item) => {
                    const active = isActive(location.pathname, item.path);
                    return (
                        <Link
                            key={item.path}
                            to={item.path}
                            draggable="false"
                            className={`
                                relative px-4 xl:px-6 py-2 rounded-full text-sm font-medium transition-colors duration-200 whitespace-nowrap z-10
                                ${active
                                    ? 'text-white dark:text-gray-900 font-semibold'
                                    : 'text-gray-700 hover:text-gray-900 dark:text-gray-400 dark:hover:text-white'
                                }
                            `}
                        >
                            {active && (
                                <motion.div
                                    layoutId="active-pill-text"
                                    className="absolute inset-0 bg-gray-900 dark:bg-white rounded-full -z-10 shadow-sm"
                                    transition={{ type: "spring", stiffness: 350, damping: 28 }}
                                />
                            )}
                            {item.label}
                        </Link>
                    );
                })}
            </nav>

            {/* 图标胶囊 (880px - 1120px) - Logo 显示文字 */}
            <nav className="max-[879px]:hidden min-[1120px]:hidden flex items-center gap-1 bg-gray-100/60 dark:bg-white/5 rounded-full p-1 border border-gray-200/50 dark:border-white/5 relative">
                {visibleNavItems.map((item) => {
                    const active = isActive(location.pathname, item.path);
                    return (
                        <Link
                            key={item.path}
                            to={item.path}
                            draggable="false"
                            className={`
                                relative p-2 rounded-full transition-colors duration-200 z-10
                                ${active
                                    ? 'text-white dark:text-gray-900'
                                    : 'text-gray-700 hover:text-gray-900 dark:text-gray-400 dark:hover:text-white'
                                }
                            `}
                            title={item.label}
                        >
                            {active && (
                                <motion.div
                                    layoutId="active-pill-icon-1"
                                    className="absolute inset-0 bg-gray-900 dark:bg-white rounded-full -z-10 shadow-sm"
                                    transition={{ type: "spring", stiffness: 350, damping: 28 }}
                                />
                            )}
                            <item.icon className="w-5 h-5 relative z-10" />
                        </Link>
                    );
                })}
            </nav>

            {/* 图标胶囊 (640px - 880px) - Logo 隐藏文字 */}
            <nav className="max-[639px]:hidden min-[880px]:hidden flex items-center gap-1 bg-gray-100/60 dark:bg-white/5 rounded-full p-1 border border-gray-200/50 dark:border-white/5 relative">
                {visibleNavItems.map((item) => {
                    const active = isActive(location.pathname, item.path);
                    return (
                        <Link
                            key={item.path}
                            to={item.path}
                            draggable="false"
                            className={`
                                relative p-2 rounded-full transition-colors duration-200 z-10
                                ${active
                                    ? 'text-white dark:text-gray-900'
                                    : 'text-gray-700 hover:text-gray-900 dark:text-gray-400 dark:hover:text-white'
                                }
                            `}
                            title={item.label}
                        >
                            {active && (
                                <motion.div
                                    layoutId="active-pill-icon-2"
                                    className="absolute inset-0 bg-gray-900 dark:bg-white rounded-full -z-10 shadow-sm"
                                    transition={{ type: "spring", stiffness: 350, damping: 28 }}
                                />
                            )}
                            <item.icon className="w-5 h-5 relative z-10" />
                        </Link>
                    );
                })}
            </nav>

            {/* 图标胶囊 (480px - 640px) */}
            <nav className="max-[479px]:hidden min-[640px]:hidden flex items-center gap-1 bg-gray-100/60 dark:bg-white/5 rounded-full p-1 border border-gray-200/50 dark:border-white/5 relative">
                {visibleNavItems.map((item) => {
                    const active = isActive(location.pathname, item.path);
                    return (
                        <Link
                            key={item.path}
                            to={item.path}
                            draggable="false"
                            className={`
                                relative p-2 rounded-full transition-colors duration-200 z-10
                                ${active
                                    ? 'text-white dark:text-gray-900'
                                    : 'text-gray-700 hover:text-gray-900 dark:text-gray-400 dark:hover:text-white'
                                }
                            `}
                            title={item.label}
                        >
                            {active && (
                                <motion.div
                                    layoutId="active-pill-icon-3"
                                    className="absolute inset-0 bg-gray-900 dark:bg-white rounded-full -z-10 shadow-sm"
                                    transition={{ type: "spring", stiffness: 350, damping: 28 }}
                                />
                            )}
                            <item.icon className="w-5 h-5 relative z-10" />
                        </Link>
                    );
                })}
            </nav>

            {/* 图标+文字下拉 (375px - 480px) */}
            <div className="max-[374px]:hidden min-[480px]:hidden block">
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
