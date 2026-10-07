# RFC：Claude Desktop Cowork Deep Compact

> 官方 Compact 机制逆向、50% 保留限制解除与低延迟深度压缩

- **Issue Type**: Architectural Breakthrough / In-Depth RFC
- **Target Component**: Claude Desktop Cowork / Antigravity Gateway
- **Implementation Status**: Experimental / Opt-in
- **Primary Platform**: macOS
- **Validated Claude Client Versions**: August build / October build
- **Contributors**: @cubelikeplayDaniel & Gemini & Claude Fable 5
- **Linked Issue**: Fixes [#3576](https://github.com/lbjlaq/Antigravity-Manager/issues/3576)

---

## 0. 摘要

本项目针对 Claude Desktop Cowork 模式中的上下文 Compact 机制进行了逆向分析，并构建了一套独立的 Deep Compact 实现。

项目的核心并不是重新实现一套摘要系统，也不是简单地伪造客户端显示的 Context 数值，而是：

> **利用 Claude 客户端原生 Compact 流程完成语义摘要，同时解除官方客户端在历史消息 pruning 阶段设置的“一次 Compact 后不得低于压缩前上下文约 50%”的保留限制，从而实现更激进的上下文裁剪。**

整个系统分为两个职责完全不同的部分：

- **Gateway**：负责触发、状态管理、Compact 请求放行以及恢复闭环。
- **Client Patch**：负责修改官方 pruning 函数，使其突破官方 50% 保留下限。

因此，Binary Patch 并不是为了“让 Compact 发生”。

官方 Compact 的触发本身可以通过 Gateway 完成。

Patch 存在的根本原因是：

> **Gateway 可以控制 Compact 的进入条件与部分输入，但无法通过普通协议参数消除客户端 pruning 函数内部的 50% 保留限制。**

实验结果进一步表明，在当前已验证的客户端和工作负载下，更激进的历史裁剪能够获得非常小的请求延迟，同时实际使用效果良好。

因此，本项目最终形成的并不是“另一个 Compact”，而是：

> **官方 Compact 摘要能力 + Gateway Reactive Compact 闭环 + 可选客户端 Deep Pruning Patch**

三者组合形成的 Deep Compact 架构。

---

## 1. 原报告存在的核心问题

此前版本的报告已经正确描述了大量逆向结果，但存在一个影响架构理解的重要遗漏：

### 1.1 错误地把 Binary Patch 描述成 Deep Compact 的一般组成部分

原报告容易给人这样的印象：

```text
Deep Compact = Gateway 触发 Compact + Binary Patch
```

这种表述不够准确。

正确关系应该是：

```text
Reactive Compact = Gateway 触发官方 Compact
```

而：

```text
Deep Compact = Reactive Compact + 解除官方 pruning 深度限制
```

后者才需要 Binary Patch。

也就是说：

> **Patch 不是 Compact 的触发器，而是 Compact 的“深度解除器”。**

---

### 1.2 原报告没有明确指出官方 50% 保留限制

这是此前报告最重要的技术遗漏。

官方 Cowork Compact 并不是简单地：

```text
计算一个目标 token 数 → 一直删除旧消息 → 直到达到目标
```

它内部存在一个重要约束：

> **一次 Compact 后，历史上下文的保留结果不能低于压缩前上下文的一定比例；在目前逆向与测试中确认的关键限制表现为约 50% 下限。**

因此，即使 Gateway 通过 synthetic HTTP 400 提供更激进的 token gap，官方 pruning 函数最终仍然受到这一限制。

这意味着：

```text
400 maximum 调得更小  ≠  真正解除 pruning 下限
```

两者不是同一件事情。

---

### 1.3 因此，“Gateway-only Deep Compact”并不能完整替代 Patch

此前报告提出过一个分级架构：

- Level 0：正常 Gateway
- Level 1：Reactive Compact
- Level 2：Gateway-only Deep Compact
- Level 3：Binary Patch

这个分级作为工程策略仍然有意义，但其中 Level 2 必须重新定义。

它可以：
- 调整触发行为；
- 调整客户端看到的 token gap；
- 影响官方 pruning 的计算过程；
- 在某些情况下使 Compact 行为发生变化。

但是：

> **它不能从根本上消除官方 pruning 函数内部的 50% 保留约束。**

因此，真正意义上的 Deep Compact 仍然需要修改客户端 pruning 逻辑。

---

## 2. 项目真正解决的问题

Claude Cowork 的官方 Compact 主要解决：

> **上下文接近限制时，通过摘要历史信息释放上下文空间。**

而本项目解决的是另一个问题：

> **官方 Compact 虽然能够释放上下文，但其历史消息保留策略过于保守，导致一次 Compact 后仍然保留大量历史上下文。**

于是出现：

```text
长上下文
   ↓
官方 Compact
   ↓
生成 Summary
   ↓
保留大量最近历史
   ↓
实际 Prompt 仍然较大
   ↓
请求延迟 / Token 消耗仍然较高
```

Deep Compact 的目标是：

```text
长上下文
   ↓
官方 Compact
   ├── 官方 Summary
   │
   └── Deep Pruning
          ↓
      大幅减少历史消息
          ↓
       更小 Prompt
          ↓
       更低延迟
```

---

## 3. 已确认的 Cowork Compact 触发机制

本项目已经确认：

> **Compact 并不是简单根据本地配置中的 context window 数值直接触发。**

关键触发链路为：

```text
Claude Client
      │
      │ 普通请求
      ▼
   Gateway
      │
      │ 正常请求
      ▼
  Backend
      │
      │
      ▼
HTTP 400
"prompt is too long"
      │
      ▼
Claude Client
      │
      │ 识别为 Context Overflow
      ▼
进入原生 Compact 流程
      │
      ▼
向 Gateway 发起 Compact 请求
```

因此 Gateway 可以利用一个与客户端原生行为兼容的：

```http
HTTP 400 Bad Request
prompt is too long
```

作为 Reactive Compact 的触发器。

这使得项目无需重新实现 Claude 的摘要逻辑。

---

## 4. 为什么必须使用官方 Compact

本项目没有选择：

```text
Gateway 自己读取全部历史 → 自己调用 LLM → 自己生成 Summary → 自己重新组织上下文
```

原因是这样会重新实现 Claude 已经拥有的复杂 Compact 能力。

官方 Compact 已经处理：
- 历史对话摘要；
- 工具调用上下文；
- 对话连续性；
- 当前任务状态；
- Cowork 特有的上下文结构。

因此本项目采取：

> **复用官方语义压缩，只修改“保留多少原始历史”的策略。**

这是整个架构中非常重要的“最小改动”原则。

---

## 5. 官方 50% 保留限制

逆向过程中确认，官方 pruning 函数并不是无限制地删除旧消息。

其核心行为可以抽象为：

```text
Input:
    当前完整消息历史
    当前 token gap
    Compact 所需上下文

Process:
    从历史消息组向后遍历
    计算可保留的消息组
    保证工具调用/结果等结构完整
    同时受到官方保留比例限制

Output:
    Summary
    +
    最近的一部分历史消息
```

关键限制：

```text
Compact 后保留上下文 ≥ Compact 前上下文的一定比例 ≈ 50%
```

这就是 Deep Compact 真正需要突破的地方。

---

## 6. 为什么不能只把 HTTP 400 的 maximum 调小

这是本项目架构中最容易产生误解的地方。

例如：
- 实际上下文：300K
- Gateway 返回：`prompt is too long: 300000 tokens > 20000 maximum`

直觉上可能认为：
> “既然 gap 已经非常大，客户端应该自然删除更多历史。”

但实际上：

```text
400 maximum
      ↓
影响 pruning 输入
      ↓
官方 pruning algorithm
      ↓
仍然存在官方保留下限
      ↓
不能无限向下裁剪
```

因此：

> **修改 400 参数 ≠ 删除官方 50% 下限。**

这也是为什么真正的 Deep Compact 需要进入客户端 pruning 函数本身。

---

## 7. Client Patch 的真正作用

Patch 的目标非常单一：

> **不改变官方 Compact 的摘要逻辑，只解除官方 pruning 函数对历史保留深度的限制。**

换言之：

原始客户端：
```text
Compact → Summary → Official pruning → ≥ 50% retention
```

Patch 后：
```text
Compact → Summary → Official pruning → Custom retention target
```

例如工程目标可以设置为约：**35K active-history budget**。

需要强调：
**35K 是工程目标，而不是 Claude 官方声明的上下文限制。**

实际最终 Prompt 仍然可能包含：
- system prompt；
- tool schema；
- Summary；
- 当前用户消息；
- 工具结果；
- 其他固定上下文。

因此不能把 35K 简单等同于最终 API "input_tokens"。

---

## 8. 为什么直接修改官方 pruning 函数，而不是重写 Compact

这是整个项目最重要的工程原则之一：

> **只修改必要的控制点。**

不修改：
- Summary 生成逻辑；
- Compact 请求格式；
- 消息语义；
- 工具调用结构；
- 官方上下文组织方式。

只修改：
> **“Compact 后究竟保留多少历史消息”**

因此：

```text
Claude 官方能力 + 一个非常小的 pruning 行为修改 = Deep Compact
```

这比重新实现整个 Compact pipeline 的风险要低得多。

---

## 9. Message Group 完整性

Pruning 不能简单地按单条消息删除。

例如：
- Assistant → Tool Call
- Tool → Tool Result
- Assistant → Next Message

这些消息之间具有结构关系。

因此 pruning 必须以：

> **完整 message group**

作为基本裁剪单位。

否则可能产生：
- Tool Call 存在，Tool Result 消失；
- 或者 Tool Result 存在，对应 Tool Call 消失；
从而破坏上下文结构。

因此 Patch 的目标不是随便删除消息，而是：
> **保持官方 message-group 语义 + 改变 group retention boundary**

---

## 10. 官方 pruning 函数逆向

本项目已经对官方 pruning 函数进行了多版本逆向和验证。

不同 Claude Code / Claude Desktop 构建中观察到过不同的函数名称，例如：`khf`, `rAt`。

函数名称本身并不是可靠的识别依据。真正需要匹配的是其：
- 控制流；
- token gap 计算；
- message group 遍历；
- retention boundary；
- 返回结果结构。

目前测试表明：
> **该函数的核心逻辑在已经测试的多个版本之间没有发生根本性变化。**

但是这并不等于可以宣称“所有未来版本永远兼容”。目前客户端层面的实际验证仍主要集中在：
- August build；
- October build。

中间版本以及更早版本尚未完成完整验证。因此兼容性应定义为：
> **已验证版本兼容，而非理论上的全版本兼容。**

---

## 11. Deep Compact 状态机

Compact 并不是单纯的 `400 → Compact → 结束`。实际存在一个非常重要的问题：

> **Compact 完成后的下一次请求仍然可能再次触发 400。**

因此 Gateway 必须区分：普通 Context Overflow 与 Compact 后恢复请求。

推荐状态模型：

```text
NORMAL
   │
   │ HTTP 400
   ▼
COMPACTION_REQUESTED
   │
   ▼
COMPACTION_IN_PROGRESS
   │
   │ Summary 完成
   ▼
POST_COMPACTION_RETRY
   │
   │ 一次性豁免
   ▼
NORMAL
```

而不是：`看到 <summary> → 永久认为“已经 Compact”`。

---

## 12. 为什么不能用静态 "<summary>" 判断状态

Compact 后的 Summary 会持续存在于上下文中。

因此：
```text
if "<summary>" exists: already compacted
```
会产生错误状态：第一次 Compact 产生 Summary 后，后续所有请求都会被误认为“这是 Compact 后的请求”，导致防护失效。

正确方式应该是建立一次性的 Compaction Generation：

```rust
struct CompactionGeneration {
    id: String,
    triggered_at: Instant,
    before_tokens: u32,
    summary_seen: bool,
    retry_consumed: bool,
}
```

并使用 **atomic consume**，保证并发请求不会同时获得 Compact 后豁免。

---

## 13. Double-400 问题

这是实现过程中发现的重要边界条件：

```text
请求 A → 400 → Compact → 请求 B → 仍然超过 Gateway 判断阈值 → 再次 400
```

如果 Gateway 不知道请求 B 是 Compact 完成后的 recovery retry，那么客户端可能进入死循环：
```text
Compact → 400 → Compact → 400 → Compact ...
```

因此 Gateway 必须维护：
- Compaction Generation
- One-shot Post-Compact Retry
- Expiry (如 180 秒生命周期)
- Atomic Consumption

---

## 14. "./compact" 的定位

`./compact` 不应该被描述为“Claude CLI 原生命令”。

更准确的定义是：

> **Cowork 环境中的显式 Compact intent / control-plane trigger。**

它表达用户明确要求：“现在进行一次 Compact。” Gateway 再根据当前状态将其转换成实际 Compact 流程：

```text
./compact
   ↓
Gateway Control Plane
   ↓
Synthetic 400 / Compact Trigger
   ↓
Claude Native Compact
```

这样可以保持用户意图与底层 HTTP 行为的分离。

---

## 15. Token Estimator

Gateway 需要判断当前上下文规模，因此实现了 Token Estimator。需要综合考虑：
- 普通文本；
- 中文；
- ASCII；
- tool schema；
- tool result；
- image；
- Base64 media；
- MCP 等复杂工具上下文。

但是：
> **Estimator 是控制逻辑，不应被描述为 API 最终真实 "input_tokens"。**

例如一次实测：
- 约 334K estimated tokens → 约 45.1K estimated tokens
- 对应净释放约 289K（约 86.5%）

这是非常显著的上下文规模下降。但如果没有 API usage 数据，不能把这些数字直接称作 Anthropic API 返回的真实 "input_tokens"。

---

## 16. 实验结果：激进压缩的低延迟收益

这是此前报告中同样应该加强的部分。

Deep Compact 的目标并不是“压得越狠越厉害”，真正重要的是：

> **压缩后的上下文足够小，同时仍然保持任务连续性，而且请求延迟很低。**

当前实验已经验证：
- 更激进的历史裁剪可以大幅降低实际 Prompt；
- 压缩后的任务连续性与使用效果良好；
- 在测试工作负载中，压缩后请求取得了非常小的延迟；
- 没有观察到与这种激进裁剪规模相对应的明显交互性能损失。

因此 Deep Compact 的价值不仅是“能塞进更大的上下文”，还包括：

```text
更小 Prompt → 更少需要处理的历史上下文 → 更低请求延迟
```

这使 Deep Compact 从一个单纯的“Context Overflow workaround”变成了：
> **一种实际的上下文性能优化机制。**

---

## 17. UI Context 与 Model Context 的分离

Deep Compact 不必删除用户界面中的完整历史。

可以形成：

```text
UI Conversation
      │
      │ 完整保存
      ▼
用户仍可查看完整历史


Model Prompt
      │
      │ Deep Compact
      ▼
只发送：
Summary
+
必要的近期历史
```

因此：
> **“界面里看起来有多少历史”与“本次请求真正发送多少历史”是两个不同维度。**

这也是 Deep Compact 能够取得较低延迟的重要原因之一。

---

## 18. 为什么 Deep Compact 不是简单的“删聊天记录”

普通清理（直接删除旧消息）会直接损失语义。

Deep Compact 的流转为：
```text
旧历史 → 官方 Compact → Summary → 保留必要近期 Message Groups
```

因此被删除的是：
> **已经被 Summary 覆盖、且不再需要直接进入模型 Prompt 的原始历史。**

而不是简单地“把前面的聊天记录砍掉”。

---

## 19. 模块独立性

Deep Compact 从设计开始就是独立模块。它不应该成为 Gateway 主流程的硬依赖，而应该是：

```text
Core Gateway
      │
      ├── Normal Proxy
      │
      └── Optional Deep Compact
```

当 Deep Compact 未启用、Patch 失败、客户端版本不支持或 Gateway 状态异常时，都不应破坏普通代理功能。

---

## 20. Opt-in 与安全策略

Deep Compact 属于实验性客户端修改，因此采用 **Opt-in**（用户明确选择后才启用）。

同时遵循安全回滚流程：
```text
原始客户端 → Backup → Patch
```

Patch 失败不应该导致整个 Gateway 不可用，而应该退化为：
```text
Gateway 正常工作 + Deep Compact 不可用
```

这也是模块独立设计的重要意义。

---

## 21. Binary Patch 的边界

Binary Patch 并不意味着“任何 Claude Desktop 都可以随便修改”。需要考虑：
- 客户端版本；
- executable discovery；
- app bundle 路径；
- 文件锁；
- 更新机制；
- 签名；
- integrity check；
- macOS hardened runtime；
- 重新签名；
- patch offset；
- 等长替换；
- rollback。

目前 macOS 是主要实现平台。在 Windows/Linux 上：
> **pruning 函数本身的核心逻辑已经验证具有一致性，但客户端发现、路径锁定、签名/完整性以及平台特有工程仍未完成。**

因此 Windows/Linux 目前不能描述成完整可用版本。

---

## 22. 为什么采用等长 Patch

如果修改后的机器码长度与原机器码保持一致：

```text
Before: AAAA BBBB CCCC DDDD
After:  AAAA XXXX YYYY DDDD
```

则不会因为插入/删除字节导致后续 offset 全部改变，降低了 Patch 的复杂度。

但：
> **等长 Patch 不等于“没有完整性风险”。**

它只能解决二进制布局变化问题，不能自动保证签名仍然有效、Hardened Runtime 行为不变、更新程序不覆盖等。因此必须将 **Binary correctness** 与 **Platform integrity** 分开讨论。

---

## 23. 当前兼容性状态

| 项目 | 状态 |
| :--- | :--- |
| Cowork Compact 触发链 | 已验证 |
| Synthetic HTTP 400 触发 | 已验证 |
| Compact 请求进入 Gateway | 已验证 |
| 官方 Summary 流程复用 | 已验证 |
| 官方 pruning 核心逻辑逆向 | 已验证 |
| 50% 保留限制 | 已验证为当前目标中的关键限制 |
| Deep Pruning Patch | 已实现/验证 |
| 激进压缩效果 | 已验证 |
| 激进压缩延迟 | 已验证为很小 |
| August Claude Client | 已验证 |
| October Claude Client | 已验证 |
| 中间版本 | 未完整验证 |
| 更早版本 | 未完整验证 |
| macOS 工程 | 主要实现平台 |
| Windows/Linux pruning 逻辑 | 已确认一致 |
| Windows/Linux 客户端工程 | 尚未完成 |
| Gateway 独立运行 | 已设计 |
| Backup/Rollback | 已设计 |
| Opt-in | 已设计 |

---

## 24. 测试矩阵

完整工程测试应至少覆盖：

### Context Size
- 20K
- 50K
- 100K
- 200K
- 300K
- 500K

### 内容类型
- 纯文本
- 长对话
- Tool Call
- Tool Result
- MCP
- 图片
- Base64
- 大型单轮工具结果

### 状态机
```text
正常请求 → 400 → Compact → Summary → Recovery Retry
```

并测试：
- 重复 400
- 并发请求
- 网络断开
- Compact 中断
- 客户端重启
- Gateway 重启
- Patch 失败
- Patch 回滚

---

## 25. 成功条件

不应该使用 `num_msgs <= 10` 作为成功标准，因为消息数量本身并不代表 Compact 是否成功。

更合理的语义条件是：
- Summary 已生成
- AND post_tokens < pre_tokens
- AND 达到目标 reduction / budget
- AND 属于当前 Compaction Generation
- AND Recovery Retry 已正确完成

这比单纯检查消息数量可靠得多。

---

## 26. 三层验证体系

项目中的结论应该分为三种：

- **A. 已直接验证**：例如 HTTP 400 可以触发 Cowork Compact。
- **B. 已在特定版本验证**：例如 August Client, October Client。
- **C. 理论推导 / 尚未完成验证**：例如所有未来 Claude Desktop 版本均兼容。

严禁把 C 类描述成 A 类。

---

## 27. 本项目真正的架构创新

本项目并不是简单修改 Claude，而是把 Compact 分成了几个独立层：

```text
┌─────────────────────────────┐
│        User Intent          │
│          ./compact          │
└──────────────┬──────────────┘
               │
               ▼
┌─────────────────────────────┐
│       Gateway Control       │
│  400 / State / Generation   │
└──────────────┬──────────────┘
               │
               ▼
┌─────────────────────────────┐
│    Claude Native Compact    │
│        Summary Logic        │
└──────────────┬──────────────┘
               │
               ▼
┌─────────────────────────────┐
│    Patched Deep Pruning     │
│    Remove 50% lower bound   │
└──────────────┬──────────────┘
               │
               ▼
┌─────────────────────────────┐
│       Smaller Prompt        │
│       Lower Latency         │
└─────────────────────────────┘
```

其中每一层只承担一个职责。

---

## 28. “如非必要，勿增实体”原则

整个设计遵循非常明确的工程原则：

> **能复用官方机制，就不重新实现。**

- 不重新实现：Summary Generator
- 不重新实现：Claude Compact Protocol
- 不重新实现：Message Semantics
- 只修改：Retention Boundary

Gateway 也只负责：Trigger, State, Recovery。这使整个系统的改动面尽可能小。

---

## 29. 对旧报告的最终勘误

原报告最主要的问题并不是技术方向错误，而是没有把“为什么需要 Patch”讲清楚。应修改以下表述：

### 原来的隐含逻辑
```text
需要 Deep Compact → 所以修改 Binary
```

### 正确逻辑
```text
需要触发 Compact → Gateway Synthetic 400 即可
需要官方摘要 → 直接复用 Native Compact
需要比官方 Compact 更激进 → 发现官方 pruning 存在约 50% 保留下限
需要突破该下限 → Patch pruning function
Patch 后 → 得到真正的 Deep Compact
```

因此：
> **Binary Patch 不是整个系统的起点，而是解决官方 Compact 深度不足的最小必要修改。**

这是旧报告必须修正的核心。

---

## 30. 最终架构定义

最终可以将本项目定义为：

> **Claude Desktop Cowork Deep Compact 是一种基于 Gateway Reactive Compact 与可选客户端 pruning patch 的上下文压缩架构。它通过兼容 Claude 原生 Compact 的 HTTP 400 触发机制进入官方 Compact 流程，复用官方 Summary 能力，同时通过修改客户端历史消息 pruning 函数解除官方一次 Compact 后约 50% 的上下文保留下限，使系统能够采用更激进的历史裁剪目标。**

其核心不是自己做一个 Compact，而是：
- 让官方 Compact 继续负责“理解并总结过去”
- 让 Gateway 负责“什么时候进入 Compact”
- 让 Patch 负责“官方允许保留多少历史”

最终得到：
> **官方语义压缩 + 自定义历史保留深度 + 更小实际 Prompt + 低延迟**

---

## 31. 结论

目前项目已经形成一个完整的、可解释的技术闭环：

```text
Context Overflow
      ↓
Synthetic HTTP 400
      ↓
Claude Native Compact
      ↓
官方 Summary
      ↓
官方 pruning
      ↓
突破 50% retention limit
      ↓
Deep Pruning
      ↓
~35K 级工程目标
      ↓
显著减少实际 Prompt
      ↓
低延迟
```

实验已经证明：
> **在当前已验证的客户端版本和测试工作负载下，激进 Deep Compact 可以取得非常小的请求延迟，并保持良好的实际使用效果。**

因此，本项目的技术价值并不是单纯“把上下文压得更小”。更准确地说，它实现的是：
> **在不重新实现 Claude Compact 语义摘要能力的前提下，解除官方历史保留策略的深度限制，并通过 Gateway 与客户端 Patch 的最小协作，使 Cowork 获得更激进、更低延迟的上下文管理能力。**

当前版本定位为 **Experimental / Opt-in**：
- macOS 为主要实现平台；
- August / October 客户端已经验证；
- 其他客户端版本仍需继续验证；
- Windows/Linux 的 pruning 逻辑已确认，但客户端工程尚未完成；
- 模块具有独立性；
- Patch 失败不应影响普通 Gateway；
- Backup / Rollback 作为客户端修改的基础安全机制。

在这些边界条件下，Deep Compact 已经从“逆向实验”进入了一个具有实际性能收益、明确技术边界和可工程化路径的架构原型阶段。
