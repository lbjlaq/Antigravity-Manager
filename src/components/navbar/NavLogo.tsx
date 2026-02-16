import React, { useState, Suspense, useRef, useEffect } from 'react';
import { Link } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import LogoIcon from '../../../src-tauri/icons/icon.png';

// [SPRING_FESTIVAL_START]
// 懒加载彩蛋组件，避免影响首屏性能
const SpringFestivalEffect = React.lazy(() => import('../effects/SpringFestivalEffect'));
import { LanternSwitch } from '../effects/LanternSwitch';
// [SPRING_FESTIVAL_END]

/**
 * Logo 组件 - 独立处理响应式
 * 
 * 响应式策略:
 * 父容器宽度 >= 200px: Logo + 文字
 * 父容器宽度 <  200px: 只有 Logo
 */
export function NavLogo() {
    const { t } = useTranslation();

    // [SPRING_FESTIVAL_START]
    const [showEasterEgg, setShowEasterEgg] = useState(false);
    const [logoCenterX, setLogoCenterX] = useState<number | undefined>(undefined);
    const imgRef = useRef<HTMLImageElement>(null);

    useEffect(() => {
        const updatePosition = () => {
            if (imgRef.current) {
                const rect = imgRef.current.getBoundingClientRect();
                setLogoCenterX(rect.left + rect.width / 2);
            }
        };

        updatePosition();
        window.addEventListener('resize', updatePosition);
        // Also update after a short delay to ensure layout stability
        setTimeout(updatePosition, 100);

        return () => window.removeEventListener('resize', updatePosition);
    }, []);

    const handleLogoClick = () => {
        // 只有点击图片才触发，避免误触链接跳转(虽然 Link 包裹了 img)
        // 这里阻止冒泡可能影响 Link 跳转，所以我们只作为附加效果
        // 如果用户想跳转首页，点击文字更稳妥；点击图标既跳转又出彩蛋也可以
        setShowEasterEgg(true);
    };
    // [SPRING_FESTIVAL_END]

    return (
        <Link to="/" draggable="false" className="flex w-full min-w-0 items-center gap-2 text-xl font-semibold text-gray-900 dark:text-base-content">
            {/* [SPRING_FESTIVAL_START] - 相对定位容器，用于从顶部挂绳 */}
            <div className="relative flex items-center justify-center">
                <img
                    ref={imgRef}
                    src={LogoIcon}
                    alt="Logo"
                    className="w-8 h-8 cursor-pointer active:scale-95 transition-transform relative z-10"
                    draggable="false"
                    onClick={handleLogoClick}
                />

                {/* 拉绳开关 - Fixed 定位，位置由 JS 计算传入 */}
                {logoCenterX !== undefined && (
                    <LanternSwitch
                        onTrigger={() => setShowEasterEgg(true)}
                        xPosition={logoCenterX}
                    />
                )}
            </div>
            {/* [SPRING_FESTIVAL_END] */}

            {/* 父容器宽度 < 200px 隐藏 */}
            <span className="hidden @[200px]/logo:inline text-nowrap">{t('common.app_name', 'Antigravity Tools')}</span>

            {/* [SPRING_FESTIVAL_START] */}
            {showEasterEgg && (
                <Suspense fallback={null}>
                    <SpringFestivalEffect onClose={() => setShowEasterEgg(false)} />
                </Suspense>
            )}
            {/* [SPRING_FESTIVAL_END] */}
        </Link>
    );
}
