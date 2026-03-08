# GeminiCLI 反代支持 - 实现文档

## 功能概述

为 Antigravity Manager 新增 **GeminiCLI 账号类型**支持。用户现在可以同时添加 Antigravity 和 GeminiCLI 两种类型的账号，系统会自动使用对应的 OAuth 凭据、上游端点和请求头进行反代。

> 2026-03-01 补充：已额外迁移 `gcli2api` 的 Gemini 路由/鉴权兼容能力，支持 `v1` 原生路径、`:countTokens` 动作风格和 `?key=` 鉴权参数。

### 核心能力

1. **双账号类型**：Antigravity（原有）+ GeminiCLI（新增），通过 `AccountType` 枚举区分
2. **独立 OAuth 凭据**：两种账号类型使用各自的 Google OAuth Client ID / Secret / Scopes
3. **差异化上游路由**：GeminiCLI 请求发往 `cloudcode-pa.googleapis.com`（PROD 单端点），Antigravity 保持原有 Sandbox → Daily → Prod 降级链
4. **差异化请求头**：GeminiCLI 只发 Authorization + Content-Type + User-Agent，不附加 x-client-name 等 IDE 头
5. **向后兼容**：已有 Antigravity 账号无需迁移，`account_type` 字段使用 `#[serde(default)]` 默认为 `antigravity`

---

## 技术差异对照

| 维度 | GeminiCLI | Antigravity（原有） |
|------|-----------|-------------------|
| OAuth Client ID | `681255809395-oo8ft2...` | `1071006060591-tmhssin2h21...` |
| OAuth Scopes | cloud-platform, email, profile | 前者 + cclog, experimentsandconfigs |
| 上游端点 | `cloudcode-pa.googleapis.com` (PROD 单一) | Sandbox → Daily → Prod 三级降级 |
| User-Agent | `GeminiCLI/0.1.5 (Windows; AMD64)` | `antigravity/x.x.x` |
| 额外 Headers | 无 | x-client-name, x-client-version, x-machine-id, x-vscode-sessionid |

---

## 代码改动清单

### 1. 数据模型层

**`src-tauri/src/models/account.rs`**
- 新增 `AccountType` 枚举（`Antigravity` / `GeminiCli`）
- 实现 `Default`（默认 Antigravity）、`Display`、`Serialize`/`Deserialize`
- `Account` 结构体增加 `#[serde(default)] pub account_type: AccountType` 字段
- `AccountSummary` 增加 `account_type` 字段
- `Account::new()` 接受 `account_type` 参数

**`src-tauri/src/models/mod.rs`**
- 导出 `AccountType`

### 2. OAuth 认证层

**`src-tauri/src/modules/oauth.rs`**
- 将原有凭据重命名为 `ANTIGRAVITY_CLIENT_ID` / `ANTIGRAVITY_CLIENT_SECRET`
- 新增 `GEMINICLI_CLIENT_ID` / `GEMINICLI_CLIENT_SECRET`
- 新增辅助函数：`get_oauth_credentials(account_type)` 和 `get_scopes(account_type)`
- `get_auth_url`、`exchange_code`、`refresh_access_token`、`ensure_fresh_token` 均增加 `account_type` 参数

**`src-tauri/src/modules/oauth_server.rs`**
- `OAuthFlowState` 增加 `account_type` 字段
- `prepare_oauth_url`、`start_oauth_flow`、`complete_oauth_flow`、`prepare_oauth_flow_manually` 均接受 `account_type`
- 新增 `get_current_flow_account_type()` 辅助函数

### 3. 账号管理层

**`src-tauri/src/modules/account.rs`**
- `add_account` 和 `upsert_account` 接受 `account_type` 参数
- 内部所有 `upsert_account`、`ensure_fresh_token`、`refresh_access_token` 调用均传入 `account.account_type`

**`src-tauri/src/modules/migration.rs`**
- 导入/迁移操作默认使用 `AccountType::Antigravity`

**`src-tauri/src/modules/account_service.rs`**
- `add_account`、`prepare_oauth_url`、`start_oauth_login`、`complete_oauth_login`、`process_oauth_token` 均增加 `account_type` 参数

### 4. 反代核心层

**`src-tauri/src/proxy/token_manager.rs`**
- `ProxyToken` 增加 `account_type` 字段
- `load_single_account` 从 JSON 读取 `account_type`（默认 Antigravity）
- `get_token`、`get_token_internal`、`get_token_by_email` 返回值从 5 元组扩展为 6 元组
- `exchange_code`、`get_user_info`、`get_oauth_url_with_redirect` 增加 `account_type` 参数

