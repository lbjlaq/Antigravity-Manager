# z.ai（GLM）集成笔记（Anthropic passthrough + MCP + usage）

目标：将 z.ai 作为上游 provider 集成到代理/服务中，核心是 Anthropic 兼容 passthrough，并提供可选 MCP 能力与（未来）usage/budget 可视化。

说明：这是工作笔记，记录实现相关的关键点、约束与 corner cases，不复制完整上游文档。

## 0) 已确认的产品决策 / 需求
- z.ai 作为 API Proxy UI 中的可选 provider（可启用/禁用）。
- z.ai 与 Google OAuth 无关，但需要在同一个“通用代理”里与其它协议并存，便于多客户端同时使用。
- 存储：z.ai 配置/凭据保存在数据目录（与 accounts 与 `gui_config.json` 同处），不使用系统 Keychain。
- 分发策略由用户配置：
  - `exclusive`（Claude 协议全走 z.ai）
  - `pooled`（z.ai 作为共享池一个槽位）
  - `fallback`（Google 池为 0 时才用）
- MCP 通过代理作为可选开关暴露，客户端无需配置 z.ai key。
- 代理鉴权（如果启用）对整个代理生效（非按路由局部绕过）。

## 1) 上游文档入口
- Anthropic 兼容：`https://docs.z.ai/devpack/tool/claude`
- Vision MCP：`https://docs.z.ai/devpack/mcp/vision-mcp-server`
- Web Search MCP：`https://docs.z.ai/devpack/mcp/search-mcp-server`
- Web Reader MCP：`https://docs.z.ai/devpack/mcp/reader-mcp-server`
- zread MCP：`https://docs.z.ai/devpack/mcp/zread-mcp-server`
- API reference：`https://docs.z.ai/api-reference/introduction`

## 2) 已实现的内容（状态）
实现细节请见：
- [`docs/zai/implementation.zh.md`](implementation.zh.md)
- [`docs/zai/mcp.zh.md`](mcp.zh.md)
- [`docs/zai/provider.zh.md`](provider.zh.md)
- [`docs/zai/vision-mcp.zh.md`](vision-mcp.zh.md)

## 3) MCP corner cases（重点）
- SSE query-string 鉴权风险高（易泄露），应优先使用 header-based auth；本项目对上游一律使用 header 注入。
- Streamable HTTP MCP 需要 `mcp-session-id`；反代必须转发该响应头，否则客户端无法继续会话。
- Web Reader 对 URL 格式可能敏感；提供 URL 归一化可提高兼容性（例如移除 tracking query）。
- zread 的 `repo_name` 必须为 `owner/repo` 且上游可访问；对私有/不可见仓库会失败。

## 4) Usage / budget（后续方向）
上游存在 monitor/usage 相关端点（不同 auth 形式可能不一致）。如需做“额度看板/预算可视化”，建议：
- 单独封装 monitor API client（避免与 MCP/Claude 上游混用）
- 明确 auth 格式（是否 Bearer）
- 做缓存与限流

