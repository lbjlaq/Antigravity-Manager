# Changelog

## Unreleased

### Proxy (Claude-compatible)

**EN**
- Fix Gemini 400 errors caused by client JSON Schema extensions inside tool definitions (e.g. `enumCaseInsensitive`, `enumNormalizeWhitespace`) by cleaning nested schema fields recursively.
- Improve `/resume` reliability for sessions created with a different upstream by retrying once with thinking disabled when the upstream rejects `thinking.signature` (invalid/missing signature), and by avoiding injecting dummy thinking blocks for non-Gemini upstream models.
- Reduce transient 429 failures by honoring upstream-provided retry delays (`RetryInfo.retryDelay` / `quotaResetDelay`) before retrying.

**中文**
- 修复因客户端在工具定义的 JSON Schema 中携带扩展字段（例如 `enumCaseInsensitive`、`enumNormalizeWhitespace`）导致 Gemini 返回 400 的问题：对嵌套 Schema 进行递归清理。
- 提升 `/resume` 场景稳定性：当上游因 `thinking.signature`（签名缺失/无效）拒绝请求时，自动进行一次“禁用 thinking”的重试；同时避免在非 Gemini 上游模型中注入不带签名的占位 thinking 块。
- 减少短时 429 报错：在重试前遵循上游返回的重试等待时间（`RetryInfo.retryDelay` / `quotaResetDelay`）。
