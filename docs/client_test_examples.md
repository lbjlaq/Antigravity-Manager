# 稳定性与搜索功能：测试示例指南

为了验证近期对 API 400 错误及“搜索文件错误”的修复效果，您可以在 Claude CLI (Claude Code) 中运行以下指令进行实测。

## 1. 验证搜索工具自愈 (Grep/Glob Fix)

针对之前的 "Error searching files" 问题，这些指令将触发 `Grep` 和 `Glob` 工具调用，并验证参数映射是否正确。

### 测试指令示例
*   **指令 A**：`在当前目录中搜索包含 "fn handle_messages" 的 Rust 文件。`
    *   *验证点*：检查代理是否能正确将 `query` 映射为 `pattern`，并注入默认的 `path: "."`。
*   **指令 B**：`列出 src-tauri 目录下所有 .rs 文件。`
    *   *验证点*：验证 `Glob` 工具名是否被正确识别，且路径过滤逻辑正常。

---

## 2. 验证协议顺序与签名稳定性 (Thinking/Signature Fix)

针对之前的 `Found 'text'` 和 `Invalid signature` 400 错误。

### 测试指令示例
*   **指令 A（推理+搜索）**：`分析本项目中处理云端请求的核心逻辑，按调用顺序总结，并给出关键代码行的 Grep 搜索证据。`
    *   *验证点*：验证在“思维 -> 工具调用 -> 结果 -> 继续思维”循环中，块顺序是否正确。
*   **指令 B（历史记录重试）**：在长对话中频繁切换模型，观察系统是否在 400 报错时静默修复签名并重试。

---

## 附录：深度错误对照与修复方案

| 错误类别 | 具体报错特征码 (Error Detail) | 代理采取的修复/应对逻辑 |
| :--- | :--- | :--- |
| **消息流顺序违规** | `If an assistant message contains any thinking blocks... Found 'text'.` | **已修复**：`streaming.rs` 不再允许在文字块之后非法追加思维块。 |
| **思维签名不匹配** | `Invalid signature in thinking block` | **已修复**：优先保留原始名称以保护 Google 后端签名校验。 |
| **思维签名缺失** | `Function call is missing a thought_signature` | **已修复**：自动注入 `skip_thought_signature_validator` 占位符。 |
| **非法缓存标记** | `thinking.cache_control: Extra inputs are not permitted` | **已修复**：全局剔除历史消息中的 `cache_control` 标记。 |
| **Plan Mode 报错**| `EnterPlanMode tool call: InputValidationError: Extra inputs are not permitted` | **已修复**：`streaming.rs` 强制清空工具参数以符合官方无参协议。 |
| **连续 User 消息**| `Consecutive user messages are not allowed` | **已修复**：`merge_consecutive_messages` 自动合并相邻同角色消息。 |

---

## 3. 验证 Claude Code Plan Mode 与角色交替 (Issue #813)

针对 Plan Mode 切换导致的协议报错问题。

### A. 验证 Plan Mode 激活 (UI 状态)
*   **指令**：`进入 Plan Mode 调研 src-tauri 的目录结构。`
*   **预期结果**：
    *   终端左下角应立即出现蓝色的 **`plan mode on`** 标签。
    *   日志中应看到 `[Streaming] Tool Call: 'EnterPlanMode' Args: {}`。

### B. 验证角色交替自愈 (Consecutive Messages)
*   **指令**：`在 Plan Mode 下帮我分析 proxy/mappers/claude/request.rs 的逻辑，然后退出 Plan Mode 并给出一个简要总结。`
*   **预期结果**：
    *   模型切换模式（如从 Plan 到 Code）时不会因“连续两条 User 消息”而报 400 错误。
    *   日志中会体现 `merge_consecutive_messages` 的合并动作。

---

## 4. QuotaData 字段逻辑解析

设置页面中的“账号管理”列表，下方的进度条数据来源于 `QuotaData`。系统会在请求前检查账号配额，并在触及阈值时自动轮换。

---

## 调试建议
```bash
RUST_LOG=debug npm run tauri dev
```
在日志中搜索 `[Claude-Request]`，关注消息角色的排列顺序。

---

## 5. 验证 Thinking 签名持久化与重启容错 (Proxy Restart Test)

此测试模拟代理服务主要逻辑：验证当代理重启（内存签名缓存丢失）后，携带旧签名的历史消息是否会导致 400 错误。这是复现 `Invalid signature` 最有效的方法。

### 测试流程
1.  **生成 Thinking (Step 1)**:
    *   **指令**：`详细分析 proxy/mappers/claude/request.rs 的代码结构，特别是它是如何处理 Thinking Block 的。请展示思维过程。`
    *   *状态*：Claude CLI 会接收到包含 Thinking 和 Signature 的响应。

