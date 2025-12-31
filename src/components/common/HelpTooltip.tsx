import { CircleHelp } from 'lucide-react';
import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

export type HelpTooltipPlacement = 'top' | 'right' | 'bottom' | 'left';

export type HelpTooltipProps = {
    text: string;
    placement?: HelpTooltipPlacement;
    ariaLabel?: string;
    iconSize?: number;
    maxWidth?: number;
    className?: string;
};

type TooltipPosition = {
    left: number;
    top: number;
    placement: HelpTooltipPlacement;
    maxWidth: number;
};

const DEFAULT_MAX_WIDTH = 320;
const VIEWPORT_MARGIN = 12;
const OFFSET = 8;

function clamp(n: number, min: number, max: number) {
    return Math.max(min, Math.min(max, n));
}

function candidatePlacements(preferred: HelpTooltipPlacement): HelpTooltipPlacement[] {
    const others: HelpTooltipPlacement[] = ['top', 'right', 'bottom', 'left'].filter(
        (p) => p !== preferred,
    ) as HelpTooltipPlacement[];
    return [preferred, ...others];
}

function computeUnclampedPosition(
    anchor: DOMRect,
    tooltipW: number,
    tooltipH: number,
    placement: HelpTooltipPlacement,
): { left: number; top: number } {
    switch (placement) {
        case 'right':
            return {
                left: anchor.right + OFFSET,
                top: anchor.top + anchor.height / 2 - tooltipH / 2,
            };
        case 'left':
            return {
                left: anchor.left - OFFSET - tooltipW,
                top: anchor.top + anchor.height / 2 - tooltipH / 2,
            };
        case 'bottom':
            return {
                left: anchor.left + anchor.width / 2 - tooltipW / 2,
                top: anchor.bottom + OFFSET,
            };
        case 'top':
        default:
            return {
                left: anchor.left + anchor.width / 2 - tooltipW / 2,
                top: anchor.top - OFFSET - tooltipH,
            };
    }
}

function overflowScore(left: number, top: number, w: number, h: number, vw: number, vh: number) {
    const minX = VIEWPORT_MARGIN;
    const minY = VIEWPORT_MARGIN;
    const maxX = vw - VIEWPORT_MARGIN;
    const maxY = vh - VIEWPORT_MARGIN;

    const overLeft = Math.max(0, minX - left);
    const overTop = Math.max(0, minY - top);
    const overRight = Math.max(0, left + w - maxX);
    const overBottom = Math.max(0, top + h - maxY);
    return overLeft + overTop + overRight + overBottom;
}

export default function HelpTooltip({
    text,
    placement = 'top',
    ariaLabel = 'Help',
    iconSize = 14,
    maxWidth = DEFAULT_MAX_WIDTH,
    className,
}: HelpTooltipProps) {
    if (!text) return null;

    const id = useId();
    const buttonRef = useRef<HTMLButtonElement | null>(null);
    const tooltipRef = useRef<HTMLDivElement | null>(null);
    const [open, setOpen] = useState(false);
    const [pos, setPos] = useState<TooltipPosition | null>(null);

    const preferredOrder = useMemo(() => candidatePlacements(placement), [placement]);

    const updatePosition = () => {
        const btn = buttonRef.current;
        const tip = tooltipRef.current;
        if (!btn || !tip) return;

        const anchor = btn.getBoundingClientRect();
        const vw = window.innerWidth;
        const vh = window.innerHeight;

        const effectiveMaxWidth = clamp(maxWidth, 160, Math.max(160, vw - VIEWPORT_MARGIN * 2));
        tip.style.maxWidth = `${effectiveMaxWidth}px`;

        // Measure after maxWidth is applied.
        const tipRect = tip.getBoundingClientRect();
        const w = tipRect.width;
        const h = tipRect.height;

        let bestPlacement = preferredOrder[0];
        let bestLeft = 0;
        let bestTop = 0;
        let bestScore = Number.POSITIVE_INFINITY;

        for (const p of preferredOrder) {
            const { left, top } = computeUnclampedPosition(anchor, w, h, p);
            const score = overflowScore(left, top, w, h, vw, vh);
            if (score < bestScore) {
                bestScore = score;
                bestPlacement = p;
                bestLeft = left;
                bestTop = top;
                if (score === 0) break;
            }
        }

        const minLeft = VIEWPORT_MARGIN;
        const minTop = VIEWPORT_MARGIN;
        const maxLeft = vw - VIEWPORT_MARGIN - w;
        const maxTop = vh - VIEWPORT_MARGIN - h;

        setPos({
            left: clamp(bestLeft, minLeft, maxLeft),
            top: clamp(bestTop, minTop, maxTop),
            placement: bestPlacement,
            maxWidth: effectiveMaxWidth,
        });
    };

    useEffect(() => {
        if (!open) return;
        // Next frame ensures the portal content is in the DOM before measuring.
        const raf = requestAnimationFrame(updatePosition);
        const onScrollOrResize = () => updatePosition();

        window.addEventListener('resize', onScrollOrResize);
        // Capture scroll events from any scrollable container.
        window.addEventListener('scroll', onScrollOrResize, true);

        return () => {
            cancelAnimationFrame(raf);
            window.removeEventListener('resize', onScrollOrResize);
            window.removeEventListener('scroll', onScrollOrResize, true);
        };
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [open, text, maxWidth, placement]);

    return (
        <span className={`relative inline-flex items-center ${className || ''}`}>
            <button
                type="button"
                ref={buttonRef}
                className="inline-flex items-center justify-center text-gray-400 hover:text-gray-700 dark:hover:text-gray-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 rounded"
                aria-label={ariaLabel}
                aria-describedby={open ? id : undefined}
                onMouseEnter={() => setOpen(true)}
                onMouseLeave={() => setOpen(false)}
                onFocus={() => setOpen(true)}
                onBlur={() => setOpen(false)}
                onClick={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                }}
                onMouseDown={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                }}
            >
                <CircleHelp size={iconSize} />
            </button>
            {open &&
                createPortal(
                    <div
                        id={id}
                        ref={tooltipRef}
                        role="tooltip"
                        className="pointer-events-none fixed z-[1000] rounded-md bg-gray-900 text-white text-[11px] leading-snug px-2 py-1 shadow-lg"
                        style={{
                            left: pos?.left ?? -9999,
                            top: pos?.top ?? -9999,
                            maxWidth: `${pos?.maxWidth ?? maxWidth}px`,
                            wordBreak: 'break-word',
                            whiteSpace: 'pre-wrap',
                        }}
                    >
                        {text}
                    </div>,
                    document.body,
                )}
        </span>
    );
}
