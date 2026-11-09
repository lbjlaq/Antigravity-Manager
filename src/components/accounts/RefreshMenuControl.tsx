import { useEffect, useRef, useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown, ChevronLeft, Clock, RefreshCw, Zap } from 'lucide-react';
import { useConfigStore } from '../../stores/useConfigStore';
import type { AppConfig } from '../../types/config';
import { cn } from '../../utils/cn';
import { showToast } from '../common/ToastContainer';

interface RefreshMenuControlProps {
    /** 触发刷新回调（无选中时刷新所有，有选中时批量刷新选中） */
    onRefresh: () => void;
    /** 当前是否正在执行刷新中 */
    isRefreshing: boolean;
    /** 当前选中的账号数量（>0 时优先展示刷新选中） */
    selectedCount?: number;
    className?: string;
}

/**
 * 复合刷新控制组件：
 * 1. 左侧为主按钮：点击立即执行刷新（刷新所有 / 刷新选中）；
 * 2. 右侧为下拉触发箭头：点击展开菜单（立即刷新、自动刷新配置）；
 * 3. 自动刷新二级菜单：鼠标悬浮或点击向左展开（大屏），在左侧空间不足时响应式自适应为手风琴折叠展开；
 * 4. 集成启动时刷新配额（开关+上限）与后台自动刷新开关，双端状态实时同步。
 */
