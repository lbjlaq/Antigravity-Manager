import { Link } from 'react-router-dom';
import LogoIcon from '../../../src-tauri/icons/icon.png';

/**
 * Logo 组件 - 独立处理响应式
 * 
 * 响应式策略:
 * - ≥ 1280px (xl): Logo + 文字
 * - < 1280px: 只有 Logo
 */
export function NavLogo() {
    return (
        <Link to="/" draggable="false" className="flex items-center gap-2 text-xl font-semibold text-gray-900 dark:text-base-content">
            <img src={LogoIcon} alt="Logo" className="w-8 h-8" draggable="false" />
            {/* 880px 以下隐藏文字 */}
            <span className="hidden min-[880px]:inline">Antigravity Tools</span>
        </Link>
    );
}
