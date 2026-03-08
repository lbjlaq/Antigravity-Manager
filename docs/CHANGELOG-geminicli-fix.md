# GeminiCLI 反代 403 修复 + 检验/预览功能 + 前端分类筛选

## 2026-03-01：gcli2api 路由/鉴权兼容迁移

### 新增兼容能力

1. **Gemini v1 原生路由兼容（gcli2api 风格）**
   - 新增 `GET/POST /v1/models/:model`
   - 新增 `POST /v1/models/:model/countTokens`
   - 继续保留 `v1beta` 路由，双前缀并存

2. **`:countTokens` 动作风格兼容**
   - `handle_generate` 新增 `countTokens` 动作分支
   - 支持 `POST /v1beta/models/{model}:countTokens` 与 `POST /v1/models/{model}:countTokens`
   - 修复此前 `Unsupported method: countTokens` 的报错

3. **`?key=` 鉴权兼容**
   - `auth_middleware` 支持从 query 参数提取密钥：`key` / `api_key`（并兼容 `x-api-key` / `x-goog-api-key` query 形式）
   - 与原有 Header 鉴权并存：`Authorization`、`x-api-key`、`x-goog-api-key`

4. **countTokens 行为对齐 gcli2api**
   - 改为本地启发式估算（约 4 字符 = 1 token）
   - 支持两种请求体结构：
     - `contents`
     - `generateContentRequest.contents`
   - 不再依赖上游 token 获取，避免无账号时额外报错

## 变更概述

本次更新解决了 GeminiCLI 账号添加后持续 403 的根本问题，并新增了检验、预览、前端分类筛选三项功能。

---

## Part 1：修复 403 根因 — project_id 获取

### 问题分析

`project_resolver.rs` 和 `quota.rs` 中的 `fetch_project_id()` 硬编码了 Antigravity 的 sandbox 端点和 User-Agent，导致 GeminiCLI 账号无法获取有效的 project_id，进而触发 403。

### 修改文件

| 文件 | 改动 |
|------|------|
| `src-tauri/src/proxy/project_resolver.rs` | `fetch_project_id()` 新增 `account_type` 参数；GeminiCLI 使用 prod 端点 `cloudcode-pa.googleapis.com` + UA `GeminiCLI/0.1.5 (Windows; AMD64)`；Antigravity 保持 sandbox 端点；新增 `try_onboard_user()` 回退（轮询 3 次，间隔 2s） |
| `src-tauri/src/modules/quota.rs` | `fetch_project_id()`、`fetch_quota()`、`fetch_quota_with_cache()`、`fetch_quota_inner()`、`fetch_all_quotas()` 全部新增 `account_type` 参数；fetchAvailableModels 的 User-Agent 也按 account_type 切换 |
| `src-tauri/src/modules/account_service.rs` | `add_account()` 和 `process_oauth_token()` 中的 `fetch_project_id()` 和 `fetch_quota()` 调用透传 `account_type` |
| `src-tauri/src/modules/account.rs` | 移除 `fetch_quota_with_retry()` 中的 GeminiCLI 早期返回；移除 `refresh_all_quotas_logic()` 中的 GeminiCLI 跳过过滤；所有 `fetch_quota()` 调用传入 `account.account_type` |
| `src-tauri/src/modules/mod.rs` | `fetch_quota` re-export 签名同步更新 |
| `src-tauri/src/proxy/token_manager.rs` | 3 处 `fetch_project_id()` + 1 处 `fetch_quota()` 调用补充 `account_type` 参数 |
| `src-tauri/src/modules/scheduler.rs` | 2 处 `fetch_quota_with_cache()` 调用补充 `account_type` 参数 |

### 端点对照

| 账号类型 | loadCodeAssist 端点 | User-Agent |
|----------|-------------------|------------|
| Antigravity | `https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist` | `vscode/1.X.X (Antigravity/...)` |
| GeminiCLI | `https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist` | `GeminiCLI/0.1.5 (Windows; AMD64)` |

---

## Part 2：新增「检验」功能

### 功能说明

对已添加的账号重新获取 project_id，并清除 403 禁用状态。适用于账号因临时问题被标记为 forbidden 后的恢复。

### 后端

