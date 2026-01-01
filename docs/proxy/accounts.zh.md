# Proxy 账号池与自动禁用行为

## 目标
- 保持代理“持续可用”，即使部分 Google OAuth 账号失效。
- 避免对已撤销的 `refresh_token` 反复尝试刷新（噪声与资源浪费）。
- 在 UI 中清晰呈现账号状态，让问题可操作。

## 结果
### 1) 被禁用账号会被代理池跳过
账号文件可在磁盘上被标记为禁用（`accounts/<id>.json`）：
- `disabled: true`
- `disabled_at: <unix_ts>`
- `disabled_reason: <string>`

加载账号池时会跳过禁用账号：
- `TokenManager::load_single_account(...)`：[`src-tauri/src/proxy/token_manager.rs`](../../src-tauri/src/proxy/token_manager.rs)

### 2) OAuth `invalid_grant` 自动禁用
当刷新 token 失败并返回 `invalid_grant` 时，代理会将该账号标记为禁用并从内存池移除：
- 刷新/禁用逻辑：`TokenManager::get_token(...)`：[`src-tauri/src/proxy/token_manager.rs`](../../src-tauri/src/proxy/token_manager.rs)
- 写回磁盘：`TokenManager::disable_account(...)`：[`src-tauri/src/proxy/token_manager.rs`](../../src-tauri/src/proxy/token_manager.rs)

这可避免“死账号”被不停轮询。

### 3) 批量刷新配额会跳过禁用账号
批量刷新所有账号配额时，禁用账号会被直接跳过：
- `refresh_all_quotas(...)`：[`src-tauri/src/commands/mod.rs`](../../src-tauri/src/commands/mod.rs)

### 4) UI 呈现禁用状态并阻止操作
Accounts UI 会展示 Disabled 标识与提示，并禁用“切换/刷新”等动作：
- 类型字段：[`src/types/account.ts`](../../src/types/account.ts)
- 卡片视图：[`src/components/accounts/AccountCard.tsx`](../../src/components/accounts/AccountCard.tsx)
- 表格行视图：[`src/components/accounts/AccountRow.tsx`](../../src/components/accounts/AccountRow.tsx)
- 过滤逻辑：“Available” 不包含禁用账号：[`src/pages/Accounts.tsx`](../../src/pages/Accounts.tsx)

翻译：
- [`src/locales/en.json`](../../src/locales/en.json)
- [`src/locales/zh.json`](../../src/locales/zh.json)

### 5) API 错误避免泄露用户邮箱
对 API 客户端返回的 token 刷新错误不再包含邮箱：
- `TokenManager::get_token(...)`：[`src-tauri/src/proxy/token_manager.rs`](../../src-tauri/src/proxy/token_manager.rs)
- 代理错误映射：`handle_messages(...)`：[`src-tauri/src/proxy/handlers/claude.rs`](../../src-tauri/src/proxy/handlers/claude.rs)

## 运维建议
- 如果账号因 `invalid_grant` 被禁用，通常表示 `refresh_token` 已被撤销或过期。
- 重新授权（或更新存储的 token）即可恢复。

路由背景：
- 这些账号用于 Google 支持链路（Gemini 协议、OpenAI 协议，以及未走 z.ai 的 Claude 链路）。
- z.ai（启用时）不使用 Google OAuth token。
- 总览：[`docs/proxy/routing.zh.md`](routing.zh.md)

## 验证
1) 保证至少一个账号文件包含 `disabled: true`。
2) 启动代理并验证：
   - 禁用账号不会被选中处理请求。
   - 批量刷新配额日志显示 “Skipping … (Disabled)”。
   - UI 显示 Disabled 且阻止操作。

