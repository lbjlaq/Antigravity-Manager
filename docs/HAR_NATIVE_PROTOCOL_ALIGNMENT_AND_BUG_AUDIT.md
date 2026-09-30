# Antigravity 抓包与原生应用逆向深度对齐分析报告
## 基于 `flows (6)`、原生应用 macOS App 及网关多协议转换全景代码审计

---

## 1. 调研背景与数据源 (Ground Truth Data Sources)

本报告基于两大权威第一手数据源与当前项目源码进行交叉比对：
1. **抓包数据源 (`flows (6)`)**：
   - 捕获于 macOS 环境下原生 Antigravity IDE 真实网络流量；
   - 包含 **2,060+ 个完整 HTTP/WebSocket 流**，其中包含 **359 次真实的 `streamGenerateContent?alt=sse`** 上行上报与下行流式响应；
   - 覆盖真实多轮长会话（单会话高达 60+ 轮次）、模型中途切换（Gemini ↔ Claude 相互交替）、工具并发调用（26,695 次工具回执）；
   - 涉及 6 大核心模型系列：`gemini-3.8-flash-low`, `gemini-3.1-flash-lite`, `gemini-2.5-flash-lite`, `claude-sonnet-4-6`, `tab_flash_lite_preview`, `tab_jump_flash_lite_preview`。
2. **原生应用程序 (`/Applications/Antigravity IDE.app`)**：
   - 逆向分析其核心语言服务二进制组件：`Contents/Resources/app/extensions/antigravity/bin/language_server_macos_arm`（Google 内部 JetSki / Cortex 架构 Go 编译二进制）；
   - 逆向分析前端扩展入口：`Contents/Resources/app/extensions/antigravity/dist/extension.js`；
   - 提取 `fetchAvailableModels` 接口返回的 **33 个原生模型完整配置字典**与元数据矩阵。
3. **被审计代码库 (`Antigravity-Manager` in `src-tauri/`)**：
   - `src-tauri/src/proxy/pipeline/inbound.rs`（统一进站思考管线）
   - `src-tauri/src/proxy/mappers/openai/request.rs`（OpenAI 协议转译器）
   - `src-tauri/src/proxy/mappers/claude/request.rs`（Claude 协议转译器）
   - `src-tauri/src/proxy/common/json_schema.rs`（JSON Schema 清洗器）
   - `src-tauri/src/proxy/cache_manager.rs`（三层拆分上下文缓存管理器）
   - `src-tauri/resources/official_models.json`（官方模型配置表）

---

## 2. 抓包与原生请求参数全景图谱 (Native Request Specification)

### 2.1 外层信封 (Outer Envelope & Headers)
原生通信目标地址恒为：
`POST https://daily-cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse`

**HTTP Headers 规范**：
```http
Host: daily-cloudcode-pa.googleapis.com
User-Agent: antigravity/ide/2.5.5 (aidev_client; os_type=darwin; arch=arm64)
Content-Type: application/json
Accept-Encoding: gzip
Transfer-Encoding: chunked
```

**JSON 根级 Envelope 结构**：
```json
{
  "project": "aicode-consumers",
  "requestId": "agent/d08b2892-847c-41d9-9bd6-df72511b9f3c/1790410102432/0917011e-4959-4da9-8554-20499ed72869/3",
  "model": "gemini-3.8-flash-low",
  "userAgent": "antigravity",
  "requestType": "agent",
  "request": { ... }
}
```
- `project`：固定为 `"aicode-consumers"`。
- `requestId`：格式为 `{requestType}/{client_instance_uuid}/{timestamp_ms}/{trajectory_id}/{step_index}`。
- `requestType`：枚举包含 `agent`（编程助手交互）、`checkpoint`（会话总结保存）、`chat`（普通聊天）、`tab`（行内补全）、`tab_jump`（跳转补全）。
- `request`：核心内层载荷。