- **`src-tauri/src/modules/account.rs`** — 新增 `verify_account(account_id)`:
  1. 加载账号
  2. 刷新 token（`ensure_fresh_token`）
  3. 调用 `fetch_project_id(access_token, account_type)` 获取新 project_id
  4. 更新 `account.token.project_id`
  5. 清除 `is_forbidden`、`proxy_disabled`、`forbidden_reason` 状态
  6. 保存账号 + 更新索引
  7. 通知反代重载账号

### API 端点

| 方式 | 路径 |
|------|------|
| Tauri Command | `verify_account { accountId }` |
| Admin HTTP API | `POST /admin/accounts/:id/verify` |

### 前端

- `src/services/accountService.ts` — `verifyAccount(accountId)`
- `src/stores/useAccountStore.ts` — `verifyAccount` action
- `src/components/accounts/AccountCard.tsx` — `CheckCircle` 图标按钮，`is_forbidden` 或 `proxy_disabled` 时高亮为琥珀色

---

## Part 3：新增「设置预览」功能（仅 GeminiCLI）

### 功能说明

为 GeminiCLI 账号配置实验性预览频道（Experimental Release Channel），解锁预览模型。

### 后端

- **`src-tauri/src/modules/account.rs`** — 新增 `configure_preview(account_id)`:
  1. 验证账号类型为 GeminiCLI
  2. 确认有 project_id
  3. 刷新 token
  4. Step 1: POST `cloudaicompanion.googleapis.com/.../releaseChannelSettings` → `{"release_channel": "EXPERIMENTAL"}`（接受 200/201/409）
  5. Step 2: POST `.../settingBindings` → `{"target": "projects/{pid}", "product": "GEMINI_CODE_ASSIST"}`（接受 200/201/409）
  6. 保存 `account.preview = true`

### 模型扩展

- **`src-tauri/src/models/account.rs`** — `Account` 新增 `#[serde(default)] pub preview: bool`

### API 端点

| 方式 | 路径 |
|------|------|
| Tauri Command | `configure_preview { accountId }` |
| Admin HTTP API | `POST /admin/accounts/:id/configure-preview` |

### 前端

- `src/types/account.ts` — `preview?: boolean`
- `src/services/accountService.ts` — `configurePreview(accountId)`
- `src/stores/useAccountStore.ts` — `configurePreview` action
- `src/components/accounts/AccountCard.tsx` — `Eye` 图标按钮，仅 GeminiCLI 账号显示，已配置时为绿色

---

## Part 4：前端账号类型分类筛选

### 修改文件

- **`src/pages/Accounts.tsx`**:
  - `FilterType` 扩展为 `"all" | "pro" | "ultra" | "free" | "antigravity" | "gcli"`
  - `filterCounts` 新增 `antigravity` 和 `gcli` 计数
  - `filteredAccounts` 新增对应过滤逻辑
  - 筛选按钮栏新增 **AG**（Antigravity）和 **GCLI** 按钮

### 筛选规则

| 标签 | 条件 |
|------|------|
| AG | `!account_type` 或 `account_type === 'antigravity'` |
| GCLI | `account_type === 'gemini_cli'` |

---

## Part 5：清理临时修复

### 修改文件

- **`src-tauri/src/proxy/token_manager.rs`**:
  - 移除 `get_account_state_on_disk()` 中的 `is_geminicli` 特殊绕过，GeminiCLI 账号现在和 Antigravity 账号使用相同的 disabled/forbidden 判定逻辑
  - 移除 `load_single_account()` 中的 GeminiCLI forbidden 自动清除逻辑（根因已修复，不再需要）

---

## 验证清单

| # | 场景 | 预期结果 |
|---|------|----------|
| 1 | 添加 GeminiCLI 账号 | 成功获取 project_id（日志可见），不报 403 |
| 2 | GeminiCLI 账号配额 | 能正常显示配额数据（模型列表） |
| 3 | 对 403 账号点击「检验」 | 重新获取 project_id → 解除禁用 |
| 4 | GeminiCLI 账号点击「设置预览」 | 配置成功 → preview 标记为 true（Eye 图标变绿） |
| 5 | 前端筛选 AG / GCLI | 正确过滤对应账号类型 |
| 6 | 已有 Antigravity 账号 | 功能不受影响（向后兼容） |

## 构建命令

```bash
# 后端
cd src-tauri && cargo build --release --bin antigravity_tools

# 前端
npm run build

# 运行
ABV_DIST_PATH=./dist API_KEY=test WEB_PASSWORD=pwd PORT=8045 RUST_LOG=info \
  ./src-tauri/target/release/antigravity_tools --headless
```
