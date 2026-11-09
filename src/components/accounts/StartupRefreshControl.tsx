import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown, RefreshCw } from 'lucide-react';
import { useConfigStore } from '../../stores/useConfigStore';
import type { AppConfig } from '../../types/config';
import { cn } from '../../utils/cn';
import { showToast } from '../common/ToastContainer';

/**
 * 账号管理页工具栏上的启动刷新状态胶囊。
 *
 * 把「启动时刷新配额 + 账号数上限」从设置页提升到账号页可见位置，
 * 同时保留设置页的同名配置，二者共用同一份 gui_config.json。
 */
export default function StartupRefreshControl() {
    const { t } = useTranslation();
    const { config, saveConfig } = useConfigStore();
    const [isOpen, setIsOpen] = useState(false);
    const [limitDraft, setLimitDraft] = useState('5');
    const containerRef = useRef<HTMLDivElement>(null);

    const enabled = config?.startup_refresh !== false;
    const limit = config?.refresh_concurrency ?? 5;

    useEffect(() => {
        setLimitDraft(String(limit));
    }, [limit]);

    useEffect(() => {
        if (!isOpen) return;
        const handleClickOutside = (event: MouseEvent) => {
            if (!containerRef.current?.contains(event.target as Node)) {
                setIsOpen(false);
            }
        };
        const handleKeyDown = (event: KeyboardEvent) => {
            if (event.key === 'Escape') {
                setIsOpen(false);
            }
        };
        document.addEventListener('mousedown', handleClickOutside);
        document.addEventListener('keydown', handleKeyDown);
        return () => {
            document.removeEventListener('mousedown', handleClickOutside);
            document.removeEventListener('keydown', handleKeyDown);
        };
    }, [isOpen]);

    if (!config) return null;

    // 始终基于 store 中的最新配置合并，避免 blur 保存上限与开关保存相互覆盖
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
        const next = Number.isNaN(parsed) ? 5 : Math.min(Math.max(parsed, 1), 20);
        setLimitDraft(String(next));
        if (next !== limit) {
            void persist({ refresh_concurrency: next });
        }
    };

    const limitLabel = !enabled
        ? t('accounts.startup_refresh_off', '关')
        : `${limit}并`;

    return (
        <div ref={containerRef} className="relative shrink-0">
            <button
                type="button"
                className={cn(
                    "flex items-center gap-1.5 px-2.5 py-2 rounded-lg text-xs font-medium border transition-colors",
                    enabled
                        ? "bg-blue-50 dark:bg-blue-900/20 text-blue-600 dark:text-blue-400 border-blue-200 dark:border-blue-800/60"
                        : "bg-gray-50 dark:bg-base-200 text-gray-500 dark:text-gray-400 border-gray-200 dark:border-base-300"
                )}
                onClick={() => setIsOpen((prev) => !prev)}
                aria-expanded={isOpen}
                title={t('accounts.startup_refresh_tooltip', '账号加载完成后异步刷新一次全部配额（受账号上限约束）')}
            >
                <RefreshCw className="w-3.5 h-3.5" />
                <span className="hidden xl:inline">{t('accounts.startup_refresh_short', '启动刷新')}</span>
                <span
                    className={cn(
                        "px-1.5 py-0.5 rounded-md text-[10px] font-bold",
                        enabled
                            ? "bg-blue-100 dark:bg-blue-500/20 text-blue-600 dark:text-blue-400"
                            : "bg-gray-200 dark:bg-gray-700 text-gray-500 dark:text-gray-400"
                    )}
                >
                    {limitLabel}
                </span>
                <ChevronDown className={cn("w-3 h-3 transition-transform", isOpen && "rotate-180")} />
            </button>

            {isOpen && (
                <div
                    role="dialog"
                    aria-label={t('settings.account.startup_refresh')}
                    className="absolute right-0 top-full mt-2 z-50 w-72 max-w-[calc(100vw-2rem)] rounded-xl border border-gray-200 dark:border-base-300 bg-white dark:bg-base-100 shadow-xl p-4"
                >
                    <div className="flex items-start justify-between gap-3">
                        <div>
                            <div className="text-sm font-bold text-gray-900 dark:text-gray-100">
                                {t('settings.account.startup_refresh')}
                            </div>
                            <p className="mt-0.5 text-xs leading-relaxed text-gray-500 dark:text-gray-400">
                                {t('settings.account.startup_refresh_desc')}
                            </p>
                        </div>
                        <label className="relative inline-flex items-center cursor-pointer shrink-0">
                            <input
                                type="checkbox"
                                className="sr-only peer"
                                checked={enabled}
                                onChange={() => void persist({ startup_refresh: !enabled })}
                            />
                            <div className="w-11 h-6 bg-gray-200 dark:bg-base-300 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-500 shadow-inner"></div>
                        </label>
                    </div>

                    <div className="mt-4 pt-4 border-t border-gray-100 dark:border-base-300">
                        <div className="flex items-center gap-2">
                            <label className="text-xs font-bold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                                {t('settings.account.refresh_concurrency', '同时探测账号数')}
                            </label>
                            <input
                                type="number"
                                className="w-20 px-2 py-1.5 bg-gray-50 dark:bg-base-200 border border-gray-100 dark:border-base-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none text-sm font-bold text-blue-600 dark:text-blue-400"
                                min="1"
                                max="20"
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
                        <p className="mt-2 text-[11px] leading-relaxed text-gray-500 dark:text-gray-400">
                            {t('settings.account.refresh_concurrency_hint', '批量配额刷新与启动探测时的同时探测账号数（滑动窗口容量），默认 5，范围 1~20。')}
                        </p>
                    </div>
                </div>
            )}
        </div>
    );
}
