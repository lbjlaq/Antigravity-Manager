// File: src/shared/ui/help-tooltip/HelpTooltip.tsx
// Help tooltip component

import { CircleHelp } from 'lucide-react';

export type HelpTooltipPlacement = 'top' | 'right' | 'bottom' | 'left';

export interface HelpTooltipProps {
    text: string;
    placement?: HelpTooltipPlacement;
    ariaLabel?: string;
    iconSize?: number;
    className?: string;
}

const placementClasses: Record<HelpTooltipPlacement, string> = {
    top: 'bottom-full mb-2 left-1/2 -translate-x-1/2',
    right: 'left-full ml-2 top-1/2 -translate-y-1/2',
    bottom: 'top-full mt-2 left-1/2 -translate-x-1/2',
    left: 'right-full mr-2 top-1/2 -translate-y-1/2',
};

export function HelpTooltip({
    text,
    placement = 'top',
    ariaLabel = 'Help',
    iconSize = 14,
    className,
}: HelpTooltipProps) {
    if (!text) return null;

    return (
        <span className={`relative inline-flex items-center group ${className || ''}`}>
            <button
                type="button"
                className="inline-flex items-center justify-center text-zinc-400 hover:text-zinc-700 dark:hover:text-zinc-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 rounded"
                aria-label={ariaLabel}
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
            <span
                className={`pointer-events-none absolute z-50 ${placementClasses[placement]} w-80 max-w-[90vw] sm:max-w-xs rounded-md bg-zinc-900 text-white text-[11px] leading-snug px-2 py-1 shadow-lg opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 transition-opacity duration-150`}
            >
                {text}
            </span>
        </span>
    );
}

export default HelpTooltip;
