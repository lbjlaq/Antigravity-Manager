import React from 'react';
import { QuotaWindow } from '../../utils/quota';

type RgbTuple = [number, number, number];

function lerpRgb(a: RgbTuple, b: RgbTuple, t: number): string {
  const r = Math.round(a[0] + (b[0] - a[0]) * t);
  const g = Math.round(a[1] + (b[1] - a[1]) * t);
  const bl = Math.round(a[2] + (b[2] - a[2]) * t);
  return `rgb(${r},${g},${bl})`;
}

const MATRIX_COLS = 20;
const MATRIX_ROWS = 2;
const MATRIX_TOTAL = MATRIX_COLS * MATRIX_ROWS;

// 预生成 2×20 空晶格底轨与缓存彩色填充层 SVG（零 DOM 冗余，120Hz 零开销切换）
function buildEmptyMatrixUri(fillHex: string, fillOpacity: number): string {
  const rects: string[] = [];
  for (let col = 0; col < MATRIX_COLS; col++) {
    for (let row = 0; row < MATRIX_ROWS; row++) {
      rects.push(
        `<rect x='${col * 7}' y='${row * 7}' width='5' height='5' rx='1.35' fill='${fillHex}' fill-opacity='${fillOpacity}'/>`
      );
    }
  }
  const svg = `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 138 12' preserveAspectRatio='none'>${rects.join('')}</svg>`;
  return `url("data:image/svg+xml,${encodeURIComponent(svg)}")`;
}

const EMPTY_MATRIX_LIGHT_URI = buildEmptyMatrixUri('#cbd5e1', 0.72);
const EMPTY_MATRIX_DARK_URI = buildEmptyMatrixUri('#ffffff', 0.08);
const FILLED_MATRIX_CACHE = new Map<string, string>();

function getFilledMatrixUri(
  paletteKey: string,
  filledTiles: number,
  startRgb: RgbTuple,
  endRgb: RgbTuple,
  seed: number
): string {
  const cacheKey = `${paletteKey}_${filledTiles}_${seed}`;
  const cached = FILLED_MATRIX_CACHE.get(cacheKey);
  if (cached) return cached;

  const rects: string[] = [];
  for (let col = 0; col < MATRIX_COLS; col++) {
    const t = col / (MATRIX_COLS - 1);
    const colColor = lerpRgb(startRgb, endRgb, t);
    const baseOpacity = 0.76 + 0.24 * t;

    for (let row = 0; row < MATRIX_ROWS; row++) {
      const idx = col * MATRIX_ROWS + row;
      if (idx >= filledTiles) continue;
      const shimmer =
        (col + row + seed) % 3 === 0 ? -0.1 : (col * 3 + row + seed) % 4 === 0 ? -0.05 : 0;
      const opacity = Math.max(0.66, Math.min(1, baseOpacity + shimmer)).toFixed(2);
      rects.push(
        `<rect x='${col * 7}' y='${row * 7}' width='5' height='5' rx='1.35' fill='${colColor}' fill-opacity='${opacity}'/>`
      );
    }
  }

  const svg = `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 138 12' preserveAspectRatio='none'>${rects.join('')}</svg>`;
  const uri = `url("data:image/svg+xml,${encodeURIComponent(svg)}")`;
  FILLED_MATRIX_CACHE.set(cacheKey, uri);
  return uri;
}

export function quotaPctTextClass(pct: number): string {
  if (pct <= 20) return 'text-rose-600 dark:text-rose-400';
  if (pct < 60) return 'text-amber-600 dark:text-amber-400';
  return 'text-slate-800 dark:text-zinc-100';
}

interface QuotaWindowBadgeProps {
  window: QuotaWindow;
  className?: string;
}

export const QuotaWindowBadge: React.FC<QuotaWindowBadgeProps> = React.memo(
  ({ window, className = '' }) => {
    const is5h = window === '5h';
    return (
      <span
        className={`inline-flex items-center justify-center font-mono font-bold rounded tracking-tight select-none shrink-0 ${
          is5h
            ? 'bg-sky-500/15 text-sky-600 dark:text-sky-300 border border-sky-500/25'
            : 'bg-indigo-500/15 text-indigo-600 dark:text-indigo-300 border border-indigo-500/25'
        } ${className}`}
      >
        {window}
      </span>
    );
  }
);

interface QuotaMatrixBarProps {
  pool: 'gemini' | 'claude';
  window: QuotaWindow;
  percentage: number;
  empty?: boolean;
  className?: string;
}

export const QuotaMatrixBar: React.FC<QuotaMatrixBarProps> = React.memo(
  ({ pool, window, percentage, empty = false, className = 'h-[12px]' }) => {
    const isGemini = pool === 'gemini';
    const is5h = window === '5h';
    const clamped = empty ? 0 : Math.max(0, Math.min(100, percentage));

    const isCritical = clamped <= 20;
    const isWarn = clamped > 20 && clamped < 60;
    const filledTiles =
      clamped >= 100
        ? MATRIX_TOTAL
        : clamped <= 0
        ? 0
        : Math.max(1, Math.min(MATRIX_TOTAL - 1, Math.floor((clamped / 100) * MATRIX_TOTAL)));

    const paletteKey = isCritical
      ? 'crit'
      : isWarn
      ? 'warn'
      : isGemini
      ? is5h
        ? 'g5h'
        : 'g7d'
      : is5h
      ? 'c5h'
      : 'c7d';

    const [startRgb, endRgb]: [RgbTuple, RgbTuple] = isCritical
      ? [
          [251, 113, 133],
          [225, 29, 72],
        ]
      : isWarn
      ? [
          [251, 191, 36],
          [217, 119, 6],
        ]
      : isGemini
      ? is5h
        ? [
            [56, 189, 248],
            [37, 99, 235],
          ]
        : [
            [45, 212, 191],
            [2, 132, 199],
          ]
      : is5h
      ? [
          [129, 140, 248],
          [124, 58, 237],
        ]
      : [
          [167, 139, 250],
          [109, 40, 217],
        ];

    const seed = isGemini ? (is5h ? 1 : 2) : is5h ? 3 : 4;
    const filledUri =
      filledTiles > 0
        ? getFilledMatrixUri(paletteKey, filledTiles, startRgb, endRgb, seed)
        : undefined;

    return (
      <div className={`relative flex-1 ${className}`} aria-hidden="true">
        <div
          style={{ backgroundImage: EMPTY_MATRIX_LIGHT_URI, backgroundSize: '100% 100%' }}
          className="absolute inset-0 bg-no-repeat dark:hidden"
        />
        <div
          style={{ backgroundImage: EMPTY_MATRIX_DARK_URI, backgroundSize: '100% 100%' }}
          className="absolute inset-0 bg-no-repeat hidden dark:block"
        />
        {filledUri && (
          <div
            style={{ backgroundImage: filledUri, backgroundSize: '100% 100%' }}
            className="absolute inset-0 bg-no-repeat"
          />
        )}
      </div>
    );
  }
);