**`src-tauri/src/proxy/upstream/client.rs`**
- 新增 `GEMINICLI_BASE_URL = "https://cloudcode-pa.googleapis.com/v1internal"`
- `call_v1_internal` 和 `call_v1_internal_with_headers` 增加 `account_type` 参数
- **端点选择分支**：GeminiCLI → 仅 PROD；Antigravity → 3 端点降级
- **Header 构造分支**：GeminiCLI → 最小化头（Authorization + Content-Type + `GeminiCLI/0.1.5` UA）；Antigravity → 保持全套 IDE 头

**`src-tauri/src/proxy/handlers/*.rs`**（5 个文件）
- `openai.rs`、`claude.rs`、`gemini.rs`、`audio.rs`、`warmup.rs`
- 所有 `get_token` 解构从 5 元组更新为 6 元组
- 所有 `call_v1_internal` 调用传入 `&account_type`

### 5. API 层

**`src-tauri/src/proxy/server.rs`**
- `AccountResponse` 增加 `account_type: String` 字段
- `AddAccountRequest` 增加可选 `account_type` 字段
- 新增 `OAuthTypeRequest` 结构体（用于 OAuth API）
- 新增 `parse_account_type()` 辅助函数
- `admin_add_account`、`admin_prepare_oauth_url`、`admin_start_oauth_login`、`admin_complete_oauth_login` 均解析并传递 `account_type`
- `handle_oauth_callback` 从 query 参数或 flow state 获取 `account_type`
- `admin_prepare_oauth_url_web` 从 query 参数接收 `account_type`

**`src-tauri/src/commands/mod.rs`**（Tauri 桌面命令）
- `add_account`、`start_oauth_login`、`complete_oauth_login`、`prepare_oauth_url` 均接受可选 `account_type` 字符串参数

### 6. 前端

**`src/types/account.ts`**
- 新增 `AccountType` 类型（`'antigravity' | 'gemini_cli'`）
- `Account` 接口增加 `account_type?: AccountType`

**`src/services/accountService.ts`**
- `addAccount`、`startOAuthLogin`、`completeOAuthLogin` 增加可选 `accountType` 参数

**`src/stores/useAccountStore.ts`**
- `addAccount`、`startOAuthLogin`、`completeOAuthLogin` 透传 `accountType`

**`src/components/accounts/AddAccountDialog.tsx`**
- 新增 Antigravity / GeminiCLI 切换按钮（蓝色 / 绿色高亮）
- 切换类型时自动清除并重新生成 OAuth URL
- 所有 OAuth 和 Token 添加流程传递 `accountType`

**`src/components/accounts/AccountCard.tsx`**
- GeminiCLI 账号显示绿色 `GCLI` 标签徽章

**`src/pages/Accounts.tsx`** + **`src/pages/Dashboard.tsx`**
- `handleAddAccount` 透传 `accountType`

---

## 向后兼容性

- `AccountType` 使用 `#[serde(default)]`，反序列化时缺少该字段自动为 `Antigravity`
- 已有 JSON 存储的账号无需任何迁移即可正常加载
- 所有前端 API 的 `accountType` 参数均为可选，省略时默认 `antigravity`
- 两种账号类型可混合存在于同一账号池，token 轮换正常

---

## 验证检查项

1. 已有 Antigravity 账号正常加载（向后兼容，无 `account_type` 字段默认为 antigravity）
2. 添加 GeminiCLI 账号时 OAuth URL 使用正确的 client_id 和 scopes
3. GeminiCLI 账号请求发往 `cloudcode-pa.googleapis.com`（PROD），Header 中无 x-client-name 等
4. Antigravity 账号请求行为不变（Sandbox → Daily → Prod 降级链）
5. 两种账号混合时 token 轮换正常
6. Web 面板正确显示账号类型标签（GCLI 绿色徽章）
7. 添加账号对话框中可切换 Antigravity / GeminiCLI 类型

---

## gcli2api 兼容迁移（2026-03-01）

### 兼容点 1：Gemini v1 路由

- 新增 `GET/POST /v1/models/:model`
- 新增 `POST /v1/models/:model/countTokens`
- 继续支持 `v1beta` 路由，不影响已有调用方

### 兼容点 2：`:countTokens` 动作路由

- `handle_generate` 新增 `countTokens` 动作支持
- 可直接处理以下风格：
  - `/v1beta/models/{model}:countTokens`
  - `/v1/models/{model}:countTokens`

### 兼容点 3：Gemini `?key=` 鉴权

- `auth_middleware` 允许从 query 参数读取 API Key：
  - `key`
  - `api_key`
  - `x-api-key`
  - `x-goog-api-key`
- 与 Header 鉴权并存：
  - `Authorization`
  - `x-api-key`
  - `x-goog-api-key`

### 兼容点 4：countTokens 返回行为

- 对齐 `gcli2api`：使用本地启发式估算（约 4 字符 = 1 token）
- 支持 `contents` 与 `generateContentRequest.contents`
- 避免依赖上游 token 获取，减少不必要失败路径
