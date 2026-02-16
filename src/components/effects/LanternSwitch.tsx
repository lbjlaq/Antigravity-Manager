import React, { useState } from 'react';

interface LanternSwitchProps {
    onTrigger: () => void;
    xPosition?: number;
}

/**
 * 春节彩蛋 - 拉绳开关
 * 
 * 悬挂在 Logo 旁边的装饰性红绳。
 * 交互：Hover 时轻微晃动，Click 时模拟拉动弹力效果。
 */
export const LanternSwitch: React.FC<LanternSwitchProps> = ({ onTrigger, xPosition }) => {
    const [isPulling, setIsPulling] = useState(false);

    const handleClick = (e: React.MouseEvent) => {
        e.preventDefault();
        e.stopPropagation();

        if (isPulling) return;

        setIsPulling(true);
        // 触发回调
        onTrigger();

        // 动画复位
        setTimeout(() => {
            setIsPulling(false);
        }, 300); // 与 CSS transition 匹配
    };

    // Wait for position calculation
    if (xPosition === undefined) return null;

    return (
        <div
            className="fixed top-0 cursor-pointer group z-50 pointer-events-auto"
            style={{ left: xPosition, transform: 'translateX(-50%)' }} // 此处 translateX(-50%) 确保绳子水平中心对齐 Logo 中心
            onClick={handleClick}
        >
            {/* 绳子容器 - 用于摇摆动画 */}
            <div className={`origin-top transition-transform duration-300 ease-in-out ${isPulling ? 'scale-y-110 translate-y-1' : 'group-hover:rotate-3'}`}>
                {/* 细绳 - 加长以穿过 Logo */}
                <div className="w-[1.5px] h-20 bg-red-800/90 mx-auto" />

                {/* 小福牌/中国结 */}
                <div className="w-5 h-5 bg-red-600 rounded-sm rotate-45 border border-yellow-500/50 shadow-sm flex items-center justify-center -mt-1 mx-auto relative z-10">
                    <div className="w-3 h-3 border border-yellow-500/30" />
                    <span
                        className="absolute text-[8px] font-bold text-yellow-300 -rotate-45 select-none"
                        style={{ fontFamily: '"Noto Serif SC", serif' }}
                    >
                        福
                    </span>
                </div>

                {/* 流苏 */}
                <div className="flex justify-center -mt-2 opacity-90">
                    <div className="w-[1px] h-4 bg-red-600 mx-[1px]" />
                    <div className="w-[1px] h-5 bg-red-600 mx-[1px]" />
                    <div className="w-[1px] h-4 bg-red-600 mx-[1px]" />
                </div>
            </div>

            {/* 点击区域扩大 (方便用户点击) */}
            <div className="absolute inset-x-[-10px] inset-y-0 bg-transparent" />
        </div>
    );
};
