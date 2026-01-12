use serde_json;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

use crate::models::{Account, AccountIndex, AccountSummary, QuotaData, TokenData};
use crate::modules;
use once_cell::sync::Lazy;
use std::sync::Mutex;

/// 全局账号写入锁，防止并发操作导致索引文件损坏
static ACCOUNT_INDEX_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

// ... existing constants ...
const DATA_DIR: &str = ".antigravity_tools";
const ACCOUNTS_INDEX: &str = "accounts.json";
const ACCOUNTS_DIR: &str = "accounts";

// ... existing functions get_data_dir, get_accounts_dir, load_account_index, save_account_index ...
/// 获取数据目录路径
pub fn get_data_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("无法获取用户主目录")?;
    let data_dir = home.join(DATA_DIR);

    // 确保目录存在
    if !data_dir.exists() {
        fs::create_dir_all(&data_dir).map_err(|e| format!("创建数据目录失败: {}", e))?;
    }

    Ok(data_dir)
}

/// 获取账号目录路径
pub fn get_accounts_dir() -> Result<PathBuf, String> {
    let data_dir = get_data_dir()?;
    let accounts_dir = data_dir.join(ACCOUNTS_DIR);

    if !accounts_dir.exists() {
        fs::create_dir_all(&accounts_dir).map_err(|e| format!("创建账号目录失败: {}", e))?;
    }

    Ok(accounts_dir)
}

/// 加载账号索引
pub fn load_account_index() -> Result<AccountIndex, String> {
    let data_dir = get_data_dir()?;
    let index_path = data_dir.join(ACCOUNTS_INDEX);
    // modules::logger::log_info(&format!("正在加载账号索引: {:?}", index_path)); // Optional: reduce noise

    if !index_path.exists() {
        crate::modules::logger::log_warn("账号索引文件不存在");
        return Ok(AccountIndex::new());
    }

    let content =
        fs::read_to_string(&index_path).map_err(|e| format!("读取账号索引失败: {}", e))?;

    let index: AccountIndex =
        serde_json::from_str(&content).map_err(|e| format!("解析账号索引失败: {}", e))?;

    crate::modules::logger::log_info(&format!(
        "成功加载索引，包含 {} 个账号",
        index.accounts.len()
    ));
    Ok(index)
}

/// 保存账号索引 (原子化写入)
pub fn save_account_index(index: &AccountIndex) -> Result<(), String> {
    let data_dir = get_data_dir()?;
    let index_path = data_dir.join(ACCOUNTS_INDEX);
    let temp_path = data_dir.join(format!("{}.tmp", ACCOUNTS_INDEX));

    let content =
        serde_json::to_string_pretty(index).map_err(|e| format!("序列化账号索引失败: {}", e))?;

    // 写入临时文件
    fs::write(&temp_path, content).map_err(|e| format!("写入临时索引文件失败: {}", e))?;

    // 原子重命名
    fs::rename(temp_path, index_path).map_err(|e| format!("替换索引文件失败: {}", e))
}

/// 加载账号数据
pub fn load_account(account_id: &str) -> Result<Account, String> {
    let accounts_dir = get_accounts_dir()?;
    let account_path = accounts_dir.join(format!("{}.json", account_id));

    if !account_path.exists() {
        return Err(format!("账号不存在: {}", account_id));
    }

    let content =
        fs::read_to_string(&account_path).map_err(|e| format!("读取账号数据失败: {}", e))?;

    serde_json::from_str(&content).map_err(|e| format!("解析账号数据失败: {}", e))
}

/// 保存账号数据
pub fn save_account(account: &Account) -> Result<(), String> {
    let accounts_dir = get_accounts_dir()?;
    let account_path = accounts_dir.join(format!("{}.json", account.id));

    let content =
        serde_json::to_string_pretty(account).map_err(|e| format!("序列化账号数据失败: {}", e))?;

    fs::write(&account_path, content).map_err(|e| format!("保存账号数据失败: {}", e))
}

