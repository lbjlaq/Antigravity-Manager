import { useEffect, useState } from 'react';
import { ZoomIn, ZoomOut, RotateCcw } from 'lucide-react';
import {
  DEFAULT_ZOOM,
  getStoredZoom,
  applyZoom,
  zoomIn,
  zoomOut,
  resetZoom,
  snapToClosestZoom,
  isMiniWindow,
} from '../../utils/zoom';
import { hydrateUiStateFromConfig, saveUiZoom } from '../../utils/uiState';

function isMacLike(): boolean {
  if (typeof navigator === 'undefined') return false;
  return /Mac|iPhone|iPad|iPod/i.test(navigator.platform || navigator.userAgent || '');
}

export function ZoomManager() {
  const [hudVisible, setHudVisible] = useState(false);
  const [currentZoom, setCurrentZoom] = useState<number>(() => getStoredZoom());

  // 挂载时立即应用本地缩放，再从 config.json 水合（与无头/CLI 配置文件一致）
  useEffect(() => {
    if (isMiniWindow()) return;

    const initial = getStoredZoom();
    applyZoom(initial, false);
    setCurrentZoom(initial);

    let disposed = false;
    void hydrateUiStateFromConfig().then((hydrated) => {
      if (disposed || typeof hydrated?.uiZoom !== 'number' || !Number.isFinite(hydrated.uiZoom)) {
        return;
      }
      const snapped = snapToClosestZoom(hydrated.uiZoom);
      applyZoom(snapped, true);
      setCurrentZoom(snapped);
    });

    return () => {
      disposed = true;
    };
  }, []);

  // 监听外部（如设置页下拉/按钮）触发的缩放变更
  useEffect(() => {
    const handleZoomChange = (e: Event) => {
      const custom = e as CustomEvent<number>;
      if (typeof custom.detail === 'number' && Number.isFinite(custom.detail)) {
        setCurrentZoom(custom.detail);
      }
    };

    window.addEventListener('app-ui-zoom-change', handleZoomChange);
    return () => window.removeEventListener('app-ui-zoom-change', handleZoomChange);
  }, []);

  // 全局快捷键：Cmd/Ctrl + (+/=), Cmd/Ctrl + (-), Cmd/Ctrl + (0)
  useEffect(() => {
    if (isMiniWindow()) return;

    let timer: ReturnType<typeof setTimeout> | null = null;
    const macLike = isMacLike();

    const triggerHud = (level: number) => {
      setCurrentZoom(level);
      setHudVisible(true);
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        setHudVisible(false);
      }, 1200);
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.repeat) return;

      const hasZoomModifier = macLike ? e.metaKey : e.ctrlKey;
      if (!hasZoomModifier || e.altKey) return;

      const target = e.target as HTMLElement | null;
      if (target) {
        const tagName = target.tagName;
        if (tagName === 'INPUT' || tagName === 'TEXTAREA' || target.isContentEditable) {
          return;
        }
      }

      if (e.key === '=' || e.key === '+') {
        e.preventDefault();
        const next = zoomIn();
        saveUiZoom(next);
        triggerHud(next);
      } else if (e.key === '-' || e.key === '_') {
        e.preventDefault();
        const prev = zoomOut();
        saveUiZoom(prev);
        triggerHud(prev);
      } else if (e.key === '0') {
        e.preventDefault();
        const reset = resetZoom();
        saveUiZoom(reset);
        triggerHud(reset);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
      if (timer) clearTimeout(timer);
    };
  }, []);

  if (!hudVisible || isMiniWindow()) return null;

  return (
    <div className="fixed top-6 left-1/2 -translate-x-1/2 z-[99999] pointer-events-none animate-in fade-in zoom-in-95 duration-200">
      <div className="flex items-center gap-2 px-4 py-2 rounded-full bg-gray-900/90 dark:bg-black/90 backdrop-blur-md text-white text-xs font-semibold shadow-2xl border border-white/10">
        {currentZoom > DEFAULT_ZOOM ? (
          <ZoomIn className="w-3.5 h-3.5 text-blue-400" />
        ) : currentZoom < DEFAULT_ZOOM ? (
          <ZoomOut className="w-3.5 h-3.5 text-orange-400" />
        ) : (
          <RotateCcw className="w-3.5 h-3.5 text-gray-400" />
        )}
        <span>{currentZoom}%</span>
      </div>
    </div>
  );
}

export default ZoomManager;