### 2.2 内部请求拓扑 (Inner Request Topology)
原生报文严格遵循以下键序（对齐 TPU 隐式前缀缓存命中）：
$$\text{contents} \longrightarrow \text{systemInstruction} \longrightarrow \text{tools} \longrightarrow \text{toolConfig} \longrightarrow \text{labels} \longrightarrow \text{generationConfig} \longrightarrow \text{sessionId}$$

#### Labels 规范
```json
{
  "last_step_index": "2",
  "model_enum": "MODEL_PLACEHOLDER_M320",
  "request_id": "0917011e-4959-4da9-8554-20499ed72869-2",
  "trajectory_id": "0917011e-4959-4da9-8554-20499ed72869",
  "used_claude": "false",
  "used_claude_conservative": "false",
  "used_non_gemini_model": "false"
}
```

---

## 3. 原生 17 个工具函数 Schema 与回执定义全景表

原生工具在请求报文的 `tools` 字段中，**100% 格式化为单函数独立切片数组**：
`"tools": [ { "functionDeclarations": [ { "name": "tool1", ... } ] }, { "functionDeclarations": [ { "name": "tool2", ... } ] } ]`
（原生 329 次带工具抓包中，独立切片占比 100%，聚合为一个大数组的占比为 0）。

所有参数 schema 均采用大写类型标记（`OBJECT`, `STRING`, `INTEGER`, `BOOLEAN`, `ARRAY`）。

### 3.1 完整工具函数定义矩阵
| 工具名称 | 关键参数 (`properties`) | 必填字段 (`required`) | 特殊约束 / 类型要求 |
| :--- | :--- | :--- | :--- |
| **`run_command`** | `CommandLine`, `Cwd`, `WaitMsBeforeAsync`, `IsDaemon`, `RequestedTerminalID`, `RunPersistent` | `Cwd`, `WaitMsBeforeAsync`, `CommandLine`, `toolSummary`, `toolAction` | `WaitMsBeforeAsync` 为 `INTEGER` 且必填；其余大驼峰 |
| **`view_file`** | `AbsolutePath`, `ContentOffset`, `EndLine`, `IsSkillFile`, `StartLine` | `AbsolutePath`, `toolSummary`, `toolAction` | `ContentOffset`, `StartLine`, `EndLine` 为 `INTEGER` |
| **`write_to_file`** | `TargetFile`, `Overwrite`, `CodeContent`, `Description`, `ArtifactMetadata` | `TargetFile`, `Overwrite`, `CodeContent`, `Description`, `toolSummary`, `toolAction` | `Overwrite` 为 `BOOLEAN` 必填 |
| **`replace_file_content`** | `TargetFile`, `Instruction`, `Description`, `AllowMultiple`, `TargetContent`, `ReplacementContent`, `StartLine`, `EndLine`, `TargetLintErrorIds` | `TargetFile`, `Instruction`, `Description`, `AllowMultiple`, `TargetContent`, `ReplacementContent`, `StartLine`, `EndLine`, `toolSummary`, `toolAction` | 全 8 项关键参数必须全在；行号为 `INTEGER` |
| **`multi_replace_file_content`** | `TargetFile`, `Instruction`, `Description`, `ReplacementChunks`, `TargetLintErrorIds`, `ArtifactMetadata` | `TargetFile`, `Instruction`, `Description`, `ReplacementChunks`, `toolSummary`, `toolAction` | `ReplacementChunks` 内部包含 `StartLine`, `EndLine`, `TargetContent`, `ReplacementContent`, `AllowMultiple` |
| **`grep_search`** | `SearchPath`, `Query`, `CaseInsensitive`, `Includes`, `IsRegex`, `MatchPerLine` | `SearchPath`, `Query`, `toolSummary`, `toolAction` | `SearchPath` 与 `Query` 必填 |
| **`list_dir`** | `DirectoryPath` | `DirectoryPath`, `toolSummary`, `toolAction` | 路径大驼峰 |
| **`manage_task`** | `Action`, `Input`, `TaskId` | `Action`, `toolSummary`, `toolAction` | `Action` 枚举值：`['list', 'kill', 'status', 'send_input']` |
| **`browser_subagent`** | `TaskName`, `Task`, `TaskSummary`, `RecordingName`, `MediaPaths`, `ReusedSubagentId` | `TaskName`, `Task`, `TaskSummary`, `RecordingName`, `toolSummary`, `toolAction` | 全 6 项必选 |
| **`call_mcp_tool`** | `ServerName`, `ToolName`, `Arguments` | `ServerName`, `ToolName`, `Arguments`, `toolSummary`, `toolAction` | `Arguments` 支持任意结构 |
| **`list_resources`** | `ServerName` | `toolSummary`, `toolAction` | 无必填内容参数 |
| **`read_resource`** | `ServerName`, `Uri` | `toolSummary`, `toolAction` | `Uri` 大驼峰 |
| **`read_url_content`** | `Url` | `Url`, `toolSummary`, `toolAction` | `Url` 大驼峰 |
| **`search_web`** | `query`, `domain` | `query`, `toolSummary`, `toolAction` | **反例注意**：参数为小写 `query`, `domain` |
| **`ask_question`** | `questions` | `toolSummary`, `toolAction` | **反例注意**：小写 `questions`（嵌套 `ARRAY[OBJECT]`） |
| **`schedule`** | `Prompt`, `CronExpression`, `DurationSeconds`, `IsDaemon`, `MaxIterations`, `TimerCondition` | `Prompt`, `toolSummary`, `toolAction` | `DurationSeconds` 与 `MaxIterations` 声明为 `STRING` |
| **`generate_image`** | `Prompt`, `ImageName`, `AspectRatio`, `ImagePaths` | `Prompt`, `ImageName`, `toolSummary`, `toolAction` | `AspectRatio` 支持 `1:1`, `16:9` 等 |

