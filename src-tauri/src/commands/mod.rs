use crate::models::{Account, AppConfig, QuotaData, TokenData};
use crate::modules;
use tauri::{Emitter, Manager};

// 导出 proxy 命令
pub mod proxy;
// 导出 autostart 命令
pub mod autostart;

/// 列出所有账号
#[tauri::command]
pub async fn list_accounts() -> Result<Vec<Account>, String> {
    modules::list_accounts()
}

/// 添加账号
#[tauri::command]
pub async fn add_account(
    app: tauri::AppHandle,
    _email: String,
    refresh_token: String,
) -> Result<Account, String> {
    // 1. 使用 refresh_token 获取 access_token
    // 注意：这里我们忽略传入的 _email，而是直接去 Google 获取真实的邮箱
    let token_res = modules::oauth::refresh_access_token(&refresh_token).await?;

    // 2. 获取用户信息
    let user_info = modules::oauth::get_user_info(&token_res.access_token).await?;

    // 3. 构造 TokenData
    let token = TokenData::new(
        token_res.access_token,
        refresh_token, // 继续使用用户传入的 refresh_token
        token_res.expires_in,
        Some(user_info.email.clone()),
        None, // project_id 将在需要时获取
        None, // session_id
    );

    // 4. 使用真实的 email 添加或更新账号
    let account =
        modules::upsert_account(user_info.email.clone(), user_info.get_display_name(), token)?;

    modules::logger::log_info(&format!("添加账号成功: {}", account.email));

    // 5. 自动触发刷新额度
    let mut account = account;
    let _ = internal_refresh_account_quota(&app, &mut account).await;

    // 6. If proxy is running, reload token pool so changes take effect immediately.
    let _ = crate::commands::proxy::reload_proxy_accounts(
        app.state::<crate::commands::proxy::ProxyServiceState>(),
    )
    .await;

    Ok(account)
}

/// 删除账号
#[tauri::command]
pub async fn delete_account(app: tauri::AppHandle, account_id: String) -> Result<(), String> {
    modules::logger::log_info(&format!("收到删除账号请求: {}", account_id));
    modules::delete_account(&account_id).map_err(|e| {
        modules::logger::log_error(&format!("删除账号失败: {}", e));
        e
    })?;
    modules::logger::log_info(&format!("账号删除成功: {}", account_id));

    // 强制同步托盘
    crate::modules::tray::update_tray_menus(&app);
    Ok(())
}

/// 批量删除账号
#[tauri::command]
pub async fn delete_accounts(
    app: tauri::AppHandle,
    account_ids: Vec<String>,
) -> Result<(), String> {
    modules::logger::log_info(&format!(
        "收到批量删除请求，共 {} 个账号",
        account_ids.len()
    ));
    modules::account::delete_accounts(&account_ids).map_err(|e| {
        modules::logger::log_error(&format!("批量删除失败: {}", e));
        e
    })?;

    // 强制同步托盘
    crate::modules::tray::update_tray_menus(&app);
    Ok(())
}

/// 重新排序账号列表
/// 根据传入的账号ID数组顺序更新账号排列
#[tauri::command]
pub async fn reorder_accounts(account_ids: Vec<String>) -> Result<(), String> {
    modules::logger::log_info(&format!("收到账号重排序请求，共 {} 个账号", account_ids.len()));
    modules::account::reorder_accounts(&account_ids).map_err(|e| {
        modules::logger::log_error(&format!("账号重排序失败: {}", e));
        e
    })
}

/// 切换账号
#[tauri::command]
pub async fn switch_account(app: tauri::AppHandle, account_id: String) -> Result<(), String> {
    let res = modules::switch_account(&account_id).await;
    if res.is_ok() {
        crate::modules::tray::update_tray_menus(&app);
    }
    res
}

