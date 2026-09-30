import { type IconType, useFillIds } from '@lobehub/icons';
import React, { memo } from 'react';
import { cn } from '../../utils/cn';

export type JeikCodeIconProps = React.ComponentProps<IconType>;

/**
 * JeikCode 官方品牌彩色矢量图标 (默认自带深邃科技背景框与霓虹发光描边，遵循 @lobehub/icons 规范)
 */
const JeikCodeBase: IconType = memo(({ size = '1em', className = '', style = {}, ...rest }) => {
    const rawIds = useFillIds('jeikcode', 3);
    const getId = (item: any) => (typeof item === 'string' ? item : item?.id || '');
    const getFill = (item: any) => (typeof item === 'string' ? `url(#${item})` : item?.fill || `url(#${item?.id})`);

    const bgGradId = getId(rawIds[0]);
    const borderGradId = getId(rawIds[1]);
    const brandGradId = getId(rawIds[2]);

    const bgFill = getFill(rawIds[0]);
    const borderFill = getFill(rawIds[1]);
    const brandFill = getFill(rawIds[2]);

    return (
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 512 512"
            width={size}
            height={size}
            className={cn("shrink-0", className)}
            style={{ flex: 'none', lineHeight: 1, ...style }}
            {...rest}
        >
            <defs>
                {/* Background Gradient */}
                <linearGradient id={bgGradId} x1="0%" y1="0%" x2="100%" y2="100%">
                    <stop offset="0%" stopColor="#0b0f19" />
                    <stop offset="50%" stopColor="#0f172a" />
                    <stop offset="100%" stopColor="#020617" />
                </linearGradient>

                {/* Border Glow Gradient */}
                <linearGradient id={borderGradId} x1="0%" y1="0%" x2="100%" y2="100%">
                    <stop offset="0%" stopColor="#38bdf8" stopOpacity="0.85" />
                    <stop offset="50%" stopColor="#818cf8" stopOpacity="0.4" />
                    <stop offset="100%" stopColor="#c084fc" stopOpacity="0.85" />
                </linearGradient>

                {/* Core Brand Electric Gradient */}
                <linearGradient id={brandGradId} x1="0%" y1="0%" x2="100%" y2="100%">
                    <stop offset="0%" stopColor="#00f2fe" />
                    <stop offset="45%" stopColor="#38bdf8" />
                    <stop offset="75%" stopColor="#818cf8" />
                    <stop offset="100%" stopColor="#c084fc" />
                </linearGradient>
            </defs>

            {/* Base Canvas */}
            <rect width="512" height="512" rx="112" fill={bgFill} />
            <rect
                width="504"
                height="504"
                x="4"
                y="4"
                rx="108"
                fill="none"
                stroke={borderFill}
                strokeWidth="10"
            />

            {/* Central Emblem: Stylized J */}
            <path
                d="M190 148 L322 148 C332 148 340 156 340 166 L340 178 C340 188 332 196 322 196 L278 196 L278 290 C278 336 242 372 196 372 C150 372 118 336 118 300 C118 288 128 278 140 278 C152 278 162 288 162 300 C162 316 176 328 196 328 C216 328 234 312 234 290 L234 196 L190 196 C180 196 172 188 172 178 L172 166 C172 156 180 148 190 148 Z"
                fill={brandFill}
            />

            {/* Left Bracket < */}
            <path
                d="M142 214 L98 256 L142 298"
                fill="none"
                stroke={brandFill}
                strokeWidth="24"
                strokeLinecap="round"
                strokeLinejoin="round"
            />

            {/* Right Bracket > */}
            <path
                d="M370 214 L414 256 L370 298"
                fill="none"
                stroke={brandFill}
                strokeWidth="24"
                strokeLinecap="round"
                strokeLinejoin="round"
            />

            {/* Quantum Spark Dot */}
            <circle cx="376" cy="148" r="18" fill="#00f2fe" />
            <circle cx="376" cy="148" r="28" fill="#00f2fe" opacity="0.35" />
        </svg>
    );
});

/**
 * 纯图形矢量（无底框），用于在已有深浅色背景中无缝嵌入
 */
const JeikCodeColor: IconType = memo(({ size = '1em', className = '', style = {}, ...rest }) => {
    const rawIds = useFillIds('jeikcode-color', 1);
    const brandGradId = typeof rawIds[0] === 'string' ? rawIds[0] : (rawIds[0] as any)?.id || '';
    const brandFill = typeof rawIds[0] === 'string' ? `url(#${rawIds[0]})` : (rawIds[0] as any)?.fill || `url(#${brandGradId})`;

    return (
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="90 140 332 236"
            width={size}
            height={size}
            className={cn("shrink-0", className)}
            style={{ flex: 'none', lineHeight: 1, ...style }}
            {...rest}
        >
            <defs>
                <linearGradient id={brandGradId} x1="0%" y1="0%" x2="100%" y2="100%">
                    <stop offset="0%" stopColor="#00f2fe" />
                    <stop offset="45%" stopColor="#38bdf8" />
                    <stop offset="75%" stopColor="#818cf8" />
                    <stop offset="100%" stopColor="#c084fc" />
                </linearGradient>
            </defs>

            {/* Central Emblem: Stylized J */}
            <path
                d="M190 148 L322 148 C332 148 340 156 340 166 L340 178 C340 188 332 196 322 196 L278 196 L278 290 C278 336 242 372 196 372 C150 372 118 336 118 300 C118 288 128 278 140 278 C152 278 162 288 162 300 C162 316 176 328 196 328 C216 328 234 312 234 290 L234 196 L190 196 C180 196 172 188 172 178 L172 166 C172 156 180 148 190 148 Z"
                fill={brandFill}
            />

            {/* Left Bracket < */}
            <path
                d="M142 214 L98 256 L142 298"
                fill="none"
                stroke={brandFill}
                strokeWidth="24"
                strokeLinecap="round"
                strokeLinejoin="round"
            />

            {/* Right Bracket > */}
            <path
                d="M370 214 L414 256 L370 298"
                fill="none"
                stroke={brandFill}
                strokeWidth="24"
                strokeLinecap="round"
                strokeLinejoin="round"
            />

            {/* Quantum Spark Dot */}
            <circle cx="376" cy="148" r="18" fill="#00f2fe" />
        </svg>
    );
});

export const JeikCode = Object.assign(JeikCodeBase, {
    Color: JeikCodeColor,
});

export const JeikCodeIcon = JeikCode;
export default JeikCode;
