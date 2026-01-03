interface SwitchProps {
    checked: boolean;
    onCheckedChange?: (next: boolean) => void;
    disabled?: boolean;
    ariaLabel?: string;
    className?: string;
}

export default function Switch({
    checked,
    onCheckedChange,
    disabled = false,
    ariaLabel,
    className
}: SwitchProps) {
    return (
        <input
            type="checkbox"
            className={`toggle toggle-sm toggle-primary ${className || ''}`}
            checked={checked}
            onChange={(e) => onCheckedChange?.(e.target.checked)}
            disabled={disabled}
            aria-label={ariaLabel}
        />
    );
}
