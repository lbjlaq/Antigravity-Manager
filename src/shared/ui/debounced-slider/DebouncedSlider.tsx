import { useState, useEffect } from 'react';

interface DebouncedSliderProps {
    value: number;
    onChange: (value: number) => void;
    min: number;
    max: number;
    step: number;
    className?: string; // For passing 'range range-purple range-xs' etc.
}

export const DebouncedSlider = ({ value, onChange, min, max, step, className }: DebouncedSliderProps) => {
    const [localValue, setLocalValue] = useState(value);
    const [isDragging, setIsDragging] = useState(false);

    // Sync local value with prop value when not dragging (for external updates)
    useEffect(() => {
        if (!isDragging) {
            setLocalValue(value);
        }
    }, [value, isDragging]);

    const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
        setLocalValue(parseFloat(e.target.value));
    };

    const handlePointerDown = () => {
        setIsDragging(true);
    };

    const handlePointerUp = (e: React.PointerEvent<HTMLInputElement>) => {
        setIsDragging(false);
        const newValue = parseFloat((e.target as HTMLInputElement).value);
        onChange(newValue);
    };

    return (
        <div className="flex items-center gap-3 w-full">
            <input
                type="range"
                min={min}
                max={max}
                step={step}
                className={className}
                value={localValue}
                onChange={handleChange}
                onPointerDown={handlePointerDown}
                onPointerUp={handlePointerUp}
            />
            <span className="text-xs font-mono font-bold text-purple-600 dark:text-purple-400 w-10 text-right">
                {Math.round(localValue * 100)}%
            </span>
        </div>
    );
};
