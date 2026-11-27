import { Link, useLocation } from 'react-router-dom';
import { NavigationDropdown } from './NavDropdowns';
import { isActive, getCurrentNavItem, isDouble11Active, type NavItem } from './constants';
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

    // 过滤隐藏的菜单项
    const visibleNavItems = navItems.filter(item => !isMenuItemHidden(item.path));

    return (
        <>
            {/* 文字胶囊 (≥ 1120px) */}
            <nav aria-label="Primary" className="max-[1119px]:hidden flex items-center gap-1 bg-gray-100 dark:bg-base-200 rounded-full p-1">
                {visibleNavItems.map((item) => {
                    const isApiKeyFun = item.path === '/apikey-fun';
                    const showEventBadge = isApiKeyFun && isDouble11Active();
                    const active = isActive(location.pathname, item.path);

                    return (
                        <Link
                            key={item.path}
                            to={item.path}
                            draggable="false"
                            aria-current={active ? 'page' : undefined}
                            className={`
                                ${showEventBadge ? 'pl-3.5 pr-2 xl:pl-5 xl:pr-2.5' : 'px-4 xl:px-6'}
                                py-2 
                                rounded-full 
                                text-sm 
                                font-medium 
                                transition-all 
                                whitespace-nowrap
                                inline-flex
                                items-center
                                gap-1.5
                                focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500
                                ${active
                                    ? 'bg-gray-900 text-white shadow-sm dark:bg-white dark:text-gray-900'
                                    : 'text-gray-700 hover:text-gray-900 hover:bg-gray-200 dark:text-gray-400 dark:hover:text-base-content dark:hover:bg-base-100'
                                }
                            `}
                        >
                            <span>{item.label}</span>
                            {/* [TEMPORARY_EVENT: DOUBLE11_2026] 菜单栏微型活动标签 */}
                            {showEventBadge && (
                                <span className="inline-flex items-center justify-center px-1.5 py-0.5 text-[10px] font-bold rounded-md bg-[#ffe8cc] text-[#b45309] dark:bg-[#fff2cc] dark:text-[#934800] leading-none select-none tracking-tight shadow-[0_1px_2px_rgba(0,0,0,0.06)] border border-[#fed7aa]/70 dark:border-[#fde68a]/80">
                                    双11
                                </span>
                            )}
                        </Link>
                    );
                })}
            </nav>

            {/* 图标胶囊 (480px - 1120px) */}
            <nav aria-label="Primary icon navigation" className="max-[479px]:hidden min-[1120px]:hidden flex items-center gap-1 bg-gray-100 dark:bg-base-200 rounded-full p-1">
                {visibleNavItems.map((item) => {
                    const active = isActive(location.pathname, item.path);
                    return (
                        <Link
                            key={item.path}
                            to={item.path}
                            draggable="false"
                            aria-label={item.label}
                            aria-current={active ? 'page' : undefined}
                            className={`
                                p-2
                                rounded-full
                                transition-all
                                focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500
                                ${active
                                    ? 'bg-gray-900 text-white shadow-sm dark:bg-white dark:text-gray-900'
                                    : 'text-gray-700 hover:text-gray-900 hover:bg-gray-200 dark:text-gray-400 dark:hover:text-base-content dark:hover:bg-base-100'
                                }
                            `}
                            title={item.label}
                        >
                            <item.icon className="w-5 h-5" aria-hidden="true" />
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