export default function RefreshMenuControl({
    onRefresh,
    isRefreshing,
    selectedCount = 0,
    className,
}: RefreshMenuControlProps) {
    const { t } = useTranslation();
    const { config, saveConfig } = useConfigStore();

    // 一级下拉菜单与二级设置菜单开关
    const [isOpen, setIsOpen] = useState(false);
    const [isSubOpen, setIsSubOpen] = useState(false);
    // 二级菜单展开定位：'left' 为向左浮动（默认），'inline' 为窄屏下在菜单内部手风琴式折叠展开
    const [subPlacement, setSubPlacement] = useState<'left' | 'inline'>('left');

    const [limitDraft, setLimitDraft] = useState('0');
    const [delayDraft, setDelayDraft] = useState('0.2');

    const containerRef = useRef<HTMLDivElement>(null);
    const primaryMenuRef = useRef<HTMLDivElement>(null);
    const subMenuRef = useRef<HTMLDivElement>(null);
    const closeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    const startupEnabled = config?.startup_refresh !== false;
    const startupLimit = config?.startup_refresh_max_accounts ?? 0;
    const rawSlideDelay = config?.refresh_slide_delay_secs;
    const slideDelay = typeof rawSlideDelay === 'number' && Number.isFinite(rawSlideDelay)
        ? rawSlideDelay
        : 0.2;
    const autoRefreshEnabled = !!config?.auto_refresh;
    const isQuotaProtectionLocked = !!config?.quota_protection?.enabled;

    useEffect(() => {
        setLimitDraft(String(startupLimit));
    }, [startupLimit]);

    useEffect(() => {
        setDelayDraft(String(Number(slideDelay.toFixed(2))));
    }, [slideDelay]);

    // 计算二级菜单是向左展开还是内联手风琴折叠展开（响应式安全防遮蔽）
    const updatePlacement = useCallback(() => {
        if (!primaryMenuRef.current) return;
        const rect = primaryMenuRef.current.getBoundingClientRect();
        // 二级菜单设计宽度约 290px，加上 20px 安全余量，需左侧有至少 310px 空间
        const windowWidth = window.innerWidth;
        if (rect.left < 310 || windowWidth < 640) {
            setSubPlacement('inline');
        } else {
            setSubPlacement('left');
        }
    }, []);

    // 监听窗口尺寸变化
    useEffect(() => {
        if (isOpen) {
            updatePlacement();
            window.addEventListener('resize', updatePlacement);
            return () => {
                window.removeEventListener('resize', updatePlacement);
            };
        }
    }, [isOpen, updatePlacement]);

    // 点击外部与 Escape 键关闭
    useEffect(() => {
        if (!isOpen) return;

        const handleClickOutside = (event: MouseEvent) => {
            if (!containerRef.current?.contains(event.target as Node)) {
                setIsOpen(false);
                setIsSubOpen(false);
            }
        };

        const handleKeyDown = (event: KeyboardEvent) => {
            if (event.key === 'Escape') {
                setIsOpen(false);
                setIsSubOpen(false);
            }
        };

        document.addEventListener('mousedown', handleClickOutside);
        document.addEventListener('keydown', handleKeyDown);
        return () => {
            document.removeEventListener('mousedown', handleClickOutside);
            document.removeEventListener('keydown', handleKeyDown);
        };
    }, [isOpen]);

    // 鼠标悬停防抖管理，防止移向左侧子菜单时发生间隙闪退
    const handleMouseEnterSub = () => {
        if (closeTimerRef.current) {
            clearTimeout(closeTimerRef.current);
            closeTimerRef.current = null;
        }
        updatePlacement();
        setIsSubOpen(true);
    };

    const handleMouseLeaveSub = () => {
        if (closeTimerRef.current) {
            clearTimeout(closeTimerRef.current);
        }
        closeTimerRef.current = setTimeout(() => {
            setIsSubOpen(false);
        }, 180);
    };

    const toggleSubMenu = () => {
        updatePlacement();
        setIsSubOpen((prev) => !prev);
    };

    // 配置实时热持久化
    const persist = async (patch: Partial<AppConfig>) => {
        const latest = useConfigStore.getState().config;
        if (!latest) return;
        try {
            await saveConfig({ ...latest, ...patch });
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const commitLimit = () => {
        const parsed = parseInt(limitDraft, 10);
        const next = Number.isNaN(parsed) ? 0 : Math.min(Math.max(parsed, 0), 100000);
        setLimitDraft(String(next));
        if (next !== startupLimit) {
            void persist({ startup_refresh_max_accounts: next });
        }
    };

    const commitDelay = () => {
        const parsed = parseFloat(delayDraft);
        const next = Number.isFinite(parsed)
            ? Number(Math.min(Math.max(parsed, 0), 10).toFixed(2))
            : 0.2;
        setDelayDraft(String(next));
        if (next !== slideDelay) {
            void persist({ refresh_slide_delay_secs: next });
        }
    };

    const limitBadgeText = !startupEnabled
        ? t('accounts.startup_refresh_off', '关')
        : startupLimit === 0
            ? '∞'
            : `≤${startupLimit}`;

    const buttonLabel = isRefreshing
        ? t('common.loading')
        : selectedCount > 0
            ? t('accounts.refresh_selected', { count: selectedCount })
            : t('accounts.refresh_all', '刷新所有');

    const buttonTitle = selectedCount > 0
        ? t('accounts.refresh_selected', { count: selectedCount })
        : t('accounts.refresh_all', '刷新所有');

    return (
        <div ref={containerRef} className={cn("relative inline-flex items-center shrink-0", className)}>
            {/* Split Button 结构：左侧主刷新，右侧下拉展开 */}
            <div className="inline-flex items-stretch rounded-lg shadow-sm border border-blue-600/20 dark:border-blue-500/30 overflow-hidden">
                {/* 主按钮：立即刷新 */}
                <button
                    type="button"
                    className={cn(
                        "px-2.5 py-2 bg-blue-500 text-white text-xs font-medium hover:bg-blue-600 active:bg-blue-700 transition-colors flex items-center gap-1.5",
                        isRefreshing && "opacity-75 cursor-not-allowed"
                    )}
                    onClick={() => {
                        if (!isRefreshing) {
                            onRefresh();
                        }
                    }}
                    disabled={isRefreshing}
                    title={buttonTitle}
                >
                    <RefreshCw className={cn("w-3.5 h-3.5 shrink-0", isRefreshing && "animate-spin")} />
                    <span className="hidden xl:inline whitespace-nowrap">{buttonLabel}</span>
                </button>

                {/* 分割线 */}
                <div className="w-[1px] bg-blue-400/50 dark:bg-blue-600/50 my-1 self-stretch" />

                {/* 下拉按钮：展开选项菜单 */}
                <button
                    type="button"
                    className={cn(
                        "px-1.5 py-2 bg-blue-500 text-white text-xs font-medium hover:bg-blue-600 active:bg-blue-700 transition-colors flex items-center justify-center",
                        isOpen && "bg-blue-600 dark:bg-blue-700"
                    )}
                    onClick={() => {
                        updatePlacement();
                        setIsOpen((prev) => !prev);
                        if (isOpen) {
                            setIsSubOpen(false);
                        }
                    }}
                    aria-expanded={isOpen}
                    title={t('accounts.refresh_menu_tooltip', '刷新选项与自动刷新设置')}
                >
                    <ChevronDown className={cn("w-3 h-3 transition-transform duration-200", isOpen && "rotate-180")} />
                </button>
            </div>

            {/* 一级下拉菜单 */}
            {isOpen && (
                <div
                    ref={primaryMenuRef}
                    role="menu"
                    className="absolute right-0 top-full mt-1.5 z-50 w-56 rounded-xl border border-gray-200 dark:border-base-300 bg-white dark:bg-base-100 shadow-xl p-1.5 text-xs select-none animate-in fade-in zoom-in-95 duration-100"
                >
                    {/* 项 1：立即刷新 */}
                    <button
                        type="button"
                        role="menuitem"
                        disabled={isRefreshing}
                        className={cn(
                            "w-full flex items-center justify-between px-2.5 py-2 rounded-lg text-left transition-colors font-medium",
                            isRefreshing
                                ? "text-gray-400 dark:text-gray-500 cursor-not-allowed bg-gray-50 dark:bg-base-200/50"
                                : "text-gray-800 dark:text-gray-200 hover:bg-blue-50 dark:hover:bg-blue-900/20 hover:text-blue-600 dark:hover:text-blue-400"
                        )}
                        onClick={() => {
                            setIsOpen(false);
                            setIsSubOpen(false);
                            onRefresh();
                        }}
                    >
                        <div className="flex items-center gap-2 min-w-0">
                            <Zap className="w-3.5 h-3.5 text-blue-500 shrink-0" />
                            <span className="truncate">
                                {selectedCount > 0
                                    ? t('accounts.refresh_selected', { count: selectedCount })
                                    : t('accounts.refresh_now', '立即刷新')}
                            </span>
                        </div>
                        {isRefreshing && (
                            <RefreshCw className="w-3 h-3 text-blue-500 animate-spin shrink-0" />
                        )}
                    </button>

                    <div className="my-1 border-t border-gray-100 dark:border-base-300" />

                    {/* 项 2：自动刷新（悬浮/点击向左展开二级菜单） */}
                    <div
                        className="relative"
                        onMouseEnter={handleMouseEnterSub}
                        onMouseLeave={handleMouseLeaveSub}
                    >
                        <button
                            type="button"
                            role="menuitem"
                            className={cn(
                                "w-full flex items-center justify-between px-2.5 py-2 rounded-lg text-left transition-colors font-medium",
                                isSubOpen
                                    ? "bg-blue-50 dark:bg-blue-900/20 text-blue-600 dark:text-blue-400"
                                    : "text-gray-800 dark:text-gray-200 hover:bg-gray-100 dark:hover:bg-base-200"
                            )}
                            onClick={toggleSubMenu}
                            title={t('accounts.startup_refresh_tooltip', '账号加载完成后异步刷新一次全部配额（滑动窗口：最多 5 个账号并发，按配置延迟让位）')}
                        >
                            <div className="flex items-center gap-2 min-w-0">
                                <Clock className="w-3.5 h-3.5 text-indigo-500 shrink-0" />
                                <span className="truncate">{t('settings.account.auto_refresh', '自动刷新')}</span>
                            </div>
                            <div className="flex items-center gap-1.5 shrink-0">
                                <span
                                    className={cn(
                                        "px-1.5 py-0.5 rounded text-[10px] font-bold transition-colors",
                                        startupEnabled || autoRefreshEnabled
                                            ? "bg-blue-100 dark:bg-blue-500/20 text-blue-600 dark:text-blue-400"
                                            : "bg-gray-100 dark:bg-gray-800 text-gray-500 dark:text-gray-400"
                                    )}
                                >
                                    {limitBadgeText}
                                </span>
                                <ChevronLeft
                                    className={cn(
                                        "w-3.5 h-3.5 text-gray-400 transition-transform duration-150",
                                        subPlacement === 'inline'
                                            ? (isSubOpen ? "-rotate-90" : "rotate-0")
                                            : (isSubOpen ? "-translate-x-0.5 text-blue-500" : "rotate-0")
                                    )}
                                />
                            </div>
                        </button>

                        {/* 二级菜单：向左悬浮展开（宽屏）或手风琴就地折叠展开（窄屏） */}
                        {isSubOpen && (
                            <div
                                ref={subMenuRef}
                                onMouseEnter={handleMouseEnterSub}
                                onMouseLeave={handleMouseLeaveSub}
                                className={cn(
                                    subPlacement === 'left'
                                        ? "absolute right-full top-0 mr-1.5 z-50 w-72 max-w-[calc(100vw-2rem)] rounded-xl border border-gray-200 dark:border-base-300 bg-white dark:bg-base-100 shadow-2xl p-4 text-xs animate-in fade-in slide-in-from-right-2 duration-150"
                                        : "mt-1.5 p-3 rounded-lg bg-gray-50 dark:bg-base-200/60 border border-gray-200/70 dark:border-base-300/80 animate-in fade-in duration-150"
                                )}
                            >
                                {/* 鼠标悬浮防抖命中桥接区域（向右微延展，防止鼠标平移时断触） */}
                                {subPlacement === 'left' && (
                                    <div className="absolute -right-2 top-0 bottom-0 w-3 pointer-events-auto" />
                                )}

                                {/* 二级面板内容：启动时刷新配额与账号上限 */}
                                <div>
                                    <div className="flex items-start justify-between gap-3">
                                        <div className="min-w-0">
                                            <div className="text-xs font-bold text-gray-900 dark:text-gray-100">
                                                {t('settings.account.startup_refresh', '启动时刷新配额')}
                                            </div>
                                            <p className="mt-0.5 text-[11px] leading-relaxed text-gray-500 dark:text-gray-400">
                                                {t('settings.account.startup_refresh_desc')}
                                            </p>
                                        </div>
                                        <label className="relative inline-flex items-center cursor-pointer shrink-0">
                                            <input
                                                type="checkbox"
                                                className="sr-only peer"
                                                checked={startupEnabled}
                                                onChange={() => void persist({ startup_refresh: !startupEnabled })}
                                            />
                                            <div className="w-9 h-5 bg-gray-200 dark:bg-base-300 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-blue-500 shadow-inner"></div>
                                        </label>
                                    </div>

                                    {/* 账号上限输入 */}
                                    <div className="mt-3 pt-3 border-t border-gray-100 dark:border-base-300">
                                        <div className="flex items-center justify-between gap-2">
                                            <label className="text-[11px] font-bold text-gray-600 dark:text-gray-300 uppercase tracking-wider">
                                                {t('settings.account.startup_refresh_limit', '启动刷新账号上限')}
                                            </label>
                                            <input
                                                type="number"
                                                className="w-16 px-2 py-1 bg-gray-50 dark:bg-base-200 border border-gray-200 dark:border-base-300 rounded-md focus:ring-2 focus:ring-blue-500 outline-none text-xs font-bold text-blue-600 dark:text-blue-400 text-center"
                                                min="0"
                                                max="100000"
                                                value={limitDraft}
                                                onChange={(e) => setLimitDraft(e.target.value)}
                                                onBlur={commitLimit}
                                                onKeyDown={(e) => {
                                                    if (e.key === 'Enter') {
                                                        e.currentTarget.blur();
                                                    }
                                                }}
                                            />
                                        </div>
                                        <p className="mt-1.5 text-[10px] leading-relaxed text-gray-400 dark:text-gray-500">
                                            {t('settings.account.startup_refresh_limit_hint', '账号数超过该值时跳过启动刷新，0 为不限制')}
                                        </p>
                                    </div>

                                    {/* 滑动窗口延迟输入 */}
                                    <div className="mt-3 pt-3 border-t border-gray-100 dark:border-base-300">
                                        <div className="flex items-center justify-between gap-2">
                                            <label className="text-[11px] font-bold text-gray-600 dark:text-gray-300 uppercase tracking-wider">
                                                {t('settings.account.refresh_slide_delay', '滑动窗口延迟（秒）')}
                                            </label>
                                            <input
                                                type="number"
                                                className="w-16 px-2 py-1 bg-gray-50 dark:bg-base-200 border border-gray-200 dark:border-base-300 rounded-md focus:ring-2 focus:ring-blue-500 outline-none text-xs font-bold text-blue-600 dark:text-blue-400 text-center"
                                                min="0"
                                                max="10"
                                                step="0.1"
                                                value={delayDraft}
                                                onChange={(e) => setDelayDraft(e.target.value)}
                                                onBlur={commitDelay}
                                                onKeyDown={(e) => {
                                                    if (e.key === 'Enter') {
                                                        e.currentTarget.blur();
                                                    }
                                                }}
                                            />
                                        </div>
                                        <p className="mt-1.5 text-[10px] leading-relaxed text-gray-400 dark:text-gray-500">
                                            {t('settings.account.refresh_slide_delay_hint', '一个账号刷新完成后等待该秒数再让位给下一个账号；并发上限固定为 5。')}
                                        </p>
                                    </div>

                                    {/* 二级面板扩展：后台自动刷新开关 */}
                                    <div className="mt-3 pt-3 border-t border-gray-100 dark:border-base-300">
                                        <div className="flex items-start justify-between gap-3">
                                            <div className="min-w-0">
                                                <div className="text-xs font-bold text-gray-900 dark:text-gray-100">
                                                    {t('settings.account.auto_refresh', '后台自动刷新')}
                                                </div>
                                                <p className="mt-0.5 text-[10px] leading-relaxed text-gray-400 dark:text-gray-500">
                                                    {isQuotaProtectionLocked
                                                        ? t('settings.account.auto_refresh_locked_hint', '配额保护已开启，后台自动刷新强制保持常驻')
                                                        : t('settings.account.auto_refresh_desc')}
                                                </p>
                                            </div>
                                            <label className={cn(
                                                "relative inline-flex items-center shrink-0",
                                                isQuotaProtectionLocked ? "cursor-not-allowed opacity-80" : "cursor-pointer"
                                            )}>
                                                <input
                                                    type="checkbox"
                                                    className="sr-only peer"
                                                    checked={autoRefreshEnabled}
                                                    disabled={isQuotaProtectionLocked}
                                                    onChange={() => void persist({ auto_refresh: !autoRefreshEnabled })}
                                                />
                                                <div className="w-9 h-5 bg-gray-200 dark:bg-base-300 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-blue-500 shadow-inner"></div>
                                            </label>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        )}
                    </div>
                </div>
            )}
        </div>
    );
}
