/**
 * 完整实现与对齐 baiwumm/theme-switch-animation 的 CIRCLE_BLUR (圆形模糊扩散) 原理：
 * 1. 采用高斯模糊滤镜 (<feGaussianBlur>) 烘焙入 SVG 蒙版
 * 2. 挂载到 ::view-transition-new(root) 新视图图层上，旧视图完整垫底
 * 3. 驱动 mask-size 从 0px 展开至覆盖全屏最大外接圆尺寸 (pinCenterGeometry 锚定在点击坐标)
 * 4. 动态注入 <style id="am-theme-switch-style">，转场结束后自动移除，0 额外运行时开销
 */

export interface Point {
  x: number;
  y: number;
}

export interface Size {
  width: number;
  height: number;
}

const STYLE_ID = 'am-theme-switch-blur-style';
const BLUR_MAX_MASK_SIZE = 8000;
const BLUR_MASK_DEVIATION_FACTOR = 1.2;

function roundTo(value: number, digits: number): number {
  const factor = 10 ** digits;
  return Math.round(value * factor) / factor;
}

/** 烘焙带高斯模糊的矢量圆 SVG mask data-URI */
export function getBlurCircleMaskImage(blurAmount = 2): string {
  const stdDeviation = roundTo(blurAmount * BLUR_MASK_DEVIATION_FACTOR, 2);
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="-50 -50 100 100">` +
    `<defs><filter id="b"><feGaussianBlur stdDeviation="${stdDeviation}"/></filter></defs>` +
    `<circle cx="0" cy="0" r="25" fill="#fff" filter="url(#b)"/></svg>`;
  return `url("data:image/svg+xml,${encodeURIComponent(svg)}")`;
}

/** 触发点到视口四角的最大距离 */
export function getMaxRadiusToCorners(center: Point, viewport: Size): number {
  const { x, y } = center;
  const { width, height } = viewport;
  return Math.max(
    Math.hypot(x, y),
    Math.hypot(width - x, y),
    Math.hypot(x, height - y),
    Math.hypot(width - x, height - y)
  );
}

/** 模糊圆的展开终尺寸 */
export function getBlurMaskEndSizePx(center: Point, viewport: Size): number {
  const longestSide = Math.max(viewport.width, viewport.height);
  return Math.min(
    BLUR_MAX_MASK_SIZE,
    Math.max((longestSide + 200) * 4, getMaxRadiusToCorners(center, viewport) * 2.5)
  );
}

export interface BlurCircleGeometry {
  maskImage: string;
  startSize: string;
  startPosition: string;
  endSize: string;
  endPosition: string;
}

export function getBlurCircleGeometry(center: Point, viewport: Size, blurAmount = 2): BlurCircleGeometry {
  const endSize = Math.round(getBlurMaskEndSizePx(center, viewport));
  const maskImage = getBlurCircleMaskImage(blurAmount);
  return {
    maskImage,
    startSize: '0px 0px',
    startPosition: `${Math.round(center.x)}px ${Math.round(center.y)}px`,
    endSize: `${endSize}px ${endSize}px`,
    endPosition: `${Math.round(center.x - endSize / 2)}px ${Math.round(center.y - endSize / 2)}px`,
  };
}

export function removeThemeStyle(): void {
  document.getElementById(STYLE_ID)?.remove();
}

/** 生成并注入遵循 baiwumm 规格的 CSS 动画规则 */
export function injectBlurCircleStyle(center: Point, duration = 700, blurAmount = 2.2): HTMLStyleElement {
  removeThemeStyle();

  const viewport = {
    width: window.innerWidth,
    height: window.innerHeight,
  };
  const geom = getBlurCircleGeometry(center, viewport, blurAmount);

  const css = `
::view-transition-old(root),
::view-transition-new(root) {
  animation: none;
  mix-blend-mode: normal;
}
::view-transition-old(root) {
  z-index: 1;
}
::view-transition-new(root) {
  z-index: 9999;
  -webkit-mask-image: ${geom.maskImage};
  mask-image: ${geom.maskImage};
  -webkit-mask-repeat: no-repeat;
  mask-repeat: no-repeat;
  will-change: mask-size, mask-position, -webkit-mask-size, -webkit-mask-position;
  animation: am-theme-circle-blur ${duration}ms cubic-bezier(0.2, 0.8, 0.25, 1) both;
}
@keyframes am-theme-circle-blur {
  from {
    -webkit-mask-size: ${geom.startSize};
    mask-size: ${geom.startSize};
    -webkit-mask-position: ${geom.startPosition};
    mask-position: ${geom.startPosition};
  }
  to {
    -webkit-mask-size: ${geom.endSize};
    mask-size: ${geom.endSize};
    -webkit-mask-position: ${geom.endPosition};
    mask-position: ${geom.endPosition};
  }
}
`;

  const style = document.createElement('style');
  style.id = STYLE_ID;
  style.textContent = css;
  (document.head || document.documentElement).appendChild(style);
  return style;
}
