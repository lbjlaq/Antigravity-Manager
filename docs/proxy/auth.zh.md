# Proxy 鉴权（auth modes）

## 目标
- 支持在本机/受控环境下以“开放模式”运行代理。
- 当代理暴露到更大范围（LAN/共享主机等）时，可开启请求鉴权。
- 为不支持鉴权头的工具提供可用模式（例如允许 health 保持开放）。
- 配置修改无需重启（热更新）。

## 结果
代理支持 `proxy.auth_mode` 四种模式：
- `off` — 不需要鉴权。
- `strict` — 所有路由都需要鉴权。
- `all_except_health` — 除 `GET /healthz`（以及 `GET /health` 别名）外都需要鉴权。
- `auto` — 推导模式：若 `proxy.allow_lan_access=true` 则为 `all_except_health`，否则为 `off`。

### 策略矩阵

| `proxy.allow_lan_access` | `proxy.auth_mode` | 有效模式 | `/healthz` 需要鉴权？ | 其它路由需要鉴权？ |
|---:|---|---|---|---|
| false | `off` | `off` | 否 | 否 |
| true | `off` | `off` | 否 | 否 |
| false | `strict` | `strict` | 是 | 是 |
| true | `strict` | `strict` | 是 | 是 |
| false | `all_except_health` | `all_except_health` | 否 | 是 |
| true | `all_except_health` | `all_except_health` | 否 | 是 |
| false | `auto` | `off` | 否 | 否 |
| true | `auto` | `all_except_health` | 否 | 是 |

说明：
- 当 `/healthz` 被配置为开放（有效模式为 `all_except_health`）时，中间件会对该路由 **完全跳过鉴权**：
  - 不要求携带鉴权头；
  - 即使携带了错误的鉴权头，也不会因此返回 401。
  - `/health` 是 `/healthz` 的兼容别名，行为一致。

实现：
- 配置枚举与序列化：[`src-tauri/src/proxy/config.rs`](../../src-tauri/src/proxy/config.rs)（`ProxyAuthMode`）
- 有效策略推导：[`src-tauri/src/proxy/security.rs`](../../src-tauri/src/proxy/security.rs)（`ProxySecurityConfig::from_proxy_config(...)`）
- 中间件校验：[`src-tauri/src/proxy/middleware/auth.rs`](../../src-tauri/src/proxy/middleware/auth.rs)
  - 校验 `Authorization: Bearer <proxy.api_key>`
  - 同时支持 `x-api-key: <proxy.api_key>`
  - 放行 `OPTIONS`（CORS 预检）
  - 在 `all_except_health` 模式下，`GET /healthz` 与 `GET /health` 跳过鉴权

热更新：
- 保存配置会更新运行中的服务：[`src-tauri/src/commands/mod.rs`](../../src-tauri/src/commands/mod.rs)
  - `save_config(...)` 调用 `axum_server.update_security(&config.proxy).await`

路由背景：
- 鉴权策略对代理提供的所有协议面一视同仁（OpenAI / Claude / Gemini / MCP）。
- 路由总览：[`docs/proxy/routing.zh.md`](routing.zh.md)

## 客户端契约
开启鉴权后，客户端需发送：
- `Authorization: Bearer <proxy.api_key>`（推荐）
- `x-api-key: <proxy.api_key>`（兼容部分工具）

说明：
- 代理 API key 不会被转发到任何上游。
- `GET /healthz` 是否开放取决于所选模式。

## 验证
1) 在 UI 中设置 `proxy.auth_mode=all_except_health` 且填写 `proxy.api_key`（`src/pages/ApiProxy.tsx`）。
2) 启动代理。
3) 验证：
   - `GET /healthz` 无需鉴权可访问。
   - 其它端点（例如 `POST /v1/messages`）不带鉴权返回 401，带鉴权成功。
