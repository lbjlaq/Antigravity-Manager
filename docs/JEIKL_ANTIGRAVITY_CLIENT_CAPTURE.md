# jeikl Windows Antigravity 客户端抓包

来源：@jeikl 本机 Windows 原生 Antigravity 客户端（不是 Antigravity IDE）。

- 样本 1：`docs/baogao.txt`，模型 `gemini-3.7-flash-high`
- 样本 2：`docs/样本2.txt`（原 `docs/报告2 并发和串行交接触发.txt`），模型 `gemini-3.8-flash-high`
- 样本 3：`docs/claude样本.txt`，模型 `claude-sonnet-4-6`
- 客户端标识：`userAgent = antigravity`，`requestType = agent`
- 应用数据目录：`C:\Users\Administrator\.gemini\antigravity`
- 对照：作者 [PR #3541](https://github.com/lbjlaq/Antigravity-Manager/pull/3541) 的 `docs/HAR_NATIVE_PROTOCOL_ALIGNMENT_AND_BUG_AUDIT.md` 依据的是 Antigravity IDE HAR
- 与作者差异的取少结论：`docs/JEIKL_VS_AUTHOR_HAR_TAKE_DROP.md`

三份报文的顶层键序、`systemInstruction` 骨架和 14 个工具声明相同。差别在目标模型家族：Gemini 的工具回执是 `role: "model"`，Claude 的工具回执是 `role: "user"`。并行/串行形态两种家族都有。

## 顶层字段

信封（`request` 之外）：

| 字段 | 样本 1 | 样本 2 | 样本 3 |
| --- | --- | --- | --- |
| `project` | `aicode-consumers` | 同左 | 同左 |
| `userAgent` | `antigravity` | 同左 | 同左 |
| `requestType` | `agent` | 同左 | 同左 |
| `model` | `gemini-3.7-flash-high` | `gemini-3.8-flash-high` | `claude-sonnet-4-6` |
| `requestId` | `agent/4cabd908-.../21` | `agent/0b7ddb01-.../13` | `agent/3c49dacf-.../22` |

`request` 键序三份都是：

1. `contents`
2. `systemInstruction`
3. `tools`
4. `labels`
5. `generationConfig`
6. `sessionId`

没有 `toolConfig`，也没有 `tool_config`。三份 `sessionId` 都是 `-3750763034362895579`。

`generationConfig`：

| | Gemini（样本 1、2） | Claude（样本 3） |
| --- | --- | --- |
| `maxOutputTokens` | `65536` | `64000` |
| `includeThoughts` | `true` | `true` |
| `thinkingBudget` | `-1` | `1024` |

`labels` 样本 1（Gemini）：

```json
{
  "last_execution_id": "fb7b9c90-03c2-4e6f-983e-369ef8fc9e5e",
  "last_step_index": "20",
  "model_enum": "MODEL_PLACEHOLDER_M298",
  "request_id": "231efd36-2a13-4fea-ab31-04cc500a7ec2-9",
  "trajectory_id": "231efd36-2a13-4fea-ab31-04cc500a7ec2",
  "used_claude": "false",
  "used_claude_conservative": "false",
  "used_non_gemini_model": "false"
}
```

样本 3（Claude）同一套键，值为：

```json
{
  "last_execution_id": "9648bd6f-4b24-495e-9b82-f5b59ecfa6cc",
  "last_step_index": "21",
  "model_enum": "MODEL_PLACEHOLDER_M35",
  "request_id": "a625a450-b0e2-435f-b99f-3c821f9149cc-8",
  "trajectory_id": "a625a450-b0e2-435f-b99f-3c821f9149cc",
  "used_claude": "true",
  "used_claude_conservative": "true",
  "used_non_gemini_model": "true"
}
```

原生 Claude 把 `used_claude_conservative` 设成 `true`。作者流水线写死成 `false`。

## systemInstruction

约 25KB 一段文本。开头身份和用户信息三份相同（Conversation ID 各会话不同）：

```xml
<identity>
You are Antigravity, a powerful agentic AI coding assistant designed by the Google Deepmind team working on Advanced Agentic Coding.
...
User requests are enclosed within <USER_REQUEST> tags.
</identity>
<user_information>
The USER's OS version is windows.
...
e:\code\Antigravity-Manager -> lbjlaq/Antigravity-Manager
App Data Directory: C:\Users\Administrator\.gemini\antigravity
</user_information>
```

后面按顺序还有：`user_rules`（嵌入本仓库 `AGENTS.md`）、`skills`、`subagents`、`messaging`、`conversation_transcript`、`artifacts`、`slash_commands`、`guidelines`、`communication_style`。`systemInstruction.role` 是 `user`。

用户轮次正文包在 `<USER_REQUEST>` 里，后面跟 `<ADDITIONAL_METADATA>`。换模型时跟 `<USER_SETTINGS_CHANGE>`。

## 工具声明

三份都是 14 项。每一项单独一个对象，`functionDeclarations` 里只有一个函数。顺序是客户端原序，不是按名字排序：

1. `view_file`
2. `run_command`
3. `manage_task`
4. `send_message`
5. `schedule`
6. `invoke_subagent`
7. `define_subagent`
8. `manage_subagents`
9. `write_to_file`
10. `replace_file_content`
11. `generate_image`
12. `read_url_content`
13. `search_web`
14. `ask_question`

实际用到的只有 `run_command`。参数有 `CommandLine`、`Cwd`、`WaitMsBeforeAsync`、`toolAction`、`toolSummary`。回执是 `response.output`。

Gemini 的调用 id 是 `call_...`；Claude 是 `toolu_vrtx_01...`。Gemini 样本 `WaitMsBeforeAsync` 是 `5000`，Claude 样本是 `3000`。

## 分类：按模型家族的回执角色

| | `functionCall` | `functionResponse` |
| --- | --- | --- |
| Gemini（样本 1、2） | `role: "model"` | `role: "model"` |
| Claude（样本 3） | `role: "model"` | `role: "user"` |

这点和作者 IDE HAR 一致。桌面端也是按目标模型改写整段历史，不会在同一次请求里混用两种回执角色。

## 分类：并行

多次调用收在同一个 `role: "model"` content 里。对应回执收在紧跟着的一块里：Gemini 那块仍是 `model`，Claude 那块是 `user`。

### 样本 2 开头（Gemini 并行三次）

用户：`请你稍作思考后并发调用三个echo helloworld函数`

同一块三个 `functionCall`（id `call_1180168` / `call_1180171` / `call_1180174`），下一块三个 `functionResponse`，stdout 都是 `helloworld`。

### 样本 1 后段（Gemini 并行三次）

用户：`请你并发调用三个工具hello`

同一块三个 `functionCall`：`Write-Output "hello 1/2/3"`。下一块三个回执。

### 样本 3 开头和末段（Claude 并行三次）

用户：`你好 请你稍作思考后并行调用三个工具试试 只需要输出helloworld就可以了`

contents[2] 一块四个 part：`thought` + 三个 `functionCall`（`echo` / `node -e` / `python -c`）。签名在**第一个** `functionCall` 上，思考块和后两个调用没有签名。

contents[3] 一块三个 `functionResponse`，`role: "user"`。

末段用户：`这次并行试试`。contents[14]/[15] 同样是三个 FC 在 model、三个 FR 在 user。

## 分类：串行

每一次调用和它的回执各占一块，按调用、回执交替，不把多次串行调用并成一块。

### 样本 1 中段（Gemini 串行）

用户：`请你串行调用三个helloworld输出工具`

| # | role | 形态 |
| --- | --- | --- |
| 7 | model | `Write-Output "helloworld 1"` |
| 8 | model | 回执 |
| 9 | model | `helloworld 2` |
| 10 | model | 回执 |
| 11 | model | `helloworld 3` |
| 12 | model | 回执 |

### 样本 2 后段（Gemini 串行）

用户：`请你再串行调用三个试试`。contents[5]–[10] 同样是 FC / FR 交替，全是 `model`。

### 样本 3 中段（Claude 串行）

用户：`请你再串行三个试试`

| # | role | 形态 |
| --- | --- | --- |
| 6 | model | thought + 正文「第一步」+ 一个 `functionCall`（签名在正文上） |
| 7 | user | 一个 `functionResponse` |
| 8 | model | 正文「第二步」+ 一个 `functionCall`（无签名） |
| 9 | user | 一个 `functionResponse` |
| 10 | model | 正文「第三步」+ 一个 `functionCall`（无签名） |
| 11 | user | 一个 `functionResponse` |

Claude 串行轮可以在同一块里同时有正文和一次调用；仍然一次只出一个 `functionCall`，回执单独一块 `user`。

## 分类：连续 user

样本 3 开头两条都是 `role: "user"`，没有插 model：

- contents[0]：同一句并行请求，切到 Claude Opus 4.6 Thinking
- contents[1]：同一句正文，再切到 Claude Sonnet 4.6 Thinking

两条独立 content，没有并成一块。作者删掉相邻 user 合并，这一点桌面端 Claude 样本支持。Gemini 两份没有连续 user。

## 思考块和签名

| | Gemini | Claude |
| --- | --- | --- |
| 历史里的 `thought` 文本 | 样本 1 第一轮有；样本 2 无 | 三次工具轮都保留 thought 文本 |
| `thoughtSignature` | 挂在该轮第一个非思考 part（正文或首个 FC） | **同样挂在该轮第一个非思考 part** |
| 思考块本身 | 不带签名 | 不带签名 |
| 回执 | 不带签名 | 不带签名 |

作者审计写 Claude「部件上绝对无 thoughtSignature」。样本 3 不是这样：并行时签名在首个 `functionCall` 上，串行第一步签名在正文上。流水线里对 Claude 轮会把非思考 part 上的签名剥掉，和这份桌面端报文不一致。

## 样本 1 完整对话

共 19 个 content。`Cwd` 都是 `e:\code\Antigravity-Manager`，`WaitMsBeforeAsync` 都是 `5000`。

| # | role | 内容 |
| --- | --- | --- |
| 0 | user | `你好 请你深度思考一下 回答我`（切到 Gemini 3.8 Flash High） |
| 1 | model | thought + 正文：`你好！我已经准备好了...` |
| 2 | user | `好的  请你直接回我  不要思考`（切到 Gemini 3.7 Flash High） |
| 3 | model | `没问题！请问有什么我可以帮您的？` |
| 4 | user | `请你再不要思考回复我一次` |
| 5 | model | `好的，收到！随时听候您的吩咐。` |
| 6 | user | `请你串行调用三个helloworld输出工具` |
| 7–12 | model | **串行**三次 `run_command` |
| 13 | model | 串行完成说明 |
| 14 | user | `请你并发调用三个工具hello` |
| 15–16 | model | **并行**三次 `run_command` |
| 17 | model | 并行完成说明 |
| 18 | user | `好的 请你不思考回我` |

## 样本 2 完整对话

共 11 个 content。同一条请求里先并行、再串行。

| # | role | 内容 |
| --- | --- | --- |
| 0 | user | `请你稍作思考后并发调用三个echo helloworld函数` |
| 1–2 | model | **并行**三次 `echo helloworld` |
| 3 | model | 并行完成说明 |
| 4 | user | `请你再串行调用三个试试` |
| 5–10 | model | **串行**三次 `echo helloworld` |

## 样本 3 完整对话

共 16 个 content。`WaitMsBeforeAsync` 都是 `3000`。

| # | role | 内容 |
| --- | --- | --- |
| 0 | user | 并行 helloworld（切到 Claude Opus 4.6 Thinking） |
| 1 | user | 同一句（再切到 Claude Sonnet 4.6 Thinking） |
| 2 | model | thought + **并行**三个 `run_command` |
| 3 | user | 三个 `functionResponse` |
| 4 | model | 并行完成说明（表格） |
| 5 | user | `请你再串行三个试试` |
| 6–11 | model/user 交替 | **串行**三次 |
| 12 | model | 串行完成说明 |
| 13 | user | `这次并行试试` |
| 14 | model | thought + **并行**三个 `run_command` |
| 15 | user | 三个 `functionResponse` |

## 和 IDE 报告 / PR #3541 对照

三份 Windows 桌面端报文都支持「一个函数一块」的 `functionDeclarations`。Gemini 回执 `model`、Claude 回执 `user`、Claude `maxOutputTokens: 64000` + `thinkingBudget: 1024`、连续 user 保持独立，这些和作者 HAR 一致。

它们都不支持下面这些已经合进 `beta` 的写法：

- 有 tools 时强制补 `toolConfig.functionCallingConfig.mode = VALIDATED`。三份都没有 `toolConfig`。
- 把 `tools` 按 `name` 字典序重排。三份都是上面的原序。
- 把 Claude 的 `used_claude_conservative` 写死成 `false`。样本 3 是 `true`。
- 断言 Claude 历史「绝对无 thoughtSignature」，并在流水线里剥掉。样本 3 把签名挂在该轮第一个非思考 part 上，和 Gemini 同一套落点。
