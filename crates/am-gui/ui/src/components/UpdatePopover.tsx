import React from 'react';
import {
  ArrowUpCircle,
  CheckCircle2,
  Download,
  ExternalLink,
  Loader2,
  RotateCcw,
  Terminal,
  X,
} from 'lucide-react';
import { UpdateSnapshot, api } from '../api';
import { tUi } from '../i18n/ui';
import { ReleaseNotesView } from './ReleaseNotesView';

interface UpdatePopoverProps {
  lang: string;
  state: UpdateSnapshot;
  open: boolean;
  onClose: () => void;
  onDownload: (forceNative?: boolean) => void;
  onRestart: () => void;
  onSkip: (version: string) => void;
}

function formatBytes(bytes: number): string {
  if (bytes <= 0) return '0 MB';
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export const UpdatePopover: React.FC<UpdatePopoverProps> = ({
  lang,
  state,
  open,
  onClose,
  onDownload,
  onRestart,
  onSkip,
}) => {
  if (!open || !state.info) return null;

  const { info, phase, install_channel, progress_pct, downloaded_bytes, total_bytes, error, updated_via_brew } =
    state;

  const isDownloading = phase === 'downloading';
  const isInstalling = phase === 'installing';
  const isReady = phase === 'ready';
  const isError = phase === 'error';
  const isHomebrew = install_channel === 'homebrew';
  const isManual = install_channel === 'manual';

  return (
    <div
      role="dialog"
      aria-label={tUi(lang, isReady ? 'update_banner_ready_title' : 'update_banner_title')}
      className="fixed top-[68px] right-4 z-[90] w-[392px] max-w-[calc(100vw-2rem)] rounded-2xl bg-white/95 dark:bg-[#16171b]/95 backdrop-blur-xl border border-gray-200/80 dark:border-white/10 shadow-[0_12px_40px_rgba(0,0,0,0.14)] dark:shadow-[0_16px_48px_rgba(0,0,0,0.48)] p-4 text-left animate-in fade-in slide-in-from-top-2 duration-200"
    >
      {/* Header */}
      <div className="flex items-start justify-between gap-3">
        <div className="flex items-center gap-2.5 min-w-0">
          <div
            className={`w-9 h-9 rounded-xl flex items-center justify-center shrink-0 ${
              isReady
                ? 'bg-emerald-500/12 text-emerald-600 dark:text-emerald-400'
                : isError
                  ? 'bg-rose-500/12 text-rose-600 dark:text-rose-400'
                  : 'bg-blue-500/12 text-blue-600 dark:text-blue-400'
            }`}
          >
            {isReady ? (
              <CheckCircle2 className="w-5 h-5" />
            ) : (
              <ArrowUpCircle className="w-5 h-5" />
            )}
          </div>
          <div className="min-w-0">
            <div className="text-[13px] font-semibold text-gray-900 dark:text-zinc-100 leading-tight">
              {tUi(lang, isReady ? 'update_banner_ready_title' : 'update_banner_title')}
            </div>
            <div className="mt-0.5 flex items-center gap-1.5 text-[11px] font-mono text-gray-500 dark:text-zinc-400">
              <span>v{info.current_version}</span>
              <span className="opacity-50">→</span>
              <span className="font-semibold text-emerald-600 dark:text-emerald-400">
                v{info.latest_version}
              </span>
              {info.published_at && (
                <span className="text-[10px] text-gray-400 dark:text-zinc-500 ml-1">
                  {info.published_at.slice(0, 10)}
                </span>
              )}
            </div>
          </div>
        </div>

        {!isInstalling && (
          <button
            type="button"
            onClick={onClose}
            className="p-1 rounded-lg text-gray-400 hover:text-gray-600 dark:text-zinc-500 dark:hover:text-zinc-200 hover:bg-gray-100 dark:hover:bg-white/10 transition-colors cursor-pointer"
            aria-label={tUi(lang, 'close')}
          >
            <X className="w-4 h-4" />
          </button>
        )}
      </div>

      {/* Body: Ready / Downloading / Installing / Manual / Release Notes */}
      {isReady && (
        <p className="mt-3 text-[12px] leading-relaxed text-gray-600 dark:text-zinc-300">
          {tUi(lang, 'update_banner_ready_desc', { version: info.latest_version })}
        </p>
      )}

      {isManual && !isDownloading && !isReady && !isError && (
        <p className="mt-3 text-[12px] leading-relaxed text-amber-700 dark:text-amber-300 bg-amber-50/80 dark:bg-amber-500/10 border border-amber-200/70 dark:border-amber-500/20 rounded-xl px-3 py-2">
          {tUi(lang, 'settings_update_linux_manual')}
        </p>
      )}

      {(isDownloading || isInstalling) && (
        <div className="mt-3.5 space-y-2">
          <div className="flex items-center justify-between text-[11px] text-gray-600 dark:text-zinc-300">
            <span className="inline-flex items-center gap-1.5 font-medium">
              <Loader2 className="w-3.5 h-3.5 animate-spin text-blue-500" />
              {isInstalling
                ? tUi(lang, 'settings_update_applying')
                : updated_via_brew
                  ? tUi(lang, 'settings_update_brew_upgrading')
                  : tUi(lang, 'update_banner_downloading')}
            </span>
            {!isInstalling && !updated_via_brew && (
              <span className="font-mono text-[10.5px] text-gray-500 dark:text-zinc-400">
                {total_bytes > 0
                  ? `${formatBytes(downloaded_bytes)} / ${formatBytes(total_bytes)} · ${progress_pct}%`
                  : progress_pct > 0
                    ? `${progress_pct}%`
                    : '...'}
              </span>
            )}
          </div>
          <div className="w-full h-1.5 rounded-full bg-gray-200/80 dark:bg-white/10 overflow-hidden">
            {isInstalling || updated_via_brew || total_bytes === 0 ? (
              <div className="h-full w-2/5 rounded-full bg-blue-500 animate-pulse" />
            ) : (
              <div
                className="h-full rounded-full bg-emerald-500 transition-all duration-200"
                style={{ width: `${Math.max(4, Math.min(100, progress_pct))}%` }}
              />
            )}
          </div>
        </div>
      )}

      {isError && error && (
        <div className="mt-3 rounded-xl bg-rose-50/90 dark:bg-rose-500/10 border border-rose-200/70 dark:border-rose-500/20 px-3 py-2 text-[11px] leading-relaxed text-rose-700 dark:text-rose-300 break-words">
          {error}
        </div>
      )}

      {!isDownloading && !isInstalling && info.release_notes.trim().length > 0 && (
        <ReleaseNotesView
          notes={info.release_notes}
          lang={lang}
          title={tUi(lang, 'settings_update_release_notes', { version: info.latest_version })}
          variant="compact"
        />
      )}

      {/* Footer Actions */}
      {!isDownloading && !isInstalling && (
        <div className="mt-3.5 space-y-2">
          {isReady ? (
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={onRestart}
                className="flex-1 h-8 px-3 rounded-xl bg-emerald-500 hover:bg-emerald-600 text-white text-xs font-semibold inline-flex items-center justify-center gap-1.5 transition-colors cursor-pointer shadow-xs"
              >
                <RotateCcw className="w-3.5 h-3.5" />
                <span>{tUi(lang, 'update_banner_btn_restart')}</span>
              </button>
              <button
                type="button"
                onClick={onClose}
                className="h-8 px-3 rounded-xl bg-gray-100 hover:bg-gray-200 dark:bg-white/[0.06] dark:hover:bg-white/10 text-gray-700 dark:text-zinc-200 text-xs font-medium transition-colors cursor-pointer"
              >
                {tUi(lang, 'update_banner_btn_later')}
              </button>
            </div>
          ) : isManual ? (
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={() => void api.openExternalUrl(info.download_url)}
                className="flex-1 h-8 px-3 rounded-xl bg-blue-500 hover:bg-blue-600 text-white text-xs font-semibold inline-flex items-center justify-center gap-1.5 transition-colors cursor-pointer shadow-xs"
              >
                <ExternalLink className="w-3.5 h-3.5" />
                <span>{tUi(lang, 'settings_update_manual_btn', { version: info.latest_version })}</span>
              </button>
              <button
                type="button"
                onClick={() => onSkip(info.latest_version)}
                className="h-8 px-2.5 rounded-xl text-gray-500 hover:text-gray-700 dark:text-zinc-400 dark:hover:text-zinc-200 hover:bg-gray-100 dark:hover:bg-white/[0.06] text-[11px] font-medium transition-colors cursor-pointer"
              >
                {tUi(lang, 'update_banner_btn_skip')}
              </button>
            </div>
          ) : isError ? (
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={() => onDownload(false)}
                className="flex-1 h-8 px-3 rounded-xl bg-blue-500 hover:bg-blue-600 text-white text-xs font-semibold inline-flex items-center justify-center gap-1.5 transition-colors cursor-pointer"
              >
                <RotateCcw className="w-3.5 h-3.5" />
                <span>{tUi(lang, 'update_banner_btn_retry')}</span>
              </button>
              <button
                type="button"
                onClick={() => void api.openExternalUrl(info.download_url)}
                className="h-8 px-3 rounded-xl bg-gray-100 hover:bg-gray-200 dark:bg-white/[0.06] dark:hover:bg-white/10 text-gray-700 dark:text-zinc-200 text-xs font-medium inline-flex items-center gap-1 transition-colors cursor-pointer"
              >
                <ExternalLink className="w-3.5 h-3.5" />
                <span>GitHub</span>
              </button>
            </div>
          ) : (
            <>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => onDownload(false)}
                  className="flex-1 h-8 px-3 rounded-xl bg-emerald-500 hover:bg-emerald-600 text-white text-xs font-semibold inline-flex items-center justify-center gap-1.5 transition-colors cursor-pointer shadow-xs"
                >
                  {isHomebrew ? (
                    <Terminal className="w-3.5 h-3.5" />
                  ) : (
                    <Download className="w-3.5 h-3.5" />
                  )}
                  <span>
                    {isHomebrew
                      ? tUi(lang, 'settings_update_brew_btn')
                      : tUi(lang, 'update_banner_btn_now')}
                  </span>
                </button>
                <button
                  type="button"
                  onClick={() => onSkip(info.latest_version)}
                  className="h-8 px-2.5 rounded-xl text-gray-500 hover:text-gray-700 dark:text-zinc-400 dark:hover:text-zinc-200 hover:bg-gray-100 dark:hover:bg-white/[0.06] text-[11px] font-medium transition-colors cursor-pointer"
                >
                  {tUi(lang, 'update_banner_btn_skip')}
                </button>
                <button
                  type="button"
                  onClick={() => void api.openExternalUrl(info.download_url)}
                  title={tUi(lang, 'settings_update_manual_download')}
                  className="w-8 h-8 rounded-xl bg-gray-100 hover:bg-gray-200 dark:bg-white/[0.06] dark:hover:bg-white/10 text-gray-600 dark:text-zinc-300 flex items-center justify-center transition-colors cursor-pointer shrink-0"
                >
                  <ExternalLink className="w-3.5 h-3.5" />
                </button>
              </div>
              {isHomebrew && (
                <button
                  type="button"
                  onClick={() => onDownload(true)}
                  className="w-full text-center text-[11px] text-blue-600 dark:text-blue-400 hover:underline cursor-pointer pt-0.5"
                >
                  {tUi(lang, 'settings_update_switch_native')}
                </button>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
};