/// 获取当前账号
#[tauri::command]
pub async fn get_current_account() -> Result<Option<Account>, String> {
    // println!("🚀 Backend Command: get_current_account called"); // Commented out to reduce noise for frequent calls, relies on frontend log for frequency
    // Actually user WANTS to see it.
    modules::logger::log_info("Backend Command: get_current_account called");

    let account_id = modules::get_current_account_id()?;

    if let Some(id) = account_id {
        // modules::logger::log_info(&format!("   Found current account ID: {}", id));
        modules::load_account(&id).map(Some)
    } else {
        modules::logger::log_info("   No current account set");
        Ok(None)
    }
}

/// 内部辅助功能：在添加或导入账号后自动刷新一次额度
async fn internal_refresh_account_quota(
    app: &tauri::AppHandle,
    account: &mut Account,
) -> Result<QuotaData, String> {
    modules::logger::log_info(&format!("自动触发刷新配额: {}", account.email));

    // 使用带重试的查询 (Shared logic)
    match modules::account::fetch_quota_with_retry(account).await {
        Ok(quota) => {
            // 更新账号配额
            let _ = modules::update_account_quota(&account.id, quota.clone());
            // 更新托盘菜单
            crate::modules::tray::update_tray_menus(app);
            Ok(quota)
        }
        Err(e) => {
            modules::logger::log_warn(&format!("自动刷新配额失败 ({}): {}", account.email, e));
            Err(e.to_string())
        }
    }
}

/// 查询账号配额
#[tauri::command]
pub async fn fetch_account_quota(
    app: tauri::AppHandle,
    account_id: String,
) -> crate::error::AppResult<QuotaData> {
    modules::logger::log_info(&format!("手动刷新配额请求: {}", account_id));
    let mut account =
        modules::load_account(&account_id).map_err(crate::error::AppError::Account)?;

    // 使用带重试的查询 (Shared logic)
    let quota = modules::account::fetch_quota_with_retry(&mut account).await?;

    // 4. 更新账号配额
    modules::update_account_quota(&account_id, quota.clone())
        .map_err(crate::error::AppError::Account)?;

    crate::modules::tray::update_tray_menus(&app);

    Ok(quota)
}

#[derive(serde::Serialize)]
pub struct RefreshStats {
    total: usize,
    success: usize,
    failed: usize,
    details: Vec<String>,
}

/// 刷新所有账号配额
#[tauri::command]
pub async fn refresh_all_quotas() -> Result<RefreshStats, String> {
    modules::logger::log_info("开始批量刷新所有账号配额");
    let accounts = modules::list_accounts()?;

    let mut success = 0;
    let mut failed = 0;
    let mut details = Vec::new();

    // 串行处理以确保持久化安全 (SQLite)
    for mut account in accounts {
        if account.disabled {
            modules::logger::log_info(&format!("  - Skipping {} (Disabled)", account.email));
            continue;
        }
        if let Some(ref q) = account.quota {
            if q.is_forbidden {
                modules::logger::log_info(&format!("  - Skipping {} (Forbidden)", account.email));
                continue;
            }
        }

        modules::logger::log_info(&format!("  - Processing {}", account.email));

        match modules::account::fetch_quota_with_retry(&mut account).await {
            Ok(quota) => {
                // 保存配额
                if let Err(e) = modules::update_account_quota(&account.id, quota) {
                    failed += 1;
                    let msg = format!("Account {}: Save quota failed - {}", account.email, e);
                    details.push(msg.clone());
                    modules::logger::log_error(&msg);
                } else {
                    success += 1;
                    modules::logger::log_info("    ✅ Success");
                }
            }
            Err(e) => {
                failed += 1;
                // e might be AppError, assume it implements Display
                let msg = format!("Account {}: Fetch quota failed - {}", account.email, e);
                details.push(msg.clone());
                modules::logger::log_error(&msg);
            }
        }
    }

    modules::logger::log_info(&format!("批量刷新完成: {} 成功, {} 失败", success, failed));
    Ok(RefreshStats {
        total: success + failed,
        success,
        failed,
        details,
    })
}

