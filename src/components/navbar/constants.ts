import type { LucideIcon } from 'lucide-react';

// 类型定义
export interface NavItem {
    path: string;
    label: string;
    icon: LucideIcon;
    priority: 'high' | 'medium' | 'low';
}

// [TEMPORARY_EVENT: DOUBLE11_2026] (Expires: 2026-11-11 23:59:59 +08:00)
// 双11活动专属标签。活动到期后由 bump-version 脚本自动提示在下一次发版时物理移除此标记块及关联代码。
export const DOUBLE11_2026_END_TIME = new Date('2026-11-11T23:59:59+08:00').getTime();
export const isDouble11Active = () => Date.now() <= DOUBLE11_2026_END_TIME;

export interface Language {
    code: string;
    label: string;
    short: string;
}

// 语言配置
export const LANGUAGES: Language[] = [
    { code: 'zh', label: '简体中文', short: 'ZH' },
    { code: 'zh-TW', label: '繁體中文', short: 'TW' },
    { code: 'en', label: 'English', short: 'EN' },
    { code: 'ja', label: '日本語', short: 'JA' },
    { code: 'tr', label: 'Türkçe', short: 'TR' },
    { code: 'vi', label: 'Tiếng Việt', short: 'VI' },
    { code: 'pt', label: 'Português', short: 'PT' },
    { code: 'ko', label: '한국어', short: 'KO' },
    { code: 'ru', label: 'Русский', short: 'RU' },
    { code: 'ar', label: 'العربية', short: 'AR' },
    { code: 'es', label: 'Español', short: 'ES' },
    { code: 'my', label: 'Bahasa Melayu', short: 'MY' },
];

// 工具函数
export const isActive = (pathname: string, itemPath: string): boolean => {
    if (itemPath === '/') {
        return pathname === '/';
    }
    return pathname.startsWith(itemPath);
};

export const getCurrentNavItem = (pathname: string, navItems: NavItem[]): NavItem => {
    return navItems.find(item => isActive(pathname, item.path)) || navItems[0];
};