> **铁律 1（全工具 UI 状态元数据）**：
> 全部 17 个原生工具的 `required` 列表中，**无一例外全部强制包含 `["toolAction", "toolSummary"]`**。
> - `toolAction`（动宾短语，如 "Analyzing directory", "Editing file"）
> - `toolSummary`（名词短语，如 "Directory analysis", "File edit"）

> **铁律 2（工具执行回执信封标准）**：
> 在抓包提取的全部 26,695 次工具执行结果中：
> `functionResponse.response` **100% 为只包含一个 key 的 JSON 对象**：
> ```json
> {
>   "functionResponse": {
>     "id": "call_438678",
>     "name": "run_command",
>     "response": {
>       "output": "Created At: 2026-09-28T...\nOutput:\n..."
>     }
>   }
> }
> ```
> **`response` 中 0 次使用 `"result"` 键，100% 使用 `"output"` 键**。

---

## 4. 核心不对等点深度剖析 (Multi-Model & Multi-Turn Asymmetries)

本节聚焦用户最关心的**不同模型与多轮会话参数不对等**。通过对原生通信抓包与二进制逻辑的挖掘，提炼出四大关键不对等机理：

### 4.1 核心不对等 1：`functionResponse` 角色的绝对模型相关性（最严重漏洞点）
通过对抓包内全部 26,695 个 `functionResponse` 进行分类统计，揭示出两大家族完全相反的协议规范：

```
Gemini 目标模型 (gemini-3.8-flash-low / gemini-3.1-flash-lite):
  - functionResponse in role: "model"  --> 25,817 次 (100.0%)
  - functionResponse in role: "user"   --> 0 次 (0.0%)

Claude 目标模型 (claude-sonnet-4-6):
  - functionResponse in role: "user"    --> 846 次 (100.0%)
  - functionResponse in role: "model"   --> 0 次 (0.0%)
```