/// 加载配置
#[tauri::command]
pub async fn load_config() -> Result<AppConfig, String> {
    modules::load_app_config()
}

/// 保存配置
#[tauri::command]
pub async fn save_config(
    app: tauri::AppHandle,
    proxy_state: tauri::State<'_, crate::commands::proxy::ProxyServiceState>,
    config: AppConfig,
) -> Result<(), String> {
    modules::save_app_config(&config)?;

    // 通知托盘配置已更新
    let _ = app.emit("config://updated", ());

    // 热更新正在运行的服务
    let instance_lock = proxy_state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        // 更新模型映射
        instance.axum_server.update_mapping(&config.proxy).await;
        // 更新上游代理
        instance
            .axum_server
            .update_proxy(config.proxy.upstream_proxy.clone())
            .await;
        // 更新安全策略 (auth)
        instance.axum_server.update_security(&config.proxy).await;
        // 更新 z.ai 配置
        instance.axum_server.update_zai(&config.proxy).await;
        // 更新可观测性配置
        instance.axum_server.update_observability(&config.proxy).await;
        tracing::debug!("已同步热更新反代服务配置");
    }

    Ok(())
}

// --- OAuth 命令 ---

#[tauri::command]
pub async fn start_oauth_login(app_handle: tauri::AppHandle) -> Result<Account, String> {
    modules::logger::log_info("开始 OAuth 授权流程...");

    // 1. 启动 OAuth 流程获取 Token
    let token_res = modules::oauth_server::start_oauth_flow(app_handle.clone()).await?;

    // 2. 检查 refresh_token
    let refresh_token = token_res.refresh_token.ok_or_else(|| {
        "未获取到 Refresh Token。\n\n\
         可能原因:\n\
         1. 您之前已授权过此应用,Google 不会再次返回 refresh_token\n\n\
         解决方案:\n\
         1. 访问 https://myaccount.google.com/permissions\n\
         2. 撤销 'Antigravity Tools' 的访问权限\n\
         3. 重新进行 OAuth 授权\n\n\
         或者使用 'Refresh Token' 标签页手动添加账号"
            .to_string()
    })?;

    // 3. 获取用户信息
    let user_info = modules::oauth::get_user_info(&token_res.access_token).await?;
    modules::logger::log_info(&format!("获取用户信息成功: {}", user_info.email));

    // 4. 尝试获取项目ID
    let project_id = crate::proxy::project_resolver::fetch_project_id(&token_res.access_token)
        .await
        .ok();

    if let Some(ref pid) = project_id {
        modules::logger::log_info(&format!("获取项目ID成功: {}", pid));
    } else {
        modules::logger::log_warn("未能获取项目ID,将在后续懒加载");
    }

    // 5. 构造 TokenData
    let token_data = TokenData::new(
        token_res.access_token,
        refresh_token,
        token_res.expires_in,
        Some(user_info.email.clone()),
        project_id,
        None,
    );

    // 6. 添加或更新到账号列表
    modules::logger::log_info("正在保存账号信息...");
    let mut account = modules::upsert_account(
        user_info.email.clone(),
        user_info.get_display_name(),
        token_data,
    )?;

    // 7. 自动触发刷新额度
    let _ = internal_refresh_account_quota(&app_handle, &mut account).await;

    // 8. If proxy is running, reload token pool so changes take effect immediately.
    let _ = crate::commands::proxy::reload_proxy_accounts(
        app_handle.state::<crate::commands::proxy::ProxyServiceState>(),
    )
    .await;

    Ok(account)
}

