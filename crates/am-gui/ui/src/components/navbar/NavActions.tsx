import React from 'react';
import { Sun, Moon, ArrowUpCircle, Loader2, RotateCcw } from 'lucide-react';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { LANGUAGES, LangCode } from '../../i18n/constants';
import { tUi } from '../../i18n/ui';
import { UpdateSnapshot } from '../../api';

interface NavActionsProps {
  theme: 'light' | 'dark';
  onThemeToggle: (e: React.MouseEvent<HTMLButtonElement>) => void;
  lang: string;
  onLanguageChange: (lang: LangCode) => void;
  updateState?: UpdateSnapshot | null;
  onUpdatePillClick?: () => void;
}

export const NavActions: React.FC<NavActionsProps> = ({
  theme,
  onThemeToggle,
  lang,
  onLanguageChange,
  updateState,
  onUpdatePillClick,
}) => {
  const currentLangObj = LANGUAGES.find((l) => l.code === lang) || LANGUAGES[0];

  const showUpdatePill =
    !!updateState?.info?.has_update &&
    (updateState.phase === 'available' ||
      updateState.phase === 'downloading' ||
      updateState.phase === 'installing' ||
      updateState.phase === 'ready' ||
      updateState.phase === 'error');

  return (
    <div className="flex items-center gap-2 select-none">
      {/* 极简更新状态胶囊（仅发现新版本 / 下载中 / 待重启时呈现） */}
      {showUpdatePill && updateState?.info && (
        <button
          type="button"
          onClick={onUpdatePillClick}
          className={`h-8 px-2.5 rounded-full text-[11px] font-semibold inline-flex items-center gap-1.5 transition-all duration-150 cursor-pointer border ${
            updateState.phase === 'ready'
              ? 'bg-emerald-500 hover:bg-emerald-600 text-white border-emerald-500/80 shadow-xs'
              : updateState.phase === 'error'
                ? 'bg-rose-500/10 hover:bg-rose-500/15 text-rose-600 dark:text-rose-400 border-rose-500/25'
                : 'bg-emerald-500/10 hover:bg-emerald-500/18 text-emerald-700 dark:text-emerald-400 border-emerald-500/25'
          }`}
          title={tUi(
            lang,
            updateState.phase === 'ready' ? 'update_banner_ready_title' : 'update_banner_title'
          )}
        >
          {updateState.phase === 'downloading' || updateState.phase === 'installing' ? (
            <>
              <Loader2 className="w-3.5 h-3.5 animate-spin shrink-0" />
              <span className="font-mono">
                {updateState.phase === 'installing'
                  ? '...'
                  : updateState.progress_pct > 0
                    ? `${updateState.progress_pct}%`
                    : `v${updateState.info.latest_version}`}
              </span>
            </>
          ) : updateState.phase === 'ready' ? (
            <>
              <RotateCcw className="w-3.5 h-3.5 shrink-0" />
              <span>{tUi(lang, 'update_nav_ready')}</span>
            </>
          ) : (
            <>
              <ArrowUpCircle className="w-3.5 h-3.5 shrink-0" />
              <span className="font-mono">v{updateState.info.latest_version}</span>
            </>
          )}
        </button>
      )}

      {/* 语言选择下拉菜单 (Radix UI) - 保持与主分支一致的胶囊文字与精致下拉 */}
      <DropdownMenu.Root>
        <DropdownMenu.Trigger asChild>
          <button
            className="w-10 h-10 rounded-full bg-gray-100 dark:bg-[#15161b] hover:bg-gray-200 dark:hover:bg-[#26272f] flex items-center justify-center transition-colors duration-150 border border-transparent dark:border-[#26272f]/40 outline-none cursor-pointer"
            title={currentLangObj.label}
          >
            <span className="text-xs font-bold font-mono text-gray-700 dark:text-gray-300">
              {currentLangObj.short}
            </span>
          </button>
        </DropdownMenu.Trigger>

        <DropdownMenu.Portal>
          <DropdownMenu.Content
            align="end"
            sideOffset={8}
            className="w-40 max-h-[380px] overflow-y-auto py-1.5 bg-white dark:bg-[#15161b] border border-gray-200 dark:border-zinc-800 rounded-2xl shadow-xl z-50 animate-in fade-in-0 zoom-in-95 duration-100 ease-out"
          >
            {LANGUAGES.map((item) => {
              const isSelected = item.code === lang;
              return (
                <DropdownMenu.Item
                  key={item.code}
                  onClick={() => onLanguageChange(item.code as LangCode)}
                  className={`px-3.5 py-2 text-xs rounded-xl mx-1 flex items-center justify-between cursor-pointer outline-none transition-colors ${
                    isSelected
                      ? 'bg-blue-50 text-blue-600 dark:bg-blue-500/10 dark:text-blue-400 font-semibold'
                      : 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-zinc-800'
                  }`}
                >
                  <div className="flex items-center gap-3">
                    <span className="font-mono font-bold w-6 text-xs">{item.short}</span>
                    <span className="text-xs opacity-80">{item.label}</span>
                  </div>
                  {isSelected && (
                    <span className="w-1.5 h-1.5 rounded-full bg-blue-500 shrink-0"></span>
                  )}
                </DropdownMenu.Item>
              );
            })}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>

      {/* 主题切换圆钮 */}
      <button
        onClick={onThemeToggle}
        className="w-10 h-10 rounded-full bg-gray-100 dark:bg-[#15161b] hover:bg-gray-200 dark:hover:bg-[#26272f] flex items-center justify-center transition-colors duration-150 border border-transparent dark:border-[#26272f]/40 outline-none cursor-pointer"
        title={theme === 'light' ? tUi(lang, 'nav_switch_dark') : tUi(lang, 'nav_switch_light')}
      >
        {theme === 'light' ? (
          <Moon className="w-5 h-5 text-gray-700 dark:text-gray-300" />
        ) : (
          <Sun className="w-5 h-5 text-amber-400" />
        )}
      </button>
    </div>
  );
};
