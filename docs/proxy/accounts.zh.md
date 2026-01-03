# 代理账号池与自动禁用行为

## 我们想要解决的问题
- 即使部分 Google OAuth 账号失效，也能让代理尽量保持可用。
- 避免对已失效/被撤销的 `refresh_token` 反复尝试刷新（噪音 + 浪费请求）。
- 让失败变得“可操作”：在 UI 中清晰呈现账号状态与原因。

## 当前实现结果
### 1）被禁用的账号会从代理池中跳过
账号文件（`accounts/<id>.json`）可以在磁盘上被标记为 disabled：
- `disabled: true`
- `disabled_at: <unix_ts>`
- `disabled_reason: <string>`

加载 token 池时会跳过这些账号：
- `TokenManager::load_single_account(...)` in [`src-tauri/src/proxy/token_manager.rs`](../../src-tauri/src/proxy/token_manager.rs)

### 2）遇到 OAuth `invalid_grant` 时自动禁用
当 refresh 过程中返回 `invalid_grant`，代理会把账号写入 disabled 并从内存池移除：
- 刷新/禁用：`TokenManager::get_token(...)` in [`src-tauri/src/proxy/token_manager.rs`](../../src-tauri/src/proxy/token_manager.rs)
- 写盘：`TokenManager::disable_account(...)` in [`src-tauri/src/proxy/token_manager.rs`](../../src-tauri/src/proxy/token_manager.rs)

这能防止代理在一个“已死账号”上无限轮询。

### 3）批量刷新配额时跳过 disabled 账号
批量刷新所有账号配额时，disabled 账号会立即跳过：
- `refresh_all_quotas(...)` in [`src-tauri/src/commands/mod.rs`](../../src-tauri/src/commands/mod.rs)

### 4）UI 会呈现 disabled 状态并阻止相关操作
账号 UI 会读取 `disabled*` 字段：
- 显示 “Disabled” badge + tooltip
- 禁用 “switch / refresh”等操作

相关实现：
- 类型定义包含 `disabled*` 字段：[`src/types/account.ts`](../../src/types/account.ts)
- Card 视图：[`src/components/accounts/AccountCard.tsx`](../../src/components/accounts/AccountCard.tsx)
- Table 行视图：[`src/components/accounts/AccountRow.tsx`](../../src/components/accounts/AccountRow.tsx)
- 过滤器：“Available” 会排除 disabled：[`src/pages/Accounts.tsx`](../../src/pages/Accounts.tsx)

翻译：
- [`src/locales/en.json`](../../src/locales/en.json)
- [`src/locales/zh.json`](../../src/locales/zh.json)

### 5）API 错误避免泄露邮箱
返回给 API 客户端的刷新错误不会包含账号邮箱：
- 错误信息拼装：`TokenManager::get_token(...)` in [`src-tauri/src/proxy/token_manager.rs`](../../src-tauri/src/proxy/token_manager.rs)
- Proxy 侧错误映射：`handle_messages(...)` in [`src-tauri/src/proxy/handlers/claude.rs`](../../src-tauri/src/proxy/handlers/claude.rs)

## 运维建议
- 账号因为 `invalid_grant` 被禁用，通常意味着 `refresh_token` 已过期或被撤销。
- 重新授权该账号（或在 UI 中更新 token）即可恢复。

## 验证方式
1）确保至少有一个账号文件被标记为 `disabled: true`。
2）启动代理后验证：
   - disabled 账号不会被选中处理请求
   - 批量刷新配额会跳过 disabled
   - UI 显示 Disabled 并阻止相关操作