/// 完成 OAuth 授权（不自动打开浏览器）
#[tauri::command]
pub async fn complete_oauth_login(app_handle: tauri::AppHandle) -> Result<Account, String> {
    modules::logger::log_info("完成 OAuth 授权流程 (manual)...");

    // 1. 等待回调并交换 Token（不 open browser）
    let token_res = modules::oauth_server::complete_oauth_flow(app_handle.clone()).await?;

    // 2. 检查 refresh_token
    let refresh_token = token_res.refresh_token.ok_or_else(|| {
        "未获取到 Refresh Token。\n\n\
         可能原因:\n\
         1. 您之前已授权过此应用,Google 不会再次返回 refresh_token\n\n\
         解决方案:\n\
         1. 访问 https://myaccount.google.com/permissions\n\
         2. 撤销 'Antigravity Tools' 的访问权限\n\
         3. 重新进行 OAuth 授权\n\n\
         或者使用 'Refresh Token' 标签页手动添加账号"
            .to_string()
    })?;

    // 3. 获取用户信息
    let user_info = modules::oauth::get_user_info(&token_res.access_token).await?;
    modules::logger::log_info(&format!("获取用户信息成功: {}", user_info.email));

    // 4. 尝试获取项目ID
    let project_id = crate::proxy::project_resolver::fetch_project_id(&token_res.access_token)
        .await
        .ok();

    if let Some(ref pid) = project_id {
        modules::logger::log_info(&format!("获取项目ID成功: {}", pid));
    } else {
        modules::logger::log_warn("未能获取项目ID,将在后续懒加载");
    }

    // 5. 构造 TokenData
    let token_data = TokenData::new(
        token_res.access_token,
        refresh_token,
        token_res.expires_in,
        Some(user_info.email.clone()),
        project_id,
        None,
    );

    // 6. 添加或更新到账号列表
    modules::logger::log_info("正在保存账号信息...");
    let mut account = modules::upsert_account(
        user_info.email.clone(),
        user_info.get_display_name(),
        token_data,
    )?;

    // 7. 自动触发刷新额度
    let _ = internal_refresh_account_quota(&app_handle, &mut account).await;

    // 8. If proxy is running, reload token pool so changes take effect immediately.
    let _ = crate::commands::proxy::reload_proxy_accounts(
        app_handle.state::<crate::commands::proxy::ProxyServiceState>(),
    )
    .await;

    Ok(account)
}

/// 预生成 OAuth 授权链接 (不打开浏览器)
#[tauri::command]
pub async fn prepare_oauth_url(app_handle: tauri::AppHandle) -> Result<String, String> {
    crate::modules::oauth_server::prepare_oauth_url(app_handle).await
}

#[tauri::command]
pub async fn cancel_oauth_login() -> Result<(), String> {
    modules::oauth_server::cancel_oauth_flow();
    Ok(())
}

// --- 导入命令 ---

#[tauri::command]
pub async fn import_v1_accounts(app: tauri::AppHandle) -> Result<Vec<Account>, String> {
    let accounts = modules::migration::import_from_v1().await?;

    // 对导入的账号尝试刷新一波
    for mut account in accounts.clone() {
        let _ = internal_refresh_account_quota(&app, &mut account).await;
    }

    Ok(accounts)
}

#[tauri::command]
pub async fn import_from_db(app: tauri::AppHandle) -> Result<Account, String> {
    // 同步函数包装为 async
    let mut account = modules::migration::import_from_db().await?;

    // 既然是从数据库导入（即 IDE 当前账号），自动将其设为 Manager 的当前账号
    let account_id = account.id.clone();
    modules::account::set_current_account_id(&account_id)?;

    // 自动触发刷新额度
    let _ = internal_refresh_account_quota(&app, &mut account).await;

    // 刷新托盘图标展示
    crate::modules::tray::update_tray_menus(&app);

    Ok(account)
}

#[tauri::command]
#[allow(dead_code)]
pub async fn import_custom_db(app: tauri::AppHandle, path: String) -> Result<Account, String> {
    // 调用重构后的自定义导入函数
    let mut account = modules::migration::import_from_custom_db_path(path).await?;

    // 自动设为当前账号
    let account_id = account.id.clone();
    modules::account::set_current_account_id(&account_id)?;

    // 自动触发刷新额度
    let _ = internal_refresh_account_quota(&app, &mut account).await;

    // 刷新托盘图标展示
    crate::modules::tray::update_tray_menus(&app);

    Ok(account)
}