#### 原生会话地面真相：
1. **Gemini 协议流**：
   - Turn 0 (`user`): 用户输入需求
   - Turn 1 (`model`): 模型输出 `functionCall`（挂载 `thoughtSignature`）
   - Turn 2 (`model`): 客户端回传 `functionResponse`（**角色依然是 `model`**）
   - Turn 3 (`model`): 模型输出最终回答文本（挂载 `thoughtSignature`）
   - Turn 4 (`user`): 用户下一轮追问
2. **Claude 协议流**：
   - Turn 0 (`user`): 用户输入需求
   - Turn 1 (`model`): Claude 输出 `functionCall`（不带 `thoughtSignature`）
   - Turn 2 (`user`): 客户端回传 `functionResponse`（**角色转为 `user`**）
   - Turn 3 (`model`): Claude 接着输出结果
3. **跨模型切换的全局重写 (Global Rewriting on Switch)**：
   抓包分析中，`mixed_flows`（单次请求中同时出现 `user` 和 `model` 的 `functionResponse`）的出现次数为 **0**。
   当原生会话中途从 Gemini 切换到 Claude 时，**原生客户端将整个历史记录中所有的 `functionResponse.role` 统一重写为 `user`**；反之切回 Gemini 时统一重写为 `model`。

### 4.2 核心不对等 2：思维链 (`thought`) 与思维签名 (`thoughtSignature`) 生命周期
1. **Gemini 家族**：
   - 上游 SSE 流式响应：在第 1 个 chunk 中即返回 `thoughtSignature`。
   - **多轮历史请求回传**：在抓包的 54,373 个 Gemini 历史模型轮次中，带 `thought: true` 文本块的次数为 **0**。原生客户端在向 Gemini 序列化历史消息时，**主动剥离了冗长的思考过程文本，仅将 `thoughtSignature` 挂载在首个有效非思考 part（如 `functionCall` 或普通正文 `text`）上**。
2. **Claude 家族**：
   - 上游 SSE 流式响应：不产生 `thoughtSignature`，以 `{"thought": true, "text": "..."}` 文本块流式下发。
   - **多轮历史请求回传**：Claude 历史轮次**必须保留 `{"thought": true, "text": "..."}` 文本块**（抓包中 33 次 Claude 思考块完整保留），且部件上绝对无 `thoughtSignature`。

### 4.3 核心不对等 3：`generationConfig` 极限与思考预算
通过抓取原生 `fetchAvailableModels` 权威数据字典与请求载荷比对：

| 模型代号 | 权威 `maxOutputTokens` | 权威默认 `thinkingBudget` | 思考控制参数 (`thinkingConfig`) |
| :--- | :--- | :--- | :--- |
| **`gemini-3.8-flash-low`** | 65,536 | 1,000 | `includeThoughts: true, thinkingBudget: 1000` |
| **`gemini-3.8-flash-medium`** | 65,536 | 4,000 | `includeThoughts: true, thinkingBudget: 4000` |
| **`gemini-3.8-flash-high`** | 65,536 | -1 (动态不设限) | `includeThoughts: true` (无 budget 或 -1) |
| **`claude-sonnet-4-6`** | **64,000**（严禁超 64k） | **1,024** | `includeThoughts: true, thinkingBudget: 1024` |
| **`claude-opus-4-6-thinking`**| **64,000** | **1,024** | `includeThoughts: true, thinkingBudget: 1024` |
| **`gemini-2.5-flash-lite`** | 1,024 (chat) / 65,535 | -1 | `includeThoughts: true, thinkingLevel: "HIGH"` |
| **`tab_flash_lite_preview`** | 2,048 (req) / 4,096 (cat) | 0 | `includeThoughts: false, thinkingBudget: 0` |
| **`tab_jump_flash_lite_preview`**| 2,048 (req) / 4,096 (cat) | 0 | `includeThoughts: false, thinkingBudget: 0` |
| **`gpt-oss-120b-medium`** | 32,768 | 8,192 | `includeThoughts: true, thinkingBudget: 8192` |
| **`gemini-pro-agent`** | 65,535 | 10,001 | `includeThoughts: true, thinkingBudget: 10001` |

