import React, { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { ArrowRightLeft, Blocks, Repeat2, Terminal } from 'lucide-react';
import type { ActiveClientsStatus } from '../../types/account';
import { cn } from '../../utils/cn';
import {
    computeAccountSwitchMenuPosition,
    isTriggerVisibleInClipBounds,
    resolveTriggerClipBounds,
    type AccountSwitchMenuPosition,
} from './accountSwitchMenuPosition';

export interface AccountSwitchMenuProps {
    accountId: string;
    disabled?: boolean;
    isSwitching?: boolean;
    activeClients?: ActiveClientsStatus;
    onSwitch: (target: 'classic' | 'ide' | 'acp' | 'agy') => void;
    size?: 'sm' | 'md';
    className?: string;
}

/**
 * 账号切换目标配置：
 * 1. classic: Antigravity 客户端（原“经典版”，依需求去除“经典版”字样）
 * 2. ide: Antigravity IDE
 * 3. acp: JEB / JetBrains 插件
 * 4. agy: AGY CLI
 */
export const CLIENT_TARGETS: Array<{
    id: 'classic' | 'ide' | 'acp' | 'agy';
    labelKey: string;
    defaultLabel: string;
    descKey: string;
    defaultDesc: string;
    icon: React.ComponentType<{ className?: string }>;
    colorClass: string;
    bgClass: string;
    activeBorderClass: string;
}> = [
    {
        id: 'classic',
        labelKey: 'accounts.actions_modal.clients.classic_short',
        defaultLabel: 'Antigravity',
        descKey: 'accounts.actions_modal.clients.classic_tooltip',
        defaultDesc: '切换到 Antigravity 客户端',
        icon: ArrowRightLeft,
        colorClass: 'text-amber-600 dark:text-amber-400',
        bgClass: 'bg-amber-50 hover:bg-amber-100 dark:bg-amber-950/40 dark:hover:bg-amber-900/50 border-amber-200/80 dark:border-amber-800/60',
        activeBorderClass: 'ring-1 ring-amber-500',
    },
    {
        id: 'ide',
        labelKey: 'accounts.actions_modal.clients.ide_short',
        defaultLabel: 'IDE',
        descKey: 'accounts.actions_modal.clients.ide_tooltip',
        defaultDesc: '切换到 Antigravity IDE',
        icon: Repeat2,
        colorClass: 'text-blue-600 dark:text-blue-400',
        bgClass: 'bg-blue-50 hover:bg-blue-100 dark:bg-blue-900/30 dark:hover:bg-blue-900/50 border-blue-200/80 dark:border-blue-800/60',
        activeBorderClass: 'ring-1 ring-blue-500',
    },
    {
        id: 'acp',
        labelKey: 'accounts.actions_modal.clients.acp_short',
        defaultLabel: 'JEB',
        descKey: 'accounts.actions_modal.clients.acp_tooltip',
        defaultDesc: '切换到 JEB / JetBrains (ACP)',
        icon: Blocks,
        colorClass: 'text-purple-600 dark:text-purple-400',
        bgClass: 'bg-purple-50 hover:bg-purple-100 dark:bg-purple-900/30 dark:hover:bg-purple-900/50 border-purple-200/80 dark:border-purple-800/60',
        activeBorderClass: 'ring-1 ring-purple-500',
    },
    {
        id: 'agy',
        labelKey: 'accounts.actions_modal.clients.agy_short',
        defaultLabel: 'AGY CLI',
        descKey: 'accounts.actions_modal.clients.agy_tooltip',
        defaultDesc: '切换到 AGY 命令行 (agy)',
        icon: Terminal,
        colorClass: 'text-emerald-600 dark:text-emerald-400',
        bgClass: 'bg-emerald-50 hover:bg-emerald-100 dark:bg-emerald-900/30 dark:hover:bg-emerald-900/50 border-emerald-200/80 dark:border-emerald-800/60',
        activeBorderClass: 'ring-1 ring-emerald-500',
    },
];

/**
 * 账号行/卡片内【切换】按钮复合组件：
 * 点击后向上（上方空间不足时自动向下）展开 4 个客户端微型图标。
 */
export default function AccountSwitchMenu({
    accountId,
    disabled = false,
    isSwitching = false,
    activeClients,
    onSwitch,
    size = 'sm',
    className,
}: AccountSwitchMenuProps) {
    const { t } = useTranslation();
    const [isOpen, setIsOpen] = useState(false);
    const [menuPosition, setMenuPosition] = useState<AccountSwitchMenuPosition | null>(null);

    const containerRef = useRef<HTMLDivElement>(null);
    const triggerRef = useRef<HTMLButtonElement>(null);
    const menuRef = useRef<HTMLDivElement>(null);

    const updatePosition = useCallback(() => {
        const triggerEl = triggerRef.current;
        const menuEl = menuRef.current;
        if (!triggerEl || !menuEl) return;

        const triggerRect = triggerEl.getBoundingClientRect();
        const viewport = { width: window.innerWidth, height: window.innerHeight };
        const clipBounds = resolveTriggerClipBounds(triggerEl, triggerRect, viewport);

        if (!isTriggerVisibleInClipBounds(triggerRect, clipBounds)) {
            setIsOpen(false);
            return;
        }

        const menuRect = menuEl.getBoundingClientRect();
        const menuSize = {
            width: menuRect.width || menuEl.offsetWidth || 220,
            height: menuRect.height || menuEl.offsetHeight || 56,
        };
        const isRtl = window.getComputedStyle(triggerEl).direction === 'rtl';

        setMenuPosition(
            computeAccountSwitchMenuPosition({
                triggerRect,
                menuSize,
                viewport,
                clipBounds,
                isRtl,
            })
        );
    }, []);

    // 通过 Portal 渲染到 body 后，在首次绘制前同步测量并计算视口绝对位置
    useLayoutEffect(() => {
        if (!isOpen) {
            setMenuPosition(null);
            return;
        }
        updatePosition();
    }, [isOpen, updatePosition]);

    // 禁用或切换进行中时自动收起菜单
    useEffect(() => {
        if ((disabled || isSwitching) && isOpen) {
            setIsOpen(false);
        }
    }, [disabled, isSwitching, isOpen]);

    // 点击外部、按下 Escape、滚动或窗口缩放时更新/收起菜单
    useEffect(() => {
        if (!isOpen) return;

        const handleClickOutside = (e: MouseEvent) => {
            const target = e.target as Node | null;
            if (!target) return;
            const insideContainer = containerRef.current?.contains(target);
            const insideMenu = menuRef.current?.contains(target);
            if (!insideContainer && !insideMenu) {
                setIsOpen(false);
            }
        };

        const handleKeyDown = (e: KeyboardEvent) => {
            if (e.key === 'Escape') {
                setIsOpen(false);
            }
        };

        document.addEventListener('mousedown', handleClickOutside);
        document.addEventListener('keydown', handleKeyDown);
        window.addEventListener('scroll', updatePosition, true);
        window.addEventListener('resize', updatePosition);
        return () => {
            document.removeEventListener('mousedown', handleClickOutside);
            document.removeEventListener('keydown', handleKeyDown);
            window.removeEventListener('scroll', updatePosition, true);
            window.removeEventListener('resize', updatePosition);
        };
    }, [isOpen, updatePosition]);

    const handleSelectTarget = (target: 'classic' | 'ide' | 'acp' | 'agy') => {
        setIsOpen(false);
        onSwitch(target);
    };

    const menuNode = isOpen ? (
        <div
            ref={menuRef}
            onClick={(e) => e.stopPropagation()}
            role="menu"
            aria-label={t('accounts.switch_target_label', '选择切换客户端')}
            style={
                menuPosition
                    ? {
                        position: 'fixed',
                        top: `${menuPosition.top}px`,
                        left: `${menuPosition.left}px`,
                        zIndex: 90,
                    }
                    : {
                        position: 'fixed',
                        top: -9999,
                        left: -9999,
                        visibility: 'hidden',
                        zIndex: 90,
                    }
            }
            className={cn(
                "flex items-center gap-1 p-1 bg-white dark:bg-base-100 rounded-xl shadow-2xl border border-gray-200 dark:border-base-300 backdrop-blur-md animate-in fade-in zoom-in-95 duration-100 whitespace-nowrap",
                menuPosition?.placement === 'bottom' ? "origin-top-right" : "origin-bottom-right"
            )}
        >
            {CLIENT_TARGETS.map((target) => {
                const Icon = target.icon;
                const isCurrentActive =
                    (target.id === 'classic' && activeClients?.classic_account_id === accountId) ||
                    (target.id === 'ide' && activeClients?.ide_account_id === accountId) ||
                    (target.id === 'acp' && activeClients?.acp_account_id === accountId) ||
                    (target.id === 'agy' && activeClients?.agy_account_id === accountId);

                const targetLabel = t(target.labelKey, target.defaultLabel);
                const targetDesc = t(target.descKey, target.defaultDesc);

                return (
                    <button
                        key={target.id}
                        type="button"
                        role="menuitem"
                        onClick={() => handleSelectTarget(target.id)}
                        title={`${targetLabel}: ${targetDesc}`}
                        className={cn(
                            "relative flex flex-col items-center justify-center p-1.5 rounded-lg border text-center transition-all duration-150 active:scale-95 group",
                            target.bgClass,
                            target.colorClass,
                            isCurrentActive && target.activeBorderClass
                        )}
                    >
                        <Icon className="w-3.5 h-3.5 flex-shrink-0 transition-transform group-hover:scale-110" />
                        <span className="text-[9px] font-bold mt-0.5 leading-none max-w-[48px] truncate">
                            {targetLabel}
                        </span>

                        {/* 活跃指示小绿点 */}
                        {isCurrentActive && (
                            <span
                                className="absolute -top-0.5 -right-0.5 w-2 h-2 rounded-full bg-emerald-500 border border-white dark:border-base-100 shadow-xs"
                                title={t('accounts.current_active_client', '当前正在使用该账号')}
                            />
                        )}
                    </button>
                );
            })}
        </div>
    ) : null;

    return (
        <div ref={containerRef} className={cn("relative inline-block w-full", className)}>
            {/* 主触发切换按钮 */}
            <button
                ref={triggerRef}
                type="button"
                className={cn(
                    "w-full flex items-center justify-center gap-0.5 rounded-lg font-semibold transition-all shadow-xs select-none",
                    size === 'sm' ? "px-1 py-1 text-xs" : "px-2 py-1.5 text-xs font-medium",
                    disabled
                        ? "bg-gray-100 dark:bg-base-200 text-gray-400 dark:text-gray-500 cursor-not-allowed border border-gray-200 dark:border-base-300"
                        : isOpen
                            ? "bg-blue-600 text-white shadow-sm border border-blue-600"
                            : "bg-blue-50 dark:bg-blue-900/20 text-blue-600 dark:text-blue-400 hover:bg-blue-100 dark:hover:bg-blue-900/40 border border-blue-200/60 dark:border-blue-800/40 active:scale-95"
                )}
                onClick={(e) => {
                    e.stopPropagation();
                    if (!disabled && !isSwitching) {
                        setIsOpen((prev) => !prev);
                    }
                }}
                disabled={disabled || isSwitching}
                title={disabled ? t('accounts.disabled_tooltip') : (isSwitching ? t('common.loading') : t('accounts.switch_menu_tooltip', '选择切换目标客户端 (Antigravity / IDE / JEB / AGY CLI)'))}
                aria-haspopup="menu"
                aria-expanded={isOpen}
            >
                <Repeat2 className={cn(size === 'sm' ? "w-3 h-3" : "w-3.5 h-3.5", "flex-shrink-0", isSwitching && "animate-spin")} />
                <span className="truncate">{t('accounts.action_switch', '切换')}</span>
            </button>

            {/* 通过 Portal 挂载到 body，避免被表格 sticky 表头/邻行层叠上下文与滚动容器遮挡裁切 */}
            {menuNode && (typeof document !== 'undefined' ? createPortal(menuNode, document.body) : menuNode)}
        </div>
    );
}
