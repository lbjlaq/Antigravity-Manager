# 上游超时与重试策略（Upload / Endpoint / Retry）

本文是上传阶段超时、端点选择、终态处置的唯一策略说明。适配器只执行
`pipeline::terminal` / `pipeline::rotation` 的裁决，不再各自维护超时常量。

## 1. 失败分类

| 类别 | 典型信号 | 是否账号故障 | 默认处置 |
|---|---|---|---|
| 账号级 | 401 / 403 / 429 / 配额耗尽 / 模型权限 | 是 | 换号、冷却、按额度恢复 |
| 传输级 | 连接复位 / GOAWAY / `SendRequest` / 上传停滞 | 否 | 同账号重连一次；不跨账号重放 |
| 模型/UA 级 | 404 `Requested entity was not found` + 非 Hub UA | 否 | 直接返回诊断；不写 900s 模型熔断 |
| 客户端级 | 客户端看门狗 / 主动断开 | 否 | 记录 499；不再重试 |
| 网关级 | 全局 270s 预算耗尽 | 否 | 结构化 504，停止轮换 |

## 2. 上传看门狗（P0）

- **进展驱动**：上传阶段每 250ms 检查 `sent` 是否增加；
- **只有 `stall_secs` 连续无进展**才终止，返回 `UpstreamTerminal::UploadStalled`；
- `upload_budget` 降级为**软预算/观测阈值**：超过时记录
  `upload_budget_extended`，不再按固定 5~18s 硬切断；
- 真正的硬上限由全局请求预算（默认 270s）兜底；
- 记录字段：`sent/body_len`、`progress_pct`、`effective_bps`、
  `budget_source`、`upload_budget`、`stall_timeout`、`idle`。

## 3. 终态与轮换（P0）

- `Upload` / `UploadStalled`：`stop_rotation=true`、`abandon_sticky_account=false`；
  - 不跨账号重放同一份 body；
  - 不解绑粘性绑定（上传失败通常是本地/网络问题，不是账号问题）；
  - 当前请求返回 504，客户端重试仍可命中同一绑定账号。
- `FirstByte`：保留原有“预算结余时换号”语义；`abandon_sticky_account=true`。
- `GlobalBudget` / `ReplayCap`：停止轮换；`GlobalBudget` 解绑，`ReplayCap` 不解绑。
- 大 body 的单次端点策略：只按 **body 字节数**判断，不再因为
  `estimated_tokens >= 30k` 把 642KB 的 agent 请求锁成单端点。

## 4. 端点选择（P0）

- 默认：健康主端点 → 不健康主端点 → 健康 Sandbox → 不健康 Sandbox
  （主端点冷却时优先探活，避免 Sandbox 抢占导致非美用户 400）。
- 大 body 上传（`body_len >= 512KB` 且内容请求）：健康主端点 → 健康 Sandbox →
  不健康主端点 → 不健康 Sandbox，避免把同一份 body 先重传到已知故障的主端点。
- Sandbox 的 400 仍按既有 `should_try_next_endpoint_for_url` 回退主端点。

## 5. 配置与生效值（P1）

- 前端「超时设置」字段与后端 `UploadTimeoutConfig` / `StreamTimeoutConfig` 一一对应；
- 公式文案使用 `1 + extra_wait_ratio` 动态计算，不再硬编码 `1.4`；
- 输入失焦按后端 `normalized()` 范围夹取：
  - `connect_secs` 3~120；
  - `min_upload_mbps` 0.1~1000；
  - `extra_wait_ratio` 0~10；
  - `stall_secs` 5~600；
  - `large_body_threshold_mb` >=1；
  - `large_body_endpoint_attempts` >=1。
- `get_effective_timeout_config` 返回配置值、归一化值、环境变量覆盖字段和
  512KB/1MB/2MB/8MB/47MB 样本预算；前端展示“当前生效值”。
- “最多尝试次数”表示**端点尝试次数**；账号轮换由 `max_attempts`
  （`pool_size * 2` clamp 4~12）控制，不能混用。

## 6. 观测（P1）

- `upstream_budget_extended`：软预算到期但仍有进展；
- `upstream_terminal kind=upload_stalled`：真正无进展；
- `upstream_send_err` 包含 `stage`、`progress_pct`、`effective_bps`、
  `budget_source`、`upload_budget`、`first_byte_budget`、`total_elapsed`；
- `service_status_middleware` 的 503 日志明确标记“service_status, not upstream”。

## 7. 迁移与回滚

- 旧配置字段全部保留；新增策略不改变线上配置的 serde 键名。
- `header_wait_secs` 已废弃但保留 round-trip，避免 UI 保存时静默重置。
- 回滚：恢复 `UpstreamTerminal::Upload` 的旧 disposition，并把上传看门狗
  的软预算改回硬终止；不涉及数据库 schema。
- Beta 验收通过后再进入 main；发布前按 `docs/RELEASE_GUIDE.md` 走 bump。

## 8. 验收

- 660KB / 1.3MB body，单请求：直连与网关均 200，上传耗时接近；
- 4 并发：不再出现 `Upload budget exhausted`；最多一次同账号重试；
- 10KB/s 慢流：不被固定 5~18s 杀死；
- Daily 冷却 + Sandbox 健康：上传走 Sandbox；
- UA 404：不写 900s 模型熔断，改 UA 后立即恢复；
- 前端公式、字段范围、账号轮换说明与实际生效值一致。
