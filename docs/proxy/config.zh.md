# Proxy 配置（持久化）

本页总结代理使用的配置字段以及它们对运行时行为的影响。配置由 UI 编辑，并存储在应用数据目录中。

## 配置存储位置

主配置文件：
- `~/.antigravity_tools/gui_config.json`

Google 账号池文件：
- `~/.antigravity_tools/accounts/*.json`

注意：
- 凭据按设计保存在本地磁盘。
- 不要提交或分享这些文件。

## Proxy 顶层配置（`proxy.*`）

### 网络与监听
- `proxy.enabled` — 启动/停止代理服务。
- `proxy.port` — 监听端口。
- `proxy.allow_lan_access` — 允许 LAN 访问（也会影响 `auth_mode=auto` 的推导）。
- `proxy.request_timeout` — 上游请求超时（秒）。
- `proxy.upstream_proxy` — 可选的出站 HTTP 代理（对 Google / z.ai / 远程 MCP 生效）。

### 诊断接口（HTTP endpoints）
代理提供一些轻量诊断接口：
- `GET /healthz` — 标准健康检查
- `GET /health` — `/healthz` 的兼容别名（提升工具兼容性）
- `GET /test-connection` — 检查 Google 账号池是否可选出账号（返回内容已脱敏）

鉴权说明：
- 以上端点仍受 `proxy.auth_mode` 控制（见 `docs/proxy/auth.zh.md`）。在 `all_except_health` 模式下，`/healthz` 与 `/health` 开放；`/test-connection` 仍需鉴权。

### 全局鉴权
- `proxy.auth_mode` — `off | strict | all_except_health | auto`
- `proxy.api_key` — 开启鉴权时必填

详情：
- [`docs/proxy/auth.zh.md`](auth.zh.md)

### 访问日志（默认安全）
- `proxy.access_log_enabled` — 仅记录 method/path/status/latency（不含敏感信息）

详情：
- [`docs/proxy/logging.zh.md`](logging.zh.md)

### 响应归因头（可选）
- `proxy.response_attribution_headers` — 开启后会在响应中注入脱敏的 `x-antigravity-*` 归因头

详情：
- [`docs/proxy/routing.zh.md`](routing.zh.md)

### 协议映射
这些映射影响非 z.ai 路径的 model 名称转换（以及相关路由/执行逻辑）：
- `proxy.anthropic_mapping`
- `proxy.openai_mapping`
- `proxy.custom_mapping`

Claude 家族/档位映射（Google 账号池路径）：
- `proxy.anthropic_mapping` 支持以下可选 “family/tier” 分组 key：
  - `claude-opus-family`
  - `claude-sonnet-family`
  - `claude-haiku-family`

路由总览：
- [`docs/proxy/routing.zh.md`](routing.zh.md)

## z.ai 配置（`proxy.zai.*`）

### Provider（仅影响 Claude 协议）
- `proxy.zai.enabled`
- `proxy.zai.base_url`（默认 `https://api.z.ai/api/anthropic`）
- `proxy.zai.api_key`
- `proxy.zai.dispatch_mode` — `off | exclusive | pooled | fallback`
- `proxy.zai.models.opus|sonnet|haiku` — 当 Claude 请求路由到 z.ai 时，对 `claude-*` 的默认映射
- `proxy.zai.model_mapping` — 当 Claude 请求路由到 z.ai 时，对 `model` 的精确匹配覆盖

详情：
- [`docs/zai/provider.zh.md`](../zai/provider.zh.md)
- [`docs/zai/implementation.zh.md`](../zai/implementation.zh.md)

### MCP 暴露与选项
- `proxy.zai.mcp.enabled`
- `proxy.zai.mcp.web_search_enabled`
- `proxy.zai.mcp.web_reader_enabled`
- `proxy.zai.mcp.zread_enabled`
- `proxy.zai.mcp.vision_enabled`

可选的 MCP 上游专用 key：
- `proxy.zai.mcp.api_key_override`（设置后，远程 MCP 反代使用该 key 覆盖 `proxy.zai.api_key`）

Web Reader URL 归一化（可选）：
- `proxy.zai.mcp.web_reader_url_normalization` — `off | strip_tracking_query | strip_query`

详情：
- [`docs/zai/mcp.zh.md`](../zai/mcp.zh.md)