/// 列出所有账号
/// 列出所有账号
pub fn list_accounts() -> Result<Vec<Account>, String> {
    crate::modules::logger::log_info("已开始列出账号...");
    let mut index = load_account_index()?;
    let mut accounts = Vec::new();
    let mut invalid_ids = Vec::new();

    for summary in &index.accounts {
        match load_account(&summary.id) {
            Ok(account) => accounts.push(account),
            Err(e) => {
                crate::modules::logger::log_error(&format!("加载账号 {} 失败: {}", summary.id, e));
                // 如果是文件不存在导致的错误，标记为无效 ID
                // load_account 返回 "账号不存在: id" 或者底层 io error
                if e.contains("账号不存在")
                    || e.contains("Os { code: 2,")
                    || e.contains("No such file")
                {
                    invalid_ids.push(summary.id.clone());
                }
            }
        }
    }

    // 自动修复索引：移除无效的账号 ID
    if !invalid_ids.is_empty() {
        crate::modules::logger::log_warn(&format!(
            "发现 {} 个无效的账号索引，正在自动清理...",
            invalid_ids.len()
        ));

        index.accounts.retain(|s| !invalid_ids.contains(&s.id));

        // 如果当前选中的账号也是无效的，重置为第一个可用账号
        if let Some(current_id) = &index.current_account_id {
            if invalid_ids.contains(current_id) {
                index.current_account_id = index.accounts.first().map(|s| s.id.clone());
            }
        }

        if let Err(e) = save_account_index(&index) {
            crate::modules::logger::log_error(&format!("自动清理索引失败: {}", e));
        } else {
            crate::modules::logger::log_info("索引自动清理完成");
        }
    }

    // modules::logger::log_info(&format!("共找到 {} 个有效账号", accounts.len()));
    Ok(accounts)
}

/// 添加账号
pub fn add_account(
    email: String,
    name: Option<String>,
    token: TokenData,
) -> Result<Account, String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let mut index = load_account_index()?;

    // 检查是否已存在
    if index.accounts.iter().any(|s| s.email == email) {
        return Err(format!("账号已存在: {}", email));
    }

    // 创建新账号
    let account_id = Uuid::new_v4().to_string();
    let mut account = Account::new(account_id.clone(), email.clone(), token);
    account.name = name.clone();

    // 保存账号数据
    save_account(&account)?;

    // 更新索引
    index.accounts.push(AccountSummary {
        id: account_id.clone(),
        email: email.clone(),
        name: name.clone(),
        created_at: account.created_at,
        last_used: account.last_used,
    });

    // 如果是第一个账号，设为当前账号
    if index.current_account_id.is_none() {
        index.current_account_id = Some(account_id);
    }

    save_account_index(&index)?;

    Ok(account)
}

/// 添加或更新账号
pub fn upsert_account(
    email: String,
    name: Option<String>,
    token: TokenData,
) -> Result<Account, String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let mut index = load_account_index()?;

    // 先找到账号 ID（如果存在）
    let existing_account_id = index
        .accounts
        .iter()
        .find(|s| s.email == email)
        .map(|s| s.id.clone());

    if let Some(account_id) = existing_account_id {
        // 更新现有账号
        match load_account(&account_id) {
            Ok(mut account) => {
                let old_access_token = account.token.access_token.clone();
                let old_refresh_token = account.token.refresh_token.clone();
                account.token = token;
                account.name = name.clone();
                // If an account was previously disabled (e.g. invalid_grant), any explicit token upsert
                // should re-enable it (user manually updated credentials in the UI).
                if account.disabled
                    && (account.token.refresh_token != old_refresh_token
                        || account.token.access_token != old_access_token)
                {
                    account.disabled = false;
                    account.disabled_reason = None;
                    account.disabled_at = None;
                }
                account.update_last_used();
                save_account(&account)?;

                // 同步更新索引中的 name
                if let Some(idx_summary) = index.accounts.iter_mut().find(|s| s.id == account_id) {
                    idx_summary.name = name;
                    save_account_index(&index)?;
                }

                return Ok(account);
            }
            Err(e) => {
                crate::modules::logger::log_warn(&format!(
                    "Account {} file missing ({}), recreating...",
                    account_id, e
                ));
                // 索引存在但文件丢失，重新创建
                let mut account = Account::new(account_id.clone(), email.clone(), token);
                account.name = name.clone();
                save_account(&account)?;

                // 同步更新索引中的 name
                if let Some(idx_summary) = index.accounts.iter_mut().find(|s| s.id == account_id) {
                    idx_summary.name = name;
                    save_account_index(&index)?;
                }

                return Ok(account);
            }
        }
    }

    // 不存在则添加
    // 注意：这里手动调用 add_account，它也会尝试获取锁，但因为 Mutex 库限制会死锁
    // 所以我们需要一个不带锁的内部版本，或者重构。简单起见，这里直接展开添加逻辑或不重复加锁

    // 释放锁，让 add_account 处理
    drop(_lock);
    add_account(email, name, token)
}

