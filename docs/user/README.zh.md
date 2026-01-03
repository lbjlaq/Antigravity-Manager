# 用户指南

本指南面向使用者，说明应用提供的能力、在 UI 中哪里能找到对应功能，以及代理在运行时的行为规则。
如果你需要维护代码，建议从 `docs/README.zh.md` 开始，并结合 `docs/proxy/*` 与 `docs/zai/*` 下的实现/行为说明。

## 这是什么

应用会在本机启动一个 **API Proxy**，暴露多个“协议面”，让不同工具都能通过同一个代理访问：
- **Claude 协议**（Anthropic 兼容）：`POST /v1/messages`、`POST /v1/messages/count_tokens`
- **OpenAI 协议**（兼容层）：`POST /v1/chat/completions`、`POST /v1/completions`、`POST /v1/responses`、图片相关端点
- **Gemini 协议**（Google 原生）：`/v1beta/*`
- 可选的 **MCP**：`/mcp/*`

代理请求可由以下提供：
- **Google 账号池**（轮询账号、刷新 token、跟踪配额），以及/或者
- 可选 **z.ai（GLM）提供商**（仅用于 Claude 协议的 passthrough）。

## 配置文件存储位置（本机）

应用将配置本地保存：
- 主配置：`~/.antigravity_tools/gui_config.json`
- Google 账号：`~/.antigravity_tools/accounts/*.json`

不要提交或分享这些文件。

## UI：API Proxy 页面（主界面）

打开 **API Proxy** 页面可以管理：

### 1）启动/停止 + 状态
- 启动/停止代理服务
- 查看 base URL（通常为 `http://127.0.0.1:<port>`）、端口、运行状态

### 2）访问授权（全局）
位置：**Service Configuration → Authorization**

开启鉴权后，客户端需发送：
- `Authorization: Bearer <proxy.api_key>`（推荐），或
- `x-api-key: <proxy.api_key>`（兼容某些工具）。

模式说明：
- `off`：开放模式（不鉴权）
- `strict`：全量鉴权（所有路由都要求密钥）
- `all_except_health`：除 `GET /healthz` 与 `GET /health` 外都鉴权
- `auto`：推荐模式：仅本机访问时默认开放；开启 LAN 访问时默认“除健康检查外”

详情：`docs/proxy/auth.zh.md`

### 3）请求日志（安全访问日志）
位置：**Service Configuration → Request Logging**

这是“安全访问日志”：
- 仅记录 method/path/status/latency
- 不记录 query/header/body

详情：`docs/proxy/logging.zh.md`

### 4）响应归因头（可选）
位置：**Service Configuration → Response Attribution Headers**

开启后，代理会在响应中加入脱敏的 Header：
- `x-antigravity-provider`：`google` 或 `zai`
- `x-antigravity-model`：解析后的上游模型 ID（尽力提供）
- `x-antigravity-account`：Google 账号池请求的匿名化账号 ID（ASCII 兼容，例如 `abcd...wxyz`）

适用于多工具并行调用时定位“到底是谁在提供响应”。

详情：`docs/proxy/routing.zh.md`

### 5）诊断接口
代理提供轻量诊断接口：
- `GET /healthz`（标准入口）
- `GET /health`（别名）
- `GET /test-connection`（检查 Google 账号池是否能选出账号；返回已脱敏）

说明：
- 是否需要鉴权由 `auth_mode` 决定
- `all_except_health` 仅对 `/healthz` 与 `/health` 开放；`/test-connection` 仍需鉴权

### 6）“Now serving / Recent usage”
位置：**API Proxy → Runtime / Recent usage 面板**

无需开启 payload 记录，也能看到最近请求的归因信息：
- provider（`google` / `zai`）
- resolved model
- account id / 已脱敏邮箱（Google 账号池）

### 7）模型路由中心（Model Router）
位置：**Model Router**

代理支持多层映射：
- 系列分组映射（group keys）
- Claude 家族/档位映射：`claude-opus-family`、`claude-sonnet-family`、`claude-haiku-family`
- 自定义精确映射（优先级最高）

这些映射影响 Google 路径的路由。z.ai 有自己的模型映射区（仅当 z.ai 启用时生效）。

#### Series Groups 之间如何相互作用（重要）

你在界面里看到的 8 个 “Series Groups” 卡片并不是同一类规则：
- **Claude 分组**会写入 `proxy.anthropic_mapping`（Claude 协议 → Google 路径）。
- **OpenAI 分组**会写入 `proxy.openai_mapping`（OpenAI 兼容协议 → Google 路径）。

这两张映射表彼此独立，不会互相覆盖。

优先级（当多个规则可能命中时谁生效）：
1) `proxy.custom_mapping` 精确匹配优先级最高（覆盖所有）。
2) Claude 家族/档位规则（`claude-*-family`）优先于 Claude 系列规则（`claude-*.?-series`）。
3) Claude 系列规则仅在未命中家族规则（或家族未配置）时生效。
4) 若都未命中，则使用系统内置默认映射。

注意点：
- 这些分组规则只影响 **Google 路径**。如果请求被路由到 **z.ai passthrough**，则使用 z.ai 自己的映射规则。
- OpenAI 的 “GPT-4o / 3.5” 分组会覆盖很多 `turbo`/`mini` 变体；因此像 `gpt-4-turbo` 这类名字会按该分组处理（这是设计行为）。
- 如果未配置 `gpt-5-series`，系统可能回退到 `gpt-4-series`（取决于你的配置）。

详情：`docs/proxy/routing.zh.md`

## z.ai（GLM）提供商（可选）

位置：API Proxy 页面中的 **z.ai（GLM）提供商** 区域。

要点：
- 仅影响 **Claude 协议** 请求（`/v1/messages`、`/v1/messages/count_tokens`）。
- Gemini 原生与 OpenAI 兼容协议仍走 Google 账号池。
- 分发模式：
  - `off`：不使用 z.ai
  - `exclusive`：所有 Claude 协议请求都走 z.ai
  - `pooled`：z.ai 作为一个槽位加入队列与 Google 账号轮询（无优先级）
  - `fallback`：仅当 Google 账号池不可用时才走 z.ai

详情：
- `docs/zai/provider.zh.md`
- `docs/zai/implementation.zh.md`

## MCP 端点（可选）

位置：API Proxy 页面中的 **z.ai MCP** 区域。

启用后，代理会暴露 `/mcp/*` 端点，并可继承同一套代理鉴权策略。

详情：`docs/zai/mcp.zh.md`

## 排障

### UI “白屏”
参考：`docs/app/frontend-logging.zh.md`

### “No available accounts / invalid_grant”
通常表示 Google refresh token 已失效/被撤销；代理会自动禁用该账号，直到重新授权。

参考：`docs/proxy/accounts.zh.md`
