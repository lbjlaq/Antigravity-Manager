import { useEffect, useState } from 'react';
import { getStoredZoom, applyZoom, zoomIn, zoomOut, resetZoom } from '../../utils/zoom';
import { ZoomIn, ZoomOut, RotateCcw } from 'lucide-react';

export default function ZoomManager() {
    const [hudVisible, setHudVisible] = useState(false);
    const [currentZoom, setCurrentZoom] = useState(getStoredZoom());

    // Apply stored zoom on mount
    useEffect(() => {
        const initial = getStoredZoom();
        applyZoom(initial, false);
        setCurrentZoom(initial);
    }, []);

    // Listen for zoom changes triggered elsewhere
    useEffect(() => {
        const handleZoomChange = (e: Event) => {
            const custom = e as CustomEvent<number>;
            if (custom.detail) {
                setCurrentZoom(custom.detail);
            }
        };

        window.addEventListener('app-ui-zoom-change', handleZoomChange);
        return () => window.removeEventListener('app-ui-zoom-change', handleZoomChange);
    }, []);

    // Global keyboard shortcuts: Cmd/Ctrl + (+/=), Cmd/Ctrl + (-), Cmd/Ctrl + (0)
    useEffect(() => {
        let timer: ReturnType<typeof setTimeout> | null = null;

        const triggerHud = (level: number) => {
            setCurrentZoom(level);
            setHudVisible(true);
            if (timer) clearTimeout(timer);
            timer = setTimeout(() => {
                setHudVisible(false);
            }, 1200);
        };

        const handleKeyDown = (e: KeyboardEvent) => {
            // Ignore key repeats to prevent continuous writes & layout thrashing
            if (e.repeat) return;

            // Must have Cmd or Ctrl, but NOT Alt to avoid conflicting with custom hotkeys like Cmd+Alt+-
            const isModifier = (e.metaKey || e.ctrlKey) && !e.altKey;
            if (!isModifier) return;

            // Do not intercept when typing in editable inputs, textareas, or contentEditable areas
            const target = e.target as HTMLElement | null;
            if (target) {
                const tagName = target.tagName;
                if (tagName === 'INPUT' || tagName === 'TEXTAREA' || target.isContentEditable) {
                    return;
                }
            }

            // Zoom in: Cmd/Ctrl + = or Cmd/Ctrl + +
            if (e.key === '=' || e.key === '+') {
                e.preventDefault();
                const next = zoomIn();
                triggerHud(next);
            }
            // Zoom out: Cmd/Ctrl + - or Cmd/Ctrl + _
            else if (e.key === '-' || e.key === '_') {
                e.preventDefault();
                const prev = zoomOut();
                triggerHud(prev);
            }
            // Reset: Cmd/Ctrl + 0
            else if (e.key === '0') {
                e.preventDefault();
                const reset = resetZoom();
                triggerHud(reset);
            }
        };

        window.addEventListener('keydown', handleKeyDown);
        return () => {
            window.removeEventListener('keydown', handleKeyDown);
            if (timer) clearTimeout(timer);
        };
    }, []);

    if (!hudVisible) return null;

    return (
        <div className="fixed top-6 left-1/2 -translate-x-1/2 z-[99999] pointer-events-none animate-in fade-in zoom-in-95 duration-200">
            <div className="flex items-center gap-2 px-4 py-2 rounded-full bg-gray-900/90 dark:bg-black/90 backdrop-blur-md text-white text-xs font-semibold shadow-2xl border border-white/10">
                {currentZoom > 100 ? (
                    <ZoomIn className="w-3.5 h-3.5 text-blue-400" />
                ) : currentZoom < 100 ? (
                    <ZoomOut className="w-3.5 h-3.5 text-orange-400" />
                ) : (
                    <RotateCcw className="w-3.5 h-3.5 text-gray-400" />
                )}
                <span>{currentZoom}%</span>
            </div>
        </div>
    );
}