/// 删除账号
pub fn delete_account(account_id: &str) -> Result<(), String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let mut index = load_account_index()?;

    // 从索引中移除
    let original_len = index.accounts.len();
    index.accounts.retain(|s| s.id != account_id);

    if index.accounts.len() == original_len {
        return Err(format!("找不到账号 ID: {}", account_id));
    }

    // 如果是当前账号，清除当前账号
    if index.current_account_id.as_deref() == Some(account_id) {
        index.current_account_id = index.accounts.first().map(|s| s.id.clone());
    }

    save_account_index(&index)?;

    // 删除账号文件
    let accounts_dir = get_accounts_dir()?;
    let account_path = accounts_dir.join(format!("{}.json", account_id));

    if account_path.exists() {
        fs::remove_file(&account_path).map_err(|e| format!("删除账号文件失败: {}", e))?;
    }

    Ok(())
}

/// 批量删除账号 (原子性操作索引)
pub fn delete_accounts(account_ids: &[String]) -> Result<(), String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let mut index = load_account_index()?;

    let accounts_dir = get_accounts_dir()?;

    for account_id in account_ids {
        // 从索引中移除
        index.accounts.retain(|s| &s.id != account_id);

        // 如果是当前账号，清除当前账号
        if index.current_account_id.as_deref() == Some(account_id) {
            index.current_account_id = None;
        }

        // 删除账号文件
        let account_path = accounts_dir.join(format!("{}.json", account_id));
        if account_path.exists() {
            let _ = fs::remove_file(&account_path);
        }
    }

    // 如果当前账号为空，尝试选取第一个作为默认
    if index.current_account_id.is_none() {
        index.current_account_id = index.accounts.first().map(|s| s.id.clone());
    }

    save_account_index(&index)
}

/// 重新排序账号列表
/// 根据传入的账号ID顺序更新索引文件中的账号排列顺序
pub fn reorder_accounts(account_ids: &[String]) -> Result<(), String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let mut index = load_account_index()?;

    // 创建一个映射，记录每个账号ID对应的摘要信息
    let id_to_summary: std::collections::HashMap<_, _> = index
        .accounts
        .iter()
        .map(|s| (s.id.clone(), s.clone()))
        .collect();

    // 按照新顺序重建账号列表
    let mut new_accounts = Vec::new();
    for id in account_ids {
        if let Some(summary) = id_to_summary.get(id) {
            new_accounts.push(summary.clone());
        }
    }

    // 添加未在新顺序中出现的账号（保持原有顺序追加到末尾）
    for summary in &index.accounts {
        if !account_ids.contains(&summary.id) {
            new_accounts.push(summary.clone());
        }
    }

    index.accounts = new_accounts;

    crate::modules::logger::log_info(&format!(
        "账号顺序已更新，共 {} 个账号",
        index.accounts.len()
    ));

    save_account_index(&index)
}

/// 切换当前账号
pub async fn switch_account(account_id: &str) -> Result<(), String> {
    use crate::modules::{db, oauth, process};

    let index = {
        let _lock = ACCOUNT_INDEX_LOCK
            .lock()
            .map_err(|e| format!("获取锁失败: {}", e))?;
        load_account_index()?
    };

    // 1. 验证账号存在
    if !index.accounts.iter().any(|s| s.id == account_id) {
        return Err(format!("账号不存在: {}", account_id));
    }

    let mut account = load_account(account_id)?;
    crate::modules::logger::log_info(&format!(
        "正在切换到账号: {} (ID: {})",
        account.email, account.id
    ));

    // 2. 确保 Token 有效（自动刷新）
    let fresh_token = oauth::ensure_fresh_token(&account.token)
        .await
        .map_err(|e| format!("Token 刷新失败: {}", e))?;

    // 如果 Token 更新了，保存回账号文件
    if fresh_token.access_token != account.token.access_token {
        account.token = fresh_token.clone();
        save_account(&account)?;
    }

    // 3. 关闭 Antigravity (增加超时时间到 20 秒)
    if process::is_antigravity_running() {
        process::close_antigravity(20)?;
    }

    // 4. 获取数据库路径并备份
    let db_path = db::get_db_path()?;
    if db_path.exists() {
        let backup_path = db_path.with_extension("vscdb.backup");
        fs::copy(&db_path, &backup_path).map_err(|e| format!("备份数据库失败: {}", e))?;
    } else {
        crate::modules::logger::log_info("数据库不存在，跳过备份");
    }

    // 5. 注入 Token
    crate::modules::logger::log_info("正在注入 Token 到数据库...");
    db::inject_token(
        &db_path,
        &account.token.access_token,
        &account.token.refresh_token,
        account.token.expiry_timestamp,
    )?;

    // 6. 更新工具内部状态
    {
        let _lock = ACCOUNT_INDEX_LOCK
            .lock()
            .map_err(|e| format!("获取锁失败: {}", e))?;
        let mut index = load_account_index()?;
        index.current_account_id = Some(account_id.to_string());
        save_account_index(&index)?;
    }

    account.update_last_used();
    save_account(&account)?;

    // 7. 重启 Antigravity
    process::start_antigravity()?;
    crate::modules::logger::log_info(&format!("账号切换完成: {}", account.email));

    Ok(())
}

