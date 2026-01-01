# 前端错误日志（UI “白屏”排障）

UI 运行在 WebView 中。如果 React 应用在早期崩溃（bundle 加载失败、运行时异常、渲染异常），窗口可能表现为白屏且缺乏可见错误提示。

为便于定位问题，应用会捕获前端错误并写入后端日志文件。

## 捕获范围
- 未处理的 JS 错误（`window.onerror`）
- 未处理的 Promise rejection（`unhandledrejection`）
- React 渲染错误（`ErrorBoundary`）
- `console.error` / `console.warn` / `console.info`（镜像到后端日志）

## 日志位置
后端日志目录：
- `~/.antigravity_tools/logs/`

可通过以下标记检索：
- `[frontend]`

## 敏感信息处理
前端日志写入磁盘前会进行“尽力而为”的脱敏：
- 去除常见 `Authorization: Bearer ...` 模式
- 去除常见 `sk-...` 形式的 token

注意：
- 该措施降低泄露风险，但不是绝对保障。不要在 UI 控制台中输出/粘贴敏感信息。

## 实现指针
后端命令：
- [`src-tauri/src/commands/mod.rs`](../../src-tauri/src/commands/mod.rs)（`frontend_log`）

Tauri 绑定：
- [`src-tauri/src/lib.rs`](../../src-tauri/src/lib.rs)

前端初始化：
- [`src/utils/frontendLogging.ts`](../../src/utils/frontendLogging.ts)
- [`src/components/common/ErrorBoundary.tsx`](../../src/components/common/ErrorBoundary.tsx)
- [`src/main.tsx`](../../src/main.tsx)

