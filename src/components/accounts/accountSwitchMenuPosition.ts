export interface RectLike {
    top: number;
    bottom: number;
    left: number;
    right: number;
}

export interface SizeLike {
    width: number;
    height: number;
}

export interface AccountSwitchMenuPosition {
    top: number;
    left: number;
    placement: 'top' | 'bottom';
}

export interface ComputePositionOptions {
    triggerRect: RectLike;
    menuSize: SizeLike;
    viewport: SizeLike;
    /** Visible clipping boundary (scroll/overflow ancestors and table header) */
    clipBounds?: RectLike;
    isRtl?: boolean;
    gap?: number;
    viewportMargin?: number;
}

export const DEFAULT_MENU_GAP = 6;
export const DEFAULT_VIEWPORT_MARGIN = 8;

const CLIPPING_OVERFLOW_VALUES = new Set(['auto', 'scroll', 'hidden', 'clip']);

function clamp(value: number, min: number, max: number): number {
    if (max < min) return min;
    return Math.min(Math.max(value, min), max);
}

/**
 * Compute fixed viewport coordinates and vertical placement (`top` vs `bottom`)
 * for the portaled AccountSwitchMenu so it avoids header/row occlusion and clipping.
 */
export function computeAccountSwitchMenuPosition(
    options: ComputePositionOptions
): AccountSwitchMenuPosition {
    const {
        triggerRect,
        menuSize,
        viewport,
        clipBounds,
        isRtl = false,
        gap = DEFAULT_MENU_GAP,
        viewportMargin = DEFAULT_VIEWPORT_MARGIN,
    } = options;

    const viewportTop = viewportMargin;
    const viewportBottom = Math.max(viewportMargin, viewport.height - viewportMargin);
    const viewportAbove = Math.max(0, triggerRect.top - viewportTop);
    const viewportBelow = Math.max(0, viewportBottom - triggerRect.bottom);

    const effectiveClipTop = clipBounds ? Math.max(viewportTop, clipBounds.top) : viewportTop;
    const effectiveClipBottom = clipBounds ? Math.min(viewportBottom, clipBounds.bottom) : viewportBottom;
    const containerAbove = Math.max(0, triggerRect.top - effectiveClipTop);
    const containerBelow = Math.max(0, effectiveClipBottom - triggerRect.bottom);

    const requiredHeight = menuSize.height + gap;

    let placement: 'top' | 'bottom';
    if (containerAbove >= requiredHeight) {
        placement = 'top';
    } else if (containerBelow >= requiredHeight) {
        placement = 'bottom';
    } else if (viewportAbove >= requiredHeight) {
        placement = 'top';
    } else if (viewportBelow >= requiredHeight) {
        placement = 'bottom';
    } else {
        placement = viewportAbove >= viewportBelow ? 'top' : 'bottom';
    }

    const rawTop =
        placement === 'top'
            ? triggerRect.top - gap - menuSize.height
            : triggerRect.bottom + gap;
    const top = clamp(rawTop, viewportTop, Math.max(viewportTop, viewportBottom - menuSize.height));

    const idealLeft = isRtl
        ? triggerRect.left
        : triggerRect.right - menuSize.width;
    const left = clamp(
        idealLeft,
        viewportMargin,
        Math.max(viewportMargin, viewport.width - viewportMargin - menuSize.width)
    );

    return { top, left, placement };
}

/**
 * Determine whether the trigger button still intersects its visible clipping bounds.
 */
export function isTriggerVisibleInClipBounds(triggerRect: RectLike, clipBounds: RectLike): boolean {
    return (
        triggerRect.bottom > clipBounds.top &&
        triggerRect.top < clipBounds.bottom &&
        triggerRect.right > clipBounds.left &&
        triggerRect.left < clipBounds.right
    );
}

/**
 * Resolve the effective visible clipping rectangle around a trigger element by
 * intersecting scroll/overflow ancestors and any table header directly above it.
 */
export function resolveTriggerClipBounds(
    trigger: HTMLElement,
    triggerRect: RectLike,
    viewport: SizeLike
): RectLike {
    const bounds: RectLike = {
        top: 0,
        left: 0,
        bottom: viewport.height,
        right: viewport.width,
    };

    let current: HTMLElement | null = trigger.parentElement;
    while (current && current !== document.body && current !== document.documentElement) {
        const style = window.getComputedStyle(current);
        const clipsY = CLIPPING_OVERFLOW_VALUES.has(style.overflowY);
        const clipsX = CLIPPING_OVERFLOW_VALUES.has(style.overflowX);

        if (clipsY || clipsX) {
            const rect = current.getBoundingClientRect();
            if (clipsY) {
                bounds.top = Math.max(bounds.top, rect.top);
                bounds.bottom = Math.min(bounds.bottom, rect.bottom);
            }
            if (clipsX) {
                bounds.left = Math.max(bounds.left, rect.left);
                bounds.right = Math.min(bounds.right, rect.right);
            }
        }
        current = current.parentElement;
    }

    const table = trigger.closest('table');
    const thead = table?.querySelector('thead');
    if (thead) {
        const theadRect = thead.getBoundingClientRect();
        if (theadRect.height > 0 && theadRect.bottom <= triggerRect.top + 1) {
            bounds.top = Math.max(bounds.top, theadRect.bottom);
        }
    }

    return bounds;
}
