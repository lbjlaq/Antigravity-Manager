import { useState, useRef, useEffect } from 'react';
import { createPortal } from 'react-dom';
import { ChevronDown, Check, Edit3 } from 'lucide-react';
import { cn } from '../../utils/cn';
import { getGroupPriority } from '../../config/modelConfig';

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
    allowCustomInput?: boolean; // 新增: 是否允许自定义输入
}

export default function GroupedSelect({
    value,
    onChange,
    options,
    placeholder = 'Select...',
    className = '',
    disabled = false,
    allowCustomInput = false // 新增: 默认不允许自定义输入
}: GroupedSelectProps) {
    const [isOpen, setIsOpen] = useState(false);
    const [dropdownPosition, setDropdownPosition] = useState({ top: 0, left: 0, width: 0 });
    const [customInput, setCustomInput] = useState(''); // 新增: 自定义输入值
    const containerRef = useRef<HTMLDivElement>(null);
    const buttonRef = useRef<HTMLButtonElement>(null);
    const dropdownRef = useRef<HTMLDivElement>(null); // 新增: 下拉菜单引用
    const customInputRef = useRef<HTMLInputElement>(null); // 新增: 自定义输入框引用

    // 按组分组选项
    const groupedOptions = options.reduce((acc, option) => {
        const group = option.group || 'Other';
        if (!acc[group]) {
            acc[group] = [];
        }
        acc[group].push(option);
        return acc;
    }, {} as Record<string, SelectOption[]>);

    // 按权威优先级对组进行动态排序 (Gemini 高版本优先，再到 Claude，再到 OpenAI/Other)
    const sortedGroupEntries = Object.entries(groupedOptions).sort(([grpA], [grpB]) => {
        const prioA = getGroupPriority(grpA);
        const prioB = getGroupPriority(grpB);
        if (prioA !== prioB) return prioA - prioB;
        return grpA.localeCompare(grpB);
    });

    // 获取当前选中项的标签
    const selectedOption = options.find(opt => opt.value === value);
    const selectedLabel = selectedOption?.label || value || placeholder;

    // 更新下拉菜单位置
    const updateDropdownPosition = () => {
        if (buttonRef.current) {
            const rect = buttonRef.current.getBoundingClientRect();
            setDropdownPosition({
                top: rect.bottom + window.scrollY + 4,
                left: rect.left + window.scrollX,
                width: Math.max(rect.width * 1.1, 220) // 增加宽度到 1.1 倍,最小 220px
            });
        }
    };

    // 点击外部关闭下拉菜单
    useEffect(() => {
        const handleClickOutside = (event: MouseEvent) => {
            // 修复: 检查点击是否在容器或下拉菜单内部
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
        console.log('[GroupedSelect] handleSelect called:', optionValue);
        onChange(optionValue);
        setIsOpen(false);
    };

    const handleCustomInputSubmit = () => {
        if (customInput.trim()) {
            console.log('[GroupedSelect] Custom input submitted:', customInput.trim());
            onChange(customInput.trim());
            setCustomInput('');
            setIsOpen(false);
        }
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
            {/* 触发按钮 */}
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

            {/* 下拉菜单 - 使用 Portal 渲染到 body */}
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
                    {sortedGroupEntries.map(([group, groupOptions]) => (
                        <div key={group}>
                            {/* 分组标题 */}
                            <div className="px-3 py-1.5 text-[9px] font-bold text-gray-500 dark:text-gray-400 uppercase tracking-wider bg-gray-50 dark:bg-gray-900/50 sticky top-0 z-10">
                                {group}
                            </div>

                            {/* 分组选项 */}
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

                    {/* 自定义输入区域 */}
                    {allowCustomInput && (
                        <div className="border-t border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-900/50 p-2">
                            <div className="flex items-center gap-1.5">
                                <Edit3 size={12} className="text-gray-400 dark:text-gray-500 flex-shrink-0" />
                                <input
                                    ref={customInputRef}
                                    type="text"
                                    value={customInput}
                                    onChange={(e) => setCustomInput(e.target.value)}
                                    onKeyDown={(e) => {
                                        if (e.key === 'Enter') {
                                            e.preventDefault();
                                            handleCustomInputSubmit();
                                        }
                                    }}
                                    placeholder="输入自定义模型 ID..."
                                    className={cn(
                                        'flex-1 px-2 py-1 text-[10px] font-mono',
                                        'bg-white dark:bg-gray-800',
                                        'border border-gray-300 dark:border-gray-600',
                                        'rounded focus:outline-none focus:ring-1 focus:ring-blue-500',
                                        'text-gray-900 dark:text-gray-100',
                                        'placeholder:text-gray-400 dark:placeholder:text-gray-500'
                                    )}
                                />
                                <button
                                    type="button"
                                    onClick={handleCustomInputSubmit}
                                    disabled={!customInput.trim()}
                                    className={cn(
                                        'px-2 py-1 text-[10px] font-medium rounded',
                                        'transition-colors duration-150',
                                        customInput.trim()
                                            ? 'bg-blue-500 hover:bg-blue-600 text-white'
                                            : 'bg-gray-200 dark:bg-gray-700 text-gray-400 dark:text-gray-500 cursor-not-allowed'
                                    )}
                                >
                                    确定
                                </button>
                            </div>
                        </div>
                    )}
                </div>,
                document.body
            )}
        </div>
    );
}