#[tauri::command]
pub async fn sync_account_from_db(app: tauri::AppHandle) -> Result<Option<Account>, String> {
    // 1. 获取 DB 中的 Refresh Token
    let db_refresh_token = match modules::migration::get_refresh_token_from_db() {
        Ok(token) => token,
        Err(e) => {
            modules::logger::log_info(&format!("自动同步跳过: {}", e));
            return Ok(None);
        }
    };

    // 2. 获取 Manager 当前账号
    let curr_account = modules::account::get_current_account()?;

    // 3. 对比：如果 Refresh Token 相同，说明账号没变，无需导入
    if let Some(acc) = curr_account {
        if acc.token.refresh_token == db_refresh_token {
            // 账号未变，由于已经是周期性任务，我们可以选择性刷新一下配额，或者直接返回
            // 这里为了节省 API 流量，直接返回
            return Ok(None);
        }
        modules::logger::log_info(&format!(
            "检测到账号切换 ({} -> DB新账号)，正在同步...",
            acc.email
        ));
    } else {
        modules::logger::log_info("检测到新登录账号，正在自动同步...");
    }

    // 4. 执行完整导入
    let account = import_from_db(app).await?;
    Ok(Some(account))
}

/// 保存文本文件 (绕过前端 Scope 限制)
#[tauri::command]
pub async fn save_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| format!("写入文件失败: {}", e))
}

/// 清理日志缓存
#[tauri::command]
pub async fn clear_log_cache() -> Result<(), String> {
    modules::logger::clear_logs()
}

/// 打开数据目录
#[tauri::command]
pub async fn open_data_folder() -> Result<(), String> {
    let path = modules::account::get_data_dir()?;

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("打开文件夹失败: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| format!("打开文件夹失败: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("打开文件夹失败: {}", e))?;
    }

    Ok(())
}

/// 获取数据目录绝对路径
#[tauri::command]
pub async fn get_data_dir_path() -> Result<String, String> {
    let path = modules::account::get_data_dir()?;
    Ok(path.to_string_lossy().to_string())
}

/// 显示主窗口
#[tauri::command]
pub async fn show_main_window(window: tauri::Window) -> Result<(), String> {
    window.show().map_err(|e| e.to_string())
}

/// 获取 Antigravity 可执行文件路径
#[tauri::command]
pub async fn get_antigravity_path(bypass_config: Option<bool>) -> Result<String, String> {
    // 1. 优先从配置查询 (除非明确要求绕过)
    if bypass_config != Some(true) {
        if let Ok(config) = crate::modules::config::load_app_config() {
            if let Some(path) = config.antigravity_executable {
                if std::path::Path::new(&path).exists() {
                    return Ok(path);
                }
            }
        }
    }

    // 2. 执行实时探测
    match crate::modules::process::get_antigravity_executable_path() {
        Some(path) => Ok(path.to_string_lossy().to_string()),
        None => Err("未找到 Antigravity 安装路径".to_string()),
    }
}

/// 获取 Antigravity 启动参数
#[tauri::command]
pub async fn get_antigravity_args() -> Result<Vec<String>, String> {
    match crate::modules::process::get_args_from_running_process() {
        Some(args) => Ok(args),
        None => Err("未找到正在运行的 Antigravity 进程".to_string()),
    }
}

/// 检测更新响应结构
#[derive(serde::Serialize)]
pub struct UpdateInfo {
    has_update: bool,
    latest_version: String,
    current_version: String,
    download_url: String,
}