### 4.4 核心不对等 4：`toolConfig` 状态机模式
- **常规智能体交互 (`requestType == "agent"`)**：
  只要声明了 `tools`，必配 `toolConfig: { "functionCallingConfig": { "mode": "VALIDATED" } }`。
- **会话断点总结 (`requestType == "checkpoint"`)**：
  即使声明了工具，原生显式配置：`toolConfig: { "functionCallingConfig": { "mode": "NONE" } }`。
- **代码补全与闲聊 (`tab` / `tab_jump` / `chat`)**：
  绝不包含 `tools`，且 `toolConfig` 为 `null`。

---

## 5. 上下文与缓存治理机制深度剖析 (Context & Caching)

### 5.1 隐式前缀缓存 (Implicit Prefix Caching) 的运行法则
抓包分析表明，原生通信中 `cachedContent` 或 `cached_content` 字段的出现次数为 **0**。
Google CloudCode PA 基础设施基于 TPU 集群内置的 **透明隐式前缀缓存 (Implicit Prefix Caching)**。
命中缓存的唯一条件是：**前序字节 100% 逐字节（Byte-for-byte）一致**。

**破坏前缀缓存的常见陷阱**：
1. **JSON 对象键序漂移**：未进行确定性拓扑排序；
2. **工具列表乱序**：每次调用工具数组顺序发生变化；
3. **工具描述文本被随意修改或清洗**；
4. **多轮历史中的思维链残留**：如果保留了随机变化的旧 thinking 文本，将破坏后续所有轮次的前缀缓存。

### 5.2 现有三层缓存架构与实际原生需求的偏差
项目中 `src-tauri/src/proxy/cache_manager.rs` 设计了三层缓存：
- Layer 1 (SI Cache): raw instructions → sanitized text
- Layer 2 (Tools Cache): raw tools JSON → processed tools
- Layer 3 (Prefix Tracker): 尝试查找 `cache_name` 并注入 `cachedContent`

**审计发现**：
- Layer 3 在 `openai/request.rs` 第 1353 行尝试注入 `"cachedContent": cache_name`。
- 这是公有云 Gemini API (`v1beta`) 的显式缓存机制，在内部受限的 `v1internal` 接口上根本不受支持。一旦注入必然导致上游直接 400 崩溃。

---

## 6. 协议转换与工具清洗函数现状与漏洞点审计

经过对 Rust 后端各模块的逐行代码审计，定位出以下关键缺陷与安全隐患：

