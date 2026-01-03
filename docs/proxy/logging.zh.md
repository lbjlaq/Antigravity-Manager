# 代理请求日志（安全访问日志）

API Proxy 提供一个可选的、默认不泄露敏感信息的“安全访问日志”，用于排障与运行可见性。

## 会记录什么
开启后（`proxy.access_log_enabled=true`），每个请求只会输出一行日志，包含：
- HTTP method
- path（不包含 query string）
- response status
- latency

不会记录：
- request/response body
- headers（包含 `Authorization` / `x-api-key`）
- query string

## 如何开启
UI 位置：**API Proxy → Service Configuration → Request Logging**。

持久化配置项：
- `proxy.access_log_enabled`（默认：`false`）

## 相关功能：响应归因头
如果你有多个工具同时调用代理，也可以开启响应归因头：
- `proxy.response_attribution_headers`（默认：`false`）

该功能会在响应中注入脱敏的 `x-antigravity-*` header，让你无需记录 body 就能看出“是哪一个 provider/model/account”在服务请求。

参考：`docs/proxy/routing.zh.md`

