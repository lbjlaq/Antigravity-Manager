@tailwind base;
@tailwind components;
@tailwind utilities;

/* 禁止过度滚动和橡皮筋效果 */
html,
body {
  overscroll-behavior: none;
  height: 100%;
  overflow-y: hidden;
  overflow-x: hidden;
  margin: 0;
  padding: 0;
  border: none;
}

html {
  background-color: #FAFBFC;
}

html.dark {
  background-color: #1d232a;
}

/* 全局样式 */
body {
  margin: 0;
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Roboto', 'Oxygen',
    'Ubuntu', 'Cantarell', 'Fira Sans', 'Droid Sans', 'Helvetica Neue',
    sans-serif;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  background-color: #FAFBFC;
}

/* Dark mode override for body strictly */
.dark body {
  background-color: #1d232a;
  /* matches base-300 commonly used */
}

#root {
  width: 100%;
  height: 100%;
  overflow-y: auto;
  overflow-x: hidden;
  overscroll-behavior: none;
}

/* 移除默认的 tap 高亮 */
* {
  -webkit-tap-highlight-color: transparent;
}

/* 只移除链接的默认下划线，不强制颜色 */
a {
  text-decoration: none;
}

/* 滚动条优化 - 彻底隐藏但保留功能 */
::-webkit-scrollbar {
  width: 0px;
  background: transparent;
}

::-webkit-scrollbar-track {
  background-color: transparent;
}

::-webkit-scrollbar-thumb {
  background-color: rgba(0, 0, 0, 0.1);
  border-radius: 99px;
  border: 3px solid transparent;
  background-clip: content-box;
  transition: background-color 0.2s;
}

::-webkit-scrollbar-thumb:hover {
  background-color: rgba(0, 0, 0, 0.3);
}

/* View Transitions API 主题切换动画 */
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
}

.dark::view-transition-old(root) {
  z-index: 9999;
}

.dark::view-transition-new(root) {
  z-index: 1;
}

/* 暗色模式下 select 下拉菜单样式 */
.dark select,
.dark select option,
.dark select optgroup {
  background-color: #1e293b;
  color: #e2e8f0;
}

.dark select option:checked {
  background-color: #3b82f6;
  color: white;
}

/* Arabic Fonts - Effra */
@font-face {
  font-family: 'Effra';
  src: url('/font/Effra/Effra-Regular.ttf') format('truetype');
  font-weight: 400; /* Regular */
  font-style: normal;
}

@font-face {
  font-family: 'Effra';
  src: url('/font/Effra/Effra-Medium.ttf') format('truetype');
  font-weight: 500; /* Medium */
  font-style: normal;
}

@font-face {
  font-family: 'Effra';
  src: url('/font/Effra/Effra-SemiBold.ttf') format('truetype');
  font-weight: 600; /* SemiBold */
  font-style: normal;
}

@font-face {
  font-family: 'Effra';
  src: url('/font/Effra/Effra-Bold.ttf') format('truetype');
  font-weight: 700; /* Bold */
  font-style: normal;
}

@font-face {
  font-family: 'Effra';
  src: url('/font/Effra/Effra-ExtraBold.ttf') format('truetype');
  font-weight: 800; /* ExtraBold */
  font-style: normal;
}

/* Apply Effra font ONLY when language direction is RTL (Arabic) */
[dir="rtl"] body {
  font-family: 'Effra', -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Roboto', 'Oxygen',
    'Ubuntu', 'Cantarell', 'Fira Sans', 'Droid Sans', 'Helvetica Neue',
    sans-serif;
}