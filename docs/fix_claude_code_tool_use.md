# 针对 Claude Code "Field required" 错误的修复方案文档

## 1. 问题背景
在使用 Claude Code CLI 并通过 Antigravity-Manager 代理时，经常会出现以下报错：
`messages.X.content.0.text.text: Field required`

**根本原因**：
Claude Code 在进行工具调用（Tool Use）时，发送或接收的消息块中可能包含空的 `text` 字段（例如 `{"text": ""}` 或 `{"text": "  "}`）。
- **Google Gemini API**：严禁在请求的 `parts` 中包含空的文本块。
- **Anthropic 协议转换**：在转换过程中，如果未能对空字符串进行 `trim()` 和有效性校验，就会导致上游 API 拒绝请求。

## 2. 针对性修改方案

本次修复主要涉及 `src-tauri/src/proxy/mappers/claude/` 目录下的两个核心文件：

### A. 请求端过滤 (src-tauri/src/proxy/mappers/claude/request.rs)

**修改说明**：在将 Claude 消息转换为 Google 格式时，对所有 `ContentBlock::Text` 和降级的思维块（Thinking）进行严格过滤。

*   **修改点 1 (`build_contents` 函数)**：
    *   **现状**：仅检查了是否等于 `(no content)` 占位符。
    *   **修复**：引入 `!text.trim().is_empty()`，确保所有发送给 Google 的文本块都包含实际内容。
*   **修改点 2 (思维块降级)**：
    *   **现状**：当思维模式被禁用或块顺序异常时，思维块会被转换为 `text` 块。
    *   **修复**：确保降级过程中排除掉仅含空格的文本，并对内容进行 `trim()` 处理。

### B. 响应端优化 (src-tauri/src/proxy/mappers/claude/streaming.rs)

**修改说明**：优化流式响应转换逻辑，防止向客户端发送诱发状态机异常的空块。

*   **修改点 1 (`emit_finish` 函数)**：
    *   **现状**：在处理 Web 搜索（Grounding）结果时，会初始化一个 `text` 块。
    *   **修复**：只有当搜索摘要 `grounding_text.trim()` 确实非空时，才允许发送该内容块分片。
*   **修改点 2 (`process_text` 处理器)**：
    *   **修复**：增强了对 `trailing_signature`（签名暂存）场景下的逻辑鲁棒性，确保不会产生无意义的“幽灵”文本块。

## 3. 已应用的代码详情 (Git Diff 摘要)

### Request Mapper:
```rust
// 修复前
if text != "(no content)" { ... }

// 修复后
if text != "(no content)" && !text.trim().is_empty() { ... }
```

### Streaming State:
```rust
// 修复前
if !grounding_text.is_empty() { ... }

// 修复后
let trimmed_grounding = grounding_text.trim();
if !trimmed_grounding.is_empty() { ... }
```

## 4. 验证与部署建议

1.  **分步确认**：通过代理日志观察，确保不再出现 `messages.X.text: Field required` 的 HTTP 400 警告。
2.  **连续对话测试**：进行一次涉及文件读写的复杂任务（例如 `claude fix bug`），确认在 `tool_use` 返回后，Claude Code 能正常发送下一轮请求。
3.  **编译命令**：
    ```bash
    npm run tauri dev
    ```

---
*文档由 Antigravity AI 助手生成，用于记录 fix/claude-code-tool-use-empty-text 分支的变更细节。*
