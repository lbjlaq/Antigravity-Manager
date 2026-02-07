import { useState, useRef, useEffect } from 'react';
import { createPortal } from 'react-dom';
import { ChevronDown, Check } from 'lucide-react';
import { cn } from '@/shared/lib';

export interface SelectOption {
    value: string;
    label: string;
    group?: string;
}

interface GroupedSelectProps {
    value: string;
    onChange: (value: string) => void;
    options: SelectOption[];
    placeholder?: string;
    className?: string;
    disabled?: boolean;
}

export const GroupedSelect = ({
    value,
    onChange,
    options,
    placeholder = 'Select...',
    className = '',
    disabled = false
}: GroupedSelectProps) => {
    const [isOpen, setIsOpen] = useState(false);
    const [dropdownPosition, setDropdownPosition] = useState({ top: 0, left: 0, width: 0 });
    const containerRef = useRef<HTMLDivElement>(null);
    const buttonRef = useRef<HTMLButtonElement>(null);
    const dropdownRef = useRef<HTMLDivElement>(null);

    // Group options
    const groupedOptions = options.reduce((acc, option) => {
        const group = option.group || 'Other';
        if (!acc[group]) {
            acc[group] = [];
        }
        acc[group].push(option);
        return acc;
    }, {} as Record<string, SelectOption[]>);

    // Get selected label
    const selectedOption = options.find(opt => opt.value === value);
    const selectedLabel = selectedOption?.label || placeholder;

    // Update dropdown position
    const updateDropdownPosition = () => {
        if (buttonRef.current) {
            const rect = buttonRef.current.getBoundingClientRect();
            setDropdownPosition({
                top: rect.bottom + window.scrollY + 4,
                left: rect.left + window.scrollX,
                width: Math.max(rect.width * 1.1, 220) // 1.1x width, min 220px
            });
        }
    };

    // Click outside handler
    useEffect(() => {
        const handleClickOutside = (event: MouseEvent) => {
            const target = event.target as Node;
            const isClickInsideContainer = containerRef.current?.contains(target);
            const isClickInsideDropdown = dropdownRef.current?.contains(target);

            if (!isClickInsideContainer && !isClickInsideDropdown) {
                setIsOpen(false);
            }
        };

        if (isOpen) {
            updateDropdownPosition();
            document.addEventListener('mousedown', handleClickOutside);
            window.addEventListener('scroll', updateDropdownPosition, true);
            window.addEventListener('resize', updateDropdownPosition);
        }

        return () => {
            document.removeEventListener('mousedown', handleClickOutside);
            window.removeEventListener('scroll', updateDropdownPosition, true);
            window.removeEventListener('resize', updateDropdownPosition);
        };
    }, [isOpen]);

    const handleSelect = (optionValue: string) => {
        onChange(optionValue);
        setIsOpen(false);
    };

    const handleToggle = () => {
        if (!disabled) {
            setIsOpen(!isOpen);
            if (!isOpen) {
                updateDropdownPosition();
            }
        }
    };

    return (
        <div ref={containerRef} className={cn('relative', className)}>
            {/* Trigger Button */}
            <button
                ref={buttonRef}
                type="button"
                onClick={handleToggle}
                disabled={disabled}
                className={cn(
                    'w-full px-3 py-2 text-left text-xs font-mono',
                    'bg-white dark:bg-gray-800',
                    'border border-gray-300 dark:border-gray-600',
                    'rounded-lg',
                    'flex items-center justify-between gap-2',
                    'transition-all duration-200',
                    'hover:border-blue-400 dark:hover:border-blue-500',
                    'focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent',
                    disabled && 'opacity-50 cursor-not-allowed',
                    isOpen && 'ring-2 ring-blue-500 border-transparent'
                )}
            >
                <span className="truncate text-gray-900 dark:text-gray-100">
                    {selectedLabel}
                </span>
                <ChevronDown
                    size={14}
                    className={cn(
                        'text-gray-500 dark:text-gray-400 transition-transform duration-200',
                        isOpen && 'rotate-180'
                    )}
                />
            </button>

            {/* Dropdown - Portal */}
            {isOpen && createPortal(
                <div
                    ref={dropdownRef}
                    style={{
                        position: 'absolute',
                        top: `${dropdownPosition.top}px`,
                        left: `${dropdownPosition.left}px`,
                        width: `${dropdownPosition.width}px`,
                        zIndex: 9999
                    }}
                    className={cn(
                        'bg-white dark:bg-gray-800',
                        'border border-gray-200 dark:border-gray-700',
                        'rounded-lg shadow-2xl',
                        'max-h-80 overflow-y-auto',
                        'animate-in fade-in-0 zoom-in-95 duration-100'
                    )}
                >
                    {Object.entries(groupedOptions).map(([group, groupOptions]) => (
                        <div key={group}>
                            {/* Group Title */}
                            <div className="px-3 py-1.5 text-[9px] font-bold text-gray-500 dark:text-gray-400 uppercase tracking-wider bg-gray-50 dark:bg-gray-900/50 sticky top-0 z-10">
                                {group}
                            </div>

                            {/* Options */}
                            {groupOptions.map((option) => (
                                <button
                                    key={option.value}
                                    type="button"
                                    onClick={() => handleSelect(option.value)}
                                    title={option.label}
                                    className={cn(
                                        'w-full px-3 py-1.5 text-left text-[10px] font-mono',
                                        'flex items-center justify-between gap-2',
                                        'transition-colors duration-150',
                                        'hover:bg-blue-50 dark:hover:bg-blue-900/20',
                                        option.value === value
                                            ? 'bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300'
                                            : 'text-gray-900 dark:text-gray-100'
                                    )}
                                >
                                    <span className="truncate">{option.label}</span>
                                    {option.value === value && (
                                        <Check size={12} className="text-blue-600 dark:text-blue-400 flex-shrink-0" />
                                    )}
                                </button>
                            ))}
                        </div>
                    ))}
                </div>,
                document.body
            )}
        </div>
    );
};
