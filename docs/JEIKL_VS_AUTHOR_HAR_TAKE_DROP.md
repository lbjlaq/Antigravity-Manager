# jeikl 桌面端 vs 作者 Mac IDE：差异取少报告

对照双方原生 Antigravity 报文，只抽取**有差异**的字段；一致的沿用，有差异的取**更少注入、更少改写、更少上下文**的一侧。

- jeikl：Windows 原生客户端，见 `docs/JEIKL_ANTIGRAVITY_CLIENT_CAPTURE.md`（样本 `docs/baogao.txt`、`docs/样本2.txt`、`docs/claude样本.txt`）
- 作者：macOS Antigravity IDE HAR，见 `docs/HAR_NATIVE_PROTOCOL_ALIGNMENT_AND_BUG_AUDIT.md`，已随 [PR #3541](https://github.com/lbjlaq/Antigravity-Manager/pull/3541) 合进 `beta`

「少」指少发给上游的字节和少做的网关发明。**签名不是可删字段**：思维链连续靠 `thoughtSignature`，思考正文可以少，签名不能少。

---

## 取舍原则（已拍板的三例）

| # | 差异 | 作者 Mac IDE | jeikl 桌面端 | 取少 | 理由 |
| --- | --- | --- | --- | --- | --- |
| 1 | 历史是否回传 `thought` 文本 | Gemini 历史 0 次回传；Claude 必须留 | Gemini / Claude 都带回 | **Gemini 不回传。Claude 原文保留。** 签名都在第一个非思考 part，不压成 `...`。 |
| 2 | `toolConfig` | agent 有 tools 就配 `VALIDATED` | 三份 agent 都没有该字段 | **jeikl（不发）** | 少一个对象。Windows agent 无此字段仍能跑工具。不要为 IDE 去发明。 |
| 3 | Claude / Gemini 回执角色 | Gemini FR → `model`；Claude FR → `user`；切模型时整段历史统一改写 | 完全一致 | **作者（与 jeikl 相同）** | 无差异，按目标模型改写。 |

---

## 其余差异与最终取舍

### A. 拓扑与工具声明

| 项 | 作者 | jeikl | 取舍 |
| --- | --- | --- | --- |
| 每个工具单独 `{functionDeclarations:[一个]}` | 329 次带工具全是切片 | 三份 14 项全是切片 | **一致，留切片**。网关把客户端工具切成一块一函数即可。 |
| `tools` 按 `name` 字典序重排 | 代码里 `sort_by` | 客户端原序：`view_file` → `run_command` → … | **jeikl（不排序）**。排序是多出来的改写，且和作者自己说的「工具乱序打前缀缓存」相反。稳定前缀靠**保持本次请求的原序**，不是每次改成字母序。 |
| 内层键序插入 `toolConfig` | `contents → SI → tools → toolConfig → labels → gc → sessionId` | `contents → SI → tools → labels → gc → sessionId` | **jeikl**。不插入不存在的键。 |
| 工具清单 17 vs 14 | IDE 多 `grep_search`、`list_dir`、`browser_subagent`、MCP 等 | 桌面端是 subagent / `send_message` 一套 | **透传客户端声明，不补 IDE 工具**。少注入。 |
| `functionResponse.response` 键名 | 100% `output` | 100% `output` | **一致，留 `output`**。`result` 方言迁到 `output`。 |
| 并行 FC 同块、并行 FR 下一块 | 有 | 有（Gemini FR 块为 model，Claude FR 块为 user） | **一致** |
| 串行 FC/FR 交替、不合并 | 有 | 有 | **一致**。只合并「被客户端拆散的并行 FC」，不要把串行并成一块。 |
| 连续 user 保持独立 | IDE 有；PR 已删相邻 user 合并 | 样本 3 开头两条 user（换模型）独立 | **一致，不合并**（少改写） |

### B. 思考与签名（和原则 1 配套）

| 项 | 作者 | jeikl | 取舍 |
| --- | --- | --- | --- |
| 历史 `thought` 文本 | Gemini 剥；Claude 必须留 | 两家都留 | **Gemini 不回传思考正文。Claude 原文保留，不压成 `...`。** 签名都在该轮第一个非思考 part。 |
| Gemini 签名落点 | 该轮第一个非思考 part（FC 或正文） | 相同 | **一致，留** |
| Claude 是否带 `thoughtSignature` | 审计写「绝对无」；代码对 Claude 剥签名 | 与 Gemini 同一落点：并行挂首个 FC，串行第一步挂正文 | **jeikl（留签名）**。这不是「多发正文」，是连续锚。作者这条和原则 1 矛盾，不能跟。 |
| 思考块本身带签名 | 不带 | 不带 | **一致，思考块不挂签** |
| 回执带签名 | 不带 | 不带 | **一致** |
| `includeThoughts` | agent 为 true | 同 | **一致** |
| Gemini high `thinkingBudget` | `-1` 或不设 | `-1` | **一致** |
| Claude `thinkingBudget` / `maxOutputTokens` | `1024` / `64000` | 同 | **一致，留作者口径** |
| flash low/medium 默认 1000 / 4000 | 来自 `fetchAvailableModels` | 无对应样本 | **留作者模型字典**。不是桌面 vs IDE 的 agent 拓扑差。 |

### C. labels 与信封

| 项 | 作者 | jeikl | 取舍 |
| --- | --- | --- | --- |
| 信封 `project` / `userAgent` JSON / `requestType` | `aicode-consumers` / `antigravity` / `agent` | 同 | **一致** |
| HTTP `User-Agent` | `antigravity/ide/... darwin; arm64` | Windows 桌面，不是这段 | **不要抄 IDE UA**。JSON 里继续 `userAgent: antigravity` 即可。 |
| `used_claude` / `used_non_gemini_model` | 随目标模型 | 同 | **一致，按目标模型填** |
| `used_claude_conservative` | 代码写死 `false`（注释「关掉」） | Gemini `false`；Claude **`true`** | **少改写：不要写死 false**。Gemini 填 false，Claude 填 true（跟 jeikl 桌面端）。作者这是产品开关，不是双方报文的最小公约。 |
| `last_execution_id` | 审计样例没有 | 三份都有，且排在 labels 最前 | **不发明**。没有真实 id 就不造；有则原样留下。不要为对齐 IDE 样例删掉已有字段。 |
| `model_enum` | 官方表占位符 | 有（Claude `M35`，Gemini `M298`/`M318`） | **有官方表就填，不编造** |
| `sessionId` | 有 | 三份同一个数值 id | **有则透传，网关不要改成随机 UUID 去「对齐」** |
| `safetySettings` 伪造 4 项 OFF | 官方默认不带 | 不带 | **一致，不发** |
| 显式 `cachedContent` | IDE 0 次；作者主张删注入 | 无 | **一致，不注入**（取少） |

### D. 作者有、jeikl 这批 agent 样本没有的

| 项 | 说明 | 取舍 |
| --- | --- | --- |
| `requestType=checkpoint` 配 `toolConfig.NONE` | 仅 IDE 审计；jeikl 无 checkpoint 包 | **agent 路径不发 toolConfig**（原则 2）。checkpoint 若仍要禁工具，再单独加 `NONE`，不要绑 `VALIDATED`。未抓到之前不在 agent 上预埋。 |
| `tab` / `chat` 无 tools | IDE 审计 | 与「无工具就不发 toolConfig」相容，**取少** |
| 调用 id：`call_...` vs `toolu_vrtx_01...` | IDE 示例偏 `call_`；jeikl Gemini 用 `call_`，Claude 用 `toolu_vrtx_` | **少改写：已有 id 透传**。不要把 Claude 的 `toolu_vrtx_` 强行改成 `call_`。缺 id 再补。 |
| `WaitMsBeforeAsync` 5000 vs 3000 | 客户端自己填 | **透传，网关不改** |

### E. 签名隔离（与报文拓扑无关）

| 项 | 取舍 |
| --- | --- |
| `initial_session_sig` 进循环前快照 | **留**。防跨轮串扰，不是多发字段。 |
| 尾轮会话签名放行 | **留**。少误杀。 |
| 测试 `TEST_CONFIG_LOCK` | **留**。与协议无关。 |

---

## 对 PR #3541 已合代码的含义

应**留下**：

- 工具切片（一块一函数）
- 按 `target_model` 把 FR 改成 Claude `user` / Gemini `model`
- 回执信封 `result` → `output`
- 连续 user 不合并
- 并行 FC/FR 打包
- Gemini 历史不回传思考正文；Claude 思考正文原文保留。签名都挂在该轮第一个非思考 part
- Claude `64000` / `1024`，Gemini high `-1`
- 签名快照隔离、测试锁

应**收回或不要做**（相对已合进 beta 的写法）：

- 有 tools 就注入 `toolConfig.VALIDATED`
- `tools` 按名字排序
- 为插入 `toolConfig` 改内层键序
- 对 Claude 剥 `thoughtSignature`
- 把 `used_claude_conservative` 写死为 `false`
- 把调用 id 一律改成 `call_...`

---

## 一句话

官方认的是**切片工具、按模型改回执角色、签名连续**；不认的是**多出来的 `toolConfig`、重排工具、思考正文、以及把 Claude 签名剥掉**。差异取少：少发思考块、不发 `toolConfig`、回执角色跟作者（也跟 jeikl）那套 Claude/Gemini 分工。
