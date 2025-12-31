# Proxy 请求日志（access logging）

## 目标
提供低噪声的请求级调试能力，同时尽量避免敏感信息泄露。

## 配置
字段：`proxy.access_log_enabled`（存储在 `gui_config.json`）。

- `false`（默认）：不输出逐请求访问日志。
- `true`：对每个请求记录 method/path/status/latency。

## 记录内容
开启后，每个请求输出一条日志，包含：
- HTTP method
- path（不含 query string）
- status code
- latency（ms）
- `upstream`（最佳努力的上游标签：例如 `zai`、`zai_mcp`、`unknown`）

安全保证：
- 不记录 query string
- 不记录 headers
- 不记录 request/response body

## 实现
后端：
- 配置字段：`src-tauri/src/proxy/config.rs`
- 中间件：`src-tauri/src/proxy/middleware/access_log.rs`
- 服务接入与热更新：`src-tauri/src/proxy/server.rs`、`src-tauri/src/commands/mod.rs`
- 上游标签：`src-tauri/src/proxy/observability.rs`（由各 handler 写入）

前端：
- UI 开关：`src/pages/ApiProxy.tsx`
- 文案：`src/locales/en.json`、`src/locales/zh.json`

相关排障：
- UI/运行时错误采集（用于定位“白屏”）：[`docs/app/frontend-logging.zh.md`](../app/frontend-logging.zh.md)

