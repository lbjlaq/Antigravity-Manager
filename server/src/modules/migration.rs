use std::fs;
use std::path::PathBuf;
use serde_json::Value;
use base64::{Engine as _, engine::general_purpose};
use crate::models::{TokenData, Account};
use crate::modules::{account, db};
use crate::utils::protobuf;

/// 扫描并导入 V1 数据
pub async fn import_from_v1() -> Result<Vec<Account>, String> {
    use crate::modules::oauth;

    let home = dirs::home_dir().ok_or("无法获取主目录")?;
    
    // V1 数据目录 (根据 utils.py 确认全平台统一)
    let v1_dir = home.join(".antigravity-agent");
    
    let mut imported_accounts = Vec::new();
    
    // 尝试多个可能的文件名
    let index_files = vec![
        "antigravity_accounts.json", // Directly use string literal
        "accounts.json"
    ];
    
    let mut found_index = false;

    for index_filename in index_files {
        let v1_accounts_path = v1_dir.join(index_filename);
        
        if !v1_accounts_path.exists() {
            continue;
        }
        
        found_index = true;
        crate::modules::logger::log_info(&format!("发现 V1 数据: {:?}", v1_accounts_path));
        
        let content = match fs::read_to_string(&v1_accounts_path) {
            Ok(c) => c,
            Err(e) => {
                crate::modules::logger::log_warn(&format!("读取索引失败: {}", e));
                continue;
            }
        };
        
        let v1_index: Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                crate::modules::logger::log_warn(&format!("解析索引 JSON 失败: {}", e));
                continue;
            }
        };
        
        // 兼容两种格式：直接是 map，或者包含 "accounts" 字段
        let accounts_map = if let Some(map) = v1_index.as_object() {
            if let Some(accounts) = map.get("accounts").and_then(|v| v.as_object()) {
                accounts 
            } else {
                map
            }
        } else {
            continue;
        };
        
        for (id, acc_info) in accounts_map {
            let email_placeholder = acc_info.get("email").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
            
            // 跳过非账号的 key (如 "current_account_id")
            if !acc_info.is_object() {
                continue;
            }
            
            let backup_file_str = acc_info.get("backup_file").and_then(|v| v.as_str());
            let data_file_str = acc_info.get("data_file").and_then(|v| v.as_str());
            
            // 优先使用 backup_file, 其次 data_file
            let target_file = backup_file_str.or(data_file_str);
            
            if target_file.is_none() {
                crate::modules::logger::log_warn(&format!("账号 {} ({}) 缺少数据文件路径", id, email_placeholder));
                continue;
            }
            
            let mut backup_path = PathBuf::from(target_file.unwrap());
            
            // 如果是相对路径，尝试拼接
            if !backup_path.exists() {
                 backup_path = v1_dir.join(backup_path.file_name().unwrap_or_default());
            }
            
            // 再次尝试拼接 data/ 或 backups/ 子目录
            if !backup_path.exists() {
                 let file_name = backup_path.file_name().unwrap_or_default();
                 let try_backups = v1_dir.join("backups").join(file_name);
                 if try_backups.exists() {
                     backup_path = try_backups;
                 } else {
                     let try_accounts = v1_dir.join("accounts").join(file_name);
                     if try_accounts.exists() {
                         backup_path = try_accounts;
                     }
                 }
            }
            
            if !backup_path.exists() {
                crate::modules::logger::log_warn(&format!("账号 {} ({}) 备份文件不存在: {:?}", id, email_placeholder, backup_path));
                continue;
            }
            
            // 读取备份文件
            if let Ok(backup_content) = fs::read_to_string(&backup_path) {
                if let Ok(backup_json) = serde_json::from_str::<Value>(&backup_content) {
                    
                    // 兼容两种格式：
                    // 1. V1 备份: jetskiStateSync.agentManagerInitState -> Protobuf
                    // 2. V2/Script 数据: 包含 "token" 字段的 JSON
                    
                    let mut refresh_token_opt = None;
                    
                    // 尝试格式 2
                    if let Some(token_data) = backup_json.get("token") {
                        if let Some(rt) = token_data.get("refresh_token").and_then(|v| v.as_str()) {
                            refresh_token_opt = Some(rt.to_string());
                        }
                    }
                    
                    // 尝试格式 1
                    if refresh_token_opt.is_none() {
                         if let Some(state_b64) = backup_json.get("jetskiStateSync.agentManagerInitState").and_then(|v| v.as_str()) {
                            // 解析 Protobuf
                            if let Ok(blob) = general_purpose::STANDARD.decode(state_b64) {
                                if let Ok(Some(oauth_data)) = protobuf::find_field(&blob, 6) {
                                    if let Ok(Some(refresh_bytes)) = protobuf::find_field(&oauth_data, 3) {
                                        if let Ok(rt) = String::from_utf8(refresh_bytes) {
                                            refresh_token_opt = Some(rt);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    
                    if let Some(refresh_token) = refresh_token_opt {
                         crate::modules::logger::log_info(&format!("正在导入账号: {}", email_placeholder));
                         
                         let (email, access_token, expires_in) = match oauth::refresh_access_token(&refresh_token).await {
                            Ok(token_resp) => {
                                match oauth::get_user_info(&token_resp.access_token).await {
                                    Ok(user_info) => (user_info.email, token_resp.access_token, token_resp.expires_in),
                                    Err(_) => (email_placeholder.clone(), token_resp.access_token, token_resp.expires_in), 
                                }
                            },
                            Err(e) => {
                                crate::modules::logger::log_warn(&format!("Token 刷新失败 (可能过期): {}", e));
                                (email_placeholder.clone(), "imported_access_token".to_string(), 0)
                            }, 
                        };

                        let token_data = TokenData::new(
                            access_token, 
                            refresh_token,
                            expires_in,
                            Some(email.clone()),
                            None, // project_id 将在需要时获取
                            None, // session_id
                    );
                        
                        // 在第153行的get_user_info中已经获取name，但这里是在match语句外，我们巴安全起见使用None
                        match account::upsert_account(email.clone(), None, token_data) {
                            Ok(acc) => {
                                crate::modules::logger::log_info(&format!("导入成功: {}", email));
                                imported_accounts.push(acc);
                            },
                            Err(e) => crate::modules::logger::log_error(&format!("导入保存失败 {}: {}", email, e)),
                        }

                    } else {
                        crate::modules::logger::log_warn(&format!("账号 {} 数据文件中未找到 Refresh Token", email_placeholder));
                    }
                }
            }
        }
    }
    
    if !found_index {
        return Err("未找到 V1 版本账号数据文件".to_string());
    }
    
    Ok(imported_accounts)
}

/// 从数据库导入当前登录账号
pub async fn import_from_db() -> Result<Account, String> {
    use crate::modules::oauth;

    let db_path = db::get_db_path()?;
    
    if !db_path.exists() {
        return Err(format!("找不到数据库文件: {:?}", db_path));
    }
    
    // 连接数据库
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("打开数据库失败: {}", e))?;
        
    // 从 ItemTable 读取
    let current_data: String = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?",
            ["jetskiStateSync.agentManagerInitState"],
            |row| row.get(0),
        )
        .map_err(|_| "未找到登录状态数据，请确认已在 IDE 中登录".to_string())?;
        
    // Base64 解码
    let blob = general_purpose::STANDARD
        .decode(&current_data)
        .map_err(|e| format!("Base64 解码失败: {}", e))?;
        
    // 1. 查找 oauthTokenInfo (Field 6)
    let oauth_data = protobuf::find_field(&blob, 6)
        .map_err(|e| format!("解析 Protobuf 失败: {}", e))?
        .ok_or("未找到 OAuth 数据，可能未登录")?;
        
    // 2. 提取 refresh_token (Field 3)
    let refresh_bytes = protobuf::find_field(&oauth_data, 3)
        .map_err(|e| format!("解析 OAuth 数据失败: {}", e))?
        .ok_or("数据中未包含 Refresh Token")?;
        
    let refresh_token = String::from_utf8(refresh_bytes)
        .map_err(|_| "Refresh Token 非 UTF-8 编码".to_string())?;
        
    // 3. 使用 Refresh Token 获取最新的 Access Token 和用户信息
    // 这一步至关重要，因为 DB 中的 Access Token 可能已过期，且我们需要 Email
    crate::modules::logger::log_info("正在使用 Refresh Token 获取用户信息...");
    let token_resp = oauth::refresh_access_token(&refresh_token).await?;
    let user_info = oauth::get_user_info(&token_resp.access_token).await?;
    
    let email = user_info.email;
    
    crate::modules::logger::log_info(&format!("成功获取账号信息: {}", email));
    
    let token_data = TokenData::new(
        token_resp.access_token,
        refresh_token,
        token_resp.expires_in,
        Some(email.clone()),
        None, // project_id 将在需要时获取
        None, // session_id 将在 token_manager 中生成
    );
    
    // 4. 添加或更新账号
    account::upsert_account(email.clone(), user_info.name, token_data)
}
