import clsx from 'clsx';

export type SwitchProps = {
    checked: boolean;
    onCheckedChange: (next: boolean) => void;
    disabled?: boolean;
    ariaLabel: string;
    className?: string;
};

export default function Switch({
    checked,
    onCheckedChange,
    disabled = false,
    ariaLabel,
    className,
}: SwitchProps) {
    return (
        <button
            type="button"
            role="switch"
            aria-checked={checked}
            aria-label={ariaLabel}
            disabled={disabled}
            onClick={() => {
                if (disabled) return;
                onCheckedChange(!checked);
            }}
            className={clsx(
                'relative inline-flex w-10 h-6 items-center rounded-full transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500',
                disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer',
                checked ? 'bg-blue-500' : 'bg-gray-300 dark:bg-base-300',
                className,
            )}
        >
            <span
                className={clsx(
                    'absolute left-1 top-1 bg-white w-4 h-4 rounded-full transition-transform',
                    checked ? 'transform translate-x-4' : '',
                )}
            />
        </button>
    );
}