/// 检测 GitHub releases 更新
#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateInfo, String> {
    const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
    const GITHUB_API_URL: &str =
        "https://api.github.com/repos/lbjlaq/Antigravity-Manager/releases/latest";

    modules::logger::log_info("开始检测更新...");

    // 发起 HTTP 请求
    let client = crate::utils::http::create_client(15);
    let response = client
        .get(GITHUB_API_URL)
        .header("User-Agent", "Antigravity-Tools")
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("GitHub API 返回错误: {}", response.status()));
    }

    // 解析 JSON 响应
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("解析响应失败: {}", e))?;

    let latest_version = json["tag_name"]
        .as_str()
        .ok_or("无法获取版本号")?
        .trim_start_matches('v');

    let download_url = json["html_url"]
        .as_str()
        .unwrap_or("https://github.com/lbjlaq/Antigravity-Manager/releases")
        .to_string();

    // 比较版本号
    let has_update = compare_versions(latest_version, CURRENT_VERSION);

    modules::logger::log_info(&format!(
        "版本检测完成: 当前 v{}, 最新 v{}, 有更新: {}",
        CURRENT_VERSION, latest_version, has_update
    ));

    Ok(UpdateInfo {
        has_update,
        latest_version: format!("v{}", latest_version),
        current_version: format!("v{}", CURRENT_VERSION),
        download_url,
    })
}

/// 简单的版本号比较 (假设格式为 x.y.z)
fn compare_versions(latest: &str, current: &str) -> bool {
    let parse_version =
        |v: &str| -> Vec<u32> { v.split('.').filter_map(|s| s.parse::<u32>().ok()).collect() };

    let latest_parts = parse_version(latest);
    let current_parts = parse_version(current);

    for i in 0..3 {
        let l = latest_parts.get(i).unwrap_or(&0);
        let c = current_parts.get(i).unwrap_or(&0);
        if l > c {
            return true;
        } else if l < c {
            return false;
        }
    }

    false
}

/// 切换账号的反代禁用状态
#[tauri::command]
pub async fn toggle_proxy_status(
    app: tauri::AppHandle,
    proxy_state: tauri::State<'_, crate::commands::proxy::ProxyServiceState>,
    account_id: String,
    enable: bool,
    reason: Option<String>,
) -> Result<(), String> {
    modules::logger::log_info(&format!(
        "切换账号反代状态: {} -> {}",
        account_id,
        if enable { "启用" } else { "禁用" }
    ));

    // 1. 读取账号文件
    let data_dir = modules::account::get_data_dir()?;
    let account_path = data_dir.join("accounts").join(format!("{}.json", account_id));

    if !account_path.exists() {
        return Err(format!("账号文件不存在: {}", account_id));
    }

    let content = std::fs::read_to_string(&account_path)
        .map_err(|e| format!("读取账号文件失败: {}", e))?;

    let mut account_json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("解析账号文件失败: {}", e))?;

    // 2. 更新 proxy_disabled 字段
    if enable {
        // 启用反代
        account_json["proxy_disabled"] = serde_json::Value::Bool(false);
        account_json["proxy_disabled_reason"] = serde_json::Value::Null;
        account_json["proxy_disabled_at"] = serde_json::Value::Null;
    } else {
        // 禁用反代
        let now = chrono::Utc::now().timestamp();
        account_json["proxy_disabled"] = serde_json::Value::Bool(true);
        account_json["proxy_disabled_at"] = serde_json::Value::Number(now.into());
        account_json["proxy_disabled_reason"] = serde_json::Value::String(
            reason.unwrap_or_else(|| "用户手动禁用".to_string())
        );
    }

    // 3. 保存到磁盘
    std::fs::write(&account_path, serde_json::to_string_pretty(&account_json).unwrap())
        .map_err(|e| format!("写入账号文件失败: {}", e))?;

    modules::logger::log_info(&format!(
        "账号反代状态已更新: {} ({})",
        account_id,
        if enable { "已启用" } else { "已禁用" }
    ));

    // 4. 如果反代服务正在运行,重新加载账号池
    let _ = crate::commands::proxy::reload_proxy_accounts(proxy_state).await;

    // 5. 更新托盘菜单
    crate::modules::tray::update_tray_menus(&app);

    Ok(())
}
