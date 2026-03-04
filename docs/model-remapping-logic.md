# 模型重映射逻辑（当前实现）

最后更新：2026-03-02

本文描述当前代理中的模型重映射链路（含 Gemini 3/3.1 Pro 调整后的行为）。

## 1）整体流程

无论是 OpenAI 协议还是 Gemini 原生协议，请求模型都会经过两段处理：

1. 静态路由解析（全局规则）：
   - 在选账号前执行一次。
   - 代码：`src-tauri/src/proxy/common/model_mapping.rs` 的 `resolve_model_route`。
2. 动态账号感知改写（条件回退）：
   - 在选中账号后执行。
   - 代码：`src-tauri/src/proxy/token_manager.rs` 的 `resolve_dynamic_model_for_account`。

使用该流程的入口：
- `src-tauri/src/proxy/handlers/openai.rs`
- `src-tauri/src/proxy/handlers/gemini.rs`

## 2）静态路由优先级

`resolve_model_route(original_model, custom_mapping)` 的优先级从高到低为：

1. 官方动态淘汰转发规则：
   - `DYNAMIC_MODEL_FORWARDING_RULES`
2. 用户自定义精确映射：
   - `custom_mapping[original_model]`
3. 用户自定义通配符映射：
   - 按“非 `*` 字符数”比较，越具体优先级越高
4. 系统内置默认映射：
   - `map_claude_model_to_gemini`

都不命中时，模型名原样透传。

## 3）当前 Gemini Pro 内置映射策略

当前策略是：具体模型 ID 直接透传；只有泛别名会归一化。

具体 ID（不做跨版本强制改写）：
- `gemini-3-pro-high -> gemini-3-pro-high`
- `gemini-3-pro-low -> gemini-3-pro-low`
- `gemini-3-pro-preview -> gemini-3-pro-preview`
- `gemini-3.1-pro-high -> gemini-3.1-pro-high`
- `gemini-3.1-pro-low -> gemini-3.1-pro-low`
- `gemini-3.1-pro-preview -> gemini-3.1-pro-preview`

泛别名（仍映射到 preview 入口）：
- `gemini-3-pro -> gemini-3-pro-preview`
- `gemini-3.1-pro -> gemini-3.1-pro-preview`

代码位置：
- `src-tauri/src/proxy/common/model_mapping.rs`

## 4）动态账号感知改写（仅在需要时触发）

选中账号后，系统会读取该账号本地 quota 里的可用模型，判断当前模型是否可用。

行为如下：

1. 读取账号 JSON：`quota.models[*].name`。
2. 仅针对 Gemini 3/3.1 Pro 家族构造候选回退列表。
3. 候选顺序：
   - 先尝试当前模型
   - 再按预设顺序尝试同家族其他兼容模型
4. 选中第一个在账号可用集合里存在的模型。
5. 若都不命中，则保持当前模型不变。

关键点：
- 如果请求模型本身可用，不会发生重映射。
- 只有请求模型不可用且存在兼容候选时，才会重映射。

代码位置：
- `src-tauri/src/proxy/token_manager.rs`
  - `get_available_models_from_json`
  - `build_dynamic_model_candidates`
  - `resolve_dynamic_model_for_account`

## 5）日志观测点

可通过日志判断每一步是否触发：

- 静态映射日志：
  - `[Router] 系统默认映射: <original> -> <mapped>`
- 动态改写日志：
  - `[Dynamic-Model-Rewrite] account=<id> <from> -> <to>`

如果某次请求没有出现 `Dynamic-Model-Rewrite`，说明该账号直接使用了当前模型。

## 6）示例

示例 A（不改写）：
- 请求：`gemini-3-pro-high`
- 账号可用模型包含：`gemini-3-pro-high`
- 最终上游模型：`gemini-3-pro-high`

示例 B（发生回退改写）：
- 请求：`gemini-3-pro-high`
- 账号不可用：`gemini-3-pro-high`
- 账号可用：`gemini-3.1-pro-high`
- 最终上游模型：`gemini-3.1-pro-high`

示例 C（泛别名）：
- 请求：`gemini-3-pro`
- 静态阶段先映射为：`gemini-3-pro-preview`
- 动态阶段再根据账号可用模型决定是否继续回退。

## 7）设计目标

该设计同时满足三点：
- 具体模型优先保持用户原始意图。
- 泛别名保留历史兼容能力。
- 多账号能力不一致时，通过动态回退提升可用性。