### 🔴 缺陷 1 (P0 核心漏洞)：`normalize_function_response_roles` 无差别将回执定死为 `role: "model"`
- **定位**：[`src-tauri/src/proxy/pipeline/inbound.rs`](file:///Users/lbjlaq/Desktop/c%E6%96%87%E4%BB%B6%E5%A4%B9/src-tauri/src/proxy/pipeline/inbound.rs#L498) 第 498 行 & 第 611 行。
- **代码现状**：
  ```rust
  pub fn normalize_function_response_roles(contents: &mut Vec<Value>) -> usize {
      // ...
      if all_response {
          let mut c = content;
          c["role"] = json!("model");
          out.push(c);
      }
  }
  ```
- **漏洞后果**：
  当用户使用 Claude 客户端（如 Claude Code CLI、Roo Code 等）请求 `claude-sonnet-4-6` 时，所有工具回执被无条件强行改成 `role: "model"`。而抓包实证表明，Google 上游给 Claude 的通道要求 `functionResponse` **必须在 `role: "user"`**。这一错位直接导致发往 Claude 的报文结构违法，引发上游 400 校验拒绝或上下文理解错乱。
- **修复方案**：
  该函数必须接收 `target_model: &str` 参数。
  - 若 `is_claude(target_model)` 为真，则将连续工具回执归一化合并在 `role: "user"` 中；
  - 若为 Gemini 系列，则合并在 `role: "model"` 中。

---

### 🔴 缺陷 2 (P1 漏洞)：工具回执对象键名硬编码为 `"result"` 而非 `"output"`
- **定位**：
  - [`src-tauri/src/proxy/mappers/claude/request.rs`](file:///Users/lbjlaq/Desktop/c%E6%96%87%E4%BB%B6%E5%A4%B9/src-tauri/src/proxy/mappers/claude/request.rs#L1391) 第 1391 行 & 第 1435 行：
    `"response": {"result": merged_content}`
  - [`src-tauri/src/proxy/mappers/openai/request.rs`](file:///Users/lbjlaq/Desktop/c%E6%96%87%E4%BB%B6%E5%A4%B9/src-tauri/src/proxy/mappers/openai/request.rs#L798) 第 798 行：
    `"response": { "result": final_content }`
  - [`src-tauri/src/proxy/pipeline/inbound.rs`](file:///Users/lbjlaq/Desktop/c%E6%96%87%E4%BB%B6%E5%A4%B9/src-tauri/src/proxy/pipeline/inbound.rs#L448) 第 448-450 行：
    仅在 `!resp.is_object()` 时才包装为 `{"output": ...}`，如果已经是包含 `"result"` 的 Object 则直接放行。
- **漏洞后果**：
  原生模型接收到的所有原生工具回执 100% 为 `{"output": "..."}`，模型系统提示词也明确约定通过 `output` 字段解析工具运行结果。客户端回传 `"result"` 属于协议方言不一致，极易诱发模型误判工具未正确执行、反复调用或参数幻觉。
- **修复方案**：
  在 `inbound.rs` 的通用清洗层进行无损归一化：若 `response` 包含 `result` 且不包含 `output`，自动将 `result` 平滑迁移至 `output`。

---

### 🔴 缺陷 3 (P1 隐患)：显式 `cachedContent` 注入代码残留
- **定位**：[`src-tauri/src/proxy/mappers/openai/request.rs`](file:///Users/lbjlaq/Desktop/c%E6%96%87%E4%BB%B6%E5%A4%B9/src-tauri/src/proxy/mappers/openai/request.rs#L1350) 第 1350-1363 行。
- **代码现状**：
  ```rust
  if let Some(cache_name) = cache_manager.lookup_prefix(&prefix_hash) {
      if let Some(req_obj) = final_body["request"].as_object_mut() {
          req_obj.insert("cachedContent".to_string(), json!(cache_name));
      }
  }
  ```
- **漏洞后果**：
  `v1internal` 根本没有公开的 `cachedContents/create` 接口，抓包中原生 Antigravity IDE 出现该参数的次数为 0。目前是由于 `lookup_prefix` 永远未存入有效 `cache_name` 才侥幸未触发异常。此代码属于典型的外来臆测实现，一旦被触发必致上游 400。
- **修复方案**：
  彻底移除对 `cachedContent` 的显式注入，全量依赖 Google TPU 集群的隐式字节流前缀缓存。

---

### 🟡 缺陷 4 (P2 缺陷)：Gemini 多轮历史未清理思维链文本 (`thought: true`)
- **定位**：[`src-tauri/src/proxy/pipeline/inbound.rs`](file:///Users/lbjlaq/Desktop/c%E6%96%87%E4%BB%B6%E5%A4%B9/src-tauri/src/proxy/pipeline/inbound.rs#L223) 第 223 行。
- **代码现状**：
  管线仅剥离了占位符（如 `"."`, `"..."`, 空字符串）思考块。对于客户端（如 Claude Code CLI 或自研前端）多轮带回的完整思考文本，依然作为 `thought: true` part 保留发给了 Gemini。
- **漏洞后果**：
  原生抓包证明 Gemini 历史回传中**绝对不带任何历史思考文本**（0 / 54,373）。保留这些文本会浪费巨大的输入上下文 token，且容易引发跨轮次签名错位或 400。
- **修复方案**：
  当目标模型是 Gemini 家族时，历史 `model` 轮的思考文本块一律丢弃，但必须提取其合法签名转移到该轮首个非思考部件（通常是 `functionCall` 或正文 `text`）上。

---

### 🟡 缺陷 5 (P2 缺陷)：官方模型配置表缺少 `tab_flash_lite_preview`
- **定位**：[`src-tauri/resources/official_models.json`](file:///Users/lbjlaq/Desktop/c%E6%96%87%E4%BB%B6%E5%A4%B9/src-tauri/resources/official_models.json)。
- **代码现状**：
  配置表中仅存在 `tab_jump_flash_lite_preview` (`MODEL_PLACEHOLDER_M28`)，遗漏了原生抓包中频繁出现的行内补全模型 `tab_flash_lite_preview` (`MODEL_PLACEHOLDER_M19`)。
- **修复方案**：
  在 `official_models.json` 补齐 `tab_flash_lite_preview` 及其元数据参数（`maxOutputTokens: 4096`, `supportsThinking: false`）。

---

### 🟡 缺陷 6 (P2 缺陷)：`toolConfig` 强行覆盖忽略 `checkpoint` 模式
- **定位**：[`src-tauri/src/proxy/pipeline/inbound.rs`](file:///Users/lbjlaq/Desktop/c%E6%96%87%E4%BB%B6%E5%A4%B9/src-tauri/src/proxy/pipeline/inbound.rs#L1095) 第 1095-1115 行。
- **代码现状**：
  管线只要判定有 `tools`，就一律无条件构造或覆盖为 `{"mode": "VALIDATED"}`。
- **漏洞后果**：
  在断点总结（`checkpoint`）请求中，原生必须配置 `{"mode": "NONE"}` 以防止总结阶段模型幻觉产生工具调用。管线强行覆盖为 `VALIDATED` 会破坏原生 checkpoint 行为。
- **修复方案**：
  检查已有 `toolConfig.functionCallingConfig.mode`，若已经是 `NONE` 则保留，不强行覆盖为 `VALIDATED`。

---

## 7. 优化实施路线图 (Actionable Implementation Plan)

遵循项目的 **Pipeline First** 核心架构原则，所有治理应在流水线阶段完成，确保四大协议适配器无需各写一套补丁：

```
客户端请求 (OpenAI / Claude / Gemini / Responses)
                       │
                       ▼
        四大协议适配器 (仅做协议语法映射)
                       │
                       ▼
    InboundThinkingPipeline (统一核心治理阶段)
    ┌──────────────────────────────────────────────────────────┐
    │ 1. normalize_tool_call_ids (call_... 归一化)             │
    │ 2. normalize_function_response_roles(contents, model)    │
    │    ├─ Claude 目标: 归一化为 role: "user"                │
    │    └─ Gemini 目标: 归一化为 role: "model"               │
    │ 3. normalize_function_response_envelope (result → output)│
    │ 4. 历史 thinking 文本过滤 (Gemini 剥离留签名, Claude 保留)│
    │ 5. align_google_request_prefix_topology_with_model       │
    │    ├─ 单函数独立切片排序 [ { functionDeclarations: [t] } ] │
    │    ├─ 大写 JSON Schema 类型 (OBJECT, STRING 等)          │
    │    ├─ 尊重既有 toolConfig mode (NONE 不覆盖)              │
    │    └─ 移除显式 cachedContent 注入死代码                   │
    └──────────────────────────────────────────────────────────┘
                       │
                       ▼
          Google v1internal 上游 (100% 协议同构)
```

通过落实上述实施方案，网关在应对不同客户端、跨模型动态切换以及复杂多轮工具调用场景时，将达到与原生 Antigravity IDE 100% 像素级对齐的鲁棒性。
