import { useState, useRef, useEffect } from 'react';
import { Link } from 'react-router-dom';
import { ChevronDown, MoreVertical, Sun, Moon, LogOut, Minimize2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { NavItem, Language } from './constants';
import { isTauri } from '../../utils/env';
import { useViewStore } from '../../stores/useViewStore';

// useClickOutside Hook
export function useClickOutside(
    ref: React.RefObject<HTMLElement | null>,
    handler: () => void
) {
    useEffect(() => {
        const listener = (event: MouseEvent) => {
            if (!ref.current || ref.current.contains(event.target as Node)) {
                return;
            }
            handler();
        };

        document.addEventListener('mousedown', listener);
        return () => document.removeEventListener('mousedown', listener);
    }, [ref, handler]);
}

// 语言下拉菜单组件
interface LanguageDropdownProps {
    currentLanguage: string;
    languages: Language[];
    onLanguageChange: (langCode: string) => void;
    className?: string;
}

export function LanguageDropdown({
    currentLanguage,
    languages,
    onLanguageChange,
    className = ''
}: LanguageDropdownProps) {
    const [isOpen, setIsOpen] = useState(false);
    const menuRef = useRef<HTMLDivElement>(null);
    const { t } = useTranslation();

    useClickOutside(menuRef, () => setIsOpen(false));

    const handleLanguageChange = (langCode: string) => {
        onLanguageChange(langCode);
        setIsOpen(false);
    };

    return (
        <div className={`relative ${className}`} ref={menuRef}>
            <button
                onClick={() => setIsOpen(!isOpen)}
                className="w-10 h-10 rounded-full bg-gray-100 dark:bg-base-200 hover:bg-gray-200 dark:hover:bg-base-100 flex items-center justify-center transition-colors"
                title={t('settings.general.language')}
            >
                <span className="text-sm font-bold text-gray-700 dark:text-gray-300">
                    {languages.find(l => l.code === currentLanguage)?.short || 'EN'}
                </span>
            </button>

            {/* 下拉菜单 */}
            {isOpen && (
                <div className="absolute ltr:right-0 rtl:left-0 mt-2 w-32 bg-white dark:bg-base-200 rounded-xl shadow-lg border border-gray-100 dark:border-base-100 py-1 overflow-hidden animate-in fade-in zoom-in-95 duration-200 ltr:origin-top-right rtl:origin-top-left">
                    {languages.map((lang) => (
                        <button
                            key={lang.code}
                            onClick={() => handleLanguageChange(lang.code)}
                            className={`w-full px-4 py-2 text-left text-sm flex items-center justify-between hover:bg-gray-50 dark:hover:bg-base-100 transition-colors ${currentLanguage === lang.code
                                ? 'text-blue-500 font-medium bg-blue-50 dark:bg-blue-900/10'
                                : 'text-gray-700 dark:text-gray-300'
                                }`}
                        >
                            <div className="flex items-center gap-3">
                                <span className="font-mono font-bold w-6">{lang.short}</span>
                                <span className="text-xs opacity-70">{lang.label}</span>
                            </div>
                            {currentLanguage === lang.code && (
                                <span className="w-1.5 h-1.5 rounded-full bg-blue-500"></span>
                            )}
                        </button>
                    ))}
                </div>
            )}
        </div>
    );
}

// 导航下拉菜单组件 (< 375px)
interface NavigationDropdownProps {
    navItems: NavItem[];
    isActive: (path: string) => boolean;
    getCurrentNavItem: () => NavItem | undefined;
    onNavigate: () => void;
    showLabel?: boolean; // 是否显示文字标签
}

export function NavigationDropdown({
    navItems,
    isActive,
    getCurrentNavItem,
    onNavigate,
    showLabel = true // 默认显示文字
}: NavigationDropdownProps) {
    const [isOpen, setIsOpen] = useState(false);
    const menuRef = useRef<HTMLDivElement>(null);

    useClickOutside(menuRef, () => setIsOpen(false));

    const handleNavItemClick = () => {
        setIsOpen(false);
        onNavigate();
    };

    const currentItem = getCurrentNavItem();
    const CurrentIcon = currentItem?.icon;

    // 如果没有当前项,不渲染
    if (!currentItem || !CurrentIcon) return null;

    return (
        <div className="relative" ref={menuRef}>
            <button
                onClick={() => setIsOpen(!isOpen)}
                className="flex items-center gap-2 px-3 py-2 rounded-full bg-gray-100 dark:bg-base-200 hover:bg-gray-200 dark:hover:bg-base-100 transition-colors"
            >
                <CurrentIcon className="w-4 h-4 text-gray-700 dark:text-gray-300" />
                {/* 根据 showLabel 控制文字显示 */}
                {showLabel && (
                    <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
                        {currentItem.label}
                    </span>
                )}
                <ChevronDown className={`w-3 h-3 text-gray-700 dark:text-gray-300 transition-transform ${isOpen ? 'rotate-180' : ''}`} />
            </button>

            {/* 下拉菜单 */}
            {isOpen && (
                <div className="absolute left-1/2 -translate-x-1/2 mt-2 w-48 bg-white dark:bg-[#1a1a1a] rounded-xl shadow-xl border-2 border-gray-200 dark:border-gray-700 py-1 overflow-hidden animate-in fade-in zoom-in-95 duration-200 origin-top">
                    {navItems.map((item) => (
                        <Link
                            key={item.path}
                            to={item.path}
                            draggable="false"
                            onClick={handleNavItemClick}
                            className={`w-full px-4 py-2.5 text-left text-sm flex items-center gap-3 hover:bg-gray-50 dark:hover:bg-base-100 transition-colors ${isActive(item.path)
                                ? 'text-blue-500 font-medium bg-blue-50 dark:bg-blue-900/10'
                                : 'text-gray-700 dark:text-gray-300'
                                }`}
                        >
                            <item.icon className="w-4 h-4" />
                            <span>{item.label}</span>
                        </Link>
                    ))}
                </div>
            )}
        </div>
    );
}

// 更多菜单组件 (< 480px)
interface MoreDropdownProps {
    theme: 'light' | 'dark';
    currentLanguage: string;
    languages: Language[];
    onThemeToggle: (event: React.MouseEvent<HTMLButtonElement>) => void;
    onLanguageChange: (langCode: string) => void;
}

export function MoreDropdown({
    theme,
    currentLanguage,
    languages,
    onThemeToggle,
    onLanguageChange
}: MoreDropdownProps) {
    const [isOpen, setIsOpen] = useState(false);
    const menuRef = useRef<HTMLDivElement>(null);
    const { t } = useTranslation();
    const { setMiniView } = useViewStore();

    useClickOutside(menuRef, () => setIsOpen(false));

    const handleThemeToggle = (event: React.MouseEvent<HTMLButtonElement>) => {
        onThemeToggle(event);
        setIsOpen(false);
    };

    const handleLanguageChange = (langCode: string) => {
        onLanguageChange(langCode);
        setIsOpen(false);
    };

    const handleLogout = () => {
        sessionStorage.removeItem('abv_admin_api_key');
        localStorage.removeItem('abv_admin_api_key');
        window.location.reload();
    };

    return (
        <div className="relative" ref={menuRef}>
            <button
                onClick={() => setIsOpen(!isOpen)}
                className="w-10 h-10 rounded-full bg-gray-100 dark:bg-base-200 hover:bg-gray-200 dark:hover:bg-base-100 flex items-center justify-center transition-colors"
                title={t('nav.more', '更多')}
            >
                <MoreVertical className="w-5 h-5 text-gray-700 dark:text-gray-300" />
            </button>

            {/* 下拉菜单 */}
            {isOpen && (
                <div className="absolute ltr:right-0 rtl:left-0 mt-2 w-40 bg-white dark:bg-base-200 rounded-xl shadow-lg border border-gray-100 dark:border-base-100 py-1 overflow-hidden animate-in fade-in zoom-in-95 duration-200 ltr:origin-top-right rtl:origin-top-left">
                    {/* 迷你视图 */}
                    <button
                        onClick={() => {
                            setMiniView(true);
                            setIsOpen(false);
                        }}
                        className="w-full px-4 py-2.5 text-left text-sm flex items-center gap-3 hover:bg-gray-50 dark:hover:bg-base-100 transition-colors text-gray-700 dark:text-gray-300"
                    >
                        <Minimize2 className="w-4 h-4" />
                        <span>{t('nav.mini_view', 'Mini View')}</span>
                    </button>

                    {/* 主题切换 */}
                    <button
                        onClick={handleThemeToggle}
                        className="w-full px-4 py-2.5 text-left text-sm flex items-center gap-3 hover:bg-gray-50 dark:hover:bg-base-100 transition-colors text-gray-700 dark:text-gray-300"
                    >
                        {theme === 'light' ? (
                            <Moon className="w-4 h-4" />
                        ) : (
                            <Sun className="w-4 h-4" />
                        )}
                        <span>{theme === 'light' ? t('nav.theme_to_dark') : t('nav.theme_to_light')}</span>
                    </button>

                    {/* 分隔线 */}
                    <div className="my-1 border-t border-gray-100 dark:border-base-100"></div>

                    {/* 语言选择 */}
                    {languages.map((lang) => (
                        <button
                            key={lang.code}
                            onClick={() => handleLanguageChange(lang.code)}
                            className={`w-full px-4 py-2 text-left text-sm flex items-center justify-between hover:bg-gray-50 dark:hover:bg-base-100 transition-colors ${currentLanguage === lang.code
                                ? 'text-blue-500 font-medium bg-blue-50 dark:bg-blue-900/10'
                                : 'text-gray-700 dark:text-gray-300'
                                }`}
                        >
                            <div className="flex items-center gap-2">
                                <span className="font-mono font-bold text-xs">{lang.short}</span>
                                <span className="text-xs opacity-70">{lang.label}</span>
                            </div>
                            {currentLanguage === lang.code && (
                                <span className="w-1.5 h-1.5 rounded-full bg-blue-500"></span>
                            )}
                        </button>
                    ))}

                    {/* 登出按钮 - 仅 Web 模式显示 */}
                    {!isTauri() && (
                        <>
                            <div className="my-1 border-t border-gray-100 dark:border-base-100"></div>
                            <button
                                onClick={handleLogout}
                                className="w-full px-4 py-2.5 text-left text-sm flex items-center gap-3 hover:bg-red-50 dark:hover:bg-red-900/20 transition-colors text-red-600 dark:text-red-400"
                            >
                                <LogOut className="w-4 h-4" />
                                <span>{t('nav.logout', '登出')}</span>
                            </button>
                        </>
                    )}
                </div>
            )}
        </div>
    );
}