2.  **模拟环境变更 (Step 2)**:
    *   **动作**：**保持当前 Claude CLI 会话不关闭**。
    *   **动作**：在另一个终端完全重启 Antigravity (或 `npm run tauri dev`)。
    *   *原理*：重启会清空代理内存中的“签名白名单”，这意味着 Step 1 中下发的签名现在对代理来说是“未知/不可信”的。

3.  **触发历史重放 (Step 3)**:
    *   **指令**：`根据上面的分析，总结一下签名验证的核心逻辑。`
    *   *原理*：CLI 会将 Step 1 中的 Thinking Block + Signature 作为历史记录发送给重启后的代理。

### 预期结果 (验证修复)
*   **如果不通过**：报错 `Invalid signature in thinking block` (因为代理无法验证该签名，直接透传给了 Google，被 Google 拒收)。
*   **如果通过 (当前版本)**：代理发现签名不在内存缓存中，**自动触发降级逻辑**（剥离 Thinking Block 或作为纯文本发送），对话正常继续，无报错。

---

## 6. 验证动态思维剥离 (Dynamic Thinking Stripping)

此测试验证系统能否在**高 Context 压力**或**签名失效**场景下，自动剥离无用的历史 Thinking Block，从而解决 "Prompt is too long" 和 "Invalid signature" 错误。

### 前置条件
*   开启 Debug 日志: `RUST_LOG=debug npm run tauri dev`
*   确保使用支持 Thinking 的模型 (如 `claude-3-7-sonnet` 或映射后的 `gemini-2.0-flash-thinking-exp`)

### 验证场景 A: 模拟超长 Context 压力 (Simulate High Load)

此场景验证当对话历史接近 Token 上限时，系统是否会自动清理旧的 Thinking。

1.  **构造长对话**:
    *   **方法 1 (自动生成)**: 运行 `docs/generate_long_payload.sh` 生成 2MB 测试文件。
        ```bash
        chmod +x docs/generate_long_payload.sh
        ./docs/generate_long_payload.sh
        cat docs/long_context_payload.txt | pbcopy
        ```
        然后将剪贴板内容多次粘贴给 Claude，直到感知到显著延迟或收到上下文警告。

    *   **方法 2 (Deep Thinking 诱导 - 持续施压)**:
        以下 Prompts 经过设计，能诱导模型进行极长的思维推理。可以轮流发送：

        > **Round 1 (History)**: "Please analyze the history of computing from the abacus to quantum computers. For every major milestone (at least 20), perform a deep 'thinking' block simulating the thought process of the inventors. Detailed thinking is required. Aim for maximum output tokens."

        > **Round 2 (Math/Logic)**: "Prove the Riemann Hypothesis. Just kidding. But please perform a deep, step-by-step derivation of the Navier-Stokes existence and smoothness problem's core challenges. Explore 10 different mathematical approaches, evaluating the pros and cons of each in extreme detail."

        > **Round 3 (System Architecture)**: "Design a distributed system capable of handling 100 billion requests per second. Detail the consensus execution flow (Paxos/Raft) for a single transaction across 5000 nodes. Simulate the network partition handling logic in your 'thinking' process for at least 50 failure scenarios."

        > **Round 4 (Literature)**: "Write a recursive story where the protagonist is a recursive function. The story must nest at least 20 levels deep, and for each level, you must 'think' about the symbolic meaning of that recursion depth before writing the narrative part."

2.  **观察日志**:
    *   在终端搜索 `[ContextManager]`。
    *   **预期日志**:
        ```
        [INFO] [ContextManager] Context pressure: 95.0% (1900000 / 2000000), Strategy: Aggressive => Purifying history
        [DEBUG] History purified successfully
        ```

3.  **验证结果**:
    *   Request 成功发送给 Gemini，没有报 "Prompt is too long"。
    *   HTTP 响应头包含 `X-Context-Purified: true`。
    *   Claude CLI 用户侧无感（历史记录仍在 CLI 本地显示，但服务端已净化）。

### 验证场景 B: 签名失效免疫 (Signature Immunity via Stripping)

此场景验证即使不触发重试逻辑，高负载下的主动剥离也能顺带解决签名问题。

1.  **生成带签名的 Thinking**:
    *   **指令**: `思考一下 Rust 的所有权机制，写 500 字。`

2.  **重启 Proxy 且注入虚假负载 (可选)**:
    *   重启代理（清空签名缓存）。
    *   继续对话。此时带有旧签名的 Thinking 会被发送给代理。

3.  **预期结果**:
    *   如果上下文压力较大触发了 Stripping，或者因签名报错触发了 RetriedWithoutThinking，系统会剥离 Thinking Block。
    *   **关键点**: 一旦 Thinking Block 被剥离，`thought_signature` 也会随之消失。
    *   Gemini 收到的是纯文本历史，**绝不会报 Invalid Signature**。
