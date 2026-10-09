# Claude Code 与 Antigravity 网关长流断裂、重试风暴与会话中毒深度排查诊断全书

> **文档版本**：v2.0 (全量反编译深度演化版)  
> **逆向目标**：Claude Code 官方发行版 v2.1.284 ~ v2.1.295（重点解构本地运行环境 v2.1.293 原生 Darwin ARM64 Bun 内嵌运行时）  
> **对齐分支**：`beta` (PR #3634)  
> **关联 Issue**：Issue #3633, Issue #3635  

---

## 目录
1. [错误症状全景总结 (Symptoms Catalog)](#一错误症状全景总结-symptoms-catalog)
2. [客户端架构逆向解构：14 核心机制证据链](#二客户端架构逆向解构14-核心机制证据链)
   - [2.1 过载判定 `PU(e)` 源码与 30 分钟惩罚性冷冻](#21-过载判定-pue-源码与-30-分钟惩罚性冷冻)
   - [2.2 指数抖动退避计算引擎 `vC` / `LF` 与 27 分钟死锁数学公式](#22-指数抖动退避计算引擎-vc--lf-与-27-分钟死锁数学公式)
   - [2.3 核心终结三元组契约 `qu !== null && zb && El === null`](#23-核心终结三元组契约-qu--null--zb--el--null)
   - [2.4 流失败原因分类器 `ufr(e)` 11 类分类体系](#24-流失败原因分类器-ufre-11-类分类体系)
   - [2.5 `progress` 5 级状态梯度与 `Qnn` 核心短路豁免模型](#25-progress-5-级状态梯度与-qnn-核心短路豁免模型)
   - [2.6 决策引擎 `lfr(...)` 全分支状态迁移矩阵与 `qz` 拦截陷阱](#26-决策引擎-lfr-全分支状态迁移矩阵与-qz-拦截陷阱)
   - [2.7 会话历史中毒机制 `resumedFromIncompleteThinking` 触发条件与阻断数学证明](#27-会话历史中毒机制-resumedfromincompletethinking-触发条件与阻断数学证明)
   - [2.8 会话转录本 (Transcript) 存储架构真相：纯 JSONL + .meta.json (无 SQLite)](#28-会话转录本-transcript-存储架构真相纯-jsonl--metajson-无-sqlite)
   - [2.9 澄清真相：官方不存在 `/undo` 斜杠命令与 `/compact` 5 级数据清洗自愈管道](#29-澄清真相官方不存在-undo-斜杠命令与-compact-5-级数据清洗自愈管道)
   - [2.10 `message_stop` 后的静默防火墙契约与 `TJ("closed")` 格式异常](#210-message_stop-后的静默防火墙契约与-tjclosed-格式异常)
   - [2.11 传输层 SSE `event: error` 即刻抛错与协议中断机制](#211-传输层-sse-event-error-即刻抛错与协议中断机制)
   - [2.12 HTTP 429 限流与 `Bur = 60000` (60s) 硬截断崩溃陷阱](#212-http-429-限流与-bur--60000-60s-硬截断崩溃陷阱)
   - [2.13 非流式降级分支 (`retryWithoutStreaming`) 与 `jAt(e)` 强契约校验](#213-非流式降级分支-retrywithoutstreaming-与-jate-强契约校验)
   - [2.14 SSE 保活心跳帧定界规范与双换行 `\n\n` 约束](#214-sse-保活心跳帧定界规范与双换行-nn-约束)
3. [Antigravity 网关交互准则与绝对红线](#三antigravity-网关交互准则与绝对红线)
4. [版本选型建议与用户自愈指引](#四版本选型建议与用户自愈指引)
5. [9 月历史回退的反思与现代融合架构](#五9-月历史回退的反思与现代融合架构)

---

## 一、错误症状全景总结 (Symptoms Catalog)

在长耗时大算力生成任务（典型如高细节坐标 SVG、深度系统架构推理、批量代码重构）中，中间网络或 Google GFE 往往在 100~116 秒发生 TCP 静默或断连。在未适配最新 Claude Code 状态机时，客户端会呈现以下 4 大恶性症状：

| 症状代号 | 客户端表象 | 触发场景 | 根本技术诱因 | 严重程度 |
| :--- | :--- | :--- | :--- | :--- |
| **S1: 重试风暴死循环** | 终端出现橙色警告并倒计时：<br>`Retrying 6/10 · 27m 24s` 或 `Retrying 8/10 · 16m+` | 长耗时流中途中断，网关透传底层 Err 或合成下发 `overloaded_error` | 触发客户端 `PU(e)` 过载判定，激活 `ofr = 1800000ms`（30分钟）冷冻期，叠加 `vC()` 10 阶指数退避 | 致命（卡死会话近半小时） |
| **S2: 会话永久中毒** | 当前轮次中断后，用户输入任何新问题，该会话**持续秒报 400 错误**；但**新建空白对话完全正常** | 断流发生在思考（Thinking）块之后、正文尚未产生或未合法闭合 | 客户端未收齐三元组，标记 `resumedFromIncompleteThinking: true` 落盘，后续请求缺失 Thought Signature 遭 Gemini 拒答 | 致命（本地历史上下文报废） |
| **S3: 幽灵 429 与秒级崩溃** | 终端弹出 `api_request_retry_after_too_long` 异常硬崩溃退出，或报账户额度超限 | 网关向客户端下发的 429 响应中 `Retry-After` 大于 60 秒 | 命中客户端非 Watchdog 模式下的硬上限常量 `Bur = 60000ms`（60s），客户端直接抛错拒绝等待 | 严重（客户端进程异常崩溃） |
| **S4: 客户端主动掐线 (Client RST)** | 终端在无报错提示下突然截断停止输出，网关收到下游 Broken pipe | 网关等待上游模型计算期间长时间静默未推流 | 客户端空闲看门狗触发 `monotonicNow() - Cy >= uon()` 超时，客户端主动发送 TCP RST 销毁连接 | 严重（生成物变为残缺代码） |

---

## 二、客户端架构逆向解构：14 核心机制证据链

### 2.1 过载判定 `PU(e)` 源码与 30 分钟惩罚性冷冻
基于对 Claude Code v2.1.293 内置运行时核心偏移地址 `186169512` 的反编译：

```javascript
// 核心过载判定函数: PU(e)
function PU(e) {
  if (!(e instanceof xt)) return !1;
  return e.status === 529 || (e.message?.includes('"type":"overloaded_error"') ?? !1);
}
```

- **判定要素**：
  1. `e` 必须为 Anthropic SDK 核心 `APIError` 实例（`xt`）；
  2. 满足 `e.status === 529` **或** 错误消息字符串包含字面量 `"type":"overloaded_error"`。
- **级联反应与 30 分钟冷冻期**：
  ```javascript
  let qo = sfr(Jt), uo = qo !== null && qo < LKe; // LKe = 20000ms (20s)
  if (!uo) {
    let ko = Math.max(qo ?? ofr, rfr); // ofr = 1800000 (30分钟!), rfr = 600000 (10分钟!)
    let ir = PU(Jt) ? "overloaded" : "rate_limit";
    ops(Date.now() + ko, ir); // 写入本地冷却期熔断时间戳
    if (jo()) h.fastMode = !1; // 强制关闭 Fast Mode
    continue;
  }
  ```
  **结论**：只要错误载荷中包含 `"type":"overloaded_error"`，若未携带小于 20 秒的 `retry-after`，客户端直接进入长达 **30 分钟 (`1,800,000ms`)** 的惩罚性冷却状态。

---

### 2.2 指数抖动退避计算引擎 `vC` / `LF` 与 27 分钟死锁数学公式
客户端在 `chunk-k2e8p61g.js` 中实现了精确的指数比例抖动算法：

```javascript
// 退避核心算法引擎
function vC({attempt: t, baseMs: r, capMs: e = 1/0, factor: o = 2, jitter: n = {kind: "none"}, random: m = Math.random}) {
  let i = Math.min(e, r * o**(t - 1));
  switch (n.kind) {
    case "none": return i;
    case "proportional": return i + m() * n.ratio * i; // 正向 0 ~ +25% 比例抖动
    case "symmetric": return Math.max(0, i + i * n.ratio * (2 * m() - 1));
    case "full": {
      let a = Math.min(n.floorMs, i);
      return a + m() * (i - a);
    }
  }
}

// API 重试封装函数
function LF(e, r, a = 32000, s = Math.random, n = me) { // me = 500ms
  let o = Math.round(vC({attempt: e, baseMs: n, capMs: a, jitter: {kind: "proportional", ratio: 0.25}, random: s}));
  if (r) {
    let i = parseInt(r, 10);
    if (!isNaN(i)) return Math.max(i * 1000, o);
  }
  return o;
}
```

- **数学公式**：
  $$\text{Delay}_{\text{base}} = \min(32000, 500 \times 2^{\text{attempt}-1})$$
  $$\text{Delay}_{\text{actual}} = \text{Delay}_{\text{base}} \times (1 + 0.25 \times \text{random}())$$
- **重试上限计算 (`eVe`)**：
  ```javascript
  var yur = 10;    // 默认交互模式上限: 10 次
  var IKe = 15;    // CLAUDE_CODE_MAX_RETRIES 硬截断限制: 15 次
  var _ur = 300;   // Watchdog 模式上限: 300 次
  ```
  默认情况下，客户端执行 **10 次** 重试。在命中 `ofr` 冷冻窗口叠加指数退避后，10 次重试累计等待时长精准落在 **27 ~ 29 分钟**，形成终端无法退出的假死风暴。

---

### 2.3 核心终结三元组契约 `qu !== null && zb && El === null`
在流事件循环生成器 `Bfr` 与仲裁器 `Qnn` 中，流的完备性取决于严格的三元组逻辑：

$$\text{complete: } qu \neq \text{null} \land zb \land El === \text{null}$$

1. **`qu` (Stop Reason)**：
   - 必须为有效字符串（`"end_turn"`、`"tool_use"`、`"max_tokens"`、`"stop_sequence"`）；
   - 必须通过 `message_delta` 的 `delta.stop_reason` 下发。
2. **`zb` (Terminal Marker)**：
   - 布尔标志位。初始为 `false`；在收到合法携带 `stop_reason` 的 `message_delta` 时置为 `true`；
   - 若随后出现乱序未闭合块事件，会被强制重置为 `false`。
3. **`El` (Open Block Pointer)**：
   - 处于打开状态的内容块索引指针；
   - 收到 `content_block_start` 时赋值为当前块索引 `El = index`；
   - **仅在收到 `content_block_stop` 时重置为 `null`**。
   - **致命断言**：若流结束（EOF）时 `El !== null`，客户端直接执行：
     ```javascript
     if (uf && El !== null) {
       throw t(`Stream ended cleanly inside open block ${El} (stop_reason=${qu}) — treating as a dropped connection`), new cwr;
     }
     ```
     抛出 `StreamTruncatedError` 并判定为异常截断！

---

### 2.4 流失败原因分类器 `ufr(e)` 11 类分类体系
当流中断或报错时，函数 `ufr(e)` 按确定性优先级将原因解析为以下 11 种枚举值：

| 原因代码 (`cause`) | 判定前置条件 | 典型根因 |
| :--- | :--- | :--- |
| `1. "denied"` | `e.denied` 为 true | DLP 审查拦截或指令逃逸检测阻断 |
| `2. "outputFiltered"` | `e.outputFiltered` 为 true | 命中客户端本地敏感内容过滤规则 |
| `3. "overloaded"` | `PU(e.error)` 命中 | 响应包含 529 或 `"type":"overloaded_error"` |
| `4. "timedOut"` | `e.isServerError` 且 `error.type === "timeout_error"` | 上游网关返回 504 Gateway Timeout |
| `5. "serverError"` | `e.isServerError` 命中 (500, 502, 503 等) | 网关崩溃或服务异常 |
| `6. "stalled"` | `e.stalled` 看门狗标记为 true | 30 秒流式静默无 chunk 产生 |
| `7. "truncated"` | `e.isConnectionError` 且 `e.truncated` (命中 `cwr`) | 块未关闭或信封未关闭即遭遇 EOF |
| `8. "suspended"` | `e.isConnectionError` 且 `uQt(e.error)` 命中 | 操作系统休眠挂起导致 Socket 破裂 |
| `9. "connectionLost"` | `e.isConnectionError` 命中底层网络集合 `P2` | TCP RST、Broken pipe、ECONNRESET |
| `10. "malformed"` | `e.malformed` 命中 (抛出 `TJ`) | 收到重复关闭块或未启动块的 delta |
| `11. "badRequest"` | `e.error instanceof xt` 且非网络层异常 | HTTP 400 系列参数非法或签名失效 |

---

### 2.5 `progress` 5 级状态梯度与 `Qnn` 核心短路豁免模型
客户端定义了 5 级输出进度：

```javascript
let n = e.anyBlockFinished 
  ? (e.anyOutputShown ? "output" : "thinkingOnly") 
  : (e.anyOutputShown ? "partialOutput" : (e.anyEvent ? "started" : "nothing"));
```

- **`"output"`**：至少 1 个块已关闭且展示了正文/工具调用（最安全状态）；
- **`"thinkingOnly"`**：已有块关闭，但全部为思考块，正文未吐出（极度危险状态）；
- **`"partialOutput"`**：正文正在吐字，但首个块尚未收到 `content_block_stop` 闭合；
- **`"started"`**：收到过首包（如 `message_start` 或打开的思考块），但无块关闭且无正文；
- **`"nothing"`**：尚未收到任何 SSE 数据帧。

#### `Qnn` 核心短路豁免守卫：
```javascript
if (e.complete) {
  if (r === "connectionLost" || r === "suspended" || r === "truncated" || r === "stalled" || 
     ((r === "overloaded" || r === "serverError" || r === "timedOut") && n === "output")) {
    return; // 关键：返回 undefined！
  }
}
```
**数学豁免事实**：只要 `complete === true` 且 `progress === "output"`，无论遭遇网络断开、超时还是 529，`Qnn` 均直接返回 `undefined`！外层 `Mp` 被赋予 `"keepPartial"`，**彻底绕过 `onStreamFailed`，绝不触发任何重试！**

---

### 2.6 决策引擎 `lfr(...)` 全分支状态迁移矩阵与 `qz` 拦截陷阱
若未满足豁免条件，错误进入 `lfr` 决策矩阵：

| `progress` 阶段 | `cause` 故障原因 | 客户端决策 (`lfr`) | 下游行为 |
| :--- | :--- | :--- | :--- |
| **`"output"`** | 除 `badRequest` 外的所有异常 | `decision: "keepPartial"` | 优雅截断，保留已有内容，交还控制权 |
| **`"thinkingOnly"`** | `overloaded` | `nVe(g, h, b)` | **27+ 分钟重试风暴死循环** |
| **`"thinkingOnly"`** | `connectionLost` / `truncated` / `malformed` | `VN("afterThinkingOnly", 2)` | 重试 2 次后强制落盘并**标记会话中毒** |
| **`"thinkingOnly"`** | `timedOut` (504) | `VN("afterThinkingOnly", 1)` | 重试 1 次后失败 |
| **`"thinkingOnly"`** | `serverError` (500) | `VN("afterThinkingOnly", 2)` | 重试 2 次后失败 |
| **`"partialOutput"`** / **`"started"`** | 任意连接或服务错误 | `Qhe(...)` | 进入非流式降级 (`retryWithoutStreaming`) |

#### `qz` 拦截标志位的致命陷阱：
```javascript
let qz = Ru?.progress === "thinkingOnly" && Ru.cause === "overloaded" && !Ru.stopReasonReceived;
if (Ru === void 0 || Ru.progress === "output" || Ru.progress === "thinkingOnly" && !qz) {
  // 正常 keepPartial 优雅退出逻辑
}
```
若断流发生在 `thinkingOnly` 阶段且错误归因于 `overloaded`，`qz` 为 `true`，**将强行阻止客户端进入正常收尾分支**，迫使其无条件坠入 27 分钟死循环。

---

### 2.7 会话历史中毒机制 `resumedFromIncompleteThinking` 触发条件与阻断数学证明
通过对函数 `bp(e, o)` 与 `Q4o(...)` 的反编译分析：

```javascript
function bp(e, o) {
  let n = e.filter((s) => !s.isApiErrorMessage && !l0e(s)),
      r = n.length === 1 ? n[0] : void 0;
  if (r === void 0 || r.message.content.length !== 1) return !1;
  let u = r.message.content[0];
  return u !== void 0
      && Fme(u) // u.type === "thinking" || u.type === "redacted_thinking"
      && VEn(u) // 必须有合法签名
      && r.message.stop_reason === "max_tokens"
      && Q4o(o, r.message.resumable, UP(r.message.usage));
}
```

- **中毒触发链条**：
  1. 思考断流发生后，若客户端仅记录到残缺思考块（`content.length === 1` 且无正文）；
  2. 进程退出或下一轮输入刷盘时，Assistant 消息被标记：
     `if ($e && E.type === "assistant" && !E.isApiErrorMessage) E.resumedFromIncompleteThinking = !0;`
  3. 下一轮提问时，客户端将该残缺思考块回传给上游。因为 Google Gemini 原生不具备 Anthropic 的私有加密思考块连续签名（Thought Signature），上游校验失败直接返回 **HTTP 400 Bad Request**。
- **网关阻断数学证明**：
  网关在思考断流时执行策略——**“显式闭合思考块，注入合成正文块，下发 `end_turn`”**。
  此时助手消息的内容变为 `[Thinking, Text]`，`content.length = 2`。
  由于 `r.message.content.length !== 1`，`bp(...)` 在首个条件分支恒返回 `false`，从而在数学层面彻底粉碎了 `resumedFromIncompleteThinking` 的产生前置条件！

---

### 2.8 会话转录本 (Transcript) 存储架构真相：纯 JSONL + .meta.json (无 SQLite)
经过对 Bun 运行时模块与存储引擎的反编译排查，澄清先前流传的技术误解：
- **无本地 SQLite 参与**：Claude Code 业务层转录本**完全未采用 SQLite**；
- **真实文件体系**：
  - 会话数据：`~/.claude/projects/<projectKey>/<sessionId>.jsonl`
  - 会话元数据：`~/.claude/projects/<projectKey>/<sessionId>.meta.json`
  - 子智能体：`~/.claude/projects/<projectKey>/<sessionId>/subagents/agent-<agentId>.jsonl`
- **流式拦截器 `n6t`**：
  ```javascript
  function n6t(e, n, r) {
    if (!r) return e.length;
    for (let s = n; s < e.length; s++) {
      let g = e[s];
      if (g.type === "assistant" && g.message.stop_reason === null) return s;
    }
    return e.length;
  }
  ```
  在流进行期间，`stop_reason === null` 的未完结助手消息被 `n6t` 严格拦截、**禁止落盘**。但一旦遭遇异常掐线且进程退出，强制刷盘会使残缺消息落盘写入 `.jsonl`，导致持久化污染。

---

### 2.9 澄清真相：官方不存在 `/undo` 斜杠命令与 `/compact` 5 级数据清洗自愈管道
反编译全量命令注册表（`type: "local"` 与 `type: "local-jsx"`）确认：
- **Claude Code 官方命令集中不存在 `/undo` 斜杠命令**。用户此前理解的 undo 实际是快捷键 `ctrl+_`（输入框单行撤销）或历史栈内存回退；
- **真正的会话级清洗管道为 `/compact`**。客户端在执行 `/compact` 时按序调用 5 级自愈管道：
  1. `Jjr`：消除思考块间的空白文本块并补占位符；
  2. `oPn` / `nPn`：过滤未闭合且无签名的孤立残缺思考块（抛出 `tengu_filtered_orphaned_thinking_message`）；
  3. `ZAn`：剥除末尾残留的孤立思考块，补入 `[No message content]`，杜绝 Anthropic 400 校验违规；
  4. `tPn`：消除空白 Assistant 消息并执行 `_I` 缝合相邻 User 角色；
  5. `Zjr`：对空 Assistant 消息补入合法文本兜底。
- **自愈指引**：对于历史损坏的会话，用户执行 `/compact` 或 `/clear` 即可激活上述清洗管道完成无损自愈。

---

### 2.10 `message_stop` 后的静默防火墙契约与 `TJ("closed")` 格式异常
在事件消费循环 `xet` 中：
```javascript
case "content_block_delta": {
  let Ba = Jg[us.index];
  if (!Ba || Ol.has(Ba)) {
    throw i("tengu_streaming_error", {
      error_type: Ba ? "content_block_closed_delta" : "content_block_not_found_delta"
    }), new TJ(Ba ? "closed" : "unstarted");
  }
}
```
- **关键机制**：收到 `content_block_stop` 后，块被存入已关闭集合 `Ol`。如果在 `message_stop` 发送之后网关继续推流（如延迟的 SSE 数据、迟到的心跳或尾部元数据），将命中 `Ol.has(Ba)`，抛出 `StreamMalformedEventError("closed")`（`TJ("closed")`）。
- **灾难后果**：`malformed` **不受** `complete` 守卫豁免！客户端会直接将整轮会话定性为畸形，在控制台弹出黄色警告并可能触发非流式重试。
- **网关防线**：下发 `message_stop` 后必须立即使连接到达 HTTP EOF，且在 `process_sse_line` 入口前置 `if state.message_stop_sent { return None; }` 守卫。

---

### 2.11 传输层 SSE `event: error` 即刻抛错与协议中断机制
SDK 底层 SSE 解析器 `fromSSEResponse`（偏移 `190503780`）：
```javascript
if (T.event === "error") {
  let P = hf(T.data) ?? T.data, M = P?.error?.type;
  throw new xt(void 0, P, void 0, n.headers, M);
}
```
- **核心判定**：一旦客户端解析到 `event: error`，立即抛出 `xt` 异常中止生成器，后续任何事件均无法到达。
- **网关规范**：推流开始后（`has_content || has_thinking`），**绝对禁止下发 `event: error`**。若发生中断，必须合成合法的 `content_block_stop` -> 兜底文本 -> `message_delta(end_turn)` -> `message_stop` 序列。

---

### 2.12 HTTP 429 限流与 `Bur = 60000` (60s) 硬截断崩溃陷阱
客户端定义了严格的退避时间天花板常量：
```javascript
var Bur = 60000; // 非 Watchdog 模式单次退避容忍上限: 60 秒
```
```javascript
$o = LF(Vt + G, Oo, void 0, r.random, ...);
if (GV()) {
  $o = Math.min($o, NKe);
} else if ($o > Bur) {
  throw i("tengu_api_retry_after_too_long", {delayMs: $o, status: Jt.status}),
        new qc(Jt, h); // 抛出异常直接中止，拒绝重试！
}
```
**严重风险**：当未开启 `CLAUDE_CODE_RETRY_WATCHDOG` 时，若网关返回的 `Retry-After` 计算后大于 60 秒，客户端**直接硬崩溃抛错退出**。网关下发 429 时必须将 `Retry-After` 安全钳制在 `1 ~ 15` 秒内。

---

### 2.13 非流式降级分支 (`retryWithoutStreaming`) 与 `jAt(e)` 强契约校验
当流在首包阶段遭遇 404 或未出字前网络中断时，客户端会通过 `Qhe` 触发非流式降级：
```javascript
let Ye = await Y.beta.messages.create({ ...Ce, stream: !1 }, ...);
```
客户端对非流式响应执行 `jAt(e)` 强模式校验：
- `content` 必须为合法 Array；
- `model` 必须为非空 String；
- `usage` 必须为包含 `input_tokens` 与 `output_tokens` 的 Object。
缺损任何字段均会导致 `Non-streaming fallback also failed` 致命报错。网关必须保持对 `/v1/messages` 的 `stream: false` 完整支持。

---

### 2.14 SSE 保活心跳帧定界规范与双换行 `\n\n` 约束
客户端内置解析器 `class rjt` 与 SDK 流迭代器中：
- SSE 帧采用连续换行符定界（`\n\n` 或 `\r\n\r\n`）。单换行会被挂起在 `pending` 缓冲中，无法触发事件解析；
- 收到注释行 `: keepalive\n\n` 时，字段解析函数 `f(n)` 返回 `{}`，触发 `this.resetLivenessTimer()`，同时重置 `Cy` 计时器；
- 网关在模型长思考期间必须至少每 15 秒（推荐每 3 秒）刷出一次保活帧，严禁长流静默超 45 秒。

---

## 三、Antigravity 网关交互准则与绝对红线

基于上述 14 维度深度逆向成果，Antigravity 网关确立以下生命周期交互红线与合规规范：

### 1. 绝对红线 (Strictly Prohibited)
1. **严禁在推流开始后透传底层连接断开 Err (如 TCP RST / Broken pipe)**：
   一旦 `has_content == true` 或 `has_thinking == true`，任何网络断裂必须被网关拦截并转入本地优雅收尾。
2. **严禁在响应中合成或下发带有 `"type":"overloaded_error"` 的任何载荷**：
   无论首包或中途，该字串均会触发客户端 `PU(e)` 判为过载，引爆 30 分钟冷却与 10 阶指数退避风暴。
3. **严禁遗留悬挂未闭合的内容块（禁止 `El !== null`）**：
   发送 `message_delta` 或 `message_stop` 之前，必须显式发送 `content_block_stop` 闭合所有已开启块。
4. **严禁 Thinking-Only 孤立断流**：
   流若在仅有思考块时中断，必须补发兜底正文文本块升级进度至 `"output"`，严禁直接以思考块收尾导致会话中毒。
5. **严禁在 `message_stop` 下发后追加任何帧或维持心跳**：
   下发 `message_stop` 后必须立即使连接到达 HTTP EOF，防止触发 `TJ("closed")` 或客户端 30s Byte Watchdog 误杀。
6. **严禁下发超过 60 秒的 `Retry-After` 头部**：
   防止触发客户端 `Bur = 60000ms` 的 `api_request_retry_after_too_long` 硬崩溃。

### 2. 必须行为 (Mandatory Requirements)
1. **严格履行黄金三元组收尾契约**：
   断流收敛序列必须严格保证：
   $$\text{content\_block\_stop (若有打开块)} \longrightarrow \text{正文兜底 (若思考孤立)} \longrightarrow \text{message\_delta (end\_turn)} \longrightarrow \text{message\_stop} \longrightarrow \text{HTTP EOF}$$
2. **首包不可逆错误注入 `x-should-retry: false` 响应头**：
   首包前发生确定性失败时，在 HTTP 头中注入该标志，促使客户端立即终止重试并暴露错误。
3. **维持每 3 秒高频 SSE 保活心跳**：
   向下游推送 `: keepalive\n\n`，持续刷新客户端活跃定时器。

---

## 四、版本选型建议与用户自愈指引

### 1. 推荐客户端版本
- **稳定生产推荐**：**Claude Code v2.1.28x**（流完成校验相对宽松，鲁棒性最高）。
- **最新版本支持**：**Claude Code v2.1.293 ~ v2.1.295**。使用最新版本时，**必须配合部署包含 PR #3634 修复的 Antigravity 网关**。

### 2. 用户操作指南 (故障自愈)
- **中断当前重试**：若终端出现 `Retrying X/10` 倒计时，直接按 `Ctrl+C` 强行终止，切勿等待 27 分钟；
- **自愈中毒历史**：若旧会话出现持续报 400 错误，在终端执行 `/compact` 压缩上下文或执行 `/clear` 新开会话，即可激活 5 级自愈管道清除脏数据。

---

## 五、9 月历史回退的反思与现代融合架构

- **9 月版本回退的初衷与局限**：
  9 月版本的核心思想是“中途断流优雅收尾”。但在前期调试中误判了报错场景，曾一度尝试完全透传底层网络错误，导致直接撞上 2.1.293 严格引入的 `PU(e)` 与 `El` 契约，引爆了 27 分钟死锁与会话中毒。
- **PR #3634 的现代融合解法**：
  将 9 月“平稳收尾、严禁裸露断连”的稳态原则，与 2.1.293 的“三元组不变式契约（`El === null` + 正文兜底升级 output）”深度融合，彻底实现了全生命周期流控制闭环。