/// 获取当前账号 ID
pub fn get_current_account_id() -> Result<Option<String>, String> {
    let index = load_account_index()?;
    Ok(index.current_account_id)
}

/// 获取当前激活账号的具体信息
pub fn get_current_account() -> Result<Option<Account>, String> {
    if let Some(id) = get_current_account_id()? {
        Ok(Some(load_account(&id)?))
    } else {
        Ok(None)
    }
}

/// 设置当前激活账号 ID
pub fn set_current_account_id(account_id: &str) -> Result<(), String> {
    let _lock = ACCOUNT_INDEX_LOCK
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let mut index = load_account_index()?;
    index.current_account_id = Some(account_id.to_string());
    save_account_index(&index)
}

/// 更新账号配额
pub fn update_account_quota(account_id: &str, quota: QuotaData) -> Result<(), String> {
    let mut account = load_account(account_id)?;
    account.update_quota(quota);
    save_account(&account)
}

/// 导出所有账号的 refresh_token
#[allow(dead_code)]
pub fn export_accounts() -> Result<Vec<(String, String)>, String> {
    let accounts = list_accounts()?;
    let mut exports = Vec::new();

    for account in accounts {
        exports.push((account.email, account.token.refresh_token));
    }

    Ok(exports)
}

/// 带有重试机制的配额查询 (从 commands 移动到 modules 以便共享)
pub async fn fetch_quota_with_retry(account: &mut Account) -> crate::error::AppResult<QuotaData> {
    use crate::error::AppError;
    use crate::modules::oauth;
    use reqwest::StatusCode;

    // 1. 基于时间的检查 (Time-based check) - 先确保 Token 有效
    let token = match oauth::ensure_fresh_token(&account.token).await {
        Ok(t) => t,
        Err(e) => {
            if e.contains("invalid_grant") {
                modules::logger::log_error(&format!(
                    "Disabling account {} due to invalid_grant during token refresh (quota check)",
                    account.email
                ));
                account.disabled = true;
                account.disabled_at = Some(chrono::Utc::now().timestamp());
                account.disabled_reason = Some(format!("invalid_grant: {}", e));
                let _ = save_account(account);
            }
            return Err(AppError::OAuth(e));
        }
    };

    if token.access_token != account.token.access_token {
        modules::logger::log_info(&format!("基于时间的 Token 刷新: {}", account.email));
        account.token = token.clone();

        // 重新获取用户名 (Token 刷新后顺便获取)
        let name = if account.name.is_none()
            || account.name.as_ref().is_some_and(|n| n.trim().is_empty())
        {
            match oauth::get_user_info(&token.access_token).await {
                Ok(user_info) => user_info.get_display_name(),
                Err(_) => None,
            }
        } else {
            account.name.clone()
        };

        account.name = name.clone();
        upsert_account(account.email.clone(), name, token.clone()).map_err(AppError::Account)?;
    }

    // 0. 补充用户名 (如果 Token 没过期但也没用户名，或者上面没获取到)
    if account.name.is_none() || account.name.as_ref().is_some_and(|n| n.trim().is_empty()) {
        modules::logger::log_info(&format!("账号 {} 缺少用户名，尝试获取...", account.email));
        // 使用更新后的 token
        match oauth::get_user_info(&account.token.access_token).await {
            Ok(user_info) => {
                let display_name = user_info.get_display_name();
                modules::logger::log_info(&format!("成功获取用户名: {:?}", display_name));
                account.name = display_name.clone();
                // 立即保存
                if let Err(e) =
                    upsert_account(account.email.clone(), display_name, account.token.clone())
                {
                    modules::logger::log_warn(&format!("保存用户名失败: {}", e));
                }
            }
            Err(e) => {
                modules::logger::log_warn(&format!("获取用户名失败: {}", e));
            }
        }
    }

    // 2. 尝试查询
    let result: crate::error::AppResult<(QuotaData, Option<String>)> =
        modules::fetch_quota(&account.token.access_token, &account.email).await;

    // 捕获可能更新的 project_id 并保存
    if let Ok((ref _q, ref project_id)) = result {
        if project_id.is_some() && *project_id != account.token.project_id {
            modules::logger::log_info(&format!(
                "检测到 project_id 更新 ({}), 正在保存...",
                account.email
            ));
            account.token.project_id = project_id.clone();
            if let Err(e) = upsert_account(
                account.email.clone(),
                account.name.clone(),
                account.token.clone(),
            ) {
                modules::logger::log_warn(&format!("同步保存 project_id 失败: {}", e));
            }
        }
    }

    // 3. 处理 401 错误 (Handle 401)
    if let Err(AppError::Network(ref e)) = result {
        if let Some(status) = e.status() {
            if status == StatusCode::UNAUTHORIZED {
                modules::logger::log_warn(&format!(
                    "401 Unauthorized for {}, forcing refresh...",
                    account.email
                ));

                // 强制刷新
                let token_res = match oauth::refresh_access_token(&account.token.refresh_token)
                    .await
                {
                    Ok(t) => t,
                    Err(e) => {
                        if e.contains("invalid_grant") {
                            modules::logger::log_error(&format!(
                                "Disabling account {} due to invalid_grant during forced refresh (quota check)",
                                account.email
                            ));
                            account.disabled = true;
                            account.disabled_at = Some(chrono::Utc::now().timestamp());
                            account.disabled_reason = Some(format!("invalid_grant: {}", e));
                            let _ = save_account(account);
                        }
                        return Err(AppError::OAuth(e));
                    }
                };

                let new_token = TokenData::new(
                    token_res.access_token.clone(),
                    account.token.refresh_token.clone(),
                    token_res.expires_in,
                    account.token.email.clone(),
                    account.token.project_id.clone(), // 保留原有 project_id
                    None,                             // 添加 None 作为 session_id
                );

                // 重新获取用户名
                let name = if account.name.is_none()
                    || account.name.as_ref().is_some_and(|n| n.trim().is_empty())
                {
                    match oauth::get_user_info(&token_res.access_token).await {
                        Ok(user_info) => user_info.get_display_name(),
                        Err(_) => None,
                    }
                } else {
                    account.name.clone()
                };

                account.token = new_token.clone();
                account.name = name.clone();
                upsert_account(account.email.clone(), name, new_token.clone())
                    .map_err(AppError::Account)?;

                // 重试查询
                let retry_result: crate::error::AppResult<(QuotaData, Option<String>)> =
                    modules::fetch_quota(&new_token.access_token, &account.email).await;

                // 同样处理重试时的 project_id 保存
                if let Ok((ref _q, ref project_id)) = retry_result {
                    if project_id.is_some() && *project_id != account.token.project_id {
                        modules::logger::log_info(&format!(
                            "检测到重试后 project_id 更新 ({}), 正在保存...",
                            account.email
                        ));
                        account.token.project_id = project_id.clone();
                        let _ = upsert_account(
                            account.email.clone(),
                            account.name.clone(),
                            account.token.clone(),
                        );
                    }
                }

                if let Err(AppError::Network(ref e)) = retry_result {
                    if let Some(s) = e.status() {
                        if s == StatusCode::FORBIDDEN {
                            let mut q = QuotaData::new();
                            q.is_forbidden = true;
                            return Ok(q);
                        }
                    }
                }
                return retry_result.map(|(q, _)| q);
            }
        }
    }

    // fetch_quota 已经处理了 403 错误,这里直接返回结果
    result.map(|(q, _)| q)
}
